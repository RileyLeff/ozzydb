# v4 Workflow State

## Current Phase: Phase 1 - Core Type System Foundation
## Current Step: Step 1.2 complete (core type language); Step 1.3 next

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

### Phase 1.2: Implement the core type language
- Added explicit builtin leaf types:
  - `bytes`
  - `utf8`
  - `json`
  - `parquet`
  - `string`
  - `bool`
  - `int64`
  - `float64`
  - `date`
  - `datetime`
- Added explicit builtin constructors:
  - `csv`
  - `unit`
  - `min`
  - `max`
  - `enum`
  - `nullable`
- Added `TypeDefinition` and `TypeDefinitions` for local named aliases before publication.
- Added local expression validation for:
  - unknown local refs
  - invalid builtin version pins
  - duplicate record fields
  - constructor argument shape
  - empty intersections
- Changed `TypeExpr::Table` to wrap a row type expression, not just an inline record.
- Changed float literals to `OrderedFloat<f64>` so the syntax layer can derive `Eq`/`Hash` ahead of canonicalization.

## Open Review Findings
- None blocking for Phase 1.2.
- Review artifacts:
  - `planning/reviews/v4/01_phase1_1_review.md`
  - `planning/reviews/v4/02_phase1_2_review.md`

## Current Blockers
- None.

## Next Recommended Steps
1. Start Step 1.3 and implement canonicalization, strict equivalence, conservative refinement, and `never`.
2. Keep the implementation narrow: no planner logic and no server wiring yet.
3. Re-run the review loop after Step 1.3 lands.

## Notes
- Existing v3 planning and type-system notes remain background context only.
- Frontend work remains intentionally deferred.
- External review tooling degraded during the Phase 1 checkpoint; see `planning/reviews/v4/review_notes_README.md`.
