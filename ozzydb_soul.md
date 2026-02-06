# The Soul of OzzyDB

**OzzyDB replaces files with functions, making data reproducible by construction.**

The instructions are the data. Everything else follows from that single inversion.

---

## 1. Kolmogorov's revenge

The shortest description of a derived dataset is usually the program that
produces it, not the output. A trillion-row unit conversion is described
perfectly by `celsius_to_kelvin(column)`. Storing the output is wasteful.
Storing the output *without* the function is a crime against reproducibility.
OzzyDB stores the function, materializes on demand, and caches the result.

## 2. The hash is the truth

If you know `blake3(inputs + transform + params + deps + platform)`, you know
*exactly* what you're getting. Not "the corrected version," not "what Sarah
sent me," not "I think this is the one from the paper." The hash. Immutable,
verifiable, unforgeable.

The platform fingerprint is part of the hash because floating-point arithmetic
is not portable — numpy on ARM and numpy on x86 can produce different results
at the least significant bits. Server-side compute resolves this: everyone
fetching from the registry gets the server's platform hash, making the guarantee
unconditional. If you run locally, your platform is your own.

## 3. Data is a function call, not a file

`data_final_v3_REAL.csv` is a lie. It's a frozen corpse pretending to be alive.
Real data has provenance, parameters, versions. It answers questions: "What if
I used a stricter QC threshold?" "What did this look like before the 2024
recalibration?"

OzzyDB endpoints are pure functions. `fetch("owner/project/endpoint@ref",
params={...})` — same inputs, same output, deterministic. They take parameters.
They have history. They're alive. The platform just happens to memoize them.

## 4. Write normal code

No DSL. No drag-and-drop GUI. No config language that's Turing-complete but
worse than every real language. You write a Python function. You use polars,
numpy, scipy — whatever you already know. The `@ozzy.transform` decorator is
a contract, not a cage. Your code runs outside OzzyDB just fine. OzzyDB just
remembers what you did.

## 5. The schema is the code

The `@ozzy.transform(input_schema=..., output_schema=...)` decorator means the
schema declaration lives in the same file as the logic. They can't drift apart.
In most data systems, schema lives in a metadata layer that someone forgets to
update. Here, the contract and the implementation are the same artifact. If you
change the function's output columns, you change the schema annotation or
validation fails.

## 6. Sunlight on the black box

When a paper says "we cleaned the data," that currently means nothing. It could
mean "we removed obvious sensor failures" or "we deleted the points that didn't
fit our hypothesis." OzzyDB makes cleaning *visible*. Every step is code. Every
parameter is recorded. Every intermediate result is inspectable. You can't hide
what you did because the what-you-did *is* the data.

## 7. Shared methodology, not just shared memoization

If Lab A in Berlin publishes their QC pipeline and Lab B in Tokyo uses it, Lab B
gets the exact same outputs — and a cache hit. But the cache hit is the least
interesting part. The real value is methodological convergence: Lab B is making
a verifiable statement that they used *exactly the same cleaning procedure*.

Curations strengthen this. The "VCR LTER" curation is a statement: "these are
the canonical pipelines and datasets for this research site." When a new grad
student starts, they don't reinvent cleaning code. They use the published
transforms, get the exact same outputs, and build on top. Computation happens
once. Knowledge compounds.

## 8. Science is a DAG

Transforms chain. Endpoints take other endpoints' outputs as inputs. Curations
reference any project. This is how science actually works — you take someone's
data, apply your methods, produce new data that someone else consumes. OzzyDB
makes that computation graph explicit and navigable. A data catalog stores
files. OzzyDB stores a computation graph that the entire community can extend.

## 9. Cite the exact thing

A DOI should point to exactly what was used — not "the dataset," but "the
dataset, cleaned this way, calibrated with these constants, at this commit."
When I read your paper in 2035, I should be able to run one command and get
*exactly* what you had. Not "approximately equivalent." Exactly.

## 10. Scientists shouldn't have to build infrastructure

Right now every lab, every LTER site, every research group builds their own
data hosting. That means every organization also builds its own broken version
of versioning, distribution, discovery, and access control. A scientist's job
is to produce and analyze data, not to become a part-time DevOps engineer
maintaining a data server. OzzyDB is the shared platform that eliminates that
duplicated effort — GitHub, not self-hosted Gitea.

## 11. Equally accessible to humans and LLMs

The platform makes no distinction between a human clicking "Download" and an
LLM calling `fetch()`. Same data, same auth, same guarantees. This constrains
the architecture: if it's not in the API, it can't be in the UI. If the UI can
do it, a script can do it. Error messages are structured and actionable — not
just for human eyeballs. The CLI and Python client are first-class interfaces,
not afterthoughts bolted onto a web app. This is a bet on how science will be
done: by humans, by agents, and by humans working with agents.

---

## What is NOT the soul

- Rust, Postgres, R2, Axum — implementation details
- gVisor vs. Firecracker vs. Docker — sandboxing decision
- The CLI syntax — UX decision
- Svelte vs. React — frontend choice
- Real-time streaming support — feature
- Multi-language runtimes — nice-to-have

If you had to rebuild OzzyDB on a completely different stack, you'd keep the
eleven things above. Those are load-bearing. Everything else is scaffolding.
