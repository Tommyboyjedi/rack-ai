# PR28 — Trusted Dynamic Workspaces

## Goal

Provide a generic Rack AI trust mechanism for bounded agent execution against dynamically created project repositories (such as ATHBA-generated projects) without requiring callers to edit Rack AI configuration manually.

## Boundary

Rack AI remains language- and framework-agnostic. It owns generic agent execution, resource/model selection, JCode integration, isolation/worktrees, process execution, timeouts, policy enforcement, stdout/stderr, evidence, crash recovery, and resulting revisions.

ATHBA owns software-development semantics: project creation, runtime/toolchain/dependency definitions, test/build commands, TDD progression, architecture, Gatekeeper/Reviewer logic, and deciding what work should happen next.

Rack AI must not learn Python, pytest, ReservationBook, or ATHBA-specific semantics.

## Problem

ATHBA creates new repositories dynamically. Rack AI currently rejects bounded work against an unknown repository before execution:

`repository <id> is not registered`

ATHBA must not solve this by editing Rack AI repository configuration manually.

## Required capability

Design and implement a generic trusted-execution mechanism that allows an authorized caller such as ATHBA to submit bounded agent work against dynamically created repositories while preserving Rack AI's trust/isolation boundary.

Do not assume explicit per-repository registration is necessarily the correct solution. Evaluate the existing repository registry and trust model first. A pre-authorized project root, generic workspace authorization, registration interface, or another small language-neutral mechanism may be preferable.

## Constraints

- No language/framework-specific logic.
- No ATHBA-specific software semantics.
- Preserve fail-closed behavior.
- Prevent path traversal or execution outside authorized roots.
- Existing statically registered repositories must continue to work.
- Keep the change small and generic.
- Do not modify ATHBA.

## Proof target

A fresh repository created beneath an approved caller-owned project area can be used for one harmless bounded Rack AI agent execution without manually changing Rack AI configuration, while an unauthorized path/repository remains rejected.

## Non-goals

- Environment provisioning.
- Python/pytest support.
- Software ticket decomposition.
- TDD orchestration.
- Gatekeeper/Reviewer semantics.
- Replacing Rack AI's generic execution/worktree/evidence machinery.
