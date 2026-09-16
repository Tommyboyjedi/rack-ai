> Historical PR35 design. Its application ceilings and service allowlists are superseded by [the reservation/work contract](reservation-work.md); existing lifecycle and safety invariants remain.

# Priority-aware runtime reservations and managed inference

Date: 2026-09-14. Status: **RackAI-only implementation and isolated qualification; production rollout deferred**.

The current user scope supersedes companion implementation instructions in this document.
Only RackAI code, configuration examples, synthetic clients, tests and PR35 are in scope.
ATHBA, Music Director and CB integrations are deferred follow-on work.

## 1. User outcome and scope

Rack AI is the single resource authority for competing applications. Applications request an authorized logical model/service tag and a priority. Rack AI resolves the runtime backend and the complete resource set, admits or rejects the reservation, starts the correct model, gates access, and safely holds/restores displaced work. There is no special operator-only `large-model mode` and no unmanaged llama-server experiment.

Global order is `low < medium < high < paramount`. A new conflicting reservation wins only when its priority is strictly greater than EVERY conflicting incumbent. The incumbent wins ties. A refused request must not disturb any incumbent, including lower-priority resources that would have been needed as part of a larger set.

This contract covers the generic reservation authority, existing vLLM lifecycle integration, existing ComfyUI integration, a bounded managed inference interface, logical tag configuration, a new llama.cpp adapter, and qualification of the downloaded GPT-OSS candidate. Companion applications implement their own adapters and interpretation; Rack AI does not acquire their domain semantics. CB is a future client demonstrated with synthetic fixtures, not an authorization to edit an unidentified repository or select/download its production model.

## 2. Reviewed baseline and relationship to existing work

Reviewed Rack AI `main`: `251c772da9f3f72247100988deb61b7ed881401f`, after PR24 and PR34. Recheck refs and live deployment before implementation; GitHub source is not proof of the deployed checkout/configuration.

Relevant implementation:

- `crates/rack_ai_application/src/generic_routing.rs`: priority vocabulary and source ceilings, not global runtime arbitration.
- `crates/rack_ai_infrastructure/src/resource_reservations.rs`: serialized, owner/generation-bearing resource sets and durable release; reservations have no priority. Reuse and extend this authority rather than create another lease directory/scheduler.
- `crates/rack_ai_infrastructure/src/registry_work_unit_worker_selector.rs`: current routing uses registered model/resource status; this is not live priority/residency enforcement.
- `crates/rack_ai_infrastructure/src/worker_record.rs`, `model_record.rs`, `resource_record.rs`: existing worker, model and resource registry boundaries.
- `crates/rack_ai_media/src/{admission,activation,lifecycle,placement,execution,supervisor,types,config}.rs`: existing managed media lifecycle, source ceilings and internal image queue ordering. Interactive sessions do not currently carry a reservation priority. The media resource acquisition does not propagate a priority.
- `config/{models,workers,resources}.json` and `config/media/config.example.json`: preserve existing identities and inspect actual local settings before migration.

PR24 deliberately shipped a dedicated 4080 media slot and protected resident development services, NOT a global preemptive scheduler. Preserve its process identity, native admission, artifacts, private access, restart and recovery protections while replacing the fixed allocation assumption with the shared authority.

