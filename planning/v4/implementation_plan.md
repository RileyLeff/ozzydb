# v4 Implementation Plan

See `architecture.md` for the v4 object model and `soul.md` for the current project principles.

---

## Working Rules

v4 should be executed with a few explicit constraints:

1. **No backwards-compatibility shims.** This is a greenfield rewrite of the platform contract, not a migration strategy for existing users.
2. **API first.** The server contract and internal model come first. CLI and Python client follow. Frontend is deferred.
3. **Delete dead abstractions early.** Fake generality and fallback paths should not survive into the new implementation by inertia.
4. **Versioned, immutable core objects.** New versions are published; old versions are not mutated in place.
5. **Pinned registry snapshots.** Push, fetch, verification, and execution all run against explicit registry revisions.

---

## Phase 0: Deletion / Migration Matrix

This comes first on purpose. The v4 implementation should not accrete on top of the v3 control plane.

| Current v3 piece | v4 disposition | Timing | Notes |
|---|---|---|---|
| `commit_state` JSON blobs (`environments`, `transforms`, `endpoints`, `project_meta`) | **Replace** | Early | New runtime model should execute from first-class versioned objects and registry revisions, not deserialized `ozzy.toml` JSON. |
| `TransformDef.inputs: HashMap<String, String>` / `output: String` / `output_schema` | **Replace** | Early | Superseded by typed ports and type references. |
| `schema.rs` as the main compatibility/type layer | **Keep, but narrow** | Early | Reuse as tabular/schema witness code inside the new type system. Do not let it remain the public typing model. |
| `DataAtom` / `Collection` / `CollectionVersion` / `CollectionMember` ontology | **Replace** | Early-Mid | Superseded by `Artifact` plus typed bundles/collections. |
| `NodeDef.machine` and user-visible provider selection | **Delete** | Early | Provider realization becomes internal infrastructure. |
| `/api/v1/compute/providers` | **Delete** | Early | Not part of the v4 public contract. |
| fake multi-provider git surface (`git_provider`) | **Collapse** | Early | Support GitHub explicitly until a second provider exists. |
| `platform_hash` / `platform` / `verification_tier` in current materialized cache model | **Redesign** | Mid | Replace with transform/environment/type-driven provenance. |
| endpoint inspection APIs backed by raw `commit_state` JSON | **Rewrite** | Mid | Re-derive from typed project revision objects. |
| CLI/API surface that assumes current collection and endpoint models | **Rewrite after server stabilizes** | Late | No need to preserve old command compatibility. |

### Immediate deletions once replacement exists

As soon as the replacement path lands, remove:

- `commit_state` runtime dependence in fetch and endpoint inspection
- `output_schema`-driven compatibility logic
- `NodeDef.machine`
- public compute-provider inspection endpoint
- fake git-provider branching where only GitHub is real

Do not keep dual paths alive for safety.

---

## Phase 1: Core Type System Foundation

**Goal:** Land the `TypeVersion` side of the v4 architecture as real Rust code, with enough power to replace schema-only typing.

### Step 1.1: Create `crates/ozzy-types`

Introduce a dedicated internal crate for the v4 type system.

Initial modules:

- `syntax.rs` — surface AST for type expressions and named definitions
- `canonical.rs` — canonical normalized form
- `registry.rs` — type registry, canonical interning, version lookup
- `relations.rs` — `refines`, `equivalent`
- `verify/` — verifier planning, reports, witnesses
- `ports.rs` — typed input/output port specs
- `conformance.rs` — `ConformanceRecord` domain model

**Deliverable:** a compilable crate with tests and no server wiring yet.

### Step 1.2: Implement the core type language

Support the minimum v4 constructors:

- conjunction / refinement
- named aliases
- record/product types
- collection types
- primitive constructors for encoding/structure/schema/semantics

Initial builtin constructors should be intentionally small:

- `bytes`, `utf8`, `csv`, `json`, `parquet`
- scalar bases: `string`, `bool`, `int64`, `float64`, `date`, `datetime`
- `tabular(...)`
- `record{...}`
- `collection<...>`
- semantic refinements like `unit`, `min`, `max`, `enum`, `nullable`

Lock the v1 grammar and syntax from `architecture.md` before coding this step:

- `&` conjunction
- named constructor args only
- `?:` for optional record fields
- `...` for open records
- closed records by default

### Step 1.3: Canonicalization and relation checks

Implement:

- canonical type hashing / interning
- strict `equivalent` via canonical identity
- conservative `refines`
- canonical bottom / `never`

Lock the v1 semantic rules from `architecture.md` as implementation constraints:

- omitted constructor args mean unconstrained, not defaulted
- conjunction is commutative and idempotent
- record fields sort lexicographically
- width subtyping only to open target records
- `collection<T>` is covariant
- `table<R>` is covariant in row type and refines `collection<R>`
- obvious builtin conflicts canonicalize to `never`

