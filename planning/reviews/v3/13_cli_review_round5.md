# CLI Commands Review - Round 5

## Findings

### 1. [minor] `secret set` prompt may not display before blocking on stdin

**File:** `/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-cli/src/commands/secret.rs`, line 37

The `eprint!` macro does not include a trailing newline, and Rust's stderr may be line-buffered when connected to a terminal. Without an explicit flush, the prompt `"Enter secret value for {}: "` may not appear before `read_line()` blocks waiting for input on line 39-41. The user would see a hanging terminal with no prompt.

```rust
eprint!("Enter secret value for {}: ", name);
// Missing: std::io::Write::flush(&mut std::io::stderr())?;
let mut value = String::new();
std::io::stdin()
    .read_line(&mut value)
    .context("Failed to read secret value")?;
```

Fix: add `std::io::stderr().flush()?;` (with `use std::io::Write;`) after the `eprint!` call.

### 2. [minor] `data download` does not verify content integrity

**File:** `/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-cli/src/commands/data.rs`, `download()` function (lines 348-406)

The `fetch` command verifies downloaded output against a BLAKE3 hash (Round 4 fix). However, `data download` has no hash verification at all. The data atom's hash is available from the server (the `show` response includes it), but `download` doesn't request or check it. A corrupted or tampered download would be silently accepted.

This is asymmetric: fetch verifies integrity but download does not. Since data atoms are content-addressed, the hash should be readily available. The download endpoint could return the hash in a response header, or the CLI could fetch it separately via `show` and verify after download.

### 3. [note] `init` writes unescaped values into TOML string literals

**File:** `/Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-cli/src/commands/init.rs`, lines 57-64

The `project_name`, `owner`, `provider`, and `repo` values from the git remote are interpolated directly into TOML using `format!("name = \"{}\"\n", ...)`. If any of these strings contained TOML-special characters (double quote, backslash, newline), the generated `ozzy.toml` would be malformed or contain injected keys. In practice this is very unlikely since GitHub restricts repository names to safe characters and `host_to_provider` restricts provider to known values. The output file is also user-visible and editable, so there's no security escalation path.

## Verdict

CLEAN (no must-fix issues). The two minor findings above are quality improvements rather than bugs that affect correctness in normal usage. The observation about TOML escaping is purely defensive.
