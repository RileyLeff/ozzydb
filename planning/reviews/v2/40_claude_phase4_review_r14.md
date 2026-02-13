# Phase 4 Review Round 14 — Claude Opus (Gemini failed)

**Date:** 2026-02-13
**Model:** Claude Opus 4.6 via Codex (Gemini 2.5 Pro failed — exit code 13, even at 150k tokens)
**Commit before:** `98e23ce`
**Commit after:** `42848ac`
**Tests:** 90 core + 108 server = 198 pass

## Findings (5 total, 4 real bugs fixed, 1 design note)

### Fixed

1. **HIGH — Poetry lockfile environments are unbuildable** (`environments/mod.rs`, `run.rs`)
   - `generate_dockerfile()` copied lockfile as `/tmp/lockfile` and ran `poetry install` in `/tmp`
   - Poetry requires `pyproject.toml` + properly named `poetry.lock`, neither available in BaseLockfile tier
   - Fix: Reject `poetry.lock` with explicit error and instructions to export to `requirements.txt`

2. **HIGH — Push can succeed while producing non-executable commits** (`push.rs`)
   - `cache_source_tarball` failures were logged but not fatal
   - Commits registered without source tarballs fail at fetch time for source-based transforms
   - Fix: Fail push when source cache fails and any transforms use `source` (not `command`)

3. **MEDIUM — Prebuilt environments can return "not built yet"** (`fetch.rs`)
   - `resolve_environment_image` required a DB row even for prebuilt envs
   - Async `build_environments_async` might not have inserted the row yet
   - Fix: Synthesize the record directly from the env definition for prebuilt tier

4. **LOW — Dashed param names produce unusable shell env vars** (`toml_spec.rs`)
   - `OZZY_PARAM_my-param` is unusable in shell (`$OZZY_PARAM_my` + `-param`)
   - Fix: Reject hyphens in both transform and endpoint param names, with rename suggestion

### Not fixed (design note)

5. **MEDIUM — TOCTOU race allows yanked members into new collection versions** (`collections.rs`)
   - Member resolution/yank checks happen before advisory lock acquisition
   - The advisory lock serializes collection mutations but doesn't re-check member yank status
   - Impact is minimal (extremely rare concurrent yank + add scenario, no data loss)
   - Added as known limitation #37

## Gemini Status

Gemini 2.5 Pro continues to fail with exit code 13 even at 150k tokens (Rust source only, no tests/SQL/frontend/Python). This is a persistent issue with the Gemini CLI for this codebase size. Proceeding with Claude-only rounds.

## Known Limitations Updated (now 37)

Added:
- #37: TOCTOU race allows yanked members into new collection versions (advisory lock doesn't re-check member yank status)
