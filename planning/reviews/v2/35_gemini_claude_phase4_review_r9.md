# Phase 4 Review Round 9 — Claude Opus + Gemini

**Date:** 2026-02-13
**Scope:** Phase 4 (Execution) — post rounds 1-8 fixes
**Models:** Claude Opus 4.6 (subagent) + Gemini 2.5 Pro

## Claude Opus Findings (1 item)

| # | Severity | Issue | Resolution |
|---|----------|-------|------------|
| 1 | HIGH | CLI uses `mat_hash` (cache key) as input hash for downstream nodes; server uses `output_hash` (blake3 of content) — divergence in multi-node DAGs | **Fixed** — added `output_hash` field to `NodeOutput`, compute via `blake3_hash_file` |

## Gemini Findings (7 items, 3 known)

| # | Severity | Issue | Resolution |
|---|----------|-------|------------|
| 1 | HIGH | Script injection in `file_path` portion of source ref — Python `from {module}` and R `source()` templates vulnerable | **Fixed** — char whitelist `[a-zA-Z0-9/_.-]` on file_path in `validate_source_ref` |
| 2 | HIGH | CLI `parse_param_value` infers types without consulting declared type — hash divergence vs server's `coerce_param_value` | **Fixed** — CLI now uses `coerce_param_value` matching server logic exactly |
| 3 | MEDIUM | R runner lacks collection (list) output support | Known limitation — R runner is minimal |
| 4 | MEDIUM | Server fetch returns only first item of collection | Known limitation — collections not fully supported |
| 5 | MEDIUM | Default param values bypass `validate_param_value` | **Fixed** — defaults now validated against min/max/enum constraints |
| 6 | LOW | Poetry lockfile filename issue in env builder | Known limitation — poetry support is best-effort |
| 7 | LOW | Inconsistent boolean coercion (CLI: true/false only; server: true/1/yes etc.) | **Fixed** — subsumed by fix #2 (unified coerce_param_value) |

## Fixes Applied

5 fixes total across 2 commits:
- `8063cff` — CLI output_hash for downstream node inputs (Claude)
- `801794d` — source ref injection, param coercion alignment, default validation (Gemini)

## Convergence Assessment

- Claude: 1 HIGH bug → clean count reset
- Gemini: 4 real bugs out of 7 items (decent hit rate this round)
- Clean count: 0
- Need 2 consecutive clean rounds for convergence
