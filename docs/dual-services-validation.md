# GPUrack dual-service validation

Validated on GPUrack on 2026-10-17. Both private profiles are qualified and discoverable. Source profiles remain unqualified installation candidates, following RackAI conventions. No consumer application or existing service profile changed.

Both services atomically own `gpu-4080-super` and `gpu-4060ti`, ordered by their existing physical UUID identities. The RTX2060 was not allocated. Context/input/output ceilings are **131072/65536/65536**; Strata additionally reserves 8 context tokens. Both have one parallel slot and retain high-reasoning defaults. Use the model-specific thinking-off recipes in [dual-services.md](dual-services.md).

| Measured bounded qualification | dual-brain | dual-worker |
| --- | ---: | ---: |
| Reported decode,1024 output tokens |39.7 tokens/s |105.06 tokens/s |
| HTTP end-to-end output rate |36.15 tokens/s |81.42 tokens/s |
| Supplied manual reference |about 45 tokens/s |about 100 tokens/s |
| Ready including asset verification |405.02s |81.07s |
| Owned-process VRAM after calls,4080/4060 |15608/15732 MiB |13760/12896 MiB |
| Cgroup host-memory peak |55000 MiB |1911.96 MiB |
| Swap/OOM kills |0/0 |0/0 |

Decode rates are backend timings; HTTP rates include dispatch, automatic counting and prefill. These are single bounded samples. Full-window occupancy/performance was not tested. Strata reached its finite host-memory cap during startup and reclaimed cache without an OOM or swap; CPU throttling was zero in retained observations. The40GiB expert budget was preserved. GPU admission allowances are 15800 MiB/card, below measured idle free memory 15945/15949 MiB.

Normal chat, high reasoning, strict JSON Schema, one client-owned function/stub-result round trip,1024-token buffered SSE, equal-priority contention, queued cancellation, release, stale access and complete owned-process cleanup passed. Exact preflight input counts matched actual usage, including tools, schema and thinking controls. Nex's actual loaded context was 131072 with one slot; Strata readiness checked the actual loaded model/context and engine process.

Ordinary `reserve` proofs used a credential with qualification permission disabled, held Ready for 5 seconds, generated chronological scene-state JSON and released. Both correctly retained current clothing/location after jacket removal and a move, while keeping future bedroom/coat intentions separate. Both returned 77 completion tokens, zero reasoning and finish_reason stop.

Automated validation passed: 471 workspace Rust tests; 110 runtime tests against the production source overlay; 35 dual/Tiny/preflight fixtures; 10 Strata adapter tests; 11 protocol/contract fixtures. Four affected HTTP-helper tests passed again after explicit response cleanup. Formatting/diff checks and independent semantic reviews passed. Higher-priority atomic transfer, draining, queued cancellation, failure cleanup and 2060 isolation were verified with private CPU fixtures; no unrelated active workload was preempted for testing.

One unrelated retained-compaction assertion failed in an initial combined debug fixture run, then passed unchanged against both the previous production binary and current binary. Logs retain all outcomes. An initial Nex schema test exposed reasoning despite top-level none; exact installed-parser inspection identified the native template control, and the corrected managed run passed with zero reasoning. Initial VRAM allowances were corrected before model startup; only our own Preparing acquisition was released.

Strata's structured output is prompt-and-validate, not constrained decoding, and rejects schema+tools in the same request. SSE is buffered. Full-context saturation, extended aptitude and MusicDirector suitability remain unqualified.

Changed files cover the two profiles/example, generic launch/config/integrity/identity handling, Strata adapter/readiness, mandatory token admission, native protocol options, public-contract documentation, CPU fixtures and CI. Existing profiles, credentials, protected 3 MiB response limit and consumer repositories were preserved.

Full sanitized evidence, qualification/profile identities, ordinary reservation bindings, mapped-library audit and rollback metadata are under:
`/srv/rack-ai/evidence/dual-services-20261017T213615Z/`.
The temporary credential was revoked (401 verified) and deleted. Final claims were empty, all owned Python/C++ processes were absent and GPUs returned to idle. [Configuration, API examples, handoff and rollback](dual-services.md) accompany this report.

| Ordinary resource | Reservation ID | Frozen qualified profile hash |
| --- | --- | --- |
| dual-brain | `69e039d6bbee6e615d6b0769f787a975` | `11c7737ea074e94dbf303e8eacc55733d117063972a739553dfb1675824d5d79` |
| dual-worker | `8c41e6ad65e42874630a70e0b718dc3d` | `eba2883519c5a1fa1b98dc01cf310ab1e63d9069bd12603bef4270a13de4f000` |

Release binary SHA-256: `e116db02e136e1a290ec808b29b3b8a66e59798be1938da769a6ac4b40a6a60c`.
Final private configuration SHA-256: `c7126735c7bca7352efe840b7822082c77a898d5ae0f5fda4924c44677a44298`.
Publication commit/merge identities are recorded in the adjacent deployment manifest after publication. The production binary preserves the pre-existing administrator-local readiness/residency overlay; these unrelated edits were excluded from the publication.
