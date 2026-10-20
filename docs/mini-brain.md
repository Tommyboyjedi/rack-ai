# mini-brain

`mini-brain` is a separately reserved llama.cpp service for Qwen3.6-35B-A3B
Unsloth UD-IQ4_XS, including its BF16 vision projector and built-in MTP.
Use the normal authenticated RackAI reservation and scoped Chat Completions
interfaces; private hosting port 8099 is not an alternative client interface.
MusicVideoDirector and other consumer applications are unchanged.

| Setting | Configuration |
| --- | --- |
| Model | `/srv/models/qwen3.6-35b-a3b/Qwen3.6-35B-A3B-UD-IQ4_XS.gguf` |
| Projector | `/srv/models/qwen3.6-35b-a3b/mmproj-BF16.gguf` |
| Runtime | Pinned `/home/tomp/llama-cpp-minicpm/build/bin/llama-server` |
| Model alias | `qwen3.6-35b-a3b` |
| Context / input / output ceilings | 32768 / 24576 / 8192 tokens |
| Parallel requests | 1 |
| Ordered resources | `gpu-4060ti`, `gpu-2060` |
| GPU allowances | 15800 MiB / 5700 MiB |
| Host allowance / CPU quota | 24576 MiB / 1600% |
| CPU affinity | 0-15 |
| Artifact verification / startup | 900 / 600 seconds |
| Inference / drain / stop | 3600 / 3600 / 60 seconds |

RackAI resolves the ordered resources to physical UUIDs and supplies
`CUDA_VISIBLE_DEVICES`. Within that assignment CUDA0 is the 4060 Ti and CUDA1
is the 2060. The 4080 SUPER is never part of this service's resource set.
Normal priority admission, exclusive ownership, preemption, inactivity expiry,
readiness and verified cleanup apply. Conflicts require RackAI reservations;
foreign/manual GPU work must be released by its operator before activation.
Systemd llama.cpp activations do not acquire Docker/vLLM warm residency.

The launch retains the proven `--device CUDA0,CUDA1`, layer split `3,1`, all GPU
layers, fit off, flash attention on, q8_0 K/V caches, 16 generation/batch threads,
reasoning on, Jinja, `draft-mtp` with maximum draft depth 2, and metrics. Vision
keeps the tested default projector placement. No 128K/256K qualification is
claimed. The profile pins the model, projector, executable and loader dependencies.
`LD_LIBRARY_PATH` selects the original runtime directory; no shared runtime or
original model files are replaced. Runtime/library changes require requalification.

Reasoning and tools use the existing OpenAI-compatible chat interface. Explicit
request-level thinking and structured-output controls remain available. Reasoning
shares the bounded completion budget; clients should choose an explicit output
limit appropriate to each request.

Image input accepts up to two base64 data-URL images, 262144 decoded bytes and
4194304 pixels per image, within RackAI's existing 1 MiB request limit. The source
profile advertises PNG, JPEG, GIF and BMP. Remote URLs and server-side file paths
are not accepted. `input_token_counter: native_multimodal` reuses RackAI's native
`/v1/chat/completions/input_tokens` adapter for exact image/template accounting.
The scoped `/chat/completions/preflight` endpoint does not generate text. RackAI
also enforces the frozen input/output/context limits before inference. Native
counting failures reject the call rather than guessing or trimming input.

The source template remains unqualified. Private deployment qualification records
managed reasoning, tool round trip, image/count equality, one bounded throughput
sample and verified release before ordinary client publication. A final ordinary
reservation verifies the published service, then releases all test resources.
