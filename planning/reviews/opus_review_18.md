# Opus Review Round 3 (R18) - 2026-02-07

## CRITICAL

### C1 - Transform hash identity confusion (systemic, spans CLI + server)
Storage uses source content hash `blake3(canonical_source)` but lookups use composite transform hash `blake3(source_hash + function_name + lockfile + runtime + params_schema_hash)`. Affects:
- Server `check_content` (push_pull.rs:559) - always says "missing"
- Server push validation (push_pull.rs:396) - every push fails "Missing transform source"
- CLI pull.rs:279-283 - every pull fails "Transform hash mismatch"
- CLI fetch.rs:186-198 - same as pull
- **Fix**: Use source_hash (not composite hash) for content storage lookups and verification

## HIGH

### H1 - No Axum body size limit (main.rs:62-77)
No DefaultBodyLimit layer. field.bytes().await buffers entire field before size check. OOM via single large multipart field.
- **Fix**: Add DefaultBodyLimit layer to the router

### H2 - load_commit doesn't verify hash integrity (project.rs:592-599)
Deserializes commit and trusts stored hash without recomputing. Corrupted/tampered commits silently accepted.
- **Fix**: Recompute and compare hash on load

## MEDIUM

### M1 - pull prune path mismatch on symlinked paths (pull.rs:308-316)
Non-canonical dir + canonical project_root = strip_prefix fails on macOS (/tmp -> /private/tmp).
- **Fix**: Canonicalize dir before passing to prune_unlisted_files

### M2 - push sends ALL tags, not just current commit's (push.rs:122-136)
Reads every tag from .ozzy/refs/tags/ and sends them all, overwriting server state for unrelated commits.
- **Fix**: Only send tags that point to the commit being pushed

### M3 - struct schema roundtrip broken (schema.rs)
format_data_type produces `struct<...>` but parse_data_type has no struct< branch. Falls to Utf8.
- **Fix**: Add struct parsing branch

### M4 - fetch allows empty endpoint/ref (fetch.rs:47-55)
`owner/project/@v1` -> empty endpoint. `owner/project/ep@` -> empty ref.
- **Fix**: Validate non-empty after parsing

### M5 - percent_decode corrupts multi-byte UTF-8 (server push_pull.rs:45-58)
`byte as char` wrong for bytes >127. Non-ASCII filenames mangled.
- **Fix**: Collect decoded bytes into a Vec<u8> and convert via String::from_utf8

### M6 - Python TransformMeta.hash typed str but assigned None (types.py:28, project.py:170)
hash: str and lockfile_hash: str should be Optional[str].
- **Fix**: Change types to Optional[str] with default None

### M7 - endpoint show calls collect_data_sources per edge (endpoint.rs:585)
Re-scans and re-hashes all parquet files for each edge. O(edges * data_files).
- **Fix**: Hoist collect_data_sources call above the loop

### M8 - No rate limiting on /auth/github/poll (server auth.rs:53-126)
Unauthenticated endpoint makes outbound GitHub requests per call.
- **Fix**: Add basic in-memory rate limiting or note as pre-deployment TODO

### M9 - GitHub username changes break project URLs (server queries.rs:70-71)
ON CONFLICT DO UPDATE SET username = EXCLUDED.username disconnects existing project URLs.
- **Fix**: Note as pre-deployment design decision (requires slug-based ownership model)

## LOW

### L1 - Silent transform skip on malformed decorator (commit.rs:298-305)
No warning when @ozzy.transform lacks a following def.

### L2 - list_commits only follows first parent (project.rs:620-639)
Merge commits' second-parent chains invisible. Latent until merge support.

### L3 - DAG renderers assume nodes.last() is output (dag.rs:202, Python client.py:373)
Creation order != topological order for multi-node pipelines.

### L4 - Python client missing tests for _is_local_ref
Primary routing function completely untested.
