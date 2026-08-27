# PR23 — ATHBA Development Handoff and Rack AI Project Context

> **Purpose of this document**
>
> This is the durable context handoff for the point at which active development moves from Rack AI into ATHBA. It is intentionally much more detailed than a normal design note. The goal is that someone returning to this work after weeks or months can understand not merely *what* the current architecture is, but *why* it exists, what was tried, what failed, what was learned, what must not be accidentally undone, and exactly what PR23 is intended to prove.
>
> Treat this document as project memory. Update it when PR23 changes an important architectural assumption.

## 1. Executive summary

We are building a local AI compute rack and a family of applications that use it. Rack AI is **not** intended to become a giant application that understands every domain. It is the rack-wide resource/control plane. Domain applications remain responsible for their own semantics and user experience.

For application development, the domain application is **ATHBA — App That Builds Apps**. ATHBA is responsible for understanding a software project, designing it, decomposing work, managing dependencies and progress, and producing very small implementation work units. Rack AI receives those work units, decides which available rack resource/model should execute them, runs them through the JCode agent harness inside the existing safety boundary, independently verifies the result, and returns structured evidence.

The immediate objective is **not** to build the perfect scheduler or the complete future ATHBA. The objective is to get a minimum working product running as quickly as possible:

```text
ATHBA
  -> tiny bounded work unit
  -> Rack AI
  -> internal resource/model selection
  -> JCode
  -> local model
  -> isolated source change
  -> Rack AI independent acceptance/evidence
  -> structured result
  -> ATHBA decides the next unit
```

PR22 implemented the Rack AI side of this boundary. PR23 must implement the minimum ATHBA side and prove a real vertical slice, using Tiny Ticket as the productivity benchmark.

The governing philosophy is:

**Move fast above the safety boundary. Keep the proven containment, authority, evidence and fail-closed mechanisms conservative.**

---

## 2. Why this project exists

The GPU rack began as a way to make useful local AI compute available without continually consuming paid cloud/agent tokens. It now needs to support multiple classes of work, including:

- application/software development;
- image/video generation through applications such as ComfyUI;
- eventually audio/music workloads;
- future human-facing orchestration through OddesyAgent or other clients;
- potentially additional local AI applications over time.

The rack therefore needs a component with final authority over scarce physical resources. A domain application must not simply decide that it owns a particular GPU because it would like to run something. Rack AI should understand the broad workload demand on the machine and decide what runs where and when.

At the same time, Rack AI must not absorb every domain. If image generation is being performed with ComfyUI, ComfyUI remains the image-generation application and its UI remains useful. Likewise, application development should have its own application rather than hiding the entire development experience beneath Rack AI.

That application is ATHBA.

---

## 3. Current physical/runtime context

At the point this document was written, the important rack roles are approximately:

- **RTX 4060 Ti 16 GB** — stronger local reasoning/coordination model role (`local-primary`).
- **RTX 2060 6 GB** — smaller coding/implementation model role (`local-coder`).
- Ubuntu Server host, conventionally referred to as `gpurack`.
- Rack AI source/control repository at `/srv/rack-ai` on the rack.
- vLLM is the preferred model-serving layer rather than Ollama.
- JCode is the preferred model-facing coding/agent harness.

Current model identities and exact GPU inventory are implementation/configuration details, **not part of the external ATHBA contract**. Hardware will change. More GPUs may be added. Models will certainly change. The architecture deliberately prevents ATHBA from coupling itself to today's topology.

The current registry can internally map a small implementation unit to the smaller implementer and a larger/context-heavy unit to the stronger worker. That is intentionally primitive MVP policy and is expected to evolve later.

---

## 4. The original Rack AI application-development approach

Before PR22, Rack AI had developed into a capable local orchestration/control layer. It could take a relatively substantial software task and supervise a planner/implementer/verifier style workflow across local models.

Important pieces built during that period included:

- explicit task contracts;
- isolated Git worktrees;
- worker selection and role configuration;
- direct JCode-backed workers;
- vLLM-backed local model roles;
- allowed-path enforcement;
- deterministic acceptance commands;
- Git evidence capture;
- worker transcripts;
- retry/repair/fallback behaviour;
- leases and recovery;
- structured campaign state and events;
- independent coordinator review;
- fail-closed handling of unsafe or invalid output;
- detection of no-change and tool/protocol problems.

