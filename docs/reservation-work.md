# Reservations and work

RackAI does not assign or cap application priority. Requested priority is caller-owned input on a reservation. Authentication establishes ownership/security, not workload importance or service entitlement. Published qualified services are available uniformly to authenticated ordinary principals.

All operations below use the existing authenticated `POST /runtime/v1` endpoint and its existing `{schema, result}` response envelope. No new work-unit version, workspace scheduler, ledger, gateway or native proxy is introduced.

## Reserve services

```json
{
  "operation": "reserve",
  "request": {
    "acquisition_id": "acquisition-123",
    "work_id": "project-456",
    "services": ["local-primary", "local-coder"],
    "priority": "paramount",
    "ttl_seconds": 1800
  }
}
```

The response contains `id`, `state`, `priority` and a `services` object keyed by service name. Each entry describes its existing managed activation/access. The public reservation associates these activations in the same canonical authority document; it does not introduce a second owner of resources.

Admission attempts requested services independently, in request order, at the reservation's single priority. The response always identifies one reservation and lists `requested_services` plus the individual service states. When states differ, the aggregate is `partial`. A failed acquisition is `unavailable`; it does not deny or undo other acquired services. Overlapping exclusive services may be requested, but cannot all be held simultaneously. Higher priority may safely preempt lower priority; ties retain the incumbent. Independent resources coexist.

Use `inspect_reservation`, `refresh_reservation`, `release_reservation` or `cancel_reservation`, each with `reservation_id`. Release/cancel controls all service activations and closes the reservation to refresh. Work is eligible when its selected service is Ready, regardless of other members. Individual service expiry/release does not close unrelated Ready members. Existing generation rotation, qualification, ownership and verified cleanup remain authoritative.

Acquisition replay with the same identity and exact request returns the durable original response, even after subsequent state changes. Changed payload conflicts. Use inspection for current state. A receiver upgraded with an older record lacking an original receipt fails closed on replay instead of inventing one.

`refresh_reservation` attempts only currently unavailable services against current rack state. It retains the reservation ID and priority, does not restart Ready members, and leaves Held members to their existing automatic restoration lifecycle. Missing services are never queued, monitored or assigned a place in line. If nothing becomes available, refresh returns the current state. Refresh does not extend the original reservation TTL and cannot reopen an explicitly released/cancelled reservation.

```json
{"operation":"refresh_reservation","reservation_id":"returned-reservation-id"}
```

An unavailable service rejects new work without creating a pending invocation. A previously acquired Held service retains its identity and pending work; restoration may resume it automatically. The caller decides whether a partial reservation is useful.

## Submit work

```json
{
  "operation": "submit_work",
  "request": {
    "reservation_id": "returned-reservation-id",
    "service": "local-primary",
    "work_id": "work-789",
    "payload": {
      "kind": "inference",
      "prompt": "A bounded request",
      "max_tokens": 128,
      "timeout_seconds": 30
    }
  }
}
```

Work contains no priority and cannot select a service outside its reservation. `work_id` is unique within the authenticated principal. Repeating the exact request reconciles its original invocation; changing the reservation, service or payload conflicts. Neither a disconnected caller nor a retry creates another execution within the retained work record. `inspect_work` and `cancel_work` take `work_id` and enforce ownership.

The existing invocation record stores accepted/started/completed/cancelled/expired/uncertain state, result, cancellation intent and late evidence. Work inspection also reports waiting/Held from current reservation state. Cancellation before dispatch prevents execution. Cancellation after dispatch records intent and retains late evidence. Receiver restart marks unresolved started work uncertain, never automatically replaying it. Ordinary inference token/context/protocol/time limits remain unchanged.

## Bounded workspace payload

Use the same work envelope with this payload shape:

```json
{
  "kind": "workspace",
  "workspace": {
    "repository": {"id": "registered-repository", "base_ref": "main", "base_sha": "exact-base-sha"},
    "objective": "Perform this bounded change",
    "allowed_paths": ["src/"],
    "acceptance": {"commands": [["cargo", "test", "--offline"]], "required_artifacts": ["src/lib.rs"]},
    "requirements": {"complexity": "small", "requires_large_context": false},
    "limits": {"max_implementation_attempts": 1, "timeout_seconds": 900, "network": "disabled"}
  }
}
```

Administrator runtime configuration enables this existing execution machinery with `workspace: {registry_root, state_root}`. These are server-owned paths, not client inputs. At least two existing dispatch slots are required so a running workspace cannot occupy the only slot needed by its nested model calls.

RackAI resolves the qualified worker compatible with the reserved model/resources. It injects the reservation's managed endpoint into that execution's private JCode configuration. It does not rewrite the registry or accept caller-supplied model endpoints. `ExecuteWorkspace` receives a typed `WorkspaceRequest` containing the reusable bounded change input directly. It retains repository registration, pinned base, worktree isolation, allowed paths, network controls, acceptance commands, candidate revision and provenance/evidence packets. The old versioned work-unit CLI, request documents, parsers and legacy worker-selection branch have been removed. There is no translation into an older contract.

The trusted runner registers its existing workspace scope with its parent invocation. The original scoped authorization may continue that bounded, already registered execution across generation rotation. It cannot authorize a fresh scope after restoration. Every model dispatch still checks current Ready state and resource ownership. Parent cancellation, expiry or uncertainty closes child dispatch authority. The same registered scope and work identity survive Held/restoration; no replacement workspace is created.

Per-model execution timeout starts at actual model dispatch, so waiting/Held does not consume it. The existing overall workspace wall-clock limit remains a separate finite bound. Uncertain nested execution is retained as uncertainty on the parent work. Acceptance checks the persisted work cancellation fence before accepting a revision.

## Interactive ComfyUI

A Ready interactive `comfyui` service returns `access` with `kind: "native_comfyui"`, `url`, `session_id`, reservation activation identity and generation. Use `url` as the native origin for `/object_info`, prompt submission, history/status and existing WebSocket operations. An LLM `gateway_path` is not returned for ComfyUI.

This is the existing protected media-native HTTP/WebSocket interface. Existing media credentials must identify the same principal as the runtime reservation. Existing session ownership, managed lease checks, backend gate and lifecycle control remain in use. No alternate native transport or security system is added. Access is unavailable while the ComfyUI service is not Ready; a Held or unavailable peer does not block it. The native interface retains its existing current-session semantics; this change does not invent a separate per-request native capability scheme.

The managed `local-image` job API, its existing request format and reservation `{id, generation}` binding remain separate and supported.

## Configuration migration

Remove source `permitted`, `default`, `maximum`, `tags` and `tag_priorities`, workspace `source_admission_policies`, and media principal `ceiling` from ordinary configuration. These application-policy types and fields have been removed. Runtime/media configuration rejects their obsolete fields. Identity/credential fields and existing operational flags remain. No recorded historical request or priority is rewritten.

Legacy single-service `acquire` still works; omitted legacy priority now uses the uniform Low default. Discovery publishes the global `priorities` vocabulary and all service profiles, including their qualification status. Ordinary unqualified requests still fail qualification independently of application identity. New reserve requests supply priority explicitly.

No deployment or client migration is performed by this change.
