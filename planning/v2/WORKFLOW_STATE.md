# v2 Workflow State

**Current Phase:** 7 — Deployment & Integration (IN PROGRESS)
**Current Step:** 7.2 E2E tests complete, exhaustive review round 1 running
**Status:** Step 7.1 (Docker config) committed. Step 7.2 (13 E2E tests) committed and passing. Review loop in progress.

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
| 5 | 5.1 | API client + types + commits endpoint | Done |
| 5 | 5.2 | Project overview page + ProjectTabs | Done |
| 5 | 5.3 | Data browser (list + detail) | Done |
| 5 | 5.4 | Collection browser (list + detail) | Done |
| 5 | 5.5 | Endpoint explorer (list + detail) | Done |
| 5 | 5.6 | Commit history (list + detail) | Done |
| 5 | 5.7 | Settings + secrets management | Done |
| 5 | 5.8 | User profile update | Done |
| 5 | review | Exhaustive review (5 rounds, converged) | Done |
| 6 | 6.1 | Types + HTTP client foundation | Done |
| 6 | 6.2-6.3 | fetch, inspect, run, upload, download | Done |
| 6 | 6.4 | Unit tests (48 passing) | Done |
| 6 | 6.5 | Package exports + README | Done |
| 6 | review | Exhaustive review (7 rounds, converged) | Done |
| 7 | 7.1 | Docker Compose config updates | Done |
| 7 | 7.2 | E2E tests (13 tests, all green) | Done |
| 7 | review | Exhaustive review (in progress) | In Progress |
| 7 | 7.3 | Deploy to production | Pending |

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
- Phase 4 exhaustive review CONVERGED:
  - 21 review rounds total (rounds 1-18 via Codex, 19-21 via Claude subagent)
  - ~47 issues found and fixed
  - Key fixes: env hash alignment, timeout kill, terminal node detection, path containment, secret blocklists, importlib runners, auth ordering, container name mismatch, lockfile hash divergence
  - 43 known limitations documented
  - 90 core + 144 server + 44 CLI unit = 278 tests passing
  - Rounds 20+21 both clean → convergence
  - Review artifacts: #27-#47 in planning/reviews/v2/
- Phase 5 implementation complete:
  - Step 5.1 (2e654ac): API client + types + commits server endpoint
  - Step 5.2 (0218887): Project overview + ProjectTabs component + utils
  - Step 5.3 (1bfb720): Data browser (list + detail with download/yank)
  - Step 5.4 (e9cc85f): Collection browser (list + detail with flatten/version history)
  - Step 5.5 (c272de3): Endpoint explorer (list + detail with param form/DAG/run)
  - Step 5.6 (c02e194): Commit history (list + detail with state display)
  - Step 5.7 (905178c): Settings with secrets management (CRUD)
  - Step 5.8 (cd297f6): User profile shows projects for all users
- Phase 5 exhaustive review CONVERGED:
  - 5 review rounds total (1 Codex, 4 Claude subagent)
  - ~13 issues found and fixed across rounds 1-3
  - Key fixes: ProjectTabs reactivity, UUID→username resolution (data/collections/commits), Object URL lifecycle, stale state on navigation, ref param collision, negative limit clamping
  - Rounds 4+5 both clean → convergence
  - Review artifacts: #48-#52 in planning/reviews/v2/
- Phase 6 implementation complete:
  - Step 6.1 (53b6744): Types + HTTP client (20+ dataclasses, OzzyClient with auth)
  - Step 6.2-6.3 (7394e12): All client functions (fetch, fetch_lazy, inspect, inspect_project, run, upload, download, download_dataframe)
  - Step 6.4 (68e0b91): Unit tests (44 passing) with mocked HTTP
  - Step 6.5 (4c105bb): README and package exports
