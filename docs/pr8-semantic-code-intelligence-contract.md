# PR8 Implementation Contract — Read-Only Semantic Code Intelligence

## Status

Implementation contract for PR #8. This PR is intentionally sequenced after PR #7.

Do not implement PR8 against the pre-PR7 control loop. Before implementation, update this branch onto the merged PR7 `main` baseline so that semantic tooling is consumed by the new diagnosis/replanning path rather than becoming a parallel architecture.

## Relationship to PR6

The original PR6 acceptance criteria remain frozen. PR8 adds capability intended to make that original substantive scenario robustly solvable; it does not expand PR6 into objective planning, web research, adaptive scheduling, or richer autonomous escalation.

PR7 supplies the failure-diagnosis/replanning loop. PR8 supplies better repository understanding for the local models.

## Problem to solve

Current implementation workers have only:

- `read`
- `write`
- `bash`

This is safe but places too much repository-navigation burden on a small local coding model. A worker or coordinator must infer when to construct shell searches, interpret textual matches, understand definitions/references/types, and connect compiler failures to call relationships.

The PR6 failure exposed this weakness: an existing out-of-scope caller was an important compatibility constraint. Rack AI should be able to discover and reason about such relationships without granting any additional mutation authority.

## Goal

Add bounded, read-only semantic code-intelligence operations to Rack AI so implementation and recovery reasoning can ask repository questions such as:

- where is this symbol defined?
- what references this symbol?
- what implementations exist?
- what type/hover information is known here?
- what diagnostics are currently reported?
- what workspace symbols match this query?
- where available, what callers/callees are related?

Mutation, shell execution, Git and promotion remain owned by existing Rack AI paths.

## Architectural principle

Semantic intelligence is evidence, not authority.

The system should preserve this split:

```text
semantic read/query -> whole registered target repository as permitted read-only evidence
write/edit          -> existing step allowed_paths only
shell/test          -> existing WorkspaceExecutor/Podman boundary
Git/commit          -> Rack AI campaign controller only
promotion           -> unchanged; no remote push/merge
```

Do not route edits through Serena/LSP workspace-edit operations in PR8. Even if the underlying semantic server supports mutation, Rack AI must expose read-only semantic capabilities only.

## Implementation approach

Prefer the smallest maintainable adapter that satisfies the contract.

For Rust repositories, `rust-analyzer`/LSP is the expected first backend. An MCP language-server bridge or Serena read-only prototype is acceptable if it keeps authority inside Rack AI and does not introduce another agent/orchestrator. A direct Rust LSP adapter is also acceptable.

Do not hard-code the entire application around one MCP server. Define a Rack AI-owned semantic-code-intelligence abstraction so the backend can be changed later.

## Required semantic operations

Exact names are open, but provide typed equivalents of at least:

1. workspace/symbol search;
2. go-to definition;
3. find references;
4. hover/type information;
5. diagnostics;
6. implementations where the language server supports them.

Call hierarchy may be added if it is straightforward and bounded, but it is not required if references/definitions provide the evidence needed by the regression tests.

Each operation must:

- be read-only;
- have bounded input and bounded output;
- enforce repository/worktree scope;
- reject path traversal or unrelated filesystem access;
- use timeouts;
- return structured errors rather than hanging;
- be recordable in worker/recovery evidence.

## Integration with workers

Expose semantic operations to the real campaign implementation worker in a way that the local-coder can call through the same model tool-call loop.

Do not remove existing `read`, `write`, or `bash` tools.

Tool descriptions should be explicit enough for a small model to choose semantic queries over ad-hoc `grep` when appropriate.

The tool protocol must remain OpenAI-compatible and preserve existing malformed-tool-call handling.

## Integration with PR7 recovery diagnosis

The local-primary recovery/diagnosis path must be able to consume semantic evidence.

This does not necessarily require giving the read-only reviewer an unrestricted interactive tool loop if a smaller controlled evidence-gathering stage is cleaner. The important behaviour is that diagnosis can request or receive repository-semantic facts relevant to the failure.

For the PR6 regression case, the system must be able to establish something equivalent to:

- the changed service/API symbol has an existing reference/caller in an out-of-scope file;
- the caller is read-visible but not writable;
- preserving that caller's contract is therefore an implementation constraint;
- replanning must change permitted implementation files rather than the caller.

## Bash result quality

While touching worker tooling, improve `bash` tool results so the model receives structured execution status rather than only concatenated stdout/stderr.

