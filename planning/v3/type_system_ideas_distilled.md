# OzzyDB Type System: Distilled Ideas

All ideas extracted from `thoughts_on_types.md`, `type_ideas_canonical.md`, and `v2_architecture.md`. Some may conflict. This is a working document to sort through.

---

## Foundation

- **One base type: `bytes`.** Everything else is refinement via constraints.
- **One operation: conjunction (`&`).** Types get more specific by adding constraints. Less specific types are supersets of more specific ones.
- **Lattice structure.** Types form a lattice where `&` is meet (intersection). Incompatible constraints = bottom (error). This is CUE's model.
- **Composition, not inheritance.** `csv` is not a subclass of `delimited_file` — it's `delimited_file & { delimiter: "," }`. No class hierarchy, no diamond problem. Just sets getting smaller.

## The Encoding/Semantics Question

- **Idea A: Product lattice with multiple dimensions.** A type has semi-independent dimensions: encoding (csv, parquet, pickle), structure (tabular, tensor, graph), schema (column names/types), and semantics (units, value constraints). The full type is a point in the product. Meet/join work componentwise, plus cross-dimensional consistency checks (e.g., CSV can't encode a graph).
- **Idea B: The distinction dissolves.** Encoding constraints (utf8, delimited, comma-separated) and semantic constraints (non-positive, unit is MPa) are all just constraints at different levels of the refinement tower. One axis, not multiple. "CSV" is just a name for a particular point on the single lattice.
- **Tension:** Idea A gives you clean separation of concerns (you can ask "what's the encoding?" vs "what's the schema?" independently). Idea B is more elegant and unified but may make it harder to do things like "find all conversions that change encoding but preserve semantics."

## Encodings and Conversions

- **Bindings ARE encodings.** `pandas.DataFrame`, `R data.frame`, `polars.DataFrame` are in-memory encodings, just like CSV and Parquet are serialized encodings. An adapter between any two encodings of the same semantic type is the same kind of operation.
- **Conversions form a category.** Objects = types, morphisms = transforms between encodings. Some are lossless (DataFrame <-> Arrow <-> Parquet), some are lossy (float64 -> float32). Composition gives multi-hop paths.
- **Hub types.** Arrow IPC is the "reserve currency" of tabular data. If every tabular format has a morphism to/from Arrow, you get n converters instead of n^2. Same idea extends to other domains (safetensors for tensors, adjacency lists for graphs, etc.).
- **Conversion path search at fetch time.** When a Python client fetches data stored as an R pickle, the system finds a path through the morphism category (pickle -> Arrow -> DataFrame). Could have a cost model: prefer lossless, prefer fewer hops, prefer hub types as intermediates.

## Transforms and Types

- **Transforms are type refinement functions.** A cleaning transform's signature `fn clean(input: Candidate) -> Validated` IS the documentation. The type system can verify at push time that uncleaned data never reaches a consumer that expects cleaned data.
- **Every port declares a type.** Strict, not optional. If you don't know the type, say `bytes` explicitly. No implicit untyped edges. Strictness constrains the space for LLM-generated adapters, making generated code verifiable.
- **The Candidate/Solid pattern.** Raw data gets a loose type (few constraints), a transform validates and refines it to a strict type. Makes data quality workflow visible in the type system.
  - **Sub-question:** Should `Candidate<T>` be a first-class construct (auto-generates a loose version of any type T), or just a naming convention? Leaning toward convention for now, can add sugar later.

## Capability Types

- **Opaque objects declare capabilities, not structure.** A NumPyro model declares `capability predict { inputs: {...}, outputs: {...} }`. A Stan model with the same capability is substitutable. The consumer depends on the capability, not the framework.

## Collections and Cross-Value Constraints

- **A constraint that mentions multiple values lives on the type that sees all of them.** If X < Y, that constraint belongs on the collection Z containing both X and Y. Same scoping rule as variable binding in programming languages.
- **Collections are nestable.** A collection of collections. Constraints can live at any level.
- **Element-level vs. aggregate-level constraints.** Per-element constraints (column types, value bounds) are checkable incrementally. Cross-element constraints ("all species in census A appear in census B") require the whole collection to be materialized.

## Three Scopes of Constraint

- **Type scope (definitional).** Holds for ALL instances, everywhere, always. `water_potential <= 0` by physical law. Checked at definition time.
- **System scope (structural).** Emerges from DAG/graph structure. "Given that this node feeds into `exp()`, the output is provably non-negative." Checkable at push time via DAG analysis, without actual data.
- **Instance scope (realized).** From actual computed/measured values. "We ran the cleaning transform and every output value is between -1.8 and -0.02." Checked at compute time.
- Each scope strictly narrows the previous.

## Units

- **Structured, not text.** Units form a free abelian group over base dimensions. A unit = tuple of rational exponents + scale factor + offset. `MPa = (mass:1, length:-1, time:-2, scale:1e6)`.
- **Machine-parseable, zero ambiguity.** stdlib registry of known units (SI + domain). The type system understands the algebra.
- **Verified at transform boundaries.** Inside a transform, user code does whatever. But the edge contract says "input: float64 in MPa, output: float64 in kPa" and the system checks dimensional consistency of the DAG without looking inside transforms.

## First-Class Versioned Types

- **Types are registered, named, referenceable objects.** Like data and transforms. Core ships common ones (csv, parquet, float, int, units). Users define domain-specific ones (water_potential, stomatal_conductance).
- **Versioned:** `ozzydb.std.csv@v1.2`. Immutable once published (like a git commit).
- **Type conformance is a triple:** `(data@version, type@version, verification_status)`. Changing the type def doesn't retroactively change conformance — existing data needs re-verification against the new version.
- **Semver-like versioning.** The lattice lets you compute whether a version bump is breaking: if new_type is a subtype of old_type, it's non-breaking (tighter constraint). If not, it's breaking.

## Runtime Type Definition

- **Types are defined at runtime, not baked into a compiler.** Users will define domain-specific types on the fly. The type registry is a database, not a compiler.
- **A type definition is:** a name (namespaced), a parent in the lattice, a set of refinement predicates (executable checks), morphisms to/from other types (with lossiness metadata), and human/LLM-readable metadata.
- **Stdlib vs. user types:** Permissive within projects (you can define whatever you want), curated in the stdlib (enforces lattice coherence). PR/proposal mechanism for stdlib additions.
- **Structural equivalence detection.** If someone defines a type structurally identical to a stdlib type, the system could suggest linking them.

## Parametric Branching and Cache Policy

- Different parameter values produce different outputs with the same type guarantees. Content-addressing distinguishes them.
- Cache policies per-endpoint: `all`, `default_params_only`, `verify_only` (compute and check hash but don't persist), `none`, `ttl:7d`.

## Client-Side Type Propagation

- **Types should flow through the fetch boundary into client objects.** An R data.frame column `leaf_wp` typed as `water_potential<MPa>` should auto-attach `units` annotations, warn on constraint violations, display as "leaf_wp (MPa, <= 0)".
- **Client libraries generate constrained types FROM OzzyDB type definitions.** Types propagate from registry -> server -> fetch -> client runtime objects.

## LLM Adapter Generation

- **LLMs generate encoding adapters, types make them verifiable.** Strict input/output types constrain the space of valid adapter code. Generated code is verifiable against the type contract.
- **"Build tools good for LLMs to use" > "insert LLMs into the architecture."** A well-typed schema language is inherently LLM-friendly.

## Verification / Proof Strategy

- **Lightweight, pragmatic proofs.** "If you claim this is CSV, we verify by parsing with a CSV parser." Not theorem-proving — checking membership in mechanically-verifiable sets.
- **Checked at boundaries:** encoding (parse-based), structure (schema checks), semantics (predicate checks on values).

## What the v2 Architecture Currently Does

- **Content types as MIME-like strings** (`application/vnd.apache.parquet`, `image/tiff`). No lattice.
- **Schema is optional metadata** on data atoms (Arrow schema for tabular, dimensions/dtype for images).
- **Collections are explicitly untyped** — no type constraints on membership.
- **Type checking at transform boundaries** — source content types must be compatible with transform's declared input types. `collection<parquet>` resolves and validates member content types.
- **Transform output schema** is optional, used for validation and metadata propagation.

## Open Questions

- Where is the constraint language boundary? Arithmetic over field names? No quantifiers?
- How do versioned types interact with content-addressing? Is the type version part of the hash?
- Can type definitions themselves be content-addressed?
- How do types travel cross-language? JSON schema? Per-language validators?
- How do system-scope constraints (DAG analysis at push time) actually work?
- How deep does client-side type propagation go? Metadata only? Active validation? Full newtype wrappers?
- How do we handle wrong LLM-generated adapters? What's the verification/rollback strategy?
- Which hub types to prioritize beyond Arrow IPC?
- Should there be a standalone Rust crate for the type system (reusable for the e-graph project too)?
