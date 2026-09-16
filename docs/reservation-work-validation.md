# Reservation/work validation

Validated in `/home/tomp/rack-ai-generic-access` on gpurack, based on main
`dd1b60715defb60cf6993c2c66d036dc4ee6fb61`. Validation preceded the publication commit.

## Essential proof

| Requirement | Evidence | Result |
|---|---|---|
| 1. Identity does not cap priority | `test_generic_principals.py`: both principals request all global priorities despite deprecated policy | PASS |
| 2. Identity does not restrict services | Uniform discovery/admission; qualification and ownership still enforced | PASS |
| 3. Reservation priority arbitrates | Strict preemption, incumbent ties, independent Paramount reservations; atomic multi-resource admission | PASS |
| 4. Work inherits reservation priority | Work rejects a priority field; inference binds the existing activation; workspace routing reads persisted reservation priority | PASS |
| 5. Multiple services per reservation | Primary/coder and primary/ComfyUI groups; group release; all-or-nothing admission denial | PASS |
| 6. JCode uses managed access | Primary and coder workspace executions use existing scoped gateways; original registry bytes remain unchanged | PASS |
| 7. Held work continues after restoration | One workspace calls before preemption, waits without dispatch while Held, then makes two further calls with the same scope/work identity | PASS |
| 8. Native ComfyUI interface is surfaced | Existing configured native URL/session is returned instead of an LLM gateway path | PASS |
| 9. Native object information works | `/object_info` succeeds through that URL; combined reservation is fenced while Held and usable after restoration | PASS |
| 10. Managed images remain supported | Existing `test_paramount_managed_images_bind_owner_priority_and_replay` passes | PASS |
| 11. Reservation/preemption regressions | Entire runtime fixture suite, including existing lifecycle, cancellation, uncertainty, ownership and media boundaries | PASS |

## Commands and outcomes

- Focused reservation/policy tests: **7 passed**.
- Focused final workspace/native/reservation proof: **5 passed**.
- `.venv/bin/python -m pytest tests/runtime -q`: **84 passed, 2 subtests passed**, 341.51 seconds.
- `cargo test --workspace --offline`: run once. **364 passed, 1 failed** because an existing idle test still required the removed ATHBA priority ceiling. The assertion was corrected to verify Paramount admission and incumbent-wins-ties denial.
- Targeted `cargo test -p rack_ai_runtime --offline`: **14 passed** after that correction. The other 351 Rust unit tests passed in the full run; combined coverage is 365 passing unit tests. The full suite was not repeated.
- Remaining workspace documentation tests: passed (zero examples).
- `cargo fmt --all -- --check`: passed.
- `cargo check --workspace --offline`: passed.
- Strict runtime clippy initially identified a redundant field name and an enlarged request enum. Both were corrected; targeted strict clippy recheck passed.
- `git diff --check`: passed.

Logs are retained untracked under `evidence/generic-reservation-work-validation/`, including the initial failures and targeted rechecks.

These are isolated development fixtures using the existing process/sandbox/media paths. They are not deployed GPU/model qualification. No service deployment, client-repository change or merge was performed during validation.
