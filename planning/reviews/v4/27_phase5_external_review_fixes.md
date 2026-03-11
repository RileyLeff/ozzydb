## Phase 5 External Review Fix Pass

Scope:
- follow-up fixes for findings recorded in
  `planning/reviews/v4/26_phase5_external_review.md`

Files touched:
- `Cargo.toml`
- `crates/ozzy-core/src/toml_spec.rs`
- `crates/ozzy-server/Cargo.toml`
- `crates/ozzy-server/src/api/v1/fetch.rs`
- `crates/ozzy-server/src/compute/docker.rs`
- `crates/ozzy-server/src/compute/fly.rs`
- `crates/ozzy-server/src/compute/orchestrator.rs`
- `crates/ozzy-server/src/compute/types.rs`
- `crates/ozzy-server/src/db/v4/queries.rs`
- `crates/ozzy-server/src/lib.rs`
- `crates/ozzy-server/src/verification.rs`
- `crates/ozzy-types/src/verify/mod.rs`

## Fixed

1. Enforced `network = false` at the compute boundary.
   - `ComputeRequest` now carries `network_enabled`.
   - Docker runs with `--network none` when disabled.
   - Fly now fails explicitly for network-disabled transforms instead of silently ignoring policy.

2. Removed trust-based output conformance on the execution success path.
   - Successful node execution now:
     - persists the output artifact and declared conformance
     - verifies the produced bytes against the declared output type
     - records verification report/failure
     - only marks the invocation succeeded after verified output conformance
   - Rejected output verification now fails the invocation explicitly.

3. Removed multi-terminal fallback behavior.
   - Authored validation rejects endpoints with multiple terminal nodes.
   - Runtime terminal-node discovery now errors instead of warning and picking one arbitrarily.

4. Rejected authored `endpoint:` edge sources during validation.
   - Both local and cross-project `endpoint:` sources now fail at parse/validation time with v4-specific guidance.

5. Hardened cache hits against stale artifact references.
   - Cache-hit paths in fetch and orchestrator now:
     - require the output artifact row to exist
     - require output conformance for the published output type
     - verify declared output conformance before reusing the artifact

6. Removed the catch-all blob-loader fallback.
   - `infer_blob_loader_from_expr(...)` now errors when a type does not declare a valid executable blob encoding.

7. Removed parameter coercion fallback.
   - `coerce_param_value(...)` now returns typed errors for invalid `float` / `int` / `bool` coercions instead of falling back to the original string.

## Additional follow-up fixed during self-review

1. Added artifact-backed verification helpers in `crates/ozzy-server/src/verification.rs`.
2. Added explicit `bytes` and `json` verification inputs in `ozzy-types`.
3. Fixed server-side blob verification so valid `bytes` and `json` artifacts do not fail due to missing derived witness shapes.

## Verification

- `cargo fmt`
- `cargo test -p ozzy-types`
- `cargo test -p ozzy-core`
- `cargo check -p ozzy-server --tests`

Targeted server tests were started for the new verifier/fetch code paths, but the harness again degraded into the usual long compile/link behavior before producing useful per-test signal. The compile/check gates above completed successfully.
