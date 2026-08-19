# Initial State - 2026-08-19

The rack has just moved from manual bring-up and exploratory testing into source-controlled integration work.

Known commissioned services:
- vllm-primary on :8017
- vllm-coder on :8018
- JCode v0.78.1

Current blocker under investigation:
- Qwen2.5-Coder-3B-Instruct-AWQ returns tool intent as plain text JSON rather than OpenAI structured tool_calls.
