# Codex task — priority reservations and managed inference

Implement the approved direction in these existing planning PRs. They currently contain contracts, not completed runtime implementation. Start in Rack AI; complete the core first and the named client adapters second, using separate branches/worktrees and each repository's rules. Do not replace this task with another design-only PR.

## Assigned PRs

1. `Tommyboyjedi/rack-ai` PR35:
   https://github.com/Tommyboyjedi/rack-ai/pull/35
   Branch: `design/priority-runtime-reservations`, based on `main`.
   Contract: `docs/priority-runtime-reservations.md`.
2. `Tommyboyjedi/musicvideo-director` PR34:
   https://github.com/Tommyboyjedi/musicvideo-director/pull/34
   Branch: `integration/rack-ai-priority-reservations`, stacked on `integration/rack-ai-media` / PR33.
   Contract: `docs/rack-ai-priority-reservations.md`.
3. `Tommyboyjedi/ATHBA` PR31:
   https://github.com/Tommyboyjedi/ATHBA/pull/31
   Branch: `integration/rack-ai-priority-reservations`, stacked on `design/post-behavior-naming-refactoring` / PR30.
   Contract: `docs/rack-ai-priority-reservations.md`.

This task-specific cross-repository scope is limited to these three contracts. Keep changes in the owning repository. Do not change CB or any other application. Do not move private Music Director code/data into public repositories; use synthetic shared fixtures. Do not merge, close existing PRs, force-push or rewrite published history.

## First actions

Read applicable AGENTS.md, agent.MD, coding_principles.MD and the three contracts. Inspect actual newer changes and tests before editing. The Rack AI baseline reviewed for the contract was `251c772da9f3f72247100988deb61b7ed881401f`; companion ancestry is recorded in their documents. Recheck current refs rather than assuming either application's integration is on its default branch.

Verify the existing authorized `gpurack` access, repo/worktree permissions and build/test environment. Use isolated worktrees and disposable state for development; do not reset live `/srv/rack-ai`, `/srv/ATHBA` or the working Director installation. Rack build/test work belongs on the rack where available, not a replacement NUC deployment. Missing access is a concrete blocker to rack-side verification, not permission to fabricate live results.

Maintain a concise checklist and evidence notes for interruption/resume. Implement in small coherent slices with focused tests; do not repeatedly reread whole repositories or conduct another broad model survey.

## Non-negotiable behavior

- One priority order: low < medium < high < paramount. For a NEW conflicting reservation, strictly higher may preempt; equal or lower loses. Incumbent wins ties.
- Evaluate ALL required resources and complete victim activations before effects. Any equal/higher blocker denies the entire new request without stopping a lower-priority incumbent on another needed GPU.
- Separate admission atomicity from slow OS effects: persist fenced transitions, drain/unload/reconcile owned services, verify physical release, then activate. No double allocation or fake all-or-none process transaction.
- ATHBA's primary/coder demand is independently preemptible. Displacing primary holds only affected work; unrelated ready coder work can continue under existing repository/dependency rules.
- A big-brain activation spans an indivisible three-GPU set. Higher-priority demand for one shard must safely stop the entire activation, never detach a live shard.
- Distinguish a new priority-denied acquisition from accepted work later held/preempted. Same-request replay returns the same decision; ordinary calls/jobs under a valid reservation are not new conflicting acquisitions. Preserve owner, generation, identities and resolved profile versions.
- Restore still-valid held demand when admissible. Never resurrect cancelled/expired work, replay uncertain/started inference or rerun workspace tools invisibly. Bounded drain is the default takeover behavior; an unproven cleanup remains recovery-required.
- ATHBA remains Low/Medium only, including Medium big-brain requests. New authorized ComfyUI interactive sessions and Music Director managed image requests use Paramount. Future CB uses an explicitly provisioned Paramount principal/tag allowlist. No priority promotion based on model size, waiting time or forged source_system; no automatic paid fallback.
- Logical tags resolve to concrete model artifacts, hosting backend, launch profile and full resource set inside Rack AI. Keep broad-capability routing compatible. Add llama_cpp alongside vLLM; JCode remains a separate harness concept.

## Implementation sequence

