# PR32 Generic Capability Routing

PR32 publishes `rack-ai/work-unit/v2` for the existing bounded workspace transaction. It adds only a typed generic routing header: opaque source/work/submission/idempotency identity, a non-empty `reasoning`/`coding`/`visual`/`audio` capability set, `small`/`medium`/`large` complexity, the existing large-context boolean, and global priority.

Rack AI owns typed source ceilings and internal profiles. The configured ATHBA ceiling is medium. `local-coder` is the least-scarce qualified choice for small coding; medium reasoning-plus-coding selects `local-primary`. Busy qualified capacity is temporary unavailability, not a capability claim.

Every selected transaction retains generic decision evidence and matching execution provenance. V1 stays compatible; v2 does not add dependency semantics, client workflow terms, a universal execution form, ComfyUI arbitration, or scheduler work.
