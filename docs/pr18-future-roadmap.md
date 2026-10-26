# PR18 — Living Future Roadmap and Decision Register

## Status

**Living roadmap / decision register. Not an implementation contract.**

This PR exists so future Rack AI capabilities, unresolved architectural questions and deferred ideas have one durable place to live after the 2026-08-23 architecture reset.

PR18 is expected to stay open and be updated over time.

It is deliberately different from PR14–PR17:

- PR14–PR17 describe the active near-term sequence;
- PR18 records what may come after PR17, why it matters, what evidence would justify it, and where the ownership boundary must be decided before implementation.

A capability appearing here does **not** mean it is approved for implementation.

## Current strategic line

Rack AI is a Rust control plane above coding harnesses and inference backends.

The standing boundary is:

### Rack AI owns

- durable campaign/task/run state;
- GPU/model/resource registry and placement;
- worker/harness selection;
- target-repository registration and worktree lifecycle;
- rootless isolation, network policy and process bounds;
- deterministic acceptance;
- independent review;
- no-change / invalid-result rejection;
- recovery/replanning/fallback decisions above the coding harness;
- evidence/audit trail;
- Git/path authority and promotion policy;
- operator-visible state, pause/cancel/recovery.

### Selected Rust coding harness owns

- model-facing coding-agent loop;
- repository navigation/search used during implementation;
- source-code editing/patching mechanics;
- harness-local context management;
- implementation-time tool selection;
- implementation-time compiler/test feedback loops where supported;
- coding-agent-specific parsing/workarounds for open-weight model output.

### vLLM owns

- local inference serving;
- model execution/runtime concerns.

### Permanent architectural rule

Rack AI should not implement general-purpose coding-agent functionality already adequately provided by the selected Rust harness.

When a capability is missing, the preferred order is:

1. configure/use the selected harness;
2. use an existing harness extension/plugin/upstream capability;
3. contribute/fix upstream where reasonable;
4. only then consider a narrow Rack AI-owned implementation, and only if the capability genuinely belongs at the control-plane boundary.

## Active sequence before this roadmap can be promoted

1. **PR14** — qualify JCode direct mode versus Abacus and select one Rust coding harness.
2. **PR15** — integrate that harness behind a small Rack AI adapter while preserving Rack AI gates.
3. **PR16** — remove/simplify obsolete Rack AI-native coding-harness code.
4. **PR17** — prove substantive unattended local software development through the new architecture.

Unless a safety-critical defect requires otherwise, the items below should not become active implementation PRs before PR17 has produced evidence.

---

# Future capability register

## A. Objective-to-campaign planning

### What it means

Today Rack AI can execute bounded/predeclared work. Objective planning would allow an operator to provide a higher-level software objective such as:

> Add exportable accessibility findings to this application without breaking existing CLI behaviour.

The system would investigate the target, propose bounded work units, dependencies, allowed scopes and acceptance checks, then execute an approved/validated plan.

This is distinct from ordinary coding-agent planning inside one implementation task. It is **control-plane planning across tasks/campaign steps**.

### Why defer until after PR17

We first need to prove that Rack AI can reliably supervise a selected coding harness when the work is already bounded. Otherwise adding autonomous campaign generation merely adds another source of uncertainty on top of an unproven execution path.

PR17 provides the evidence needed to decide whether the remaining limitation is genuinely campaign planning rather than coding execution/recovery.

### Ownership decision after PR17

Ask:

**Does the selected harness already provide high-level planning only inside one coding session, or does it provide durable multi-task planning with authority semantics suitable for the rack?**

Likely split:

- harness owns implementation planning inside an assigned task;
- Rack AI owns campaign-level decomposition, dependencies, resource placement and authority validation.

If the harness already exposes useful plan proposals, Rack AI should consume/validate them rather than recreate its planner.

### Decision line

Implement in Rack AI only when the desired plan crosses one or more control-plane boundaries such as:

- multiple separately accepted tasks;
- different workers/models/GPUs;
- different path scopes or authority envelopes;
- durable pause/resume/revision;
- dependencies between work units;
- operator-approved acceptance/promotion policy.

Do **not** implement Rack AI planning merely to duplicate a harness's internal TODO list or coding plan.

### Safety principle

The model may propose a plan; Rack AI authorises it. Planning must never silently widen repository scope, paths, network access, credentials, runtime, promotion authority or objective.

---

## B. Recovery escalation and capability exhaustion

### What it means

PR7 already provides failure diagnosis and bounded replan/fallback concepts. A future escalation layer would explicitly distinguish situations such as:

