# Phase 2 Review Round 1 — 2026-02-14

**Models**: Claude Opus 4.6 (Gemini failed: E2BIG, Codex skipped: 368k tokens > 258k limit)
**Context**: ~368k tokens
**Scope**: Phase 2 — Async Job Model + Parallel DAG (Steps 2.1-2.7)

## Findings Fixed (commit e0b4379)

### Major
- **M1+M2**: Job output endpoint used wrong storage prefix ("materialized") and wrong hash lookup (output_hash vs materialized_hash). Fixed to use state.storage directly with content hash.
- **M4**: CLI polled for status "error" but server uses "failed" — infinite poll on failed jobs. Fixed.
- **M5**: Python client same issue. Fixed.

### Minor
- **N3→promoted**: Secrets hash divergence between fetch.rs (ozzy_core::hash::secrets_hash) and orchestrator.rs (manual format). Would cause cache misses. Fixed to use canonical function.
- **m1**: Non-deterministic wave ordering (HashMap iteration). Added sort().
- **m2**: CLI fetch had no poll timeout. Added --timeout flag (default 600s).
- **m7**: OZZY_PARAM_* env var keys unsanitized. Now stripped to [a-zA-Z0-9_].
- **N10**: Redundant blake3 hash computation in orchestrator. Now uses store_with_hash.
- **N4**: Python sys import inside loop. Moved to module level.

## Findings Deferred (by design)

### Minor
- **m3**: No concurrency limit on spawned jobs (tokio::spawn fire-and-forget). Defer to Phase 3 rate limiting.
- **m4**: PlatformFingerprint::detect() on server host. Design decision — will address with Fly backend.
- **m5**: TempDir lifetime with spawned tasks (safe by current wave-await logic).
- **m6**: compute_inputs always empty. Intentional scaffolding — input staging not yet wired.
- **m8**: 409 Conflict for in-progress job output (semantic, not functional).
- **m9**: NodeOutput constructor (minor maintainability).
- **M6**: Race condition in dedup check (low probability, can add advisory lock later).

### Notes
- **N1**: Orphaned job detection not implemented.
- **N2**: cleanup_expired_jobs exists but never called (no background task).
- **N5**: find_terminal_node picks arbitrary node when multiple exist (inherited from v2).
- **N6**: retrieve_source_code runs unconditionally even on cache hits.
- **N9**: update_job_status return value ignored by orchestrator.
