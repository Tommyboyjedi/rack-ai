# External reservation idle reclamation

Date: 2026-09-15. Implementation base: PR35 `815afeb`.
All edits and tests are on gpurack in `/srv/rack-ai/.worktrees/pr35`.
This is an implementation and isolated-test report, **not a production cutover or GPU/model qualification**.

## Architecture found and change

PR35 already supplies one canonical `state/resources/managed.json` document,
the common `authority.lock`, complete resource claims, authenticated source policies,
frozen logical profiles, generation-fenced dispatch and verified backend teardown.
Legacy RackAI leases and the media adapter use the same resource authority.
This change completes those semantics; it does not create a CB reservation service.

The existing manual ComfyUI receiver still uses its dedicated legacy lease when started
from its launcher. Shared PR35 media activations use canonical managed grants. Both
paths use the existing native ComfyUI admission barrier. A small durable activity
record beside the media authority file records work and a closed admission latch;
it cannot grant resource ownership or reopen a released reservation.

A focused media configuration-validation extraction keeps listener, local-deployment
and source-policy responsibilities small. No class-size or safety exception was added.

## Common runtime lifecycle

Server `idle_timeout_seconds` defaults to **1800**, validated in 1..86400 seconds.
It applies to all authenticated runtime reservations, independently of source priority.
Acquire separate `local-primary` and `local-image` reservations for independent use.
An indivisible multi-GPU model remains one reservation because its shards cannot
be released independently.

Each demand serializes `last_activity_at: integer|null` in Unix seconds in the canonical
atomic authority document. Null means no recorded activity; `created` supplies the
initial idle baseline, including legacy records without the new field.
Timestamp updates never move backward. Status and result reads remain read-only.

| Event | GPU activity |
|---|---|
| New validated inference accepted under a reservation | Refresh that reservation |
| Accepted invocation changes to Started | Refresh under the same dispatch/authority transaction |
| Known completion, including known cancelled late output | Refresh that reservation |
| New validated reservation-bound managed image job | Refresh native activity under the enqueue barrier |
| Successfully admitted native POST /prompt or /api/prompt | Refresh native activity |
| Observed running/pending native queue, and its completion | Refresh native activity |
| Discover, inspect, result, reconcile, health/status polling | No idle refresh |
| Renewal, source credentials, browser connection, heartbeat, upload | No idle refresh |
| Idempotent replay of an accepted inference or media job | No idle refresh |
| Rejected request | No deliberate activity refresh |

A queue observation refreshes only when actual admitted work is busy or has just
completed; repeated observations of an empty queue do not refresh it.

The supervisor checks a read-only hint, then reevaluates inactivity under the canonical
authority lock. It records `released=true`, `state=releasing`, `reason=idle_timeout`.
New inference then fails the existing ownership/state fence. The existing retirement
path stops only the owned backend and proves cleanup before removing claims.
The terminal state is `expired`, with `idle_timeout` retained even if a client subsequently
sends release/cancel. Cleanup failure retains claims and recovery evidence.

Started and Uncertain runtime invocations block idle reclamation. Started work is never
killed by the idle reaper. Unknown outcomes remain fenced rather than treating a clock
as proof that a GPU is free. Explicit release/cancel retains its separately bounded
retirement behavior. Public reservation groups extend the persisted ownership deadlines
of owned Ready members while any member has recent activity or unresolved running work.
Initial TTL expiry still applies before activity; terminal or elapsed members are not revived.

