# Honcho Shared Bridge adapter

This sidecar adapter resolves the current RackAI Shared Bridge reservation and
forwards Honcho Chat Completions and Responses requests to its scoped gateway.

The adapter sends a 120-second RackAI execution budget by default. Override it
with `--execution-timeout SECONDS` for the adapter process. RackAI starts this
clock at dispatch and enforces the selected profile's upper limit. The existing
`--timeout` is the separate HTTP response wait and includes time in the queue.
The execution budget is also sent on the existing reservation invalidation
retry. Transport and inference failures do not trigger automatic replay here.

Deploy the RackAI runtime that supports `RackAI-Execution-Timeout-Seconds` first.
On sidecar, install `proxy.py` as
`~/.local/share/honcho-rackai-bridge/proxy.py` and restart the existing
`honcho-rackai-bridge.service` once its traffic has drained. Existing unit
arguments can be retained. The default applies without a unit change.
Verify the adapter against the active scoped RackAI gateway after rollout.

Run the local stub tests with `python3 -B -m unittest -q` from this directory.
