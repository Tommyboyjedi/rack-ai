# PR32 live generic routing qualification

Date: 2026-09-03 UTC

Published PR32 commit before qualification: `ddda2f8d375b0a9cd5243583bbe9f249bfb287d9`.

## Scope and fixture

This proof used one neutral disposable Git repository at `/srv/ATHBA/state/projects/rack-ai-pr32-routing-fixture-20260903T0730Z`. The path is beneath Rack AI's administrator-configured trusted dynamic root; no Rack AI trusted-root policy was changed. The fixture contained only `README.md` and `value.txt`, used `main` at `64f0c7bbbf8e4444c35b3769ba01e5f9ef0068bc`, and was not an ATHBA feature proof.

ATHBA itself was not invoked. No ATHBA source, configuration, persisted application state, project record, test, documentation, branch, or commit was modified. Rack AI created isolated worktrees under `/srv/rack-ai/state/workspaces` and stored proof inputs, command captures, and terminal packets under `/srv/rack-ai/state/pr32-live-generic-routing-20260903T0730Z`.

## Final v2 wire contract for connector reconciliation

`rack-ai/work-unit/v2` requires `work_unit.routing` with this exact shape:

```json
{"source_system":"athba","work_id":"opaque-work","submission_id":"opaque-submission","idempotency_key":"opaque-key","required_capabilities":["reasoning","coding"],"priority":"medium"}
```

The generic capability values are `reasoning`, `coding`, `visual`, and `audio`; complexity values are `small`, `medium`, and `large`; global priority values are `low`, `medium`, `high`, and `paramount`. The typed source-admission policy caps `athba` at `medium`; `high` and `paramount` fail before worker selection. The client does not select a worker, model, GPU, endpoint, or JCode profile.

## Deterministic gate

Focused v2 tests: 11 explicit tests passed, including multi-capability parsing, ATHBA admission rejection, coding-small least-scarce selection, reasoning+coding medium selection, decision persistence, temporary-unavailable distinction, persisted idempotency, old v1 compatibility, and a deliberate v2 selection/provenance mismatch that fails closed.

`cargo fmt --check` passed. `cargo test --workspace --offline` passed: 158 application, 9 CLI, 52 domain, and 99 infrastructure tests.

## Live qualification results

All three requests were versioned v2 documents under `/srv/rack-ai/state/pr32-live-generic-routing-20260903T0730Z/proof-inputs/`. Each objective was a neutral, bounded append to `value.txt`; each acceptance command checked that exact added line. Every request used priority `medium`, `requires_large_context: false`, a fresh opaque work ID, submission ID, and idempotency key, and the trusted base SHA above.

| Qualification | Request evidence | Selected worker / actual provenance | Terminal evidence |
| --- | --- | --- | --- |
| A reasoning+coding medium | `proof-inputs/qualification-a-v2.json` | `local-primary` / `local-primary` | approved, `value.txt`, candidate `accfe57e6c878cf7f75f9883288a3d4bf65c0156`; packet `state/changes/rack-ai-pr32-qualification--pr32-a-reasoning-coding--submission-17950471542946550452/review-packet.json` |
| B coding small | `proof-inputs/qualification-b-v2.json` | `local-coder` / `local-coder` | approved, `value.txt`, candidate `47bf899df2086c2e8815517b87e6da7936b3ccc1`; packet `state/changes/rack-ai-pr32-qualification--pr32-b-coding-small--submission-17950474841481435085/review-packet.json` |
| C reasoning+coding medium | `proof-inputs/qualification-c-v2.json` | `local-primary` / `local-primary` | approved, `value.txt`, candidate `1da9937d1ae298c8857c2fcf5aef94c003e72ffa`; packet `state/changes/rack-ai-pr32-qualification--pr32-c-stronger-generic-profile--submission-17950473741969806874/review-packet.json` |

The durable selection decisions record eligible/ineligible workers and reasons. A and C record `local-coder` as `capability_unsupported`; B records both workers eligible and selects `local-coder` for `least_scarce_sufficient`. Packet summaries are retained at `live-output/packet-summary.jsonl`; raw CLI captures are `live-output/qualification-{a,b,c}.txt`.

The terminal packet currently has no persisted duration field. Conservative wall-clock upper bounds, measured from durable request-input creation to durable packet creation, are A under 12.84 seconds, B under 52.89 seconds, and C under 17.06 seconds. The packet summaries retain the terminal status, changed paths, candidate revision, selection decision, execution provenance, and deterministic command evidence.

## Admission and idempotency checks

The ATHBA high and paramount request documents are `proof-inputs/athba-high-v2.json` and `proof-inputs/athba-paramount-v2.json`. Both command captures contain `source priority exceeds configured admission ceiling`, exit nonzero, and no selected worker or execution provenance. Review-packet count was 3 before and 3 after, recorded in `live-output/packet-paths-before-negative.txt` and `live-output/packet-paths-after-negative.txt`; therefore neither request reached JCode invocation or fabricated terminal provenance.

The exact A request was replayed with the same submission and idempotency identities. It returned `duplicate idempotent submission`; the packet count remained 3 before and after, recorded in `live-output/idempotency-replay.txt`. No second model execution occurred.

Temporary unavailable versus no capability is deterministic evidence: `registry_work_unit_worker_selector::tests::generic_distinguishes_temporary_capacity_from_no_capability` passed. No disruptive GPU outage was created.

## Cleanup confirmation

The three Rack AI-managed worktrees were removed, then `/srv/ATHBA/state/projects/rack-ai-pr32-routing-fixture-20260903T0730Z` was removed and its absence was verified. The retained Rack AI-owned packets, request documents, and command captures remain under `/srv/rack-ai/state/pr32-live-generic-routing-20260903T0730Z`. `git -C /srv/ATHBA status --short --branch` showed only its pre-existing branch tracking line; no ATHBA source or configuration change was made. No Rack AI configuration change was used for the proof.
