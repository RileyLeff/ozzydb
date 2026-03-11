# Phase 8.3 Review

Date: 2026-03-11
Phase: 8.3 — Update docs from the new ontology

## Scope reviewed
- `README.md`
- `docs/getting_started.md`
- `planning/v4/soul.md`
- `planning/v4/architecture.md`
- `planning/v4/implementation_plan.md`

## Summary
- Rewrote the top-level README around the live v4 model:
  - typed artifacts
  - typed transforms
  - versioned environments
  - published project revisions and registry snapshots
- Rewrote the getting-started guide so the examples use:
  - `ozzy artifact ...`
  - typed `[types]`
  - typed transform input/output ports
  - endpoint `input:<name>` edges
  - fetch-time artifact bindings
- Added a dedicated `planning/v4/soul.md` so the active baseline has a v4 principle document instead of pointing back to v3.
- Updated v4 planning references to point at the new soul doc.

## Findings
- No blocking findings from the doc sweep.
- The docs now describe the live v4 public surface instead of the removed v3 data/collection and schema-only model.
- Frontend/web walkthroughs remain intentionally light because frontend work is still deferred relative to the v4 API and clients.

## Verification
- Manual consistency check against:
  - `crates/ozzy-cli/src/main.rs`
  - `crates/ozzy-cli/src/commands/init.rs`
  - `crates/ozzy-cli/src/commands/fetch.rs`
  - `crates/ozzy-cli/src/commands/artifact.rs`
  - `clients/python/src/ozzydb/client.py`
  - `planning/v4/architecture.md`

## Notes
- This pass is documentation-only; no code paths changed.
