# Thoughts on a Scientific Type System

Notes from design session 2026-02-15. Not a spec — raw thinking to revisit.

## Core Idea

A type is a named set of constraints over bytes, composed via conjunction. There is one base type: `bytes`. Everything else is refinement.

```
bytes
  & utf8
  & delimited { delimiter: string, quote: string | null }
  & { delimiter: "," }
  & { has_header: true }
  & { columns: [{name: "leaf_wp", type: float}] }
  & { leaf_wp: <= 0.0 }
  & { leaf_wp: unit(MPa) }
```

Every `&` narrows the valid set. `csv` is just a name for a particular level of this tower. No categorical difference between "encoding" constraints (utf8, delimited, comma-separated) and "semantic" constraints (non-positive, unit is MPa). They all compose the same way.

## The Encoding/Semantics Distinction Dissolves

Initially we thought there were two orthogonal axes: encoding (how data is organized) and semantics (what data means). But both are just constraints at different levels of the tower. UTF-8 is a constraint on bytes. `<= 0` is a constraint on floats. Delimiter is a constraint on text. Same mechanism, different levels.

This means the type system has ONE operation: conjunction (`&`). Types get more specific by adding constraints. Less specific types are supersets of more specific ones. `csv` is a subset of `delimited_file`. `water_potential<MPa>` is a subset of `float`.

## Three Scopes of Constraint

Constraints are valid at different times, each scope strictly narrower than the previous:

### Type scope (definitional)
Constraints that hold by definition, for ALL instances, everywhere, always.
- `water_potential <= 0` — true for every water potential measurement, by physical law.
- `percent_composition: 0 to 100` — true by definition of percentage.
- Checked at definition time. Part of the type itself.

### System scope (structural)
Constraints that emerge from the structure of a particular system (DAG, e-graph, model).
- "Given that this node feeds into `exp()`, the output is provably non-negative" — even though its type doesn't require non-negativity.
- "Given the structure of this hydraulic model, this instance of `water_potential` is between -2.0 and 0.0" — even though the type allows anything <= 0.
- Valid at push time / compile time. Provable from DAG/graph structure without actual data.

### Instance scope (realized)
Constraints from actual measured/computed values.
- "This other node is exactly 7, so now we can prove this instance is between X and Y."
- "We ran the cleaning transform and every output value is between -1.8 and -0.02."
- Valid at runtime / compute time. Requires actual data.

Each scope narrows further. In OzzyDB, type scope is enforced at push time, instance scope at compute time. System scope is partially enforceable at push time (DAG analysis). In the e-graph system, all three scopes would be unified — type constraints, structural constraints, and data constraints all propagate through the same graph.

## Transforms as Type Refinement Functions

A transform doesn't just process data — it REFINES types. The cleaning pattern:

```
Water_Potential_Candidate = float & unit(MPa)
    # no value constraint — raw measurement, could be anything

Water_Potential = float & unit(MPa) & (<= 0.0)
    # validated — provably non-positive

clean_wp: Water_Potential_Candidate -> Water_Potential
    params: { cut_threshold: float, default: 0.5 }
```

The transform's type signature IS the documentation. It says: "I take unvalidated measurements and produce validated ones." The type system can verify at push time that nothing downstream consumes `Candidate` directly — you MUST go through a cleaning step.

This is "make invalid states unrepresentable" applied to data pipelines. Invalid data (uncleaned measurements) literally has a different type than valid data (cleaned measurements). The pipeline structure enforces the data quality workflow.

## Parametric Branching

`clean_wp(cut_threshold=0.5)` and `clean_wp(cut_threshold=0.6)` both produce `Water_Potential` (same type guarantees) but with different values. Content-addressing distinguishes them: different params = different hash = different cache key.

Successive parametric steps create exponential branching:
- `clean(0.3) -> analyze("gp") -> predict([0,1,2])`
- `clean(0.3) -> analyze("gp") -> predict([0,0.5,1])`
- `clean(0.5) -> analyze("gp") -> predict([0,1,2])`
- ...

