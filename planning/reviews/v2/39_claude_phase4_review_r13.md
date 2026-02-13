# Phase 4 Review Round 13 — Claude Opus (Gemini failed)

**Date:** 2026-02-12
**Model:** Claude Opus 4.6 (Gemini 2.5 Pro failed — exit code 13, context too large despite aggressive exclusions)
**Commit before:** `d5784a1`
**Commit after:** `642afa2`
**Tests:** 90 core + 108 server + 44 CLI unit = 242 pass

## Findings (8 total, 4 real bugs fixed)

### Fixed

1. **HIGH — uv.lock not pip-installable** (`environments/mod.rs`, `run.rs`)
   - `generate_dockerfile()` ran `pip install -r` on uv.lock (TOML format, not pip-compatible)
   - Fix: Return explicit error with instructions to export via `uv export --no-hashes > requirements.txt`
   - Applied to both server and CLI

2. **MEDIUM — CLI doesn't validate param constraints** (`run.rs`)
   - Server validated min/max/enum on params; CLI skipped entirely
   - Fix: Added `validate_param_value()` matching server-side logic (type, min/max, enum checks)

3. **MEDIUM — Secret names can override OZZY_* env vars** (`fetch.rs`)
   - Secrets injected after OZZY_PARAMS etc. could shadow them
   - Fix: Reject secrets with `OZZY_` prefix, return 400

4. **LOW — Endpoint params named `ref` or `format` collide with query params** (`toml_spec.rs`)
   - serde `#[serde(rename = "ref")]` and `format` consume values before `#[serde(flatten)]`
   - Fix: Reject `ref` and `format` as endpoint param names at TOML validation time

### Not fixed (known limitations / by design)

5. **HIGH — Dockerfile-tier builds have incomplete build context** — Known limitation (Dockerfile build context is a design issue for future work)
6. **HIGH — Dockerfile cache key ignores build-context files** — Same family as #5
7. **HIGH — Collection outputs truncated to single item** — Known limitation (#9/#11 in known list)
8. **MEDIUM — Cache invalidation incomplete for imported modules** — By design (source_hash covers directory, not import-level tracking)

## Gemini Status

Gemini 2.5 Pro failed with exit code 13 (context too large) on three separate attempts:
1. Full context (~210k tokens) — exit 13
2. Excluded planning/, frontend/, clients/, docker/ — exit 13
3. Excluded tests, SQL migrations, queries.rs additionally — still exit 13

Proceeding with Claude-only for this round. Gemini may need even more aggressive context reduction or manual file selection for future rounds.

## Known Limitations Updated (now 36)

Added to known list:
- #33: Dockerfile-tier build context incomplete (only lockfile, not COPY targets)
- #34: Dockerfile cache key ignores build-context files
- #35: Collection nodes output single item (no aggregation)
- #36: Import-level cache invalidation not tracked (source_hash covers directory)
