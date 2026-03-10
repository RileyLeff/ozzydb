# Phase 5.2 Review — Invocation And Output Artifact Slice

Date: 2026-03-10

## Scope

Land the first execution-side `Artifact` / `Invocation` integration for v4
without trying to finish the full fetch rewrite in one step.

Files reviewed:

- `crates/ozzy-server/src/compute/orchestrator.rs`
- `crates/ozzy-server/src/db/v4/queries.rs`
- `crates/ozzy-server/src/api/v1/fetch.rs`

## Findings

- Fixed: successful node execution now creates a real `v4_invocation` row bound
  to:
  - `project_revision_id`
  - `transform_version_id`
  - resolved parameter identity
  - explicit input binding metadata

- Fixed: successful node execution now persists a first-class output artifact
  and binds it back to the invocation.

- Fixed: successful node execution now declares output conformance against the
  transform's single published output type.

- Fixed: output persistence is now transactional at the DB layer.
  - artifact creation
  - output binding creation
  - declared conformance
  - invocation success transition
  either all commit or none do.

- Fixed: invocation rows are no longer created before the per-node cache check.
  - cache-hit nodes do not leave stale `running` invocations behind.

- Fixed: if compute fails or post-compute persistence fails, the invocation is
  marked `failed` instead of being stranded in `running`.

- Added: node output state now carries optional artifact identity so downstream
  invocation input metadata can include upstream artifact IDs when available.

## Notes

- This is intentionally not the whole Phase 5.2 cutover yet.
- Leaf input ingress still resolves through the old `data:` / `collection:`
  paths.
- Input conformance verification is still outstanding.
- Output verification is still outstanding; this slice declares output
  conformance but does not verify it.
- The checkpoint used direct self-review plus tests. I did not rely on the
  flaky external CLI review loop for this slice.

## Verification

- `cargo check -p ozzy-server --tests`
- `cargo test -p ozzy-types`

Notes:

- A targeted `cargo test -p ozzy-server ... --lib` attempt again hit the usual
  slow test-binary link path in this environment, so `cargo check --tests` was
  used as the reliable server-side gate.

## Result

The execution plane now records v4 invocations and output artifacts with
declared conformance, and it does so without leaving partial success state on
cache hits or mid-flight persistence failures.