- retryable implementation defect;
- strategy failure;
- stronger local model required;
- selected harness capability failure;
- insufficient authority;
- exhausted local capability;
- operator decision required;
- external/frontier expertise required;
- terminal failure.

### Why defer until after PR17

The selected harness will change the failure surface dramatically. We should first observe real harness-backed failures before designing a comprehensive escalation taxonomy around the old native worker behaviour.

### Ownership boundary

Harness-local repair (for example fixing a compiler error inside the current assignment) should remain in the harness where it is effective.

Rack AI owns escalation when the decision affects:

- switching models/GPUs/workers;
- restarting/replacing a harness session;
- changing campaign strategy;
- consuming attempt/resource budget;
- requesting operator/frontier assistance;
- declaring insufficient authority;
- terminating/pausing the campaign.

### Decision line

If the next action changes **who/what executes the task or what campaign-level policy applies**, it belongs in Rack AI.

If it is simply another bounded coding attempt inside the same assigned task/authority, prefer the harness.

### Non-negotiable rule

Escalation must not automatically broaden authority.

---

## C. Technical research / web access

### What it means

Allow the system to obtain current technical information when repository-local evidence is genuinely insufficient: library documentation, upstream bugs, release notes, compiler behaviour, etc.

Earlier ideas included a locally hosted SearXNG/search broker and bounded source fetching.

### Why defer

A coding harness may already offer search/fetch/MCP capabilities. We should not build a parallel Rack AI research stack before understanding what the selected harness can safely provide.

### Ownership decision

Three possible outcomes after PR17:

1. **Harness-owned research** — acceptable if it can be strongly bounded, isolated from mutation authority, audited, and disabled by Rack AI policy.
2. **Rack AI research broker** — appropriate if research needs separate network/security authority from mutation workers, provenance capture, shared caching or cross-task policy.
3. **No autonomous research yet** — if local tasks do not justify the added attack surface.

### Decision line

Research belongs in Rack AI when network access itself is a **control-plane privilege** that must be granted independently of the coding worker and recorded across campaigns.

Do not give a mutation container general internet access merely because the harness supports web tools.

### Security principles

- fetched content is untrusted data, not instructions;
- no credentials exposed to models;
- block metadata/private-network targets;
- bounded source count/size/time/redirects;
- provenance and retrieval time retained;
- research evidence cannot expand mutation authority.

---

## D. Adaptive multi-worker / multi-GPU scheduling

### What it means

Exploit multiple GPUs/models and potentially multiple harness sessions concurrently, including future hardware additions.

Possible behaviours:

- select workers based on model capability and VRAM;
- run independent tasks concurrently;
- respect dependency DAGs;
- avoid conflicting write scopes;
- isolate branches/worktrees;
- integrate accepted changes in a controlled order;
- reassign failed work to another model;
- preserve sequential operation when concurrency is unnecessary.

### Why defer

First prove one harness-backed worker path end-to-end. Concurrency multiplies debugging and integration complexity and should not mask basic worker reliability.

### Ownership boundary

Rack AI clearly owns **resource and campaign scheduling** because it knows the physical rack, GPU occupancy, task dependencies and acceptance state.

The harness may own internal subagents or helper sessions. Rack AI should not recreate those unless it needs cross-resource/cross-authority orchestration.

### Decision line

Use harness subagents when they operate as an implementation detail inside one Rack AI assignment and one resource/authority envelope.

Use Rack AI scheduling when work must be placed across:

- different GPUs/models;
- separately accepted tasks;
- independent worktrees/branches;
- different resource leases;
- dependency or integration boundaries.

### Future hardware

The scheduler should remain registry-driven so a third GPU or later 3090/other accelerator can add capacity without rewriting orchestration.

---

## E. Semantic code intelligence

### What it means

Definition/reference lookup, symbols, hover/type information, diagnostics, implementations and related semantic navigation.

This was previously PR8 as a Rack AI-owned rust-analyzer backend.

### New strategy

Default assumption: semantic code intelligence is coding-harness functionality.

Rack AI should consume semantic evidence when useful for review/recovery, but should not automatically own the language-server client/editor integration.

### Decision line

Only add a Rack AI semantic abstraction if, after harness integration, Rack AI itself needs language-semantic evidence for a **control-plane decision** that the harness cannot provide in a structured/reusable way.

Example potentially valid Rack AI use:

- independent recovery evidence proving an out-of-scope caller exists before selecting a campaign-level strategy.

Invalid reason to rebuild it:

- the coding worker wants nicer go-to-definition navigation.

