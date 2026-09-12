# PR24 Codex handoff

Use one implementation request for both milestones. The source-controlled contract carries the detailed decisions and acceptance cases; the prompt should not duplicate that entire document. No particular model's token usage or one-run completion is guaranteed.

## Implementation prompt

```text
Implement Rack AI PR24 end to end: dedicated ComfyUI service ownership plus one bounded managed image workflow. This is an implementation task, not another design proposal. Complete both milestones and their tests in this run where the environment permits.

Repository: Tommyboyjedi/rack-ai
PR: 24
Head branch: roadmap/pr24-comfyui-resource-switching
Contract: docs/pr24-comfyui-resource-switching.md
Source review/map: docs/pr24-code-review.md

Respect the current checkout's AGENTS.md before operating. Fetch current refs, check ancestry and local worktree status, and prepare the PR24 branch in an isolated clean worktree. Incorporate current origin/main through an ordinary merge, preserving PR24's documentation and all unrelated/uncommitted work. The reviewed main was e197079c26cd0d0cb0fb2a85ba5a2af605c244b8; PR32 is already incorporated. Do not force-push, reset a live checkout, revive old PR stacks, or merge PR24 into main. In the prepared worktree, read the applicable mandatory documents and the PR24 contract/review once before implementation; those PR24 files are not present on the older main checkout.

Use the source map for targeted inspection, not another whole-repository research sweep. Implement the contract in two internal milestones: A, correct resource ownership and native interactive ComfyUI lifecycle; B, typed managed image submission, reconciliation, artifacts and durable outcomes. Do not stop at milestone A, scaffolding, documentation, or mocked success. Preserve the existing Rust architecture/coding rules and workspace v1/v2 behavior; add focused collaborators, not a new monolithic manager.

Non-negotiable: keep the 2060/4060 Ti development services and ATHBA untouched. Use verified UUID placement for the dedicated 4080; do not invent its UUID. No automatic GPU reassignment, vLLM restart, whole-stack Compose operation, driver/dependency upgrade of existing runtimes, heavyweight inference, universal scheduler, frontend replacement or video/audio automation. A busy/uncertain GPU waits or fails closed.

Implement owner-safe canonical reservations, recovery, native submission fencing including in-flight requests, bounded service control and the separate image execution path specified in the contract. ComfyUI history is not durable and repeating a prompt UUID does not deduplicate execution. Do not weaken those safeguards to shorten the task.

Work efficiently: use fixtures rather than additional local/hosted LLM calls for development tests; no paid inference or subagent swarm. Run targeted tests while developing, then the full workspace suite and one final diff review. Keep progress reports brief. Audit legacy smoke scripts before running them: the existing resource-admission smoke deletes a gpu-2060 lease and must never run unchanged against live state. Tests use temporary job AND resource roots and fake workers/GPU probes; include real middleware/HTTP race tests.

When authorized gpurack access exists, perform read-only preflight first. You may prepare an isolated ComfyUI environment and start/stop only the newly owned ComfyUI service. Reuse available local model assets; do not download model collections or alter existing environments. Perform the contract's real image/non-interference/release proof only when prerequisites are satisfied. Missing access, permissions or model assets must not stop completion of code, fixture tests and the runbook; record the exact live gate instead of fabricating success.

Commit and push the scoped work to PR24 without rewriting history. Keep the PR draft until live acceptance is evidenced. Update its implementation/qualification status. Final response: commit SHA; concise changes; exact tests/results; CODE_COMPLETE and LIVE_QUALIFIED separately; remaining blockers; exact commands to open ComfyUI, submit the example, inspect and release. Do not merge.
```

## Only when a first run leaves a concrete blocker

Use this continuation only after resolving the stated prerequisite or to repair an actual failing test. Do not start a second design/review cycle by default.

```text
Continue PR24 from its current head and retained evidence. Read the implementation status and docs/pr24-comfyui-resource-switching.md. Do not redesign or restart completed work. Resolve the specifically recorded remaining implementation/test/live-qualification gates, run the affected regression checks, and finish the real ComfyUI image/coexistence/release proof when authorized prerequisites are present. Preserve working development services, local changes, and all ownership/safety rules. Commit/push only scoped corrections to the same PR; do not merge. Report the new SHA, exact evidence, CODE_COMPLETE, LIVE_QUALIFIED, and any remaining blocker honestly.
```
