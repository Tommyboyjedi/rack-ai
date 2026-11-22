# Shared-Bridge V2

Shared-Bridge V2 is a small reservation manager for RackAI. It remembers one
operator-selected logical service, asks RackAI for that service, and publishes
the resulting reservation-scoped access material to consumer applications.

It is **not** an inference proxy, GPU scheduler, backend manager or second
reservation authority. RackAI remains the only authority for resource
allocation, admission, priority, preemption, lifecycle, queuing, idle expiry,
release and recovery.

## 1. Legacy bridge removal record

The legacy bridge (a Python inference proxy in a Hermes worktree with its own
job queue, reconnect loop and privileged control proxy) was removed before V2
was installed.

| Component | Location | Disposition |
| --- | --- | --- |
| Bridge application | `/home/tomp/.hermes/worktrees/shared-rackai-bridge/` | Git worktree removed from `hermes-agent` |
| User unit | `~/.config/systemd/user/shared-rackai-bridge.service` | Stopped, disabled, file removed |
| Durable state | `/home/tomp/.local/share/rackai-shared-bridge/` | Removed (snapshot retained) |
| Control proxy unit | `/etc/systemd/system/rack-ai-bridge-control-proxy.service` | Process stopped (clean exit); unit file needs root to disable/remove |
| Control proxy script | `/srv/rack-ai-media/https/ops/bridge_proxy.py` | Removed |
| Ingress `/bridge/control/` | `/srv/rack-ai-media/https/config/nginx.conf` | Removed and nginx reloaded |
| Bridge-owned RackAI reservations | `managed.json` (owner `hermes`, state `preempted`) | Released through `release_reservation`; all terminal |

**Recoverable snapshot** (mode `0700`, contains no credential contents):

```
/srv/rack-ai/deployments/bridge-legacy-snapshot-20261103T125650Z/
  MANIFEST.sha256
  shared_bridge.py, shared_bridge_runtime.py, *.before-*
  shared-rackai-bridge.service, rack-ai-bridge-control-proxy.service
  bridge_proxy.py, state.json, nginx.conf.before
```

The RackAI `hermes` principal and its credential file were **not** removed; the
bridge continues to use that principal (see limitations).

**Root-required remainder.** The control-proxy unit is a root-owned system unit
that cannot be disabled without interactive authentication. It is inert: the
process is stopped, its script is deleted and its only route is gone. Remove it
with:

```bash
sudo systemctl disable --now rack-ai-bridge-control-proxy.service
sudo rm /etc/systemd/system/rack-ai-bridge-control-proxy.service
sudo systemctl daemon-reload
```

## 2. Implementation

| Module | Responsibility |
| --- | --- |
| `shared_bridge/config.py` | Typed, validated operator configuration |
| `shared_bridge/rack.py` | The five allowed RackAI calls: `discover`, `reserve`, `inspect_reservation`, `release_reservation` |
| `shared_bridge/state.py` | Atomic, mode-0600 state (`selected_service`, reservation identity, bounded retry outcome, switch phase) |
| `shared_bridge/projection.py` | Pure mapping of RackAI state to the `ready` / `pending` / `unavailable` consumer contract |
| `shared_bridge/manager.py` | Demand-driven acquisition, invalidation handling, operator selection, restart reconciliation |
| `shared_bridge/switching.py` | Release-before-acquire switch worker (operator-initiated, bounded) |
| `shared_bridge/api.py` | Loopback HTTP surface |
| `shared_bridge/ui.py` | Single operator page (service selector, activate/change, current reservation) |
| `shared_bridge/app.py` | Entry point (`python3 -m shared_bridge.app --config ...`) |

There is no inference path, no job queue, no consumer session tracking and no
second claim mechanism.

## 3. Consumer contract

```
GET https://gpurack.duckdns.org/bridge/reservation
```

No consumer identity, header or credential is required or accepted. The
response is always derived from a live RackAI `inspect_reservation` (or a fresh
`reserve`), never from a cached reservation id. Responses are `no-store`.

**READY**

