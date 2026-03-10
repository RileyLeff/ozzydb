# v4 Workflow State

## Current Phase: Phase 3 underway
## Current Step: Phase 3.1 complete; ready for Phase 3.2 publication bundle rewrite

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

### Phase 2.1: First-class registry persistence objects
- Added a new additive PostgreSQL migration:
  - `crates/ozzy-server/migrations/004_v4_registry.sql`
- Introduced a dedicated v4 DB module:
  - `crates/ozzy-server/src/db/v4/mod.rs`
  - `crates/ozzy-server/src/db/v4/models.rs`
  - `crates/ozzy-server/src/db/v4/queries.rs`
- Added first-class persisted rows for:
  - canonical types
  - type versions
  - environment versions
  - transform versions
  - transform ports
  - registry revisions
  - registry revision memberships
  - project revisions
  - invocations
  - conformance records
  - verification attempts
- Scoped versioned registry objects by `project_id` and kept canonical types global.
- Added project-to-registry revision integrity checks in the schema instead of relying on application convention.
- Updated local Postgres references from `postgres:17-alpine` to `postgres:18-alpine` in the checked-in Docker Compose/test docs that would otherwise fail to run the new `uuidv7()` migration.
- Added a DB-backed v4 round-trip test in the server lib test module. It is authored and compiled as part of the library surface, but it was not executed in this environment because `DATABASE_URL` is unset.
- Verification for this checkpoint:
  - `cargo check -p ozzy-server`
  - `cargo test -p ozzy-types`

### Phase 2.2: Immutable registry snapshots
- Added `crates/ozzy-server/src/registry.rs` as the server-side v4 snapshot layer.
- Added immutable snapshot loading for pinned registry revisions.
- Added a small in-memory `RegistrySnapshotCache` to `AppState`, keyed by registry revision ID.
- Added snapshot loader entry points for:
  - direct registry revision lookup
  - project revision lookup by source commit
- Added batch DB query helpers to load:
  - registry revisions
  - canonical types for a revision
  - transform ports for all transforms in a revision
- Reconstructed the following into immutable snapshot state:
  - canonical types
  - published type versions
  - canonical-equivalence classes
  - environment versions
  - transform versions with typed input/output ports
- Added a DB-gated server test that loads and reuses cached snapshots for one registry revision.
- Verification for this checkpoint:
  - `cargo check -p ozzy-server`
  - `cargo test -p ozzy-types`
  - the new DB-gated server snapshot test was authored, but the `cargo test -p ozzy-server ...` link/run step remained a poor checkpoint signal in this harness

### Phase 2.2 cleanup pass
- Wired the pinned snapshot layer into live server flows:
  - `fetch`
  - `compute::orchestrator`
  - endpoint inspection
- Added registry-backed `equivalent(...)` and `refines(...)` queries on `RegistrySnapshot`.
- Reworked `RegistrySnapshotCache` into a bounded single-flight cache instead of an unbounded map.
- Split internal snapshot corruption from user-facing unknown-type errors with a dedicated `MissingStoredTypeRowForResolvedType` error.
- Removed touched silent fallback paths from fetch/orchestrator/endpoint inspection.
- Verification for this checkpoint:
  - `cargo check -p ozzy-server`
  - `cargo test -p ozzy-types`

### Phase 2.3: Project revision objects
- Added `crates/ozzy-server/migrations/005_v4_project_revision_payloads.sql`.
- Extended `v4_project_revisions` to persist the authored runtime payloads needed to interpret a published commit:
  - `environments`
  - `transforms`
  - `endpoints`
  - `project_meta`
- Extended the v4 DB models/query layer so project revisions round-trip those payloads.
- Added `PublishedProjectRevision` as the server-visible object that combines:
  - the stored project revision row
  - the pinned `RegistrySnapshot`
  - bound runtime definitions
  - published endpoint definitions
- Moved runtime server reads onto `PublishedProjectRevision`:
  - `fetch`
  - `compute::orchestrator`
  - endpoint inspection