That belongs in the harness.

### Language neutrality

Whatever architecture is chosen should not make Rack AI fundamentally Rust-only as a software-development target. The trusted control software/harness can be Rust-native while developing Python, JavaScript/TypeScript, Go, Java, C#, etc. through appropriate target toolchains.

---

## F. Self-development / Rack AI improving Rack AI

### What it means

Use Rack AI to implement changes to the Rack AI repository itself.

### Permanent safety boundary

The executing `/srv/rack-ai` checkout must never be the mutation target of its own active controller.

Self-development uses a separate registered clone/worktree under the same rules as any external target.

### Why revisit after PR17

PR17 proves ordinary unattended development first. Once that works, self-development is primarily a target-selection/promotion problem rather than a reason to weaken the safety model.

### Ownership boundary

The selected harness performs implementation in the separate clone.

Rack AI owns:

- campaign planning/selection;
- isolated target clone;
- acceptance/review;
- evidence;
- promotion decision.

### Decision line

Do not add a special self-modification execution path. If self-development cannot use the ordinary external-repository path, treat that as an architecture smell and review why.

---

## G. Remote GitHub promotion / pull-request automation

### What it means

After local acceptance, optionally push a branch, open/update a PR, attach evidence, or request review.

### Why defer

Current safety benefits from no remote credentials in mutation workers and manual promotion. We should first prove the local development loop.

### Ownership boundary

This is Rack AI/operator integration, not coding-harness authority.

The harness should not receive GitHub credentials simply to implement code.

### Decision line

Consider when local unattended qualification is reliable and remote promotion becomes the dominant manual burden.

Potential staged model:

1. local commit only;
2. operator-approved push/PR;
3. policy-approved automatic PR creation;
4. automatic merge, if ever, is a separate much higher bar.

### Safety principle

Implementation success and promotion authority remain separate decisions.

---

## H. Operator interfaces

### Candidates

- richer CLI/status/events;
- Telegram/chat control;
- local web UI/dashboard;
- API for outside machines;
- notifications for blocked/completed work.

### Why defer substantial UI work

Backend reliability and architecture should stabilize first. UI must not become another orchestration layer with duplicated state.

### Ownership boundary

Rack AI owns a stable control API/command surface. Interfaces are clients of that surface.

### Decision line

Add an interface when it reduces real operational burden without introducing a second source of truth.

Telegram or a web UI should call Rack AI controls; they should not directly drive harness/model sessions.

---

## I. Long-horizon campaigns

### What it means

Runs lasting many hours/days with multiple accepted checkpoints, restart recovery, bounded storage, liveness, pause/cancel and resource changes.

### Existing foundation

Much of the durable campaign/supervision work from merged PR3/PR4/PR6 remains relevant.

### Decision after PR17

Determine whether PR17 already demonstrates enough durability or whether a dedicated soak/long-horizon qualification is needed.

### Ownership boundary

Rack AI owns long-horizon campaign durability. The harness should be treated as replaceable/terminable execution sessions rather than the authoritative long-lived state store.

---

## J. Additional model roles and future GPUs

### Possible roles

- primary implementer;
- stronger fallback implementer;
- planner/recovery reasoner;
- independent reviewer;
- test/debug specialist;
- future vision/audio/non-coding workers.

### Decision principle

Add roles based on measured capability and resource economics, not because more agents sound desirable.

The registry should describe model capability, endpoint, GPU/resource, concurrency and cost/latency characteristics.

### Harness boundary

Rack AI chooses the role/model/resource.

The selected harness runs the coding session against the chosen endpoint.

Avoid harness-specific hard-coding of physical GPU topology.

---

## K. Coding model replacement / specialization

### Current concern

The small `local-coder` has shown weak tool-call compliance. A selected harness may mitigate this through textual tool-call parsing and better agent-computer interaction, but it may still be too weak for some tasks.

### Decision after PR14/PR17

Separate two questions:

1. Is the harness giving the model an appropriate interface?
2. Is the model itself capable enough even with a good harness?

Only replace the model after the harness is proven not to be the primary failure source.

### Future options

Continue evaluating small coding-specialist models that fit the available GPU with sufficient context and reliable local serving.

Do not make a model swap an architectural rewrite; model identity belongs in registry/configuration.

---

## L. Automatic frontier/cloud expertise

### What it means

Use a paid/frontier model automatically when local capability is exhausted.

### Current position

Not part of the near-term architecture. Local-first operation and minimal cloud token burn remain important.

### Decision line

Only consider after local escalation is well understood and there is a clear policy for:

