# PR18 Supporting Note — Rust-Native Operator / Workspace Layer Evaluation

## Status

Future roadmap research only. Not an implementation contract and not part of PR14–PR17.

## Why this exists

Odysseus highlighted a useful class of capability that Rack AI may eventually need for operator-facing use: chat/workspace UI, multi-channel access, memory, notifications, research/tool UX and remote interaction.

However, Rack AI is intentionally pursuing a Rust-first trusted stack. If a future commercial or operator layer is needed, prefer a Rust-native project before introducing Python. If Python ever becomes justified, Django is the preferred ecosystem rather than FastAPI-style application sprawl.

## Permanent ownership decision

Do **not** move model discovery, model selection, GPU/resource placement, runtime capability registration or scheduling out of Rack AI merely because an operator-layer project offers similar features.

Those are strategic Rack AI control-plane capabilities and remain Rack AI-owned.

The operator/workspace layer should be a client of Rack AI, not a second control plane.

## Odysseus position

Odysseus is useful as a product/reference comparison for a future commercial workspace, but it is not currently a preferred dependency because its trusted backend stack is Python/FastAPI and its scope overlaps too broadly with Rack AI's own control-plane opportunities.

If revisited later, evaluate it primarily for UX/product ideas, not for model placement or rack orchestration.

## Rust-native projects worth evaluating later

### 1. Moltis

Position: strong candidate for a future Rust-native operator/gateway layer.

Relevant characteristics:
- single Rust binary;
- built-in web UI;
- persistent agent/session server;
- multi-provider LLM support;
- Telegram, Discord, WhatsApp and other channels;
- MCP/tool support;
- local-first deployment.

Why relevant:
Moltis could potentially provide the human-facing gateway/chat/channel layer while Rack AI remains the source of truth for models, GPUs, routing and campaign state.

Decision line:
Use Moltis only if it can consume Rack AI APIs/events without becoming authoritative for model placement, harness routing, target authority, acceptance or promotion.

### 2. thClaws

Position: strong reference/candidate for Rust-native operator workspace and possibly agent UI.

Relevant characteristics:
- native Rust agent workspace;
- GUI, CLI, non-interactive and webapp surfaces from one engine;
- generic OpenAI-compatible provider support including vLLM/internal proxies;
- files/terminal/chat surfaces;
- MCP, skills, plugins and agent teams;
- MIT/Apache-2.0 licensing.

Why relevant:
thClaws demonstrates that a polished operator workspace does not require Python/Node as the trusted backend. It may be useful either as a client/workspace above Rack AI or as a source of reusable ideas/components.

Decision line:
Do not use thClaws to replace Rack AI orchestration, model/GPU placement or acceptance logic. Evaluate only the operator/workspace boundary unless later evidence justifies more.

### 3. MIRA

Position: broad Rust-native personal-assistant/operator platform; useful reference, potentially too broad for near-term Rack AI needs.

Relevant characteristics:
- Rust core;
- web and terminal interfaces;
- Telegram, Signal, Discord, Matrix, Slack, email and other channels;
- audit trail, memory, automations and sandboxed tools;
- local/OpenAI-compatible model support;
- AGPL-3.0 licensing.

Why relevant:
MIRA covers many future PR18 operator-channel/memory/automation ideas and shows they can remain Rust-centric.

Decision line:
Its breadth and AGPL obligations make it a larger strategic commitment. Prefer narrower integration unless Rack AI later needs a full assistant/product layer.

### 4. croit/llm-gateway

Position: infrastructure/gateway candidate rather than a full operator workspace.

Relevant characteristics:
- Rust single-binary gateway;
- OpenAI-compatible front door;
- multi-backend routing, health checks and failover;
- built-in chat UI;
- OIDC/RBAC and per-user tokens;
- MCP/tool execution and scheduled actions.

Why relevant:
Could be useful if Rack AI later needs a standardized external API/authentication gateway for multiple users or machines.

Decision line:
Do not delegate hardware-aware scheduling or model selection to it by default. Use only as an API/auth/access layer if that becomes an operational need.

### 5. Lightweight Rust UI references

Projects such as Overlooked and OpenYoke show that simple local OpenAI-compatible chat/frontends can also be delivered with Rust-native/Tauri/Dioxus approaches.

These are useful references if Rack AI eventually needs a modest first-party UI rather than adopting a broad assistant platform.

## Future decision criteria

Revisit after PR17 or when operator friction becomes material. Before choosing an operator/workspace layer, assess:

1. Rust-native trusted backend/runtime.
2. Ability to consume Rack AI APIs/events instead of duplicating state.
3. Clean local OpenAI-compatible/vLLM integration where needed.
4. Authentication and multi-user needs.
5. Web/desktop/mobile/channel requirements.
6. Auditability and local-first operation.
7. Licensing implications for future commercial deployment.
8. Whether integration reduces Rack AI code rather than introducing a second orchestration system.
9. Whether model/GPU selection can remain fully Rack AI-controlled.

## Current recommendation

No operator/workspace project should be adopted now.

Continue PR14–PR17 first.

When the operator layer becomes active, evaluate Rust-native candidates before Odysseus. Current research priority order for that future evaluation:

1. Moltis — gateway/channel/operator layer.
2. thClaws — richer Rust-native workspace/UI reference or client.
3. croit/llm-gateway — external API/auth gateway if required.
4. MIRA — broad assistant/platform option if product scope expands substantially.
5. Odysseus — UX/product reference or commercial comparison, not preferred control/runtime dependency.

## Standing rule

Rack AI keeps ownership of model discovery, model capability metadata, GPU/resource placement, vLLM runtime registration, harness routing and scheduling. External operator software may display or request those capabilities, but it must not become the authoritative source of truth for them.