```json
{
  "schema": "rack-ai/shared-bridge/v1",
  "state": "ready",
  "service": "mini-brain",
  "reservation_id": "…",
  "generation": "…",
  "model": "…",
  "base_url": "https://gpurack.duckdns.org",
  "gateway_path": "/scoped/<id>/<capability>/v1",
  "chat_completions_url": "https://gpurack.duckdns.org/scoped/<id>/<capability>/v1/chat/completions",
  "responses_url": "…/v1/responses",
  "authentication": "reservation_capability_path",
  "idempotency_key_required": true,
  "expires_at": 1760000000,
  "limits": {
    "context_tokens": 131072,
    "max_input_tokens": 65536,
    "max_output_tokens": 65536,
    "context_reserve_tokens": 0,
    "profile_version": "…"
  }
}
```

Consumers POST directly to RackAI at the supplied URL. RackAI's scoped gateway
authorises those calls by the capability path alone; the bridge's RackAI bearer
token is never returned.

**PENDING** — `{"state":"pending","reason":…,"retry_after":N}`. Retry after
`retry_after` seconds. Reasons include `preparing`, `service_switch_in_progress`,
`previous_reservation_recovery_required` and `rackai_temporarily_unavailable`.

**UNAVAILABLE** — `{"state":"unavailable","reason":…,"conflict":…,"retry_after":N}`.
`reason` is RackAI's own refusal string (`incumbent_priority:<id>`,
`transition_or_recovery_blocked:<id>`, `insufficient_host_memory`,
`unqualified_profile`, …). `conflict` is `higher_priority` **only** when RackAI
supplied `incumbent_priority:*` or `preempted_by_higher_priority`; the bridge
never invents priority information.

**Invalidation.** After expiry, release or preemption, the scoped path returns
HTTP 409 with a typed error (`reservation_not_dispatchable`,
`reservation_preempted`, `invalid_scoped_capability`, …). Consumers must then
re-query `/bridge/reservation` and use the new details.

**Concurrency.** One RackAI reservation is shared by all consumers. RackAI
serialises inference within a reservation and queues it (bounded by
`max_pending`, `max_pending_per_reservation`, `max_wait_seconds` and
`max_gateway_waiters`); consumers must tolerate queue latency and HTTP 429
capacity responses.

**Idempotency.** Every inference request must carry a globally unique
`Idempotency-Key`. RackAI scopes submission identity by owner + reservation, so
two consumers using the same key and body would reconcile to one invocation and
receive the same result.

## 4. Operator UI

`https://gpurack.duckdns.org/bridge/` — no HTTP authentication. Access is
restricted by the Tailscale-only listener itself, so the page is reachable only
from authorised tailnet clients. The page lists the qualified inference services
discovered from RackAI, activates or changes the selected service, and reports
the current reservation.

Selection persists across restarts in
`/srv/rack-ai/deployments/shared-bridge/state/state.json`.

## 5. Lifecycle

- **Activation / reacquisition** is demand-driven. A consumer lookup with no
  usable reservation starts exactly one `reserve` (a process-wide lock prevents
  duplicate acquisitions). A blocked acquisition returns RackAI's reason and
  `retry_after`; the bridge suppresses re-tries until that guidance expires.
