Phase 1.4 Review: Verification Planning And Witnesses
===================================================

Scope
-----

- [verify/mod.rs](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/src/verify/mod.rs)
- [verify/witness.rs](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/src/verify/witness.rs)
- supporting crate-surface updates:
  - [lib.rs](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/src/lib.rs)
  - [crates/ozzy-types/Cargo.toml](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/Cargo.toml)
- governing docs:
  - [architecture.md](/Users/rileyleff/Documents/dev/ozzydb/planning/v4/architecture.md)
  - [implementation_plan.md](/Users/rileyleff/Documents/dev/ozzydb/planning/v4/implementation_plan.md)

What changed
------------

- Added a first verification-planning layer:
  - `VerificationInput`
  - `VerificationPlan`
  - `RecordFieldPlan`
  - `BuiltinVerifierRegistry`
  - `VerificationError`
- Implemented verifier compilation from canonicalized type expressions.
- Implemented builtin verification for the initial v1 surface:
  - scalar builtin checks
  - `csv(...)`
  - `min(...)`, `max(...)`, `enum(...)`
  - record verification
  - collection verification
  - table verification
- Reused `ozzy-core::schema` narrowly to derive `TableWitness` from Parquet files.
- Expanded the public crate surface so later phases can call the verifier and witness APIs directly.

Review method
-------------

- Self-review plus targeted tests.
- External review CLIs were intentionally skipped here because the current repository guidance deprioritizes the stale external-model wrapper path.

Findings fixed before commit
----------------------------

1. Verification still contained "validated earlier" branches that could panic or silently collapse malformed constructor state.

   Why it mattered:
   - v4 explicitly rejects silent fallback paths in core semantic code.
   - malformed constructor state should surface as structured verifier errors, not as implicit rejection or panic.

   Fix:
   - added explicit `VerificationError::MissingConstructorArg` and `VerificationError::InvalidConstructorArg`
   - removed the panic-based `list_arg(...)` helper
   - removed the `unwrap_or(false)` path in scalar constructor verification
   - added a regression test proving malformed `min(...)` input errors instead of degrading into a false rejection

Accepted tradeoffs
------------------

1. Verification is witness-driven in Phase 1.

   Meaning:
   - `csv(...)` currently verifies against `CsvWitness`, not raw bytes.
   - table verification currently works against `TableWitness` or a Parquet file path that can be converted into a `TableWitness`.

   Why it is acceptable now:
   - Phase 1 is about the verifier model and witness flow, not full artifact ingestion.
   - later phases can add more direct artifact-backed verifier inputs without changing the semantic contract.

2. Some builtin constraints remain intentionally unsupported in this phase.

   Meaning:
   - `unit(...)` currently returns an explicit unsupported error because v4 does not yet have measurement metadata wired into witness generation.
   - `json`, `date`, and `datetime` are not yet implemented as full scalar/table verifiers.

   Why it is acceptable now:
   - the architecture explicitly allows the builtin surface to grow incrementally.
   - returning a typed error is preferable to guessing or silently broadening acceptance.

Checkpoint result
-----------------

- `cargo test -p ozzy-types` passes
- no blocking findings remain for Step 1.4
- ready to start Step 1.5
