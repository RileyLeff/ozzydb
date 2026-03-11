## Phase 6.2 Review

Scope:
- add direct v4 inspection APIs for artifacts, conformance records, type versions,
  environment versions, and transform versions

Files:
- `crates/ozzy-server/src/api/v1/artifacts.rs`
- `crates/ozzy-server/src/api/v1/registry_objects.rs`
- `crates/ozzy-server/src/api/v1/mod.rs`
- `crates/ozzy-server/src/api/v1/auth.rs`
- `crates/ozzy-server/src/db/v4/queries.rs`

## What changed

1. Added artifact inspection endpoints.
   - `GET /v1/artifacts/{owner}/{slug}`
   - `GET /v1/artifacts/{owner}/{slug}/{artifact_id}`
   - `GET /v1/artifacts/{owner}/{slug}/{artifact_id}/conformance`
   - Responses are first-class v4 artifact/conformance objects, not wrappers around the old data/collection ontology.

2. Added registry-object inspection endpoints.
   - `GET /v1/types/{owner}/{slug}`
   - `GET /v1/types/{owner}/{slug}/resolve?name=...&version=...`
   - `GET /v1/environments/{owner}/{slug}`
   - `GET /v1/environments/{owner}/{slug}/resolve?name=...&version=...`
   - `GET /v1/transforms/{owner}/{slug}`
   - `GET /v1/transforms/{owner}/{slug}/resolve?name=...&version=...`
   - These expose published type/environment/transform versions directly.

3. Added the DB query surface needed for those APIs.
   - list and point lookup for type versions
   - list and point lookup for environment versions
   - list and point lookup for transform versions
   - canonical type lookup by row ID

4. Added a centralized `From<V4QueryError> for ApiError` conversion.
   - This keeps the new handler code explicit without repeated glue at every call site.

## Self-review notes

- The `resolve` pattern for type/environment/transform detail is intentional because type names can contain `/`, which makes path-segment routing awkward.
- These routes are project-scoped and read-only. They do not depend on the latest commit or latest registry revision unless the underlying object lookup requires it.
- Artifact detail deliberately exposes manifest structure via the typed `ArtifactManifest` model instead of reviving the old collection API.
- Conformance inspection is strict: missing referenced type versions or canonical types are internal errors, not silent omission.

## Verification

- `cargo fmt`
- `cargo test -p ozzy-core`
- `cargo test -p ozzy-types`
- `cargo check -p ozzy-server --tests`

The server check still emits the pre-existing `unused_parens` warnings in older test files, but the build completed successfully.
