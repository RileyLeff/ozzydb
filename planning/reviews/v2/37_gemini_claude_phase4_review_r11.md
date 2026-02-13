# Phase 4 Review Round 11 — Claude Opus + Gemini

**Date:** 2026-02-13
**Scope:** Phase 4 (Execution) — post rounds 1-10 fixes
**Models:** Claude Opus 4.6 (subagent) + Gemini 2.5 Pro

## Claude Opus Findings (1 item)

| # | Severity | Issue | Resolution |
|---|----------|-------|------------|
| 1 | MEDIUM | CLI Python runner `_write_item` missing return value — manifest records base path without extension (same bug fixed in server round 10, not applied to CLI copy) | **Fixed** — CLI `_write_item` returns actual path, manifest uses it |

## Gemini Findings (10 items, 8 known/not-applicable)

| # | Severity | Issue | Resolution |
|---|----------|-------|------------|
| 1 | HIGH | Collection manifest missing `content_type` | Known limitation (part of compute_inputs TODO) |
| 2 | HIGH | Compute stdout OOM | Production hardening, not correctness |
| 3 | HIGH | Case-sensitive username in push | Not Phase 4 scope (push.rs / Phase 3) |
| 4 | MEDIUM | Server doesn't prefer `result.*` files (diverges from CLI) | **Fixed** — `find_primary_output` helper added |
| 5 | MEDIUM | Idempotent push retry | Not Phase 4 scope |
| 6 | MEDIUM | Workspace leak on timeout | Known limitation (#8 in known list) |
| 7 | MEDIUM | Transform-level defaults ignored | Not a bug — by design (endpoints are the API surface) |
| 8 | LOW | Redundant output_bytes copy | Performance, not correctness |
| 9 | LOW | Image tag truncation divergence | Consistency, not correctness |
| 10 | LOW | Missing Content-Disposition header | UX improvement |

## Fixes Applied

2 fixes in 1 commit: `2f11d06`
- CLI Python runner manifest path mismatch (Claude)
- Server find_primary_output with result.* preference (Gemini)

## Convergence Assessment

- Claude: 1 MEDIUM bug (missed update to CLI copy of server-side fix)
- Gemini: 1 real LOW bug, rest known/not-applicable
- Clean count: 0 (reset by bugs)
- Pattern: bugs getting smaller — mostly parity issues between CLI and server copies
- Need round 12 for potential convergence
