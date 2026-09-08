# PR33 design: heavyweight local reasoning on the full GPU rack

Date: 2026-09-08

## Status

Documentation-only design and qualification plan.

This PR does **not** change the production model runtime, worker registry, GPU scheduling, JCode profiles, service definitions, or Rack AI execution behaviour. It records the target architecture and proof required before implementation.

## Goal

Add a heavyweight local reasoning capability that can temporarily use the whole rack when a task justifies it.

The target is not merely to make a very large model load. The target is:

> Run the strongest useful local reasoning model that this rack can sustain at **more than 10 generated tokens/second** for a single heavyweight reasoning request, with useful context, correct tool/chat behaviour, stable memory residency, and repeatable evidence.

`>10 tok/s` is a hard acceptance gate for the heavyweight role.

The first baseline is **GPT-OSS-120B MXFP4**. The preferred larger challenger is **Qwen3.8-Flash-Next** in a practical 4-bit GGUF quantization. Other models may enter the qualification only if they can plausibly beat these candidates on reasoning quality while satisfying the same throughput and stability gate.

## Target rack

The design is for the existing `gpurack` host after the incoming RTX 4080 Super is installed:

- AMD Ryzen Threadripper 1950X;
- ASRock X399 Taichi;
- 64 GB DDR4 system RAM;
- WD SN770 NVMe storage;
- NVIDIA RTX 4080 Super 16 GB;
- NVIDIA RTX 4060 Ti 16 GB;
- NVIDIA RTX 2060 6 GB.

Nominal aggregate GPU VRAM is 38 GB, but the cards are heterogeneous and do not provide a unified 38 GB memory pool. The implementation must treat placement, compute speed, PCIe traffic, workspaces, KV cache, and per-device fragmentation explicitly.

## Architectural decision

### Keep the existing workers

The current `local-primary` and `local-coder` paths remain vLLM-backed unless changed by a later, explicitly approved implementation PR.

The heavyweight model should initially be a separate service and role. The likely runtime is `llama.cpp` because the experiment requires fine-grained CPU/GPU MoE offload, mmap/lazy loading, heterogeneous multi-GPU placement, and GGUF quantization controls.

Using `llama.cpp` for this heavyweight sidecar is **not** a proposal to replace vLLM for the existing workers.

### Heavyweight mode is exclusive

The heavyweight role may require most or all accelerator memory plus a large fraction of system RAM. Rack AI should therefore treat it as an exclusive resource mode rather than another always-resident worker.

Target lifecycle:

1. checkpoint or drain conflicting rack work;
2. unload model services that hold required GPU memory;
3. acquire the selected GPUs and host-memory budget;
4. start or activate the heavyweight service;
5. execute the bounded heavyweight request;
6. persist result and qualification evidence;
7. stop/unload the heavyweight service;
8. restore the normal worker configuration.

The control plane responsible for this transition must not depend on an LLM process that it has just unloaded.

## Candidate 1: GPT-OSS-120B MXFP4 baseline

GPT-OSS-120B is approximately 116.8B total parameters but is a sparse MoE model with only a small subset active per token. The native MXFP4 checkpoint is approximately 60 GiB and is specifically suitable for mixed GPU/system-RAM deployment.

The important llama.cpp capability is CPU-MoE offload: dense/attention components and selected expert layers can remain on GPU while the remaining experts stay resident in system RAM.

The baseline exists for three reasons:

1. prove the heavyweight service path before attempting the larger Qwen configuration;
2. establish measured throughput and latency on the Threadripper/X399 memory subsystem;
3. provide a strong fallback model if the larger candidate cannot clear the throughput gate.

Do not prune GPT-OSS experts or re-quantize below its native MXFP4 representation during the first qualification phase.

## Candidate 2: Qwen3.8-Flash-Next preferred challenger

Qwen3.8-Flash-Next is the preferred larger candidate because its sparse architecture offers a much better total-parameter-to-active-compute ratio than a dense model of similar headline size.

Research snapshot for this PR:

- approximately 125B backbone parameters;
- approximately 6B active parameters per token;
- a separate large n-gram embedding/lookup component;
- optional multi-token-prediction support;
- current GGUF quantizations include practical 4-bit variants.

