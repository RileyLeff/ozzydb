# CLI Commands Review (Round 4)

Reviewed files: `crates/ozzy-cli/src/commands/{shared.rs, fetch.rs, data.rs, collection.rs, endpoint.rs, secret.rs, push.rs, auth.rs}` plus `init.rs`, `transform.rs`, `cache.rs`, and `main.rs` for context.

---

## Findings

### 1. [minor] `fetch.rs` — `download_output` does not verify hash of downloaded content

**File:** `crates/ozzy-cli/src/commands/fetch.rs`, function `download_output` (lines 214-265) and `write_output` (lines 268-282)

The server provides an `output_hash` alongside the download URL. The `write_output` function receives this hash but only prints it to stderr as informational output (`eprintln!("Hash: {}", h)`). It never actually verifies the downloaded bytes against this hash.

For a system built on content-addressed storage with BLAKE3, this is a missed opportunity to detect data corruption or tampering during download. The `data download` command has the same gap -- no hash verification on the downloaded bytes.

Compare this with the server-side code which was previously fixed to verify BLAKE3 hashes on `get_stream()` (Review 19 M2). The client side does not perform the equivalent check.

**Suggested fix:** After downloading bytes, compute `blake3::hash(&bytes)` and compare to the expected hash. Bail if they don't match.

---

### 2. [minor] `secret.rs` — Secret value sent in JSON body over the wire without transport-layer note

**File:** `crates/ozzy-cli/src/commands/secret.rs`, function `set` (lines 29-76)

The secret value is read from stdin and sent in a JSON body (`{"name": ..., "value": ...}`) via `POST`. This is fine if the registry_url uses HTTPS, but the code does not verify that the URL scheme is HTTPS before transmitting secret material. If a user has configured a registry URL with `http://` (e.g., for local dev, or if `ozzy.toml` has `[remote] url = "http://..."`) the secret is transmitted in plaintext.

While the default is `https://api.ozzydb.com`, the `get_registry_url()` function in `auth.rs` happily accepts any URL from `ozzy.toml`. The same applies to `auth.rs` login which sends the auth token over whatever scheme is configured.

**Suggested fix:** At minimum, emit a warning when the registry URL uses `http://` and a secret or credential is being transmitted. Or refuse to send secrets over non-HTTPS connections.

---

### 3. [minor] `auth.rs` — `token_create` does not validate `scope` input

**File:** `crates/ozzy-cli/src/commands/auth.rs`, function `token_create` (lines 305-350)

The `scope` parameter accepts arbitrary strings and sends them directly to the server. The server presumably validates this, but a client-side check would provide a better UX with an immediate error message rather than a round-trip. The scope should be either `"account"` or `"project:owner/slug"`. There is no validation that it matches either pattern. The `name` parameter is also not validated with `validate_name()` here (though `token_revoke` does validate the name).

**Suggested fix:** Validate `name` with `validate_name()` in `token_create`, and validate that `scope` is either `"account"` or matches `"project:VALID_NAME/VALID_NAME"`.

---

### 4. [minor] `fetch.rs` — No validation on `git_ref` portion of the endpoint reference

**File:** `crates/ozzy-cli/src/commands/fetch.rs`, function `run` (lines 31-192)

When parsing `owner/project/endpoint[@ref]`, the `owner`, `project`, and `ep_name` parts are each validated with `validate_name()`. However, the `git_ref` part (after the `@`) is passed through without any validation. While refs can contain characters like `.` and `/` that `validate_name` would reject, there should still be some sanitization -- for example, the ref should not be empty (e.g., `user/proj/ep@` would produce `git_ref = Some("")`), and should not contain newlines or other control characters that could cause issues in query parameters.

**Suggested fix:** At minimum, check that `git_ref` is non-empty when present. Consider also rejecting control characters and excessively long refs.

---

### 5. [minor] `endpoint.rs` — `dag` format parameter is not validated

**File:** `crates/ozzy-cli/src/commands/endpoint.rs`, function `dag` (lines 212-245)

The `format` parameter is passed through to the server query string without validation. The server likely only supports `ascii`, `mermaid`, `json`, `svg`, etc. Sending an unsupported format results in a server-side error and round-trip. A client-side validation (or at least constraining it via clap's `value_parser` or `PossibleValue`) would give users immediate feedback.

**Suggested fix:** Use clap's `#[arg(value_parser = ["ascii", "mermaid", "json", "svg"])]` or validate in the `dag` function before making the request.

---

### 6. [note] `data.rs` — `download` writes to a path derived from the data atom name

**File:** `crates/ozzy-cli/src/commands/data.rs`, function `download` (lines 348-406)

When no `--output` flag is provided, the output path defaults to `name` (line 386: `let out_path = output.unwrap_or(name);`). Since `name` is validated with `validate_name()` (alphanumeric, underscores, hyphens only), this is safe from path traversal. However, the resulting file has no extension, which may confuse users. For example, `ozzy data download readings` creates a file called `readings` with no `.csv` or `.parquet` extension. The `DataAtomDetail` response includes `content_type` which could be used to append an appropriate extension, but this information is not available in the download flow.

This is a UX observation, not a bug.

---

### 7. [note] `collection.rs` — `rm` sends raw member strings as `refs` to server

**File:** `crates/ozzy-cli/src/commands/collection.rs`, function `rm` (lines 164-219)

The `rm` function validates each member format client-side (lines 176-191), then sends the raw `members` strings (e.g., `["data:readings", "collection:train"]`) as the `refs` field in the JSON body (line 193: `let body = serde_json::json!({ "refs": members });`). Note that the `add` function sends parsed `{member_type, member_ref}` objects, while `rm` sends the raw `"type:name"` strings. This inconsistency means the server must handle two different input formats for what is conceptually the same data. If the server's remove endpoint expects the parsed object format (like `add`), the remove will silently fail or error.

This depends on the server API contract -- worth verifying that the server's remove endpoint actually accepts the `"type:name"` string format.

---

### 8. [note] `shared.rs` — `load_project_from_toml` uses relative path "ozzy.toml"

**File:** `crates/ozzy-cli/src/commands/shared.rs`, function `load_project_from_toml` (line 46)

The function reads `"ozzy.toml"` as a relative path, which means it depends on the process's current working directory. The main function sets `cwd = std::env::current_dir()` but does not `chdir` to it -- the process CWD is whatever the shell had when invoking `ozzy`. For `init` and `transform scaffold`, the `cwd` is explicitly passed as a parameter. For all other commands, the implicit reliance on CWD is consistent (they all call `load_project_from_toml()` which reads from CWD), so this is not a bug per se, but it's a different pattern from the `init`/`transform` commands that accept explicit paths.

This is an architectural observation, not a bug.

---

## Summary

No critical/major issues found. The codebase is clean and well-structured after 3 prior rounds of fixes. The findings above are all minor or observational:

| # | Severity | File | Issue |
|---|----------|------|-------|
| 1 | minor | fetch.rs | Downloaded output hash not verified against BLAKE3 |
| 2 | minor | secret.rs | Secret value transmitted without checking for HTTPS |
| 3 | minor | auth.rs | `token_create` does not validate `name` or `scope` client-side |
| 4 | minor | fetch.rs | `git_ref` portion not validated (could be empty string) |
| 5 | minor | endpoint.rs | `dag` format parameter not validated client-side |
| 6 | note | data.rs | Download default filename has no extension |
| 7 | note | collection.rs | `rm` sends raw strings vs `add` sends parsed objects (inconsistent API shape) |
| 8 | note | shared.rs | Implicit CWD dependency vs explicit path parameter pattern |
