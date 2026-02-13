# Phase 4 Review Round 18 — Claude Opus via Codex

**Date:** 2026-02-13
**Model:** Claude Opus 4.6 via Codex
**Commit before:** `72a0e72`
**Commit after:** `ed2bd7d`
**Tests:** 90 core + 110 server + 4 CLI unit = 204 pass

## Findings (5 total, 2 real bugs fixed, 3 design notes)

### Fixed

1. **HIGH — Docker timeout doesn't kill container** (`compute/docker.rs`)
   - `tokio::time::timeout` around `cmd.output()` drops the future on timeout
     but doesn't kill the child process. Docker container continues running.
   - Fix: Use `cmd.spawn()` + `child.wait_with_output()`, assign container
     a `--name ozzydb-{id}`, and use `docker kill` on timeout.

2. **MEDIUM — Branched DAGs return arbitrary sink node** (`fetch.rs`, `run.rs`)
   - Both server and CLI used `execution_order.last()` as the endpoint output
     node. For DAGs with multiple independent sinks, this is order-dependent.
   - Fix: Added `find_terminal_node()` that identifies nodes with zero
     outgoing edges to other nodes. Warns if multiple terminals exist,
     deterministically picks lexicographically first.

### Design notes (known limitations)

3. **HIGH claimed — Large output/logs OOM** — Design note. Outputs are bounded
   by Docker memory limit. Single-tenant deployment limits risk. Streaming
   would require significant refactor. Added as known limitation #41.

4. **MEDIUM — Fetch resolves env_hash from GitHub on every request** — Design
   note. Computing env_hash requires lockfile content which is fetched from
   GitHub even for cache-hit paths. Storing env_hash in commit state during
   push would avoid this. Added as known limitation #42.

5. **LOW — Static node params not validated against transform schema** — Design
   note. Would require schema cross-referencing at validation time, which the
   current validator doesn't do. Added as known limitation #43.

## Known Limitations Updated (43 total)

#41: Large transform outputs/logs fully buffered in server memory
#42: Fetch re-fetches lockfile from GitHub to compute env_hash (blocks cached results on GitHub availability)
#43: Static node params not validated against transform's declared param schema
