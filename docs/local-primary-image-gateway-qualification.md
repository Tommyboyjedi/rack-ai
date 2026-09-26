# Local-primary image gateway: review and qualification

## Status and purpose

Prepared on 2026-09-26 from main `d06e14f71d0f8a3fa99aa9f99935f7d245879c3b`.
The initial commit on `codex/local-primary-image-gateway` contains this review
checklist only. It does not contain the rack-side implementation, certify that
implementation, or change the deployed runtime. Codex will add the reviewed
implementation to this same branch when ready and update the status/evidence.

This is a focused extension of the existing managed inference interface, not a
new service, scheduler, execution authority, or client-application integration.
Follow [AGENTS.md](../AGENTS.md), the [engineering contract](engineering-contract.md),
[reservations and work](reservation-work.md), and the
[resource accounting policy](resource-accounting-retirement-policy.md).

## Required execution boundary

```text
Client with a reservation-provided gateway
  -> scoped chat/completions gateway
  -> inference::Submission
  -> canonical runtime dispatch
  -> reservation-owned local-primary backend
```

Image calls must use the existing ownership, qualification, profile/generation,
idempotency, queue, cancellation, pre-emption, uncertainty and cleanup machinery.
Do not weaken EndpointFence or add a fallback directly to port 8017. Keep
text-only rack-primary behavior unchanged. Generic visual capability alone must
not authorize image input: use the reservation's frozen image-input policy.

Chat Completions is the required image interface. Document Responses support
only if implemented and qualified; do not add it just to widen this change.
No additional tool, model placement, token/output-limit or scheduling change is
approved by this checklist. Preserve protected response and accounting behavior.

## Reported rack-side evidence, not independently verified here

The operator supplied a Codex report for `/srv/rack-ai` on `gpurack` identifying
release `image-gateway-20260926T130631Z` and host-local evidence:

```text
/srv/rack-ai/evidence/local-primary-image-gateway-20260926T130631Z/live-gateway-proof.json
```

The report describes a real Ready reservation, red image -> `red`, blue image ->
`blue`, text-only prompt -> `text-ok`, invalid image ->
`409 unsupported_image_type`, and released/stale access ->
`409 reservation_not_dispatchable`. It reports no active demands or queued/running
invocations at the end of that run, not a claim about the rack's current state.
These are basic gateway and image-dependence smoke checks, not a general vision
accuracy benchmark.

The report also lists passing formatting/diff checks, scoped image-client tests,
CLI task-image tests, runtime Rust tests and `tests/runtime/test_image_gateway.py`.
Read the retained evidence and attach source/binary/profile provenance before
using those results as qualification of a subsequently pushed commit.

## Final review gates

- [ ] Isolate the feature-only diff, including new source/test files; inspect
  mixed files at hunk level and exclude pre-existing unrelated work.
- [ ] Reconcile discovery, reservation inspection, schemas, embedded contract
  documentation and examples with actual supported protocols, formats, source
  types, error behavior and disabled/default profile behavior.
- [ ] Document actual max_images_per_request and max_image_bytes values and
  whether byte limits apply per decoded image or in aggregate. Check bounded
  HTTP request allowance, base64 expansion, text/envelope overhead and relevant
  proxy limits together. Keep transport bytes separate from input-token limits.
- [ ] Test near-limit/over-limit image bytes and image counts, malformed base64,
  corrupt image data, mismatched declared formats and image input on a text-only
  profile. Invalid inputs must not dispatch. Verify bounded decode allocation
  and document dimension/pixel limits and their enforcement point.
- [ ] Verify the actual source policy. Encode local CLI files client-side; do
  not accept arbitrary server filesystem paths or unrestricted server-side URL
  fetching. Do not publish raw image payloads, credentials or scoped access URLs
  in logs, examples or PR evidence.
- [ ] Prove same explicit key + same payload reconciles one invocation; changed
  payload conflicts; distinct explicit keys permit deliberate identical calls.
  Retries must not silently generate another invocation after dispatch.
- [ ] Verify stale/released access rejection and preserve pre-emption,
  cancellation, uncertainty and text-only behavior. Audit every task/DAG image
  path for a managed route with no direct-port fallback.
- [ ] Run targeted tests, image HTTP regressions, cargo fmt --check,
  cargo check --workspace --offline, cargo test --workspace --offline and
  git diff --check on the intended feature-only candidate.
- [ ] Complete the repository-required independent semantic review after
  deterministic checks. Record unavailable or failed gates honestly.
- [ ] Reconcile the final commit with the live-tested release using source HEAD,
  feature patch/new-file hashes, binary identity and profile identity. Identify
  changes not covered by retained live evidence; do not relabel old evidence.

## Codex publication handoff

Fetch and build on `origin/codex/local-primary-image-gateway`; preserve this
preparation commit. Use an isolated review worktree as needed so unrelated
changes in the dirty deployment checkout cannot enter the PR or mask failures.
Do not switch/reset the dirty checkout destructively, bulk-stage it, clean it,
force-push, or rewrite published history. Preserve its existing index and
unrelated `compose.yaml`, `config/repositories.json`, worktrees, runtime state,
model files, local deployment artifacts and raw evidence.

When the intended candidate is ready, commit and push only the image feature to
this branch and update the existing draft PR with the exact changed-file list,
checks/review outcome, limits/source policy and verified qualification status.
This publication step supersedes the earlier preparation-only stop instruction;
it does not authorize a merge, auto-merge or another deployment.

Any additional live check must use available capacity through a managed
low-priority reservation without pre-empting other work or restarting services.
Release only the test's own resources. If new code needs deployment to qualify,
report that separately rather than changing the live service under this task.
