## Phase 6.1 Review

Scope:
- rewrite endpoint and project-revision inspection APIs around v4 objects

Files:
- `crates/ozzy-server/src/api/v1/inspection.rs`
- `crates/ozzy-server/src/api/v1/endpoints.rs`
- `crates/ozzy-server/src/api/v1/commits.rs`
- `crates/ozzy-server/src/api/v1/mod.rs`
- `crates/ozzy-server/src/registry.rs`

## What changed

1. Added a shared inspection layer in `api/v1/inspection.rs`.
   - endpoint summaries/details now derive from:
     - `PublishedProjectRevision`
     - `RegistrySnapshot`
     - bound runtime transforms and environments
   - commit detail now exposes a structured `project_revision` object instead of raw JSON payload blobs

2. Endpoint inspection now surfaces typed inputs and resolved transform/environment identities.
   - endpoint detail includes:
     - `project_revision_id`
     - `registry_revision_id`
     - typed endpoint inputs
     - resolved transform version/environment IDs per node
     - resolved typed transform input/output ports

3. Commit inspection now exposes project-revision inspection rather than stored payload blobs.
   - structured environments
   - structured transforms
   - structured endpoint summaries

4. `PublishedProjectRevision` now retains the authored environment bindings and authored transform map so API inspection can reflect the published object graph without reparsing raw JSON at the handler layer.

## Self-review notes

- This is an API-shape rewrite, not a control-plane rewrite. Runtime behavior is unchanged.
- The new responses are intentionally version/object-oriented and no longer optimized for backwards compatibility with old CLI/frontend assumptions.
- `get_endpoint_dag(...?format=json)` now serializes the structured endpoint inspection object instead of dumping the raw endpoint definition.

## Verification

- `cargo fmt`
- `cargo test -p ozzy-core`
- `cargo check -p ozzy-server --tests`

I also started `cargo test -p ozzy-server --lib inspection::tests::`, but the server test target fell into the same slow/no-signal link behavior seen in earlier phases. The compile gate completed successfully, and the new pure inspection tests compile as part of that target.
