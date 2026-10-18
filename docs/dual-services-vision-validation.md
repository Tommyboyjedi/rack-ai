# GPUrack dual-service vision validation

Managed qualification ran on GPUrack on 2026-10-18. Both services retain the
131072/65536/65536 context/input/output ceilings and the exclusive ordered
4080 SUPER + 4060 Ti resource pair. The RTX 2060 was not allocated. All other
service profiles and administrator-local working changes were preserved.

Both correctly interpreted two different shape/color images and their ordered
pair, returned valid JSON Schema output with zero reasoning, and matched exact
preflight counts to actual prompt usage. Expected answers were absent from the
prompts. Nex additionally interpreted JPEG, GIF and BMP variants. Strata's CPU
helper COUNT and ENC agreed for all five supported formats. Scene-state tests
kept jacket removal, the current studio location, current shirt/jeans and future
bedroom/coat intentions separate.

| Bounded managed measurement | dual-brain | dual-worker v3 |
| --- | ---: | ---: |
| Ready, including asset hashing | 428.794s | 95.547s |
| Input tokens, image A / image B / pair | 329 /329 /513 | 212 /212 /362 |
| Image A / B / pair request latency | 14.119 /9.377 /9.356s | 4.843 /5.109 /6.853s |
| Decode, 1024-token text sample | 35.3 tokens/s | 104.784 tokens/s |
| HTTP end-to-end output rate | 32.222 tokens/s | 81.289 tokens/s |
| Previous text-only observation | 39.7 tokens/s | 105.06 tokens/s |
| Supplied manual reference | about 45 tokens/s | about 100 tokens/s |
| Owned VRAM after calls, 4080/4060 | 15606 /15738 MiB | 13760 /12896 MiB |
| Cgroup host-memory peak | 55000 MiB | 27966.996 MiB |
| Swap / OOM kills | 0 / 0 | 0 / 0 |

These are bounded single-run observations, not controlled causal comparisons or
maximum-window benchmarks. Strata reached its existing 55000-MiB limit, with
reclaim pressure (memory.events.max = 12786); it has no demonstrated spare host
headroom. Its decode sample was 11.1% below the prior measured result and 21.6%
below the manual reference. GPU allowances remain 15800 MiB per card; the tight
4060 margin needs renewed qualification before increasing image/context work.
Nex stayed within its 32768-MiB allowance. Continuous maximum VRAM, maximum-size
images, full-context occupancy and extended visual aptitude were not tested.

Normal text/high reasoning, strict schema, client-owned tool round-trip,
buffered SSE, queued cancellation, equal-priority contention, five-second Ready
holding, release, stale access rejection and owned-process cleanup passed.
Higher-priority transfer and unaffected 2060 ownership were covered with private
CPU fixtures; no unrelated workload was preempted for testing. Normal Strata
shutdown removed its vision helper directory. Repeated hard SIGKILL can leave
per-generation cache directories requiring operator cleanup; no global bound
across repeated crash leftovers is claimed.

The final Nex profile explicitly accepts PNG/JPEG/GIF/BMP and binds native
codec lookup to `/dev/null`. A malformed PNG passed header/pixel validation but
failed native preflight (502); mandatory invocation admission persisted
`failed` with `input_admission_input_preflight_backend_failure`, and a following
valid image succeeded. No codec executable can be launched at the configured
non-directory paths. WebP is rejected before backend access by the typed MIME
policy. Strata accepts WebP through its pinned Pillow normalization.

Automated checks passed: 480 workspace Rust tests with serial execution, 119
runtime tests against the preserved production source overlay, 52 HTTP
regressions, 31 Strata adapter/vision tests, and 10 final production contract
checks. Independent source and operational reviews accepted the measured scope.
The default parallel workspace run reproduced an unchanged campaign fallback
assertion on both the branch and canonical baseline; isolated executions and
the complete serial run passed. All failures and reruns are retained. No tests
were weakened to obtain a pass.

Source changes cover typed native counting/MIME policy, loaded-vision readiness,
owned Strata helper adapters and COUNT source patch, pinned profiles/example,
strict public metadata schemas, documentation, CPU fixtures and CI. Original
model weights, GPU scheduling/admission/priority behavior, protected response
limits and consumer repositories were preserved.

Evidence and deployment metadata:
`/srv/rack-ai/evidence/dual-vision-20261018T015349Z/`.
The qualification credential was revoked (401 verified) and deleted. An ordinary
CB-principal reservation was accepted without qualification privileges. During
its long Strata load, the user requested immediate GPU availability. The
reservation was released through RackAI; all three owned processes disappeared,
claims were empty, and NVIDIA reported no compute processes. This is a user
interruption, not a startup failure. The additional ordinary-client image call
and worker repeat were deferred, with no further GPU tests started. Completed
managed qualification already covered Ready holding, image inference and release
for both profiles. Publication identities are retained in the evidence directory.