The inference transaction and the idle transaction serialize on `authority.lock:
new execution admission winning refreshes activity; expiry winning rejects new work.
The final dispatch transaction still verifies owner, current generation, complete claims,
profile, waiting deadline and workspace scope. There is no second ownership mechanism.

Explicit renewal extends the ownership deadline without recording workload activity.
Public reservation activity also keeps ownership ahead of the inactivity decision, so
an active group cannot be retired by its initial TTL. Legacy standalone acquisitions
and inactive/held reservations retain their explicit renewal requirements. Clients
should explicitly release when done; idle expiry still requires verified shutdown cleanup.

## ComfyUI activity and the manual workflow

The native gate exposes authenticated internal idle observation and managed job-admission
controls. Its existing asyncio barrier covers both validation/enqueue and the idle decision.
It checks the actual pinned ComfyUI queue while that barrier is held. A nonempty queue
cannot be idle-closed. A late submission after idle closure is rejected.

Activity persists atomically in the authority file's `.activity.json` sibling, with
activation, last_activity_at, busy and idle_closed. This is generation-bound evidence,
not an ownership grant. A heartbeat cannot reopen its idle closure, and a restart of
the same activation retains the timestamp and closure. Corrupt/unwritable activity
evidence fails closed. The supervisor mirrors the observed timestamp into the canonical
managed demand before its terminal release.

A new validated managed job refreshes activity through the native barrier before its
media queue transaction commits. This closes the admission-versus-idle gap before
native dispatch. It is called only after owner/generation/priority/profile validation
and capacity checks; idempotent media replays return before this step. A failed durable
queue commit after admission may leave a conservative activity timestamp, but cannot
dispatch a missing job or grant ownership.

Legacy manual sessions use server `reservation_idle_seconds`, also default 1800.
This is separate from the existing short managed-service warm-idle `idle_seconds`
and the existing absolute `session_seconds` ownership limit.
The launcher still requires Start -> Ready -> Open -> Finish. It never auto-starts.
Idle closure sets the session's durable `terminal_reason=idle_timeout`, closes access,
drains, and releases through the existing path. Session inspection returns the reason;
the launcher displays an inactivity explanation and permits a fresh Start.
No interface redesign or model/library change is included.

## PR35 operation mapping

| Contract | Result with idle reclamation |
|---|---|
| acquire | Fresh identity; source policy, logical tag and ordinary priority admission |
| preparing / ready / held | Retain existing transitions; applicable idle demand expires |
| denied | Definitive retained denial; no reservation or activity privilege |
| generation | Existing fencing and rotated restoration generation retained |
| renew | Extends ownership deadline only |
| release | Existing explicit cleanup; idle reason remains auditable |
| infer | New admitted work refreshes its own demand; replays do not |
| reconcile / result / inspect | Read-only; never execute or refresh |
| cancel | Existing pending cancellation and bounded in-flight drain retained |
| source / priority | Authenticated server policy; no caller-selected identity privilege |
| profile/tag | RackAI selects physical devices and freezes qualified profile |
| reacquire | New acquisition identity; no continuing claim after idle release |

## CB policy and actual deployment findings

The checked-in runtime example authorizes source `cb` only for `local-primary`
and `local-image`, at Paramount. Its media example adds the matching CB identity
and Paramount ceiling, with `operator=false`. Other source records are unchanged:
ATHBA remains Low/Medium, including its existing Medium-only big-brain rule.
CB ordinary OpenRouter dialogue is outside this allocation path.

These are configuration examples, with synthetic token hashes, **not installed credentials**.
For deployment, provision one strong source credential securely; place only its SHA-256
in matching RackAI runtime and media principal records. Both must use source ID `cb`.
Use administrator mode-0600 configuration. Give the raw credential to CB separately
through the operator's approved credential mechanism; no credential was generated,
copied to the NUC, or printed by this task.

Live managed media currently has operator Paramount and director Medium; there is no
CB principal. Its approved managed profile is `local-image`, checkpoint
`/srv/fast/comfyui-models/checkpoints/Juggernaut_Ragnarok.safetensors`.
Repository qualification documents confirm that binding. It is **not a qualified
Krea2 managed profile**. A manual library selection is not equivalent to this pinned
managed workflow. Configure and qualify the intended Krea2 profile/workflow before
claiming that CB receives Krea2 through the managed job interface. No silent checkpoint
swap, CPU offload, CPU inference service, model download or qualification was performed.

PR35 examples still contain unqualified candidate profiles and placeholder artifact/config
bindings. They are not safe production replacement configurations. Pin actual service
config hashes, executable/artifact identities and qualification evidence, and migrate
legacy ownership through the existing PR35 rollout procedure.

## Backend exposure: found versus prepared

| Surface | Live inventory on gpurack | Prepared change |
|---|---|---|
| primary /8017 | Docker published on 0.0.0.0 and IPv6 all interfaces | Compose publishes only 127.0.0.1:8017 |
| coder /8018 | Docker published on 0.0.0.0 and IPv6 all interfaces | Compose publishes only 127.0.0.1:8018 |
| PR35 vLLM/llama.cpp hosting | Loopback endpoint URL was validated, actual bind argument was not | Require exactly one matching loopback --host value before non-fixture hosting |
| Native ComfyUI | Configured 127.0.0.1:8190; not listening in initial snapshot | Remains loopback; native authenticated mutation barrier retained and extended |
| RackAI media API/native proxy | 127.0.0.1:8191 /8192; Tailscale HTTPS :8444 -> /8192 | No live ingress change |
| llama.cpp/other raw GPU ports | No additional raw listener or llama-server found in process/socket inventory | No new execution surface added |

**Live network publications and deployed service configurations are unchanged.** Compose edits are in the isolated
PR35 worktree. Neither resident container was recreated, and no firewall, Tailscale,
production binary, unit, live source policy or permanent media gate was changed.
The current all-interface vLLM publications remain a potential external bypass.
A firewall-effective external reachability proof was not obtained: passwordless sudo
for firewall inspection was unavailable. This report does not claim the live invariant.

The original live checkout also contains administrator changes to compose.yaml and
config/repositories.json. Do not overwrite it with the worktree's whole Compose file;
apply only the loopback port mapping change during the planned cutover.
Quiesce direct backend clients, deploy the authenticated RackAI gateway and matching
native gate first, then recreate the two port publications on loopback (or apply
equivalent reviewed firewall isolation). Validate authorized API access and explicit
denial of each raw port from another host. Binding validation is defense against
configuration mistakes, not isolation from a trusted administrator shell.

## Verification

See the validation section below and logs under
`evidence/idle-reservations-20260915/`. Tests use disposable CPU HTTP processes and
fixture systemd/Docker/GPU inventory transports. They do not claim live GPU execution.

Coverage includes CB primary/image Paramount grants, disjoint allocations, exact 1800-second
simulated expiry and staggered use, polling/renewal/replay non-activity, start and completion
activity, concurrent authority-lock admission/expiry, running native queue protection,
native enqueue/barrier races in both orders, durable gate restart/failure behavior,
explicit release, resource reuse/reacquisition, ATHBA ceiling and shared media ownership.

Existing workspace/JCode gateway, cancellation, restart/teardown, manual browser launcher,
authentication and artifact tests remain part of the regression runs. No ATHBA or CB
repository, configuration, test or runtime was modified.

## Rollback and retention

Before deployment, reverting this patch has no live effect because the deployed services
and authority were not changed. Preserve untracked evidence and unrelated administrator
configuration. No commit, push, merge or history rewrite was performed.

For an eventual deployed rollback: stop new admissions, explicitly release/drain owners,
prove GPU/process cleanup and empty claims, and retain checksum-verified permission-preserving
copies of the full canonical authority, media state, gate activity evidence, binaries,
configs and service definitions. Restore the previously approved binaries/gate/configs
as a matched set while quiescent; remove only the added configuration fields if the
older strict schema rejects them. Older code ignores the added state fields but cannot
provide the new idle policy. Do not rewind state to resurrect an expired reservation,
delete unknown claims, or reopen raw ports as an automatic code rollback.
A failed/uncertain cleanup remains a recovery blocker.

## Remaining boundaries

- No production cutover, fresh GPU/model qualification, CB integration or Krea2 workflow
  qualification is claimed.
- No forced cleanup of unknown/in-flight operations. Such claims may outlive 30 minutes
  until ownership and cleanup are proven through the existing recovery path.
- Applying the policy to pre-upgrade records cannot recover historical activity that was
  never recorded. Quiesce/migrate existing activity before rollout.
- The manual legacy path retains its dedicated placement model; PR35 clients use shared
  priority claims. This patch does not replace the manual launcher with automatic acquisition.
- Independent coordinator review used a canonical RackAI lease and the existing endpoint
  fence, with no tools or write-capable workspace access. See its result below.

## Exact files changed

All paths below are relative to `gpurack:/srv/rack-ai/.worktrees/pr35/`.

- `compose.yaml`
- `config/media/config.example.json`
- `config/runtime/config.example.json`
- `config/runtime/response.schema.json`
- `crates/rack_ai_media/src/admission.rs`
- `crates/rack_ai_media/src/api.rs`
- `crates/rack_ai_media/src/config.rs`
- `crates/rack_ai_media/src/config_policy.rs`
- `crates/rack_ai_media/src/idle.rs`
- `crates/rack_ai_media/src/job_activity.rs`
- `crates/rack_ai_media/src/lib.rs`
- `crates/rack_ai_media/src/ready_lifecycle.rs`
- `crates/rack_ai_media/src/shared_activation.rs`
- `crates/rack_ai_media/src/types.rs`
- `crates/rack_ai_media/web/launcher.html`
- `crates/rack_ai_runtime/src/admission.rs`
- `crates/rack_ai_runtime/src/config.rs`
- `crates/rack_ai_runtime/src/control.rs`
- `crates/rack_ai_runtime/src/dispatch.rs`
- `crates/rack_ai_runtime/src/idle.rs`
- `crates/rack_ai_runtime/src/idle_tests.rs`
- `crates/rack_ai_runtime/src/inference.rs`
- `crates/rack_ai_runtime/src/lib.rs`
- `crates/rack_ai_runtime/src/media.rs`
- `crates/rack_ai_runtime/src/media_idle.rs`
- `crates/rack_ai_runtime/src/network.rs`
- `crates/rack_ai_runtime/src/retirement.rs`
- `crates/rack_ai_runtime/src/supervisor.rs`
- `crates/rack_ai_runtime/src/transition.rs`
- `crates/rack_ai_runtime/src/types.rs`
- `crates/rack_ai_runtime/src/validation.rs`
- `docs/external-reservation-idle.md`
- `docs/runtime-public-contract.md`
- `media/comfy_gate/gate.py`
- `tests/media/fake_comfy.py`
- `tests/media/test_idle_gate.py`
- `tests/media/test_idle_session.py`
- `tests/runtime/backend.py`
- `tests/runtime/test_hosting.py`
- `tests/runtime/test_idle.py`
- `tests/runtime/test_shared_idle.py`
- `tests/runtime/test_teardown.py`

## Final deterministic results

- Offline Rust workspace: **362 passed**, zero failures.
- Runtime HTTP/process/workspace suite: **75 passed, 2 subtests passed** (300.08 seconds).
- Complete media/browser/lifecycle suite: **80 passed** (618.19 seconds).
- Focused native ownership/idle correction, including shared active-image work: **21 passed**.
- Strict runtime Clippy, Rust formatting and patch whitespace: **passed**.
- Strict media Clippy: **8 existing findings**, reproduced exactly on an isolated archive of
  untouched PR35 HEAD. No lint suppression or unrelated cleanup was added.
- Structural inspection: every changed application implementation unit is below 100
  executable lines; other source-policy entries compare equal to HEAD.
- These tests use disposable CPU processes and mocked machine transports, including
  the browser tests. They are not client integration or production GPU qualification.

The first regression runs exposed stale response-schema/hosting fixtures and an
activity-status merge that masked an authoritative activation field. All were corrected;
the final tests above include direct regressions for the identity issue. Prior failed
logs remain retained alongside the final results.


## Independent review and final ownership check

The independent read-only coordinator review **accepted the code implementation**
after the deterministic checks. It received the complete task and patch, including
all new files, and the explicit distinction between code acceptance and production
cutover. The frozen reviewed patch SHA-256 is retained in
`evidence/idle-reservations-20260915/review-diff.sha256`.

The internal review acquired its resource through the existing canonical RackAI
reservation library, verified ownership and passed the existing endpoint fence
before one bounded inference. It did not start/replace a model or obtain write-capable
tools. The exact request, raw response, parsed result, lease handle and release proof
are retained under the same evidence directory. The review finished successfully;
its exact durable release receipt was verified, and it retains no GPU lease.
This one internal review inference is not a live external-client or Krea2 qualification.

Final acceptance is **implementation accepted; production cutover not performed**.
The remaining live-network exposure, credential/profile provisioning and migration
requirements above must be resolved before claiming production authority for external
applications. No commit, push, merge, CB/ATHBA edit or NUC code edit occurred.


## Deployment follow-up — 2026-09-15

The later authorized deployment installed matched receivers/native gate, qualified
image/interactive lifecycle, 1800-second policies and private backend publications.
The subsequent maintenance window enabled managed inference and corrected two live
startup/ownership issues. See [the current cutover report](inference-reservation-cutover.md)
for qualification, compatibility limits and rollback, and
[the first deployment report](external-reservation-deployment.md) for media evidence.
