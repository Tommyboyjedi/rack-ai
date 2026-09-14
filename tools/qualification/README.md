# Single-model qualification tools

These are administrator-operated RackAI tools for a separately authorized live
window. They are **not live-qualified**. The September 14 GPT-OSS experiment
stopped at preflight because both original vLLM model-cache bind sources were
missing. Do not run a window until original model restartability is proven.

`cases.py` fixes checkable answers before model output: dependency timing,
closed-interval merging, Bayes counts and uncertainty handling. Three distinct
warm prompts request substantial final explanations. `benchmark.py` records one
new explicit submission ID before each deliberate invocation. Lost responses
stop the run; reconcile the persisted exact request identity before taking any
further action. Do not rerun the script to retry an HTTP attempt.

`window.py` combines canonical managed acquisition, initial trials, a release and
reload check, bounded monitoring and restoration of the exact original Docker
containers. It validates the administrator configuration before disruption and
rejects missing bind sources before stopping any service. A missing source must
be repaired by its owner under separate authorization, never by creating an
empty cache on `/srv/fast`.

Example, only after the reviewed preflight passes:

```sh
python3 tools/qualification/window.py PRIVATE_CONFIG.json PRIVATE_CONNECTION.json NEW_PRIVATE_EVIDENCE_DIRECTORY
```

The connection JSON has `url` (loopback receiver origin) and `token`. Keep both
input files and output directory protected. Config must use the existing
`/srv/rack-ai/state/resources` authority, one unqualified `big-brain` profile,
and a dedicated `gptoss-qualification` source permitted only Medium with explicit
qualification permission. No ordinary application principal is provisioned.
`window.py` assumes backend port 8096 for read-only metrics collection and a
4K acquisition; changing those requires reviewing this bounded recipe.

The proposed first profile (never launched) uses all three UUIDs, in this order:

- 4080 SUPER: `GPU-f9435bc0-a243-ad20-8b8b-166ab076e80b`, 15000 MiB budget.
- 2060: `GPU-357ef569-8fac-7c7d-ee1c-51677efb174f`, 5000 MiB budget.
- 4060 Ti: `GPU-042e18f2-bf9f-c8f6-6975-6f25b15ac71c`, 15000 MiB budget.

Proposed llama.cpp arguments: `--ctx-size 4096 --parallel 1 --n-gpu-layers 99
--n-cpu-moe 20 --split-mode layer --tensor-split 15,5,15 --threads 4
--threads-batch 4 --batch-size 512 --ubatch-size 128 --flash-attn on --jinja
--reasoning-budget 128 --metrics --cache-ram 0 --no-context-shift --no-warmup`,
plus the pinned existing artifact, alias `big-brain` and loopback port 8096.
These are an untested fit hypothesis, not qualified memory placement.

Limits: host 45056 MiB, CPU quota 400%, zero cgroup swap; artifact verification
180 s, startup 600 s, drain 10 s, stop 30 s, execution 240 s, admission wait
30 s, reservation TTL 1800 s. One pending invocation and dispatch worker; one
transition worker. Output cap 1024 tokens / retained response bound 256 KiB.
Pin the completed executable digest before configuration validation or launch.

Monitor aborts on less than 6 GiB available host RAM, swap growth above 256 MiB,
GPU temperature at least 85 C, actual CPU **Tdie** at least 68 C, lost monitoring,
or ten successive inference samples exceeding 256 MiB/s page-in activity.
Loading and artifact hashing are distinguished from inference disk activity.
Signals and ordinary errors enter managed cancellation/cleanup. Claims or
uncertain cleanup prevent original GPU consumers from restarting; the receiver
is retained for reconciliation. SIGKILL or host loss cannot promise immediate
restoration: use saved identities, inspect canonical evidence and prove process,
cgroup and GPU cleanup first. Never delete claims or retry uncertain inference.

The backend's `timings.predicted_n` and `predicted_per_second` supply decode
measurements. At least 256 newly generated tokens are required for a sustained
warm sample. Prompt/cache tokens never count as generated tokens. Around 10 tok/s
is marginal; passing requires every warm sample above 10, preferably 12+.
Reported timings remain linked to the durable managed invocation and activation.
TTFT is unavailable through this buffered path. Request-to-final-response time
includes managed overhead. Check `cache_n` for prompt/KV reuse; reload does not
clear the filesystem cache. Never count result replay as a fresh trial.

JSON expected-answer checks and truncation checks are deterministic screening,
not full semantic review of explanations or the returned interval-merging code.
Read all retained final answers, compare execution counts with backend logs and
metrics, and verify exact original configuration, library metadata, identities
and bounded primary/coder responses before declaring live restoration.
The full window has not yet been run; these latter operator checks remain
mandatory in addition to its automated health checks.

Run offline tests:

```sh
python3 -m unittest discover -s tools/qualification -p 'test_*.py'
```

Raw evidence, credentials, builds, model files and private configs stay outside
Git. This experiment permits at most three documented launch configurations;
this blocked attempt consumed **zero**. It authorizes no additional model,
competing-priority live scenario, application integration or production rollout.
