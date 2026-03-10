# GPT Type System Synthesis

Date: 2026-03-05

## Take

The lattice idea is good. It is probably the right semantic core for OzzyDB.

The main design risk is overloading one structure with too many different jobs.
In particular, these should be related but distinct:

1. Refinement / entailment
2. Representation / encoding
3. Conversion / adaptation
4. Runtime behavior / capabilities

If those collapse into one undifferentiated type relation, the model becomes
elegant on paper but much harder to query, debug, negotiate, and extend.

## What The Lattice Is Good For

Use the lattice for:

- narrowing valid sets via conjunction
- checking producer/consumer compatibility
- expressing semantic refinement
- expressing structural refinement
- reasoning about version relationships
- detecting conflicts (`bottom`)

Examples:

- `float & unit(MPa) & <= 0`
- `table & columns{leaf_wp: float & unit(MPa) & <= 0}`
- `delimited & utf8 & delimiter(",") & header(true)`

This is the right place for "make invalid states unrepresentable" in data
pipelines.

## What Should Not Be Forced Into The Same Relation

Encodings are not the same kind of thing as refinements.

Examples:

- `csv`
- `parquet`
- `Arrow IPC`
- `pandas.DataFrame`
- `R data.frame`

These are representations of data, not merely narrower semantic subsets of one
another. Some pairs are losslessly interconvertible. Some are not. Some preserve
runtime behavior, some only preserve logical content.

That means subtype / entailment and conversion planning are different problems.

## Recommended Split

Keep one unified user-facing language if desired, but model the internals as:

### 1. Logical Type

Language-neutral meaning and structure.

Examples:

- scalar with unit and bounds
- table with columns and column-level semantic types
- tensor with dtype, shape constraints, units
- graph with node/edge schemas
- collection of typed members with aggregate constraints

This is where the lattice lives.

### 2. Encoding

How the logical type is represented right now.

Examples:

- csv
- parquet
- json
- Arrow IPC
- pickle
- pandas.DataFrame
- R data.frame

Encodings should carry metadata such as:

- serialized vs in-memory
- language/runtime
- dependency requirements
- capability for random access / streaming

### 3. Adapters

Explicit edges between encodings.

Each adapter should record:

- source encoding
- target encoding
- logical type family supported
- lossless vs lossy
- verification strategy
- preferred / canonical status
- estimated cost

This turns conversion into a graph search problem instead of pretending it is
subtyping.

## Equivalence Needs To Be Split

"Equivalent" is overloaded. Use narrower terms:

### Semantic equivalence

Two artifacts represent the same logical type.

### Representation isomorphism

There is a verified lossless round-trip between two encodings.

### Adapter availability

There exists some conversion path between two encodings.

### Capability equivalence

Two runtime objects expose the same usable interface.

Do not use one undifferentiated `equivalent` relation for all of these.

## Arrow And Other Hub Types

The "liquid currency" idea is good.

Treat Arrow as a hub encoding for one family of logical types: tabular/columnar
data. It should be a standard-library priority because it reduces the number of
required adapters dramatically.

Same general pattern may apply elsewhere:

- Arrow / Parquet for tables
- Zarr / safetensors for arrays
- a canonical graph interchange for graphs

The hub is not the ontology. It is a convenient center of the adapter network.

## Bindings / Runtime Classes

The binding idea is useful, but it should be framed as runtime representation,
not as the same kind of type as the logical contract.

Example:

- logical type: `table<{x: float, y: float}>`
- runtime representation: `pandas.DataFrame`

The logical type says what the data is.
The runtime representation says what object form the client receives.

That separation keeps "what this data means" distinct from "what methods this
object has."

## Collections

Collections should be first-class.

A constraint that mentions multiple values belongs on the collection that sees
them.

Examples:

- `x < y` belongs on the containing record/collection
- "all species in census A appear in census B" belongs on a collection that
  contains both censuses

Element constraints and aggregate constraints should be treated differently:

- element constraints can often be checked incrementally
- aggregate constraints generally require the whole collection version

## Versioning And Conformance

Type conformance should be attached to a concrete pairing:

- artifact version
- type version
- verification result

For collections, that means conformance belongs to a collection version, not the
collection name in the abstract.

If collection membership changes, re-validation is required.

This implies:

- type versions should be immutable once published
- collection versions should be immutable snapshots
- conformance records should never silently migrate across versions

## Candidate / Validated Pattern

The Candidate -> Validated pattern is one of the strongest ideas in the current
notes.

It makes cleaning and QC visible in the type system:

- raw field upload gets a loose type
- validation / cleaning transform narrows it
- downstream consumers can require the validated type

This is a very good fit for OzzyDB because it turns hidden methodology into an
explicit contract.

## Suggested Minimal Core For A Shared Rust Crate

Keep the shared crate narrow and reusable:

1. Type expression AST
2. Constraint conjunction / meet
3. Entailment / compatibility checker
4. Conflict detection
5. Unit algebra
6. Validator interface for boundary checks
7. Encoding and adapter metadata types

Do not try to force OzzyDB and the e-graph project to share one whole solver.

Shared language: yes.
Shared runtime / propagation engine: probably only partially.

## Practical Recommendation

If building this incrementally, start with:

1. Logical types for scalars, tables, and collections
2. A small standard library of encodings: csv, parquet, Arrow, json
3. Explicit adapter graph metadata
4. Versioned conformance records
5. Candidate -> Validated workflow support

Do not start by solving arbitrary type equivalence or full client-side active
type propagation. Those are valuable, but they are second-wave features.

## Bottom Line

Use the lattice for refinement and compatibility.

Use an adapter graph for encoding conversion.

Keep logical type and runtime representation distinct.

Define equivalence in tiers rather than as one overloaded relation.

That keeps the system mathematically clean enough to reason about and practical
enough to operate.