Cache policy should be user-controllable per-endpoint:
- `all` — cache every parameter combination (most storage, least compute)
- `default_params_only` — only cache the default parameter values
- `verify_only` — compute and check hash, but don't persist output (re-run on demand)
- `none` — never cache, always recompute
- `ttl:7d` — cache with expiration

`verify_only` is interesting for parameter sweeps where you're searching for the right value and don't want to store every intermediate result.

## Capability Types for Opaque Objects

Models, custom objects, etc. declare what they can DO, not what they ARE:

```
numpyro_gp_model = bytes
  & { framework: "numpyro" }
  & { runtime: "python" }
  & capability predict {
      inputs: { depth: float<meters> }
      outputs: { density_mean: float<g/cm3>, density_sd: float<g/cm3> }
    }
```

A Stan model with the same capability declaration is substitutable. The consumer depends on the capability, not the framework. NumPyro ~ Stan if same capability. Different encoding, same semantic interface.

## Composition, Not Inheritance

`csv` is not a subclass of `delimited_file`. It's a refinement: `delimited_file & { delimiter: "," }`. Constraint conjunction, not class hierarchy. No vtables, no diamond problem, no method resolution. Just sets getting smaller.

This is like CUE's type system where types and values live on a lattice and `&` is meet (intersection).

## First-Class, Versioned Types

Types should be registered, named, referenceable objects in OzzyDB:
- Core ships common ones: `csv`, `parquet`, `json`, `png`, `float`, `int`, `string`, `bool`, units
- Users define domain-specific ones: `water_potential`, `stomatal_conductance`, `species_code`
- Types are versioned: `ozzydb.std.csv@v1.2` for backwards compatibility
- Validation against a type is a first-class operation: "is this blob a valid csv?" is a type check
- If validation fails: upload as `blob`, add a transform to fix it, output the validated type

"Everything is a specification" — the type definition IS the parser spec, the validator spec, the documentation, and the LLM-readable schema, all in one artifact.

## Strictness Over Optionality

Leaning toward strict, not optional. Every port declares a type. If you don't know, say `bytes` explicitly. No implicit untyped edges. Rust's power comes from enforcement — the type system should too.

Strict types also make LLM-generated adapters MORE viable: strict input/output types constrain the space of valid adapter code, making generated code verifiable against the type contract.

## Class Identity, Bindings, and Equivalence

### The problem: constraints capture data, not behavior

A pandas DataFrame isn't just "tabular bytes with these columns." It's an object with methods (`.groupby()`, `.merge()`, `.plot()`), an index, numpy-backed dtypes. When you pickle it and unpickle it, you get the methods back. That's emergent meaning from construction — the blob of bytes can be reconstituted into a live object in the right runtime.

Our constraint system captures WHAT the data is (structure, values, units). Class identity captures what you can DO with it (methods, protocols, behaviors). These are orthogonal:

```
Constraint type:  table<{x: float, y: float}>     (language-neutral, structural)
Class identity:   pandas.DataFrame                  (language-specific, behavioral)
```

A pandas DataFrame and an R data.frame both satisfy `table<{x: float, y: float}>`, but they're different classes with different methods.

### Bindings: linking constraints to classes

The type system needs a concept of **binding** — a mapping from a constraint type to a language-specific class:

```
table<{x: float, y: float}> {
    bindings: {
        python: ["pandas.DataFrame", "polars.DataFrame"],
        r: ["data.frame", "tibble"],
        julia: ["DataFrames.DataFrame"],
    }
}
```

When a Python transform outputs a pandas DataFrame:
1. Validate data against the constraint (correct columns, types, semantic constraints)
2. Serialize to portable encoding (parquet)
3. Tag with constraint type AND source binding (`python:pandas.DataFrame`)

When an R client fetches:
1. See constraint: `table<{x: float, y: float}>`
2. Look up R bindings: `data.frame`
3. Construct an R `data.frame` from the parquet bytes

The source binding is metadata — it records provenance. The consumer only needs the constraint type and its own binding table.

### Type-level equivalence via adapters

Class equivalence is a TYPE-LEVEL statement, not a data transformation:

```
A(pandas.DataFrame) ≡ R data.frame
```

`A` is not converting bytes — it's declaring that these two classes represent the same kind of thing, linked through the shared constraint type. The actual byte conversion (parquet as interchange) follows from the equivalence.

LLM-generated adapters operate at this level: "given this Python class definition and this OzzyDB constraint, generate the serialization/deserialization code that bridges them." The adapter is verifiable because both sides must satisfy the same constraint.

Equivalences can be:
- **Built-in** for common types (pandas DF ↔ R data.frame ↔ polars DF)
- **LLM-generated** for exotic types (custom classes, domain-specific objects)
- **Verified by the constraint system** (both sides must satisfy the same constraint)

### Grabbing class definitions from the environment

At push time, OzzyDB could inspect the Python/R environment:
1. What version of pandas? What classes available?
2. Inspect transform's return type (type hints, test execution)
3. Look up or generate the binding: "returns `pandas.DataFrame` → maps to `table<...>` → here's serialization"

For well-known libraries (pandas, polars, numpy, xarray, scikit-learn), bindings are known and stable. For custom classes, an LLM inspects the class definition and generates the binding.

### Hub types / liquid currency

Certain types (Arrow, JSON, Parquet) are so well-connected — so many things have conversion functions to/from them — that they become "hub" types in the equivalence network. Like reserve currencies: once you convert your data to Arrow IPC, you can go anywhere. Nearly every language and library can read Arrow.

This suggests a strategy: the standard library should prioritize bindings for hub types first. If every semantic type has an Arrow binding, then cross-language interop comes "for free" through Arrow as the interchange format. The hub types are the center of the equivalence network — maximize connectivity there.

### Multiple output formats and the "center of the network"

(Riley) Certain formats like Arrow and JSON might be useful "center of the network" types — so many things have conversion functions for them that once you get into the ecosystem, you have liquid currency. Like reserve currencies in economics.

What if clients could generate valid refinement types FOR the semantic types? If an R data.frame comes back with a `water_potential` column and a `percent_composition` column, maybe the Python client pulls those in, turns it into pandas or polars, and the columns reflect those constrained values — not just `float64` but actually carrying the semantic type info.

(Claude) The hub type insight is economics: Arrow IPC is the US dollar of data interchange. Maximize connectivity through the hubs and the long tail of exotic formats becomes reachable via two-hop conversions (exotic → Arrow → target). The standard library should prioritize bindings for hub types first. If every semantic type has an Arrow binding, cross-language interop comes "for free."

### Client-side type propagation

When a client fetches typed data, the semantic types shouldn't stop at the fetch boundary. They should flow INTO the client's runtime objects.

If an R data.frame has a column `leaf_wp` typed as `water_potential<MPa>`, the R client could:
- Attach `units` package annotations to the column
- Return a custom vector class that carries the constraint
- Validate on assignment (catch `leaf_wp <- 0.5` as a constraint violation)

Similarly in Python:
- Polars has column-level metadata (could carry OzzyDB type info)
- Pandas ExtensionArrays could wrap constrained types
- A thin wrapper around the DataFrame that type-checks mutations

The DX goal: `df$leaf_wp` isn't just `numeric` — it's `water_potential<MPa>`, and the client knows it. If you accidentally assign a positive value, the client catches it before it ever reaches OzzyDB.

This means the client library generates constrained column types FROM the OzzyDB type definitions. The semantic types propagate all the way from the type registry, through the server, through the fetch, into the client's native type system.

## What to Build

### Standalone Rust crate (the type system)
1. Type expression AST (the language for writing types)
2. Constraint conjunction operator (`&`)
3. Compatibility checker (`fn compatible(producer: &Type, consumer: &Type) -> Result`)
4. Data validator (`fn validate(data: &[u8], typ: &Type) -> Result<(), Vec<Violation>>`)
5. Serialization to/from JSON/TOML (portability across languages)
6. Proc macros for defining types in Rust code

