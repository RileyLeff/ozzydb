Phase 1.1 Review: `crates/ozzy-types` Skeleton
==============================================

Scope
-----

- Workspace membership update in [Cargo.toml](/Users/rileyleff/Documents/dev/ozzydb/Cargo.toml)
- New crate under `crates/ozzy-types`
- Governing docs:
  - [architecture.md](/Users/rileyleff/Documents/dev/ozzydb/planning/v4/architecture.md)
  - [implementation_plan.md](/Users/rileyleff/Documents/dev/ozzydb/planning/v4/implementation_plan.md)

Review process
--------------

- `cargo test -p ozzy-types`
- direct self-review against the v4 architecture
- attempted external review via Gemini and Claude CLIs

External review status
----------------------

- Gemini degraded into MCP startup noise and did not produce useful findings.
- Claude one-shot review calls did not return usable output in this environment.
- The checkpoint proceeded with explicit self-review plus tests, per `review_notes_README.md`.

Findings fixed before commit
----------------------------

1. Registry duplicate detection was initially keyed only by `TypeVersionId`.

   Why it mattered:
   - It allowed duplicate `(name, version)` pairs if a caller supplied a mismatched ID string.
   - That would have undermined the public `TypeVersion` identity model immediately.

   Fix:
   - Added `TypeVersion::new(name, version, expr)` to derive the public ID from the published name/version pair.
   - Tightened `TypeRegistry::insert` to reject duplicate `(name, version)` pairs even if the ID differs.
   - Added a regression test for the mismatched-ID case.

2. Typed ports initially duplicated their own names inside the port value.

   Why it mattered:
   - It allowed the map key and the embedded `name` field to disagree.
   - That redundancy would have been pure drift surface in later transform typing work.

   Fix:
   - Removed the embedded `name` field from `TypedPort`.
   - Added `TypedPort::new(...)` and `TypedPortSet::insert(...)`.

Open findings
-------------

- No blocking issues found for Phase 1.1.
- The crate is still intentionally skeletal:
  - no canonicalization logic yet
  - no real refinement logic yet
  - no verifier execution yet
- That is acceptable for this checkpoint because Step 1.1 only requires a compilable crate skeleton.

Checkpoint result
-----------------

- `cargo test -p ozzy-types` passes
- review findings for Phase 1.1 are resolved
- ready to proceed to Phase 1.2
