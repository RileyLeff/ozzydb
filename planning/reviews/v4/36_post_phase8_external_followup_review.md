# Post-Phase-8 External Follow-Up Review

Date: 2026-03-11
Phase: Post-plan stabilization

## Scope reviewed
- `crates/ozzy-core/src/toml_spec.rs`
- `crates/ozzy-types/src/verify/mod.rs`
- `crates/ozzy-server/src/api/v1/push.rs`

## Context
- `just test-all` was attempted first, but the `cargo test --workspace` path again stalled in the heavy `ozzy-server` test build/link step and did not provide useful signal in this environment.
- External review was retried with a focused snapshot.
- Gemini remained unreliable for this pass.
- Claude produced a usable isolated review once its plugin/MCP surface was disabled.

## Findings fixed
- Endpoint validation now rejects endpoints with zero nodes.
- Endpoint edge validation now checks source/target port type compatibility instead of only checking node/input names.
- `collection<T>` verification now accepts `table<T>` witnesses, matching the type relation where `table<T>` refines `collection<T>`.
- Base+lockfile environment classification no longer silently treats unknown lockfiles as pip requirements; unsupported lockfile formats now error explicitly.

## Non-findings / deferred
- Cache race concern was not accepted as a bug because the cache write path already uses `ON CONFLICT (materialized_hash) DO UPDATE`.
- The `csv()` omitted-arg verifier strictness question was left unchanged; it is not clearly a correctness defect.
- Non-numeric published versions were not changed in this pass because that path is not exercised by normal publication and needs a separate design decision if it is to be hardened.

## Verification
- `cargo fmt`
- `cargo test -p ozzy-core`
- `cargo test -p ozzy-types`
- `cargo check -p ozzy-server --tests`

## Notes
- This was a targeted stabilization pass after the v4 plan was already complete.
- No new fallback paths were introduced; the fixes made validation and publication stricter.
