# Codex handoff v2 — one coordinated end-to-end task

Revision: 2026-09-12. Replaces the earlier CLI-only/two-milestone prompt. Paste the prompt below into the existing local Codex environment that has authorized access to gpurack. Use the user's selected model/reasoning setting. One task covers both repositories; no guaranteed duration or single-run completion is implied.

Before leaving it unattended, let its initial access check finish and handle any genuinely required scoped access approvals. A prompt cannot grant OS/service-manager/Git/network permissions. Do not disable safeguards or bypass host-key validation to remove approval prompts.

## Implementation prompt

```text
Implement the complete ComfyUI user journey in one coordinated task. Do not stop at a plan, CLI, unused API client or scaffolding. The contracts are written; execute them, test, self-review/fix, commit/push and perform the authorized live qualification.

Repositories and existing PR branches:
- Tommyboyjedi/rack-ai, PR24, roadmap/pr24-comfyui-resource-switching.
- Tommyboyjedi/musicvideo-director, PR33, integration/rack-ai-media.
This is Music Director PR33, NOT Rack AI's heavyweight-inference PR33.

Start with an access check: use the existing authorized ssh tomp@gpurack route, verify Git access to BOTH repositories, isolated writable worktree/install paths and service-manager permissions. Rack build/test/service work runs on gpurack, not a replacement NUC runtime. Report any required approval immediately; never bypass permissions, host-key checks or authentication. Do not reveal credentials.

Follow each repository's applicable agent/coding rules. Fetch current refs, inspect local changes and prepare clean isolated worktrees for both branches, retaining their documentation. Incorporate current origin/main by ordinary merge as needed. Do not switch/reset the working /srv/rack-ai checkout, force-push or merge either PR into main. Clone the second repository into an isolated workspace if needed and already authorized.

Read once, then use targeted source inspection:
- Rack AI: docs/pr24-comfyui-resource-switching.md, revision v2 complete user journey (authoritative), and docs/pr24-code-review.md (historical source map).
- Music Director: docs/rack-ai-media-integration.md.
The contracts are on the PR branches, not necessarily main. The old CLI-only exclusions are superseded. I authorize this specific companion Music Director change under its own rules; do not modify ATHBA or put private application data/code into the public Rack AI repo.

Deliver ALL four internal milestones without asking me to approve routine design choices:
1. Dedicated 4080 ownership, isolated ComfyUI lifecycle, recovery and native submission fencing.
2. Durable bounded image jobs, one approved local image workflow, safe replay/cancellation and verified artifacts.
3. Always-available authenticated private receiver/API and a small browser launcher: Start, status, Open normal ComfyUI, Finish session. Return the actual usable URL after deployment.
4. Wire Music Director's real image Generate action to Rack AI, including settings/Test connection, durable asynchronous progress and exact image import into the correct project. Generation must start ComfyUI automatically; preserve the direct/VastAI backend.

Keep the 2060/4060 Ti vLLM services, existing development runtime, databases, media and unrelated local changes intact. Verify UUIDs; never guess them. No whole-stack Compose operations, driver updates, automatic GPU reassignment, heavyweight inference, paid generation, model collections or additional LLM/subagent swarm calls. Use existing compatible model assets. Prepare only isolated candidate services/environments and disposable application test data; do not overwrite the user's running application or production database.

Follow the contracts' resource/authentication/CSRF/native-UI/recovery/idempotency/artifact safeguards. Implement shared schema fixtures and actual cross-repository HTTP/browser tests, not just mocked clients. Use fake GPUs/workers and temporary resource roots for dangerous cases. Never run the legacy lease-deleting resource smoke against live state. Run focused tests, full applicable suites and a final self-review/fix pass. Keep progress brief and persist a concise completion checklist/evidence paths for recovery from interruption.

Deploy and qualify the new media services through the existing authorized private access path when permitted. Prove native UI use and Music Director Generate from a stopped ComfyUI backend with a real local image, correct project import, duplicate prevention and verified resource release. A disposable candidate test is not proof the existing client installation was updated. Missing access/model/permission must be reported precisely while you complete all independent code/tests/runbooks; never invent a pass.

Commit/push scoped work to both existing PRs and update their status; do not merge. Final report: both SHAs/PRs, CODE_COMPLETE, FIXTURE_E2E_PASSED, RACK_LIVE_QUALIFIED, DIRECTOR_LIVE_QUALIFIED, deployed SHAs, actual launcher URL or NOT_DEPLOYED, non-secret connection/activation instructions, exact tests and blockers, stop/rollback instructions. Do not call the task complete while required paths remain TODOs or live evidence is missing; distinguish implemented, tested and deployed.
```

## Only after an actual interruption or remaining prerequisite

Resume the same task/thread and retained work rather than launching a duplicate implementation. A short continuation is sufficient:

```text
Continue the PR24 v2 + musicvideo-director PR33 task from the current branches and retained checklist/evidence. Do not restart research or redesign completed work. Finish only the remaining implementation/test/deployment gates, preserving all safety rules and running services; push scoped corrections to the same PRs, do not merge, and update the separate completion/qualification fields honestly.
```
