# PR18 Supporting Note — Model Qualification and Evaluation Framework

## Status

Future roadmap functionality only. Not part of PR14–PR17 and not approved for immediate implementation.

## Why this belongs in Rack AI

PR14 showed that model quality cannot be inferred safely from parameter count, context-window size, a `Coder` label, or harness choice alone. The current `Qwen2.5-Coder-3B-Instruct-AWQ` worker failed under both JCode and Abacus, while `local-primary` behaved materially better. This validates treating **model + quantization + runtime configuration + GPU + harness + task class** as a qualified capability profile.

Rack AI should eventually be able to evaluate new local models reproducibly before promoting them into production worker roles.

## Future capability

A future model qualification framework could allow an operator or developer to say, in effect:

> A new model has appeared. Import it, serve it safely on an eligible GPU, run the Rack AI qualification suite against the available harnesses, compare it with current workers, and report whether it should be registered for any production role.

The system should be able to perform this as a bounded experiment without changing production routing automatically.

## What the framework should evaluate

At minimum, a candidate model profile should record and test:

- publisher/model/revision identity;
- quantization format and exact artifact;
- runtime/version and launch configuration;
- GPU/VRAM requirement and observed headroom;
- usable context configuration, not merely advertised maximum context;
- throughput/latency and stability;
- instruction following;
- truthful completion/no false-success behaviour;
- repository navigation and tool-use behaviour;
- JCode compatibility;
- Abacus compatibility;
- deterministic coding-task acceptance;
- compiler/test feedback and repair;
- context/truncation behaviour;
- timeout/cancellation behaviour;
- evidence/transcript quality;
- task classes for which the model is qualified;
- regressions relative to currently registered models.

## Important design principle

Qualification is **not a generic model leaderboard**.

The question is not simply whether model A has a higher benchmark score than model B. The question is whether a concrete deployed profile works reliably inside Rack AI's actual software-development stack.

A qualification key should therefore be closer to:

`model + revision + quantization + vLLM configuration + GPU + harness + task class`

than merely `model name`.

## Promotion model

A future workflow should separate:

1. discovery/import;
2. isolated serving;
3. qualification runs;
4. evidence and comparison;
5. capability classification;
6. operator/developer approval;
7. registration into production routing.

A candidate should never become a production route merely because it downloaded successfully or passed one synthetic benchmark.

## Rack AI ownership boundary

This capability belongs in Rack AI because Rack AI owns:

- GPU/resource knowledge;
- model/runtime registry;
- harness routing;
- task/campaign policy;
- deterministic acceptance;
- evidence and qualification state;
- production promotion decisions.

JCode and Abacus remain execution harnesses used *within* qualification. They should not become the authoritative model registry.

vLLM remains the inference runtime and should not own capability/promotion policy.

## Automation opportunity

Once the base architecture is stable, much of model evaluation could become automatic:

- fetch metadata/artifact;
- check expected VRAM fit;
- launch candidate in an isolated/non-production runtime slot;
- run a fast screening suite;
- eliminate obvious false-success/tool-protocol failures;
- run a deeper qualification suite only for survivors;
- compare against current registered worker baselines;
- emit a capability report and recommended role/harness routes.

This could substantially reduce the cost of keeping Rack AI current as open-weight models improve.

## What should remain human/policy controlled initially

- whether an external model source is trusted;
- disk/download budget;
- whether a candidate is worth a deeper run;
- whether it may replace an existing production worker;
- whether a new model license is acceptable;
- whether a regression in one task class is acceptable in exchange for gains elsewhere.

## Why defer

Do not build this before PR17.

Right now we need to prove the core harness-backed architecture and establish a small number of known-good worker profiles manually. The current PR14 follow-up model tests should be treated as the prototype evidence from which a future automated qualification framework can be designed.

Building the automation before the manual qualification process is stable would risk automating the wrong standards.

## Current developer lesson from PR14

Do not require a model to be a dedicated coding model or to expose an arbitrarily large context window merely because those properties sound desirable.

Prefer evidence from actual Rack AI tasks. A general instruct/agent model with stronger tool use and instruction following may be more useful than a coding-specialist model with higher raw code-generation benchmarks.

Likewise, advertised context maximum and sensible deployed context are different things, especially on VRAM-constrained workers.

## Future success criterion

A mature implementation should make adding a candidate model closer to:

> evaluate this model for Rack AI

than a bespoke multi-hour engineering exercise, while still retaining reproducible evidence and explicit promotion control.
