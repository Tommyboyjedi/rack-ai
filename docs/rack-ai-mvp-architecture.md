# Rack AI MVP Architecture and Roadmap

## Decision

Rack AI is the rack-wide heterogeneous AI resource control plane. It is not the software-development product, the media-generation UI, or the eventual human-facing assistant.

The immediate goal is a usable vertical slice, not a complete resource scheduler. We will get ATHBA and Rack AI building software together, retain direct use of ComfyUI, and then improve the rack from real operating experience.

## Product boundaries

### Rack AI owns
- global knowledge of rack resources and current/anticipated workloads;
- workload priority and arbitration;
- GPU and model selection;
- model/service lifecycle and resource leases;
- JCode-backed bounded execution where agentic execution is required;
- isolation, timeouts, allowed-path policy, evidence and fail-closed safety;
- switching rack capacity between workloads such as application development and ComfyUI.

Rack AI has final authority over rack resources. Client applications may describe urgency, complexity and demand, but may not dictate or supersede rack-wide scheduling policy.

### ATHBA owns
- application specification and architecture;
- decomposition into small dependency-aware tickets;
- ticket complexity and execution requirements;
- TDD-oriented development contracts and acceptance;
- project progress and development-domain semantics.

ATHBA describes the work. It does not choose a physical GPU or hard-code a model. Concepts such as "big brain" and "small brain" remain Rack AI implementation policy, not ATHBA domain concepts.

### JCode owns
- model-facing agent/tool execution for bounded work allocated by Rack AI.

### ComfyUI owns
- its media-generation workflows and user interface. Rack AI supplies/reclaims the GPU resources required to run it; Rack AI does not hide or replace ComfyUI.

### OddesyAgent
- is the likely future human-facing interface/orchestrator. It is not required for the MVP.

## Workload awareness

Rack AI must understand more than isolated requests. A client can identify a wider workload and provide a coarse forecast so Rack AI knows whether it is servicing a tiny script, a multi-week ATHBA build, an interactive ComfyUI session, or an intensive media-generation pipeline.

The MVP contract should carry only what is necessary now, while leaving room for:
- workload identity/type;
- requested priority;
- anticipated duration/volume;
- concurrency opportunity;
- work-unit complexity;
- context/capability requirements;
- latency sensitivity;
- preemptibility/safe boundaries.

Applications forecast demand; Rack AI owns the actual schedule.

## Application-development MVP

ATHBA should turn a build into a dependency-aware pool of deliberately small tickets. Where practical, tickets should be test-driven: establish a failing test/observable contract, allow only the smallest bounded implementation required to satisfy it, and preserve existing tests.

TDD is a development strategy owned by ATHBA, not a Rack AI primitive. Non-test work may use another machine-verifiable acceptance contract.

Rack AI receives a work unit with complexity/capability/context information and selects the execution environment. Initially this policy may be deliberately simple, for example lower-complexity coding work on the small coding model and harder/planning/review work on the stronger model. The policy must remain replaceable without changing ATHBA.

The existing PR17 safety machinery remains valuable and should be reused beneath work-unit execution: JCode integration, isolation, allowed paths, leases, timeouts, Git evidence, bounded authority, acceptance/review and fail-closed behaviour.

## MVP definition of done

Rack AI 0.1 is useful when all of the following are true:

1. ATHBA can submit a small application-development workload and small dependency-aware work units to Rack AI.
2. Rack AI can allocate those units across the currently available local model/GPU resources and execute agentic work through JCode.
3. The development loop produces a real working small application; Tiny Ticket is the initial productivity benchmark.
4. Rack AI can drain/release the required development resource and make ComfyUI available as an interactive workload.
5. Development can subsequently resume without manually reconstructing the rack configuration.
6. Existing safety boundaries remain intact.

The MVP does not require an optimal scheduler, universal workload ontology, automatic optimisation for every future GPU, or OddesyAgent integration.

## PR plan

### PR22 — Architecture reset and minimum workload contract
Document the ownership boundaries above and implement only the minimum workload/work-unit/resource contract required by the vertical slice. Preserve existing safety mechanisms. Avoid building a general scheduler prematurely.

### PR23 — ATHBA + Rack AI application-development vertical slice
Make the minimum coordinated changes required in Rack AI and ATHBA. ATHBA decomposes a small application into deliberately small dependency-aware work units, preferably TDD-oriented where appropriate. Rack AI accepts work, selects the current execution resource/model, invokes JCode, returns evidence/results, and ATHBA advances the project.

Definition of done: Tiny Ticket is actually built successfully through the ATHBA/Rack AI path. Safety-only blocking is not a productivity PASS for this PR.

### PR24 — MVP workload/resource switching with ComfyUI
Implement the minimum lifecycle control needed to move between an application-development configuration and a ComfyUI configuration. Development must drain/pause at a safe boundary, the required GPU must become usable by ComfyUI, and the development configuration must be restorable afterward. Do not build a universal scheduler first.

### PR25 — Post-MVP roadmap backlog
PR25 is the durable home for good ideas deliberately deferred from the MVP. It must remain open/living and prevent future work from being lost while PR22–24 stay focused.

Candidate post-MVP work includes:
- richer rack inventory, GPU/VRAM telemetry and health;
- model/service registry and capability profiles;
- richer workload forecasts and global demand planning;
- capability-, complexity- and context-aware model selection;
- adaptive routing based on observed model success/cost/latency;
- concurrent work-unit scheduling across all available GPUs;
- dependency-aware queueing, fairness and backpressure;
- requested versus effective priority and rack-wide policy;
- deadlines, latency classes and interactive/background service classes;
- safe preemption, draining and resumable workloads;
- anti-thrashing/model-residency optimisation;
- automatic model loading/unloading and vLLM profile management;
- GPU reconfiguration for additional/future hardware, including multi-GPU resources;
- richer JCode execution profiles and harness selection;
- smarter escalation/retry/repair policies without weakening authority boundaries;
- workload history, evidence, observability and utilisation reporting;
- resource forecasting and capacity planning over hours/days/weeks;
- additional client workload types: image, video, audio/music and other AI services;
- Music Video Director integration as a resource-intensive workload client;
- OddesyAgent integration as the eventual human-facing control/orchestration layer;
- client API/versioning and remote control surfaces;
- persistence/recovery across rack restarts;
- policy for maintenance, upgrades and unavailable/degraded GPUs;
- energy/thermal-aware scheduling where useful;
- improved ATHBA decomposition, ticket sizing, TDD strategy and parallelism based on real MVP evidence;
- productivity benchmarks beyond Tiny Ticket;
- dynamic use of new GPUs/models without leaking hardware choices into domain applications.

## Delivery principle

Move fast above the safety boundary. Keep the proven containment, authority, evidence and fail-closed mechanisms conservative; allow the workload orchestration and optimisation layers to evolve rapidly from real usage.
