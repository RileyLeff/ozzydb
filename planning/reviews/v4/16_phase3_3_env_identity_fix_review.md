# Phase 3.3 Environment Identity Fix Review

## Scope

Follow-up to the Phase 3.3 review finding that `EnvironmentVersion` identity was
still path-sensitive even though the v4 architecture requires content-bound
publication.

Files reviewed:

- `crates/ozzy-core/src/toml_spec.rs`
- `crates/ozzy-server/src/api/v1/push.rs`
- `crates/ozzy-server/src/environments/mod.rs`
- `crates/ozzy-server/src/environments/hash.rs`
- `crates/ozzy-server/src/environments/docker.rs`
- `crates/ozzy-server/src/db/v4/queries.rs`
- `crates/ozzy-server/src/registry.rs`

## Findings

- Fixed: published environment identity no longer includes authored path strings.
  - `PublishedEnvironmentDef::BaseLockfile` now stores `installer` plus resolved content.
  - `PublishedEnvironmentDef::Dockerfile` now stores only resolved content.
  - Environment hashing and publication dedup now treat authored path renames as
    non-semantic.
- Fixed: build-time environment realization no longer infers semantics from
  authored lockfile paths. Push classifies the installer strategy once at
  publication time, and Dockerfile generation consumes that normalized strategy.
- No new silent fallback behavior introduced.

## Verification

- `cargo test -p ozzy-core`
- `cargo test -p ozzy-types`
- `cargo check -p ozzy-server --tests`

## Result

The Phase 3.3 environment publication model is now aligned with the v4
architecture’s content-bound identity requirement. No additional blocking issues
were found in this follow-up pass.