Two variants should be tested in order:

1. `UD-IQ4_XS` or equivalent lower-memory 4-bit build to prove the deployment;
2. `UD-Q4_K_XL` or equivalent higher-quality 4-bit build if memory and throughput permit.

The exact file names and byte sizes must be captured from the chosen immutable model revision at qualification time. Do not encode moving Hugging Face metadata as a permanent runtime invariant.

### Why the Qwen n-gram table matters

The larger Qwen configuration is only credible on 64 GB host RAM if its large lookup table can be memory-mapped/lazily accessed rather than forcing every byte to become permanently resident working memory.

The intended strategy is:

- ordinary model weights and active expert working sets: GPU VRAM and system RAM;
- large eligible lookup table: NVMe-backed mmap/lazy access;
- frequently touched pages: operating-system page cache in available host RAM.

This is **not** permission to page ordinary expert weights continuously from disk during generation. Sustained expert-weight swap activity is a qualification failure.

## Multi-GPU strategy

### Default: layer/model placement, not tensor parallelism

The initial heterogeneous-GPU strategy should use llama.cpp layer/model splitting so that whole layers/tensors are assigned to devices and only intermediate activations cross devices where possible.

Do not assume conventional tensor parallelism is appropriate across RTX 4080 Super, RTX 4060 Ti, and RTX 2060. The cards have different compute capability, VRAM size, bandwidth, and PCIe behaviour; repeated collective operations can erase the benefit of adding the slower card.

### The RTX 2060 must earn its place

Benchmark at least these configurations with identical prompts and runtime settings:

A. RTX 4080 Super + RTX 4060 Ti;

B. RTX 4080 Super + RTX 4060 Ti + RTX 2060.

The three-GPU configuration wins only if it improves useful generated tokens/second, time-to-correct-result, or the quality-capable quantization/context that can be run without dropping below the throughput floor.

If the RTX 2060 makes the workload slower, it remains available for the normal rack role rather than being forced into heavyweight inference for cosmetic aggregate-VRAM reasons.

### Placement objective

A first-pass allocator should attempt to leave safety margin on every GPU for compute buffers and KV/cache allocations rather than filling each device to the last MiB with static weights.

A rough starting objective for model-weight residency is:

- RTX 4080 Super: approximately 13-14 GiB;
- RTX 4060 Ti: approximately 13-14 GiB;
- RTX 2060: approximately 4-4.5 GiB if the three-GPU test is beneficial.

These are tuning targets, not hard-coded production constants. The final values must come from measured allocator output and repeatable benchmarks.

## Host RAM and NVMe strategy

The Threadripper/X399 platform provides quad-channel DDR4. For CPU-offloaded sparse experts, sustained host-memory bandwidth can be more important than headline CPU core count.

Qualification must therefore record:

- actual system-RAM residency;
- swap use;
- page faults and NVMe read behaviour;
- CPU thread/affinity configuration;
- NUMA policy;
- GPU utilization and VRAM residency;
- tokens/second after prompt ingestion, not just model-load success.

The SN770 is permitted to hold the GGUF files and provide mmap/lazy backing for eligible lookup structures. It is not a substitute for sufficient resident working memory.

## Runtime baseline

Build a dedicated, pinned `llama.cpp` runtime for heavyweight qualification rather than mutating the existing vLLM services.

Required runtime capabilities include:

- CUDA support for all selected NVIDIA devices;
- OpenAI-compatible `llama-server` endpoint;
- GGUF support for the selected model revision;
- CPU-MoE offload controls where applicable;
- layer/model multi-GPU splitting;
- mmap and model-specific lazy loading where supported;
- flash attention where supported and stable;
- metrics sufficient to record prompt processing and generation throughput.

A representative first Qwen qualification profile is conceptually:

```text
single request
32K context target
layer split
memory fitting enabled
mmap enabled
lazy lookup loading enabled where supported
flash attention enabled where supported
no speculative decoding in the baseline
```

