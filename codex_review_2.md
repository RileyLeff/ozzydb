# OzzyDB Comprehensive Review (Codex Round 2)

Date: February 5, 2026

## Findings (Ordered by Severity)

1. High: Remote config can be silently deleted by unrelated commands.
Evidence:
`crates/ozzy-cli/src/commands/remote.rs:31`, `crates/ozzy-core/src/project.rs:93`, `crates/ozzy-core/src/project.rs:309`, `crates/ozzy-core/src/project.rs:364`, `crates/ozzy-cli/src/commands/endpoint.rs:175`, `crates/ozzy-cli/src/commands/commit.rs:67`

`ozzy remote add` writes `[remotes]`, but `ProjectConfig` does not model that section. Any command that calls `save_config()` rewrites `ozzy.toml` from a lossy struct and drops unmodeled tables, including remotes.

2. High: Tar extraction in `pull`/`fetch` is vulnerable to path traversal.
Evidence:
`crates/ozzy-cli/src/commands/pull.rs:77`, `crates/ozzy-cli/src/commands/pull.rs:88`, `crates/ozzy-cli/src/commands/fetch.rs:210`, `crates/ozzy-cli/src/commands/fetch.rs:211`

Archive paths are joined and written directly without verifying they remain under the intended extraction root.

3. High: Python client `override_params` contract is broken for local and remote fetch.
Evidence:
`clients/python/src/ozzydb/client.py:174`, `clients/python/src/ozzydb/client.py:218`, `crates/ozzy-cli/src/commands/run.rs:27`, `crates/ozzy-cli/src/commands/run.rs:116`

The Python client emits `transform.param=value`, but runtime treats keys literally and does not map namespaced keys to per-transform param dicts. Overrides can be silently ignored.

4. Medium: `push` cleanup is incomplete on DB failure.
Evidence:
`crates/ozzy-server/src/api/v1/push_pull.rs:243`, `crates/ozzy-server/src/api/v1/push_pull.rs:258`

Uploaded blobs are cleaned up on storage-stage failure, but not if commit DB insertion fails afterward.

5. Medium: One-line `@ozzy.transform(...)` decorators can be skipped by parser.
Evidence:
`crates/ozzy-core/src/commit.rs:172`, `crates/ozzy-core/src/commit.rs:179`

Decorator parser logic can advance past the associated `def` line in common single-line decorator forms.

6. Medium: `org` visibility behaves like “any authenticated user”.
Evidence:
`crates/ozzy-server/src/api/v1/projects.rs:90`, `crates/ozzy-server/src/api/v1/push_pull.rs:49`

`visibility="org"` can be set, but enforcement allows all authenticated users due to missing org membership model/validation.

7. Medium: Transform source-path fidelity is not preserved through push/pull.
Evidence:
`crates/ozzy-cli/src/commands/push.rs:72`, `crates/ozzy-server/src/api/v1/push_pull.rs:464`, `crates/ozzy-server/src/api/v1/push_pull.rs:528`

Transforms are materialized by transform name (`{name}.py`) instead of preserving original source file paths.

8. Low: `push::run()` signature is misleading for current CLI call path.
Evidence:
`crates/ozzy-cli/src/commands/push.rs:26`, `crates/ozzy-cli/src/main.rs:550`

`push::run()` accepts `remote: Option<&str>`, but clap always supplies a value (`origin` by default), and `main` always passes `Some(&remote)`. This is a type-shape cleanup issue rather than a runtime bug.

## Phase 1 and 2 Items Not Fully Implemented

1. Phase 1: DAG visualization does not support SVG output.
Evidence:
`ozzydb-architecture_draft_1.md:1824`, `crates/ozzy-cli/src/commands/dag.rs:31`

2. Phase 1: Local execution path does not build/use lockfile-based runtime envs as specified.
Evidence:
`ozzydb-architecture_draft_1.md:1835`, `crates/ozzy-cli/src/commands/run.rs:362`, `crates/ozzy-core/src/runtime.rs:553`, `crates/ozzy-core/src/runtime.rs:631`

3. Phase 1 validation: No sap-flux-specific end-to-end fixture/test was found.
Evidence:
`ozzydb-architecture_draft_1.md:1850`, `crates/ozzy-cli/tests/integration_test.rs:21`

4. Phase 2: `GET /resolve/{owner}/{project}/{endpoint}@{ref}` is not implemented.
Evidence:
`ozzydb-architecture_draft_1.md:1864`, `crates/ozzy-server/src/api/v1/push_pull.rs:93`

5. Phase 2: CLI exposes `ozzy fetch --registry`, but current command path ignores this flag.
Evidence:
`crates/ozzy-cli/src/main.rs:160`, `crates/ozzy-cli/src/main.rs:556`, `crates/ozzy-cli/src/commands/fetch.rs:73`

6. Phase 2: Remote tag lifecycle is incomplete. Push updates branch refs only, with no explicit tag push path.
Evidence:
`ozzydb-architecture_draft_1.md:1861`, `crates/ozzy-cli/src/commands/tag.rs:7`, `crates/ozzy-server/src/api/v1/push_pull.rs:272`

7. Phase 2: Commit history API endpoint is missing.
Evidence:
`ozzydb-architecture_draft_1.md:1863`, `crates/ozzy-server/src/api/v1/projects.rs:27`, `crates/ozzy-server/src/api/v1/push_pull.rs:93`

No `GET /{owner}/{project}/commits` route is currently exposed, so clients cannot browse project history via API without pulling project artifacts.

## Follow-up Review (Post-Fix)

1. Medium: Pulling a tag updates a branch ref path instead of a tag ref path.
Evidence:
`crates/ozzy-cli/src/commands/pull.rs:153`, `crates/ozzy-core/src/project.rs:436`, `ozzydb-architecture_draft_1.md:1861`

`pull` always wrote `refs/heads/{ref}` locally, which could leave local tag refs out-of-sync.

2. Medium: Scoped API tokens were only partially enforced.
Evidence:
`crates/ozzy-server/src/auth/middleware.rs:37`, `crates/ozzy-server/src/auth/middleware.rs:52`, `ozzydb-architecture_draft_1.md:1867`

Write endpoints enforced `write`, but read-protected paths accepted tokens without validating read scope.

3. Low: CLI shape drift from architecture for DAG command.
Evidence:
`ozzydb-architecture_draft_1.md:1223`, `ozzydb-architecture_draft_1.md:1824`, `crates/ozzy-cli/src/main.rs:62`

Architecture specifies `ozzy dag show`; CLI only exposed `ozzy dag`.

4. Low: CLI shape drift from architecture for tag command.
Evidence:
`ozzydb-architecture_draft_1.md:1876`, `crates/ozzy-cli/src/main.rs:404`

Architecture specifies `ozzy tag <name>`; CLI only exposed `ozzy tag create <name>`.

5. Low: Phase 1 validation checklist remained partially unproven in tests.
Evidence:
`ozzydb-architecture_draft_1.md:1851`, `ozzydb-architecture_draft_1.md:1852`, `ozzydb-architecture_draft_1.md:1853`, `crates/ozzy-cli/tests/integration_test.rs:21`

At review time, explicit tests for cache invalidation on transform code changes, deterministic hash stability in end-to-end flow, and negative schema-mismatch endpoint creation were not yet present.

## Validation Performed

1. `cargo test --workspace` passed.
2. `clients/python`: `uv run pytest -q` passed.
3. Note: server DB/R2 integration tests are gated by environment variables and can pass without exercising real DB/R2 if those vars are absent.
