Phase 1.3 Review: Canonicalization And Relations
================================================

Scope
-----

- [canonical.rs](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/src/canonical.rs)
- [relations.rs](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/src/relations.rs)
- supporting syntax/cargo updates:
  - [syntax.rs](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/src/syntax.rs)
  - [lib.rs](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/src/lib.rs)
  - [Cargo.toml](/Users/rileyleff/Documents/dev/ozzydb/Cargo.toml)
  - [crates/ozzy-types/Cargo.toml](/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-types/Cargo.toml)
- governing docs:
  - [architecture.md](/Users/rileyleff/Documents/dev/ozzydb/planning/v4/architecture.md)
  - [implementation_plan.md](/Users/rileyleff/Documents/dev/ozzydb/planning/v4/implementation_plan.md)

What changed
------------

- Added canonicalization of the v1 type surface, including:
  - alias expansion
  - sorted record fields
  - flattened/deduplicated intersections
  - `never` reduction for obvious conflicts
  - canonical constructor merging for `csv`, `min`, `max`, and `enum`
- Added canonical IDs and an in-memory canonical interner.
- Added strict `equivalent(...)` and conservative `refines(...)`.
- Added first structural refinement rules for:
  - records
  - collections
  - tables
  - builtin constructor families

External review
---------------

- Gemini produced a useful review pass.
- The useful finding was addressed before commit.

Findings fixed before commit
----------------------------

1. `canonicalize(...)` was validating the entire local definition set on every call.

   Why it mattered:
   - During interning or later registry publication, that would have created avoidable `O(N^2)` behavior.
   - The canonicalizer only needs the root expression and the aliases it actually traverses.

   Fix:
   - Removed the eager `validate_all()` call from `canonicalize(...)`.
   - Kept explicit validation of the root expression.
   - Added validation of each traversed local alias at the point of expansion.

2. The temporary recursion-limit workaround became dead scaffolding after the hash path changed.

   Why it mattered:
   - It existed only to support the earlier `serde_json`-based canonical fingerprint.
   - After switching to an explicit fingerprint over the canonical AST, it no longer served a purpose.

   Fix:
   - Removed the crate-level `recursion_limit` attribute.

Accepted tradeoffs
------------------

1. Source-side intersections are still treated conservatively.

   Meaning:
   - `(A & B) <: C` currently succeeds if one component refines `C`, or if `C` is itself an intersection and each component can be satisfied separately.
   - The engine does **not** yet perform structural merge of intersected records.

   Why it is acceptable now:
   - The v4 plan explicitly asks for conservative refinement.
   - Structural merge, if needed, can be added later as a deliberate canonicalization improvement rather than sneaking in under the current rules.

2. Canonical fingerprints are now derived from an explicit AST writer, not generic serde serialization.

   Why it is acceptable now:
   - The fingerprint is deterministic and under OzzyDB’s control.
   - It avoids serializer trait-recursion problems and keeps canonical identity logic explicit.

Checkpoint result
-----------------

- `cargo test -p ozzy-types` passes
- no blocking findings remain for Step 1.3
- ready to start Step 1.4
