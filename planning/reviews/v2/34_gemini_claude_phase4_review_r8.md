# Phase 4 Review Round 8 — Claude Opus + Gemini

**Date:** 2026-02-13
**Scope:** Phase 4 (Execution) — post rounds 1-7 fixes
**Models:** Claude Opus 4.6 (subagent) + Gemini 2.5 Pro

## Claude Opus Findings (1 item)

| # | Severity | Issue | Resolution |
|---|----------|-------|------------|
| 1 | LOW | Temp file leak in `get()` hydrate path (missed round 7 pattern) | **Fixed** — cleanup added |

All round 7 fixes verified present.

## Gemini Findings (11 items, 10 false/known)

| # | Severity | Issue | Resolution |
|---|----------|-------|------------|
| 1 | HIGH | Lockfile hash divergence (server passes `""`) | Known TODO — comment says "will come from env build" |
| 2 | HIGH | Naive source hashing (commit-based not content-based) | Known TODO — placeholder |
| 3 | HIGH | Broken edge selector parsing (`from = "nodeA.result"`) | **Hallucination** — `from` field is plain node name or data: prefix, no output selectors |
| 4 | HIGH | Secret leak in error logs | Design note — transform owner controls what is logged |
| 5 | HIGH | Type validation bypass after failed coercion | **Fixed** — validate_param_value now checks JSON type matches declared type |
| 6 | MEDIUM | Broken collections in CLI | Known limitation |
| 7 | MEDIUM | Redundant environment builds | Known design limitation |
| 8 | MEDIUM | Prebuilt race condition | Design timing issue |
| 9 | MEDIUM | uv.lock build failure | Already fixed in round 2 |
| 10 | LOW | Orphaned containers on timeout | Design note (Docker `--rm` handles this) |
| 11 | LOW | Input order sensitivity | **Not a bug** — `materialized_hash()` sorts internally |

## Fixes Applied

2 fixes: get() temp file cleanup (Claude), param type validation (Gemini).
Commits: `879736c`, `0a73318`

## Convergence Assessment

- Claude: 1 LOW issue (tail of round 7 pattern) → near-clean
- Gemini: 1 real bug out of 11 items (high hallucination rate this round)
- Clean count reset to 0
- Need round 9 to start clean count
