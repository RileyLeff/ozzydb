# v4 Architecture

See also: `../v3/soul.md` (current project principles), `../v3/implementation_plan.md` (last implementation plan), `../v3/thoughts_on_types.md` and related v3 type notes.

---

## Purpose

v4 reorganizes OzzyDB around a first-class type system and a first-class compute plane.

The main v3 architecture treated types, environments, transforms, and execution as related concerns, but not yet as one coherent object model. v4 makes that model explicit.

The core idea is:

- artifacts are concrete data
- types are versioned contracts over artifacts
- transforms are versioned typed morphisms implemented by source code
- environments are versioned execution substrates
- invocations are concrete applications of transforms to artifacts
- conformance is an explicit claim, backed by executable verification

This is the level where OzzyDB should live. It is not just blob storage plus jobs, and it is not just a type graph. It is a provenance system over typed computation.

---

## Core Thesis

OzzyDB has two tightly coupled planes:

1. The **data/type plane**
   - what an artifact is
   - what guarantees it satisfies
   - how those guarantees relate to other guarantees

2. The **compute plane**
   - what code runs
   - in what environment it runs
   - on what typed inputs it runs
   - what typed outputs it promises

The compute DAG sits on top of the type system. A transform is not separate from the type system; it is a typed arrow through it, implemented by real code in a real environment.

---

## The Six Primitives

v4 treats the following as the irreducible core objects.

### 1. `TypeVersion`

An immutable, versioned type definition.

A `TypeVersion` is a contract over artifacts. It is declarative. It is defined in terms of constraints and type constructors, not arbitrary user code.

A type may describe:

- encoding
- structure
- schema
- semantics
- runtime requirements intrinsic to interpreting the artifact

Examples:

- `csv(delimiter=",", header=true)`
- `tabular(columns={species: string, wp: float64, date: date})`
- `record{obs: O, meta: M}`
- `collection<ForestCensus>`

### 2. `EnvironmentVersion`

An immutable, versioned execution substrate.

This is the thing transforms and environment-backed verifiers run in. It is first-class because it has its own identity, reuse, lifecycle, and reproducibility story.

An `EnvironmentVersion` may be defined from:

- base image + lockfile
- Dockerfile
- prebuilt image reference

But those are construction modes, not the conceptual center. The important thing is that the environment is a reproducible execution substrate with stable identity.

### 3. `TransformVersion`

A versioned typed transform implementation.

A `TransformVersion` is a typed morphism with:

- named input ports
- named output ports
- parameter schema
- implementation reference
- execution metadata
- a bound `EnvironmentVersion`

There is one primitive transform concept. "Adapter", "scientific transform", "validator", and similar categories are higher-level roles or traits over the same primitive object.

### 4. `Artifact`

A concrete piece of data.

An artifact is what actually exists in the world: uploaded bytes, a materialized output, a bundle, a manifest-backed collection, and so on.

Artifacts are not types. Types are contracts over artifacts.

### 5. `Invocation`

A concrete application of a `TransformVersion` to specific input artifacts and parameter values.

An invocation is the instance-level node in OzzyDB's compute graph. It binds:

- the transform version
- the environment version used
- the input artifact bindings
- the parameter bindings
- the resulting output artifacts

This is the object that makes provenance concrete.

### 6. `ConformanceRecord`

An explicit claim that an `Artifact` conforms to a `TypeVersion`.

Conformance is not implicit. It must be recorded, with evidence.

Minimum semantic states:

- `declared`
- `verified`
- `rejected`

Verification attempt logs, failures, and debugging details belong in evidence/history, not as extra semantic states.

---

## Sanctioned Relations

v4 keeps the core relation set intentionally small.

### `refines(T1, T2)`

Every artifact conforming to `T1` also conforms to `T2`.

This is the main subtype / guarantee-strengthening relation. It forms the backbone of the type graph.

Examples:

- `WaterPotential` refines `float64`
- `csv(delimiter=",", header=true)` refines `csv`
- `tabular{wp: WaterPotential}` refines `tabular{wp: float64}`

### `equivalent(T1, T2)`

`T1` and `T2` normalize to the same constraint set.

This should be strict. It is not "close enough" or "round-trips through some transform". It is semantic identity after normalization.

In practice this is mostly canonicalization, aliasing, and exact equality of meaning.

### `conforms_to(A, T)`

Artifact `A` has a recorded conformance claim against type `T`.

This relation is carried by `ConformanceRecord`, not inferred from hope or naming.

### Not primitive: `converts_to`

Convertibility is not a primitive type relation.

