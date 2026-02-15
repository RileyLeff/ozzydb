# CLI Commands Code Review (ozzy-cli crate)

## Scope
Files reviewed:
- `crates/ozzy-cli/src/commands/shared.rs`
- `crates/ozzy-cli/src/commands/push.rs`
- `crates/ozzy-cli/src/commands/data.rs`
- `crates/ozzy-cli/src/commands/collection.rs`
- `crates/ozzy-cli/src/commands/endpoint.rs`
- `crates/ozzy-cli/src/commands/secret.rs`
- `crates/ozzy-cli/src/commands/fetch.rs`
- `crates/ozzy-cli/src/commands/init.rs`
- `crates/ozzy-cli/src/commands/cache.rs`
- `crates/ozzy-cli/src/main.rs`
- `crates/ozzy-cli/tests/integration_test.rs`

Cross-referenced with server-side handlers in `crates/ozzy-server/src/api/v1/`.

---

## Findings

### 1. [MAJOR] fetch.rs: Incorrect URL path for job output download

**File:** `crates/ozzy-cli/src/commands/fetch.rs`, lines 166-167 and 218

**Bug:** The job output download URL is constructed with `/v1/jobs/...` instead of `/api/v1/jobs/...`.

In the cache-hit path (line 107-121), the server returns `output_url` as `/v1/jobs/{id}/output` (see server `fetch.rs` line 146 and 227). In the poll path, the CLI constructs it identically at line 166:
```rust
let output_url = format!("/v1/jobs/{}/output", job_id);
```

Then in `download_output` at line 218:
```rust
let url = format!("{}{}", registry_url, output_path);
```

This produces `https://api.ozzydb.com/v1/jobs/{id}/output`, but the server mounts all v1 routes under `/api/v1` (see `api/mod.rs` line 27: `.nest("/api/v1", v1::router())`). The correct path is `/api/v1/jobs/{id}/output`.

Contrast with the poll URL at line 126 which correctly uses `/api/v1/`:
```rust
let poll_url = format!("{}/api/v1/jobs/{}", registry_url, job_id);
```

This means every `ozzy fetch` that completes (either via cache hit or async execution) will fail at the download step with a 404.

**Fix:** Either:
- Change CLI line 166 to `format!("/api/v1/jobs/{}/output", job_id)`
- Or change `download_output` to prepend `/api` when the path starts with `/v1/`

The server-side `output_url` in `FetchResponse` should also be updated to use `/api/v1/` prefix consistently.

---

### 2. [MAJOR] Panic on short `created_at` strings (6 locations)

**Files:** `data.rs:182,283`, `collection.rs:295,341`, `secret.rs:110,111`

**Bug:** All timestamp display code uses direct string slicing like `&atom.created_at[..10]` without bounds checking. If the server ever returns a `created_at` string shorter than 10 characters (e.g., empty string, malformed timestamp, or a different format), the CLI will panic with an index-out-of-bounds error.

Example at `data.rs:182`:
```rust
&atom.created_at[..10],
```

The server serializes `DateTime<Utc>` which always produces RFC 3339 format (at least 20 chars), so this works in practice. However, the CLI uses `String` deserialization, meaning any unexpected format from the server causes a panic rather than a graceful error.

**Fix:** Use `.get(..10).unwrap_or(&atom.created_at)` for safe slicing, matching the pattern already used for hash truncation elsewhere in the code. Alternatively, parse with `chrono` and format on the client side.

---

### 3. [MAJOR] No URL-encoding of user-supplied path segments

**Files:** `data.rs:152-153,197-199,254-255,297-298,327-329,353-355`, `collection.rs:75-76,127-128,177-178,213-214,311-312,356-357`, `secret.rs:54-55,86-87,127-128`, `endpoint.rs:78-79,128-129,207-213`

