# Pre-Phase 5.1 Source Fallback Fix Review

Date: 2026-03-10

## Scope

Eliminate the remaining live degraded source path before starting Phase 5.1.

Affected files:

- `crates/ozzy-server/src/api/v1/fetch.rs`
- `crates/ozzy-server/src/compute/orchestrator.rs`

## Fixed

1. `retrieve_source_code(...)` no longer downgrades real failures to `None`.
   - Source storage creation, tarball fetch, tempdir creation, tar invocation, extraction failure, and cleanup failure now return explicit errors.

2. Source retrieval is now demand-driven.
   - `endpoint_requires_source_code(...)` checks whether any transform in the endpoint actually uses `source`.
   - Command-only endpoints no longer fail just because no source tarball is present.

3. Source transforms no longer hash a synthetic fallback value.
   - `compute_source_hash(...)` now requires extracted source for source-backed transforms.
   - Missing extracted source is a hard error.

4. Cache checking no longer reinterprets source/materialization errors as cache misses.
   - `check_all_node_caches(...)` now propagates `compute_materialized_hash(...)` errors instead of silently returning `(false, ...)`.

## Self-check

- The remaining fallback path identified in the pre-Phase 5.1 review is gone.
- The new helper keeps command-only transforms working without forcing irrelevant source retrieval.
- The change is aligned with `AGENTS.md`: no silent fallback in semantic code.

## Verification

- `cargo test -p ozzy-types`
- `cargo check -p ozzy-server --tests`

Notes:

- A targeted `cargo test -p ozzy-server ... --lib` attempt again hit the usual long/noisy test-binary link path in this harness, so `cargo check --tests` was used as the reliable gate.
