# PR32 Generic Capability Routing

PR32 publishes `rack-ai/work-unit/v2` for the existing bounded workspace transaction. It adds only a typed generic routing header: opaque source/work/submission/idempotency identity, a non-empty `reasoning`/`coding`/`visual`/`audio` capability set, `small`/`medium`/`large` complexity, the existing large-context boolean, and global priority.

Rack AI owns typed source ceilings and internal profiles. The configured ATHBA ceiling is medium. `local-coder` is the least-scarce qualified choice for small coding; medium reasoning-plus-coding selects `local-primary`. Busy qualified capacity is temporary unavailability, not a capability claim.

Every selected transaction retains generic decision evidence and matching execution provenance. V1 stays compatible; v2 does not add dependency semantics, client workflow terms, a universal execution form, ComfyUI arbitration, or scheduler work.

## 2026-09-05: bounded JCode runtime socket paths

Historical trigger: ATHBA run `pr29-fresh-signalboard-20260905T162901Z` failed
before its first worker started. Rack AI retained `status=failed`, empty worker
output/changed paths, and `last_error="path must be shorter than SUN_LEN"` in
`state/changes/pr29-fresh-signalboard-20260905T162901Z--REQ-001--scenario-draft--pr29-fresh-signalboard-20260905T162901Z--REQ-001--scenario-draft-1--submission-8484878532335806390/review-packet.json`.
The on-disk separators are ASCII `--`, not en dashes. Its change ID is 164 bytes;
its worktree path is 199 bytes. The packet retains full selection and worker
provenance. ATHBA reported an external harness blocker with no candidate or
consumed Tester attempt; no ATHBA files were inspected or changed for this fix.

The execution chain is `JCodeChangeImplementer::implement` ->
`JCodeProcessRunner::run_with_allowed_paths` -> `run_internal` ->
`JCodeExecutionConfig::prepare_at` -> `build_command` ->
`prepare_bubblewrap_command` -> `HostUnixBridge::start` -> `UnixListener::bind`.
The final bridge pathname was constructed in `prepare_bubblewrap_command` as
`<root>/selected-vllm.sock`. The old root was
`std::env::temp_dir()/rack-ai-jcode-run-<epoch-nanoseconds>-<counter>`.
On Linux, inherited `TMPDIR` could therefore put arbitrary client/path text in
the socket prefix. Worktree/change IDs are not directly appended by the runner.
Linux's pathname socket limit is 107 bytes (108-byte `sun_path`, including NUL).
The retained packet and Rack AI logs do **not** record the historical TMPDIR,
root timestamp/counter, or socket pathname. The exact original pathname and byte
length are unrecoverable from this evidence; attributing particular historical
identity components to TMPDIR would be speculation.

Before production changes, the deterministic subprocess regression failed with
that exact SUN_LEN error using a realistic long opaque work/submission identity
in inherited TMPDIR and a fixture executable, without inference. It also checks
that the legacy constructed pathname is rejected as `InvalidInput` by an actual
Unix listener. With the fix, two concurrent processes with distinct long
identities execute successfully, use distinct runtime paths, and remove them.

`JCodeRuntimeRoot` now exclusively creates a private mode-0700 directory at
`/tmp/rack-ai-jcode-run-<32 lowercase hex digits>`. The token contains 128 bits
from the OS random source; atomic directory creation fails closed on collision
and never reuses another execution's directory. There is no new global counter,
new dependency, client-specific naming rule, or audit-ID hashing/truncation.
The full bridge socket pathname is **74 bytes**, independent of TMPDIR and all
client IDs. A scoped owner removes the root after success and every ordinary
error return; the existing bridge guard removes the socket before root cleanup.
Cleanup tests cover success, worker failure, timeout, spawn failure, and setup
failure. Abrupt host/process death is not a newly claimed cleanup guarantee.

The durable evidence regression round-trips the entire packet through the real
manifest repository after fixture execution: full change ID and packet pathname,
work ID, submission ID, idempotency key, complete selection decision, and worker
provenance remain equal. Persisted idempotency lookup still distinguishes a
changed submission. Runtime tokens never replace those identities.

Validation on gpurack (no model calls): four new focused tests passed;
`cargo fmt --check`, `cargo test --workspace --offline` (158 application,
9 CLI, 52 domain, 103 infrastructure), and `git diff --check` passed. The suite
includes sandbox path/network restrictions, bidirectional TCP/Unix bridge,
and descendant termination tests. The unmodified legacy
`tests/rack_change_timeout_smoke.sh` stopped before launch with
`no enabled JCode implementer worker configured`: its v1 fixture expects the
old implementer role. A retained copy adapted **only the disposable fixture's**
worker role to `implementer-tester` (and fixed the script's repository path);
its fake-worker production-CLI bridge smoke passed with
`rack_change_timeout_smoke: ok`. No production worker configuration was changed.
Red/green, focused, workspace, and both smoke captures are retained untracked
under `state/pr32-socket-hotfix/`.

No ATHBA, routing, source admission, worker selection, model/GPU configuration,
JCode installation/arguments, execution budget, timeout, endpoint restriction,
network sandbox, allowed-path policy, or provenance semantics changed. The
max-turns, stderr/tool-call, executor-kind, and unused-getter TODOs remain intact.
PR32 is not merged. `MODEL_CALLS_MADE=0`.
