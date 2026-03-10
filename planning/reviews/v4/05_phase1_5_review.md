Phase 1.5 Review: Conformance Model
===================================

Scope
-----

- [conformance.rs](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/src/conformance.rs)
- supporting crate-surface update:
  - [lib.rs](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/src/lib.rs)
- governing docs:
  - [architecture.md](/Users/rileyleff/Documents/dev/ozzydb/planning/v4/architecture.md)
  - [implementation_plan.md](/Users/rileyleff/Documents/dev/ozzydb/planning/v4/implementation_plan.md)

What changed
------------

- Replaced the placeholder conformance model with an explicit record/attempt split.
- Added:
  - `VerificationFailure`
  - `VerificationAttempt`
  - `ConformanceRecord::declared(...)`
  - `ConformanceRecord::record_report(...)`
  - `ConformanceRecord::record_failure(...)`
  - `latest_report()` / `latest_evidence()` helpers
- Removed top-level duplicated evidence storage from `ConformanceRecord`.
- Kept semantic status constrained to:
  - `declared`
  - `verified`
  - `rejected`

Review method
-------------

- Self-review plus targeted unit coverage.
- External review tooling was again skipped for this narrow Phase 1 checkpoint.

Findings fixed before commit
----------------------------

1. The old model stored evidence directly on `ConformanceRecord` without any attempt history.

   Why it mattered:
   - v4 explicitly says verification attempt history and evidence should live separately from semantic state.
   - a top-level `evidence` blob made it easy to lose failed-attempt context and encouraged silent overwrites.

   Fix:
   - moved verification evidence into append-only `VerificationAttempt::Completed` records
   - added `VerificationAttempt::Failed` for verifier execution failures that should not mutate semantic state
   - kept convenience accessors for the latest completed report/evidence

Accepted tradeoffs
------------------

1. `ConformanceRecord` still uses a plain `String` for `artifact_id` in Phase 1.

   Why it is acceptable now:
   - the dedicated `Artifact` primitive is a later phase concern.
   - introducing a speculative `ArtifactId` type here would add churn before the actual artifact model lands.

2. Attempt records are lightweight and do not yet carry timestamps or invocation references.

   Why it is acceptable now:
   - Phase 1 only needs a correct domain model for semantic state versus attempt history.
   - persistence-specific metadata belongs with the registry/database work in Phase 2.

Checkpoint result
-----------------

- `cargo test -p ozzy-types` passes
- no blocking findings remain for Step 1.5
- Phase 1 is complete enough to move toward registry persistence work, unless you want a dedicated Phase 1 consolidation pass first
