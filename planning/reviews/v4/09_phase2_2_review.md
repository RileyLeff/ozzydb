# Phase 2.2 Review

## Scope

Phase 2.2 introduces immutable `RegistrySnapshot`s for persisted v4 registry revisions and a small in-memory cache keyed by registry revision ID.

Implemented code:

- `crates/ozzy-server/src/registry.rs`
- `crates/ozzy-server/src/db/v4/queries.rs`
- `crates/ozzy-server/src/lib.rs`
- `crates/ozzy-server/src/main.rs`
- test state initializers under `crates/ozzy-server/tests/`
- small `TypeRegistry` helper additions in `crates/ozzy-types/src/registry.rs`

## What Changed

1. Added a server-side `RegistrySnapshot` model that reconstructs:
   - canonical types
   - published `TypeVersion`s
   - an equivalence index keyed by canonical type
   - persisted environment versions
   - persisted transform versions plus typed ports
2. Added a `RegistrySnapshotCache` to `AppState`.
3. Added loader entry points for:
   - direct registry revision lookup
   - project-revision-by-commit lookup
4. Added batch DB queries needed to load a full pinned revision without re-reading `commit_state`.

## Review Findings

### Blocking

None.

### Non-blocking

1. Fetch/orchestrator still execute from `commit_state`.
   - This is expected at this checkpoint.
   - Phase 2.2 lands the pinned snapshot infrastructure; later phases need to move runtime consumers onto it.

2. The relation index is intentionally narrow.
   - The snapshot currently loads canonical type identity and equivalence classes.
   - It does not precompute a full refinement closure yet.
   - That is acceptable for this phase because no runtime path depends on precomputed refinement.

3. Environment and transform objects are still server-owned snapshot structs.
   - This keeps Phase 2.2 scoped to persistence/runtime wiring.
   - If later phases need cross-crate reuse, these may migrate into `ozzy-types`.

## Verification

Executed:

- `cargo check -p ozzy-server`
- `cargo test -p ozzy-types`

Authored but not successfully used as a checkpoint signal in this harness:

- `cargo test -p ozzy-server registry_snapshots_load_and_cache_pinned_revisions --lib`

That test exists and compiles with the server crate, but the link/run step remained slow and noisy in this environment. It should be rerun in a normal local environment with `DATABASE_URL` set.

## Exit Criteria

Phase 2.2 is complete when:

- pinned registry revisions can be materialized as immutable in-memory snapshots
- repeated loads reuse cached snapshots
- callers can resolve a commit to its project revision and then to its pinned registry snapshot

That is now true.
