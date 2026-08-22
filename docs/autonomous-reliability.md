# Autonomous Reliability Iteration

This branch/PR is the integration bucket for narrow reliability improvements discovered through substantive autonomous campaigns after the P0/P1 merge.

Changes belong here when real campaign evidence exposes a concrete blocker or when a bounded change materially improves autonomous task success while preserving the existing safety contract.

Guardrails:

- preserve rootless Podman-only mutation;
- preserve allowed-path enforcement and fail-closed behavior;
- keep retries, model calls, tool use, and recovery bounded;
- do not weaken deterministic acceptance or semantic review;
- do not accept no-change work as success;
- avoid speculative or unrelated refactors;
- prefer evidence-driven fixes that can be validated by rerunning the substantive campaign that exposed them.

The first observed blocker is local-model tool-call reliability from the AdaptOS autonomous campaign, where malformed/incomplete tool arguments exhausted an implementation attempt even though correction within the same bounded conversation should be possible.
