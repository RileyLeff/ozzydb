# OzzyDB Code Review (Round 7)

Date: February 7, 2026

Scope: Full codebase review via dirgrab + codex exec (gpt-5.3-codex, xhigh reasoning). Only NEW findings not covered in rounds 1-6.

## Findings (Ordered by Severity)

1. **High: Multi-input execution order is non-deterministic while cache key assumes deterministic ordering**
Description: Multi-input hashes are computed from inputs sorted by input name, but runtime input loading uses `HashMap` iteration order when building the Python `inputs` dict.
Evidence: `crates/ozzy-core/src/hash.rs:81`, `crates/ozzy-core/src/runtime.rs:278`, `crates/ozzy-core/src/runtime.rs:383`, `crates/ozzy-core/src/runtime.rs:682`
Impact: Two executions can share the same materialized hash while producing different outputs for transforms that iterate over `inputs` order, violating reproducibility and cache correctness.
Suggested fix: Sort inputs by key before generating Python loading code (or use `BTreeMap` end-to-end), and add a regression test with an order-sensitive multi-input transform.

2. **High: `ozzy pull` can silently overwrite/delete uncommitted local work**
Description: Pull writes incoming files directly and prunes non-listed local files without any clean-worktree preflight.
Evidence: `crates/ozzy-cli/src/commands/pull.rs:167`, `crates/ozzy-cli/src/commands/pull.rs:250`, `crates/ozzy-cli/src/commands/pull.rs:262`
Impact: Local edits in `data/` and `transforms/` can be lost permanently on pull.
Suggested fix: Refuse pull on dirty state by default (require `--force` to discard), or auto-backup/restore local changes similarly to staged-endpoint backup behavior.

3. **Medium: `pull` advances refs even if `commit.json` is missing or inconsistent**
Description: `commit.json` is treated as optional and never hash-validated against the manifest before updating refs.
Evidence: `crates/ozzy-cli/src/commands/pull.rs:229`, `crates/ozzy-cli/src/commands/pull.rs:274`, `crates/ozzy-cli/src/commands/pull.rs:285`, `crates/ozzy-core/src/project.rs:592`
Impact: Local refs can point to a commit file that does not exist (or mismatched commit content), breaking `log/status/latest_commit` and corrupting provenance expectations.
Suggested fix: Require `commit.json` to be present, parse it, verify `commit.hash == manifest.commit_hash`, and only then update refs.

4. **Low: Non-reproducible runs leak persistent `nocache_*.parquet` files**
Description: Non-cache execution writes timestamped outputs into the global cache directory and does not clean them up.
Evidence: `crates/ozzy-cli/src/commands/run.rs:478`, `crates/ozzy-cli/src/commands/fetch.rs:606`
Impact: Repeated runs with non-reproducible transforms can accumulate unbounded files and consume disk space over time.
Suggested fix: Track and delete intermediate `nocache_*` artifacts after completion (keep only explicit user output/final artifact), or move them to a managed temp dir with cleanup.

## Summary

4 new findings. Decreasing severity trend — no critical issues, security surface addressed in Round 6. Remaining issues are data safety (pull overwrite) and correctness (input ordering, ref integrity).