It is derived from the existence of typed `TransformVersion`s. If there is a transform `f: T1 -> T2`, then OzzyDB knows there is an operational path from `T1` to `T2`.

This keeps the type ontology small and prevents the system from storing the same idea twice.

### Not primitive: siblinghood

Siblinghood is a graph pattern, not a relation kind.

Two types are siblings when they share a broader ancestor but neither refines the other.

---

## Type Language

The type language needs more than simple refinement. v4 assumes three major forms.

### 1. Conjunction / refinement on one artifact

This is the familiar case where one artifact satisfies multiple constraints at once.

Examples:

- `bytes & utf8 & csv(delimiter=",")`
- `float64 & unit("MPa") & <= 0`

### 2. Product / record types

OzzyDB must support composite types built from multiple typed components.

Example:

```text
record{
  obs: WaterPotentialTable,
  meta: SiteMetadata
}
```

This is essential for cases where one logical object is made of multiple tracks of information.

A product type is not automatically a subtype of one of its components. Access to a component should typically happen through explicit transforms such as projection.

### 3. Collection types

OzzyDB must support typed collections and aggregate constraints.

Examples:

- `collection<ForestCensus>`
- `collection<ImageTile>`

Cross-value constraints belong on the smallest type scope that binds all referenced members.

---

## v1 Type Semantics Addendum

The v4 architecture needs a concrete enough v1 type language to make Phase 1 implementable. v1 should stay deliberately small.

### Surface grammar

v1 should support the following expression forms:

```text
TypeExpr :=
    Ref
  | TypeExpr "&" TypeExpr
  | Constructor
  | Record
  | "collection" "<" TypeExpr ">"
  | "table" "<" Record ">"
  | "(" TypeExpr ")"

Ref :=
    Ident
  | Ident "@" Version

Constructor :=
    Ident "(" NamedArgs? ")"

NamedArgs :=
    name "=" Literal ("," name "=" Literal)*

Record :=
    "{" FieldList "}"

FieldList :=
    Field ("," Field)* ("," "...")?

Field :=
    name ":" TypeExpr
  | name "?:" TypeExpr
```

Examples:

```text
float64 & unit("MPa") & max(0)

csv(delimiter=",", header=true) & table<{
  species: string,
  wp: float64 & unit("MPa") & max(0),
  date: date
}>

{
  site_id: string,
  instrument: string,
  timezone?: string,
  ...
}
```

### Omitted constructor arguments

Omitted constructor arguments are unconstrained, not defaulted.

This means:

- `csv()` means any CSV artifact
- `csv(delimiter=",")` constrains only the delimiter
- `csv(delimiter=",", header=true)` refines both `csv(delimiter=",")` and `csv()`

Defaults may exist at the verifier layer for execution convenience, but they are not part of the type semantics.

### Records

Records are closed by default.

Examples:

- `{ a: string }` means exactly one required field `a`
- `{ a: string, ... }` means at least one required field `a`, with extra fields allowed
- `{ a?: string }` means field `a` may be absent, but if present must satisfy `string`

### Collections and tables

For v1:

- `collection<T>` means an ordered homogeneous collection of `T`
- `table<R>` is a distinct constructor for rectangular tabular data whose rows satisfy record type `R`

`table<R>` is not just surface sugar for `collection<record{...}>`, because it carries stronger structural guarantees about shared row shape and tabular representation. However:

- `table<R>` refines `collection<R>`

### Canonicalization rules

Canonicalization should obey these rules:

- conjunction is commutative
- conjunction is idempotent
- aliases disappear in canonical form
- constructor args are sorted by name
- record fields are sorted lexicographically
- closed and open records are distinct
- required and optional fields are distinct

Strict `equivalent(T1, T2)` means the two types reduce to the same canonical form.

### Refinement rules

`refines` should be structural and conservative.

Rules to lock for v1:

- `A & B` refines `A`
- `A & B` refines `B`
- `collection<A>` refines `collection<B>` if `A` refines `B`
- `table<A>` refines `table<B>` if row record `A` refines row record `B`
- `table<R>` refines `collection<R>`

Record refinement uses:

- depth subtyping on shared fields
- width subtyping only when the target record is open
- a required source field may refine an optional target field
- an optional source field does not refine a required target field

This implies:

- `{ a: int64 }` refines `{ a?: int64, ... }`
- `{ a: int64, b: string }` does not refine `{ a: int64 }`
- `{ a: int64, b: string }` does refine `{ a: int64, ... }`

### Conflicting constraints and bottom