- Removed direct `commit_state` reads from those runtime paths.

### Phase 2.3 cleanup pass
- Removed `NodeDef.machine` from the authored endpoint model and made unknown `machine` fields parse errors.
- Removed `machine` from endpoint inspection responses and stopped using node-level provider selection in the orchestrator.
- Moved commit detail reads onto `PublishedProjectRevision` instead of `commit_state`.
- Added `project_meta` to commit detail responses so commit inspection reflects the published project revision payloads.
- Added `crates/ozzy-server/migrations/006_v4_project_revision_payload_checks.sql` to enforce object-shaped JSON payloads for:
  - `environments`
  - `transforms`
  - `endpoints`
  - `project_meta`
- Renamed snapshot-binding errors so they refer to published project revisions instead of legacy commit-state terminology.
- Added a DB-backed regression test proving non-object project-revision payloads are rejected.
- Verification for this checkpoint:
  - `cargo test -p ozzy-core`
  - `cargo check -p ozzy-server --tests`
  - `cargo test -p ozzy-types`
- Added test coverage for:
  - project revision payload persistence
  - published project revision loading from a pinned snapshot

### Phase 3.1: `ozzy.toml` parser rewrite
- Moved schema/witness helpers from `ozzy-core` into `ozzy-types` and removed the dependency-cycle blocker.
- Added `ozzy-types::parse` with a minimal v1 parser for:
  - full top-level type expressions
  - port-level type references
- Rewrote `ozzy_core::toml_spec` around:
  - top-level `[types]`
  - typed transform `inputs` and `outputs`
  - removal of `output` and `output_schema`
- Constrained authored transforms to exactly one output port until endpoint edges can address output ports explicitly.
- Tightened validation so:
  - ports cannot inline arbitrary type expressions
  - builtin type refs cannot be version-pinned
  - transform binding to published snapshot rows compares both port names and resolved type identities
- Removed the old silent schema fallback behavior when moving witness/schema parsing into `ozzy-types`.
- Verification for this checkpoint:
  - `cargo test -p ozzy-core`
  - `cargo test -p ozzy-types`
  - `cargo check -p ozzy-server --tests`
- Review artifact:
  - `planning/reviews/v4/13_phase3_1_review.md`
- Verification for this checkpoint:
  - `cargo check -p ozzy-server`
  - `cargo test -p ozzy-types`

## Open Review Findings
- None blocking for Phase 2.3.
- Review artifacts:
  - `planning/reviews/v4/01_phase1_1_review.md`
  - `planning/reviews/v4/02_phase1_2_review.md`
  - `planning/reviews/v4/03_phase1_3_review.md`
  - `planning/reviews/v4/04_phase1_4_review.md`
  - `planning/reviews/v4/05_phase1_5_review.md`
  - `planning/reviews/v4/06_phase1_consolidation_review.md`
  - `planning/reviews/v4/07_phase1_followup_fix_review.md`
  - `planning/reviews/v4/08_phase2_1_review.md`
  - `planning/reviews/v4/09_phase2_2_review.md`
  - `planning/reviews/v4/10_phase2_2_cleanup_review.md`
  - `planning/reviews/v4/11_phase2_3_review.md`

## Current Blockers
- None.

## Next Recommended Steps
1. Start Phase 3.1 and replace the remaining v3 parser/control structures with a v4-oriented `ozzy.toml` ingestion surface.
2. Rewrite push so it publishes `PublishedProjectRevision` payloads directly instead of leaving them as manual/test-only objects.
3. Preserve the Phase 1 rule that semantic subsystems return typed errors instead of panicking or falling back.
4. Extend the remaining unsupported builtin verifier surface only when the required registry/artifact/execution infrastructure exists.

## Notes
- Existing v3 planning and type-system notes remain background context only.
- Frontend work remains intentionally deferred.
- External review tooling degraded during the Phase 1 checkpoint; see `planning/reviews/v4/review_notes_README.md`.
