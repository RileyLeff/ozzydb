Phase 1.2 Review: Core Type Language Surface
============================================

Scope
-----

- [syntax.rs](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/src/syntax.rs)
- [lib.rs](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/src/lib.rs)
- workspace dependency update in [Cargo.toml](/Users/rileyleff/Documents/dev/ozzydb/Cargo.toml)
- crate dependency update in [crates/ozzy-types/Cargo.toml](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/Cargo.toml)
- governing docs:
  - [architecture.md](/Users/rileyleff/Documents/dev/ozzydb/planning/v4/architecture.md)
  - [implementation_plan.md](/Users/rileyleff/Documents/dev/ozzydb/planning/v4/implementation_plan.md)

What changed
------------

- Added explicit builtin leaf types and builtin constructors.
- Added local named type definitions and local expression validation.
- Made builtin names promote out of generic refs.
- Added constructor argument validation and duplicate-record-field checks.
- Added explicit validation for local aliases and external version-pinned refs.

External review
---------------

- Gemini produced one useful review pass.
- Claude review subprocesses remained unreliable in this environment.

Findings fixed before commit
----------------------------

1. `Literal::Float(f64)` would have blocked `Eq`/`Hash` on the AST.

   Why it mattered:
   - Step 1.3 needs canonicalization and interning.
   - Leaving raw `f64` in the syntax tree would force awkward workarounds immediately.

   Fix:
   - Replaced `Literal::Float(f64)` with `Literal::Float(OrderedFloat<f64>)`.
   - Added `ordered-float` as an explicit dependency.
   - Derived `Eq`/`Hash` on the relevant syntax structs.

2. `TypeExpr::Table` was too restrictive.

   Why it mattered:
   - `table<R>` in the architecture is supposed to take a row type `R`, including named aliases.
   - The initial `Table(RecordExpr)` shape forced inline row definitions and contradicted the v4 grammar.

   Fix:
   - Changed `TypeExpr::Table` to wrap `Box<TypeExpr>`.
   - Added a local validation test using a named row type alias.

3. `TypeExpr::intersection(...)` allowed construction of an invalid empty intersection.

   Why it mattered:
   - The architecture says intersections must be non-empty.
   - The helper was bypassing that invariant and relying on later validation.

   Fix:
   - Changed the helper to return `Result<TypeExpr, TypeLanguageError>`.
   - Added a regression test for the empty case.

Accepted tradeoffs
------------------

1. `RecordExpr` still stores fields as `Vec<RecordField>`.

   Why it is acceptable now:
   - Canonical sorting belongs in Step 1.3.
   - Keeping the authored field order in the surface AST is reasonable for this phase.

2. `BuiltinConstructor` is closed for v1.

   Why it is acceptable now:
   - The v4 plan explicitly says the initial builtin constructor set should stay small.
   - If extension points are needed later, they should be added deliberately rather than by keeping the constructor surface stringly-typed by default.

Checkpoint result
-----------------

- `cargo test -p ozzy-types` passes
- no blocking findings remain for Step 1.2
- ready to start Step 1.3
