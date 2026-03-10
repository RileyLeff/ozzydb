Tell me about type theory. What links eg Rust and CUE? How would I design a type system from scratch?

4:15 PM

Synthesized type theory concepts linking Rust and CUE systems

Type theory is a rich area that sits at the intersection of mathematics, logic, and programming language design. Let me walk through the key ideas.

The Core Idea
At its heart, a type system is a way of classifying values and constraining the programs you can write so that certain classes of errors are impossible. The foundational insight, going back to Bertrand Russell and later formalized by Alonzo Church (the simply-typed lambda calculus, 1940), is that you can use types as a proof system — a program that type-checks is, in a deep sense, a proof that certain invariants hold. This is the Curry-Howard correspondence: types are propositions, programs are proofs.

What Links Rust and CUE
This is a great pairing to notice. On the surface they look nothing alike — Rust is a systems language, CUE is a configuration/data validation language — but they share a deep structural commitment: both use lattice-based type systems to enforce constraints at compile time rather than runtime.

CUE makes this explicit. In CUE, types and values live on the same lattice. A type like string is just a very broad constraint (the "top" of all strings), and a concrete value like "hello" is a very narrow one. You refine values by unifying constraints — the & operator computes the greatest lower bound (meet) in the lattice. If two constraints are incompatible, you get bottom (an error). There's no separate "type" vs "value" distinction; it's constraints all the way down.

Rust does something analogous but in a different domain. Its ownership and borrow checker is essentially a substructural type system (specifically, an affine type system — values can be used at most once unless explicitly borrowed). The trait system forms a lattice of capabilities: Send, Sync, Clone, etc. are constraints on types, and trait bounds compose via intersection, much like CUE's unification. Rust's lifetime system is also lattice-structured — lifetimes have a partial order (outlives relationships), and the compiler computes the tightest valid lifetime at each point.

The shared DNA is: constraint propagation over a partial order, checked statically. Both reject the "just check it at runtime" philosophy in favor of making illegal states unrepresentable.

Other systems in this family include refinement types (Liquid Haskell), dependent types (Idris, Lean), and SMT-backed verification (Dafny, F*).

Designing a Type System from Scratch
If you were building one, here's a roadmap organized by the decisions you'd need to make:

1. Choose your foundation. The main options are:

Simply-typed lambda calculus — the baseline. Types are ground types and function types (A → B). Easy to implement, limited expressiveness.
Hindley-Milner (System F restricted to rank-1 polymorphism) — this is what ML, Haskell, and Rust's core are built on. You get parametric polymorphism (∀a. a → a) with full type inference. The key algorithm is Algorithm W (Damas-Milner), which does unification-based inference.
System F / System Fω — full polymorphism, type operators. More expressive, but you lose complete inference (some annotations required). Haskell with extensions moves toward this.
Dependent types — types can depend on values (Vec n a, a vector of exactly n elements). Maximum expressiveness, but type checking can become undecidable or require programmer-supplied proofs.
2. Decide on subtyping vs. parametricity. Subtyping (as in Java, TypeScript, CUE) says "an Int is a Number." Parametric polymorphism (as in ML, Haskell) says "this function works for any type a, and I can't inspect what a is." These give you very different design tradeoffs. Subtyping is intuitive but makes inference much harder (you need constraint solving, not just unification). Rust threads the needle: no traditional subtyping, but lifetime subtyping and trait-based bounded polymorphism.

3. Pick your inference strategy:

Unification-based (Hindley-Milner): bidirectional flow of type information, complete inference. Fast, well-understood.
Bidirectional type checking: types flow "down" from annotations and "up" from terms. A good middle ground — less magical than HM, but handles richer type systems (used in Agda, Idris, and increasingly in Rust and TypeScript).
SMT/constraint-based: generate constraints, throw them at a solver. Used for refinement types and flow-sensitive typing.
4. Decide what invariants you want to enforce. This is where it gets creative:

Memory safety → ownership/borrowing (Rust's affine types)
Data shape validation → unification lattices (CUE)
Null safety → option types or flow-sensitive narrowing (Kotlin, TypeScript)
Concurrency safety → effect systems or capability types (Send/Sync in Rust)
Totality → termination checking (Idris, Lean)
5. Implement it. The practical steps are roughly: define your AST with type annotations, write a constraint generation pass over the AST, write a solver (unification for HM, or something more sophisticated), and produce typed IR or error messages. Benjamin Pierce's Types and Programming Languages (TaPL) is the standard reference and walks through implementations of progressively richer systems.

A minimal starting point would be: implement a simply-typed lambda calculus with Int, Bool, and → types, then add Hindley-Milner inference, then experiment with one advanced feature (algebraic data types, traits/typeclasses, or refinement types) depending on what problems you want to solve.

The key insight across all of this is that type system design is fundamentally about choosing which invariants matter enough to verify statically, and then finding the right algebraic structure (usually some kind of lattice or partial order) to express and check those invariants efficiently.

Cool. Here’s my specific problem domain. I have 2 applications:

OzzyDB. This is basically a stateless, functional database. The idea is that you upload your raw, ugly data, not your final product or output. Then, you link a GitHub repository to your project that contains “transforms” (functions) and “endpoints” (links in a DAG between data and transforms). The endpoints can easily be queried from a client library. Transforms are executed in “environments”, which are just plain docker files. Everything is stored behind the scenes in s3 and compute is hosted on fly.io or other providers. It’s a boon for reproducibility, reusability, and sharing. Imagine my paper has a figure, and you could grab the exact state of my data with ozzydb.fetch(“rileyleff/cool-paper/figure1@latest”). It’s polyglot, collaborative, and handles data versioning in a Kolmogorov-friendly way. Imagine you have a unit conversion on a huge table stored in git; it’s a text diff, it isn’t super meaningful. Same with a diff on a binary format. Here, we just need to store the semantic description in code. It’s built to be equally friendly for humans and LLMs.
I have a prototype of it that works well but what I’m finding is that the tricky part is the surface between the edges of transforms: without a type system, how do you know what can be connected where? How do you know which client libraries can load what? Right now it’s kind of a mess where transforms are like implicitly reading in files to the environment, writing out stuff to the file system and recovering it to the DAG, it’s weird. I want a way to describe data’s type — both the semantics (this is a plant water potential, it’s in MPa, it has to be less than zero) and the encoding (this is a float64, or this is a csv, or this is a python object, or whatever). I’m wondering if there’s a unified way to handle semantics and encoding in a CUE like way as constraints or refinements on a lattice. There’s also this idea of type equivalence or linking — say you have a R data frame. It’s a case of an encoding in a tabularg format. It could losslessly be represented as a csv, parquet, pandas df, whatever. If we have some transform in ozzydb that links a type to another type instead of just data to data, we can build a sort of spiderweb of conversions that would make for a very nice UX. Imagine that arrow is sort of a universal linker for all the different tabular types. If you can get in and out of arrow, you can easily adapt to these other formats. You could imagine the same for other categories or refinements, like hash tables, blobs, linked lists, graphs, whatever. I think the explicit, reproducible, well-tracked nature of the ozzydb design makes it a good candidate for LLM agent generated adapter types — imagine a statistical model in R that you want in numpyro. An agent could probably adapt that very well. And even if it misses slightly, at least it’s auditable and versioned so you can see it and fix it. I want data to move like liquid!

For “proof”, I would love to have some simple rules, like “if you claim this is a csv refinement, we can verify by parsing with the rust csv parser” and as long as we version everything it should be good?

One challenge with this type system is that we need to be able to define new types at “runtime” live on the db. Imagine I have my csv type for my raw data, but I want to refine by saying the columns of my csv have these types with these constraints, understand?

My other project doesn’t have a name yet, let’s call it “sock” for now. The idea is that it’s a modeling engine where you represent your equations as e-graphs, and the operators are invertible pairs like add/sub or mul/div.  You can use e-graphs to link together multiple “LEGO bricks” of models via equivalent points. Systems can be overconstrained or under constrained, unlike modelingtoolkit.jl or modelica — in those cases we need either a resolution strategy (over constrained) or combined numeric and symbolic solves (under). It’s nice because there are no hard coded inputs or outputs, and again it  makes it fast to build, extend, reproduce, share. I am an ecologist and thought of this because of the sparsity/weirdness in my real data. I hope to make it so that models with directions imposed on top of them can be exported to jax/torch in the future. The types and constraints are interesting here because types propagate through the graph like values. Another thing to note about this is that I have explicitly made time a non-first-class item: different time points are just value nodes that connect to previous time points. Time is just a semantic thing. Here, I would like my type verification to happen as a “compile time” step.

What do you think of these cases? They share a little dna but have different requirements (eg dynamic at runtime vs static at compile time). Wondering if I could build one type system core in rust and use it to construct these two different cases of type system. Let’s not try to one shot a design, let’s just think it over.

Show less
4:45 PM

Synthesized shared constraint-propagation core for divergent type system requirements













This is a really compelling pair of projects, and your instinct that they share DNA is correct. Let me think through what I see.

The Shared Core
Both OzzyDB and Sock are fundamentally doing the same thing: constraint propagation over a graph, where the constraints live on a lattice. The graph topology differs (a DAG of transforms vs. an e-graph of equations), the lattice elements differ (data format/schema/semantics vs. units/dimensions/numeric bounds), and the timing differs (incremental at runtime vs. batch at compile time) — but the algebraic machinery is the same. That's your shared Rust core.

The core would need to provide roughly:

An open, runtime-definable lattice — not a lattice baked into Rust's type system, but a lattice-as-data-structure where you register new type constructors, refinement rules, and subtype (or convertibility) relationships dynamically. Both projects need this: OzzyDB because users define new schema refinements on the fly, Sock because modelers introduce new quantity types and unit systems.
A constraint store and propagation engine — you accumulate constraints ("this node has type T", "these two nodes must be compatible", "this edge requires a conversion") and propagate until fixpoint or conflict. This is the same algorithm in both cases, just parameterized differently.
A conflict/slack detector — overconstrained (type conflict) and underconstrained (ambiguous, needs more info). Both projects need this. In OzzyDB an ambiguous type means you can't pick a client deserializer. In Sock it means the system is underdetermined.
Where They Diverge (and Why That's Fine)
The divergences are essentially in what I'd call the "frontend" (how constraints are generated) and the "proof backend" (how you verify claims).

OzzyDB's specific needs:

Your multi-dimensional type structure is the most interesting design challenge. A single piece of data in OzzyDB has at least three facets that refine semi-independently:

Encoding — the physical format: CSV, Parquet, Arrow IPC, pickle, RDS, a raw TIFF, whatever.
Shape/Schema — the structural contract: "tabular with columns [species: string, wp: float64, date: date]", or "3D array of float32", or "directed graph with node attributes."
Semantics — the scientific meaning and constraints: "column wp is plant water potential in MPa, must be ≤ 0", or "this is a phylogenetic tree where branch lengths are in substitutions per site."
The CUE-like insight is that these aren't separate systems — they're all just refinements on the same lattice. any ⊐ tabular ⊐ tabular{cols: [species: string, wp: float64]} ⊐ tabular{cols: [species: string, wp: float64 & ≤0]}. The encoding dimension is orthogonal but connected via your conversion morphisms.

And this is where your conversion graph idea gets really elegant. The conversions form a category:

Objects are types (at various levels of refinement)
Morphisms are transforms that convert between representations
Some morphisms are isomorphisms (lossless roundtrips: DataFrame ↔ Arrow ↔ Parquet)
Some are lossy projections (float64 → float32, or dropping columns)
Composition gives you multi-hop conversion paths
Arrow as a hub type is exactly the right instinct — it's a universal object in the subcategory of tabular formats. If every tabular format has a morphism to/from Arrow, you get n converters instead of n². And you're right that this extends beyond tabular: you could have a "universal graph" hub (e.g., adjacency list in Arrow or something like GraphBLAS), a "universal tensor" hub (safetensors? zarr?), etc.

The "proof" strategy for OzzyDB can be lightweight and pragmatic exactly as you described — if you claim something is a CSV with a certain schema, the system can verify by attempting a parse. The key insight is that you're not trying to prove arbitrary theorems; you're checking membership in well-defined, mechanically-verifiable sets. That's tractable.

The runtime extensibility requirement means your lattice has to be a runtime data structure — essentially a DAG of type nodes with meet/join operations defined by registered rules. New refinements just add nodes and edges. This is more like a database of type relationships than a traditional compiler's fixed type system.

Sock's specific needs:

Here the lattice is different in character. You're dealing with:

Dimensional types — units form a free abelian group under multiplication (m·s⁻¹·kg, etc.). Type checking is essentially dimensional analysis. This is well-studied and maps perfectly onto lattice-based checking.
Numeric constraints — bounds, relationships. "Pressure must be positive." "These two quantities sum to this third." This is closer to refinement types or an SMT problem.
Bidirectional propagation — because operators are invertible, type information flows both ways. If a + b = c and you know the types of a and c, you can infer b. This is a significant difference from OzzyDB where the DAG has a clear direction.
The e-graph representation is a natural fit because e-graphs already handle the equality reasoning you need. Type propagation over an e-graph is essentially abstract interpretation: you're computing a fixpoint of type assignments over equivalence classes. The constraint propagation core handles this — you just wire it up differently.

The "compile time" checking for Sock means you can afford to do more expensive analysis (full fixpoint computation, conflict detection across the whole graph) because it happens once before execution, not incrementally as data flows.

Can One Core Serve Both?
Yes, and I think the architecture looks something like this:

┌─────────────┐     ┌──────────────┐
│   OzzyDB    │     │     Sock     │
│  Frontend   │     │   Frontend   │
│ (DAG edges, │     │ (e-graph,    │
│  schema     │     │  dimensional │
│  claims)    │     │  analysis)   │
└──────┬──────┘     └──────┬───────┘
       │                   │
       ▼                   ▼
   constraint          constraint
   generation          generation
       │                   │
       ▼                   ▼
┌──────────────────────────────────┐
│        Shared Core (Rust)        │
│                                  │
│  • Open lattice (TypeRegistry)   │
│  • Constraint store              │
│  • Propagation / unification     │
│  • Conflict & slack detection    │
│  • Conversion path finding       │
└──────────────────────────────────┘
The TypeRegistry is the key abstraction — it's an open, runtime-mutable registry of:

Type constructors (with parameters)
Refinement rules (how types relate in the lattice)
Morphisms (conversions, with metadata about lossiness)
Verification hooks (for OzzyDB's "proof by parsing")
The propagation engine is generic over the lattice contents. It just needs meet, join, and subtype-checking operations, which dispatch to the registry.

Things I'd Want to Think More About
A few tensions and open questions worth sitting with:

The "refinement at runtime" problem for OzzyDB. When someone says "my CSV has columns [x: float64, y: float64 & <0]", they're defining a new type that didn't exist before. This is basically dependent typing — the type depends on the data's schema, which is itself data. CUE handles this gracefully because there's no type/value distinction. Your system might want a similar philosophy: a type is just a constraint expression, and the registry stores them as data. You're not compiling a fixed type system; you're building a constraint database.

Conversion path search. When someone does ozzydb.fetch("rileyleff/cool-paper/figure1@latest") from Python, the system needs to find a path from whatever encoding the data is stored in to something Python can consume. That's a graph search problem over the morphism category, potentially with a cost model (prefer lossless, prefer fewer hops, prefer Arrow as intermediate). This is a cool problem — it's basically type-directed coercion search, which has nice parallels in typeclass resolution (Haskell/Rust).

The LLM-generated adapter idea is genuinely good. If conversions are just transforms in the DAG, and they're versioned and auditable, then having an LLM generate a conversion from "Stan model" to "NumPyro model" is no different from having a human write one — it's just another morphism in the category, subject to the same verification and versioning. The type system gives the LLM a contract to satisfy, and the verification hooks tell you if it succeeded. That's a really natural fit.

Sock's over/underconstrained detection maps directly onto the lattice. Overconstrained = you computed a meet and got bottom. Underconstrained = some type variables haven't been refined below a threshold. The interesting question is what the resolution strategies look like for overconstrained systems — that's more of a domain-specific policy layer on top of the core.

Shared vocabulary between the two systems. There might eventually be a bridge — a Sock model that produces predictions could feed into OzzyDB as a transform, and the type system should be able to verify that the Sock model's output types are compatible with the OzzyDB endpoint's expected input types. If both use the same core, this comes for free.

I think the right next step isn't to design the full type system, but to nail down the lattice structure for one of the two cases (probably OzzyDB since you have a working prototype and a concrete pain point) and see what the core abstractions need to be. Once you have that, you can check whether Sock's needs fit into the same abstractions or require extensions. Want to dig into that?

Yeah I agree. I have a prototype for sock too but it's just some python slop. Ozzydb is literally live on ozzydb.com and i have a demo project on there, though sign-in is just gated to github users named rileyleff (me).

So i envision that types should be "first class objects" in ozzydb. If I fire up a project, i might want to say that this project includes dataset A (say it's water potentials), transforms B, C, and D (functions that have typed input and output), and types E, F, G, H, etc (think water potential, these units, less than 0, etc). Then I could import from ozzydb standard lib: parquet, float64, etc. And then I build the lattice: i say that my dataset A is tabular@latest, csv@latest, columns blah blah blah with these constraints. One interesting thing is like "collection" types or similar. Say I have a constraint that depends on another object: say that value X must be value than greater Y, enforced at the type level. I think it makes sense to enforce that constraint on a container type, where Z contains X and Y, and Z "owns" the constraint? Thoughts? any cases that might break that? And maybe containers/collections (collections probably a better word because container = docker container, means something else) would be nice because you refer to a group, you could say like this csv is one forest census, but this collection of csvs is the set of forest censuses. Right? A design pattern i like is say you upload a csv and it's raw and busted from the field, lots of typos or fucked up values. You might assign it a type like "Candidate_Forest_Census" that has very few restrictions (e.g. maybe it fails csv parse! maybe the values violate the intended constraints on the column due to weird outliers or errors) and then a transform converts it into the "solid" type that you actually want, Forest_Census. I like the idea of runtime types because then I don't have to decide up front, like, here's what types are valid or acceptable for you to use. I would never be able to get it right. And I think versioning is essential because it's inevitable that I will need to change some type definitions, and I need to be able to make guarantees about which data conforms to which version of the type specification. I think one risk this runs is to have a bunch of "parallel" types that could in theory be just one type. Like say we have tabular -> csv and tabular -> tsv, is there anythign stopping anyone from saying "tsv is a top level type at the same level as tabular, lemme just use that"? Maybe we should let people just footgun themselves within their project but have a tighter grip on the standard lib. And have some PR or proposal mechanism for updates or additions to the standard lib. Thoughts?

Show less
5:25 PM

Synthesized design patterns for type systems and governance structures


Really good set of questions. Let me work through them one by one because there are some subtle interactions.

Collection Types and Cross-Value Constraints
Your instinct about ownership of constraints is right, and there's a clean theoretical grounding for it. The principle is: a constraint that mentions multiple values must live on a type that "sees" all of them. This is essentially the same rule as variable scoping in programming languages — a constraint is a predicate, and it can only be evaluated in a scope where all its free variables are bound.

So yes, if you have "X < Y", that constraint belongs on the collection Z that contains both X and Y. This falls out naturally if you think of it as refinement: Z isn't just {x: WaterPotential, y: WaterPotential}, it's {x: WaterPotential, y: WaterPotential} & (x < y). The refinement predicate x < y is part of Z's type, not part of X's or Y's type individually.

Where this could get tricky: what about constraints that span collections? Say you have two separate forest census CSVs from different years, and you want to assert that every species in census 2024 also appears in census 2020. That constraint lives on... what? You'd need a higher-level collection that contains both censuses. This suggests your collection types should be nestable — a collection of collections — and constraints can live at any level. That's fine, it just means your lattice has product types with refinements at every level.

I don't think this breaks anything, but it does mean you need to be clear about when constraints are checked. A constraint on a collection can only be verified when the whole collection is materialized. If someone is streaming in rows of a CSV one at a time, the per-row constraints (column types, bounds on individual values) can be checked incrementally, but the cross-row constraint ("all species in census A appear in census B") can only be checked at the end. This is a natural distinction: element-level refinements vs. aggregate-level refinements. Both live on the lattice, but they have different verification strategies and timing.

The Candidate/Solid Pattern
This is a really nice design pattern and it maps onto something well-understood in type theory: gradual typing or, more precisely, the idea of a type ascription with deferred verification.

Candidate_Forest_Census is essentially saying "I intend for this to be a forest census, but I haven't verified it yet." The transform from Candidate to Forest_Census is a validation boundary — it's a morphism in your type category that is partial (it can fail) and whose success constitutes proof of the refinement.

This is actually a beautiful fit for your system because it makes the data cleaning pipeline legible in the type system. The DAG tells the story: raw upload → Candidate type (loose) → cleaning transform → validated type (tight). Anyone looking at the project can see exactly where validation happens and what it checks.

One thought: you might want to make the Candidate/Solid relationship explicit in the type system rather than just a naming convention. Something like: Candidate_Forest_Census is automatically related to Forest_Census as a supertype — it's strictly looser. This way the system knows that a transform from Candidate to Solid is a refinement (narrowing), not an arbitrary conversion. That metadata is useful for tooling, search, and LLM-generated adapters.

You could even have a general mechanism: any type T implicitly has a Candidate<T> or Unvalidated<T> supertype that strips some or all of its refinement predicates but keeps the structural shape. That gives you the pattern for free without users having to manually define both types.

Runtime Type Definition
Totally agree this has to be runtime-defined. You can't anticipate what types ecologists, physicists, economists, etc. will need. The type registry is a database, not a compiler.

The key architectural decision is: what is the representation of a type? I think it's something like:

A name (namespaced to the project or stdlib: rileyleff/cool-paper/WaterPotential@v2)
A parent in the lattice (what it refines: float64 & ≤0, or tabular & {cols: [...]})
A set of refinement predicates (executable checks, with defined verification strategies)
A set of morphisms to/from other types (conversions, with lossiness metadata)
Metadata (units, semantic description, documentation — stuff that's useful for humans and LLMs but not mechanically checked)
Types are versioned just like data and transforms. A type is essentially a document in the system — it has a history, it can be forked, it can be imported.

Versioning and Conformance
This is where it gets subtle and important. You're absolutely right that you need to track which data conforms to which version of a type.

The key invariant is: a type assignment is a triple of (data version, type version, verification status). When you say "dataset A conforms to Forest_Census@v3", that's a claim that was verified at a specific point in time. If Forest_Census evolves to v4 (say, adding a new constraint), existing data tagged as v3-conformant is not automatically v4-conformant — it would need re-verification.

This means type versions should be immutable once published (like a git commit). If you want to change a type, you publish a new version. The system can then tell you: "you have 47 datasets conforming to Forest_Census@v3; here's what changed in v4; want to re-verify them?"

There's a nice analogy to semver here. You could have:

Patch: tightened a constraint (all v3-conformant data is still v4-conformant by construction — the new version is a subtype of the old)
Minor: added a new optional field or loosened a constraint (old data conforms, but new data might not conform to old version)
Major: breaking change (no guarantees, re-verification needed)
Whether you want to enforce semver or just track it as metadata is a governance question, but having the lattice means you can actually compute the subtype relationship between type versions and tell the user whether a version bump is breaking or not. That's powerful.

The Parallel Types Problem
Your example of someone defining TSV as a top-level type instead of as a refinement of tabular — this is a real problem and I think your proposed solution is exactly right: permissive within projects, opinionated in the stdlib.

Within a project, you're the author. If you want to define TSV as a top-level type, fine, it's your footgun. The system might warn you ("hey, the stdlib has tabular > tsv, and your my_tsv looks structurally similar — did you mean to refine the stdlib type?"), but it doesn't block you.

The stdlib, on the other hand, should be curated and should enforce lattice coherence. If csv and tsv are both delimiter-separated tabular formats, they should both refine a common delimited_tabular type, which refines tabular. The stdlib encodes the "correct" ontology.

The PR/proposal mechanism for stdlib additions is good. A few additional thoughts on governance:

Stdlib types should be defined using the same mechanism as user types. No magic. The stdlib is just a blessed project that other projects can import from. This keeps the system honest — if the stdlib type mechanism can't express something, that's a bug in the core, not an excuse to add special cases.
Allow project-local types to be "promoted" to stdlib. If someone defines a great PhylogeneticTree type in their project and it gets adopted by others, there should be a path to standardize it.
Allow type aliases and structural equivalence. If someone independently defines a type that's structurally identical to a stdlib type, the system should be able to detect that and suggest linking them. This reduces fragmentation without forcing everyone to know the stdlib intimately upfront.
One More Thing: The Encoding/Schema/Semantics Stack
I want to come back to how these three dimensions interact in practice, because I think this is where the UX lives or dies.

When someone uploads a CSV of water potentials, the system should be able to infer some things and ask for others:

Encoding: auto-detected (it's a CSV, we can try parsing it). ✓
Schema: partially inferable (column names from header, column types from sampling values). Might need user confirmation.
Semantics: not inferable at all — the system doesn't know that column wp means water potential in MPa. The user (or an LLM reading the README) has to provide this.
The type assignment UX might look like: upload → auto-detect encoding → infer schema → prompt for semantic annotation → produce a full type. Each step narrows the type from any down through csv → csv{cols: [species: string, wp: float64, date: date]} → csv{cols: [species: string, wp: WaterPotential, date: date]} where WaterPotential = float64 & ≤0 & unit(MPa).

And crucially, each of those intermediate types is a real type in the lattice. Someone could import your data at the schema level without caring about the semantics, or at the encoding level without caring about the schema. The lattice gives you that flexibility.

I think the next concrete step might be to sketch out what the type definition language looks like — the actual syntax someone would write in their project repo to define types, assign them to data, and declare constraints. That would force a lot of these design questions into sharp focus. Want to go there?

Re: collections, versioning is the critical thing to get correct here. If we have a collection that contains like the first 2 months of my field data, and it meets those criteria, but then i expand the collection to include the 3rd month of my field data later in a new version, we need to check again, right?

Re: I actually think that I'm fine with Candidate and Solid being loose. I don't think I need the type system to encode that they're related, as the candidate type is probably just going to be "generic csv" or "blob of bytes" in most cases. The transform should have a "here's our input" and "here's our output" type, like a rust function, that makes the relationship (candidate in, solid out) explicit.

Actually the general mechanism where T implicitly has a Candidate might be really nice UX. Hm. I'll keep thinking on it. In general i prefer to go leaner, but encoding a good design pattern into the type system might be worth it.

Re: what is a type:

wonder if there's a way to make the units meaningful. something akin to uom in rust. maybe we could do this for like verifying interoperability, though that would be hard to do as we kinda have open season on what programming languages and tools you can use. maybe we verify at the inputs and outputs of each transform? at the very least, we should have a very nicely organized and thoughtful selection of the different possible units, with some attached meta/info about their relationships. i want units to be structured, not text. the idea is that units should be consistently defined so as to be machine parsable without any intelligent model necessary or ambiguity in parsing.

also, are there different lattices for structure and semantics?

Last question from me, how do we handle something like "this is a python object, produced by numpyro version xyz on python 3.13, maybe it's compatible with these other versions". Two cases to reason about are tabular data, which is encoded in a way specific to python (e.g. imagine pickling a pandas df) but could be losslessly represented in R, julia, parquet, whatever. A good design pattern might be to make it customary for users to say "here's my transform that produces a pickled python object, but ALSO here's a transform on that pickled python object to convert it into a serialized format, or something". But maybe common cases like pandas dfs would already have std adapters. The other case to reason about is something like a statistics framework: you could reasonably represent the same model in numpyro vs stan , but those have pretty different structures, no? even though they contain the same information. I'm not sure how to represent (or whether we even should represent) like, this is inherently tabular data, but you can only pop it open if you're in the python client library on these versions.

Show less
5:44 PM

Parsed multiple interconnected design questions about OzzyDB's type system

These are all great questions and they're getting at some of the hardest parts of the design. Let me take them in order.

Collection Versioning
Yes, exactly. A collection's type conformance is a function of its contents at a specific version. If collection C@v1 = {month1, month2} conforms to type T, and C@v2 = {month1, month2, month3}, then C@v2 needs re-verification against T. This falls out naturally from the principle that a type claim is always (data@version, type@version, verified: bool).

The nice thing is you can be smart about what you re-check. If the type T on the collection has two kinds of constraints — element-level ("each CSV must have these columns with these bounds") and aggregate-level ("all CSVs must share the same species list") — then when month3 is added, you only need to:

Check month3 against the element-level constraints (the existing months already passed).
Re-check the aggregate constraints across all three months (because these are inherently whole-collection properties).
This is basically incremental type-checking. The system could track which constraints are invalidated by a given change and only re-verify those. Adding an element to a collection invalidates aggregate constraints but not previously-verified element constraints. Modifying an element invalidates that element's constraints plus any aggregate constraints. This is an optimization, not a correctness concern — you could always just re-verify everything — but for large collections it matters a lot.

Candidate Types — Keep It Lean
I agree with your instinct to keep it lean for now. The transform signature fn clean(input: Blob) -> ForestCensus or fn clean(input: GenericCSV) -> ForestCensus already captures the relationship. The transform is the proof that the output conforms. You don't need the type system to additionally encode "these two types are related as unvalidated/validated versions of the same concept" — that's metadata for humans, not something the propagation engine needs.

If the Candidate<T> pattern proves useful later as a UX convenience (auto-generating a loose version of a type), you can add it as sugar without changing the core. Good call to defer it.

Units: Structured, Not Text
Absolutely right that units should be structured. The uom crate in Rust is a good model for the algebra even if the implementation strategy differs. Units form a well-understood algebraic structure: they're elements of a free abelian group generated by base dimensions. So:

Base dimensions: length, mass, time, temperature, amount, electric current, luminous intensity (the SI base quantities, plus any domain-specific ones you want to add like "number of individuals" for ecology)
A unit is: a tuple of rational exponents over dimensions, plus a scale factor and offset relative to a reference. So MPa is (mass: 1, length: -1, time: -2, scale: 1e6) relative to Pa.
Unit compatibility is: same dimensional exponents. Unit conversion is: apply scale and offset.
This is fully machine-parseable with zero ambiguity. You'd have a stdlib registry of known units (SI, common derived units, domain units), and the type system understands the algebra. So if someone defines WaterPotential = float64 & unit(MPa) & ≤0, the system knows that MPa has dimensions of pressure, and if a transform expects input in kPa, it can automatically verify that the dimensions match and insert (or suggest) a scale conversion.

The question of where to verify is important. I think your instinct is right: verify at transform boundaries. Inside a transform, the user's code does whatever it does in whatever language — the system can't inspect Python or R internals. But the contract at each edge says "this input must be float64 in MPa" and "this output will be float64 in kPa", and the system checks dimensional consistency of the DAG without needing to look inside the transforms. This is exactly the same philosophy as Rust's type system: the function signature is the contract, the compiler doesn't need to understand the function body to verify that call sites are well-typed (well, it does for inference, but the boundary is what matters for modularity).

For the "open season on languages" problem — the unit algebra lives in your Rust core, not in Python or R. The transforms declare their input/output types (including units) in OzzyDB's type language, and verification happens in the core. The runtime inside the Docker container can be anything; it just has to produce data that conforms to the declared output type.

One Lattice or Multiple?
This is a key design question. I think the answer is: one lattice, but with product structure. Let me explain.

You have several dimensions of refinement:

Encoding: blob → binary format → csv / parquet / arrow / pickle / ...
Structure: any → tabular / tensor / graph / scalar / ...
Schema: tabular → tabular{cols: [x: float64, y: string]} → ...
Semantics: float64 → float64 & unit(MPa) → float64 & unit(MPa) & ≤0
These refine semi-independently, which means your type lattice is naturally a product lattice — the Cartesian product of the individual lattices, with componentwise meet and join. A concrete type like "CSV-encoded tabular data with columns [species: string, wp: WaterPotential]" is a point in the product: (encoding: csv, structure: tabular, schema: {species: string, wp: float64}, semantics: {wp: unit(MPa) & ≤0}).

But the dimensions aren't fully independent — there are cross-cutting constraints. Not every encoding can represent every structure (you can't encode a graph as a CSV without flattening it). Not every schema is compatible with every encoding (CSV can't natively express nested structures). These cross-cutting constraints are basically validity rules in the product lattice: certain combinations are bottom (impossible), and the system should detect that.

So pragmatically, I'd implement it as one lattice with tagged/structured nodes, where each node has encoding, structure, schema, and semantic components, and the meet operation works componentwise but then checks cross-dimensional consistency. This gives you the cleanness of separate concerns with the power of a unified constraint propagation engine.

The Python Object / Opaque Blob Problem
This is maybe the most practically important question because it's where the real friction will be for users. Let me think through the cases:

Case 1: Tabular data trapped in a language-specific format (pickled DataFrame).

The data is logically tabular, but physically opaque to anything that isn't Python with the right libraries. Your instinct about the design pattern is right: the best practice is to have the transform that produces the pickle also produce (or be followed by) a conversion to an open format. But I think the type system can make this more than a convention — it can make it a visible property of the data.

The type of a pickled DataFrame might be something like:

encoding: python_pickle{requires: [pandas>=2.0, python>=3.11]}
structure: tabular
schema: {cols: [species: string, wp: float64]}
semantics: {wp: WaterPotential}
The encoding field carries the dependency information. The system knows that this data is tabular (structurally), so it could be converted to Parquet or Arrow, but the current encoding requires Python. A stdlib adapter pickle_dataframe → arrow would exist, and the system could suggest or auto-insert it when someone tries to fetch from a non-Python client.

This is where your conversion category pays off. The system doesn't need to understand pickles — it just needs to know that a morphism pickle_dataframe → arrow exists, is lossless for tabular data, and requires a Python environment to execute. The client library says "I can consume Arrow", the data is currently in pickle, the system finds the conversion path and executes it (or tells the user it needs to be run).

Case 2: Semantically equivalent but structurally different models (NumPyro vs. Stan).

This is genuinely harder. A Bayesian model in NumPyro and the "same" model in Stan don't just differ in encoding — they differ in structure. The likelihood, priors, and parameters are the same mathematically, but the code structure, parameter representations, and even inference algorithms differ.

I think the honest answer is: the type system should represent what it can verify, and punt on what it can't. You could define a type like:

structure: bayesian_model
schema: {
  parameters: [alpha: real & >0, beta: real, sigma: real & >0],
  likelihood: normal,
  priors: {alpha: half_cauchy(1), beta: normal(0,10), sigma: half_cauchy(1)}
}
encoding: numpyro_pickle{requires: [numpyro>=0.15, jax>=0.4]}
And a Stan version would have the same structure and schema but encoding: stan_code. A conversion between them is a morphism that preserves structure and schema but changes encoding. That conversion is hard to write automatically (this is where your LLM adapter idea shines), and the system can't fully verify semantic equivalence, but it can verify structural equivalence (same parameters, same distributional families, same likelihood) and flag anything that doesn't match.

The key principle: don't try to make the type system prove that the NumPyro and Stan models are mathematically equivalent. Make it capture enough structure that a human or LLM can write a conversion with confidence, and make the claim of equivalence explicit and auditable. The type system is a tool for managing complexity, not a theorem prover.

The "environment as part of the type" idea is important. I'd make runtime dependencies first-class in the encoding dimension. Something like:

encoding: opaque{
  runtime: python,
  runtime_version: >=3.11 <3.14,
  dependencies: [numpyro>=0.15, jax>=0.4.20],
  serialization: pickle
}
This way the system can answer "can client X consume this data?" by checking whether the client's environment satisfies the encoding's requirements. If not, it looks for a conversion path to an encoding the client can handle. The stdlib would provide common adapters (pickle → arrow for DataFrames, RDS → arrow for R data frames, etc.), and users can register their own for domain-specific formats.

Summary of Where I Think We've Landed
The type core is shaping up as:

Product lattice with encoding, structure, schema, and semantic dimensions
Units as structured algebra, verified at transform boundaries
Collections with element-level and aggregate-level constraints, incrementally verifiable
Versioned type definitions, immutable once published, with conformance tracked as (data@version, type@version)
Runtime dependencies as first-class part of the encoding dimension
Conversion morphisms forming a category, with cost model for path search
Open type registry (stdlib + user-defined), permissive within projects, curated in stdlib
Verification at boundaries: parse-based proofs for encoding, schema checks for structure, predicate checks for semantics
The thing I'd want to prototype first is the lattice representation and the meet/join operations, because everything else builds on top of that. If the lattice algebra works, the rest is plumbing. If it doesn't, you'll find out fast.

