# v2 Workflow State

**Current Phase:** 4 — Execution
**Current Step:** Complete — exhaustive review in progress (round 18 done)
**Status:** All 6 steps implemented. 18 review rounds complete, ~45 bugs fixed. Need 2 consecutive clean rounds. Blocked on Codex rate limit (resets 7:30 AM ET).

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
| 4 | 4.1 | Environment building | Done |
| 4 | 4.2 | Runner generation | Done |
| 4 | 4.3 | Compute backend (Docker) | Done |
| 4 | 4.4 | Server fetch endpoint (DAG execution) | Done |
| 4 | 4.5 | CLI run + fetch | Done |
| 4 | 4.6 | Cache management CLI | Done |
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
- Phase 3 exhaustive review converged:
  - 11 review rounds (Codex, Gemini, Claude)
  - ~70 issues found and fixed across all rounds
  - 3 consecutive Claude clean rounds (9, 10, 11) + 1 Codex clean (round 9)
  - Review artifacts: #01-#26 in planning/reviews/v2/
- Phase 4 implementation complete:
  - Step 4.1 (e79e12b): Environment building
    - EnvironmentBuilder trait, DockerEnvironmentBuilder
    - Tier 1 (base+lockfile), Tier 2 (Dockerfile), Tier 3 (prebuilt)
    - Environment hash computation, Dockerfile generation
    - DB integration (environment_images table)
    - Background build spawning from push endpoint
  - Step 4.2 (94cd10f): Runner generation
    - Python runner (loads inputs, calls function, writes parquet/csv output)
    - R runner (same pattern with arrow/jsonlite)
    - Command runner (template substitution, no shell injection)
    - Init script generation (input download, runner execution, output packaging)
  - Step 4.3 (746beb9): Docker compute backend
    - ComputeBackend trait, DockerBackend implementation
    - Workspace setup, bind mounts, --network=none
    - Determinism env vars, gVisor runtime support
    - Timeout enforcement, output collection
  - Step 4.4 (bdd04f9): Server fetch endpoint
    - GET /v1/fetch/{owner}/{project}/{endpoint}
    - Full DAG execution: resolve → toposort → cache check → compute → store
    - Param validation, yank checking, output schema verification
    - Materialized cache integration
  - Step 4.5 (fcabc8f): CLI run + fetch
    - `ozzy run` — local DAG execution with Docker
    - `ozzy fetch` — HTTP client for remote execution
    - Local cache at ~/.ozzy/cache/materialized/{hash}/
    - --local-data for binding local files to data references
  - Step 4.6 (1f795cb): Cache management CLI
    - `ozzy cache ls/size/clear`
    - Recursive directory size computation
    - Human-readable size formatting
- Phase 4 exhaustive review in progress:
  - 18 rounds of Claude Opus via Codex (Gemini consistently fails at ~260k tokens)
  - ~45 issues found and fixed
  - Key fixes: env hash alignment, timeout kill, terminal node detection, path containment, secret blocklists, importlib runners, auth ordering
  - 43 known limitations documented
  - 90 core + 110 server + 4 CLI unit = 204 tests passing
  - Review artifacts: #27-#44 in planning/reviews/v2/
  - Blocked on Codex rate limit for round 19 (resets 7:30 AM ET 2026-02-13)
