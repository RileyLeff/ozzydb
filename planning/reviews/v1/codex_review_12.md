# Round 12 Code Review (2026-02-07)

## Reviewers
- Gemini 2.5 Pro (3 findings)
- Claude Opus 4.6 (6 findings)

## Findings (Deduplicated: 9 unique)

### Fixed

| # | Severity | Source | Finding | File | Fix |
|---|----------|--------|---------|------|-----|
| 1 | MEDIUM | Opus | revoke_token missing validate_path_segment | client.rs:190 | Added validate_path_segment call |
| 2 | MEDIUM | Opus | pull_manifest/pull raw ref in query string (injection risk) | client.rs:425,458 | Added validate_path_segment for ref names |
| 3 | MEDIUM | Opus | TOML newline escaping insufficient in fetch.rs | fetch.rs:206-207 | Added \n, \r, \t escaping |

### Skipped (by design or low impact)

| # | Severity | Source | Finding | Reason |
|---|----------|--------|---------|--------|
| 4 | LOW | Opus | ref_count never decremented in content_refs | No GC yet; by design for now |
| 5 | LOW | Opus | Tags pushed outside transaction (best-effort) | R10 design decision |
| 6 | MEDIUM | Opus | check_content returns names not hashes (naming) | Consistent behavior; naming issue only |

### False Positives

| # | Source | Finding | Reason |
|---|--------|---------|--------|
| 7 | Gemini | @ozzy.transform without parens not handled | Already handled: `(!saw_open_paren && j == i)` breaks immediately on no-paren decorators |
| 8 | Gemini | justfile shell-specific commands | `test` and `set` are POSIX builtins, not bash-specific |
| 9 | Gemini | localhost healthcheck in docker-compose | Gemini itself says no change needed |
