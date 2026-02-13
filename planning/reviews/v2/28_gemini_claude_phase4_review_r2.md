# Phase 4 Review Round 2 — Claude Opus + Gemini

**Date:** 2026-02-13
**Scope:** Phase 4 (Execution) — post round 1 fixes
**Models:** Claude Opus 4.6 (subagent) + Gemini 2.5 Pro
**Context:** ~110k tokens focused on Phase 4 files + core dependencies

## Findings Summary

### Claude Opus (13 findings)
| # | Severity | Issue | Resolution |
|---|----------|-------|------------|
| 1 | CRITICAL | Empty compute_inputs (transforms get no data) | Known TODO — not a review bug |
| 2 | HIGH | BaseLockfile env hash divergence (CLI vs server) | **Fixed** — CLI uses raw content now |
| 3 | HIGH | PlatformFingerprint per-node in loop | **Fixed** — hoisted above loop |
| 4 | HIGH | Environment insert TOCTOU (no ON CONFLICT) | **Fixed** — added ON CONFLICT |
| 5 | HIGH | source_dir: None (source code never mounted) | Known TODO — not a review bug |
| 6 | MEDIUM | Prebuilt env hash divergence | **Fixed** — server uses blake3 now |
| 7 | MEDIUM | decrypt_secret no key length validation | **Fixed** — added guard |
| 8 | MEDIUM | Unrecognized params silently ignored | **Fixed** — error on unknown params |
| 9 | MEDIUM | CLI fetch doesn't cache locally | Deferred — nice-to-have |
| 10 | MEDIUM | assert!() in generate_dockerfile | **Fixed** (round 2 commit) — returns Result |
| 11 | LOW | Multi-output only returns first file | Known limitation |
| 12 | LOW | ContentStorage re-created per node | Minor perf, deferred |
| 13 | LOW | Fly init uses Python for downloads | Fly not implemented yet |

### Gemini (9 findings)
| # | Severity | Issue | Resolution |
|---|----------|-------|------------|
| 1 | CRITICAL | Empty compute_inputs | Same as Claude #1 — known TODO |
| 2 | CRITICAL | Missing source code | Same as Claude #5 — known TODO |
| 3 | HIGH | Invalid uv.lock handling | **Fixed** — removed broken handler |
| 4 | HIGH | Source hash inconsistency | Known design concern — separate cache domains |
| 5 | HIGH | Runner code injection via newlines | **Fixed** — validate_source_ref() |
| 6 | MEDIUM | Non-streaming I/O | Deferred — not blocking |
| 7 | MEDIUM | Placeholder endpoint resolution | Known TODO |
| 8 | MEDIUM | Username instability | Known design note |
| 9 | LOW | Advisory lock collisions / CLI memory | Minor |

## Fixes Applied

**Commit 1** (`bebd3f1`): 7 fixes — hash convergence, safety, performance
**Commit 2** (`11e30c9`): 2 fixes — runner injection validation, uv.lock removal

## Outstanding TODOs (Not Review Bugs)

These are known incomplete features marked with TODO in code:
- `compute_inputs: Vec::new()` — input resolution not implemented
- `source_dir: None` — source mounting not implemented
- `resolve_edge_source` endpoint variant — returns placeholder hash

## Next

Round 3 to verify fixes and check for remaining issues. Need 2 consecutive clean rounds.
