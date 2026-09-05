# PR25 — Living Post-MVP Rack AI Roadmap

This PR is intentionally a living backlog. It captures valuable work deliberately excluded from PR22–24 so the MVP can ship quickly without losing the longer-term architecture.

Do not treat every item below as a prerequisite for Rack AI 0.1. Reorder and split items into implementation PRs based on evidence from real ATHBA and ComfyUI usage.

## Resource knowledge
- richer physical GPU inventory and topology;
- VRAM/capacity telemetry;
- utilisation, temperature, power and health;
- degraded/unavailable resource states;
- model/service registry;
- model capability profiles;
- measured memory/context/performance profiles rather than static assumptions;
- multi-GPU and future hardware representation.

## Workload model and forecasting
- richer workload registration;
- current plus anticipated demand over hours/days/weeks;
- expected work-unit volume and complexity distribution;
- concurrency opportunities;
- deadlines;
- latency classes;
- interactive versus background workloads;
- preemptibility and safe boundaries;
- capacity forecasting;
- persistence of workload intent across rack restarts.

## Global scheduling and priority
- rack-wide arbitration across all client applications;
- requested priority versus Rack-AI effective priority;
- fairness and starvation prevention;
- dependency-aware work queues;
- backpressure;
- concurrent execution across all available GPUs;
- safe preemption and draining;
- resumable work;
- deadline-aware scheduling;
- interactive workload responsiveness;
- policy so no client application can seize/supersede rack resources directly.

## Model and execution selection
- capability-aware routing;
- complexity-aware routing;
- context-size-aware routing;
- tool/harness requirement matching;
- adaptive routing using observed success rate, repair count, latency and resource cost;
- escalation based on evidence rather than fixed small→large chains;
- bounded repair after safe rejections where appropriate without weakening authority boundaries;
- richer JCode execution profiles;
- future harness selection while retaining JCode as a principal agent execution mechanism;
- model residency decisions based on anticipated workload.

## Model/service lifecycle
- automatic vLLM profile management;
- model load/unload;
- service start/stop/restart;
- health verification;
- GPU reclamation;
- anti-thrashing policies;
- warm model/service retention when future demand justifies it;
- reconfiguration planning when GPUs are added/removed;
- maintenance and upgrade modes.

## ATHBA evolution informed by MVP
- improved specification→architecture→ticket decomposition;
- automatic ticket sizing for available model capability;
- stronger dependency DAG construction;
- strict/optional Uncle Bob TDD modes;
- machine-verifiable non-test ticket contracts;
- test-author/implementation-worker separation where valuable;
- parallel ticket claiming;
- ticket requeue/escalation semantics;
- complexity estimation calibration;
- project-level workload forecasting to Rack AI;
- productivity metrics and benchmark suites beyond Tiny Ticket.

## Additional workload clients
- richer ComfyUI lifecycle/profile integration;
- image-generation workloads;
- video-generation workloads;
- audio/music generation and stem workflows;
- Music Video Director integration and multi-stage media pipelines;
- future AI services using the same workload/resource contract.

## Interfaces and product integration
- stable/versioned client API;
- remote workload submission/control;
- rack status and observability surfaces;
- OddesyAgent as eventual human-facing intent/orchestration layer;
- notifications and progress reporting;
- client-visible resource availability without leaking scheduling authority.

## Observability and evidence
- workload history;
- execution history;
- GPU/model utilisation;
- queue state;
- scheduling decisions and rationale;
- failure/recovery evidence;
- productivity/throughput metrics;
- capacity reports;
- operator diagnostics.

## Optimisation opportunities
- minimise model load/unload churn;
- maximise useful concurrent GPU utilisation;
- choose cheap/small models where they reliably succeed;
- escalate only when evidence warrants it;
- energy/thermal-aware scheduling where useful;
- exploit future 24GB/48GB resources and multi-GPU configurations without changing domain clients;
- benchmark alternative local models as hardware evolves.

## Safety principles retained
Future optimisation must not casually weaken the mechanisms already proven during the PR17 qualification work:
- bounded authority;
- allowed-path enforcement;
- isolation;
- timeouts;
- leases;
- Git/evidence capture;
- independent acceptance/review;
- fail-closed behaviour;
- no client-owned GPU authority.

## Roadmap rule
When an item becomes concrete enough to implement, create a focused PR and link it back here. Keep this document focused on deferred capabilities and architectural intent rather than turning PR25 itself into one enormous implementation branch.
