# Phase 2 Exhaustive Review — Converged

**Rounds**: 4 (Round 1: 8 fixes, Round 2: 1 fix, Rounds 3-4: CLEAN)
**Models**: Claude Opus 4.6 only (Gemini: E2BIG at 368k tokens, Codex: skipped at 368k > 258k limit)
**Context**: ~368k tokens

## Round 1 Findings (8 fixes, commit e0b4379)

### Major
- **M1+M2**: Job output endpoint used wrong storage prefix ("materialized") and wrong hash lookup (output_hash vs materialized_hash). Fixed to use state.storage directly.
- **M4+M5**: CLI and Python client polled for status "error" but server uses "failed". Fixed.

### Minor
- **N3**: Secrets hash divergence between fetch.rs and orchestrator.rs. Fixed.
- **m1**: Non-deterministic wave ordering. Added sort().
- **m2**: CLI fetch had no poll timeout. Added --timeout flag.
- **m7**: OZZY_PARAM_* env var keys unsanitized. Stripped to [a-zA-Z0-9_].
- **N10**: Redundant blake3 hash computation. Now uses store_with_hash.
- **N4**: Python sys import inside loop. Moved to module level.

## Round 2 Findings (1 fix, commit 19e0ca3)

### Minor
- Orchestrator silently skipped missing secrets instead of erroring (divergence from fetch.rs). Fixed.

## Deferred Items (by design)
- compute_inputs always empty (intentional scaffolding)
- No concurrency limit on spawned jobs (Phase 3)
- PlatformFingerprint on server host (design decision)
- TempDir lifetime (safe by wave logic)
- 409 for in-progress job output (semantic)
- find_terminal_node arbitrary when multiple exist (inherited)
- retrieve_source_code on cache hits (optimization)
- Orphaned job detection, cleanup_expired_jobs (infrastructure)
- update_job_status return value, NodeOutput constructor (minor)
- Race condition in dedup check (low probability)

## Rounds 3-4: CLEAN

No new issues found. Review converged with 2 consecutive clean rounds.
