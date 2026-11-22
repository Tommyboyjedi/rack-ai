"""Pure mapping from RackAI public reservation state to the bridge contract.

No I/O and no policy: every field either comes from RackAI or is a documented
bridge constant. RackAI remains the only authority on reservation state.
"""
SCHEMA = "rack-ai/shared-bridge/v1"
READY = "ready"
PENDING = "pending"
UNAVAILABLE = "unavailable"
TERMINAL = "terminal"
TERMINAL_STATES = frozenset({"released", "cancelled", "expired", "preempted"})
PENDING_STATES = frozenset({"preparing", "releasing", "partial"})
INFERENCE_CAPABILITIES = frozenset({"reasoning", "coding"})
DEFAULT_RETRY_SECONDS = 2
HIGHER_PRIORITY_REASONS = frozenset({"preempted_by_higher_priority"})


def compatible_services(discovery):
    """Discoverable, qualified text-inference services only (never media/speech)."""
    services = []
    for profile in discovery.get("tags") or []:
        capabilities = set(profile.get("capabilities") or [])
        if not profile.get("qualified") or not capabilities & INFERENCE_CAPABILITIES:
            continue
        services.append(
            {
                "service": profile["tag"],
                "version": profile.get("version"),
                "context_tokens": profile.get("context_tokens"),
                "max_input_tokens": profile.get("max_input_tokens"),
                "max_output_tokens": profile.get("max_output_tokens"),
            }
        )
    services.sort(key=lambda entry: entry["service"])
    return services


def member(view, service):
    return (view.get("services") or {}).get(service) or {}


def is_terminal(view):
    return view.get("state") in TERMINAL_STATES


def higher_priority(reason):
    if not reason:
        return None
    return "higher_priority" if reason.startswith("incumbent_priority:") or reason in HIGHER_PRIORITY_REASONS else None


def pending(service, reason, retry_after=DEFAULT_RETRY_SECONDS):
    return {
        "schema": SCHEMA,
        "state": PENDING,
        "service": service,
        "reason": reason,
        "retry_after": retry_after,
    }


def unavailable(service, reason, retry_after=DEFAULT_RETRY_SECONDS):
    return {
        "schema": SCHEMA,
        "state": UNAVAILABLE,
        "service": service,
        "reason": reason,
        "conflict": higher_priority(reason),
        "retry_after": retry_after,
    }


def ready(service, view, base_url):
    entry = member(view, service)
    gateway_path = entry["gateway_path"].rstrip("/")
    return {
        "schema": SCHEMA,
        "state": READY,
        "service": service,
        "reservation_id": view["id"],
        "generation": entry.get("generation"),
        "model": entry.get("model"),
        "base_url": base_url.rstrip("/"),
        "gateway_path": gateway_path,
        "chat_completions_url": base_url.rstrip("/") + gateway_path + "/chat/completions",
        "responses_url": base_url.rstrip("/") + gateway_path + "/responses",
        "authentication": "reservation_capability_path",
        "idempotency_key_required": True,
        "expires_at": entry.get("deadline"),
        "limits": {
            "context_tokens": entry.get("context_tokens"),
            "max_input_tokens": entry.get("max_input_tokens"),
            "max_output_tokens": entry.get("max_output_tokens"),
            "context_reserve_tokens": entry.get("context_reserve_tokens"),
            "profile_version": entry.get("profile_version"),
        },
    }


def classify(view, service, base_url):
    """Return (kind, payload) for one authoritative reservation view."""
    state = view.get("state")
    entry = member(view, service)
    member_state = entry.get("state")
    if state == READY and member_state == READY and entry.get("gateway_path"):
        return READY, ready(service, view, base_url)
    if state in TERMINAL_STATES:
        return TERMINAL, unavailable(service, entry.get("reason") or state)
    if state in PENDING_STATES or member_state == "preparing":
        return PENDING, pending(service, entry.get("reason") or state)
    reason = entry.get("reason") or view.get("reason") or member_state or state or "unavailable"
    retry_after = entry.get("retry_after") or DEFAULT_RETRY_SECONDS
    return UNAVAILABLE, unavailable(service, reason, int(retry_after))