v1 should include a bottom type, surfaced as `never` and represented internally as canonical bottom.

Conjunctions that are obviously unsatisfiable at canonicalization time should reduce to `never`.

Initial builtin conflict detection should include at least:

- incompatible scalar bases
- conflicting `unit(...)` constraints
- `min(x) & max(y)` where `x > y`
- empty intersections of `enum(...)`
- record fields whose field type reduces to `never`

`never` refines every type, and no artifact conforms to `never`.

---

## Composite Types And Input Sufficiency

The O-M pattern is a useful stress test.

If:

- `O` = observations
- `M` = metadata
- `OM = record{obs: O, meta: M}`

then a bare artifact conforming to `O` is insufficient for `OM`. A bare artifact conforming to `M` is also insufficient for `OM`. Together, they may be sufficient as inputs to a transform that constructs `OM`.

This is not a special relation kind. It is a consequence of:

- product types in the type language
- multi-port transform signatures
- explicit invocation bindings

This means OzzyDB can express both:

- richer composite types
- transforms that assemble or project those composites

without inventing extra ontology.

---

## Verification Semantics

Conformance must be executable.

The design rule for v4 is:

- types are declarative
- primitive constructors have executable verifier implementations
- verification returns evidence, not just a boolean

Conceptually:

```text
verify(artifact, type, context) -> VerificationReport
```

A `VerificationReport` should include at least:

- verdict
- structured evidence / witness
- diagnostics
- verifier identity

### Types own acceptance criteria

The type decides the acceptance test.

Artifacts may provide hints or claims, but they do not choose their own verification parameters.

Good:

- type says `csv(delimiter=",", header=true)`
- artifact metadata claims it is comma-delimited
- verifier may use that as a hint, but verdict is against the type

Bad:

- artifact asks to be tested as tab-delimited in order to pass

### Verifiers may be builtin or environment-backed

Builtin verifiers are preferred for common primitive constructors such as:

- csv
- json
- parquet metadata
- numbers
- bounds
- units
- common schemas

Environment-backed verifiers are allowed where necessary, provided they are:

- versioned
- reproducible
- sandboxed
- evidence-producing

For example, a JPEG verifier implemented through Pillow in a pinned Python environment is acceptable.

### Verification should produce witnesses

Verification should return structured facts that downstream checks can build on.

Examples:

- CSV verifier: delimiter, header presence, row count, column names
- JPEG verifier: width, height, channels, decoder used
- schema verifier: required fields present, inferred field types, nullability facts

This makes verification compositional instead of forcing every higher-level type to start from raw bytes again.

### v1 witness families

The first implementation should ship a small fixed set of witness types.

At minimum:

- `CsvWitness`
  - delimiter
  - header flag
  - column names
  - row count if known
- `TableWitness`
  - schema / field list
  - nullability facts
  - row count if known
- `RecordWitness`
  - present fields
  - absent optional fields

These are implementation-facing Rust structs. Persistence and API layers may serialize them into JSON evidence, but the verifier layer should work with typed witnesses internally.

### Verifier coverage is intentionally phased

Phase 1 establishes the execution model for verification:

- the type language is explicit and closed
- the verifier model is executable
- witness-based verification is compositional
- published type refs can be resolved through a registry context
- layered conjunctive checks can be satisfied through multiple witness views of one artifact
- malformed semantic input returns structured errors

Even with that base in place, the v1 type language is still broader than the v1 verifier surface.

The remaining gap is mostly about artifact-backed witness derivation and richer semantic metadata, not about whether verification is a first-class concept.

Examples of builtin areas that may still lag behind full execution coverage after Phase 1:

- `bytes`
- `json`
- `date`
- `datetime`
- `unit(...)`

That gap should close in later phases, not through fallback behavior.

#### Phase 2

Registry persistence and snapshots make the registry-backed verification context durable.

Phase 1 may already resolve published refs in-memory, but Phase 2 is where that behavior becomes part of the real persisted platform contract through pinned registry revisions and immutable snapshots.

#### Phase 3

`ozzy.toml` publication makes verifier requirements part of the published object model.

This is where published type/environment/transform objects become the canonical inputs to verification planning, rather than AST fragments that only exist transiently during local parsing.

#### Phase 4

The `Artifact` model is where raw verification inputs become first-class.

This phase should provide the bridge from:

- raw uploaded blobs
- typed bundles/manifests
- collection-like artifacts

into the witness system.

Builtin coverage that depends on actual artifact bytes or manifest structure, such as `bytes`, `json`, and richer collection/bundle conformance, should be extended here.

