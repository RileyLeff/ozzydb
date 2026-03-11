## Phase 5 External Review

Scope:
- `a7e651b` Bind execution to published transform versions
- `a445524` Record v4 invocations and output artifacts
- `911b89f` Rewrite fetch around typed endpoint inputs
- `91568b0` Rewrite v4 cache identity around typed artifacts
- `36cf304` Keep compute providers internal

Reviewers:
- Gemini CLI
- Claude CLI (`--setting-sources project,local`, strict empty MCP config)

Both reviews were run against a focused packet containing the current Phase 5
runtime files plus v4 architecture/plan excerpts.

## Findings

### High

1. Network access policy is not enforced at the compute boundary.
   - `crates/ozzy-server/src/compute/types.rs`
   - `crates/ozzy-server/src/compute/orchestrator.rs`
   - `crates/ozzy-server/src/compute/fly.rs`
   - `crates/ozzy-server/src/compute/mod.rs`
   - `transform.network` is published and carried into runtime bindings, but
     `ComputeRequest` has no network policy field and no backend-enforced
     isolation toggle. A transform declared with `network = false` still runs
     with whatever network access the selected backend normally provides.
   - Source:
     - Claude

### Medium

2. Output conformance is declared but not verified in the execution path.
   - `crates/ozzy-server/src/db/v4/queries.rs`
   - `crates/ozzy-server/src/api/v1/fetch.rs`
   - Successful execution persists output conformance as `declared`, and input
     resolution only rejects `rejected` conformance. There is no Phase 5
     verification step yet, so typed execution currently trusts declared
     outputs.
   - Source:
     - Claude
     - Gemini

3. Multiple terminal nodes are handled with a warning plus alphabetical
   selection instead of an error.
   - `crates/ozzy-server/src/api/v1/fetch.rs`
   - This violates the repo rule against silent fallback in semantic code and
     can change endpoint output arbitrarily if a DAG grows an extra terminal.
   - Source:
     - Claude

4. Local `endpoint:` edge sources are accepted by authored validation but still
   fail at runtime as unimplemented.
   - `crates/ozzy-core/src/toml_spec.rs`
   - `crates/ozzy-server/src/api/v1/fetch.rs`
   - Cross-project endpoint dependencies with `endpoint:...` are not yet
     implemented at runtime, but authored validation does not reject the local
     form early.
   - Source:
     - Claude

5. Materialized cache hits trust stored `output_artifact_id` without checking
   that the artifact row still exists.
   - `crates/ozzy-server/src/api/v1/fetch.rs`
   - `crates/ozzy-server/src/compute/orchestrator.rs`
   - `crates/ozzy-server/src/db/queries.rs`
   - If the cache row points at a missing artifact, the hit is accepted and
     the bad artifact identity propagates downstream.
   - Source:
     - Claude

### Low

6. `infer_blob_loader_from_expr(...)` still has a catch-all `Bytes` default for
   non-composite, non-encoding-specific types.
   - `crates/ozzy-server/src/compute/orchestrator.rs`
   - This may be acceptable for generic blob types, but it is still a fallback
   shape in semantic code and should be revisited if scalar/non-encoding input
   types become more common.
   - Source:
     - Gemini

7. `coerce_param_value(...)` falls back to the original string on parse
   failure.
   - `crates/ozzy-server/src/api/v1/fetch.rs`
   - This is later caught by validation, so it is not a correctness bug today,
     but it is a policy mismatch with the project’s "no silent fallback" rule.
   - Source:
     - Claude

## Rejected / Non-Blocking Notes

1. Cache identity based on artifact ID instead of content hash is not a bug for
   v4.
   - The Phase 5 architecture explicitly says cache identity should use typed
     input artifact identities. Gemini flagged this as a reproducibility risk,
     but it matches the current design.

2. Source hash recomputation from extracted tarballs is not currently treated
   as a bug.
   - It is an efficiency/design note, not a concrete correctness issue in the
     reviewed implementation.

3. The secret-hash query-path mismatch is not currently a concrete bug.
   - `get_secret_info(...)` and `get_secret(...)` both read from the same
     `secrets` table and both carry `version_id`. This is worth keeping aligned,
     but it is not currently evidence of divergent behavior.

## Summary

The Phase 5 rewrite is directionally sound. The real remaining issues are:
- network policy not being enforced
- output conformance still being trust-based
- one remaining terminal-node fallback
- one validation/runtime mismatch for `endpoint:` edges
- cache hits trusting stale artifact IDs

No external reviewer found evidence that the old anonymous `data:` /
`collection:` ingress or public compute-provider selection leaked back into the
runtime path.