- when local capability is genuinely exhausted;
- maximum spend/token budget;
- what evidence is sent externally;
- privacy/source constraints;
- whether the frontier model may implement or only advise;
- independent verification after its work.

A compact evidence-based handoff packet is preferable to sending an entire uncontrolled history.

Automatic authority expansion remains prohibited.

---

## M. Non-coding rack workloads

### Long-term context

Rack AI was originally conceived as the broader local GPU control plane, not merely a software-development product. Future rack services may include vision, speech, audio, image/video generation or other local AI workloads.

### Why this matters to the architecture

The selected coding harness is **one execution backend**, not Rack AI itself.

Rack AI's resource/model/service registry and scheduling concepts should remain general enough that later non-coding backends can coexist without forcing them through the coding harness.

### Decision line

Do not generalize prematurely while PR14–PR17 are focused on coding reliability. But do not collapse Rack AI's domain model into assumptions that every worker is a coding harness session.

---

## N. Harness upstream evolution / replacement

### Principle

The selected harness is a dependency, not a permanent religion.

Rack AI should track upstream improvements and keep the adapter narrow enough to replace the harness later if another Rust-native option materially outperforms it.

### JCode-specific future question

If JCode is selected or remains relevant, periodically retest whether swarm provider/model rebinding has been fixed. If upstream swarm eventually becomes reliable, decide whether any Rack AI orchestration can be simplified without giving up control-plane responsibilities.

### Abacus-specific future question

If Abacus is selected, monitor maturity, release stability and compatibility with the rack's local model families.

### Decision line

Replace the harness only on measured evidence. Avoid multi-harness production complexity unless there is a genuinely distinct workload requiring it.

---

# Deferred ideas that are not currently commitments

These may be added/refined as evidence accumulates:

- richer automated code-quality/static-analysis gates;
- test-generation specialist workers;
- per-language toolchain images/caches for fully offline builds;
- reusable build environments for Python/npm/Go/etc.;
- campaign-level artifact/cache management;
- smarter disk/evidence retention;
- capability benchmarking of local models;
- operator-defined cost/energy/performance scheduling;
- hardware-aware batch scheduling across future GPUs;
- controlled notifications/alerts;
- local documentation/RAG if repository/search evidence later proves insufficient;
- structured project memory, but only if durable repository/campaign state is insufficient and the ownership/privacy model is clear.

No item in this section should become implementation merely because it appears here.

---

# Decision process for promoting a PR18 item

Before turning any future item into an implementation PR, answer:

1. **What observed problem are we solving?**
2. **Did PR17 or later real evidence demonstrate the problem?**
3. **Does the selected Rust harness already solve it?**
4. **Could an upstream harness contribution/configuration solve it cleanly?**
5. **Is it fundamentally a Rack AI control-plane responsibility?**
6. **What authority/security boundary changes?**
7. **What deterministic qualification proves success?**
8. **Can implementation reduce or avoid custom code rather than adding another parallel subsystem?**

Only then create a new numbered implementation/qualification PR.

## Ownership heuristic

A useful rule of thumb:

- **Inside one coding task, one workspace and one authority envelope:** prefer the harness.
- **Across tasks, workers, GPUs, authority, acceptance, durable state or promotion:** Rack AI.
- **Inference/runtime:** vLLM/provider layer.
- **Human-facing UX:** client of Rack AI, not a parallel controller.

---

# Architecture reset historical note — 2026-08-23

PR8–PR13 were closed as superseded, not because every capability was unwanted, but because they were based on an increasingly incorrect assumption that Rack AI should grow its own coding-agent harness.

Useful concepts from those PRs are preserved in this roadmap:

- PR8 semantic intelligence -> section E;
- PR9 unattended qualification -> replaced by PR17;
- PR10 escalation -> section B;
- PR11 technical research -> section C;
- PR12 adaptive scheduling -> section D;
- PR13 objective planning -> section A.

Merged PR2–PR7 remain part of repository history. Their Rack AI-owned control/safety concepts should be retained where they still belong at the supervisory boundary; coding-agent mechanics may be removed during PR16 once the selected harness has replaced them.

The architectural reset is therefore a simplification, not a rejection of the entire body of work.

## Update rule for this document

Whenever a future idea becomes material in discussion, add it here with:

- what capability means;
- why it may be useful;
- evidence/trigger for revisiting it;
- likely ownership boundary;
- the explicit decision line between Rack AI, selected harness, inference runtime or operator/client;
- safety/qualification implications.

When an item becomes an active implementation PR, update this document to point at that PR and record the decision that promoted it.