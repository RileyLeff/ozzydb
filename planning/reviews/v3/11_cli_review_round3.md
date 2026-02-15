## Review Round 3 — CLI Commands

### 1. `load_project_from_toml()` does not validate owner/slug names (minor)

**File:** `/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-cli/src/commands/shared.rs`, lines 44-85

`load_project_from_toml()` reads `project.owner` and `project.name` from `ozzy.toml` and returns them without running `validate_name()`. These values are then interpolated into URL paths throughout the CLI:

```rust
// data.rs line 151
format!("{}/api/v1/data/{}/{}", registry_url, project.owner, project.slug)

// collection.rs line 76
format!("{}/api/v1/collections/{}/{}", registry_url, project.owner, project.slug)

// endpoint.rs line 84
format!("{}/api/v1/endpoints/{}/{}", registry_url, project.owner, project.slug)

// secret.rs line 55
format!("{}/api/v1/secrets/{}/{}", registry_url, project.owner, project.slug)
```

A malformed `ozzy.toml` with `owner = "a/../admin"` or `name = "foo bar"` would produce malformed or unexpected URLs. The server would likely reject them, but adding `validate_name(&owner, "owner")?; validate_name(&name, "project name")?;` inside `load_project_from_toml()` would provide earlier, clearer error messages and a defense-in-depth layer against URL path manipulation.

---

### 2. `collection add` does not validate member type or member ref (minor)

**File:** `/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-cli/src/commands/collection.rs`, lines 109-123

After splitting on `:`, neither `mtype` nor `mref` are validated:

```rust
let (mtype, mref) = m.split_once(':').ok_or_else(|| { ... })?;
Ok(serde_json::json!({
    "member_type": mtype,
    "member_ref": mref,
}))
```

`mtype` could be any arbitrary string (including empty if the input is `:foo`), and `mref` is not passed through `validate_name()`. The server should reject invalid values, but a client-side check like `validate_name(mref, "member reference")?` and validating `mtype` is one of `"data"`, `"collection"` would catch user mistakes immediately with a clear error message instead of a generic server error.

Similarly, `collection rm` (line 169) uses `m.contains(':')` which would accept `:` (empty type, empty ref) or `:::` as valid.

---

### 3. `secret set` does not mask stdin input (note)

**File:** `/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-cli/src/commands/secret.rs`, lines 37-41

The secret value is read via `std::io::stdin().read_line()` with `eprint!("Enter secret value for {}: ", name)` as a prompt. The typed value is visible on the terminal. Standard practice for secret input is to disable terminal echo (e.g., using the `rpassword` crate or equivalent). This is a UX improvement, not a security vulnerability per se, since the user is deliberately entering the value into their own terminal.

---

### 4. `data upload` reads entire file into memory (note)

**File:** `/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-cli/src/commands/data.rs`, lines 82-83

```rust
let file_bytes = std::fs::read(path)
    .with_context(|| format!("Failed to read {}", file_path))?;
```

For very large data files (multi-GB), this loads the entire file into memory before uploading. `reqwest::multipart` supports streaming from a file (via `reqwest::Body::wrap_stream` or `Part::stream`), which would handle arbitrarily large files without requiring proportional memory. This is an optimization concern for large datasets, not a correctness bug.

---

### 5. `fetch` user params can shadow the `ref` query parameter (note)

**File:** `/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-cli/src/commands/fetch.rs`, lines 70-81

```rust
let mut query: Vec<(&str, String)> = Vec::new();
if let Some(r) = git_ref {
    query.push(("ref", r.to_string()));
}
for param in params {
    if let Some((key, value)) = param.split_once('=') {
        query.push((key, value.to_string()));
    }
    ...
}
```

A user passing `--param ref=malicious` would add a second `ref` entry to the query string. The server's behavior with duplicate query parameters depends on its framework (Axum typically takes the first or last occurrence). This could cause confusion rather than a security issue, but checking that user param keys do not collide with reserved keys (`ref`) would be defensive.

---

No critical or major issues found. All 5 findings are minor or note-level.
