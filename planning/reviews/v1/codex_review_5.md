# OzzyDB Comprehensive Review (Codex Round 5)

Date: February 7, 2026

Scope: review against `planning/outdated/ozzydb-architecture_draft_1.md`, `planning/ozzydb_soul.md`, and `planning/NEXT_STEPS.md`, plus full code/test audit of current phase 1/2 implementation.

## Findings (Ordered by Severity)

1. High: Transform file moves are not detected as changes.
Evidence:
`crates/ozzy-core/src/commit.rs:550`, `crates/ozzy-cli/src/commands/status.rs:60`

`has_changes()` and `status` compare transform hash only, not `source_path`/function/runtime metadata. Moving `transforms/t.py` to `transforms/sub/t.py` can incorrectly report a clean working tree, so `commit` may no-op.

2. High: Push can persist commits that reference transform hashes not present in storage.
Evidence:
`crates/ozzy-server/src/api/v1/push_pull.rs:406`, `crates/ozzy-server/src/api/v1/push_pull.rs:435`

Push validates schema availability for committed data hashes but does not perform equivalent existence checks for transform hashes before commit insertion.

3. High: First-time project push is blocked by content-check preflight.
Evidence:
`crates/ozzy-cli/src/commands/push.rs:103`, `crates/ozzy-server/src/api/v1/push_pull.rs:512`, `crates/ozzy-server/src/api/v1/push_pull.rs:205`

CLI always calls `/content/check` first. Server `/content/check` requires an existing project, while `/push` supports create-on-push. This can fail bootstrap pushes before they reach project creation logic.

4. Medium: `endpoint ls` can show duplicate entries for the same endpoint.
Evidence:
`crates/ozzy-cli/src/commands/endpoint.rs:460`, `crates/ozzy-cli/src/commands/endpoint.rs:471`

When an endpoint exists in both committed state and staged JSON, both entries are printed.

5. Medium: Remote `fetch` execution path skips output schema contract validation.
Evidence:
`crates/ozzy-cli/src/commands/run.rs:460`, `crates/ozzy-cli/src/commands/fetch.rs:528`

`run` validates output schema after transform execution; `fetch` does not, creating inconsistent enforcement between local and remote execution paths.

6. Medium: `transform add <file:function>` is still blocked in multi-transform files.
Evidence:
`crates/ozzy-cli/src/commands/transform.rs:47`

This conflicts with the documented CLI contract in the architecture spec where `transform add <file:function>` is the selector mechanism.

7. Medium (Phase 2 completeness gap): server API surface remains partial vs architecture spec section 7.
Evidence:
`crates/ozzy-server/src/api/v1/projects.rs:33`, `crates/ozzy-server/src/api/v1/push_pull.rs:170`

Current routes cover project metadata, refs/collaborators, push/pull, resolve/fetch, but not `/dag`, `/dag.svg`, `/endpoints`, `/transforms`, `/schemas/{name}` endpoints listed in phase-2 architecture API scope.

8. Low (Next-steps alignment): legacy tiered remote-cache stack is still present.
Evidence:
`crates/ozzy-core/src/cache/mod.rs:6`, `crates/ozzy-cli/src/main.rs:300`

`cache push/pull/sync/status` and remote tiered cache modules still exist, while `planning/NEXT_STEPS.md` calls this dead code slated for removal before hosted-platform work.

9. Low (Next-steps alignment): `ozzy init` does not create project-root gitignore rules for `data/*.parquet` and `.ozzy/`.
Evidence:
`crates/ozzy-core/src/project.rs:358`

Current behavior writes `.ozzy/.gitignore` internals only; next-steps explicitly calls for root `.gitignore` generation to keep raw data and local internals out of git.

## Phase 1/2 Alignment Summary

Implemented and working:
1. Local lifecycle: `init`, `data`, `transform`, `endpoint`, `run`, `commit`, `log`, `status`.
2. Content-addressing, canonicalization, platform fingerprint hashing, deterministic runtime defaults.
3. Local cache and registry push/pull/fetch/resolve flows.
4. Auth, scoped tokens, collaborators, refs/tags, commit-history API.

Needs cleanup before treating phase 1/2 as fully clean:
1. Transform move detection in change tracking and commit readiness.
2. Push integrity checks for transform artifact availability.
3. Bootstrap push path consistency (`/content/check` vs create-on-push behavior).
4. Endpoint listing dedup for staged+committed overlays.
5. Output schema validation parity between `run` and `fetch`.
6. `transform add <file:function>` behavior alignment with CLI contract.
7. Remaining phase-2 API parity gaps listed above.

## Validation Performed

1. `cargo test --workspace` passed.
2. `uv run --directory clients/python --with pytest pytest -q tests` passed (`18 passed`).
3. Manual CLI repros were run for:
   - transform move undetected by `status`/`commit`
   - duplicate endpoint entries in `endpoint ls`
   - `transform add <file:function>` multi-transform rejection
