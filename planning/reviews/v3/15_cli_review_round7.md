# CLI Commands Review (Round 7)

## Scope
Reviewed: `shared.rs`, `fetch.rs`, `data.rs`, `collection.rs`, `endpoint.rs`, `secret.rs`, `push.rs`, `auth.rs`, `main.rs`, `init.rs`, `cache.rs`, `transform.rs`, and `mod.rs`.

## Findings

### 1. [minor] `fetch.rs` — `download_output` constructs URL incorrectly for cache-hit path

**File:** `crates/ozzy-cli/src/commands/fetch.rs`, lines 118-131 vs 176-177

When the fetch response comes back as `"done"` (cache hit), the `output_url` from the server response is used directly. This URL is a relative path like `/api/v1/jobs/{id}/output`. In `download_output`, the URL is constructed as `format!("{}{}", registry_url, output_path)` (line 228), which works correctly.

However, the cache-hit path passes `fetch_resp.output_url` which could potentially be an absolute URL (e.g., a presigned S3/R2 URL) depending on the server implementation. If the server ever returns a full URL as `output_url`, the concatenation `format!("{}{}", registry_url, output_path)` on line 228 would produce a malformed URL like `https://api.ozzydb.comhttps://r2.example.com/...`.

The job-polling path (line 176) always constructs a relative path locally, so it is safe. But the cache-hit path trusts the server's `output_url` field.

**Suggestion:** Check whether `output_path` is already an absolute URL before prepending `registry_url`:

```rust
let url = if output_path.starts_with("http://") || output_path.starts_with("https://") {
    output_path.to_string()
} else {
    format!("{}{}", registry_url, output_path)
};
```

### 2. [minor] `cache.rs` — `dir_size` follows symlinks, potential infinite loop

**File:** `crates/ozzy-cli/src/commands/cache.rs`, lines 112-127

`dir_size` uses `entry.file_type()?` which follows symlinks. If a symlink in the cache directory creates a cycle, this function could recurse infinitely (until stack overflow). While the cache directory is controlled by the application, a malicious or corrupted cache entry with a symlink loop would crash `ozzy cache ls` or `ozzy cache size`.

**Suggestion:** Use `entry.metadata()` -> check `is_symlink()` and skip, or use `entry.file_type()` from `DirEntry` which does NOT follow symlinks on most platforms (this is actually correct -- `DirEntry::file_type()` is `lstat`-equivalent). Actually, looking more carefully, `DirEntry::file_type()` does not follow symlinks, but `is_file()` on a `Path` does. The `path.is_file()` check on line 114 follows symlinks. If `path` is a symlink to a directory, `is_file()` returns false and it falls through to `read_dir`, potentially following a symlink loop. Consider adding a symlink check: `if path.is_symlink() { return Ok(0); }`.

### 3. [note] `auth.rs` — `load_credentials` silently swallows parse errors

**File:** `crates/ozzy-cli/src/commands/auth.rs`, lines 44-50

When `serde_json::from_str` fails, the function returns `Ok(None)` (treating it as "not logged in"). This means if the credentials file is corrupted (not just a v1 format, but actual corruption), the user gets no warning and is told they're not logged in. Adding an `eprintln!` warning when the file exists but fails to parse would improve debuggability.

### 4. [note] `push.rs` — `current_git_branch` returns "HEAD" for detached HEAD

**File:** `crates/ozzy-cli/src/commands/shared.rs`, line 163 / `push.rs`, line 46

When `ref_name` is `None` (the default), push defaults to `current_git_branch()`. On a detached HEAD, `git rev-parse --abbrev-ref HEAD` returns the literal string `"HEAD"`. This would push with `ref_name: Some("HEAD")`, which might not be what the user expects, and could create a ref named "HEAD" on the server side. Consider warning or erroring when the branch is "HEAD" (detached state).

---

## Verdict

CLEAN

The codebase is in good shape after 6 rounds of fixes. The findings above are minor/notes rather than bugs or security issues. No new major or critical issues found.
