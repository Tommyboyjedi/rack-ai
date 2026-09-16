# Reservation/work correction validation

Validated on gpurack in `/home/tomp/rack-ai-generic-access`, branch
`codex/generic-reservation-access`, based on published commit
`5b20155a8bd81b591c80a994e00f14f036de10f0`.

## Final architecture

Workspace submissions construct a typed `WorkspaceRequest` for the reused
`ExecuteWorkspace`/`ExecuteChange` machinery. The versioned work-unit request
parser, serializer, CLI command and legacy worker-selection branch are removed.
Worktrees, registered repositories, JCode, path/network enforcement, acceptance,
revisions, provenance, durable evidence and managed access remain in use.

One reservation retains its requested services and one priority. Acquisition is
independent per service, in request order. Missing services are never queued;
explicit refresh attempts only missing services. Ready and Held members retain
their service records. Original reserve replay returns its persisted original receipt.

## Required proof

| Requirement | Evidence | Result |
|---|---|---|
| 1. Three requested services yield two Ready and one unavailable | Partial-reservation fixture includes an unavailable first member and two usable peers | PASS |
| 2. Work runs on a Ready member of a partial reservation | Coder inference completes while big-brain is unavailable | PASS |
| 3. Work cannot run on an unavailable member | Submission is rejected; no invocation or backend start is created | PASS |
| 4. A Ready peer remains usable while another member is Held | Coder work completes during primary preemption | PASS |
| 5. Explicit refresh later acquires a missing service | Freed capacity remains unused until refresh; big-brain then starts | PASS |
| 6. Refresh preserves already acquired members | Existing member IDs/generations and startup counts remain unchanged | PASS |
| 7. Held keeps its restoration lifecycle | Refresh leaves primary Held; incumbent release restores the same service reservation ID | PASS |
| 8. Reserve replay never refreshes | Replay matches the original receipt before and after explicit refresh/restoration | PASS |
| 9. Versioned work-unit execution paths are gone | Retired CLI command rejects execution; production source has no version parser or old executor references | PASS |
| 10. JCode and native ComfyUI behavior remains supported | Managed workspace calls resume through the same scope after Held; primary/coder provenance, native object_info and managed images pass | PASS |

## Final validation

- Focused executor/selection tests: **6 passed**.
- All **15 focused runtime scenarios** pass, including the targeted correction of the unavailable-service response schema; the complete runtime run below confirms them together.
- Full `cargo test --workspace --offline`: **355 passed**, zero failures; run once. Documentation tests also pass.
- Full `.venv/bin/python -m pytest tests/runtime -q`: **82 passed, 2 subtests passed**, 313.98 seconds; run once.
- Rust format, workspace check, strict runtime clippy and diff whitespace checks: **PASS**.
- Production scan: no work-unit v1/v2 request/execution path, source-admission policy type, priority ceiling or tag-priority field remains.
- `Cargo.toml`, `Cargo.lock` and administrator-owned `config/repositories.json`: unchanged.

Development-only failed checks and their targeted corrections are retained alongside
final logs under untracked `evidence/reservation-work-corrections/`. Obsolete CLI
contract tests were removed or moved to reservation-owned work; reusable acceptance,
path, timeout, provenance and scope protections remain tested.

These are isolated development fixtures, not deployed GPU/model qualification.
No deployment, merge or client-repository changes were performed.
Configuration migration and fail-closed replay of older records without original
receipts are documented in [reservation-work.md](reservation-work.md).
