# Passive rack energy history

The authenticated operator launcher includes Energy usage, with live/latest sample watts, stored kWh, recorded coverage and history plots for 24 hours, 7 days, 30 days and 365 days. GPU traces use solid lines; the estimated rack total uses a dashed line. Sampling and retention are independent of reservations, inference and media sessions.

## Measurement boundary

Each GPU uses NVIDIA NVML accumulated device-energy counters. Counter deltas provide GPU kWh; these are device telemetry, not a mains electricity meter. CPU uses only package energy domains from Linux powercap. Core subzones overlap the package and are never added again.

The total is explicitly an uncalibrated estimate:
(GPU energy + CPU package energy + assumed other-component energy) / assumed PSU efficiency.

Initial provisional assumptions are 50 W for unmeasured RAM, disks, motherboard and fans, and 85% PSU efficiency. Neither value is a rack measurement or an accuracy guarantee. They may be inaccurate for this hardware/load. The UI shows both assumptions and the formula. CPU telemetry is counter-derived, GPU telemetry is measured-device, and total is estimated. No nominal GPU power limits/TDP values are used as measured consumption.

Missing CPU/GPU counters make the total unavailable for that interval, rather than undercounting a partial component sum. Device changes, counter resets, reboots, collection gaps over 90 seconds, and wall-clock changes do not generate invented history. CPU wraparound is accepted only inside a conservative single-wrap envelope. History coverage is explicit. Snapshot age over 90 seconds is stale. First startup needs a full sample interval.

## Collection and persistence

tools/energy/collector.py is a separate passive Python standard-library service. No added pip dependencies, network listeners, model calls, CUDA allocations, reservations, workload control or runtime configuration changes. It samples every 30 seconds. Root access is confined to this hardened collector for restricted CPU counters; the web receiver remains unprivileged and CPU sysfs permissions are unchanged.

The root-owned immutable collector release is installed under /opt/rack-ai-energy/releases. The root-owned /etc/rack-ai-energy.json is typed and validated. StateDirectory /var/lib/rack-ai-energy holds a private SQLite database and atomic, bounded public.json snapshot readable by the media receiver's group. Raw checkpoints (boot IDs/counters) are never published. SQLite transactions keep checkpoints and minute/hour/day rollups consistent; restarts within a short valid interval resume from the durable checkpoint. A single collector lock prevents duplicate accounting.

Defaults retain minute buckets for 30 days, hourly buckets for 400 days and daily buckets for 1825 days. SQLite has a 128 MiB page bound, a 4 MiB WAL target/checkpoints, and hourly retention cleanup. Public snapshots are bounded to 1 MiB. Snapshot failure retains committed history. Stored history survives service restarts, reservation release and immutable code deployments. Estimate methodology is frozen with the database; changing assumptions requires an explicit versioned/history migration, rather than silently relabelling earlier estimates.

## Website and interface

A private optional STATE_ROOT/energy.json binding contains only:
{"snapshot":"/var/lib/rack-ai-energy/public.json"}

GET /api/media/v1/energy is operator-only, read-only, schema rack-ai/energy/v1. Missing auth is rejected, other owners receive 403. Errors do not reveal private paths or raw counters. GET status advertises energy_supported only behind existing authentication. Unconfigured deployments keep the panel hidden. Existing media/runtime contracts and frozen model profiles remain compatible.

The snapshot contains current readings, classification, formula/assumptions, collection start/latest timestamps, warnings, retention, and bounded history series with kWh/coverage. History ranges use aligned buckets, and the plot shows the actual range. Missing intervals are gaps, not zero measurements.

## Deployment and rollback

Run deterministic collector tests, API/browser tests and workspace gates before independent local coordinator review. Install root-owned collector files and the hardened system unit; retain the previous immutable media release/helper binding. Enable the passive collector, verify advancing counters and persisted rollups, then switch only the quiescent media receiver. Never restart inference, change models/limits or interrupt a live session.

Rollback switches the prior media link/bindings and disables the collector if necessary; retain /var/lib/rack-ai-energy history. Never restore old resource-authority snapshots. No reset action or render is needed for energy qualification.
