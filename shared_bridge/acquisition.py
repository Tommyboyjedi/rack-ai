"""Demand-driven acquisition and bounded RackAI refusal handling."""
from dataclasses import replace
import secrets

from . import projection
from .rack import RackAIError
from .state import Outcome, Reservation

WORK_ID = "shared-bridge"
RETRY_SECONDS = 2
CAPACITY_RETRY_SECONDS = 5
AUTHORIZATION_RETRY_SECONDS = 30


class Acquisition:
    def __init__(self, core):
        self.core = core

    def acquire(self):
        with self.core.lock:
            state = self.core.state
            if not state.enabled:
                return projection.unavailable(state.selected_service, "bridge_disabled")
            now = self.core.clock()
            outcome = state.outcome
            if outcome is not None and now < outcome.recorded_at + outcome.retry_after:
                return _from_outcome(outcome, state.selected_service)
            service = state.selected_service
            request = {
                "acquisition_id": "bridge-" + secrets.token_hex(16),
                "work_id": WORK_ID,
                "services": [service],
                "priority": self.core.config.priority,
                "ttl_seconds": self.core.config.ttl_seconds,
            }
            try:
                view = self.core.rack.reserve(request)
            except RackAIError as error:
                return self._error_outcome(error, service)
            kind, payload = projection.classify(view, service, self.core.config.public_base_url)
            if kind in (projection.READY, projection.PENDING):
                reservation = Reservation(
                    id=str(view["id"]),
                    acquisition_id=request["acquisition_id"],
                    service=service,
                    created_at=now,
                )
                self.core.persist(replace(state, reservation=reservation, outcome=None))
                return payload
            self.remember(payload)
            return payload

    def remember(self, payload):
        self.core.persist(replace(self.core.state, outcome=Outcome(
            state=str(payload.get("state", projection.UNAVAILABLE)),
            reason=payload.get("reason"),
            retry_after=int(payload.get("retry_after") or RETRY_SECONDS),
            recorded_at=self.core.clock(),
        )))

    def _error_outcome(self, error, service):
        if error.status == 429:
            payload = projection.pending(service, error.reason, CAPACITY_RETRY_SECONDS)
        elif error.status in (401, 403):
            payload = projection.unavailable(service, "bridge_not_authorized", AUTHORIZATION_RETRY_SECONDS)
        elif error.status == 409:
            payload = projection.unavailable(service, error.reason, RETRY_SECONDS)
        else:
            payload = projection.pending(service, "rackai_temporarily_unavailable", RETRY_SECONDS)
        self.remember(payload)
        return payload


def _from_outcome(outcome, service):
    reason = outcome.reason or "unavailable"
    if outcome.state == projection.PENDING:
        return projection.pending(service, reason, outcome.retry_after)
    return projection.unavailable(service, reason, outcome.retry_after)