#### Phase 5

Execution integration is where runtime metadata becomes part of the platform contract.

This includes:

- carrying measurement/runtime metadata far enough to support constraints like `unit(...)`
- making typed input/output verification part of the fetch/execute path
- deriving and persisting the witness views required by execution-time conformance policy

The rule across all phases is:

- never silently broaden acceptance because a verifier is incomplete
- add real witness derivation or artifact support when a constructor becomes executable
- keep unsupported areas explicit until the required infrastructure exists

---

## Error Handling Principle

v4 should not rely on silent fallback paths in core semantic code.

This applies especially to:

- type parsing
- canonicalization
- refinement checking
- verification
- registry publication
- registry snapshot loading
- execution planning

The rule is:

- errors are explicit data
- semantic failure should fail explicitly
- degraded or best-effort behavior is allowed only when intentionally designed and documented

In particular, v4 should not:

- replace failed parsing or unknown constructors with broader fallback types
- silently downgrade failed verification into apparent success
- continue from partial publication state
- hide semantic failure behind default values that change meaning

This is not just a coding preference. It is required for provenance integrity.

---

## Compute Model

A `TransformVersion` is a typed morphism implemented by real code.

Its core contract is:

- typed named input ports
- typed named output ports
- parameter schema
- bound environment
- runtime / entrypoint information

The execution model is:

1. resolve required input artifacts
2. check declared or verified conformance against required input types
3. run the transform in its bound environment
4. materialize output artifacts
5. record conformance claims for outputs
6. verify outputs where required

The compute graph is therefore built from `Invocation`s over typed transforms.

This is the operational layer of OzzyDB provenance.

---

## Environment Identity Versus Provider Realization

v4 draws a hard line between three different concepts.

### Type identity

What an artifact is, what guarantees it satisfies, and what is required to interpret it.

### Environment identity

The reproducible substrate a transform or verifier runs in.

This includes things like:

- interpreter version
- system libraries
- dependency lockfiles
- base image digest

### Provider realization

How an environment is materialized on a particular backend.

Examples:

- a GHCR image reference
- a Fly mirror image reference
- a local Docker cache entry

Provider realization should not change the identity of the environment or of the type. It is operational infrastructure, not part of the scientific ontology.

This separation is necessary for clean provenance, caching, and backend portability.

---

## Publication Model Addendum

The publication model for v4 should be explicit.

### Publication bundle

The unit of publication is a `PublicationBundle`.

This is an internal compiled Rust object, not a public wire format.

It contains at least:

- type definitions
- environment definitions
- transform definitions
- endpoint definitions
- source commit identity
- raw `ozzy.toml`

### Publication transaction

Push compiles a `PublicationBundle`, then publishes it in one atomic database transaction.

The transaction should:

1. intern canonical type nodes
2. insert or resolve `TypeVersion`s
3. insert or resolve `EnvironmentVersion`s
4. insert or resolve `TransformVersion`s and typed ports
5. insert endpoint and project-revision objects
6. insert the new `registry_revision`
7. insert the new `project_revision`
8. commit

If anything fails before commit, nothing is published.

Provider-specific environment realization work such as image building or registry mirroring happens only after this transaction commits.

### Version conflict rules

Publication should obey these rules:

- same type name + same version + same canonical form: idempotent
- same type name + same version + different canonical form: hard error
- same type name + new version + same canonical form: allowed
- same type name + new version + different canonical form: allowed

The public identity is `TypeVersion`. Canonical type nodes are internal deduplication and reasoning objects.

Multiple `TypeVersion`s may therefore point at the same canonical type node.

---

## What v4 Does Not Yet Lock Down

This document fixes the object model and main semantics. It does not yet freeze every detailed mechanism.

Open or intentionally deferred items include:

- the exact normalized syntax for the type language
- exact canonicalization rules for `equivalent`
- the full witness/evidence schema for verification reports
- planner behavior for automatic transform insertion
- governance rules for the stdlib type registry
- detailed execution policy fields on transforms and invocations

These can be specified after the object model is accepted.

---

## Immediate Consequences For The Rest Of v4

If this architecture stands, the next planning documents should derive from it.

In particular:

- `implementation_details.md` should explain how these six primitives map to storage, APIs, and internal Rust structures
- `soul.md` should be updated to reflect OzzyDB as a provenance system over typed computation, not just a fetch/cache/orchestration system
- the existing v3 type notes should either be incorporated into a v4 type-system section or archived as design background

That work should happen only after the v4 architecture is reviewed and accepted.
