# Round 13 Code Review (2026-02-07)

## Reviewers
- Gemini 2.5 Pro (5 findings)
- Claude Opus 4.6 (5 findings)

## Findings (Deduplicated: 10 unique)

### Fixed

| # | Severity | Source | Finding | File | Fix |
|---|----------|--------|---------|------|-----|
| 1 | MEDIUM | Opus | Schema type conflicts pushed to warnings instead of errors (never shown to user) | endpoint.rs:333 | Changed to result.errors.push |
| 2 | MEDIUM | Opus | __pycache__ not pruned by WalkDir (binary .pyc causes read_to_string error) | canon.rs:39-48 | Use filter_entry to prune directories before descent |
| 3 | LOW | Opus | Missing as_u64() in JSON canonicalization (large u64 loses precision via f64) | canon.rs:131-133 | Added u64 branch between i64 and f64 |
| 4 | LOW | Opus | Python client glob vs rglob (misses nested transforms) | project.py:137 | Changed to rglob + relative path |

### Skipped (by design or low impact)

| # | Severity | Source | Finding | Reason |
|---|----------|--------|---------|--------|
| 5 | MEDIUM | Opus | dict<> comma splitting in parse_data_type | Parquet dict keys are never nested types in practice |
| 6 | MEDIUM | Gemini | parse_simple_dict comma in strings | params= values are type names, not strings with commas |
| 7 | HIGH | Gemini | pull handler OOM (in-memory tar) | Architecture issue; MAX_TAR_SIZE_BYTES limit exists |
| 8 | MEDIUM | Gemini | get_stream reads entire file to memory | Performance, not correctness |

### False Positives

| # | Source | Finding | Reason |
|---|--------|---------|--------|
| 9 | Gemini | Corrupted justfile | Already fixed in prior round |
| 10 | Gemini | @latest with leading space | Code already shows "@latest" (no space) |
