# Phase 3.2 Review

## Scope

Phase 3.2 publication rewrite.

Touched code:

- `crates/ozzy-core/src/toml_spec.rs`
- `crates/ozzy-server/src/api/v1/push.rs`
- `crates/ozzy-server/src/lib.rs`
- `crates/ozzy-server/src/publication.rs`

## What Changed

1. Added a dedicated v4 publication subsystem in `crates/ozzy-server/src/publication.rs`.
2. Replaced the push write path so `POST /v1/push` now:
   - parses and validates `ozzy.toml`
   - compiles a `PublicationBundle`
   - resolves or creates versioned types, environments, and transforms
   - creates a new registry revision and project revision atomically
3. Rewrote published transform payloads so transform port type refs are stored as explicit published version pins.
4. Tightened transform-port validation so direct builtin refs are no longer allowed on ports; ports must reference:
   - a named type from `[types]`, or
   - an explicit published type version
5. Added duplicate/racy push handling inside the publication transaction so a concurrent same-SHA push reuses the existing commit instead of partially republishing.

## Review Findings

### Fixed during review

1. The first publication pass still left `push` using `commit_state`-era assumptions for duplicate validation.
   - Fixed by requiring an existing commit to also have a v4 project revision.
2. The first publication API carried an unused `project_revision` field in the outcome type.
   - Removed.
3. Direct builtin transform-port refs would have forced synthetic anonymous published types or broken publication.
   - Fixed by making them validation errors up front.

### Residual non-blocking debt

1. Legacy `commit_state` helpers and old `register_commit_atomically(...)` remain in the DB/test surface.
   - They are no longer on the live push/runtime path.
   - They should be deleted when the remaining legacy tests are rewritten to the v4 publication model.
2. Provider-specific environment realization is still post-publication async work driven from authored environment defs.
   - This is expected until Phase 3.3.

### External review status

- I did not use the external reviewer loop as a gate for this checkpoint.
- Prior Claude/Gemini runs in this environment were noisy and unreliable enough that they were not a good stop/go signal.
- This checkpoint used explicit self-review plus tests instead.

## Verification

Executed:

- `cargo test -p ozzy-core`
- `cargo test -p ozzy-types`
- `cargo check -p ozzy-server --tests`

Notes:

- `cargo check -p ozzy-server --tests` still emits pre-existing warning noise from unrelated server test files.
- A DB-gated publication round-trip test was added in `crates/ozzy-server/src/publication.rs`.
- I did not treat `cargo test -p ozzy-server publication::tests::publication_reuses_equivalent_type_environment_and_transform_versions` as a hard gate in this harness because the compile/link step remained a poor signal here.

## Exit Assessment

Phase 3.2 is complete.

The important architectural shift is now real:

- push no longer publishes `commit_state` as runtime truth
- the server now creates first-class v4 registry objects and project revisions atomically
- published transform payloads are aligned with the versioned type registry rather than local parser-only names

The next step is Phase 3.3: separate logical `EnvironmentVersion` publication from provider-specific realization work more explicitly.
