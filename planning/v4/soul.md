# The Soul of OzzyDB, v4

**OzzyDB makes scientific computation inspectable by treating artifacts, types,
transforms, and environments as first-class published objects.**

The point is not to cache files more cleverly. The point is to make the full
chain from uploaded artifact to derived result explicit, typed, and reproducible.

---

## 1. Data is an artifact with a contract

An uploaded blob without a type is just bytes. A derived result without
conformance is just a claim. OzzyDB treats artifacts as first-class objects and
attaches explicit type contracts to them through conformance records.

## 2. Computation is typed movement

Transforms are not opaque pipeline boxes. They are typed morphisms from named
input ports to named output ports, running in versioned environments. The code
is the implementation of a typed movement through the graph.

## 3. The registry is the scientific memory

The important thing is not just that a project had an `ozzy.toml`. The
important thing is that a specific git commit published:

- specific `TypeVersion`s
- specific `EnvironmentVersion`s
- specific `TransformVersion`s
- a specific project revision bound to a registry revision

That published graph is the thing fetch runs against.

## 4. Artifacts replace ad hoc ontology

v4 does not split the world into “data atoms” and “collections” as separate
primitive classes. There are artifacts, and artifacts may be blobs, bundles, or
collections. The ontology should stay small and explicit.

## 5. Verification is executable, not rhetorical

“This artifact is a CSV” or “this output matches the declared table type” must
be backed by executable verification. Types are declarative. Verifiers are
implementations. Conformance is explicit: `declared`, `verified`, or `rejected`.

## 6. The published graph outranks authored convenience

`ozzy.toml` is authored declaration, not runtime truth. The runtime truth is the
published project revision plus the pinned registry snapshot. This is what keeps
push atomic and fetch reproducible.

## 7. Hashes are for identity, not vibes

Cache identity is based on the real execution inputs:

- typed input artifact identities
- transform version
- environment version
- source hash
- parameter hash
- secret provenance hash

Not “approximately the same run.” The actual run.

## 8. Provider realization is infrastructure, not ontology

GitHub, Docker, Fly, R2, local dev containers: these matter operationally, but
they are not the scientific object model. OzzyDB owns the binding between typed
computation and reproducible execution, not the whole substrate.

## 9. API first means human and agent parity

If a user can inspect an endpoint, bind input artifacts, declare conformance, or
fetch a result, the API must support it directly. The CLI and Python client sit
on top of that contract. The frontend is downstream of the API, not the other
way around.

## 10. No compatibility theater

Dead abstractions should be deleted. Fake provider generality should be cut.
Silent fallback paths should be removed. v4 exists to replace the old control
plane cleanly, not to carry it forever.

## 11. Error states are part of provenance

If verification fails, publication fails, or a runtime precondition is not met,
the system should say so explicitly. Errors are data. Silent degradation is a
form of dishonesty in a provenance system.

## 12. The right abstraction boundary is the boundary that survives publication

Many authored conveniences are useful locally. Few deserve to survive as
published identity. The registry should preserve what matters scientifically and
operationally:

- types
- environments
- transforms
- artifacts
- invocations
- conformance

Everything else is scaffolding.

---

## What is not the soul

- the exact CLI syntax
- Axum vs. another server framework
- Docker vs. another local runtime
- Fly vs. another remote backend
- the frontend stack
- whether a verifier is builtin Rust or environment-backed
- whether the next client is Python, R, or something else

If OzzyDB were rebuilt on another stack, the soul would still be:

- typed artifacts
- typed transforms
- explicit conformance
- published registry objects
- pinned project revisions
- reproducible fetch