### OzzyDB integration
- Import the crate as the validation/schema layer
- Enforce types at push time (structural, unit compatibility)
- Enforce constraints at compute time (value validation)
- Expose types via `inspect()` API
- Client libraries use types for smart deserialization

### E-graph integration (later)
- Same type definitions, different solver
- Forward/backward constraint propagation across all three scopes
- Symbolic reasoning about scientific models

## Reading List

- **CUE language** (cuelang.org) — types as constraints, conjunction as composition. Most aligned philosophically.
- **Liquid Haskell** — refinement types checked by SMT solver. "Refinement Types for Haskell" (Vazou et al. 2014).
- **F# Units of Measure** — "Types for Units-of-Measure" (Andrew Kennedy, 2010). Units as type parameters, dimensional analysis at compile time.
- **Types and Programming Languages** (Benjamin Pierce) — the foundational textbook.
- **The Little Typer** (Friedman & Christiansen) — accessible intro to dependent types.
- **Row polymorphism** (PureScript, OCaml) — for typing tables with "at least these columns" vs "exactly these columns."
- **Apache Arrow type system** — practical cross-language types for columnar data. Study what it gets right (wide support) and what's missing (no units, no refinements).
- **JSON Schema** — unglamorous but widely adopted. Practical reference for "what works."
- **Dhall** — programmable configuration with types and guaranteed termination.
- **floco** (github.com/rileyleff/floco) — Riley's own refinement types experiment in Rust.

## Encoding in Rust / Proc Macros / LLM Positioning

(Riley) Not sure about optionality — part of the magic of Rust is you can make way more guarantees with strictness. What's stopping us from encoding this in Rust's type system? What about proc macros? And LLMs should be useful for generating otherwise-laborious adapters, perhaps "lazily" on request. But it's better to build tools that are good for LLM agents to use than to insert LLMs into the architecture itself.

(Claude) Proc macros are the obvious tool:
```rust
#[ozzy_type(unit = "pressure", constraint = "lte(0.0)")]
struct WaterPotential(f64);
```
The macro generates: the newtype, validation logic, serialization, unit conversion traits, and schema metadata. Incompatible port connections become compile errors in the orchestrator.

Cross-language bridge: Rust types are source of truth → derive JSON/TOML schema descriptions → Python/R runners validate against derived schemas.

Strictness enables LLM adapters: strict types constrain the space of valid adapter code. An adapter between encoding A and encoding B for type T is a well-defined generation problem. Generated code is VERIFIABLE against type constraints. Sloppy types give the LLM too much rope; strict types make correctness checkable.

Triad: **Rust types define the contract, the server enforces it, LLMs generate encoding adapters — types make the glue code verifiable.**

On LLMs in architecture: agree that "build tools good for LLMs to use" > "insert LLMs into the architecture." A well-typed, serializable schema language is inherently LLM-friendly — structured, parseable, constraining. An LLM reading `water_potential<MPa> where value <= 0.0` knows exactly what it's looking at. The type system IS the LLM-friendly API.

## Client-Side Type Propagation: DX Details

The fetch boundary shouldn't be where types die. If the R client knows `leaf_wp` is `water_potential<MPa>`:
- Attach `units::set_units(leaf_wp, "MPa")` automatically
- Warn or error on `leaf_wp[1] <- 0.5` (constraint violation)
- Display column as "leaf_wp (MPa, ≤0)" in print output

Similarly in Python:
- Polars column-level metadata carries OzzyDB type info
- Pandas ExtensionArrays wrap constrained types
- Thin wrapper type-checks mutations

The types become living annotations on native objects. The scientist never writes `units::set_units()` — the fetch did it. They never wonder "is this MPa or Bar?" — it's annotated. The type system is invisible when things are right and loud when things are wrong.

The client library generates constrained column types FROM OzzyDB type definitions. Semantic types propagate from the type registry, through the server, through the fetch, into the client's native type system.

## Open Questions