1. Freeze versioned reservation/inference schemas, typed outcomes and synthetic Rust/Python fixtures. Explicitly amend old capability-only/dedicated-GPU documentation for the authorized logical-tag and shared-authority boundary; preserve all unrelated safety and semantic rules.
2. Extend the existing canonical reservation authority and durable state. Consolidate priority types. Implement authenticated source policy, full-set conflict decisions, fenced takeover, per-model hold/restoration, expiry/cancel and crash recovery. Keep decisions separate from I/O and obey small-type/composition rules.
3. Bring supported existing vLLM services and ComfyUI under managed activation and the same authority. Preserve PR24 native admission/private access/restart/artifact protections and PR34's permanent model library. Gate every supported model path immediately before dispatch, including JCode, local review/recovery and direct reasoning. A priority field with bypassable raw endpoints is not completion.
4. Add bounded managed inference and the isolated pinned llama.cpp adapter. Preserve the actual Chat/Responses/streaming/structured-output/tool protocols callers use, with tested translation where needed. Register the downloaded GPT-OSS candidate through a versioned, initially unqualified profile; do not launch it outside Rack AI. Qualify model identity, context and per-device/host-memory budgets rather than equating /models HTTP 200 with readiness.
5. Implement Director PR34 against the frozen core contract: typed new-request Paramount policy, definite denial versus uncertainty/hold, persistent identities and exact artifact import. Preserve stored Medium requests and explicit legacy backend selection. Do not rerender or silently fall back after denial.
6. Implement ATHBA PR31: preserve priority mappings and semantic boundaries; durable remote status/result/cancel and hold/resume; managed local reasoning behind its existing gateway; no lost attempts, bypassed dependency/revision gates or hidden reinvocations. Leave TDD/naming/refactoring prompts and algorithms alone.
7. Run the exact scenario below through real transports with fake backends/disposable client state, plus failure/race/recovery and existing regressions. Complete all code/fixture work possible without a production cutover.

## Required scenario

ATHBA Low has independent primary on 4060 Ti and coder on 2060. ComfyUI Paramount reserves 4080. CB Paramount requests local-fun-chat on 4060: primary yields safely and becomes held, coder remains usable. Another authorized app requests High big-brain needing all three: immediate whole-request denial, with no change to coder. Equal Paramount conflicts also lose. CB release restores valid held primary without disturbing ComfyUI/coder.

Also test the reverse: Medium big-brain may take all three from only Low incumbents; a later Paramount ComfyUI request safely drains/stops that whole activation before using the 4080. Preserve truthful invocation/interruption evidence.

## Live change gate

No production preemption, existing vLLM/media stop/reconfiguration, client cutover, live database migration, driver change, whole-stack Compose operation, lease-file deletion or GPT-OSS GPU run is authorized merely by this implementation prompt.

Prepare and report a read-only deployment/caller/GPU UUID/resource-memory inventory and exact migration/rollback plan. Keep production preemption disabled until the operator separately authorizes the live window and all actual clients have gated routes. Continue code/tests even if that window is unavailable; report the live gate explicitly rather than stopping after planning.

Use fake models for normal development. No paid calls, additional large downloads or production-model retuning. The existing artifact is `/srv/models/gpt-oss-120b/gpt-oss-120b-MXFP4.gguf`; verify it locally before any later authorized load. Preserve PR33's measured >10 generated tok/s and representative useful-context qualification gate separately from safe boot and scheduler code completion. Do not claim memory fit or throughput in advance.

## Verification and handoff

Run focused unit/contract/race/persistence tests, real HTTP/CLI fixture integration, relevant media/JCode/workspace/client regressions, each repository's required static/test checks and diff/whitespace review. Do not weaken tests or claim inherited failures are caused/fixed by this work without evidence.

Push changes to the three existing branches and update their PR descriptions. Report changed responsibilities, actual tests, SHAs and evidence. Separate CONTRACTS_FROZEN, CODE_COMPLETE, FIXTURE_E2E_PASSED, CLIENT_ADAPTERS_COMPLETE, CLIENTS_CUT_OVER, LIVE_PREEMPTION_QUALIFIED, BIG_BRAIN_BOOTED, BIG_BRAIN_QUALIFIED and PRODUCTION_DEPLOYED, with NOT_RUN/BLOCKED reasons. Include the prepared migration/rollback plan and remaining approval gates. No secrets in output. No automatic merge.
