# OzzyDB Architecture Review (Codex)

Date: February 5, 2026

**Summary**
OzzyDB’s core idea is strong and timely: version transforms instead of materialized data, enforce content addressing, and make derived datasets reproducible and shareable. The local-first strategy is a major differentiator and lowers adoption risk. The architecture is coherent and the design decisions (Arrow/Parquet, BLAKE3, optional server compute, gVisor isolation) are defensible. The biggest gaps are around determinism contracts, platform fingerprinting, local commit/ref mechanics, and how cross-project dependencies are pinned for reproducibility. These are solvable with a few concrete spec additions and some schema/UX tightening.

**What’s Strong**
- Clear vision anchored in scientific workflows and reproducibility pain.
- Content-addressed DAG is a clean mental model and scales from local to server.
- Local-first rollout avoids infra overreach and can prove value quickly.
- Technology choices are pragmatic and aligned with low-latency data movement.
- Explicit platform hashing acknowledges real cross-architecture numerical drift.

**Primary Risks / Gaps**
- Determinism is underspecified. A transform can be “pure” but still nondeterministic (random seeds, multithreaded BLAS, time, locale, nondeterministic parallel reductions).
- “Platform” is not fully defined. CPU features, OS, libc, BLAS implementation, and container base image can all affect results.
- Local commit/ref model is unclear. A single `ozzy.toml` file doesn’t capture a commit graph or refs without extra structure.
- Cross-project dependencies are not pinned. `external_project` without a commit/ref or content hash breaks reproducibility.
- DAG node references use string names without enforced referential integrity at the DB level.
- Cache and access control interact in subtle ways. Content-deduped caches across private projects can leak existence without strict ACL gating.

**Idea Review: Refinements That Strengthen the Core**
- Define a formal determinism contract. Provide a default runtime policy that sets fixed seeds, disables nondeterministic parallelism, and forbids time/network access. Mark transforms that violate the contract as `reproducible=false` and treat them as second-class in releases/DOIs.
- Canonicalize hashes. Specify canonical JSON encoding for params, a canonical line ending and file ordering for source hashing, and a canonical Parquet write config for raw inputs if you want dedupe at ingest.
- Make platform a structured fingerprint. Include OS, arch, libc/GLIBC version, CPU feature flags, and numeric libraries (BLAS, MKL, OpenBLAS) in the fingerprint that flows into the hash.
- Pin external dependencies with full refs. Any cross-project edge should encode `project_ref` as `{owner}/{project}@{commit|tag}` and resolve to a content hash.

**Implementation Strategy Review**
- Phase 1 is well-scoped but could be narrower for speed. Consider limiting initial support to Python only, a single DAG form (one or two input nodes), and no releases. Validate determinism and caching first.
- Phase 2 registry-only server is the right next step. Add an explicit `refs` table for `@latest`, `@vX.Y.Z`, and branch-like names. This makes pull/push semantics crisp.
- Phase 3 schema validation is important and should likely move earlier. Without strict schema validation, caches will be polluted by mismatched assumptions.
- Phase 4 compute should enforce deterministic execution by default and only allow nondeterministic transforms when explicitly flagged.

**Data Model Review**
- Commits should allow merges. Add `parent_hashes` array or a join table for multiple parents.
- Enforce transform resolution at the DB level. `pipeline_nodes.transform_name` should reference `transforms` for the same commit. This likely needs a trigger or composite FK.
- `pipeline_edges.source_ref` should be constrained by `source_type`. Consider replacing with `source_node_id` and `source_data_id` to make integrity enforcement easier.
- `external_project` should be split into `external_owner`, `external_project`, `external_ref`, and `external_commit_hash` to guarantee pinned dependencies.
- Add `refs` table for endpoints and branches, not just releases. `@latest` must map to a specific commit.
- Consider storing `input_schema_hash` and `output_schema_hash` to avoid giant JSON comparisons.

**Execution and Determinism**
- Define a formal transform signature and a cross-language interface. For example: `transform(inputs: {name: ArrowRecordBatch}, params: dict) -> ArrowRecordBatch` and require deterministic ordering of input batches.
- Use streaming execution in early phases. Arrow IPC streaming + Parquet write streaming will help avoid full materialization in memory and position you for large datasets.
- Enforce reproducibility at runtime. Example defaults: fixed `PYTHONHASHSEED`, `OMP_NUM_THREADS=1`, `MKL_NUM_THREADS=1`, `numpy.random.seed(0)` if not provided.
- Consider a “determinism report” in `ozzy transform test` that captures non-deterministic indicators.

**Security and Multi-Tenancy**
- gVisor is reasonable for early phases, but set explicit CPU/mem/time quotas and no network access by default.
- Caches shared across projects should be gated by ACL at retrieval time even if the underlying blob is shared. Avoid direct access by hash without permission checks.
- If R2 is used, ensure object keys do not reveal private project names or endpoint names when public listings are possible.

**API and UX**
- Define explicit “resolve” semantics: `GET /resolve/{owner}/{project}/{endpoint}@{ref}` should return commit hash and DAG metadata.
- Add a lightweight `ozzy explain` or `ozzy lineage` that outputs a reproducibility report (hashes, platform fingerprint, lockfile hash).
- Ensure `ozzy run` and `ozzy fetch` can produce a deterministic materialized hash and a human-readable provenance summary.

**Suggested Decisions to Set Early**
- The exact platform fingerprint and how it is computed.
- Canonical hashing rules for code, params, and schema JSON.
- Transform interface and how to express multi-input joins.
- Ref model for `@latest`, branches, and tags in local-first mode.
- Policy for nondeterministic transforms in releases and DOI minting.

**Concrete Next Steps**
1. Write a short spec for hashing and canonicalization, including platform fingerprint.
2. Define the local commit/ref model and how it is stored on disk.
3. Implement a minimal Python-only engine with deterministic execution defaults and streaming Arrow.
4. Add schema validation to Phase 1 or early Phase 2 to prevent silent corruption.
5. Add a `refs` table and resolve endpoint references through it.

If you want, I can turn these into specific changes in the spec and a tighter Phase 1 plan aligned with a prototype milestone.
