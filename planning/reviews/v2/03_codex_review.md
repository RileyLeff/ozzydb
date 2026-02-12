# Review 03 — Codex Round 2 (Post-Fix Verification)

**Date:** 2026-02-12
**Model:** gpt-5.3-codex (xhigh reasoning)
**Session:** 019c537e-cc0d-7941-803f-9c3c19ab68da
**Scope:** Round 2 verification of Phase 1 fixes

## Findings

### `crates/ozzy-server/src/db/queries.rs`

1. **[MAJOR] `upsert_session_token` violates scope/project CHECK.** ON CONFLICT update sets `scope='account'` but didn't clear `project_id`. If existing row had `project_id != NULL`, `chk_scope_project` fails. **FIXED:** Added `project_id = NULL` to the ON CONFLICT clause.

### `crates/ozzy-core/src/toml_spec.rs`

2. **[MINOR] Endpoint param type values not validated.** Invalid types like `"vector"` pass `validate()`. (Same as Round 1 #5 — intentionally deferred to runtime.)

3. **[MINOR] Cross-project endpoint ref structural parsing weak.** Malformed refs with `/` and `@` like `endpoint:org/proj@v1` pass validation. (Structural parsing is intentionally loose — the registry validates ref existence at push/fetch time.)

### `crates/ozzy-server/tests/db_tests.rs`

4. **[MINOR] No regression test for session token scope conflict.** (Will add when building auth CLI in Phase 3.)

5. **[NOTE] Negative tests for composite FK rejection still missing.** (Acknowledged — will add incrementally.)

## Verdict

Round 1 fixes correctly implemented. One new MAJOR (#1) found and fixed. Remaining items are MINORs/NOTEs that are intentionally deferred to later phases.