Do not implement planner logic here. Do not overbuild a theorem prover.

### Step 1.4: Verification planning and witnesses

Implement verification as executable plans over primitive constructors.

Introduce:

- `VerificationReport`
- typed witness structs
- builtin verifier registry

The first witness family should be tabular/schema-based, reusing logic from `ozzy-core/src/schema.rs`.

Initial witness structs to land in this phase:

- `CsvWitness`
- `TableWitness`
- `RecordWitness`

### Step 1.5: Conformance model

Implement the first Rust model for:

- `ConformanceRecord`
- semantic states: `declared`, `verified`, `rejected`
- verification attempt logs/evidence separate from semantic state

**Tests for Phase 1:**

- canonicalization equality tests
- refinement tests for simple builtin types
- record/product and collection conformance tests
- verification report generation tests

---

## Phase 2: First-Class Registry Objects And Persistence

**Goal:** Replace the current JSON snapshot runtime model with real persisted v4 primitives.

### Step 2.1: Add new registry tables and models

Add new persistence for at least:

- registry revisions
- canonical types
- type versions
- environment versions
- transform versions
- transform ports
- invocations
- conformance records
- verification attempts / evidence
- project revisions that bind a git commit to a registry revision

This phase should produce new Rust models and query code alongside the existing DB layer.

### Step 2.2: Introduce registry snapshots

Implement `RegistrySnapshot` loading and caching in the server.

Requirements:

- every fetch/push/typecheck resolves against a pinned revision
- reads should not observe partially published graph updates
- canonical types and relation indexes should be loadable as immutable snapshots
- publication should be atomic at the registry-revision boundary
- no partially published bundle should be externally visible

### Step 2.3: Define project revision objects

Introduce the v4 equivalent of "what a pushed commit means".

A project revision should point to:

- source commit identity
- registry revision
- the published transforms/types/environments/endpoints for that commit

This object replaces the current role of `commit_state` as runtime control plane.

---

## Phase 3: `ozzy.toml` Ingestion Rewrite

**Goal:** Keep `ozzy.toml` as the authored declaration layer, but compile it into first-class v4 objects instead of storing it as execution JSON.

### Step 3.1: Replace or heavily revise `toml_spec.rs`

The new parser should emit typed definitions, not the v3 schema-only structures.

Major changes:

- typed transform ports instead of `inputs: HashMap<String, String>`
- typed outputs instead of `output = "parquet"`
- no `output_schema` as the public typing mechanism
- remove `machine` from node definitions

Keep parsing/validation logic that is still structurally useful, but do not preserve the old data model for compatibility.

### Step 3.2: Push compiles and publishes registry objects

Rewrite push so it:

1. reads `ozzy.toml`
2. validates it against the v4 parser
3. compiles an internal `PublicationBundle`
4. publishes or resolves types, environments, and transforms
5. creates a new registry revision / project revision in one atomic DB transaction
6. stores the raw file only as audit/debug source, not as runtime truth

### Step 3.3: Separate environment definition from provider realization

Push should publish `EnvironmentVersion`s.

Those published environment versions should be **content-bound**:

- authored lockfile and Dockerfile paths are resolved at push time
- the resulting published environment definition stores the resolved content
- provider-specific build and mirror state is derived later from that published definition

Project revision payloads should store versioned environment bindings, not raw authored environment path specs.

Provider-specific image pushes and mirrors should be treated as realization/indexing work, not as part of the logical publication model.

**Tests for Phase 3:**

- parse valid v4 `ozzy.toml`
- reject invalid typed port graphs
- repeated push of equivalent type definitions reuses canonical type nodes but creates correct version objects where intended

---

## Phase 4: Artifact Model Rewrite

**Goal:** Replace the v3 `data atom` / `collection` split with the v4 `Artifact` model.

### Step 4.1: Introduce `Artifact`

Unify the persisted model around a single artifact primitive.

Artifacts may represent:

- raw uploaded blobs
- transform outputs
- typed bundles/manifests
- collection-like aggregates

### Step 4.2: Replace collection ontology with typed bundle/collection artifacts

Current collections are a separate subsystem. In v4 they should become a type/structure expressed over artifacts.

This likely means:

- removing dedicated collection mutation logic from the core ontology
- representing bundle membership in artifact manifests or equivalent typed structures
- re-deriving whatever user-facing collection operations are still worth keeping from the new model

### Step 4.3: Attach conformance to artifacts

Every artifact should be able to carry:

- declared type claims
- verified type claims
- rejected verification results
- evidence

The current metadata/yank model should be re-evaluated once artifact identity is settled.

**Tests for Phase 4:**

- raw upload artifact creation
- transform output artifact creation
- typed bundle/collection artifact creation
- conformance declaration and verification persistence

---

## Phase 5: Execution Integration

**Goal:** Make the compute plane actually run through typed transforms and versioned environments.

