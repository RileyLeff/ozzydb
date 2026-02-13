# Phase 5 Review Round 2 — Claude

**Date:** 2026-02-13
**Model:** Claude Opus (subagent)
**Scope:** Phase 5 frontend + server type alignment

## Findings

### Fixed

1. **HIGH** DataAtomDetail.uploaded_by returns UUID instead of username
   - Server `data.rs` had `uploaded_by: Uuid`
   - Fixed: resolve to username via `get_user_by_id`

2. **HIGH** MetadataEntryResponse.set_by returns UUID instead of username
   - Server `data.rs` had `set_by: Uuid`
   - Fixed: resolve to username in both describe and get_metadata handlers

3. **HIGH** VersionDetail.created_by / VersionLogEntry.created_by return UUID
   - Server `collections.rs` had `created_by: Uuid` in both structs
   - Fixed: added `resolve_username()` helper, used in all 4 construction sites

4. **MEDIUM** fetchEndpoint `ref` param can be overwritten by user params named "ref"
   - Fixed: moved ref set after params so it takes priority

5. **MEDIUM** Endpoint detail Object URL memory leak on repeated runs
   - Fixed: `URL.revokeObjectURL` before overwriting `execResult`

### Dismissed (not fixed)

6. **MEDIUM** `formatBytes` negative input — `byte_size` is always non-negative from server; defensive but not a real bug
7. **MEDIUM** `relativeTime` future dates — clock skew gives "just now" which is acceptable
8. **MEDIUM** Yank button shown to non-collaborators — server enforces properly; requires additional API call to check client-side
9. **MEDIUM** Settings page accessible to non-collaborators — same as above; server enforces access
10-18. Various LOW issues — cosmetic/minor UX choices, not bugs

## Status: 5 fixes applied, rest dismissed as non-actionable
