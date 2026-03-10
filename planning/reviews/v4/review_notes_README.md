# Review Notes — v4

Persistent notes on settled implementation choices and review caveats for the v4 rewrite.

## Current Notes

### Review tooling may degrade independently of the code under review

Gemini CLI currently emits MCP startup noise in this environment, and one-shot Claude CLI review calls may hang. Use available external output when it is useful, but do not block implementation checkpoints on flaky reviewer process behavior. Record the degradation and continue with explicit self-review plus tests.

### Early v4 scaffolding should prefer deletion over speculative abstraction

Phase 1 crate scaffolding is allowed to be narrow and incomplete. It should model the v4 architecture directly, avoid compatibility layers, and delete redundant fields or identifiers as soon as they are discovered.

### The v1 constructor surface is intentionally closed

`BuiltinConstructor` is a fixed enum in Phase 1. This is intentional. v4 should start with an explicit, auditable builtin constructor set rather than preserving stringly-typed extensibility before the canonicalization and verifier model are stable.

### Phase 1 refinement is intentionally conservative

The first `refines(...)` implementation does not attempt structural merge of intersected records or theorem-prover-style inference. It is allowed to return `false` in cases that may become provable later, as long as it does not return incorrect `true` results.

### Verification should error on malformed semantic constraints

Verification code must not assume malformed constructor state is impossible and then panic or implicitly reject. If verification receives an invalid semantic constraint shape, it should return a structured verifier error. Rejection is reserved for artifacts that fail a valid check, not for malformed verifier inputs.

### Conformance semantic state stays small; attempt history carries operational noise

`ConformanceRecord` semantic state is limited to `declared`, `verified`, and `rejected`. Verifier crashes, missing backends, and other execution failures belong in append-only verification attempts and must not introduce an extra semantic state or silently mutate an existing verified/rejected claim.

### Phase 1 semantic code should not rely on "validated earlier" panics

Phase 1 consolidation removed remaining panic-based constructor handling from canonicalization and relation evaluation. Going forward, malformed semantic inputs must return typed errors across syntax validation, canonicalization, relation evaluation, verification, and conformance handling.

### Published type verification requires a registry context

The verifier is no longer allowed to treat versioned type refs as opaque external leaves. Verification planning must resolve published refs through an explicit registry context so the runtime contract matches the v4 object model.

### Layered conjunctive verification uses derived witness inputs

Phase 1 follow-up introduced `VerificationInput::Derived(...)` so one artifact can satisfy a conjunctive type through multiple witness views. This is an explicit bridge until Phase 4/5 artifact-backed witness generation makes those views first-class in the wider platform.
