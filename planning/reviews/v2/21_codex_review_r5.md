# Phase 3 Review Round 5 (Codex)

## Reviewer
- Codex (primary review)

## Findings

### Fixed
1. **LOW (Codex): Idempotent push hardcodes source_cached: true** (push.rs:179)
   - Duplicate-push path returned `source_cached: true` even if initial caching failed
   - Fixed: Check source_cache table via `get_source_cache()` for actual status

### Dismissed
- Codex HIGH: Collection flatten loads latest child version — By design (collections are live reference groups, flatten is intentionally dynamic; same family as previously dismissed items)
- Codex MEDIUM: TOCTOU yanked member in add_members — Phase 2 code, minimal impact (flatten already skips yanked members at read time), advisory lock serializes within-project mutations
