# v4 Workflow State

## Current Phase: Phase 1 complete; Phase 2 next
## Current Step: Phase 1 follow-up fix pass complete

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
  - `latest_completed_report()` / `latest_completed_evidence()`
- Kept semantic conformance state constrained to:
  - `declared`
  - `verified`
  - `rejected`
- Added unit coverage proving that:
  - declared records start empty
  - completed verification updates semantic state
  - failed verification attempts do not mutate semantic state
  - rejected reports set `rejected`

### Phase 1 consolidation review
- Reviewed the entire `ozzy-types` crate at the phase boundary.
- Removed remaining non-test panic-based constructor handling from canonicalization and relation evaluation.
- Added regression coverage proving malformed semantic constructor state errors instead of panicking.
- Reconfirmed `cargo test -p ozzy-types` passes for the whole Phase 1 surface.

### Phase 1 follow-up fix pass
- Made published type verification registry-backed instead of treating versioned refs as opaque verifier dead ends.
- Added `TypeRegistry::resolve_ref(...)` and `get_by_name_version(...)`.
- Made verifier compilation/execution require a registry context.
- Added `VerificationInput::Derived(...)` so conjunctive verification can consume multiple witness views of one artifact.
- Renamed conformance helpers so latest attempt and latest completed report/evidence are not conflated.
- Updated the architecture doc to stage the remaining verifier-surface gap across Phases 2 through 5.

## Open Review Findings
- None blocking for the Phase 1 library surface.
- Review artifacts:
  - `planning/reviews/v4/01_phase1_1_review.md`
  - `planning/reviews/v4/02_phase1_2_review.md`
  - `planning/reviews/v4/03_phase1_3_review.md`
  - `planning/reviews/v4/04_phase1_4_review.md`
  - `planning/reviews/v4/05_phase1_5_review.md`
  - `planning/reviews/v4/06_phase1_consolidation_review.md`
  - `planning/reviews/v4/07_phase1_followup_fix_review.md`

## Current Blockers
- None.

## Next Recommended Steps
1. Start Phase 2.1 and model first-class registry persistence objects.
2. Keep `commit_state` out of the new runtime path entirely.
3. Preserve the Phase 1 rule that semantic subsystems return typed errors instead of panicking or falling back.
4. Extend the remaining unsupported builtin verifier surface only when the required registry/artifact/execution infrastructure exists.

## Notes
- Existing v3 planning and type-system notes remain background context only.
- Frontend work remains intentionally deferred.
- External review tooling degraded during the Phase 1 checkpoint; see `planning/reviews/v4/review_notes_README.md`.