- **Expiry** (RackAI's existing 30-minute inactivity policy) makes the next
  consumer lookup reacquire with a **new** `acquisition_id`.
- **Preemption** returns `unavailable` with RackAI's reason; once the old
  reservation is terminal the next lookup reacquires.
- **Switching** (operator only): persist the new selection, stop publishing the
  old reservation, `release_reservation`, poll `inspect_reservation` until a
  terminal state (which RackAI publishes only after verified backend stop and
  GPU cleanup), then `reserve` with a new acquisition id. A `recovery_required`
  state blocks replacement until RackAI proves the effect absent. Only one
  switch runs at a time and only one bridge reservation ever exists.
- **Restart** reconciles the persisted reservation: terminal/missing records are
  dropped, live ones are reused, and an interrupted switch resumes.
- Polling the bridge only performs read-only `inspect_reservation` calls, so it
  never keeps a GPU reservation alive.

## 6. Ingress

`config/media/https/nginx.conf` (installed at
`/srv/rack-ai-media/https/config/nginx.conf`):

- `location ~ ^/scoped/[^/]+/[^/]+/v1/(chat/completions|responses)$` → RackAI
  `127.0.0.1:8095`, POST only, 1 MiB body, 3700 s read/send timeout,
  `access_log off` because the capability is part of the path.
- `location = /bridge/reservation` → bridge, GET only, no authentication
  (consumers do not identify themselves), `access_log off`.
- `location /bridge/` → bridge UI/control, no HTTP authentication (the ingress
  is reachable only from the tailnet), 16 KiB body limit.
- RackAI's `/runtime/v1`, voice registration, preflight and scope-control
  routes are **not** proxied.

Large request bodies are buffered to
`/srv/rack-ai-media/https/run/client-body` (proxy temp data uses `run/proxy`).
Those directories must be owned by the nginx worker user with mode `0700`. If
they are owned by another account, nginx answers HTTP 500 for any body larger
than its in-memory buffer while small requests still succeed — which is exactly
how an OpenCode request with a large system prompt and tool definitions exposed
a pre-existing ingress defect on 2026-11-03. The directories were recreated with
the correct ownership; verify them after any ingress redeployment.

The ingress is reachable only over the Tailscale-only HTTPS listener
(`IPAddressAllow` limited to loopback and `100.64.0.0/10`).

## 7. Deployment

| Artifact | Path |
| --- | --- |
| Release tree | `/srv/rack-ai/deployments/shared-bridge/releases/<sha>/` |
| Active release | `/srv/rack-ai/deployments/shared-bridge/current` (symlink) |
| Configuration | `/srv/rack-ai/deployments/shared-bridge/config.json` (mode 0600) |
| State | `/srv/rack-ai/deployments/shared-bridge/state/state.json` (mode 0600) |
| User unit | `~/.config/systemd/user/rack-ai-shared-bridge.service` |

Deploy:

```bash
bash config/bridge/deploy.sh /srv/rack-ai/.worktrees/shared-service-v2
```

The script validates the configuration (`--check`), installs a fresh release
directory, repoints `current`, and restarts the user unit. It never rewrites the
operator configuration and never touches RackAI.

## 8. Tests

```bash
python3 -m unittest discover -s tests/bridge -t tests/bridge -v
```

Coverage map (deterministic, against a controllable RackAI double):

| Required behaviour | Test |
| --- | --- |
| Initial demand-driven acquisition | `test_manager.AcquisitionTests.test_initial_acquisition_is_demand_driven_and_starts_pending` |
| Pending → ready | `test_pending_becomes_ready_without_a_second_reservation` |
| Two consumers, same details | `test_two_consumers_receive_identical_details` |
| Concurrent lookups, one acquisition | `test_concurrent_lookups_produce_one_acquisition` |
| Expiry → reacquisition | `InvalidationTests.test_expiry_triggers_reacquisition_with_a_new_identity` |
| Preemption → reacquisition | `test_preemption_reacquires_and_reports_the_rackai_conflict` |
| Switch releases before acquiring | `SwitchingTests.test_switch_releases_before_acquiring` |
| Release draining while active | `test_no_ready_details_while_the_old_reservation_drains` |
| `recovery_required` blocks replacement | `test_recovery_required_blocks_replacement` |
| Restart reconciliation | `RestartTests.*` |
| Unavailable + retry guidance | `RefusalTests.test_retry_guidance_prevents_a_busy_retry_loop` |
| No stale READY | `test_released_reservation_is_replaced_not_reused`, `test_missing_reservation_is_never_reported_ready` |
| 409 / 429 handling | `test_capacity_error_is_pending_and_conflict_is_unavailable` |
| Unique inference identity | `test_two_consumers_receive_identical_details` + `idempotency_key_required` in `test_projection` |
| No bridge owner credential for consumers | `test_api.HttpSurfaceTests.test_consumer_lookup_needs_no_bridge_credentials` |
| Privileged routes isolated | `test_ingress.IngressTests` |

## 9. Rollback

1. Stop and disable V2:

   ```bash
   systemctl --user disable --now rack-ai-shared-bridge.service
   ```

2. Release any bridge-held reservation through RackAI's public API (owner
   `hermes`, `release_reservation`), or let RackAI's 30-minute inactivity policy
   reclaim it.

