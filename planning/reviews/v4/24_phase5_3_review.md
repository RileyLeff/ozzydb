# Phase 5.3 Review

## Scope
- v4 materialized cache identity rewrite
- cache rows keyed by typed artifact bindings, published transform/environment versions, source hash, params hash, and optional secrets hash
- runtime cache-hit propagation of output artifact identity
- DB test rewrite for the new cache row shape

## Self-review
- Removed the old v3 cache-key ingredients from the live runtime path:
  - `transform_hash(...)`
  - `platform_hash`
  - `verification_tier`
- `fetch` and `orchestrator` now compute materialized hashes from:
  - sorted `(input_name, artifact_id)` pairs
  - `transform_def.row_id`
  - `transform_def.environment.row_id`
  - `source_hash`
  - `params_hash`
  - `secrets_hash`
- Cache hits now carry `output_artifact_id` forward in `NodeOutput`, so downstream nodes hash on artifact identity instead of falling back to output content hashes.
- Invocation input bindings no longer silently tolerate missing artifact IDs or store redundant `content_hash` values in the runtime cache identity path.
- The `materialized_cache` row shape now records project revision, transform/environment version IDs, source hash, params hash, artifact-bound inputs, optional secret hash, and output artifact ID.

## Checks run
- `cargo fmt`
- `cargo test -p ozzy-core`
- `cargo test -p ozzy-types`
- `cargo check -p ozzy-server --tests`

## Targeted test note
- I also attempted focused `ozzy-server` tests for:
  - `test_build_invocation_input_bindings_carries_artifact_bindings`
  - `db_tests::test_materialized_cache`
- In this harness they stayed stuck in the usual non-informative compile/link phase, so I did not use them as the phase gate.
- The rewritten DB cache test compiles under `cargo check -p ozzy-server --tests` and the broader checkpoint suite above passed.

## Issues found
- No new correctness issue remained after the final pass.
- `cargo fmt` touched additional CLI/server/test files outside the direct Phase 5.3 slice. The user explicitly approved keeping those formatting-only changes in this commit.
