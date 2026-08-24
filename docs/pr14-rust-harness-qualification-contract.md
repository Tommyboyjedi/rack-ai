# PR14 Contract — Rust Coding Harness Qualification and Routing Policy

## Status

Final qualification contract for the architecture-reset harness decision.

PR14 is documentation/experiment focused. It does not implement the production adapter.

The purpose of PR14 is to establish which Rust-native coding harness is qualified against the actual rack workers and what routing constraints later implementation must preserve.

## Strategic boundary

Rack AI is the control plane above coding harnesses and inference backends.

Rack AI owns:

- campaign/task state;
- worker/model/GPU registration and placement;
- target repository/worktree authority;
- filesystem, network and process isolation;
- timeouts/cancellation;
- deterministic acceptance;
- independent review;
- no-change rejection;
- retry/replan/fallback/escalation;
- durable evidence;
- Git commit/promotion authority;
- selection of the qualified worker/harness route.

The coding harness owns model-facing coding mechanics inside the bounded execution environment:

- source navigation/search;
- edit/patch mechanics;
- implementation-time tool/command use;
- compiler/test feedback loops;
- model/tool-call handling;
- harness-local context management.

The harness may report success. Rack AI decides whether the work is accepted.

## Rust requirement

Production coding harnesses in the trusted long-running Rack AI stack must be Rust-native.

Target-project languages are unrestricted; this requirement applies to Rack AI and its coding-harness layer.

## Final candidate outcome

The initial candidate set was JCode direct/non-swarm execution and Abacus.

Qualification produced:

```text
JCode direct/non-swarm:
  local-primary: qualified
  local-coder: qualified_with_constraints

Abacus:
  not_qualified
```

The production harness for the next integration PR is therefore JCode.

Rack AI does not maintain multiple harnesses merely for optionality. A future Rust-native harness may be added only after independently passing the qualification requirements relevant to a real production role.

## JCode route

### `local-primary`

```text
endpoint = 127.0.0.1:8017
served_model = local-primary
model = cyankiwi/gemma-4-12B-it-AWQ-INT4
context_window = 131072
classification = qualified
```

### `local-coder`

```text
endpoint = 127.0.0.1:8018
served_model = local-coder
model = NotaMG/eqaq-v2
context_window = 16368
max_num_seqs = 1
JCode tool profile = minimal
classification = qualified_with_constraints
```

The local-coder route is intended initially for bounded implementation work with deterministic acceptance and independent final worktree inspection.

## Local-coder constraints

The approved small-worker route must preserve these constraints until superseded by new qualification evidence:

- JCode must be configured with the real `context_window = 16368`;
- use JCode `minimal`, not `full`, for the current 16K coder route;
- Rack AI must inspect final Git diff/status independently;
- unexpected/unapproved files are acceptance failures unless explicitly permitted;
- deterministic tests/checks remain authoritative;
- harness-reported success is not acceptance;
- broader/harder work may be routed directly to a stronger qualified worker.

These constraints are capability metadata, not permanent GPU-size or parameter-count rules.

## Context conclusion

PR14 demonstrated that incorrect context metadata and excessive harness/tool-profile overhead can make a viable small worker appear unusable.

With the real 16,368-token context configured and JCode `minimal`, the Qwen3.5 worker had sufficient runway for the tested bounded multi-step repair class.

Therefore PR14 does **not** require a bespoke Rack AI context-budget governor/checkpoint subsystem before production integration.

Rack AI should continue to record true context capacity as worker/model metadata and may add more sophisticated admission, checkpoint or escalation behaviour later when measured production evidence justifies it.

## Required qualification evidence satisfied

The selected JCode route has evidence covering:

1. localized real repository edits;
2. compiler/test repair;
3. multi-file compatibility work;
4. repository navigation;
5. native tool-call behaviour for the active local-coder model;
6. bounded failure/recovery observations;
7. truthful no-change behaviour;
8. network-disabled operation while retaining local vLLM access;
9. explicit provider/model binding;
10. simultaneous independent sessions against `8017` and `8018` without cross-binding;
11. independent final Git/worktree inspection;
12. true-context configuration and comparison of JCode `full` versus `minimal` overhead.

The final evidence and exact observations are recorded in `docs/pr14-harness-qualification-report.md`.

