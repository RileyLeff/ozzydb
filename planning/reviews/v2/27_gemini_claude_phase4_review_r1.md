# Phase 4 Review Round 1 — Gemini + Claude Opus

**Date:** 2026-02-12
**Scope:** Phase 4 (Execution) — environments, runners, compute, server fetch, CLI run/fetch, cache
**Models:** Gemini 2.5 Pro + Claude Opus 4.6 (parallel subagent review)
**Context:** ~91k tokens focused on Phase 4 files + dependencies

## Findings (Deduplicated)

### Critical / High

1. **copy_dir follows symlinks** (both reviewers) → Fixed: skip symlinks with warning (server + CLI)
2. **WorkspaceCleanup Drop race** (both reviewers) → Fixed: replaced RAII with explicit `cleanup()` on ComputeResult
3. **detect_runner_type defaults to Python** (both reviewers) → Fixed: returns `Option<RunnerType>`, callers handle None
4. **as_millis() as i32 overflow** (Claude) → Fixed: `try_into().unwrap_or(i32::MAX)`
5. **Dockerfile base_image injection** (Claude) → Fixed: assert no newlines in base_image
6. **CLI R runner template minimal** (Claude) → Fixed: expanded to match server's full R template

### Medium

7. **CLI fetch requires auth** (Claude) → Fixed: optional auth, default registry fallback
8. **Docker timeout doesn't kill container** (Claude) → Noted: needs container name tracking, deferred
9. **Python/R runner code injection via source file path** (Claude) → Noted: source files come from committed git content, not user input
10. **Docker -e flag env var expansion** (Claude) → Noted: env vars are set by server, not user-controlled
11. **Primary output detection brittle** (Gemini) → Noted: works for current simple case, improve when multi-output needed
12. **Silent env build failures** (Gemini) → Noted: env builds logged but don't block push (by design)

### Design Concerns (Not Bugs)

13. **Hash divergence CLI vs server (5 dimensions)** (Gemini) → Analyzed: separate cache domains, not a correctness issue. CLI and server compute different hashes intentionally (different env hashing, platform context).
14. **Empty compute_inputs / source_dir TODO** (Gemini) → Known: server fetch is still incomplete, marked as TODO in code
15. **Env hash divergence (lockfile hash vs raw content)** (Claude) → Same as #13: CLI and server hash differently, separate cache domains

## Fixes Applied

11 fixes across 8 files. Commit: `3e25437`

## Next

Review round 2 — need 2 consecutive clean rounds for convergence.
