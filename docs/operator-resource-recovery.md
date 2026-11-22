# Operator resource recovery

The authenticated rack launcher provides a Resource recovery panel with Refresh resources and Reset stuck resources. Only the existing operator principal can see/use it. A confirmation explains that active work is preserved and jobs are not rerun.

The action checks every configured resource. Results are per resource, not an assertion that everything was freed:
- Healthy claims, active work and resident warm models are preserved.
- Unresolved work whose owner has not closed the reservation is reported for investigation/owner release.
- Already-closed RecoveryRequired demands retry the existing release/recovery path. Original cancellation/closure intent, uncertain outcomes and claims are retained until ordinary verified cleanup completes.
- Failed warm-cache cleanup is requeued through the existing eviction lifecycle; healthy warm caches remain resident.
- An unclaimed failed configured media unit can have its failed systemd latch reset only after the media-operation lock and canonical authority lock protect checks of unit definition, frozen configuration, GPU identity, no PID/job/populated cgroup/GPU process and no active/pending media session/job.
- An entirely identity-free closed media quarantine can return to Stopped only after the same physical absence checks, no canonical/legacy claim, no session and no pending or cleanup job. Any retained lease/process/generation identity remains a blocker. No assets or history are purged by this operation.
- Unknown owner, changed binding, foreign process or failed probe is a blocker. No arbitrary units, PIDs, commands, reservations or paths are accepted from the browser.

The repeated ComfyUI OOM incident left a failed InvocationID after physical cleanup, preventing a fresh managed start. Resetting that latch repairs admission. It does not increase the RAM cap, change models/workflows or make an oversized render fit. The 1800-second inactivity policy is unchanged.

## Interfaces

GET /api/media/v1/resources returns rack-ai/resource-recovery/v1 with resources, published service tags and generic status codes. Warm-preserved resources also include the optional preserved_service tag from their actual warm-residency record; the launcher displays that service beside the warm status. It is read-only.

POST /api/media/v1/resources/recover accepts exactly:
    {"schema":"rack-ai/resource-recovery/v1","confirm":"recover_stuck_resources"}

Both routes require operator authentication. Existing browser-cookie origin/CSRF checks apply. Non-operators receive 403; missing authentication receives 401. Wrong/extra request fields are rejected. Server-side serialization prevents overlapping recovery commands; busy calls receive 409. A local helper has a 20-second wall bound and bounded output. Partial/failed verification is never success; refresh status after timeout. Ordinary status remains available.

The latest POST result, actor and timestamp replace one bounded resource-recovery-last.json record beside media state. No prompts, model responses, credentials or private paths are returned by these endpoints.

## Architecture and configuration

The media web server cannot depend on runtime (runtime already uses media). A small local rack_ai_resource_admin inspect|recover ADMIN_CONFIG helper owns typed authority access in the runtime crate. It opens no listener and never dispatches inference. The web server invokes only a server-selected hash-pinned executable with fixed arguments.

An optional mode-0600 file STATE_ROOT/resource-recovery.json enables the panel:
    {
      "executable":"/absolute/immutable/release/rack_ai_resource_admin",
      "sha256":"SHA256 of that executable",
      "config":"/absolute/runtime/admin-config.json",
      "receiver_guard":{"sha256":"SHA256 of the qualified running rack_ai_runtime executable"}
    }

The production receiver guard verifies the actual process behind rack-ai-runtime.service. A receiver upgrade requires compatible helper requalification/rebinding. The helper's runtime data contract includes the already-deployed GNU file-pin verifier field and its existing verification behavior; it does not change verifier selection in the running receiver.

The optional binding lives outside frozen model/media profiles, so enabling this operator UI does not rewrite model limits, profile hashes or runtime configuration. Missing configuration hides the panel; current clients remain compatible. Configuration and helper identity failures fail closed.

Lock order remains media-operation before canonical authority. POST mutations reuse existing typed control and cleanup transitions. Inspection uses a typed read snapshot, not a write transaction. Historical records are not revived and no claim is deleted by the reset action.

## Deployment

Build both rack_ai_media and rack_ai_resource_admin into an immutable release. Keep the current inference receiver unchanged; install the private helper binding, then atomically switch only the media release link and restart the media receiver after verifying quiescence. Retain the previous link and binding for rollback. Do not restore old authority snapshots.

Validate with workspace Rust tests, media/API/browser tests and the helper's disposable OOM/foreign-process tests. Verify the public page, non-operator denial and per-resource inventory. Exercise live reset only when its inventory is quiescent or when an operator explicitly intends cleanup; never use a live caller's unresolved work as a test fixture.