## Network/isolation requirement

A qualifying harness must be invokable without remote Git credentials or promotion authority and must work inside Rack AI's outer isolation boundary.

PR14 proved JCode can operate while normal external TCP access is denied and loopback access to the local vLLM endpoint remains available.

The PR14 `LD_PRELOAD` guard was a qualification mechanism only. Production isolation remains a Rack AI responsibility and should use the normal kernel/container/process controls of the deployed platform.

## Endpoint-binding requirement

Direct JCode provider/model routing must remain explicit.

Current endpoints are:

```text
local-primary -> 127.0.0.1:8017 -> served model local-primary
local-coder   -> 127.0.0.1:8018 -> served model local-coder
```

Concurrent qualification proved both sessions can operate simultaneously without cross-binding.

## JCode swarm decision

JCode swarm is not part of the current production architecture.

The current RTX 2060 coder is configured with `max_num_seqs = 1`. On the present two-card rack, swarm-lite therefore adds little useful hardware parallelism over Rack AI directly scheduling one independent task per worker.

Rack AI also deliberately owns worker selection, resource policy, acceptance and recovery. Deep/DAG swarm would overlap with those control-plane responsibilities.

Swarm may be reconsidered later if:

- the rack has more genuinely concurrent workers;
- a worker endpoint supports useful concurrent sequences;
- or swarm provides a concrete capability that Rack AI cannot provide more cleanly through direct scheduling.

No swarm qualification or dependency is required for the next production integration PR.

## Abacus decision

Abacus is `not_qualified` for the current Rack AI production stack.

Although it passed small tasks, repeated bounded multi-step repair runs showed provider timeouts, repair drift, failing final repositories and unexpected workspace artifacts after the real context limit was configured.

Abacus has been removed from the rack after preserving qualification evidence locally.

It may be reconsidered after upstream improvements using the same qualification process.

## Initial routing policy

```text
local-primary:
  preferred_harness = jcode
  classification = qualified

local-coder:
  preferred_harness = jcode
  tool_profile = minimal
  context_window = 16368
  max_num_seqs = 1
  classification = qualified_with_constraints

fallback_harness:
  none
```

Routing is based on measured worker/model/task capability evidence, not raw GPU size or model parameter count.

## Multi-GPU scheduling boundary

PR14 does not implement intelligent task placement between GPUs.

A later routing/scheduling PR may allow Rack AI to choose dynamically between the current workers and use both concurrently for independent work.

That future capability remains a Rack AI control-plane responsibility rather than a JCode swarm responsibility.

## PR15 implementation contract

The next production integration PR should replace the bespoke model-facing coding loop with the qualified JCode route while preserving Rack AI control.

Conceptually:

```text
Rack AI
  -> select worker/model/task
  -> invoke JCode with approved provider/model/tool profile
  -> enforce outer workspace/network/process/time policy
  -> independently run acceptance/review
  -> own Git/promotion/evidence
```

The integration PR must not:

- add Abacus;
- add a JCode swarm dependency;
- give JCode remote Git promotion authority;
- move acceptance ownership into JCode;
- introduce another bespoke Rack AI coding-agent framework without new evidence.

## Merge gate

PR14 is merge-ready when:

- the final qualification report reflects the current Qwen3.5/JCode evidence;
- truthful no-change is proven;
- network-disabled operation is proven;
- simultaneous dual-endpoint isolation is proven;
- the final routing policy selects JCode and records the local-coder constraints;
- Abacus is explicitly recorded as not qualified;
- PR15's architectural boundary is unambiguous.

These conditions are now satisfied by the recorded qualification evidence.

## Machine-readable contract summary

```text
QUALIFIED_HARNESSES = jcode
LOCAL_PRIMARY_PREFERRED_HARNESS = jcode
LOCAL_CODER_PREFERRED_HARNESS = jcode
LOCAL_CODER_JCODE_TOOL_PROFILE = minimal
LOCAL_CODER_CONTEXT_WINDOW = 16368
LOCAL_CODER_MAX_NUM_SEQS = 1
FALLBACK_HARNESS = none
ABACUS = not_qualified
JCODE_SWARM = deferred
CONTEXT_GOVERNOR_REQUIRED_FOR_PR15 = false
PR15_READY = true
```
