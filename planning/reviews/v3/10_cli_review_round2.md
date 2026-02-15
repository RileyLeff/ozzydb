## Review Round 2 -- CLI Commands

### 1. [minor] endpoint.rs: Query parameters not URL-encoded (manual string concatenation)

**Files:** `crates/ozzy-cli/src/commands/endpoint.rs` lines 88, 139, 223-234

The `ls`, `show`, and `dag` functions build query strings by manual string concatenation without URL-encoding:

```rust
// endpoint.rs:88
url.push_str(&format!("?ref={}", r));

// endpoint.rs:139
url.push_str(&format!("?ref={}", r));

// endpoint.rs:223-225
let mut query_parts = vec![format!("format={}", format)];
if let Some(r) = ref_name {
    query_parts.push(format!("ref={}", r));
}
```

If `ref_name` or `format` contains characters like `&`, `=`, `#`, or spaces, the resulting URL will be malformed. For example, `--ref "v1&admin=true"` would inject an extra query parameter.

Compare with `fetch.rs` line 85 which correctly uses `request.query(&query)` for automatic encoding via reqwest.

**Fix:** Use reqwest's `.query()` method instead of manual string concatenation. For example:
```rust
let mut request = client.get(&url);
if let Some(r) = ref_name {
    request = request.query(&[("ref", r)]);
}
```

---

### 2. [minor] fetch.rs: No validation of owner/project/endpoint parsed from user input

**File:** `crates/ozzy-cli/src/commands/fetch.rs` lines 42-49, 80-83

The `owner`, `project`, and `ep_name` values parsed from user input are interpolated directly into the URL path without any `validate_name()` call:

```rust
let parts: Vec<&str> = path.splitn(3, '/').collect();
let (owner, project, ep_name) = (parts[0], parts[1], parts[2]);
// ...
let fetch_url = format!(
    "{}/api/v1/fetch/{}/{}/{}",
    registry_url, owner, project, ep_name,
);
```

Since `splitn(3, '/')` is used, `ep_name` could contain additional slashes (e.g., `owner/project/ep/../../admin`). While the server would reject invalid paths, client-side validation via `validate_name` would give a cleaner error message and prevent malformed requests.

**Fix:** Validate all three components after parsing:
```rust
shared::validate_name(owner, "owner")?;
shared::validate_name(project, "project")?;
shared::validate_name(ep_name, "endpoint")?;
```

Note: `shared` is not currently imported in `fetch.rs` (it uses `super::auth::load_credentials` directly), so importing the module would also be needed.

---

### 3. [minor] auth.rs: Token name in URL path not validated in `token_revoke`

**File:** `crates/ozzy-cli/src/commands/auth.rs` line 413

The token name is directly interpolated into the URL path without validation:

```rust
.delete(format!("{}/api/v1/auth/token/{}", creds.registry_url, name))
```

While token names are validated at creation time on the server (alphanumeric + underscore + dash), the CLI `token_revoke` function does not validate the name before constructing the URL. A user typing a name with `/` or `?` would produce a malformed URL and a confusing error.

**Fix:** Add `validate_name`-style validation or reuse the existing function from `shared.rs`.

---

### 4. [note] data.rs: `upload` reads entire file into memory

**File:** `crates/ozzy-cli/src/commands/data.rs` lines 82-83

```rust
let file_bytes = std::fs::read(path)
    .with_context(|| format!("Failed to read {}", file_path))?;
```

The upload function reads the entire file into memory before building the multipart form. For very large data files (multi-GB), this will exhaust memory. reqwest supports streaming uploads via `reqwest::Body::wrap_stream()` or `reqwest::multipart::Part::stream()` with a `tokio::fs::File`, which would allow uploading files of arbitrary size.

This is an architectural limitation, not a correctness bug, and is fine for the typical use case (data atoms under ~1 GB).

---

### 5. [note] data.rs: `format_bytes` accepts `i64` but does not handle negative values

**File:** `crates/ozzy-cli/src/commands/data.rs` lines 409-419

```rust
fn format_bytes(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    }
    // ...
}
```

If the server ever returns a negative `byte_size` (e.g., due to a bug), this would display `-5 B`. Unlikely in practice since PostgreSQL `bigint` maps to `i64` and the server stores file sizes. Low risk.

---

### 6. [note] Inconsistency: `fetch.rs` uses `load_credentials` directly while other commands use `shared::require_auth`

**File:** `crates/ozzy-cli/src/commands/fetch.rs` line 9, 52

`fetch.rs` imports and calls `load_credentials()` directly from `auth`, while all other commands that need auth (`push.rs`, `data.rs`, `collection.rs`, `endpoint.rs`, `secret.rs`) go through `shared::require_auth()`. This is intentional because fetch allows unauthenticated access to public projects. No functional issue, just worth noting the different code path.

---

CLEAN on major issues. Three minor findings (1-3) and three notes (4-6). No re-reports of the 6 already-fixed items.
