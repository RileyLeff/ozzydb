# Phase 5 Review Round 1 — Codex

**Date:** 2026-02-13
**Model:** Codex (gpt-5.3-codex)
**Session:** 019c57d2-cdbd-7fc1-84d1-f7baee5cbb62
**Scope:** Phase 5 frontend implementation (all 8 steps)

## Findings

### Fixed

1. **HIGH** ProjectTabs hrefs stale after param change
   - `tabs` was `const` (computed once), not reactive to `owner`/`project` prop changes
   - Fixed: changed to `$derived([...])`

2. **MEDIUM** Retry button on project overview doesn't re-trigger fetch
   - Effect depends on `owner`/`slug` params; setting `loading=true` doesn't re-run it
   - Fixed: added `retryCount` state, effect reads it, button increments it

3. **MEDIUM** Collection member_type mismatch
   - Frontend checked `member.member_type === 'atom'`, server returns `'data'`
   - Fixed: changed to `'data'`

4. **MEDIUM** Commits API returns UUID for pushed_by, frontend shows as username
   - Server `pushed_by: c.pushed_by.to_string()` produces UUID
   - Fixed: server now resolves UUID to username via `get_user_by_id`

5. **MEDIUM** Commits API negative limit
   - `query.limit.min(100)` had no lower bound
   - Fixed: added `.max(1)` clamp

6. **LOW** Data download drops file extension
   - `a.download = name` ignores server Content-Disposition
   - Fixed: parse filename from Content-Disposition header, fallback to name

### Not Fixed (false positive)

7. **LOW** Endpoint param type aliases — Codex claimed server passes `boolean`/`integer`/`number`, but TOML parser validates only `float`/`int`/`string`/`bool`. Frontend checks are correct.

## Status: 5 fixes applied, 1 false positive dismissed
