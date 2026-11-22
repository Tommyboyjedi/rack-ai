# Interactive inference queue

RackAI accepts interactive priority on scoped inference when the caller proves
it knows the configured `RACKAI_INTERACTIVE_QUEUE_KEY`. Keep the same random
key in private service environment files for RackAI and the loopback Hermes
provider proxy. Do not forward that key from Open WebUI or expose it in config
commits. Existing callers without the key remain background requests. Older
background work starts competing with interactive work after 60 seconds.

The proxy uses `Prefer: respond-async` with an idempotency key. A 202 receipt
identifies the invocation. Subsequent GETs to the same reservation capability
return queue state or the original terminal result without posting the prompt
again. This status endpoint is needed on the loopback RackAI tunnel only.
Existing OpenAI clients retain the blocking endpoint behavior by default.

Apply `hermes.patch` at the Hermes source root and `open-webui.patch` at the
Open WebUI backend source root, with `git apply --check` before installation.
The patches target the installed sidecar versions and the configured loopback
Hermes provider and Docker gateway URL. For a different deployment, adapt those
two URLs before applying. Open WebUI marks interactive chat turns. Its internal
tasks omit that marker. Hermes API clients can mark an interactive chat with
`X-RackAI-Interactive-Chat: 1`; a local Hermes CLI in a terminal also receives
priority. Open WebUI renders Hermes queue events as a status on the message.

Install the RackAI runtime first, then the matching sidecar proxy, then the
Hermes and Open WebUI integration. Use a drained cutover. Verify one waiting
interactive turn ahead of queued background work and one older background turn
making progress. Active generation is never interrupted by queue priority.

Run the proxy regression tests with
`python3 -B -m unittest discover -s deploy/runtime/interactive-queue -q`.
