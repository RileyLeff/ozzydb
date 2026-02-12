# Fixes for Review 03 — Codex Round 2

**Date:** 2026-02-12

## MAJOR Fix

### #1: upsert_session_token scope/project CHECK violation
Added `project_id = NULL` to the ON CONFLICT UPDATE clause, and explicit `NULL` to the INSERT values. Session tokens are always account-scoped, so project_id must always be NULL.

## Deferred MINORs

- Endpoint param type validation → runtime (Phase 4)
- Cross-project ref structural parsing → registry validates at push/fetch (Phase 3)
- Session token scope conflict test → Phase 3 auth CLI
- Negative FK tests → incremental addition in Phase 2+
