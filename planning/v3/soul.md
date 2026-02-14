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

If you know `blake3(inputs + transform + params + platform)`, you know
*exactly* what you're getting. Not "the corrected version," not "what Sarah
sent me," not "I think this is the one from the paper." The hash. Immutable,
verifiable, unforgeable.

The platform fingerprint is part of the hash because floating-point arithmetic
is not portable — numpy on ARM and numpy on x86 can produce different results
at the least significant bits. Remote compute resolves this: everyone fetching
from the registry gets the same platform hash (same Fly Machine image), making
the guarantee unconditional. If you run locally, your platform is your own —
and your cache is separate, correctly.

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
numpy, scipy — whatever you already know. Your function takes `(inputs, params)`
and returns a result. It runs outside OzzyDB just fine — there's no decorator,
no import, no OzzyDB dependency in your code. `ozzy.toml` declares which
function to call; the function itself is pure, portable Python. OzzyDB just
remembers what you did.

## 5. The schema is a contract

Transform output schemas are declared in `ozzy.toml`, adjacent to the transform
definition. When the pipeline runs, the output is validated against the schema —
if your function returns the wrong columns or types, execution fails. The schema
isn't buried in a separate metadata layer that someone forgets to update; it's
in the same file that defines the pipeline. Change the transform, update the
schema, or validation catches the drift.

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

Collections strengthen this. The "VCR LTER" collection is a statement: "these
are the canonical datasets for this research site." When a new grad student
starts, they don't reinvent cleaning code. They use the published transforms,
get the exact same outputs, and build on top. Computation happens once.
Knowledge compounds.

## 8. Science is a DAG

Transforms chain. Endpoints take other endpoints' outputs as inputs. Collections
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

## 12. The switchboard, not the monolith

OzzyDB doesn't try to own everything. Git owns code. Container registries own
environments. Fly (or another compute provider) owns execution. R2 owns blob
storage. OzzyDB is the switchboard — it owns data, orchestration, and caching.
It knows which function to run on which data with which parameters, and it
remembers what happened. This separation of concerns is load-bearing: it means
OzzyDB doesn't need to be a git host, a container registry, or a compute
cluster. It connects them.

## 13. Data enters through the front door

There is no magic. Data must be explicitly uploaded to OzzyDB before transforms
can reference it. No local file paths smuggled into pipelines, no implicit
bind mounts, no "it works on my machine" escape hatches. The same data contract
applies whether you're talking to the cloud registry or a local dev stack —
upload, then reference. This is what makes the hash meaningful: every input
is content-addressed and tracked, from the moment it enters the system.

---

## What is NOT the soul

- Rust, Postgres, R2, Axum — implementation details
- Fly vs. Modal vs. Docker — compute provider choice
- gVisor vs. Firecracker — sandboxing decision
- The CLI syntax — UX decision
- Svelte vs. React — frontend choice
- Real-time streaming support — feature
- Multi-language runtimes — nice-to-have

If you had to rebuild OzzyDB on a completely different stack, you'd keep the
thirteen things above. Those are load-bearing. Everything else is scaffolding.
