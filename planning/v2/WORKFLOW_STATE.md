# v2 Workflow State

**Current Phase:** 3 — Git Integration & Push
**Current Step:** Phase 3 exhaustive review
**Status:** All Phase 3 steps complete. Running exhaustive review.

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
| 3 | 3.1 | GitHub App integration | Done |
| 3 | 3.2 | Push endpoint | Done |
| 3 | 3.3 | Endpoint inspection + project API | Done |
| 3 | 3.4 | CLI init + transform scaffold | Done |
| 3 | 3.5 | Auth CLI commands | Done |
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

- Phase 3 implementation complete:
  - Step 3.1 (74c6eb9): GitHub App integration
    - GitHubProvider with JWT RS256 signing, installation token flow
    - fetch_archive, get_file, resolve_ref methods
    - Webhook handler (installation events, HMAC verification)
    - 10 unit tests
  - Step 3.2 (44e70cd): Push endpoint
    - POST /v1/push registers git commits
    - Validates ozzy.toml from git, verifies transform sources
    - Source tarball caching, idempotent push
    - 1 unit test
  - Step 3.3 (02204e1): Endpoint inspection + project API
    - GET /v1/projects/{owner}, /v1/projects/{owner}/{slug}
    - GET /v1/endpoints/{owner}/{slug}, /{name}, /{name}/dag
    - resolve_commit_state helper, Mermaid DAG rendering
  - Step 3.4 (2f483b9): CLI init + transform scaffold
    - `ozzy init` with git/language auto-detection
    - `ozzy transform scaffold` for Python and R
    - 15 unit tests + 2 integration tests
  - Step 3.5 (3cd9207): Auth CLI commands
    - login (GitHub device flow), logout, status
    - token create/ls/revoke
    - Credentials at ~/.config/ozzy/credentials.json
    - 3 unit tests
- Running Phase 3 exhaustive review
