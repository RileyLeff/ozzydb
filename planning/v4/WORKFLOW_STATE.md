# v4 Workflow State

## Current Phase: Phase 1 - Core Type System Foundation
## Current Step: Step 1.1 complete (`crates/ozzy-types` scaffold); Step 1.2 next

## Completed Steps

### Planning baseline
- Created `planning/v4/architecture.md`
- Created `planning/v4/implementation_plan.md`
- Created `planning/v4/AGENT_WHITEBOARD.md`
- Created `planning/v4/WORKFLOW_STATE.md`
- Added the v1 type-semantics and publication addenda to `architecture.md`

### Phase 1.1: Create `crates/ozzy-types`
- Added `crates/ozzy-types` to the workspace.
- Created the initial module layout:
  - `syntax.rs`
  - `canonical.rs`
  - `registry.rs`
  - `relations.rs`
  - `verify/`
  - `ports.rs`
  - `conformance.rs`
- Added crate-level re-exports and basic unit coverage.
- Added the first witness structs:
  - `CsvWitness`
  - `TableWitness`
  - `RecordWitness`
- Added `TypeVersion::new(...)` so public IDs are derived from `(name, version)`.
- Tightened `TypeRegistry` duplicate detection to reject duplicate `(name, version)` pairs.
- Removed redundant embedded port names from `TypedPort`.

## Open Review Findings
- None blocking for Phase 1.1.
- Review artifact: `planning/reviews/v4/01_phase1_1_review.md`

## Current Blockers
- None.

## Next Recommended Steps
1. Start Step 1.2 and implement the v1 type language constructors and named aliases in `ozzy-types`.
2. Keep the implementation narrow: AST and constructors first, no relation solver or planner work beyond the v1 baseline.
3. Re-run the review loop after Step 1.2 lands.

## Notes
- Existing v3 planning and type-system notes remain background context only.
- Frontend work remains intentionally deferred.
- External review tooling degraded during the Phase 1.1 checkpoint; see `planning/reviews/v4/review_notes_README.md`.