This work was valuable. The mistake was not primarily the safety/control plane. The problem was the *granularity and ownership of software-development reasoning*.

Rack AI was effectively being asked to give small local models tasks that were still too large and semantically rich, then supervise them until they succeeded or correctly failed.

---

## 5. PR17 qualification and the critical lesson

PR17 was the major unattended qualification exercise. The test application was deliberately small: **Tiny Ticket**, a tiny ticketing application with domain, persistence and CLI behaviour.

The qualification repeatedly exposed model failures. Examples included:

- syntax and compile errors;
- incorrect persistence implementations;
- failure to honour a supplied store path;
- no-change attempts;
- malformed tool usage;
- timeouts;
- edits outside required paths;
- generated runtime side effects such as a repository-root `store.txt` when only `src/` was allowed.

A representative terminal CLI failure was especially informative. The fallback worker implemented CLI persistence using a hard-coded/default `store.txt` rather than the `<store>` path supplied by the command. The acceptance script exercised the generated binary, which then created `store.txt` at repository root. Rack AI detected that mutation outside `allowed_paths` and rejected the attempt with `path_policy_failed`.

That was **correct behaviour**.

The final PR17 conclusion was intentionally recorded as:

```text
PR17_QUALIFICATION = FAIL
```

This does **not** mean Rack AI as a control plane failed. Quite the opposite: the exercise demonstrated that it could detect bad implementation behaviour, reject it, preserve evidence and fail closed rather than pretending that passing-looking output was success.

But there was an equally important product-level conclusion:

> A system that safely rejects everything is not yet a useful application-development product.

Tiny Ticket is small. If the system cannot reliably produce it, then despite having a good control plane, the application-development workflow is not productive enough.

The evidence suggested that the limiting factor was largely the cognitive size of the implementation steps given to the small local models, not an absence of safety mechanisms.

Therefore we explicitly chose **not** to solve the problem by weakening the safeguards.

---

## 6. What must not be undone

The PR17/PR21 work produced a valuable safety boundary. Future work must not casually remove it merely to increase apparent model success.

Preserve or build upon:

- repository registration/resolution;
- isolated worktree execution;
- allowed writable path policy;
- post-run Git inspection;
- bounded worker authority;
- timeouts;
- controlled network policy;
- deterministic acceptance controlled by the orchestration layer, not invented by the worker;
- evidence packets;
- worker transcripts/tool evidence;
- no-change detection;
- rejection of unauthorized changes;
- fail-closed behaviour;
- recovery/lease concepts where applicable;
- JCode as the preferred coding-agent harness rather than inventing another model tool loop.

The desired productivity improvement should come from **better decomposition and better allocation of intelligence**, not from making the guardrails porous.

---

## 7. The architectural realization

The central design realization was that Rack AI and ATHBA have different jobs.

### Rack AI is the resource/control plane

Rack AI should know things such as:

- what physical resources exist;
- which GPUs are occupied or available;
- which model/worker profiles are available;
- which broad workloads are underway;
- what resource/capability a submitted work unit needs;
- which worker/model/harness should receive it;
- whether a resource should be reclaimed or reconfigured in future;
- whether execution stayed inside policy;
- whether acceptance passed;
- where the evidence lives.

Rack AI has final authority over the rack.

### ATHBA is the software-development application

ATHBA should know things such as:

- what application is being built;
- the user's requested product behaviour;
- software architecture;
- source/repository structure;
- features and requirements;
- dependencies between pieces of work;
- what tests should exist;
- how a feature should be decomposed;
- whether work should be split further;
- project progress;
- what the next software-development ticket should be;
- how a failed implementation result changes the development plan.

Rack AI should not need to understand `TicketStore`, Django models, React components, business rules, or the meaning of a product feature.

ATHBA should not need to understand that the current small worker happens to live on an RTX 2060 or that the strong worker happens to be Gemma on an RTX 4060 Ti.

---

## 8. JCode's role

JCode remains important.

The intended layering is not:

```text
ATHBA -> raw LLM API
```

and it is not:

```text
Rack AI -> home-grown replacement for JCode
```

Instead:

```text
ATHBA -> Rack AI -> JCode -> selected local model
```

JCode is the agent/tool harness that lets the selected model inspect and modify the bounded repository workspace. Rack AI controls the environment and authority around that execution.