3. Restore the previous ingress configuration:

   ```bash
   cp /srv/rack-ai-media/https/rollback/nginx.conf.before-scoped-<timestamp> \
      /srv/rack-ai-media/https/config/nginx.conf
   /usr/sbin/nginx -s reload -p /srv/rack-ai-media/https/ \
      -c /srv/rack-ai-media/https/config/nginx.conf
   ```

   The pre-removal bridge configuration is preserved as
   `nginx.conf.before` inside the legacy snapshot.

4. Restore the legacy bridge only if explicitly required (code, unit, state and
   proxy are in the snapshot). The V2 release tree remains under
   `deployments/shared-bridge/releases/` and can be re-selected by repointing
   `current` and restarting the unit.

## 10. Known limitations

- **Shared principal.** V2 authenticates to RackAI as the existing `hermes`
  source, as the legacy bridge did, so no RackAI configuration change was
  required. A dedicated `shared-bridge` principal is a future option.
- **Unauthenticated consumer lookup.** By design consumers do not identify
  themselves; anyone able to reach the private tailnet ingress can read the
  current reservation capability. The ingress is tailnet-only.
- **Root-required removal** of the obsolete control-proxy system unit (section 1).
- **One service at a time.** The bridge manages exactly one logical service and
  one reservation; it is not a multi-tenant broker.
- **RackAI serialises inference** inside a reservation; the bridge does not add
  parallelism, context management or scheduling.
- **UI authentication** is network-level only: the operator page has no HTTP
  authentication and relies on the Tailscale-only ingress.

## 11. Deployment record — 2026-11-03

| Item | Value |
| --- | --- |
| Branch / commit | `shared-service-v2` / `708360912171887b1576f166a8836e27fa08c09c` |
| Release | `/srv/rack-ai/deployments/shared-bridge/releases/708360912171` |
| Active release | `/srv/rack-ai/deployments/shared-bridge/current` |
| Configuration | `/srv/rack-ai/deployments/shared-bridge/config.json` (0600) |
| Unit | `rack-ai-shared-bridge.service` (user, enabled, active) |
| Listen | `127.0.0.1:18098` |
| Initial selection | `tiny-uncd` (deliberately low-impact; change in the UI) |
| Priority / TTL | `medium` / `1800` s |
| RackAI principal | existing `hermes` source (no RackAI configuration change) |
| Ingress backups | `/srv/rack-ai-media/https/rollback/nginx.conf.bridge-v2-20261103T125718Z`, `…nginx.conf.before-scoped-20261103T130220Z` |

Live verification performed against the production ingress:

| Check | Result |
| --- | --- |
| UI `GET /bridge/` | 200 without credentials over the private ingress |
| Discovered services | 9 qualified text-inference services; media/speech excluded |
| Operator activation | `reserve` issued, `preparing` → `ready` (≈20 s) |
| Consumer lookup (no credentials) | `ready`, no bridge bearer token in the payload |
| Second consumer | byte-identical reservation details |
| Direct inference without bridge token | HTTP 200, model completion returned |
| Distinct `Idempotency-Key`s | two invocations admitted (`accepted_calls = 2`) |
| Replay of one key | same result, no third invocation |
| Switch `tiny-uncd` → `local-uncd` | consumer saw `pending`; claims never owned by two reservations; replacement ready only after terminal release |
| Restart during an interrupted switch | switch resumed; no duplicate reservation |
| Restart at steady state | same selected service and reservation id; consumer still `ready`; single claim owner |
| Unrelated services | `rack-ai-runtime` 401, media receiver, private HTTPS all active; `/runtime/v1` not reachable through HTTPS ingress |
| Legacy remnants | no bridge process, worktree, state, user unit or 18099 listener |

Regression runs: `cargo test --workspace --offline` (all crates ok, 0 failures),
47 shared-bridge tests, and the RackAI runtime suites
`test_reservation_ownership_v2`, `test_idle`, `test_group_idle` (9 tests).

At the end of verification the bridge holds a ready `tiny-uncd` reservation;
RackAI's existing 30-minute inactivity policy expires it if it stays idle, and
the bridge reacquires on demand.
