# CLI Commands Review - Round 6

## Files Reviewed
- `crates/ozzy-cli/src/main.rs`
- `crates/ozzy-cli/src/commands/shared.rs`
- `crates/ozzy-cli/src/commands/fetch.rs`
- `crates/ozzy-cli/src/commands/data.rs`
- `crates/ozzy-cli/src/commands/collection.rs`
- `crates/ozzy-cli/src/commands/endpoint.rs`
- `crates/ozzy-cli/src/commands/secret.rs`
- `crates/ozzy-cli/src/commands/push.rs`
- `crates/ozzy-cli/src/commands/auth.rs`
- `crates/ozzy-cli/src/commands/init.rs`
- `crates/ozzy-cli/src/commands/cache.rs`
- `crates/ozzy-cli/src/commands/transform.rs`
- `crates/ozzy-cli/src/commands/mod.rs`

## Findings

### 1. [minor] `data upload` does not validate `--name` or `--collection` arguments

**File:** `crates/ozzy-cli/src/commands/data.rs`, function `upload()` (lines 56-140)

Every other data subcommand validates its `name` argument with `shared::validate_name()` before using it:
- `show()` line 190
- `describe()` line 245
- `yank()` line 320
- `download()` line 349

But `upload()` passes the optional `--name` (line 98-99) and `--collection` (line 107-109) values directly into the multipart form without calling `validate_name()`. If a user provides a name with spaces, slashes, or other invalid characters (e.g., `ozzy data upload file.csv --name "my data/set"`), the CLI would send it to the server without warning. The server likely rejects it, but the error message would be less helpful than the CLI's own validation message. More importantly, this is an inconsistency with every other command's validation pattern.

**Fix:** Add validation calls before building the form:
```rust
if let Some(n) = name {
    shared::validate_name(n, "data atom")?;
}
if let Some(c) = collection {
    shared::validate_name(c, "collection")?;
}
```

---

No other new issues found. The codebase is well-structured with consistent error handling, proper auth flows, safe string slicing throughout (`.get(..N).unwrap_or()`), and the previously reported issues have all been addressed.

CLEAN (aside from the one minor finding above).
