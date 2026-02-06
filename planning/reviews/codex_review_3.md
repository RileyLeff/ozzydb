# OzzyDB Comprehensive Review (Codex Round 3)

Date: February 6, 2026

Scope: review against `ozzydb-architecture_draft_1.md`, `ozzydb_soul.md`, and `NEXT_STEPS.md`, plus full code and test audit of phase 1/2 implementation.

## Findings (Ordered by Severity)

1. High: `ozzy push -m` can violate content-addressing invariants.
Evidence:
`crates/ozzy-cli/src/commands/push.rs:126`, `crates/ozzy-cli/src/commands/push.rs:146`, `crates/ozzy-server/src/db/queries.rs:276`

`push` can override commit message after loading a pre-hashed commit, and server-side commit persistence does not recompute/verify hash integrity before insert.

2. High: Committed endpoints are effectively undeletable.
Evidence:
`crates/ozzy-cli/src/commands/endpoint.rs:357`, `crates/ozzy-cli/src/commands/endpoint.rs:370`, `crates/ozzy-core/src/commit.rs:24`, `crates/ozzy-cli/src/commands/commit.rs:27`

`endpoint rm` only removes staged endpoints. Committed endpoints are always carried forward when new commits are created, so there is no supported delete path for already committed endpoints.

3. High: `transform add --name` and `file:function` selection are misleading/no-op for persisted behavior.
Evidence:
`crates/ozzy-cli/src/commands/transform.rs:6`, `crates/ozzy-cli/src/commands/transform.rs:69`, `crates/ozzy-core/src/commit.rs:149`

`--name` is ignored. `file:function` filters display output only; commit-time transform discovery still includes all decorated functions in the file.

4. Medium: Endpoint creation only supports multi-input on the first node.
Evidence:
`crates/ozzy-cli/src/commands/endpoint.rs:126`, `crates/ozzy-cli/src/commands/endpoint.rs:142`

First transform can receive multiple named inputs, but downstream nodes are forced into single-input chaining via `main`, blocking richer multi-input DAG composition beyond step 1.

5. Medium: `visibility = "org"` is exposed without org data model enforcement.
Evidence:
`crates/ozzy-server/migrations/001_initial.sql:35`, `crates/ozzy-server/src/db/models.rs:25`, `crates/ozzy-server/src/db/queries.rs:94`, `crates/ozzy-server/src/api/v1/projects.rs:57`

Server schema and ownership model remain user-owned only; org visibility cannot be enforced according to architecture semantics.

6. Medium: Scope matching does not support wildcard project scope patterns shown in spec examples.
Evidence:
`crates/ozzy-server/src/auth/middleware.rs:71`, `crates/ozzy-server/src/auth/middleware.rs:95`

Scope matching is exact owner/project equality only.

7. Medium: API tests are too permissive and can mask regressions.
Evidence:
`crates/ozzy-server/tests/api_tests.rs:176`, `crates/ozzy-server/tests/api_tests.rs:217`, `crates/ozzy-server/tests/api_tests.rs:239`

Several route tests accept `500` as a valid outcome where strict status assertions should be expected.

8. Low: `pull` extraction does not reconcile removals and leaves stale local files.
Evidence:
`crates/ozzy-cli/src/commands/pull.rs:152`, `crates/ozzy-cli/src/commands/pull.rs:169`, `crates/ozzy-cli/src/commands/pull.rs:175`

Pull extracts incoming archive entries but does not remove local files deleted upstream, so local working trees can drift.

## Phase 1/2 Alignment Summary

Implemented and working:
1. Local-first CLI lifecycle and local DAG execution.
2. Content-addressed cache behavior and deterministic env defaults.
3. Schema extraction and endpoint create-time schema checks.
4. Registry push/pull/fetch/resolve flows.
5. Commit history API endpoint (`GET /api/v1/{owner}/{project}/commits`).
6. Push signature cleanup (`push::run(..., remote: &str)`).
7. Scoped token plumbing and collaborator CRUD paths.

Still out-of-sync before treating phase 2 as fully complete:
1. Hash invariants for push (`-m` mutation issue).
2. Endpoint deletion lifecycle.
3. Transform add CLI contract mismatches (`--name`, `file:function`).
4. Multi-input DAG expressiveness after first node.
5. Org visibility semantics not backed by org ownership/membership model.

## Validation Performed

1. `cargo test --workspace` passed.
2. `uv run --directory clients/python --with pytest pytest -q tests` passed (`17 passed`).

