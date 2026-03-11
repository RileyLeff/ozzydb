# v4 Workflow State

## Current Phase: Phase 5 underway
## Current Step: Phase 5.3 next; typed fetch rewrite is complete

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

### Phase 5.1: Bind runtime execution to published transform and environment versions
- Enriched `RuntimeTransformDef` so runtime bindings now carry:
  - published `versioned_name`
  - published `row_id`
  - typed `inputs`
  - typed `outputs`
  - bound `RuntimeEnvironmentDef`
- Bound authored runtime definitions to published snapshot rows using typed port
  identity instead of only authored transform names.
- Reworked cache planning and node execution to resolve environments directly
  from the bound published environment on the runtime transform.
- Added strict node input-binding validation before cache lookup and before
  compute execution.
- Persisted cache metadata now records the published transform versioned name
  instead of the authored transform label.
- Verification for this checkpoint:
  - `cargo test -p ozzy-types`
  - `cargo check -p ozzy-server --tests`
- External review degraded again for this checkpoint, so the milestone gate was
  completed with direct self-review plus tests.

### Phase 5.2 slice: Invocation and output artifact persistence
- Added the first live execution-side use of:
  - `v4_invocations`
  - `v4_artifacts`
  - `v4_invocation_artifacts`
  - `v4_conformance_records`
- Successful node execution now:
  - inserts a `running` invocation after the per-node cache check
  - persists the output artifact
  - binds that artifact to the invocation output port
  - declares output conformance against the published output type
  - marks the invocation `succeeded`
- Added a transactional DB helper so output artifact persistence and invocation
  success transition happen atomically.
- Failed compute or post-compute persistence now marks the invocation `failed`
  instead of leaving it stranded in `running`.
- `NodeOutput` now carries optional artifact identity so downstream invocation
  input metadata can include upstream artifact IDs when available.
- Remaining work for full Phase 5.2:
  - remove old leaf-source dependence in fetch/orchestrator
  - move required input resolution onto first-class artifacts
  - add input conformance gating
  - add output verification where policy requires it
- Verification for this checkpoint:
  - `cargo check -p ozzy-server --tests`
  - `cargo test -p ozzy-types`
- Review artifact:
  - `planning/reviews/v4/22_phase5_2_invocation_artifacts_review.md`

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

### Phase 3.2: Atomic publication bundle rewrite
- Added `crates/ozzy-server/src/publication.rs` as the dedicated v4 publication subsystem.
- Replaced the live push write path so `POST /v1/push` now publishes:
  - versioned type rows
  - versioned environment rows
  - versioned transform rows and typed ports
  - a new registry revision
  - a new project revision
  in one transaction
- Made the publication transaction lock the project row so per-project auto-version assignment is serialized.
- Added numeric auto-version assignment for:
  - `TypeVersion`
  - `EnvironmentVersion`
  - `TransformVersion`
  with reuse of existing equivalent definitions instead of duplicate republishing
- Rewrote published transform payloads so transform port type refs are stored as explicit published version pins.
- Tightened authored transform-port validation so direct builtin refs are rejected; ports must use:
  - a named local type from `[types]`, or
  - a published version-pinned type ref
- Tightened duplicate push handling so an existing same-SHA commit without a published v4 project revision is treated as an internal error, not a valid idempotent success case.
- Kept provider-specific environment building as post-publication async work; this remains a Phase 3.3 concern.
- Verification for this checkpoint:
  - `cargo test -p ozzy-core`
  - `cargo test -p ozzy-types`
  - `cargo check -p ozzy-server --tests`
- Review artifact:
  - `planning/reviews/v4/14_phase3_2_review.md`

### Phase 3.3: Environment publication / realization split
- Added `PublishedEnvironmentDef` as the content-bound published environment payload shape.
- Changed environment publication so authored lockfile and Dockerfile paths are resolved at push time and persisted as published environment definitions.
- Changed transform publication so stored transform payloads refer to version-pinned environments like `python_sci@1`, not authored environment names.
- Changed project revision environment payloads to store authored-name -> published-version bindings instead of raw authored environment path specs.
- Reworked runtime binding so fetch/orchestrator resolve environments from published environment rows in the pinned snapshot.
- Reworked environment hashing and build resolution so provider realization is keyed off the published environment definition, not ad hoc git fetches at execution time.
- Reworked async post-push environment builds so they consume published environment rows directly.
- Tightened environment source handling so invalid/missing lockfiles and Dockerfiles fail publication-time validation instead of degrading at build/runtime.
- Verification for this checkpoint:
  - `cargo test -p ozzy-core`
  - `cargo test -p ozzy-types`
  - `cargo check -p ozzy-server --tests`
- Review artifact:
  - `planning/reviews/v4/15_phase3_3_review.md`

### Phase 3.3 follow-up: Environment identity normalization
- Removed authored path strings from the logical published environment identity:
  - `BaseLockfile` now stores normalized installer strategy + resolved content
  - `Dockerfile` now stores resolved content only
