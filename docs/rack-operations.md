# Rack AI Operations

This document defines the unattended operating model for `rack-ai` on `gpurack`.

## Purpose

`rack-ai` is operated as a control-plane process, not as an interactive coding session.

The operational objective is:
- restart safely after the runner process exits;
- restart safely after host reboot;
- reconcile campaigns left in `running` state;
- preserve campaign evidence and fail closed on degraded dependencies;
- clean up stale campaign containers and orphaned repository leases before resume;
- prevent uncontrolled growth of terminal campaign state and auxiliary artifacts.

## Required Configuration

Operational supervision is controlled by [config/operations.json](/srv/rack-ai/config/operations.json).

Current fields:
- `schema_version`: must be `rack-ai/operations/v1`
- `supervisor.scan_interval_seconds`: loop interval for unattended scans
- `supervisor.resume_running_campaigns`: whether the supervisor should attempt durable resume of campaigns left in `running`
- `supervisor.podman_command`: Podman binary used for stale container cleanup during recovery
- `retention.max_terminal_campaign_age_seconds`: age threshold before old terminal campaign state becomes prune-eligible
- `retention.retain_terminal_campaigns`: newest terminal campaigns to keep regardless of age
- `retention.max_auxiliary_artifact_age_seconds`: age threshold before logs/history/change artifacts become prune-eligible
- `retention.retain_auxiliary_artifacts`: newest auxiliary artifacts to keep per supervised directory regardless of age

## Supervisor Command

Use:

```bash
bin/rack-campaign supervise --loop
```

Behavior per scan:
- load and validate `config/operations.json`
- inspect durable campaign state under `state/campaigns/`
- if a `running` campaign still records an active container, stop/remove that stale container id before attempting resume
- attempt `resume` for campaigns still marked `running`
- leave `paused`, `blocked`, `completed`, `cancelled`, and `expired` campaigns untouched
- prune old terminal campaign state/worktrees according to retention policy
- prune old auxiliary logs/history/change artifacts according to retention policy
- remove stale orphan repository lease files with no surviving campaign directory

If worker or executor health is degraded during resume, the existing campaign runner preflight blocks the campaign and preserves evidence. The supervisor does not force progress.

## User-Level systemd

Detached campaigns already use `systemd-run --user --collect`.

For unattended supervision across SSH logout and host reboot:

```bash
loginctl enable-linger "$USER"
systemctl --user daemon-reload
systemctl --user enable --now rack-ai-campaign-supervisor.service
```

Example unit: [docs/examples/systemd/rack-ai-campaign-supervisor.service](/srv/rack-ai/docs/examples/systemd/rack-ai-campaign-supervisor.service)

## Safe Upgrade / Restart

Recommended sequence:

1. `systemctl --user stop rack-ai-campaign-supervisor.service`
2. update the repository and build/test the new revision
3. `systemctl --user start rack-ai-campaign-supervisor.service`
4. inspect `bin/rack-campaign supervise --emit-json` or `journalctl --user -u rack-ai-campaign-supervisor.service`

If a campaign was active when the supervisor or host stopped, its durable state remains on disk. On restart, the supervisor re-inspects `running` campaigns and attempts a bounded resume. Continuity, lease, health, and worktree checks still fail closed.

## Observable States

Operator-facing inspection remains:
- `bin/rack-campaign status <campaign-id>`
- `bin/rack-campaign events <campaign-id>`
- `bin/rack-campaign inspect <campaign-id> --step <step-id>`
- `bin/rack-healthcheck --emit-json`

Expected unattended outcomes:
- healthy dependencies: campaign resumes or remains complete
- unhealthy model endpoint / Podman / continuity failure: campaign becomes `blocked` with durable reason and evidence
- stale recorded container id on a crashed action: supervisor removes it before resume or blocks if cleanup itself fails
- operator pause/cancel: supervisor does not override it

## Retention and Cleanup

The supervisor only removes:
- terminal campaign directories beyond the configured age/keep policy
- their associated worktree `.../campaign-<id>/repo`
- stale recorded campaign containers before a bounded resume
- stale orphan repository lease files whose campaign directory is already gone
- auxiliary artifacts under `logs/runs`, `logs/specs`, `state/runs`, `state/queue/history`, and `state/changes` once they exceed retention policy

It does not delete active campaign evidence.

## Current Boundary

This operating path currently hardens:
- startup/restart behavior
- process crash recovery through durable `resume`
- host reboot recovery through the supervisor loop
- reconciliation of interrupted `running` campaigns
- stale campaign-container cleanup before resume
- bounded retention of terminal campaign state
- bounded retention of auxiliary logs/history/change artifacts
- stale orphan repository-lease cleanup

It still depends on bounded live-rack soak and repeated crash/reboot proving before it should be treated as fully mature for indefinite unattended operation.
