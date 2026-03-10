# Phase 5.2 Review — Typed Fetch And Artifact-Bound Inputs

Date: 2026-03-10

## Scope

Finish the larger Phase 5.2 cut:

- endpoint inputs become typed authored ports
- fetch binds artifacts directly to endpoint inputs
- endpoint edges use `input:<name>` instead of `data:` / `collection:`
- runtime input manifests are built from first-class artifacts and typed bundle
  / collection shapes

Files reviewed:

- `crates/ozzy-core/src/toml_spec.rs`
- `crates/ozzy-server/src/api/v1/fetch.rs`
- `crates/ozzy-server/src/compute/orchestrator.rs`
- `crates/ozzy-server/src/compute/types.rs`
- `crates/ozzy-server/src/runners/python.rs`
- `crates/ozzy-server/src/runners/r.rs`
- `crates/ozzy-server/src/publication.rs`
- `crates/ozzy-server/src/db/queries.rs`
- `crates/ozzy-server/src/db/models.rs`
- `crates/ozzy-server/migrations/010_jobs_input_bindings.sql`

## Findings

- Fixed: endpoint definitions now declare typed input ports explicitly.
  - authored endpoints use `[endpoints.<name>.inputs.<port>]`
  - edge sources now use `input:<port>`

- Fixed: publication now rewrites endpoint input type refs the same way it
  already rewrites transform port refs.
  - local endpoint input types are resolved to published versioned types
  - external published refs are attached to the registry revision payload

- Fixed: fetch no longer accepts anonymous leaf data ingress.
  - request body now carries explicit `inputs: { <port>: <artifact_uuid> }`
  - fetch validates exact endpoint input coverage
  - bound artifacts must belong to the same project
  - bound artifacts must already have a non-rejected conformance record for the
    required endpoint input type

- Fixed: job dedup now incorporates endpoint input bindings.
  - jobs persist `input_bindings`
  - dedup uses `input_bindings_hash` in addition to `params_hash`

- Fixed: runtime input materialization now runs through first-class artifacts.
  - blob artifacts materialize with an explicit loader
  - collection artifacts materialize recursively from `ArtifactManifest`
  - bundle artifacts materialize recursively and enforce closed-record shape

- Fixed: Python and R runners now load recursive bundle/collection manifests
  instead of the legacy `is_collection` flag contract.

- Fixed: the remaining `data:` ingress in live fetch/orchestrator code is gone.

## Notes

- The v3 collection and data APIs still exist elsewhere in the server, but they
  are no longer part of the live v4 fetch/execution ingress path.

- Endpoint input conformance policy is still intentionally simple at this step:
  fetch accepts `declared` or `verified` conformance and rejects only missing
  or explicitly `rejected` conformance. Stronger verification policy can be
  layered later without reopening the ingress model.

- A direct `cargo test -p ozzy-server --lib` run again hit the same slow
  test-binary link path that has been a poor checkpoint signal in this
  environment. I did not treat that long link as a passing result.

- External CLI review was not used for this checkpoint. The current reliable
  review signal was direct self-review plus the compile/test set below.

## Verification

- `cargo test -p ozzy-core`
- `cargo test -p ozzy-types`
- `cargo check -p ozzy-core -p ozzy-types -p ozzy-server --tests`

## Result

Phase 5.2 now uses typed endpoint inputs and first-class artifacts end-to-end in
the live fetch path. The old anonymous `data:` / `collection:` ingress model is
no longer part of the runtime control plane.
