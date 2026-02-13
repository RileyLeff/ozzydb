# Phase 4 Review Round 5 — Claude Opus + Gemini

**Date:** 2026-02-13
**Scope:** Phase 4 (Execution) — post rounds 1-4 fixes
**Models:** Claude Opus 4.6 (subagent) + Gemini 2.5 Pro

## Claude Opus Findings (4 items)

| # | Severity | Issue | Resolution |
|---|----------|-------|------------|
| 1 | MEDIUM | Command function_name: CLI uses `"__command__"`, server uses `"command"` → transform hash divergence | **Fixed** — CLI changed to `"command"` |
| 2 | MEDIUM | Python runner list output manifest records wrong paths (no extension) | Known limitation — part of collection propagation TODO |
| 3 | MEDIUM | Workspace directory leaked on compute timeout/setup error | **Fixed** — cleanup guards added in docker.rs |
| 4 | LOW | CLI poetry lockfile detection uses `.contains()` (overly broad) | **Fixed** — changed to `== || .ends_with()` |

## Gemini Findings (5 items)

| # | Severity | Issue | Resolution |
|---|----------|-------|------------|
| 1 | HIGH | Compute workspace leak on error | **Fixed** — same as Claude #3 |
| 2 | HIGH | Query param type inference (strings not coerced to declared types) | **Fixed** — added `coerce_param_value()` |
| 3 | HIGH | Fragile source hashing (single file only) | Design limitation — not a bug |
| 4 | MEDIUM | Blocking IO in server async context (std::fs) | Known limitation, not critical |
| 5 | MEDIUM | Non-deterministic primary output selection in CLI | Known limitation |

## Fixes Applied

4 fixes: function_name mismatch, workspace leak, param type coercion, poetry lockfile.
Commit: `741649c`

## Convergence Assessment

- Round 5 found real bugs → NOT clean
- Multiple genuine issues remain across reviews (4 fixes applied)
- Need round 6 to start clean count
