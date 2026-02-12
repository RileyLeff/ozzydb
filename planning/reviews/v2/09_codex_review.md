# Codex Review Round 2 — Phase 2 Post-Fix Verification

Session ID: `019c53ae-9dc1-7a13-9305-1fb1bd5b5b5c`

## Findings

### MAJOR 1: ref_count drift on failed uploads
`data.rs` increments `content_refs.ref_count` before inserting `data_atoms`. If atom insert fails (e.g., duplicate name), ref_count is still incremented.

### MAJOR 2: Collection cycle TOCTOU
Cycle detection done before transaction; atomic add only locks the target collection row. Two concurrent ops can produce A<->B cycles.

### MAJOR 3: Yanked collection immutability TOCTOU
`yanked` checked pre-transaction, but atomic add/remove methods don't re-check under lock. Concurrent yank + mutation can create new versions after yank.

### MINOR 4: verify_content_hash ignored for remote reads
`from_config_with_prefix` disables verification, but remote `get()` always verifies. Breaks key-addressed stores when remote is enabled.

### MINOR 5: Flatten path omits immediate collection
Leaf emission uses parent `path` directly, missing the current collection name in the nesting path.

### NOTE 6: Testing gap
Integration tests don't exercise secret list auth hardening, invalid remove prefix handling, endpoint-member rejection, or upload+collection race cases.

## Verification of Round 1 Fixes
All 8 prior fixes confirmed correct.
