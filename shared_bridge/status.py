"""Read-only operator projection of the bridge's current state."""
from . import projection
from .rack import RackAIError


def operator_status(core):
    state = core.state
    reservation = state.reservation
    view = None
    if reservation is not None:
        try:
            view = core.rack.inspect(reservation.id)
        except RackAIError:
            view = None
    return {
        "schema": projection.SCHEMA,
        "enabled": state.enabled,
        "selected_service": state.selected_service,
        "services": core.services,
        "reservation": None if reservation is None else {
            "id": reservation.id,
            "service": reservation.service,
            "state": None if view is None else view.get("state"),
            "generation": None if view is None else projection.member(view, reservation.service).get("generation"),
        },
        "outcome": None if state.outcome is None else {
            "state": state.outcome.state,
            "reason": state.outcome.reason,
            "retry_after": state.outcome.retry_after,
            "recorded_at": state.outcome.recorded_at,
        },
        "switch": None if state.switch is None else {
            "to_service": state.switch.to_service,
            "phase": state.switch.phase,
            "started_at": state.switch.started_at,
        },
    }