This keeps model-facing agent behaviour replaceable/upgradable without embedding another agent framework into either ATHBA or Rack AI.

---

## 9. The small-ticket hypothesis

The most important hypothesis for the next development phase is:

> Small local models can become useful software-development workers if the architectural/decomposition intelligence is moved upward and the implementation unit is made sufficiently small, explicit and mechanically verifiable.

Instead of asking a 3B-ish coding worker:

> Add dependency-free persistence to Tiny Ticket, preserve IDs, implement deterministic next-ID behaviour, wire it into the library and satisfy the persistence tests.

ATHBA should be capable of producing much smaller units such as:

> Implement parsing of one valid persisted ticket line into a `Ticket`.

or:

> Implement `TicketStore::save(path)` for the existing store representation. Only modify `src/store.rs`. Acceptance is `cargo test save_single_open_ticket`.

The local model then becomes closer to a **ticket completion machine** than a miniature autonomous software architect.

This is a deliberate way to exploit inexpensive/small local models rather than expecting them to reproduce the performance profile of large cloud coding agents.

---

## 10. TDD direction

A strong candidate for ATHBA's development method is very small-grained test-driven development inspired by the classic red/green discipline:

1. a failing test expresses the next required behaviour;
2. the implementation worker receives only the bounded task needed to make that test pass;
3. it should not be asked to implement speculative additional behaviour;
4. Rack AI independently executes the acceptance test;
5. ATHBA consumes the result and chooses the next development unit.

This is attractive for small models because it narrows the search space and gives an unambiguous stopping condition.

However, **do not turn Rack AI into a TDD framework**. TDD is a software-development strategy and therefore belongs primarily to ATHBA.

Also do not make PR23 unnecessarily enormous by trying to perfect a universal TDD engine before proving the vertical slice. The initial objective is to test whether fine-grained decomposition materially improves productivity.

---

## 11. Pool-of-work direction

Longer term, ATHBA should be able to maintain a pool/DAG of ready development work rather than one giant sequential campaign.

Conceptually:

```text
Application objective
        |
        v
ATHBA architecture/decomposition
        |
        +--> unit A ----+
        +--> unit B ----+--> ready work pool / dependency DAG
        +--> unit C ----+
        +--> unit D (depends on A+B)
                         |
                         v
                       Rack AI
                         |
             +-----------+-----------+
             |                       |
         worker/GPU 1            worker/GPU 2
```

This matters as the rack grows. Multiple GPUs should eventually be capable of taking independent ready work units concurrently.

But the scheduler remains Rack AI's authority. ATHBA may say that units A and B are independent and ready; it must not seize GPUs for them.

Sophisticated parallel scheduling is **not required to prove PR23**.

---

## 12. Workload awareness beyond one ticket

Rack AI should not see every submitted unit as an unrelated one-off request.

ATHBA may be building a substantial application over several days or weeks and may submit dozens or hundreds of work units. Rack AI should therefore receive a stable workload/project identity and coarse context that tells it these units belong to a continuing workload.

The reason is future rack optimisation. For example, Rack AI may eventually know that:

- ATHBA has a large application build with significant queued demand;
- a music-video generation workload is expected to dominate a GPU for a day;
- ComfyUI has interactive priority for a period;
- several low-complexity coding units are ready and can be scheduled around larger jobs.

No individual application gets to override Rack AI's global authority.

PR22 intentionally implemented only the minimal workload identity/context necessary to establish this boundary. Advanced forecasting, fairness, preemption and optimisation belong later.

---

## 13. ComfyUI and other domain applications

The architecture should be consistent across domains.

ComfyUI is useful as an application in its own right. If the user wants to generate images/video, Rack AI should eventually be able to configure/allocate the required resources, but the user should still be able to use ComfyUI's own application/front end.

Similarly:

- ATHBA is the application-development domain application;
- ComfyUI is an image/video-generation domain application;
- future audio/music tools may remain their own applications;
- OddesyAgent may later become a primary human-facing conversational/interface client that can request work from Rack AI and/or domain applications.

Rack AI is the common resource authority beneath these applications, not a replacement UI for all of them.

---

## 14. Why we deliberately rejected a big-bang scheduler build

There is a large amount of attractive future Rack AI functionality we could build:

- dynamic model loading/unloading;
- sophisticated GPU packing;
- workload forecasting;
- priorities and preemption;
- capacity planning;
- fairness;
- model benchmarking and learned routing;
- automatic resource reconfiguration for ComfyUI;
- multi-GPU strategies;
- queue optimisation;
- long-running workload reservations.

We explicitly chose **not** to build all of that before returning to ATHBA.

The reason is product discipline: we want a live working vertical slice quickly. Building an elaborate rack manager only to discover later that the ATHBA integration requires different abstractions would be wasteful.

The roadmap therefore favours incremental end-to-end capability:

1. establish the minimal Rack AI/ATHBA contract;
2. make ATHBA actually build something through it;
3. add minimal resource switching with another domain such as ComfyUI;
4. improve the scheduler/resource manager iteratively from real workload evidence.

---

## 15. PR21 and PR22 state

### PR21

PR21 carried forward the useful hardening from the PR17 qualification era into `main`. It preserved the control-plane improvements while allowing PR17 itself to be closed with an honest qualification failure rather than pretending the productivity benchmark had passed.

### PR22

PR22, **Rack AI MVP architecture and workload contract**, was merged into `main` immediately before this document was created.

PR22 implemented `rack-ai/work-unit/v1` and the Rack AI-side execution boundary.

Important implementation areas include:

- `crates/rack_ai_domain/src/workload_id.rs`
- `crates/rack_ai_domain/src/workload_kind.rs`
- `crates/rack_ai_domain/src/work_unit_id.rs`
- `crates/rack_ai_domain/src/work_unit_capability.rs`
- `crates/rack_ai_domain/src/work_unit_complexity.rs`
- `crates/rack_ai_application/src/work_unit_request_document.rs`
- `crates/rack_ai_application/src/work_unit_request.rs`
- `crates/rack_ai_application/src/execute_work_unit.rs`
- `crates/rack_ai_infrastructure/src/registry_work_unit_worker_selector.rs`
- `crates/rack_ai_cli/src/work_unit_command.rs`
- `docs/work-unit-contract.md`
- `docs/rack-ai-mvp-architecture.md`

PR22 deliberately reuses `ExecuteChange` and the qualified JCode path rather than creating a parallel implementation engine.

---

## 16. The PR22 work-unit contract

The external contract version is:

```text
rack-ai/work-unit/v1
```

The caller supplies concepts including:

- `workload.id`
- `workload.kind`
- repository identity and base ref/revision
- `work_unit.id`
- exact bounded objective
- allowed writable paths
- deterministic acceptance commands
- required artifacts where appropriate
- readiness/dependency information
- capability
- complexity
- large-context hint
- bounded implementation attempts
- timeout
- network policy

The external caller does **not** supply:

- physical GPU ID;
- Rack AI worker ID;
- model ID.

Unknown fields are deliberately rejected in the request schema, which helps prevent clients from smuggling resource-selection authority into the contract.

Current MVP workload kind is `application-development`; current capability is `implementation`.

---

## 17. Current Rack AI internal selection policy

PR22 added a deliberately simple and replaceable internal worker selector.

Conceptually:

- small bounded implementation work prefers a minimal implementer;
- medium/large work or `requires_large_context=true` prefers a stronger non-minimal worker;
- selection is constrained to enabled JCode workers with active model bindings.

On the current rack this generally maps small work to `local-coder` and stronger/context-heavy work to `local-primary`.

This is **not a permanent scheduling algorithm**. It is enough to prove that ATHBA describes the work while Rack AI retains execution placement authority.

---

## 18. PR22 execution flow

The implemented flow is approximately:

```text
WorkUnitRequestDocument
        |
        v
WorkUnitRequest validation
        |
        v
WorkUnitWorkerSelector
        |
        v
internal ImplementWorkerRuntime selection
        |
        v
translate to existing ChangeRequestDocument
        |
        v
ExecuteChange
        |
        +--> registered repository resolution
        +--> isolated worktree
        +--> JCode implementer
        +--> allowed-path policy
        +--> deterministic acceptance
        +--> review/evidence packet
        |
        v
ExecuteWorkUnitResult
```

The result exposes enough information for a caller such as ATHBA to understand the outcome and find evidence, including workload/work-unit/change identity, selected worker, placement, status, acceptance verdict, branch/worktree and packet path.