- Where exactly is the constraint language boundary? Arithmetic over field names seems right. No quantifiers, no matrix properties.
- How do versioned types interact with content-addressing? Is the type version part of the hash?
- Can type definitions themselves be content-addressed? (Hash of definition = identity.)
- What's the base representation? Pure bytes, or something with more structure (Arrow-like memory layout)?
- How do types travel cross-language? Serialize to JSON schema? Generate per-language validators?
- How do system-scope constraints (structural inference) work in OzzyDB specifically? DAG analysis at push time?
- Is there a clean way to express "this type is a refinement of that type" for the Candidate -> Validated pattern?
- Cache policy syntax and semantics for parametric branching.
- How do bindings work for custom/exotic classes? Introspection + LLM generation?
- Which hub types to prioritize? Arrow IPC seems like the strongest candidate for tabular data.
- How deep does client-side type propagation go? Just metadata? Active validation? Full newtype wrappers?
- Can we auto-generate R/Python constrained column types from OzzyDB type definitions?
- What happens when a binding is wrong? (e.g., LLM-generated adapter produces bad conversions) Verification strategy?

## Bindings ARE Encodings (the three-concept collapse)

(Riley) Re: bindings, I'm not sure exactly how this is different from the "semantics vs encodings" idea we had earlier. Here's a simple example: a CSV with rows like `A,B,C,DE,F` and the same data represented as `ABCDEF` + `[commas at positions 1,2,3,5]` — these are the same semantic class with different representations. An `A()` function can interoperate between them. Same thing with pandas DataFrames and R data.frames — they're just different encodings of the same semantic content. The type system infrastructure should be for defining a "type universe" — the meta-structure of how types work.

(Claude) You're right — the three-concept model (constraints + encodings + bindings) collapses to two: **semantic types** and **encodings**. A binding IS just an encoding. `pandas.DataFrame` is an encoding of `table<{x: float, y: float}>`, exactly like CSV or Parquet is. The only difference is that some encodings are serialized (bytes on disk) and some are in-memory (live runtime objects), but that's a property of the encoding, not a separate category.

This means:
- `csv` = encoding (serialized, text)
- `parquet` = encoding (serialized, binary)
- `pandas.DataFrame` = encoding (in-memory, Python)
- `R data.frame` = encoding (in-memory, R)
- `Arrow IPC` = encoding (serialized, cross-language)
- `polars.DataFrame` = encoding (in-memory, Python)

An adapter between any two encodings of the same semantic type is the same kind of operation: `A: encoding_a → encoding_b`. Whether that's CSV→Parquet, Parquet→DataFrame, or DataFrame→data.frame, it's all encoding conversion. The semantic type is the invariant that both sides must satisfy.

The CSV example is perfect: `A,B,C,DE,F` (delimiter-separated) and `ABCDEF + [1,2,3,5]` (positional offsets) encode the same semantic content (a sequence of strings) differently. The adapter between them is well-defined because both satisfy the same semantic type.

## Meta-Type-System / "Type Universe"

(Riley) The type system infrastructure should be for defining a "type universe" — the meta-structure of how types work (conjunction, entailment, encoding, adapters) — and then instantiating it differently for different contexts. The e-graph system would be a compiled version (types resolved at compile time, constraints propagated symbolically). OzzyDB would be a runtime version (types checked at push/compute time, constraints validated against actual data).

(Claude) This is a clean separation:

**The type universe** defines:
- What a type IS (a set of constraints over bytes)
- How types compose (conjunction = intersection)
- What entailment means (A entails B if A's constraint set is a superset of B's)
- What an encoding is (a way to represent a semantic type as bytes or runtime objects)
- What an adapter is (a function between encodings that preserves the semantic type)

**OzzyDB instantiation** (runtime):
- Types are registered objects, checked at push/compute time
- Constraints validated against actual data values
- Adapters are concrete functions (serialize/deserialize)
- Encoding negotiation happens at fetch time

**E-graph instantiation** (compiled/symbolic):
- Types are nodes in the e-graph
- Constraints propagate forward/backward through edges
- Adapters are rewrite rules
- Entailment is provable without data (structural reasoning)

Same vocabulary, same laws, different evaluation strategy. The standalone Rust crate would implement the type universe. OzzyDB and the e-graph system would each instantiate it with their own evaluation backend.
