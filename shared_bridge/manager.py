"""Reservation manager: demand-driven acquisition, status and service selection.

All reservation decisions are RackAI's. The bridge only chooses *when* to ask
and remembers *what* was asked for.
"""
from dataclasses import replace
import threading
import time

from . import projection
from .rack import RackAI, RackAIError
from .state import StateStore, Switch
from .acquisition import Acquisition
from .status import operator_status

class UnknownService(ValueError):
    """The requested service is not a RackAI-published inference service."""


class SwitchInProgress(RuntimeError):
    """Another service switch is already running."""


class Core:
    """Collaborators and durable state shared by the bridge's two actors."""

    SERVICES_TTL = 60

    def __init__(self, config, rack=None, store=None, clock=None):
        self.config = config
        self.rack = rack or RackAI(config)
        self.store = store or StateStore(config.state_path)
        self.clock = clock or (lambda: int(time.time()))
        self.lock = threading.RLock()
        self.stopped = threading.Event()
        self.state = self.store.load(config.default_service)
        self.services = []
        self.services_at = 0

    def persist(self, state):
        self.store.save(state)
        self.state = state

    def refresh_services(self):
        now = self.clock()
        if self.services and now - self.services_at < self.SERVICES_TTL:
            return self.services
        try:
            discovered = projection.compatible_services(self.rack.discover())
        except RackAIError:
            return self.services
        self.services = discovered
        self.services_at = now
        return self.services


class ReservationManager:
    """Consumer lookups, operator selection and restart reconciliation."""

    def __init__(self, config, core=None, switcher_factory=None):
        self.core = core or Core(config)
        self._acquisition = Acquisition(self.core)
        factory = switcher_factory or _default_switcher
        self._switcher = factory(self.core, self.acquire)

    def services(self):
        with self.core.lock:
            return list(self.core.refresh_services())

    def close(self):
        """Stop any in-flight switch worker (used on shutdown and in tests)."""
        self.core.stopped.set()

    def operator_status(self):
        with self.core.lock:
            self.core.refresh_services()
            return self._status()

    def consumer_outcome(self):
        with self.core.lock:
            if not self.core.state.enabled:
                return projection.unavailable(self.core.state.selected_service, "bridge_disabled")
            switch = self.core.state.switch
            if switch is not None:
                return self._switch_outcome(switch)
            reservation = self.core.state.reservation
            if reservation is not None:
                try:
                    view = self.core.rack.inspect(reservation.id)
                except RackAIError:
                    return projection.pending(reservation.service, "rackai_temporarily_unavailable")
                if view is not None:
                    kind, payload = projection.classify(
                        view, reservation.service, self.core.config.public_base_url
                    )
                    if kind in (projection.READY, projection.PENDING):
                        return payload
                    if kind == projection.UNAVAILABLE:
                        self._acquisition.remember(payload)
                        return payload
                self.core.persist(replace(self.core.state, reservation=None))
            return self.acquire()

    def select(self, service):
        with self.core.lock:
            if not isinstance(service, str) or not service:
                raise UnknownService("service is required")
            known = {entry["service"] for entry in self.core.refresh_services()}
            if known and service not in known:
                raise UnknownService(service)
            current = self.core.state.switch
            if current is not None:
                if current.to_service == service and self.core.state.enabled:
                    return self._status()
                raise SwitchInProgress("a service switch is already in progress")
            self.core.persist(replace(self.core.state, selected_service=service, outcome=None, enabled=True))
            reservation = self.core.state.reservation
            if reservation is None:
                self.acquire()
                return self._status()
            if reservation.service == service and self._usable(reservation):
                return self._status()
            self.core.persist(
                replace(
                    self.core.state,
                    switch=Switch(service, phase="releasing", started_at=self.core.clock()),
                )
            )
            self._switcher.start()
            return self._status()

    def release(self):
        """Persist operator shutdown before retiring the bridge-owned backend."""
        with self.core.lock:
            state = self.core.state
            switch = None if state.reservation is None else Switch(
                state.selected_service, phase="releasing", started_at=self.core.clock()
            )
            self.core.persist(replace(state, enabled=False, outcome=None, switch=switch))
            self._switcher.start()
            return self._status()

    def reconcile(self):
        with self.core.lock:
            self.core.refresh_services()
            state = self.core.state
            if state.reservation is None:
                if state.switch is not None:
                    self.core.persist(replace(state, switch=None))
                return
            if not state.enabled and state.switch is not None:
                self._switcher.start()
                return
            try:
                view = self.core.rack.inspect(state.reservation.id)
            except RackAIError:
                if state.switch is not None:
                    self._switcher.start()
                return
            if view is None or projection.is_terminal(view):
                self.core.persist(replace(state, reservation=None))
                if self.core.state.switch is not None:
                    self.core.persist(replace(self.core.state, switch=None))
                return
            if state.switch is not None:
                self._switcher.start()

    def acquire(self):
        return self._acquisition.acquire()

    def _status(self):
        return operator_status(self.core)

    def _usable(self, reservation):
        try:
            view = self.core.rack.inspect(reservation.id)
        except RackAIError:
            return False
        if view is None or projection.is_terminal(view):
            return False
        kind, _ = projection.classify(view, reservation.service, self.core.config.public_base_url)
        return kind in (projection.READY, projection.PENDING)

    @staticmethod
    def _switch_outcome(switch):
        if switch.phase in ("release_timeout", "release_error"):
            return projection.unavailable(
                switch.to_service, "previous_reservation_" + switch.phase, 10
            )
        if switch.phase == "recovery_required":
            return projection.pending(
                switch.to_service, "previous_reservation_recovery_required", 5
            )
        return projection.pending(switch.to_service, "service_switch_in_progress", 3)


def _default_switcher(core, acquire):
    from .switching import SwitchRunner

    return SwitchRunner(core, acquire)