---

## 19. PR23 — what we are doing now

PR23 is the transition into **ATHBA development mode**.

ATHBA has not been actively developed for roughly six months. Do not assume its current implementation already matches this architecture. Begin by auditing it as an existing product/codebase: understand what it currently does, what concepts remain valuable, and what should be adapted rather than blindly rewritten.

The target of PR23 is a **minimum real application-development vertical slice** between ATHBA and Rack AI.

The core question is:

> Can ATHBA take a small application objective, decompose it into sufficiently small bounded units, submit those units through `rack-ai/work-unit/v1`, consume the results, and thereby get useful implementation work from the local rack models without weakening Rack AI's safety boundary?

Tiny Ticket remains the preferred benchmark because it is small enough to understand and we already possess useful evidence about where the previous coarse-grained approach failed.

---

## 20. PR23 desired responsibilities

### ATHBA should minimally gain the ability to

- represent a development project/workload;
- understand/retain the target application's objective and architecture at the appropriate level;
- inspect or reason about the target repository;
- represent small development work units;
- express dependencies/readiness;
- assign complexity/capability requirements without choosing hardware;
- define deterministic acceptance for a unit;
- serialize/submit a `rack-ai/work-unit/v1` request;
- receive/parse Rack AI's structured result;
- mark the unit accepted/rejected based on authoritative Rack AI output;
- decide what development action happens next;
- preserve enough project history/evidence that a longer application build can continue coherently.

### Rack AI should continue to

- validate the request;
- choose the worker/model/resource;
- run the bounded implementation;
- enforce path and execution policy;
- run acceptance independently;
- preserve evidence;
- return the result.

---

## 21. What PR23 should *not* become

Do not use PR23 as an excuse to build the complete imagined future system.

Avoid, unless absolutely necessary for the vertical slice:

- a universal distributed scheduler;
- sophisticated GPU priority/preemption;
- ComfyUI orchestration;
- image/video/audio workflows;
- OddesyAgent integration;
- a generic all-domain workflow language;
- learned model routing;
- a huge new database architecture;
- cloud services;
- a replacement for JCode;
- application-development semantics inside Rack AI;
- direct GPU/model selection from ATHBA;
- enormous autonomous software tickets;
- weakening acceptance/path policy to get a green result.

Deferred ideas belong in the future-work/PR25 roadmap rather than silently expanding PR23.

---

## 22. Suggested Tiny Ticket strategy for PR23

Do **not** simply reproduce the old campaign steps (`domain`, `persistence`, `cli`) at the same granularity and call that ATHBA integration. That would miss the entire lesson.

Instead, start with an intentionally fine-grained plan. The exact decomposition should be decided after inspecting the fixture/code, but the shape might look more like:

```text
TT-001 create/verify one domain type or constructor behaviour
TT-002 add one validation rule with one focused test
TT-003 add one status transition behaviour
TT-004 parse one persistence record
TT-005 serialize one persistence record
TT-006 load missing store as empty
TT-007 calculate deterministic next id
TT-008 save to supplied path
TT-009 implement one CLI create path
TT-010 implement one CLI list path
TT-011 implement one CLI close path
TT-012 final integration acceptance
```

The point is not those exact tickets. The point is that each implementation worker should receive a task small enough that it does not need to architect the application while coding it.

Where possible, give each unit a narrow deterministic test.

---

## 23. How to think about TDD in PR23

A useful initial workflow is:

```text
ATHBA chooses next behaviour
       |
       v
ensure/create focused failing acceptance test
       |
       v
submit tiny implementation unit to Rack AI
       |
       v
Rack AI selects worker and executes through JCode
       |
       v
Rack AI independently runs focused test
       |
   +---+---+
   |       |
 PASS     FAIL
   |       |
   v       v
advance   ATHBA examines evidence and decides
          retry / refine / split / escalate
```

A crucial distinction: a failed Rack AI result is **information for ATHBA**, not automatically evidence that Rack AI is defective.

ATHBA should eventually be capable of deciding that a failed unit was still too large and splitting it further.

That is far preferable to automatically granting the model broader authority.

---

## 24. Failure handling philosophy

We learned during PR17 that failures need classification.

At a high level distinguish:

- **model/implementation weakness** — worker produced bad code or failed to complete the bounded unit;
- **correct Rack AI rejection** — policy/acceptance correctly caught bad output;
- **task/fixture ambiguity** — ATHBA's unit was underspecified or contradictory;
- **Rack AI product defect** — the control plane itself behaved incorrectly;
- **ambiguous/infrastructure failure** — insufficient evidence or external failure.

PR23 does not need to reproduce the exact old campaign taxonomy mechanically, but ATHBA must not react to every rejection by changing Rack AI.

Likewise Rack AI should not be changed merely because a model is weak.

---

## 25. Escalation and model usage

The rack has a small worker and a stronger worker. The long-term aim is to use both efficiently.

The preferred direction is:

- architect/decompose so that most implementation work can be handled by the smaller/cheaper model;
- use complexity/context hints to let Rack AI choose stronger resources when justified;
- allow future policy to use observed performance and queue pressure;
- do not encode today's worker names into ATHBA.

If a tiny unit repeatedly fails on the small worker, future designs may permit controlled escalation. The exact retry/escalation policy should be evidence-driven and remain Rack AI's execution/resource concern, while ATHBA owns decisions such as splitting or redefining the software task.

---

## 26. Definition of success for the first vertical slice

The first PR23 vertical slice is successful when we can demonstrate something materially like:

1. ATHBA owns a project/workload.
2. ATHBA has a small ready development unit.
3. ATHBA produces a valid `rack-ai/work-unit/v1` request.
4. Rack AI receives it without ATHBA naming a GPU/model/worker.
5. Rack AI internally selects a resource.
6. JCode/local model performs the bounded implementation in isolation.
7. Rack AI independently runs acceptance.
8. Rack AI returns a structured accepted/rejected result and evidence.
9. ATHBA records/consumes that result and advances or replans.
10. Repeating this process produces actual useful application progress.

The stronger productivity proof is that this method can build Tiny Ticket where the previous coarse campaign failed.

That is the experiment we care about.

---

## 27. MVP versus future work

### MVP / immediate

- Rack AI work-unit contract — **done in PR22**.
- Minimal ATHBA integration/client.
- Fine-grained work-unit representation/decomposition.
- Deterministic acceptance/TDD-oriented unit flow.
- Structured result ingestion.
- Tiny Ticket end-to-end productivity proof.
- Preserve Rack AI safety boundary.

### Near-term after the application-development slice

- minimal coexistence/resource switching with ComfyUI;
- prove Rack AI can arbitrate between application development and another real GPU workload;
- improve operational UX around starting/stopping/configuring workloads.

### Later / PR25-style roadmap

- richer ready-work pools;
- concurrent dispatch to multiple GPUs;
- sophisticated priorities;
- preemption;
- long-horizon workload forecasting;
- model loading/unloading;
- capability-aware placement;
- performance history;
- learned/adaptive routing;
- queue optimisation;
- richer rack observability;
- broader domain workload contracts;
- OddesyAgent integration;
- media-generation workload management;
- future hardware expansion and multi-GPU strategies.

---

## 28. Product principles to retain

### 28.1 Rack AI is not the product UI for everything

Domain applications should remain usable and visible. Rack AI manages resources and execution authority beneath them.

### 28.2 Domain semantics stay with domain applications

ATHBA knows software development. ComfyUI knows its generation graph. Rack AI should not become a monolith containing both.

### 28.3 Resource authority stays with Rack AI

No client gets to demand a particular GPU/model by contract.

### 28.4 Small models should receive small jobs

Do not ask a tiny local model to perform architecture, planning, coding and verification in one prompt when a stronger upstream component can narrow the task.

### 28.5 Acceptance is independent

The model does not decide whether its own work passed.

### 28.6 Safety success and product success are different

Correctly rejecting broken code is necessary but insufficient. The system must eventually produce useful output.

### 28.7 Build vertically and iterate

Do not spend months perfecting future scheduling before proving real domain applications can use the rack.

### 28.8 Hardware and models are replaceable

External contracts describe needs, not today's physical topology.

---

## 29. Practical development guidance when opening ATHBA

Because ATHBA has been dormant for months, begin with discovery rather than immediate large-scale rewriting.

Recommended first pass:

1. Read ATHBA's README/docs and inspect its current architecture.
2. Identify existing concepts corresponding to project, task/ticket, agent, execution, tests, repository and progress.
3. Identify what is still useful from the original premise: ATHBA was already conceived as an "app that builds apps", so preserve good domain concepts where possible.
4. Identify old assumptions about cloud models, direct execution, orchestration or resource ownership that conflict with the new Rack AI boundary.
5. Design the thinnest adapter/client for `rack-ai/work-unit/v1`.
6. Avoid rewriting unrelated ATHBA functionality.
7. Add a deterministic integration seam before relying on live models.
8. Then use the real rack for a tiny live work unit.
9. Grow toward Tiny Ticket one behaviour at a time.

Keep this document and `docs/work-unit-contract.md` open while doing that work.

---

## 30. Questions PR23 should answer with evidence

By the end of this phase we want evidence for questions including:

- How small does a unit need to be for the 6 GB worker/model to succeed reliably?
- Does focused TDD materially improve success rate?
- Which failures are fixed by decomposition versus stronger-model escalation?
- How much architectural context must accompany a tiny unit?
- Can ATHBA preserve coherence across many tiny independently executed changes?
- Is the current PR22 contract sufficient, or are there concrete missing fields discovered by real use?
- Does the stronger local model perform well enough when genuinely required?
- Can Rack AI remain domain-agnostic while still providing enough execution evidence?
- What is the throughput cost of extremely small TDD units?
- At what point can independent ready units safely run concurrently?

Do not answer these by speculation if the vertical slice can generate evidence.

---

## 31. Anti-patterns and warning signs

If future work starts doing any of the following, stop and reconsider the boundary:

- ATHBA sends `gpu-2060`, `gpu-4060ti`, `local-coder`, `local-primary`, or a concrete model ID in its normal work-unit request.
- Rack AI begins understanding application feature semantics in order to decide what source code should be written.
- A worker is allowed to modify broader paths because it repeatedly failed the narrow task.
- Acceptance is changed after seeing the worker's output merely to make it pass.
- JCode is bypassed by a second ad-hoc agent loop without a compelling reason.
- PR23 grows into a universal scheduler before one real application is built.
- ATHBA emits large "implement this whole subsystem" tickets to the small model and reproduces PR17.
- A correct rejection is interpreted as a Rack AI defect without evidence.
- resource scheduling becomes owned by whichever domain application shouts loudest.

---

## 32. The intended end-state, in one picture

```text
                         HUMAN / FUTURE ODDESYAGENT
                                   |
                +------------------+------------------+
                |                                     |
                v                                     v
              ATHBA                                ComfyUI
      software-development app              image/video application
                |                                     |
      project / architecture                         |
      decomposition / TDD                            |
      ready work units                               |
                |                                     |
                +------------------+------------------+
                                   |
                                   v
                              RACK AI
                    global rack resource authority
                 workload awareness / placement / policy
                                   |
                 +-----------------+------------------+
                 |                                    |
                 v                                    v
          coding worker(s)                     media workload(s)
             via JCode                           / other harness
                 |
                 v
          vLLM/local models
                 |
                 v
         bounded repository changes
```

The precise set of domain applications and GPUs can evolve. The ownership boundaries should remain understandable.

---

## 33. Immediate next action after this document

Switch active development attention to the **ATHBA repository**.

Audit the current ATHBA codebase and produce a concrete PR23 implementation plan for the ATHBA side of the vertical slice. The plan should map existing ATHBA concepts to the new architecture rather than assuming a greenfield rewrite.

Then implement the smallest end-to-end path that can submit one tiny development unit to Rack AI and consume its result.

After that, begin the Tiny Ticket productivity experiment with deliberately small units.

Do not wait for the full future Rack AI scheduler.

---

## 34. Durable summary

If all other context is lost, remember this:

**Rack AI is the rack-wide resource and safety control plane. ATHBA is the software-development brain/application. JCode is the coding-agent harness. Small local models should be fed tiny, explicit, independently testable work units. PR17 proved the safety plane but failed the productivity benchmark. PR21 preserved the useful hardening. PR22 created the minimal external work-unit boundary without exposing GPU/model identity. PR23 now moves into ATHBA and must prove that better decomposition—likely fine-grained TDD-oriented decomposition—can turn the local rack into a system that actually builds software. We want a working vertical slice quickly, not a perfect scheduler first. Keep the safety boundary; improve productivity above it.**