At minimum make exit code and timeout status unambiguous, while retaining bounded stdout/stderr. This may be represented as JSON/text generated from a typed result, but the model must not have to infer command success solely from compiler text.

Do not bypass the existing deterministic acceptance path; worker-invoked bash is still advisory implementation evidence.

## Security and isolation requirements

PR8 must preserve:

- rootless Podman for external-repository execution;
- network disabled for mutation work;
- no target-repository writes outside `allowed_paths`;
- no semantic-tool mutation endpoints exposed to models;
- no access to Rack AI's live checkout when another repository is the target;
- no home/SSH/socket/host credential exposure;
- campaign pause/cancel/timeout behaviour;
- bounded model/tool operations;
- path normalization rules.

Semantic tools must not become a path-policy bypass. Whole-repository read access is acceptable only inside the registered target worktree/repository boundary.

## Mandatory tests

### Unit/integration tests

Prove at least:

1. definition lookup resolves a symbol in a fixture Rust repository;
2. reference lookup returns a caller in another file;
3. diagnostics return a bounded compiler/language-server diagnostic for an intentionally broken fixture;
4. hover/type information works where supported;
5. semantic queries cannot write files;
6. path traversal/unrelated repository access is rejected;
7. timeout/backend failure returns a bounded typed failure;
8. worker tool-call recording includes semantic calls/results;
9. existing write `allowed_paths` enforcement remains unchanged;
10. `bash` result reports explicit exit status/timeout state.

### PR6 regression integration

Extend or reuse the PR7 compatibility-regression fixture.

Prove that semantic evidence can identify the out-of-scope caller/reference and that PR7 recovery uses that evidence to select a strategy that preserves the caller while mutating only permitted implementation paths.

The final accepted diff must not modify the caller.

### Live rack proof

Run an opt-in live-model campaign using actual local endpoints and the semantic backend.

Evidence must show at least one real semantic query invoked by the local model or recovery layer, recorded in the attempt/recovery evidence, followed by a successful bounded implementation/replan path.

## Self-build objective

PR8 is the preferred first attempt for Rack AI to implement a meaningful Rack AI enhancement itself.

The live controller must NOT target its own executing checkout. Use a separate registered clone/check-out of this repository as the target workload. The controller may operate on that separate clone using the same external-repository campaign rules as any other target repository.

No self-modification exception is permitted.

If Rack AI cannot implement PR8 from this contract and a predeclared bounded campaign, preserve the failure evidence and identify the smallest missing capability. Do not immediately replace the self-build attempt with a frontier model unless needed to repair the bootstrap capability.

## Likely code areas

Inspect existing architecture first. Likely areas include:

- `crates/rack_ai_application` worker/coder tool contracts and campaign recovery integration;
- `crates/rack_ai_infrastructure/src/direct_coder_worker.rs`;
- `workspace_coder_tool_runner.rs`;
- new semantic backend adapter(s) in infrastructure;
- Podman/workspace executor integration where needed;
- worker transcript/evidence serialization;
- test fixtures and live smoke scripts;
- configuration/docs for required local semantic backend.

Follow `AGENTS.md` and existing small typed-component style.

## Non-goals

Do NOT add in PR8:

- write/edit authority through LSP/Serena;
- autonomous campaign generation;
- PR5 parallel/adaptive scheduling;
- web/SearXNG research;
- cloud APIs/paid services;
- arbitrary MCP tool exposure;
- another agent framework;
- remote Git promotion;
- broad new escalation behaviour beyond PR7.

## Required validation

At minimum run all existing workspace/smoke coverage plus new semantic-tool and PR6-regression tests:

```bash
cargo test --workspace --offline
bash tests/rack_change_executor_smoke.sh
bash tests/rack_change_implement_smoke.sh
bash tests/rack_change_path_policy_smoke.sh
bash tests/rack_campaign_smoke.sh
```

Also run the opt-in live-model semantic/recovery smoke on the rack.

## Merge gate

Merge PR8 only when:

- PR7 is already merged and PR8 is rebased/updated onto it;
- semantic tools are demonstrably read-only and bounded;
- the PR7 compatibility regression remains green;
- local models successfully use/consume semantic evidence in a live proof;
- no existing path, Podman, network, commit, pause/cancel or evidence invariant regresses;
- the separate-clone self-build experiment is documented with evidence, whether it succeeds or exposes a smaller bootstrap gap.

After PR8 merges, the next milestone is NOT a broader feature PR. It is the frozen original PR6 substantive acceptance rerun.