**Bug:** User-supplied names (data atom names, collection names, secret names, endpoint names) are interpolated directly into URL paths without percent-encoding. While the server validates names to `[a-zA-Z0-9_-]+`, the CLI performs no such validation before constructing URLs. If a user somehow passes a name containing `/`, `?`, `#`, or other URL-special characters (e.g., via a shell that doesn't split on these), the URL will be malformed and could route to unintended endpoints.

Example at `data.rs:197-199`:
```rust
"{}/api/v1/data/{}/{}/{}",
registry_url, project.owner, project.slug, name
```

**Fix:** Apply URL percent-encoding to all user-supplied path segments, or add client-side name validation matching the server's `[a-zA-Z0-9_-]+` pattern before making API calls.

---

### 4. [MINOR] endpoint.rs: CLI NodeDetail missing `params` field

**File:** `crates/ozzy-cli/src/commands/endpoint.rs`, lines 49-53

**Bug:** The CLI's `NodeDetail` struct only has `transform` and `machine` fields:
```rust
struct NodeDetail {
    transform: String,
    machine: Option<String>,
}
```

But the server's `NodeDetail` (in `endpoints.rs:90-95`) also returns a `params` field:
```rust
struct NodeDetail {
    transform: String,
    params: HashMap<String, serde_json::Value>,
    machine: Option<String>,
}
```

Since serde defaults to ignoring unknown fields during deserialization, this won't cause a runtime error. However, the `params` data from the server is silently discarded, meaning `ozzy endpoint show` never displays per-node parameter bindings. This is a functional gap rather than a crash.

**Fix:** Add `params: HashMap<String, serde_json::Value>` to the CLI's `NodeDetail` and display the bound parameters in the `show` output.

---

### 5. [MINOR] endpoint.rs: CLI ParamDetail missing `min`, `max`, `enum_values` fields

**File:** `crates/ozzy-cli/src/commands/endpoint.rs`, lines 39-47

**Bug:** Similar to finding #4, the CLI's `ParamDetail` omits `min`, `max`, and `enum_values` fields that the server includes in its response. These constraint fields are useful for users inspecting endpoint parameters, but are silently dropped.

Server struct includes:
```rust
min: Option<f64>,
max: Option<f64>,
#[serde(rename = "enum")]
enum_values: Option<Vec<serde_json::Value>>,
```

**Fix:** Add the missing fields and display them in `endpoint show` output.

---

### 6. [MINOR] endpoint.rs: `ref` query parameter not URL-encoded

**File:** `crates/ozzy-cli/src/commands/endpoint.rs`, lines 81-83, 131-133, 202-204

**Bug:** The `ref` query parameter is appended via string formatting without URL-encoding:
```rust
url.push_str(&format!("?ref={}", r));
```

If the ref name contains characters like `&`, `=`, `#`, or spaces, the URL will be malformed. Branch names can contain `/` (e.g., `feature/my-branch`), which is valid in the path but the `?ref=feature/my-branch` part should technically have the `/` encoded (though most servers accept it).

**Fix:** Use `urlencoding::encode(r)` or build the URL with `reqwest::Url::parse_with_params()` which handles encoding automatically.

---

### 7. [MINOR] fetch.rs: `query` params use borrowed key lifetimes from loop variable

**File:** `crates/ozzy-cli/src/commands/fetch.rs`, lines 67-77

**Bug:** The query parameter vector uses `(&str, String)`:
```rust
let mut query: Vec<(&str, String)> = Vec::new();
```

For user params, the key comes from `param.split_once('=')` which borrows from the `param` string (which is borrowed from the `params` slice). This works because `params` outlives `query`. However, the static string `"ref"` is used for the ref parameter.

The real issue is more subtle: if `key` from `param.split_once('=')` is not URL-encoded, a param like `"foo bar=123"` would produce an unencoded key `"foo bar"` in the query string. The `reqwest::RequestBuilder::query()` method does handle encoding, so this is actually fine for the query part. No action needed here -- this is a note rather than a bug.

---

### 8. [MINOR] data.rs: `upload` reads entire file into memory

**File:** `crates/ozzy-cli/src/commands/data.rs`, lines 83-84

**Bug:** The upload command reads the entire file into memory with `std::fs::read(path)` before sending it as multipart. For large data files (multi-GB parquet files), this will consume excessive memory and may OOM.

```rust
let file_bytes = std::fs::read(path)
    .with_context(|| format!("Failed to read {}", file_path))?;
```

**Fix:** Use `reqwest::multipart::Part::stream()` with a `tokio::fs::File` to stream the file instead of buffering it entirely in memory.

---

### 9. [MINOR] data.rs: `download` writes entire file into memory before writing to disk

**File:** `crates/ozzy-cli/src/commands/data.rs`, lines 380 and 396

**Bug:** Both the redirect path and direct response path buffer the entire download in memory:
```rust
let bytes = dl_resp.bytes().await?;
std::fs::write(out_path, &bytes)?;
```

For large data atoms, this has the same memory pressure issue as finding #8.

**Fix:** Stream the response body to a file using `tokio::io::copy()`.

---

### 10. [MINOR] secret.rs: Secret value visible in terminal (no TTY masking)

**File:** `crates/ozzy-cli/src/commands/secret.rs`, lines 36-41

**Bug:** The secret value is read via `stdin().read_line()` with a plain `eprint!("Enter secret value for {}: ")` prompt. On most terminals, the typed value will be visible as plaintext. For a security-sensitive operation (setting secrets), the input should be masked.

```rust
eprint!("Enter secret value for {}: ", name);
let mut value = String::new();
std::io::stdin()
    .read_line(&mut value)
    .context("Failed to read secret value")?;
```

**Fix:** Use a crate like `rpassword` to read the value without echoing it to the terminal, similar to how password prompts work.

---

### 11. [MINOR] data.rs: `_meta` parameter accepted but silently ignored

**File:** `crates/ozzy-cli/src/commands/data.rs`, line 61 and `main.rs` lines 118-119

**Bug:** The `upload` function accepts a `_meta: Option<&str>` parameter (prefixed with underscore, indicating unused), and the `DataCommands::Upload` enum has a `meta: Option<String>` field described as "Metadata sidecar TOML file". The parameter is passed through from `main.rs` but never used in the upload logic -- it's not added to the multipart form.

A user specifying `--meta metadata.toml` would get no error but the metadata would be silently dropped.

**Fix:** Either implement the metadata sidecar feature (parse the TOML file and add fields to the form), or remove the `--meta` flag and document it as not-yet-implemented to avoid user confusion.

---

### 12. [MINOR] fetch.rs: Presigned URL redirect uses same no-redirect client

**File:** `crates/ozzy-cli/src/commands/fetch.rs`, lines 237-244

**Bug:** In `download_output`, when following a presigned URL redirect, the code reuses the same `client` that was created with `redirect::Policy::none()` (line 62-63):
```rust
let bytes = client
    .get(location)
    .send()
    .await
```

If the presigned URL itself redirects (some S3-compatible services do multi-step redirects), the download will fail because the client won't follow redirects.

Contrast with `data.rs:373` where a new `http_client()` (with default redirect policy) is created for following presigned URLs.

**Fix:** Create a separate client with the default redirect policy for following presigned URLs, or use `shared::http_client()`.

---

### 13. [MINOR] collection.rs: `rm` sends different body format than `add`

**File:** `crates/ozzy-cli/src/commands/collection.rs`, lines 165-174

**Bug:** The `add` command sends members as structured objects with `member_type` and `member_ref` fields, matching the server's `AddMembersRequest`:
```rust
{"members": [{"member_type": "data", "member_ref": "readings"}]}
```

But the `rm` command validates the `type:name` format (line 165-172) then sends the raw strings:
```rust
{"refs": ["data:readings"]}
```

This matches the server's `RemoveMembersRequest` which expects `refs: Vec<String>`. However, the validation in `rm` only checks for `:` presence (line 166: `if !m.contains(':')`) but doesn't validate that the type is "data" or "collection". The `add` command doesn't validate the type either, but the server does. The concern is more about consistency: `rm` could accept invalid types like `foo:bar` without the user getting feedback until the server rejects it.

**Fix:** Add client-side validation for member_type in both `add` and `rm` to match the server's allowed types ("data", "collection").

---

### 14. [NOTE] shared.rs: `load_project_from_toml` uses relative path

**File:** `crates/ozzy-cli/src/commands/shared.rs`, line 46

**Observation:** The function reads `"ozzy.toml"` as a relative path:
```rust
std::fs::read_to_string("ozzy.toml")
```

This depends on the process's current working directory. The `main.rs` captures `cwd` at line 353 but doesn't pass it to `load_project_from_toml`. This works because the CLI binary naturally runs in the user's CWD, but it's fragile -- if any code changes the CWD before calling this function, it would break. The `init.rs` and `transform.rs` commands correctly take `cwd` as a parameter.

---

### 15. [NOTE] data.rs: `format_bytes` accepts `i64` but negative values produce odd output

**File:** `crates/ozzy-cli/src/commands/data.rs`, line 406

**Observation:** `format_bytes` takes `i64` (matching the server's `byte_size: i64` field) but doesn't handle negative values. A negative byte size (which shouldn't occur in practice) would produce output like "-1024 B" since none of the comparison thresholds match.

---

### 16. [NOTE] push.rs: `ref_name` is always `Some`

**File:** `crates/ozzy-cli/src/commands/push.rs`, lines 44-47

**Observation:** The `ref_name` is set to `Some(current_branch)` when not explicitly provided, meaning it's never `None` in the request. The `PushRequest` serialization with `skip_serializing_if = "Option::is_none"` will never trigger for `ref_name`. This is intentional behavior (always tracking the branch), but worth noting that the "no ref" path in the serialization test (line 122-134) covers a code path that doesn't occur in production.

---

### 17. [NOTE] Test gaps

**File:** `crates/ozzy-cli/tests/integration_test.rs`

**Observation:** The integration tests appropriately cover offline behavior (init, help text, auth errors, argument parsing). The following areas have no offline test coverage but could be tested:

- `data upload` with `--name` and multiple files (should fail with a helpful message)
- `collection add` with invalid member format (no `:`)
- `collection rm` with invalid member format
- `secret set` with empty input (would require stdin mocking)
- `fetch` with endpoint refs containing `@` and various formats
- `endpoint dag --format=invalid` error handling
- `cache clear` on an empty cache

Most of these require a running server for full E2E testing, which the comment at the top of the test file acknowledges.

---

## Summary

| Severity | Count | Key Issues |
|----------|-------|------------|
| Major    | 3     | Wrong URL path for job output download (fetch broken), panic on short timestamps, no URL-encoding of path segments |
| Minor    | 9     | Missing response fields, file streaming, secret masking, meta flag dead code, redirect client, member type validation |
| Note     | 4     | Relative path dependency, negative byte_size, always-Some ref, test gaps |

The most critical fix is #1 (wrong URL path for job output download), which would cause every `ozzy fetch` to fail at the download step. Fix #2 (panic on short timestamps) and #3 (no URL-encoding) are lower probability but high impact when triggered.