- Changed environment hashing and publication dedup so path renames no longer create new logical `EnvironmentVersion`s or rebuild keys.
- Moved base-lockfile installer classification to publication time so build-time realization no longer infers semantics from authored file paths.
- Updated DB-backed fixture payloads and snapshot tests to use the normalized published environment shape.
- Verification for this checkpoint:
  - `cargo test -p ozzy-core`
  - `cargo test -p ozzy-types`
  - `cargo check -p ozzy-server --tests`
- Review artifact:
  - `planning/reviews/v4/16_phase3_3_env_identity_fix_review.md`

## Open Review Findings
- None blocking for Phase 5.1.
- Residual legacy debt to delete later:
  - `commit_state` helpers in the DB/test surface
  - `register_commit_atomically(...)` in legacy tests
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
  - `planning/reviews/v4/12_phase2_3_cleanup_review.md`
- `planning/reviews/v4/13_phase3_1_review.md`
- `planning/reviews/v4/14_phase3_2_review.md`
- `planning/reviews/v4/15_phase3_3_review.md`
- `planning/reviews/v4/16_phase3_3_env_identity_fix_review.md`
- `planning/reviews/v4/17_phase4_1_review.md`
- `planning/reviews/v4/18_phase4_2_review.md`
- `planning/reviews/v4/19_phase4_3_review.md`
- `planning/reviews/v4/20_pre_phase5_source_fallback_fix_review.md`

## Current Blockers
- None.

## Next Recommended Steps
1. Start Phase 5.1 and bind execution to `TransformVersion`, typed ports, and pinned registry snapshots.
2. Remove the remaining dead `commit_state`/legacy publication helpers once the old DB/e2e tests are moved onto the v4 publication path.
3. Preserve the Phase 1 rule that semantic subsystems return typed errors instead of panicking or falling back.
4. Extend the remaining unsupported builtin verifier surface only when the required registry/artifact/execution infrastructure exists.

## Notes
- Existing v3 planning and type-system notes remain background context only.
- Frontend work remains intentionally deferred.
- External review tooling degraded during the Phase 1 checkpoint; see `planning/reviews/v4/review_notes_README.md`.

### Phase 4.1: First-class `Artifact` foundation
- Added additive v4 persistence for `Artifact` in `migrations/007_v4_artifacts.sql`.
- Introduced:
  - `v4_artifacts`
  - `v4_invocation_artifacts`
- Added Rust models and query helpers for:
  - blob artifacts backed by `content_refs`
  - manifest artifacts
  - invocation input/output artifact bindings
- Kept the old `DataAtom` / `Collection` runtime surface untouched for now; this phase is foundation only.
- Deliberately deferred the `v4_conformance_records.artifact_id` foreign key to Phase 4.3 so the artifact-backed conformance migration can happen explicitly instead of being forced indirectly by schema timing.
- Verification for this checkpoint:
  - `cargo test -p ozzy-types`
  - `cargo check -p ozzy-server --tests`
- Review artifact:
  - `planning/reviews/v4/17_phase4_1_review.md`

### Phase 4.2: Typed bundle and collection artifacts
- Added `ozzy_core::artifacts` as the shared manifest model for artifact-backed structure.
- Introduced typed manifest variants:
  - `ArtifactManifest::Bundle { entries }`
  - `ArtifactManifest::Collection { items }`
- Added explicit Rust-side manifest validation instead of treating manifest payloads as unstructured JSON.
- Added `migrations/008_v4_artifact_manifest_checks.sql` so manifest artifacts must declare a supported outer shape in the database:
  - `bundle` with `entries`
  - `collection` with `items`
- Added v4 query helpers for:
  - validated manifest artifact creation
  - manifest decoding from stored artifacts
  - same-project member validation before persistence
- Kept the old `Collection` API/runtime surface untouched for now; this phase replaces the v4 ontology, not the public API contract.
- Verification for this checkpoint:
  - `cargo test -p ozzy-core`
  - `cargo test -p ozzy-types`
  - `cargo check -p ozzy-server --tests`
- Review artifact:
  - `planning/reviews/v4/18_phase4_2_review.md`

### Phase 4.3: Artifact-backed conformance
- Added `migrations/009_v4_conformance_artifact_fk.sql` so persisted conformance rows now have a real foreign-key relationship to `v4_artifacts`.
- Tightened v4 conformance writes so conformance records only accept:
  - existing artifacts
  - existing type versions
  - artifact/type pairs from the same project
- Added explicit query helpers for:
  - listing conformance records for an artifact
  - listing verification attempts for a conformance record
- Replaced raw verification-attempt inserts with semantic recording helpers:
  - completed verification now updates conformance status to `verified` or `rejected`
  - failed verification attempts keep semantic status unchanged and only update attempt history / `updated_at`
- Added DB-gated tests covering:
  - verified status transitions
  - failed-attempt persistence without semantic downgrade
  - cross-project artifact/type rejection
