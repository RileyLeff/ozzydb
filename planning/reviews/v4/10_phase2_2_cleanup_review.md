# Phase 2.2 Cleanup Review

## Scope

This follow-up pass addressed the five review findings left open after the initial Phase 2.2 snapshot landing.

Touched code:

- `crates/ozzy-server/src/registry.rs`
- `crates/ozzy-server/src/api/v1/fetch.rs`
- `crates/ozzy-server/src/compute/orchestrator.rs`
- `crates/ozzy-server/src/api/v1/endpoints.rs`

## Findings Addressed

1. **Pinned snapshot path is now live in active server flows.**
   - `fetch` now binds authored commit-state transforms/environments against the pinned v4 snapshot before cache checks.
   - `orchestrator` now does the same before execution.
   - endpoint inspection now requires that the commit resolves to a published v4 project revision before serving data.

2. **Snapshot relation surface is no longer equivalence-only.**
   - `RegistrySnapshot` now exposes registry-backed `equivalent(...)` and `refines(...)` queries over published type refs.

3. **Snapshot cache is no longer a naive unbounded map.**
   - `RegistrySnapshotCache` now has bounded capacity and single-flight loading for concurrent misses on the same revision.

4. **Snapshot corruption is no longer flattened into “unknown type ref”.**
   - `resolve_type_ref(...)` now returns a dedicated internal inconsistency error when a resolved published type is missing its stored row.

5. **Touched runtime fallback paths were removed.**
   - removed `unwrap_or_default()`-style JSON fallback in fetch/orchestrator materialization logic
   - removed silent source-hash fallback when source files cannot be read
   - removed JSON-inspection fallback behavior in endpoint inspection by switching to typed endpoint parsing

## Remaining Limits

- Endpoint definitions still come from `commit_state` during Phase 2.2, but they are now gated behind successful resolution of the corresponding v4 project revision.
- Full removal of `commit_state` as the runtime control plane remains Phase 2.3+ work.

## Verification

Executed:

- `cargo check -p ozzy-server`
- `cargo test -p ozzy-types`

## Exit Assessment

The original five review findings are addressed strongly enough to proceed to Phase 2.3.

The remaining work is architectural progression, not cleanup debt from the initial snapshot landing.
