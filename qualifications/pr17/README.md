# PR17 unattended qualification

PR17 uses two disposable product builds to qualify Rack AI's real unattended campaign path. The target repositories are created under `/tmp/rack-ai-pr17-qualification`; Rack AI and AdaptOS are never used as mutation targets.

## Campaign 1: tiny-ticket

Build a small Rust command-line ticket tracker with no external dependencies.

Required behaviour:
- `tiny-ticket create <store> <title...>` creates an open ticket with the next numeric id and prints `created <id>`.
- `tiny-ticket list <store>` prints one line per ticket as `<id>|<open|closed>|<title>` in id order.
- `tiny-ticket close <store> <id>` closes an existing ticket and prints `closed <id>`.
- Data persists in the supplied store file.
- Invalid ids, empty titles, or titles containing `|` fail cleanly.

The campaign is split into domain, persistence, CLI, and final-documentation steps. Acceptance scripts are pre-seeded in `tests/` and are outside the campaign's allowed mutation paths.

## Campaign 2: tiny-dodge

Build a dependency-free browser game using HTML, CSS and JavaScript.

Required behaviour:
- A visible player moves left/right with keyboard controls.
- Falling obstacles are animated with `requestAnimationFrame`.
- Collision ends the game.
- Survival increases an on-screen score.
- Start/restart control resets the game.
- The game uses no external URLs or libraries and can be opened directly from `index.html`.

The campaign is split into page structure, game logic, styling, and final-polish steps. Acceptance scripts are pre-seeded in `tests/` and are outside allowed mutation paths.

## Prepare

From the PR17 checkout:

```bash
bash qualifications/pr17/setup.sh
```

This creates the disposable repositories, isolated Rack AI state/config, and renders both campaign JSON files with the correct base SHAs.

## Run

Ticketing campaign:

```bash
cargo run -q -p rack_ai_cli -- campaign start /tmp/rack-ai-pr17-qualification/campaigns/tiny-ticket.json --repo-root /tmp/rack-ai-pr17-qualification/rack --state-root /tmp/rack-ai-pr17-qualification/rack
```

Game campaign:

```bash
cargo run -q -p rack_ai_cli -- campaign start /tmp/rack-ai-pr17-qualification/campaigns/tiny-dodge.json --repo-root /tmp/rack-ai-pr17-qualification/rack --state-root /tmp/rack-ai-pr17-qualification/rack
```

Run them one at a time for the first qualification. Do not intervene after start unless the campaign reaches a terminal state.

Inspect afterwards with:

```bash
cargo run -q -p rack_ai_cli -- campaign status pr17-tiny-ticket --emit-json --repo-root /tmp/rack-ai-pr17-qualification/rack --state-root /tmp/rack-ai-pr17-qualification/rack
cargo run -q -p rack_ai_cli -- campaign status pr17-tiny-dodge --emit-json --repo-root /tmp/rack-ai-pr17-qualification/rack --state-root /tmp/rack-ai-pr17-qualification/rack
```

Built worktrees are under `/tmp/rack-ai-pr17-qualification/workspaces/` and campaign evidence is under `/tmp/rack-ai-pr17-qualification/rack/state/campaigns/`.