- Verification for this checkpoint:
  - `cargo test -p ozzy-core`
  - `cargo test -p ozzy-types`
  - `cargo check -p ozzy-server --tests`
- Review artifact:
  - `planning/reviews/v4/19_phase4_3_review.md`

### Pre-Phase 5.1 cleanup: eliminate source fallback
- Removed the remaining degraded source path before execution integration work.
- `retrieve_source_code(...)` now returns explicit errors instead of warning and returning `None`.
- Added `endpoint_requires_source_code(...)` so source retrieval is only required for endpoints that actually contain source-backed transforms.
- Source-backed transforms now fail if extracted source is missing instead of hashing `transform_name:commit_sha`.
- Cache checking now propagates materialization/source errors instead of treating them as cache misses.
- Added fetch-level unit tests covering:
  - source-required endpoint detection
  - source-transform hashing failure when extracted source is absent
- Verification for this checkpoint:
  - `cargo test -p ozzy-types`
  - `cargo check -p ozzy-server --tests`
- Review artifact:
  - `planning/reviews/v4/20_pre_phase5_source_fallback_fix_review.md`

### Phase 5.1: Bind `TransformVersion` to execution
- Bound execution to published `TransformVersion` data through pinned
  `PublishedProjectRevision` snapshots.
- Execution now resolves typed transform input/output ports and published
  `EnvironmentVersion`s instead of using the older authored-transform runtime
  model.
- Verification for this checkpoint:
  - `cargo test -p ozzy-types`
  - `cargo check -p ozzy-server --tests`
- Review artifact:
  - `planning/reviews/v4/21_phase5_1_review.md`

### Phase 5.2: Invocation/output artifact slice
- Successful node execution now creates real `v4_invocations`.
- Successful node execution now persists output artifacts and declared output
  conformance.
- Invocation success/failure transitions are transactional instead of leaving
  stale `running` rows on cache hits or failed persistence.
- Verification for this checkpoint:
  - `cargo test -p ozzy-types`
  - `cargo check -p ozzy-server --tests`
- Review artifact:
  - `planning/reviews/v4/22_phase5_2_invocation_artifacts_review.md`

### Phase 5.2 completion: typed fetch and artifact-bound inputs
- Replaced endpoint leaf ingress with typed endpoint inputs.
  - endpoints now declare `[endpoints.<name>.inputs.<port>]`
  - endpoint edges now use `input:<port>`
- `POST /v1/fetch/...` now accepts explicit artifact bindings per endpoint input
  instead of the old anonymous `data:` / `collection:` model.
- Fetch now validates:
  - exact endpoint input coverage
  - same-project artifact ownership
  - existence of non-rejected conformance for the required endpoint input type
- Job dedup now includes `input_bindings_hash` as part of the identity.
- Runtime input manifests for Python and R are now recursive artifact-backed
  bundle/collection manifests instead of the old `is_collection` flag contract.
- The live v4 fetch/orchestrator path no longer depends on `data:` or
  `collection:` ingress.
- Verification for this checkpoint:
  - `cargo test -p ozzy-core`
  - `cargo test -p ozzy-types`
  - `cargo check -p ozzy-core -p ozzy-types -p ozzy-server --tests`
- Review artifact:
  - `planning/reviews/v4/23_phase5_2_typed_fetch_review.md`

### Phase 5.3: Cache identity rewrite
- Replaced the old v3-style materialized cache identity with a v4 cache key based on:
  - sorted `(input_name, artifact_id)` bindings
  - published `TransformVersion` identity
  - published `EnvironmentVersion` identity
  - `source_hash`
  - `params_hash`
  - optional `secrets_hash`
- Added `migrations/011_v4_materialized_cache_identity.sql` to replace the old cache row shape with a v4 row that stores:
  - `project_revision_id`
  - `transform_version_id`
  - `environment_version_id`
  - `params_hash`
  - `input_artifact_bindings`
  - `source_hash`
  - `secrets_hash`
  - `output_artifact_id`
- Removed the live runtime dependence on:
  - `transform_hash(...)`
  - `platform_hash`
  - `verification_tier`
- Cache hits now propagate `output_artifact_id` through `NodeOutput`, so downstream nodes hash on artifact identity instead of output content hash.
- Invocation input bindings are now strict artifact-bound JSON objects and no longer silently carry `null` artifact IDs or redundant content hashes.
- Rewrote the DB integration test for `materialized_cache` to use v4 project revision, transform/environment version, and output artifact fixtures.
- Verification for this checkpoint:
  - `cargo fmt`
  - `cargo test -p ozzy-core`
  - `cargo test -p ozzy-types`
  - `cargo check -p ozzy-server --tests`
- Review artifact:
  - `planning/reviews/v4/24_phase5_3_review.md`


## Current Phase: Phase 5 underway
## Current Step: Phase 5.4 next; cache identity rewrite is complete
