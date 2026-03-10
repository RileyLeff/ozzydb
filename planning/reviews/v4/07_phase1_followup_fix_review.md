Phase 1 Follow-Up Fix Review
============================

Scope
-----

- [registry.rs](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/src/registry.rs)
- [verify/mod.rs](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/src/verify/mod.rs)
- [conformance.rs](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/src/conformance.rs)
- [architecture.md](/Users/rileyleff/Documents/dev/ozzydb/planning/v4/architecture.md)

Why this pass happened
----------------------

A Phase 1 review found two real correctness gaps:

1. conjunctive verification could not naturally consume multiple witness views of the same artifact
2. versioned published type refs could not be verified through the registry

It also found one API sharp edge:

3. `ConformanceRecord` helper names implied "latest" meant latest attempt, when they actually meant latest completed report

What changed
------------

- Added registry-backed type-ref resolution:
  - `TypeRegistry::get_by_name_version(...)`
  - `TypeRegistry::resolve_ref(...)`
- Changed verifier compilation and execution to require a registry context.
- Removed `VerificationPlan::ExternalRef` and replaced it with recursive registry-backed plan compilation for published refs.
- Added `VerificationInput::Derived(...)` so a single verification call can satisfy conjunctive checks through multiple witness views of one artifact.
- Renamed conformance helpers to make the semantics explicit:
  - `latest_attempt()`
  - `latest_completed_report()`
  - `latest_completed_evidence()`
- Updated the architecture doc so the remaining verifier-surface gap is accurately staged across Phases 2 through 5.

Findings fixed before commit
----------------------------

1. Published type refs were still verification-dead ends.

   Fix:
   - verifier compilation now resolves versioned refs through `TypeRegistry`
   - added regression coverage proving a versioned published type can be verified through the registry

2. Conjunctive verification required one input shape per subplan, with no way to model multiple witness views of one artifact.

   Fix:
   - added `VerificationInput::Derived(Vec<VerificationInput>)`
   - `VerificationPlan::All` now allows each subplan to match against the derived witness set
   - added regression coverage for `csv(...) & table<Row>`

3. Conformance helper names were stale after the attempt-history model landed.

   Fix:
   - renamed the helpers so "latest" is no longer ambiguous
   - added coverage for latest attempt vs latest completed report/evidence

Accepted tradeoffs
------------------

1. `VerificationInput::Derived(...)` currently uses ordered candidate matching.

   Meaning:
   - a non-`All` verification plan tries derived candidates in order and takes the first compatible one.

   Why it is acceptable now:
   - the current witness families are intentionally small and non-overlapping in normal use.
   - if derived witness ambiguity becomes real later, Phase 2/4 artifact-backed witness generation is the right place to impose a stronger contract.

Checkpoint result
-----------------

- `cargo test -p ozzy-types` passes
- no blocking findings remain from the Phase 1 review
- Phase 2 can start with the library in a cleaner state than before this follow-up pass
