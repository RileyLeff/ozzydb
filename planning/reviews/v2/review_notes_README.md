# Review Notes — v2

Persistent notes on design decisions and intentional tradeoffs. Future reviewers: check here before flagging something as a bug.

## Intentional Design Decisions

### Rule 11 (content type compatibility) is a runtime check
Content type compatibility between edge sources (`data:`, `collection:`, `endpoint:`) and transform inputs can't be validated at TOML parse time because it requires DB lookups for data atom types. It will be validated at fetch/run time in Phase 4.

### Endpoint param types are strings, validated at runtime
Endpoint `type_` is a string field, not an enum. Consumer parameter validation (type checking, min/max/enum enforcement) happens at the fetch endpoint, not at TOML parse time.

### DB tests skip without DATABASE_URL
This is intentional — they need real Postgres. CI must set DATABASE_URL.

### Cross-project integrity enforced via composite FKs
Tables that reference both `project_id` and `commit_id` (refs, endpoint_yanks, materialized_cache) use composite FKs to `commits(id, project_id)`, preventing cross-project references at the DB constraint level.

### Collection members use set semantics
`collection_members` has `UNIQUE (collection_version_id, member_hash)` to prevent duplicate members per version. `collection_hash()` also deduplicates defensively. Ordinals provide deterministic ordering for display.

### set_secret.created flag is cosmetically race-prone
The `created` field in `SetSecretResponse` is derived from a pre-upsert existence check. Two concurrent `set_secret` calls for the same name could both report `created: true`. The actual upsert is atomic and correct — only the informational response field may be misleading. Not worth adding a separate transaction for a cosmetic field.

### Endpoint collection members deferred
Endpoint members in collections are rejected in Phase 2 with a clear error message. They require materialized hash resolution at execution time, which depends on compute pipeline work in Phase 4. When implemented, `resolve_member_hash` will need to look up the endpoint's latest materialized hash.

### Collection member hashes are point-in-time snapshots
When adding a collection-type member, the stored `member_hash` reflects the child collection's hash at the time of addition. If the child is subsequently updated, the parent's stored hash becomes "stale" — this is intentional. Content-addressed systems record the state at the point of reference. The parent must be explicitly updated (re-add the member) to pick up the child's new hash.

### Upload + collection add are separate operations
Data atom upload and optional collection-add are two separate DB operations. If the collection is yanked between upload and collection-add, the atom persists (it's valid data) and the user gets a 410. This is acceptable — the atom exists and is usable independently. Full transactional atomicity across upload + collection-add would require wrapping storage writes + two different DB operations in a single transaction, adding complexity for minimal benefit.

### Streaming reads don't verify content hash
`get_stream()` remote branch returns the stream without hash verification. Hash verification requires consuming the entire stream first, which defeats the purpose of streaming. A hash-verifying stream wrapper could be added in the future if needed, but the primary use case (serving large files) benefits from streaming without full buffering.
