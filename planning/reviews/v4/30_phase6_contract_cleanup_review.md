## Phase 6 Contract Cleanup Review

Scope:
- finish the remaining public-contract cleanup after Phase 6.2
- replace the old `data` / `collections` ingress with v4 artifact writes
- resolve the v4 yank decision in code

Files:
- `crates/ozzy-server/src/api/v1/artifacts.rs`
- `crates/ozzy-server/src/api/v1/mod.rs`
- `crates/ozzy-server/src/api/v1/fetch.rs`
- `planning/v4/implementation_plan.md`
- `planning/v4/WORKFLOW_STATE.md`

## What changed

1. Added live v4 artifact write endpoints.
   - `POST /v1/artifacts/{owner}/{slug}/upload`
   - `POST /v1/artifacts/{owner}/{slug}/manifest`
   - `POST /v1/artifacts/{owner}/{slug}/{artifact_id}/conformance`
   - `GET /v1/artifacts/{owner}/{slug}/{artifact_id}/download`

2. The public router no longer exposes the old v3 `data` and `collections` routes.
   - The modules remain on disk for later deletion, but they are no longer part of the live contract.

3. The live fetch path no longer consults endpoint yanks.
   - This removes another v3 semantic from the v4 runtime path.

4. The implementation plan now records the decision explicitly:
   - yanks do not survive as a first-class v4 primitive
   - conformance state is the live mechanism for marking artifacts unusable

## Self-review notes

- This is an intentional contract break. There is no compatibility shim for `/v1/data` or `/v1/collections`.
- Conformance declaration currently resolves against the latest published project revision, not an arbitrary historical registry revision. That is acceptable for the current v4 API surface and can be widened later if needed.
- Artifact upload now provides a real public path for blob inputs before the CLI/Python rewrite.
- Manifest creation provides the minimal public path for bundle/collection-style artifact inputs without reviving the old collection ontology.

## Verification

- `cargo fmt`
- `cargo test -p ozzy-core`
- `cargo test -p ozzy-types`
- `cargo check -p ozzy-server --tests`

The remaining warnings are the pre-existing `unused_parens` warnings in older server test files.
