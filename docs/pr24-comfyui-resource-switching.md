# PR24 — MVP Workload/Resource Switching with ComfyUI

## Goal
Prove that Rack AI can arbitrate the current rack between application-development work and an interactive ComfyUI workload without becoming or replacing ComfyUI.

## Required behaviour
- Rack AI owns the decision about which GPU/model/service configuration is active.
- An application-development workload can reach a safe drain/pause boundary.
- Rack AI can stop/unload the conflicting model/service required to free the target GPU.
- Rack AI can activate the minimum ComfyUI service/profile needed for interactive use.
- The user continues to use ComfyUI's own front end.
- When the ComfyUI workload ends, Rack AI can reclaim the resource and restore/resume the development configuration.
- Resource ownership and state transitions are observable and fail closed.
- Existing bounded-execution/safety mechanisms must not be weakened.

## MVP constraints
The initial implementation may use explicit known profiles for the current rack rather than a universal model/service registry. Prefer a small correct state machine over premature generalisation.

Rack AI has final scheduling authority. Clients may request urgency/interactive service but do not directly seize GPUs.

## Definition of done
Demonstrate this real sequence on the rack:

1. ATHBA/Rack AI development workload is available/running.
2. A ComfyUI workload is requested.
3. Conflicting development execution drains safely.
4. The required GPU is made available and ComfyUI becomes usable through its normal UI.
5. The ComfyUI workload is released.
6. Rack AI restores the development execution configuration and work can continue.

## Not in scope
- optimal multi-workload scheduling;
- arbitrary service discovery;
- sophisticated preemption;
- all future media/audio workloads;
- long-horizon capacity optimisation.

These are retained in PR25.
