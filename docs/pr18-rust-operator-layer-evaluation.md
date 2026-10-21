# PR18 Supporting Note — Rust-Native Operator / Workspace Layer Evaluation

Future roadmap research only. Not an implementation contract and not part of PR14–PR17.

Odysseus highlighted a useful future capability class: chat/workspace UI, multi-channel access, memory, notifications, research/tool UX and remote interaction. Rack AI is intentionally pursuing a Rust-first trusted stack. Prefer Rust-native operator/workspace software before introducing Python; if Python later becomes justified for a commercial application layer, Django is preferred.

Permanent ownership rule: model discovery, model selection, GPU/resource placement, runtime capability registration, vLLM runtime registration, harness routing and scheduling remain Rack AI-owned. An operator/workspace layer is a client of Rack AI, not a second control plane.

Odysseus remains a useful UX/product reference for future commercial deployment, but is not a preferred current dependency because its backend stack is Python/FastAPI and overlaps too broadly with Rack AI control-plane opportunities.

Rust-native candidates to evaluate later:

- **Moltis** — Rust single-binary persistent agent/gateway with web UI, channels, MCP and local-first deployment. Strong future operator/gateway candidate if it can consume Rack AI APIs/events without becoming authoritative for placement, routing, acceptance or promotion.
- **thClaws** — native-Rust GUI/CLI/headless/web workspace with generic OpenAI-compatible/vLLM support, files/terminal/chat, MCP, skills and plugins. Strong workspace/reference candidate; do not use it to replace Rack AI orchestration or hardware/model authority.
- **croit/llm-gateway** — Rust single-binary OpenAI-compatible gateway with chat UI, health/failover, OIDC/RBAC and MCP/tool support. Potential future external access/auth layer, not hardware-aware scheduler.
- **MIRA** — broad Rust-core assistant with web/TUI/channels/memory/automations and audit trail. Useful reference, but broader scope and AGPL licensing make it a larger strategic commitment.
- Lightweight Rust UI references such as **Overlooked** and **OpenYoke** if a modest first-party interface becomes preferable.

Future decision criteria: Rust-native trusted runtime; ability to consume Rack AI APIs/events; authentication/multi-user requirements; channel/UI needs; auditability; local-first operation; licensing/commercial implications; whether adoption reduces rather than duplicates Rack AI code; and preservation of Rack AI model/GPU authority.

Current recommendation: adopt none now. Continue PR14–PR17. When operator friction becomes material, evaluate Rust-native options before Odysseus.
