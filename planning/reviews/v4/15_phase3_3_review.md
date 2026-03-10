# Phase 3.3 Review

## Scope

Reviewed the Phase 3.3 environment publication / realization split:

- `crates/ozzy-core/src/toml_spec.rs`
- `crates/ozzy-server/src/publication.rs`
- `crates/ozzy-server/src/api/v1/push.rs`
- `crates/ozzy-server/src/registry.rs`
- `crates/ozzy-server/src/api/v1/fetch.rs`
- `crates/ozzy-server/src/compute/orchestrator.rs`
- `crates/ozzy-server/src/environments/docker.rs`
- `crates/ozzy-server/src/environments/hash.rs`
- `crates/ozzy-server/src/db/v4/queries.rs`

Plan baseline:

- `planning/v4/architecture.md`
- `planning/v4/implementation_plan.md`

## What Changed

- Published environment versions now store resolved, content-bound environment definitions.
- Push resolves lockfile and Dockerfile content before publication and fails if that authored environment input is invalid.
- Published transform payloads now pin environment refs to published versions.
- Published project revision payloads store authored-name -> published-version environment bindings.
- Runtime environment resolution now comes from published environment rows in the pinned snapshot, not git fetches against authored path specs.
- Async environment builds now consume published environment rows directly.

## Review Findings

### Resolved in this step

1. Logical `EnvironmentVersion` identity no longer drifts from realized image identity when a path-stable lockfile or Dockerfile changes contents.
2. Runtime environment resolution no longer depends on ad hoc git fetches for lockfiles or Dockerfiles.
3. Provider realization is now clearly downstream of published environment identity.

### Remaining acceptable limits

1. The old `environment_images` and `environment_provider_images` tables are still realization infrastructure keyed by `env_hash`. They are now fed by published definitions, but they have not yet been redesigned as first-class v4 persistence.
2. Legacy DB/e2e tests still mention old publication helpers and `commit_state` in places outside the live runtime path.

## Verification

- `cargo test -p ozzy-core`
- `cargo test -p ozzy-types`
- `cargo check -p ozzy-server --tests`

## Verdict

Phase 3.3 is clean enough to move on.
