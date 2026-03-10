# v4 Workflow State

## Current Phase: Phase 1 - Core Type System Foundation
## Current Step: Step 1.5 complete (conformance model); Phase 1 consolidation checkpoint

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

### Phase 1.3: Canonicalization and relation checks
- Added canonicalization of the v1 type language surface.
- Added canonical IDs and an in-memory canonical interner.
- Implemented:
  - strict `equivalent(...)`
  - conservative `refines(...)`
  - canonical `never`
- Added canonical reduction for:
  - scalar base conflicts
  - conflicting `csv` constraints
  - conflicting `unit(...)` constraints
  - `min > max`
  - empty `enum(...)` intersections
  - record fields that canonicalize to `never`
- Added the first structural refinement rules for:
  - records
  - collections
  - tables
  - builtin constructor families
- Replaced the temporary serde-driven canonical hash path with an explicit deterministic AST fingerprint.

### Phase 1.4: Verification planning and witnesses
- Added verifier compilation from canonical type expressions into executable `VerificationPlan`s.
- Added the first verification execution surface:
  - `VerificationInput`
  - `VerificationPlan`
  - `BuiltinVerifierRegistry`
  - `VerificationError`
  - `VerificationReport`
- Implemented the first builtin verification paths for:
  - scalar builtin types
  - `csv(...)`
  - `min(...)`, `max(...)`, and `enum(...)`
  - record verification against record values and table schemas
  - collection verification
  - table verification
- Narrowly reused `ozzy-core::schema` to derive `TableWitness` values from Parquet files.
- Tightened verification error handling so malformed semantic constraints return typed verifier errors instead of panicking or silently degrading into rejection.
- Expanded test coverage for:
  - plan compilation
  - CSV witness rejection
  - table-schema verification
  - malformed constructor regression

### Phase 1.5: Conformance model
- Replaced the placeholder top-level evidence field with an explicit append-only verification attempt log.
- Added:
  - `VerificationFailure`
  - `VerificationAttempt`
  - `ConformanceRecord::declared(...)`
  - `record_report(...)`
  - `record_failure(...)`
  - `latest_report()` / `latest_evidence()`
- Kept semantic conformance state constrained to:
  - `declared`
  - `verified`
  - `rejected`
- Added unit coverage proving that:
  - declared records start empty
  - completed verification updates semantic state
  - failed verification attempts do not mutate semantic state
  - rejected reports set `rejected`

## Open Review Findings
- None blocking for Phase 1.
- Review artifacts:
  - `planning/reviews/v4/01_phase1_1_review.md`
  - `planning/reviews/v4/02_phase1_2_review.md`
  - `planning/reviews/v4/03_phase1_3_review.md`
  - `planning/reviews/v4/04_phase1_4_review.md`
  - `planning/reviews/v4/05_phase1_5_review.md`

## Current Blockers
- None.

## Next Recommended Steps
1. Run a Phase 1 consolidation review over `crates/ozzy-types` before starting Phase 2.
2. If that review is clean, start Phase 2.1 registry persistence and snapshot modeling.
3. Keep server/runtime rewrites deferred until the new persistence model exists.

## Notes
- Existing v3 planning and type-system notes remain background context only.
- Frontend work remains intentionally deferred.
- External review tooling degraded during the Phase 1 checkpoint; see `planning/reviews/v4/review_notes_README.md`.
