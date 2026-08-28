# PR23 — ATHBA + Rack AI Application-Development Vertical Slice

## Goal
Deliver the first useful end-to-end application-development product using ATHBA as the development-domain application and Rack AI as the rack resource/execution control plane.

## Required behaviour
- ATHBA owns specification, architecture, decomposition, dependencies, ticket complexity and development acceptance.
- Work is decomposed aggressively enough for small local coding models to have a realistic completion target.
- Prefer TDD-oriented tickets where appropriate: establish a failing test/observable contract first, then implement only enough production code to satisfy it while preserving existing tests.
- ATHBA submits work-unit requirements to Rack AI without selecting physical GPUs or hard-coding model identities.
- ATHBA also identifies the wider project/workload so Rack AI has coarse awareness of continuing demand rather than seeing unrelated one-off calls.
- Rack AI chooses the execution resource/model using a deliberately simple initial policy.
- Agentic coding execution continues through JCode.
- Existing Rack AI safety mechanisms remain active: isolation, allowed paths, timeouts, leases, evidence, bounded authority and fail-closed review.
- Rack AI returns execution/evidence/results; ATHBA owns project progression and ticket state.

## Initial routing policy
The first implementation may use a simple deterministic policy appropriate to the current rack. It must be replaceable later and must not leak physical hardware choices into ATHBA.

## Cross-repository work
This vertical slice is expected to require coordinated changes to both `Tommyboyjedi/rack-ai` and `Tommyboyjedi/ATHBA`. Keep domain ownership clean rather than moving ATHBA concepts into Rack AI for convenience.

## Definition of done
Tiny Ticket is successfully built as a real application through the ATHBA → Rack AI → JCode/local-model path.

A safe terminal block demonstrates retained control-plane safety but is not a productivity PASS for PR23.

The result must provide enough evidence to reconstruct which tickets ran, which execution resources were selected, what verification ran, and what was accepted.

## Implemented Rack AI progression primitive

PR23 adds the missing trusted repository progression step required by ATHBA PR12:

- approved work-unit execution now produces `accepted_head_sha`
- the SHA is created under Rack AI Git/worktree authority, not inferred by ATHBA
- rejected/non-approved work does not advance the accepted repository base
- dependent work units can use the returned SHA as the next `repository.base_sha`

This is intentionally the sequential MVP only:

```text
S0 -> A -> S1 -> B -> S2
```

Parallel DAG merge/reconciliation remains out of scope for this PR.

## Runtime isolation repair

Live ATHBA PR13 exposed a real Rack AI isolation defect: target Rust worktrees were being created under `/srv/rack-ai/state/workspaces`, which physically nested them under the Rack AI control-plane Cargo workspace.

That made a natural target acceptance command such as `cargo test` fail in the managed target repository with Cargo's parent-workspace detection:

```text
current package believes it's in a workspace when it's not
workspace: /srv/rack-ai/Cargo.toml
```

The fix belongs in Rack AI, not ATHBA:

- ATHBA should continue to submit natural target-repository commands such as `cargo test`
- Rack AI now treats any configured `workspace_root` nested inside the live Rack AI repository as legacy/unsafe and externalizes it to a sibling runtime root
- production config is now explicit: `/srv/rack-ai-workspaces`

This keeps target worktrees operationally separate from the Rack AI control repository without adding Rust-specific command rewriting or Tiny Ticket-specific workarounds.

Live proof on August 28, 2026 used the real ATHBA coordinator/gateway path with workload `pr13-live` and the existing Tiny Ticket fixture:

```text
X = d0fb9cff096ef6e9a6d38c854e0f97e22a7f5771
WU-A accepted -> Y = 05109113fdd7658a3b5c306b86c6690917d108a8
WU-B from Y accepted -> Z = b628c9945d3c0112afa862bcc61826ce9193e229
```

The important runtime proof is that the temporary live config deliberately pointed `workspace_root` back under `/tmp/rack-ai-pr23/state/workspaces`, but Rack AI executed the real accepted work units in `/tmp/rack-ai-pr23-workspaces/...` instead. `cargo test` therefore ran in the target repository context rather than inheriting `/tmp/rack-ai-pr23/Cargo.toml`.

## Not in scope
- universal/adaptive scheduler;
- optimal GPU utilisation;
- full model registry;
- all future workload types;
- OddesyAgent integration;
- sophisticated long-horizon forecasting.

Those belong to the post-MVP roadmap.
