# Agent Whiteboard — v4 Planning

## Session 2026-03-09: v4 Architecture Scaffold

### Decisions Carried Forward
1. v4 is organized around six primitives:
   - `TypeVersion`
   - `EnvironmentVersion`
   - `TransformVersion`
   - `Artifact`
   - `Invocation`
   - `ConformanceRecord`
2. `EnvironmentVersion` is first-class, not just a field on transforms.
3. Convertibility is derived from typed transforms, not modeled as a primitive type relation.
4. The sanctioned relation set is intentionally small:
   - `refines`
   - `equivalent`
   - `conforms_to`
5. The type language must support:
   - conjunction/refinement
   - product/record types
   - collection types
6. Conformance states stay simple:
   - `declared`
   - `verified`
   - `rejected`
7. Verification logs and failed verification attempts belong in evidence/history, not a separate semantic state.

### Architectural Pressure Points To Revisit
- Exact type-language syntax and normalization rules.
- Exact witness/evidence schema returned by verification.
- How runtime requirements belong inside type identity for artifacts like pickle-backed objects.
- How much of transform execution policy belongs on `TransformVersion` versus `Invocation`.
- When and how OzzyDB should auto-insert transforms, if at all.

### Immediate Next Docs After Review
- `implementation_details.md`
- updated `soul.md`
- a v4-specific type-system document or merged section, depending on how much of the v3 type note cluster survives intact.
