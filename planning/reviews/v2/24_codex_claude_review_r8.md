# Phase 3 Review Round 8 (Codex + Claude)

## Reviewers
- Codex (primary review)
- Claude Opus 4.6 (parallel sub-agent review)
- Gemini (failed — CLI exit 1, empty output; excluded from round)

## Findings

### Fixed
1. **HIGH (Codex + Claude): CLI auth URL path mismatch** (auth.rs:174,212,277,319,361,413)
   - All 6 CLI auth endpoints used `/v1/auth/...` but server mounts at `/api/v1/auth/...`
   - Frontend correctly uses `/api/v1` prefix — only CLI was wrong
   - Fixed: Changed all CLI auth URLs from `/v1/` to `/api/v1/`

### Dismissed
- Codex HIGH: Data upload + collection partial write — Data atoms are idempotent/content-addressed. An orphaned atom after collection add failure is harmless (same content, no duplicate). Can't transact filesystem writes with DB writes. Design trade-off.
- Codex MEDIUM: Frontend API contract mismatch — Comment on api.ts:52 says "v2: will be reimplemented with new API". Project list/detail pages are known v2 placeholder stubs, not yet wired to the v2 API.
