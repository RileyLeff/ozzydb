# v2 Workflow State

**Current Phase:** 2 — Data Plane (COMPLETE, running exhaustive review)
**Current Step:** Phase 2 exhaustive review
**Status:** All server-side API endpoints implemented and tested

## Progress

| Phase | Step | Description | Status |
|-------|------|-------------|--------|
| 1 | 1.1 | Replace Postgres schema with v2 DDL | Done |
| 1 | 1.2 | Build ozzy.toml parser (toml_spec.rs) | Done |
| 1 | 1.3 | Update core hash functions | Done |
| 1 | 1.4 | Update server AppState and DB layer | Done |
| 2 | 2.1 | Data upload API + CLI | Done (server) |
| 2 | 2.2 | Collections API + CLI | Done (server) |
| 2 | 2.3 | Secrets API + CLI | Done (server) |
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

- Step 2.1 complete (db7cb23): Data atom API
  - 7 routes: upload, list, get, download, yank, describe, metadata
  - Multipart upload with content dedup via content_refs
  - 410 Gone on yanked download
  - 5 Docker integration tests + 4 unit tests
  - Fixed testcontainers to use Postgres 17 (was 11)
- Step 2.2 complete (f918621): Collections API
  - 8 routes: create, list, get, log, flatten, add, remove, yank
  - DFS cycle detection, recursive flatten, member dedup
  - 2 Docker integration tests (lifecycle + cycle detection) + 1 unit test
- Step 2.3 complete (575dd6f): Secrets API
  - 3 routes: set, list, delete
  - AES-256-GCM encryption, values write-only
  - Config: SECRETS_ENCRYPTION_KEY env var
  - 3 Docker integration tests + 3 unit tests
- **Phase 2 server-side complete.** Running exhaustive review.