### Step 5.1: Bind `TransformVersion` to execution

A transform execution should now resolve through:

- `TransformVersion`
- typed input ports
- typed output ports
- `EnvironmentVersion`
- pinned registry revision

### Step 5.2: Rewrite fetch around typed project revisions

Replace the current fetch flow that deserializes endpoint/transforms/environments out of `commit_state`.

New fetch flow:

1. resolve project revision
2. load pinned registry snapshot
3. resolve endpoint DAG in terms of transform versions
4. typecheck invocation inputs
5. verify required conformance where policy demands
6. run async jobs
7. declare and verify outputs

### Step 5.3: Redesign materialized cache identity

Replace the current cache key model that depends on `platform_hash` and `verification_tier`.

The v4 cache identity should be derived from the new primitive model:

- typed input artifact identities
- transform version identity
- environment version identity
- parameter identity
- relevant secret/version provenance
- any execution provenance that truly affects semantic reproducibility

### Step 5.4: Keep compute providers internal

Retain the backend abstraction, but remove provider selection from user-authored graphs and public API contracts.

`docker`, `fly`, and similar providers should remain infrastructure concerns.

**Tests for Phase 5:**

- typed fetch success path
- type mismatch at invocation boundary
- cache hit behavior under new identity model
- output conformance verification after execution

---

## Phase 6: Public API Rewrite

**Goal:** Make the API match the v4 ontology before updating CLI/Python consumers.

### Step 6.1: Rewrite endpoint inspection APIs

Inspection should no longer read raw JSON out of `commit_state`.

Instead, endpoint/project inspection should be derived from the new project revision and registry objects.

### Step 6.2: Introduce artifact and conformance endpoints

The API should expose the v4 primitives directly where appropriate.

Likely areas:

- artifact lookup/inspection
- type/conformance inspection
- transform/environment inspection
- project revision inspection

### Step 6.3: Remove public compute-provider introspection

Delete compute-provider listing endpoints from the public contract.

### Step 6.4: Decide the fate of yanks

Re-evaluate whether v3-style endpoint/data yanks survive into v4, and if so at which primitive layer.

Do not automatically preserve them just because they exist.

Decision:

- v4 does **not** preserve yanks as a first-class primitive.
- bad or unusable artifacts are represented through explicit conformance state
  (`declared`, `verified`, `rejected`), not soft-delete-style public yanks.
- the public v4 contract should expose artifact creation, manifest creation, and
  conformance declaration/verification instead of the old `data`/`collections`
  yank surface.
- legacy endpoint yank checks should be removed from the live fetch path.

---

## Phase 7: CLI And Python Client Rewrite

**Goal:** After the server contract stabilizes, bring the CLI and Python client up to date.

### Step 7.1: Rewrite CLI around the v4 API

Update or replace commands that currently assume:

- collection as a top-level ontology
- endpoint inspection backed by old JSON
- current push request shape
- old fetch/materialization semantics

Prefer removing commands over preserving awkward compatibility.

### Step 7.2: Rewrite Python client for v4 fetch and inspection

The Python client should target the new API directly.

Do not preserve old response-shape assumptions if the server model has improved.

### Step 7.3: Frontend remains deferred

Do not spend v4 implementation budget on the frontend until the server and clients settle.

---

## Phase 8: Deletion Sweep And Stabilization

**Goal:** Remove v3 codepaths as soon as the v4 replacements exist, then review hard.

### Step 8.1: Delete superseded v3 code

This should include, once replacements exist:

- `commit_state` runtime execution path
- old schema-only typing surface
- public provider-selection surface
- fake multi-provider git plumbing
- old collection ontology if replaced by artifacts

### Step 8.2: Review and simplify aggressively

Run a deletion-oriented review, not a compatibility-oriented one.

Questions for the review:

- what code only exists to bridge old and new worlds?
- what abstractions are now fake?
- what tables or APIs are no longer first-class in v4?

### Step 8.3: Update docs from the new ontology

After the code is stable enough:

- update `soul.md`
- update README and getting-started docs
- rewrite CLI/API examples to match v4

---

## Suggested Execution Order

1. Phase 0 — lock the deletion matrix
2. Phase 1 — type system crate
3. Phase 2 — registry persistence and snapshots
4. Phase 3 — `ozzy.toml` ingestion rewrite
5. Phase 4 — artifact model rewrite
6. Phase 5 — execution integration
7. Phase 6 — API rewrite
8. Phase 7 — CLI/Python rewrite
9. Phase 8 — deletion sweep and stabilization

This ordering is deliberate. It prevents the v3 runtime model from remaining the hidden truth underneath a nominal v4 surface.

---

## Finish Criteria For v4 Planning

v4 planning is complete when:

- the architecture and implementation plan agree on the primitive set
- the first implementation phase is small enough to begin immediately
- the deletion plan is explicit enough that old code will not linger by default
- the workflow state points at a concrete first implementation step
