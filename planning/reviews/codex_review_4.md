# OzzyDB Comprehensive Review (Codex Round 4)

Date: February 7, 2026

Scope: review against `planning/outdated/ozzydb-architecture_draft_1.md`, `planning/ozzydb_soul.md`, and `planning/NEXT_STEPS.md`, plus full code/test audit of phase 1/2 implementation.

## Findings (Ordered by Severity)

1. High: Staged endpoint deletions are bypassed by execution and DAG views.
Evidence:
`crates/ozzy-cli/src/commands/run.rs:288`, `crates/ozzy-cli/src/commands/dag.rs:42`, `crates/ozzy-cli/src/commands/endpoint.rs:22`

`ozzy endpoint rm <name>` stages `.deleted`, but `ozzy run` and `ozzy dag` still load committed endpoint definitions and execute/render them.

2. Medium: Multi-input schema validation remains effectively single-input.
Evidence:
`crates/ozzy-cli/src/commands/endpoint.rs:115`, `crates/ozzy-cli/src/commands/endpoint.rs:312`

Validation is anchored to one primary input schema (`main` or first provided), so richer multi-input transform contracts can be misvalidated.

3. Medium: `pull` does not reconcile staged endpoint state.
Evidence:
`crates/ozzy-cli/src/commands/pull.rs:176`, `crates/ozzy-cli/src/commands/pull.rs:223`, `crates/ozzy-cli/src/commands/commit.rs:28`

Pull updates data/transforms/commits/refs but leaves `.ozzy/staged_endpoints` untouched, so stale staged endpoint adds/deletes can leak into later commits.

4. Medium: Decorator parser claims dict-style `inputs={...}` support but only parses lists.
Evidence:
`crates/ozzy-core/src/commit.rs:309`

Implementation only handles list parsing for `inputs=...`; dict form silently falls back to defaults.

5. Medium: Python client metadata ignores staged endpoint deletions.
Evidence:
`clients/python/src/ozzydb/project.py:184`, `clients/python/src/ozzydb/project.py:245`, `clients/python/src/ozzydb/project.py:269`

`Project.endpoints` and `Project.get_endpoint()` include committed endpoints even when they are staged for deletion.

6. Low: `transform add --name` remains unimplemented.
Evidence:
`crates/ozzy-cli/src/commands/transform.rs:9`

CLI exposes `--name`, but command aborts with “not supported yet.”

## Phase 1/2 Alignment Summary

Implemented and working:
1. Local-first lifecycle (`init`, `data`, `transform`, `endpoint`, `run`, `commit`, `log`, `status`).
2. Content-addressing + canonicalization + platform fingerprint hashing.
3. Local cache and remote cache integration plumbing.
4. Registry push/pull/fetch/resolve flows with scoped tokens and collaborators.
5. Commit history endpoint (`GET /api/v1/{owner}/{project}/commits`).
6. Push command signature cleanup (`push::run(..., remote: &str)`).

Still out-of-sync before treating phase 2 as fully clean:
1. Staged endpoint deletion consistency across all CLI/read paths.
2. Multi-input schema validation model.
3. Pull reconciliation of staged endpoint state.
4. Decorator parser `inputs` contract fidelity.
5. Python client parity with staged endpoint deletion semantics.
6. `transform add --name` command contract.

## Validation Performed

1. `cargo test --workspace` passed.
2. `uv run --directory clients/python --with pytest pytest -q tests` passed (`17 passed`).