Existing open [PR33](https://github.com/Tommyboyjedi/rack-ai/pull/33) records heavyweight-model research and qualification. This contract supersedes its **integration-last/unmanaged-test sequence and special exclusive-mode integration** for this work. Retain its useful benchmark evidence and `>10 generated tokens/second` practical acceptance gate; a process loading is not qualification. Do not merge its stale branch wholesale, expand this task to every research candidate, or close it without instruction. PR18/PR25 remain broader roadmaps.

## 3. Explicit, narrow boundary amendment

The user has authorized logical tags, managed inference and priority-controlled yielding. Update `AGENTS.md`, `agent.MD`, the relevant engineering/runtime docs and ATHBA boundary docs during implementation so these decisions do not conflict with older capability-only/dedicated-GPU statements.

A logical tag such as `big-brain` or `local-fun-chat` is a **Rack-AI-published service alias**, not a concrete model ID, worker, GPU, endpoint or JCode profile. Clients may request an allowed tag, or retain existing broad-capability routing. They cannot author internal eligibility profiles, resource placements, launch arguments or backend choices. Tag resolution must still satisfy capability, context, qualification and source policy. Rebinding a tag to a new concrete model affects future reservations only; existing work retains its resolved version/provenance.

Pure inference is a separate bounded operation, not a fake Git/workspace transaction. Repository-changing work still uses the existing trusted workspace/harness/evidence contract. Chat access never grants host tools or repository mutation. Source ceilings and path/network/acceptance/review protections remain intact. No client-specific workflow states, TDD concepts or escalation tiers belong in Rack AI.

Adding `llama_cpp` is explicitly allowed **alongside** vLLM, not as a replacement. This contract authorizes the named media-client Paramount policy, not a general increase to other clients' ceilings or weakening of authentication. No architectural size/unsafe exception is authorized.

## 4. Priority, identity and policy

Use one canonical typed priority domain across workspace, media, inference and reservations. Preserve wire spellings and deliberately migrate the two existing enums.

The authenticated server-side principal determines source identity, allowed tags/operations, allowed priorities/defaults and ceiling. A submitted `source_system` string cannot confer authority. CLI/local adapters need a documented trusted identity binding too; a caller-selectable state root, source name or environment variable is not an authentication mechanism. Unknown sources fail closed on the new service surface; do not inherit the existing wildcard Paramount ceiling as anonymous remote access.

Deployment policy to document and validate:

| Source/use | Permitted request policy |
| --- | --- |
| ATHBA ordinary local work | low or medium only; retain current per-operation mappings |
| ATHBA big-brain | medium; never implicitly high because the model is large |
| ComfyUI interactive operator session | paramount for new sessions |
| Music Director managed local image work | paramount for new submissions under its authenticated principal |
| CB future interactive model client | paramount, explicitly provisioned principal/tag allowlist |
| Another authorized application requesting big-brain | medium or high, within its own ceiling |

Priority belongs to a granted demand/reservation, not a model's permanent identity or an application's label. An existing Low ATHBA reservation does not become Medium because of a model switch, and there is no automatic aging/promotion through a ceiling. Defaults, permitted values and ceilings must be distinct, typed configuration. Existing immutable submitted requests retain their recorded priority; do not rewrite history or replay a Medium submission as Paramount.

## 5. Reservation semantics

Distinguish: logical request, granted reservation, resident runtime activation, individual invocation, suspended demand, and infrastructure transition. Record owner/principal, opaque request/idempotency identity, tag and resolved profile version/hash, full resource set, priority, activation and generation, desired/observed state, deadlines, and decision/evidence references.

### New acquisition

1. Authenticate and validate the request, policy, tag, qualification, full resource requirements and memory/disk preflight.
2. Under the shared authority, evaluate the complete resource set AND the complete activations that would need eviction.
3. If ANY conflicting active/transitional reservation has equal or higher priority, deny the entire new reservation. A lower conflict cannot be stopped as a side effect of a denied multi-GPU request.
4. Unknown, malformed, legacy-unmigrated, unhealthy or uncertain ownership also blocks. Priority never overrides safety.
5. If all conflicts are lower, persist a serialized transition plan/fencing generation before side effects, close their admission, drain bounded in-flight work, unload the owned runtimes, verify release, acquire/activate the requested set, verify readiness, and only then enable access.

Priority makes a takeover eligible; it does not prove resources are ready. Return an accepted transition promptly and expose its progress. Do not block an HTTP handler throughout model load/unload.

### Ties and ordinary use

A new competing reservation at the same priority is denied even from the same application. An identical replay of the original request returns the SAME durable decision/handle and never counts as a competing reservation. Changed-payload identity reuse is a conflict. A denial replay remains the same denial; a deliberate later acquisition uses a new reservation-request identity linked to the same logical work.

Requests made under an already granted, valid reservation are ordinary use, not repeated reservation acquisitions. Enforce owner, generation, resolved model, expiry and bounded per-runtime concurrency. This distinction permits multiple images/chat turns inside a legitimate session without violating incumbent-wins-ties. No implicit cross-principal joining or borrowing of another client's Paramount reservation. Managed media jobs must be explicitly associated with the correct authorized reservation rather than obtaining a priority-less service lease.

### Independent versus indivisible resources

ATHBA's normal primary and coder are **two independently preemptible model reservations**, optionally linked by a client grouping ID. They are not one indivisible two-GPU activation. Losing the primary must not release, revoke or stop the coder.

The first `big-brain` profile is one indivisible three-GPU activation. A future Paramount request for one of those GPUs must safely stop the entire lower-priority big-brain activation; it cannot detach a live model shard. Its other GPUs become available after verified stop, and any prior suspended demands are reconsidered under normal policy. Model tags that alias the same physical GPU must conflict on the actual canonical resource, not just their tag strings.

### Hold, restoration and invocation truth

Preempted granted demand becomes Held/Preempted, not a semantic failure and not a live claim to physical VRAM. New work under that held reservation is durably held; other independent reservations remain usable. Preserve opaque identities and return visible reasons. Resume still-valid held demand when blockers release, ordered by priority and durable original order, but do not let restoration preempt a new equal/higher incumbent. Cancelled/expired demand is never resurrected. Persistent Paramount demand may starve Low demand under this strict policy; expose this instead of silently raising priority.

Default takeover is bounded drain: close new dispatch and allow an already-running invocation to complete before unload. Do not use OS process suspension as GPU release. A deadline without proven cleanup yields an explicit blocked/recovery outcome; it does not authorize stealing VRAM. Any destructive abort policy must be separately explicit and report interruption.

Automatic restoration means restoring availability and dispatching accepted work that has **not started**. It does not mean rerunning an interrupted/uncertain model invocation or replaying workspace tools. Preserve actual invocation-start/outcome evidence. Additional semantic model attempts remain client-owned. Pause/cancel and exact revision/acceptance checks continue to win over late completion.

Define bounded reservation renewal/expiry and hold deadlines. Browser disconnect is not release. Heartbeat expiry alone is not proof that CUDA allocations disappeared. Retain a fenced recovery record until process/GPU release is verified.

## 6. Atomic decisions, recoverable effects

Do not claim that several OS process stops/starts are physically atomic. The guarantee is serialized all-or-none admission and a durable, recoverable multi-step transition with exclusive fencing.

Use one machine-wide administrator-configured resource authority across all worktrees, processes and service adapters. Preserve existing owner/generation verification and durable write/release protections. Do not hold a global filesystem lock while performing slow model startup/network calls; persist an exclusive transition under the lock and revalidate its generation at effect boundaries.

Test concurrent equal/high requests, opposite acquisition order, cancellation during drain/load, a newer higher request during transition, backend failure after one victim stops, receiver death at each persistence boundary, partial release, reboot and stale callbacks. Failed activation must reconcile/restore only still-valid displaced demand. Never announce Ready or release a lease based on an HTTP response, idle utilization or a recycled PID alone. Unknown cleanup remains quarantined and visible.

## 7. Registry and runtime adapters

Extend the existing registry deliberately rather than constructing a second conflicting model catalog. Use typed definitions for logical aliases, concrete model artifacts, runtime profiles, capability/qualification constraints and physical resource requirements. The application-facing tag resolves to an immutable configuration snapshot per reservation.

Illustrative configuration relationship (not an already implemented wire schema):

```json
{
  "tag": "big-brain",
  "model_profile": "gpt-oss-120b-mxfp4",
  "runtime_profile": "llama-cpp-gpt-oss-120b",
  "resource_ids": ["gpu-4080-super", "gpu-4060ti", "gpu-2060"]
}
```

The referenced runtime profile owns `backend = llama_cpp`, pinned executable/container identity, local artifact path/hash, launch configuration, CPU/RAM limits, context/concurrency, backend protocol and health/stop policy. Tag configuration must not grant a client arbitrary executable paths or shell arguments.

Initial profiles:

| Tag/service | Hosting backend | Placement |
| --- | --- | --- |
| local-primary | existing qualified vLLM | 4060 Ti |
| local-coder | existing qualified vLLM | 2060 |
| local-fun-chat | configurable supported inference backend | 4060 Ti; synthetic fixture until its actual model is supplied/qualified |
| ComfyUI / existing local-image operation | existing ComfyUI adapter | 4080 Super |
| big-brain | new pinned llama.cpp adapter | all three GPUs plus explicitly budgeted host RAM/CPU |

Changing/adding a model within a supported backend should be configuration plus qualification, not application code changes. Adding an entirely new hosting backend still requires an adapter and contract tests. Validate tags, missing artifacts, duplicate/overlapping physical IDs, unsupported backend parameters, ports, per-device budgets and profile versions before effects. Host UUIDs belong in protected administrator configuration, not positional `CUDA_VISIBLE_DEVICES=0` assumptions.

JCode is a tool/execution harness; vLLM/llama.cpp host model inference. Do not conflate these backend layers. Preserve single-resource worker loading compatibility while making placement, reservations and provenance truthfully reflect multi-resource activations where applicable.

## 8. No bypass: managed inference and all existing callers

Add the smallest versioned authenticated service/reservation API needed by applications: discover allowed tags/capabilities without activation; acquire/read/renew/release reservation; submit bounded inference under a reservation; inspect/cancel/reconcile submissions; retrieve results and generic evidence. A submission may request an implicit reservation in the same operation, but must obey exactly the same authority and identities.

Publish exact versioned schemas/OpenAPI and synthetic request/result/denial/held/interrupted fixtures before companion implementation. Freeze and share those files across Rust/Python; names used here are conceptual unless explicitly adopted. Define HTTP/error distinctions for unauthorized/ceiling violation, unknown or unqualified tag, incumbent-priority denial, preparing/held, interrupted/uncertain, expired/cancelled, timeout and recovery-required. A priority-denied request is NOT accepted queued work. Same-reservation capacity queuing and already-held accepted demand remain separate.

Preserve whatever Chat Completions/Responses/streaming/structured-output/tool-call semantics the existing callers actually use. Qualify translation where a backend lacks a protocol; an OpenAI-shaped endpoint alone is insufficient. Bound input/output/context/request time/concurrency and record actual invocation identity, usage and outcome. A reconnect cannot silently run a second invocation. Readiness verifies the expected model and activation, not merely `/models` HTTP 200.

All normal model callers must pass the authority: workspace execution, JCode provider routes, local-primary review/recovery paths, direct reasoning clients and native ComfyUI admission. Gate immediately before backend dispatch, with generation checks, not just at initial selection. Use a stable Rack-AI-owned gateway or equivalently enforceable scoped proxy so a client cannot continue sending to a preempted backend/changed model endpoint. Raw hosting endpoints remain private and are not advertised as an alternate client route.

Map the trusted-host/direct-CLI routes explicitly; do not claim protection against root or arbitrary same-user out-of-band administrator actions. A cooperative API without real gating on the supported client paths does not satisfy acceptance.

## 9. ComfyUI and application integration

Reuse the existing ComfyUI lifecycle and native admission gate. A session is Paramount while its authorized reservation remains active even when no render is running. All its actual GPU usage must fit its declared resources; qualify supported workflows and reject out-of-profile multi-GPU behavior rather than advertise global enforcement for arbitrary custom nodes.

Replace hard-coded dedicated-4080/protected-two-workers assumptions only when their shared-authority replacements are implemented and tested. Do not simply remove the existing guards. Start/Finish/normal restart, authenticated native HTTP/WebSockets, closed browser behavior, managed rendering and exact artifact import must remain working. Different paramount clients needing the same GPU receive a deterministic denial; they cannot evict each other. Native interactive and managed sessions retain their ownership distinction.

The Music Director companion must emit the authorized policy for new image requests and display denial/held/interrupted states truthfully without new renders or provider fallback. The ATHBA companion must preserve Low/Medium mapping, add durable hold/resume and managed local reasoning/big-brain access, and keep its semantic ledger/attempt decisions internal. Neither client owns GPUs, installs runtimes, stops containers or raises its own ceiling. No CB repository edits in this task.

## 10. Migration, rollout and live safety

This is meaningful lifecycle/contract work, not merely adding a field to a lease. Keep the implementation cohesive and staged; do not promise a small refactor before measuring affected paths.

1. Build deterministic authority/state/adapter tests first, using disposable roots and fake backend processes. Freeze cross-repository fixtures.
2. Implement managed vLLM/ComfyUI/llama.cpp adapters and gate all supported paths. Preserve existing deployments by default until explicit migration.
3. Exercise RackAI-owned synthetic clients through real transport against controllable backend processes and disposable state. Companion adapters are a later task after RackAI review/acceptance.
4. Prepare a read-only inventory and written migration/rollback plan: deployed SHAs, service/container identities, verified full GPU UUIDs, actual caller endpoints, in-flight work, all relevant state roots, resident memory and restart policies. Inspect service configuration without printing secrets. Checked-in Compose is not proof of live configuration.
5. **Do not enable production preemption or stop existing vLLM/media services until the migration plan has been reported and the operator explicitly authorizes the disruptive live window.** Requesting these planning PRs does not itself grant that window. Build/test completion must not be blocked by this approval gate.
6. Under that later authorization, drain active work, install/pin the isolated new runtime without replacing NVIDIA drivers or vLLM dependencies, import/reconcile managed service ownership and reservation state, switch supported clients/gates, then qualify. Never run whole-stack Compose down/up, delete lease files, force-reset live checkouts or adopt a foreign process by port alone.
7. Verify release/restoration and document rollback to the prior known deployment while retaining new durable evidence. Restoring an old binary against an incompatible state schema is not a rollback plan. Reconciliation and schema backup/migration must be explicit.

Legacy requests and records remain readable or produce a precise migration-required result. Missing new configuration must not accidentally break the old deployment before cutover. Do not reinterpret old priority-less ownership as Low/free. Persisted accepted Medium image jobs are not promoted; drain/migrate them deliberately. Do not enable global preemption while an actual application still bypasses admission; report client cutover status separately.

## 11. GPT-OSS qualification

User-supplied host snapshot: Threadripper 1950X, approximately 60 GiB visible RAM / 49 GiB available at capture; 4080 Super 16 GiB, 4060 Ti 16 GiB, 2060 6 GiB; existing model file `/srv/models/gpt-oss-120b/gpt-oss-120b-MXFP4.gguf`, 63,387,346,208 bytes. Snapshot availability is not a reservation or capacity guarantee. Verify file integrity, pinned runtime support and actual per-device/host memory before load. Nominal VRAM is not unified memory and mmap does not eliminate working-set requirements.

Use the first all-three profile through the new authority, not a free-standing server. Start with bounded context/concurrency to establish safe loading, then measure representative populated context. Record runtime/build, artifact/config hashes, placement, actual RAM/VRAM, CPU/threading, startup/recovery time, prompt processing, time to first token, sustained decode, total response latency, structured/tool compatibility where claimed and a fixed quality corpus. Continuous swap/expert paging or impact outside declared resource bounds fails qualification.

Carry forward PR33's practical gate: sustained decode **greater than 10 tokens/second on every representative test**, representative approximately 24K populated input within a 32K context, correct outputs and clean restoration. Smaller-context boot success and fixture success are separate evidence, not this gate. If hardware fails it, report measured failure and leave the profile unqualified; the scheduling feature may still be code-complete. A later measured two-GPU profile may be compared through the same authority, but must not silently replace the three-GPU tag/resource requirement or evade its priority denial.

Do not download additional large models, use paid providers, or silently retune existing production models to make acceptance pass. Normal development uses fakes; live GPU runs occur only after authorization and admission.

## 12. Required acceptance matrix

| Case | Required observable result |
| --- | --- |
| ATHBA Low primary + Low coder | separate grants; both usable |
| ComfyUI Paramount acquires idle 4080 | succeeds; both ATHBA models unaffected |
| CB Paramount requests fun-chat on 4060 | boundedly drains/unloads primary; only primary demand held; coder continues |
| High big-brain then requests all three | immediate whole-request priority denial; no stop, lease or mutation of the otherwise available/Low coder |
| Another Paramount conflicts with Paramount | incumbent unchanged; incoming denied |
| Equal/low requests on disjoint resources | allowed when otherwise valid; priority is not a rack-wide global lock |
| CB release | valid held primary restored; ComfyUI/coder unaffected |
| Medium big-brain with only Low incumbents | wins after safe transition; all three reserved under one activation |
| ComfyUI Paramount arrives during Medium big-brain | entire indivisible big-brain drains/stops; image can start only after verified release; interrupted calls are not replayed |
| ATHBA tries High/Paramount or forged source | denied before effects at client and server boundaries |
| Same reservation replay / ordinary model call | same decision or valid use, not a competing tie |
| Changed-payload replay / different principal | rejected; no duplicate model call or authority leak |
| Simultaneous conflicting acquisitions | exactly one valid owner; no deadlock or partial unrecorded takeover |
| Alias maps to already occupied physical GPU | conflict detected despite different tag names |
| Owner crashes, reboot, stale generation, failed stop/load | durable reconciliation or recovery-required; no double allocation/false readiness |
| Cancel/expiry while held or starting | no resurrection, no late dispatch/promotion |
| Registered tag's backend changes for future requests | clients unchanged; current reservation keeps frozen profile; actual provenance verified |
| Unsupported/unknown model, insufficient host memory or unqualified profile | precise refusal before destructive effects |
| Existing CLI/JCode/Responses/media routes | real gate enforced; normal protocol and evidence preserved |
| Client restart/browser close/import replay | no lost ownership/work, duplicated invocation or duplicated image attachment |

Use public-interface tests plus actual HTTP/CLI transport with controllable fake services. Include forced race/persistence-failure tests, not only mocked policy comparisons. Retain existing regression tests; report baseline failures separately and never weaken safety tests. Keep Rust types small, typed and composed under the repository coding principles; extract responsibilities before extending oversized units.

## 13. Delivery and reporting

This PR begins as a contract, not completed implementation. Implementation stays exclusively on this RackAI branch. Linked companion branches are deferred references only. No merge, force push, production migration or unrestricted cross-repository repair is authorized.

Report independently: `RACKAI_CODE_COMPLETE`, `PRIORITY_SCENARIO_PASSED`, `RECOVERY_AND_RACE_TESTS_PASSED`, `RACKAI_LIVE_QUALIFIED`, `BIG_BRAIN_BOOTED`, `BIG_BRAIN_QUALIFIED`, and `PRODUCTION_ROLLOUT_STATUS`. State NOT_RUN/BLOCKED with exact reason where relevant. Include PRs/SHAs, tests, schema versions, evidence, benchmark results, rollback and remaining approval gates. Do not label a planning document or a running process a qualified system.