Do not copy command-line flags from this design into production blindly. `llama.cpp` evolves quickly; the implementation PR must pin a tested commit and record the exact supported CLI contract.

## Benchmark methodology

### Measure the right number

The acceptance metric is **single-request generated output throughput**, not aggregate batched throughput and not prompt-processing speed.

Record separately:

- model load/start time;
- prompt-processing tokens/second;
- time to first generated token;
- generated tokens/second;
- generated reasoning-token count where observable;
- wall-clock time to correct answer;
- peak and steady GPU memory;
- peak and steady host memory;
- swap and storage activity.

### Test contexts

At minimum test:

1. short prompt / long generation;
2. medium context;
3. approximately 24K populated input inside a 32K window;
4. long reasoning response sufficient to expose steady-state decode behaviour rather than a short burst.

A configuration that exceeds 10 tok/s only at empty context but collapses below the floor at useful context is not qualified.

### Quality corpus

Use a fixed local qualification corpus with checkable results. Include:

- Python debugging;
- C# debugging;
- SQL/data transformation;
- algorithmic reasoning;
- architecture/design trade-offs;
- multi-step codebase reasoning;
- structured output/tool-call behaviour where the model is expected to support it.

Prefer executable acceptance tests and deterministic answer checks where possible. Human semantic review may supplement those checks but must not hide deterministic failures.

The decision metric is not benchmark eloquence. Prefer **time to a correct usable result**.

## Acceptance criteria

The heavyweight model/configuration is qualified only when all of the following are true.

### Throughput

- more than **10 generated tokens/second** on every representative sustained-decode test;
- target median at least approximately 12 tok/s to provide operational margin.

### Context

- stable at the selected useful context size;
- initial target: 32K total context with a representative approximately 24K populated-context test.

### Stability

- no OOM;
- no continuous expert-weight paging from swap/NVMe;
- no unbounded process growth;
- deterministic startup/shutdown and resource release;
- normal Rack AI workers can be restored after heavyweight mode exits.

### Correctness

- passes the fixed reasoning/coding qualification corpus at an acceptable quality level;
- valid chat-template behaviour;
- valid structured/tool-call behaviour for the role that Rack AI intends to expose.

### Evidence

Persist enough evidence to reproduce the result:

- model repository and immutable revision;
- quantization and file hashes;
- llama.cpp commit/build information;
- exact launch command/configuration;
- selected GPUs and placement information;
- context and sampling settings;
- benchmark prompts or prompt hashes;
- throughput/latency/memory metrics;
- qualification verdict and rationale.

## Qualification sequence

### Phase 0 - harness

Create a reproducible benchmark harness before optimization. The harness must make it impossible to confuse prompt-processing throughput with generated-token throughput.

### Phase 1 - GPT-OSS-120B on the current rack

Before or independently of the final three-GPU layout:

- load GPT-OSS-120B native MXFP4;
- establish CPU-MoE offload;
- measure decode throughput;
- validate OpenAI-compatible serving and chat formatting;
- record the Threadripper/RAM bottleneck profile.

This is the baseline proof, not necessarily the final winner.

### Phase 2 - GPT-OSS-120B on RTX 4080 Super + RTX 4060 Ti

Once the 4080 Super is installed:

- maximize useful GPU residency while retaining workspace margin;
- test layer/model splitting;
- sweep CPU-MoE residency and thread counts;
- establish the best two-Ada-card result.

### Phase 3 - all three GPUs

Add the RTX 2060 and repeat the same fixed workload. Keep it only if evidence shows an advantage.

### Phase 4 - Qwen3.8-Flash-Next lower-memory 4-bit

Prove the larger model using the lower-memory practical 4-bit quantization first:

- mmap/lazy lookup path;
- two-GPU versus three-GPU placement;
- CPU thread and NUMA sweep;
- 32K-context stability;
- quality corpus.

### Phase 5 - Qwen3.8-Flash-Next higher-quality 4-bit

Attempt the higher-quality 4-bit quantization using the same qualification harness. It becomes preferred only if it remains above the throughput floor and materially improves reasoning quality or reliability.

### Phase 6 - advanced acceleration experiments

Only after a stable baseline exists, A/B test:

- multi-token prediction/speculative techniques supported by the selected Qwen runtime;
- GPU expert caching;
- alternative CPU kernels/forks such as `ik_llama.cpp`;
- KTransformers AVX2 path if it provides a credible advantage.

Advanced features must be measured independently before combinations are attempted. Do not multiply separate published speedup percentages and treat the result as a forecast.

### Phase 7 - Rack AI integration

After a model/configuration qualifies:

- add a typed heavyweight worker/service profile;
- add exclusive resource acquisition and normal-worker drain/restore lifecycle;
- expose health, start, stop, ready, and benchmark evidence;
- integrate the heavyweight capability into generic Rack AI routing without teaching clients GPU/model details;
- preserve existing execution provenance so clients can prove which model/resource configuration ran.

The client should request a broad capability/complexity requirement. Rack AI remains responsible for selecting the concrete heavyweight model, runtime, GPUs, and resource mode.

## Initial model decision rule

The winner is the strongest configuration that satisfies all gates, in this order of importance:

1. correct and usable reasoning;
2. sustained `>10 tok/s` generated output;
3. stable useful context;
4. operational recoverability;
5. time to correct result;
6. lower resource cost / faster restoration of normal rack capacity.

A larger parameter count does not win automatically.

## Current hypothesis

The working hypothesis to prove or falsify is:

- GPT-OSS-120B MXFP4 should be the lower-risk baseline because its sparse MoE representation and llama.cpp CPU-MoE controls are already aligned with this hardware class;
- Qwen3.8-Flash-Next should be the preferred final challenger because it offers stronger current reasoning capability while retaining sparse active compute and a lookup structure that can plausibly use mmap/lazy NVMe backing;
- the RTX 4080 Super + RTX 4060 Ti pair is likely to provide the best compute core of the heavyweight setup;
- the RTX 2060 may improve residency but must be removed from heavyweight mode if its slower execution/transfer path lowers useful throughput;
- the Threadripper 1950X plus quad-channel DDR4 is the main uncertainty for sustained CPU-offloaded expert throughput and therefore must be measured rather than guessed.

This hypothesis is deliberately falsifiable.

## Non-goals for this PR

- no model download;
- no llama.cpp installation;
- no service changes;
- no vLLM replacement;
- no worker registry changes;
- no scheduler/preemption implementation;
- no production GPU reassignment;
- no new hardware purchase;
- no claim that any unmeasured configuration already satisfies the gate;
- no merge without explicit human instruction.

## Research references

These references capture the research basis as of 2026-09-08. Implementation must re-check moving runtime flags and model revisions before use.

- OpenAI GPT-OSS model repository: https://huggingface.co/openai/gpt-oss-120b
- GPT-OSS llama.cpp deployment discussion and CPU-MoE examples: https://github.com/ggml-org/llama.cpp/discussions/15396
- Qwen3.8-Flash-Next model repository: https://huggingface.co/Qwen/Qwen3.8-Flash-Next
- llama.cpp multi-GPU documentation: https://github.com/ggml-org/llama.cpp/blob/master/docs/multi-gpu.md
- llama.cpp server/runtime options: https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md
- Unsloth Qwen3.8 documentation and quantized-model guidance: https://unsloth.ai/docs/models/qwen3.8-next
- Experimental llama.cpp GPU MoE expert cache work: https://github.com/ggml-org/llama.cpp/pull/27861
- KTransformers AVX2 kernel path: https://github.com/kvcache-ai/ktransformers/blob/main/doc/en/kt-kernel/AVX2-Tutorial.md

## Follow-on implementation PRs

This design should be implemented in bounded steps rather than one opaque GPU/runtime rewrite. Suggested split:

1. heavyweight benchmark harness and GPT-OSS baseline qualification;
2. three-GPU placement and lifecycle qualification after RTX 4080 Super installation;
3. Qwen3.8-Flash-Next qualification and winner decision;
4. Rack AI exclusive heavyweight resource-mode integration;
5. optional advanced acceleration experiments.

Each implementation PR must retain measured evidence and keep unrelated Rack AI orchestration changes out of scope.
