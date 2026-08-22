# Rack AI Operations

This document defines the unattended operating model for `rack-ai` on `gpurack`.

## Purpose

`rack-ai` is operated as a control-plane process, not as an interactive coding session.

The operational objective is:
- restart safely after the runner process exits;
- restart safely after host reboot;
- reconcile campaigns left in `running` state;
- preserve campaign evidence and fail closed on degraded dependencies;
- prevent unbounded buildup of stale terminal campaign state.

## Required Configuration

Operational supervision is controlled by [config/operations.json](/srv/rack-ai/config/operations.json).

Current fields:
- `schema_version`: must be `rack-ai/operations/v1`
- `supervisor.scan_interval_seconds`: loop interval for unattended scans
- `supervisor.resume_running_campaigns`: whether the supervisor should attempt durable resume of campaigns left in `running`
- `retention.max_terminal_campaign_age_seconds`: age threshold before old terminal campaign state becomes prune-eligible
- `retention.retain_terminal_campaigns`: newest terminal campaigns to keep regardless of age

## Supervisor Command

Use:

```bash
bin/rack-campaign supervise --loop
```

Behavior per scan:
- load and validate `config/operations.json`
- inspect durable campaign state under `state/campaigns/`
- attempt `resume` for campaigns still marked `running`
- leave `paused`, `blocked`, `completed`, `cancelled`, and `expired` campaigns untouched
- prune old terminal campaign state/worktrees according to retention policy
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
- operator pause/cancel: supervisor does not override it

## Retention and Cleanup

The supervisor only removes:
- terminal campaign directories beyond the configured age/keep policy
- their associated worktree `.../campaign-<id>/repo`
- stale orphan repository lease files whose campaign directory is already gone

It does not delete active campaign evidence.

## Current Boundary

This operating path currently hardens:
- startup/restart behavior
- process crash recovery through durable `resume`
- host reboot recovery through the supervisor loop
- reconciliation of interrupted `running` campaigns
- bounded retention of terminal campaign state
- stale orphan repository-lease cleanup

It does not yet provide durable container-id tracking or automatic cleanup of orphaned live Podman containers after an unclean host/process failure. Until that exists, unattended operation should be treated as bounded but not fully self-healing at the container layer.
