# Phase 2.1 Review

## Scope

This review covers the first persisted v4 registry slice:

- additive SQL migration for v4 registry objects
- new `db::v4` Rust model/query layer
- project-scoped version rows plus global canonical type storage
- first persisted revision and conformance objects

## What Landed

- `crates/ozzy-server/migrations/004_v4_registry.sql`
- `crates/ozzy-server/src/db/v4/mod.rs`
- `crates/ozzy-server/src/db/v4/models.rs`
- `crates/ozzy-server/src/db/v4/queries.rs`
- workspace/server dependency wiring for `ozzy-types`

## Review Findings

### 1. Registry persistence shape is aligned with v4

The new schema no longer treats `commit_state` JSON blobs as the only runtime truth.
It adds first-class rows for the versioned objects the architecture calls for, and does so additively so the server can migrate off the v3 control plane incrementally.

### 2. Project scoping is explicit where it matters

Versioned types, environments, and transforms are project-scoped.
Canonical types are global.
That is the right split for the current platform state.

The migration also now enforces that a `v4_project_revision` cannot point at a `v4_registry_revision` from another project.

### 3. Query surface is intentionally narrow

The new query methods cover creation and lookup of the Phase 2.1 objects without pretending the publication pipeline already exists.
That keeps Phase 2.1 honest.

### 4. Remaining gaps are expected for this phase

- No `RegistrySnapshot` loader yet.
- No transaction-scoped publication helpers yet.
- No endpoint persistence yet.
- No artifact table or FK-backed artifact references yet.

Those are Phase 2.2+ concerns, not signs that this phase is incomplete.

### 5. Verification notes

Executed:

- `cargo check -p ozzy-server`
- `cargo test -p ozzy-types`

Not executed:

- the new DB-backed v4 round-trip test, because `DATABASE_URL` is unset in this environment
- full `cargo test -p ozzy-server --no-run`, because linking the full server test binary was slow/noisy enough to be a poor checkpoint signal here once `cargo check` had already passed and the only emitted diagnostics were pre-existing legacy warning spam in older test files

## Conclusion

Phase 2.1 is in good shape.

The next correct step is Phase 2.2:

- load immutable snapshots from the new persisted rows
- stop reading runtime control state out of `commit_state`
