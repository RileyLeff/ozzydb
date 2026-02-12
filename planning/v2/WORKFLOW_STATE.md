# v2 Workflow State

**Current Phase:** 1 — Foundation (COMPLETE)
**Current Step:** Moving to Phase 2
**Status:** Phase 1 review pending

## Progress

| Phase | Step | Description | Status |
|-------|------|-------------|--------|
| 1 | 1.1 | Replace Postgres schema with v2 DDL | Done |
| 1 | 1.2 | Build ozzy.toml parser (toml_spec.rs) | Done |
| 1 | 1.3 | Update core hash functions | Done |
| 1 | 1.4 | Update server AppState and DB layer | Done |
| 2 | 2.1 | Data upload API + CLI | Pending |
| 2 | 2.2 | Collections API + CLI | Pending |
| 2 | 2.3 | Secrets API + CLI | Pending |
| 3 | 3.1 | GitHub App integration | Pending |
| 3 | 3.2 | Push endpoint | Pending |
| 3 | 3.3 | Endpoint inspection + project API | Pending |
| 3 | 3.4 | CLI init + transform scaffold | Pending |
| 3 | 3.5 | Auth CLI commands | Pending |
| 4 | 4.1 | Environment building | Pending |
| 4 | 4.2 | Runner generation | Pending |
| 4 | 4.3 | Compute backend + Fly Machines | Pending |
| 4 | 4.4 | Server fetch endpoint (DAG execution) | Pending |
| 4 | 4.5 | CLI run + fetch | Pending |
| 4 | 4.6 | Cache management CLI | Pending |
| 5 | 5.1-5.8 | Frontend pages | Pending |
| 6 | 6.1-6.3 | Python client | Pending |
| 7 | 7.1-7.3 | Deployment + integration | Pending |

## Blockers

None.

## Recent Activity

- Step 1.1 complete (3ded031): v2 DDL, models, queries, auth middleware, tests
  - Fresh 001_v2_initial.sql with all v2 tables
  - Rewrote models.rs, queries.rs for v2 (singular scope, git-referenced commits, etc.)
  - Simplified auth middleware for singular scope model
  - 14 DB tests, 5 API tests, all passing
  - Server integration/e2e tests gutted (will be rewritten with v2 endpoints)
- Step 1.2 complete (4bcb31f): ozzy.toml parser
  - toml_spec.rs: all v2 structs, 11 validation rules, DAG cycle detection
  - 27 tests covering parsing, roundtrip, all validation rules
- Step 1.3 complete (c1d295a): hash functions updated
  - transform_hash: added environment_image_hash parameter
  - secrets_hash: new function for sorted (name, version_id) pairs
  - materialized_hash: unified multi-input API with optional secrets_hash
  - collection_hash: new function for sorted member hashes
  - 20 tests including golden values
- Step 1.4 complete (d4133ab): remaining DB queries + tests
  - Project update/delete, collection CRUD + versioning
  - Endpoint yanks, secrets, environment images, source/materialized cache
  - 7 new DB tests (21 total), all passing
- **Phase 1 complete.** Running review before Phase 2.
