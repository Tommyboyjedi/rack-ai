"""Release-before-acquire service switching.

Runs only for an operator-requested change. It is not a background
reacquisition loop: it ends as soon as the old reservation is terminal and the
replacement has been requested, or when the bounded switch deadline expires.
"""
from dataclasses import replace
import threading
import time

from . import projection

POLL_SECONDS = 1.0
ERROR_PAUSE_SECONDS = 2.0


class SwitchRunner:
    def __init__(self, core, acquire, sleep=None):
        self._core = core
        self._acquire = acquire
        self._sleep = sleep or time.sleep
        self._thread = None

    def start(self):
        with self._core.lock:
            if self._core.stopped.is_set() or self._core.state.switch is None:
                return
            if self._thread is not None and self._thread.is_alive():
                return
            self._thread = threading.Thread(target=self._loop, name="bridge-switch", daemon=True)
            self._thread.start()

    def _loop(self):
        cancelled = None
        while not self._core.stopped.is_set():
            with self._core.lock:
                switch = self._core.state.switch
                if switch is None:
                    self._thread = None
                    return
                expired = self._core.clock() - switch.started_at > self._core.config.switch_timeout_seconds
                if expired:
                    self._phase("release_timeout")
                    self._thread = None
                    return
                reservation = self._core.state.reservation
                stop_only = not self._core.state.enabled
            if reservation is None:
                with self._core.lock:
                    self._core.persist(replace(self._core.state, switch=None, outcome=None))
                    if self._core.state.enabled:
                        self._acquire()
                    self._thread = None
                return
            if not stop_only or cancelled != reservation.id:
                if not self._release(reservation):
                    self._sleep(ERROR_PAUSE_SECONDS)
                    continue
                if stop_only:
                    cancelled = reservation.id
            try:
                view = self._core.rack.inspect(reservation.id)
            except Exception:  # transport uncertainty keeps the switch waiting
                self._sleep(ERROR_PAUSE_SECONDS)
                continue
            if view is None or projection.is_terminal(view):
                self._persist_cleared()
                continue
            state = view.get("state")
            if stop_only and state not in ("releasing", "recovery_required"):
                cancelled = None
            self._phase("recovery_required" if state == "recovery_required" else "releasing")
            self._sleep(POLL_SECONDS)

    def _persist_cleared(self):
        try:
            with self._core.lock:
                self._core.persist(replace(self._core.state, reservation=None, outcome=None))
        except OSError:
            # A temporarily unwritable state directory must not kill the switch.
            self._sleep(ERROR_PAUSE_SECONDS)

    def _release(self, reservation):
        try:
            with self._core.lock:
                if self._core.state.enabled:
                    self._core.rack.release(reservation.id)
                else:
                    self._core.rack.cancel(reservation.id)
            return True
        except Exception as error:  # RackAIError or transport
            status = getattr(error, "status", 0)
            if not self._core.state.enabled:
                self._phase("release_error")
                return False
            if status in (0, 404, 409):
                return True
            self._phase("release_error")
            return False

    def _phase(self, phase):
        with self._core.lock:
            switch = self._core.state.switch
            if switch is None or switch.phase == phase:
                return
            self._core.persist(replace(self._core.state, switch=replace(switch, phase=phase)))
