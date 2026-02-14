//! Integration tests for OzzyDB CLI (v3).
//!
//! v3 commands that talk to the registry (push, data, collection, endpoint,
//! secret, fetch) require a running server, so they're tested in the server's
//! E2E test suite. These tests cover offline behaviour: init, help text,
//! error messages for missing config, and argument parsing.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

fn ozzy() -> Command {
    Command::cargo_bin("ozzy").unwrap()
}

// ── Init ────────────────────────────────────────────────────────

#[test]
fn test_init_project() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created ozzy.toml"));

    assert!(dir.path().join("ozzy.toml").exists());
    assert!(dir.path().join("transforms").exists());
    let toml = fs::read_to_string(dir.path().join("ozzy.toml")).unwrap();
    assert!(toml.contains("[project]"));
    assert!(toml.contains("[remote]"));
    let gitignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".ozzy/"));
    assert!(gitignore.contains("data/*.parquet"));
}

#[test]
fn test_init_already_exists() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("already initialized"));
}

// ── Help text ───────────────────────────────────────────────────

#[test]
fn test_help_output() {
    ozzy()
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Data management platform"));
}

#[test]
fn test_data_help() {
    ozzy()
        .args(["data", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("upload"))
        .stdout(predicate::str::contains("ls"))
        .stdout(predicate::str::contains("show"))
        .stdout(predicate::str::contains("yank"))
        .stdout(predicate::str::contains("download"));
}

#[test]
fn test_collection_help() {
    ozzy()
        .args(["collection", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("create"))
        .stdout(predicate::str::contains("add"))
        .stdout(predicate::str::contains("rm"))
        .stdout(predicate::str::contains("ls"))
        .stdout(predicate::str::contains("log"))
        .stdout(predicate::str::contains("flatten"));
}

#[test]
fn test_endpoint_help() {
    ozzy()
        .args(["endpoint", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ls"))
        .stdout(predicate::str::contains("show"))
        .stdout(predicate::str::contains("dag"));
}

#[test]
fn test_secret_help() {
    ozzy()
        .args(["secret", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("set"))
        .stdout(predicate::str::contains("ls"))
        .stdout(predicate::str::contains("rm"));
}

#[test]
fn test_push_help() {
    ozzy()
        .args(["push", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Push current commit to registry"));
}

#[test]
fn test_fetch_help() {
    ozzy()
        .args(["fetch", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fetch and execute a remote endpoint"));
}

// ── Error handling (no auth / no ozzy.toml) ─────────────────────

#[test]
fn test_push_requires_auth() {
    let dir = tempdir().unwrap();

    // Create minimal ozzy.toml
    fs::write(
        dir.path().join("ozzy.toml"),
        "[project]\nname = \"test\"\nowner = \"alice\"\n[git]\nprovider = \"github\"\nrepo = \"alice/test\"\n[remote]\nurl = \"http://localhost:9999\"\n",
    )
    .unwrap();

    // Push should fail because not logged in (no credentials file)
    ozzy()
        .current_dir(dir.path())
        .env("HOME", dir.path()) // Isolate credential lookup
        .args(["push"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in"));
}

#[test]
fn test_data_ls_requires_auth() {
    let dir = tempdir().unwrap();

    fs::write(
        dir.path().join("ozzy.toml"),
        "[project]\nname = \"test\"\nowner = \"alice\"\n[git]\nprovider = \"github\"\nrepo = \"alice/test\"\n[remote]\nurl = \"http://localhost:9999\"\n",
    )
    .unwrap();

    ozzy()
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .args(["data", "ls"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in"));
}

#[test]
fn test_data_upload_requires_file() {
    ozzy()
        .args(["data", "upload"])
        .assert()
        .failure();
}

#[test]
fn test_push_requires_ozzy_toml() {
    let dir = tempdir().unwrap();

    // Create fake credentials so auth passes.
    // On macOS, dirs::config_dir() → $HOME/Library/Application Support
    // On Linux, dirs::config_dir() → $HOME/.config
    let config_dir = if cfg!(target_os = "macos") {
        dir.path().join("Library/Application Support/ozzy")
    } else {
        dir.path().join(".config/ozzy")
    };
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("credentials.json"),
        r#"{"token":"fake","registry_url":"http://localhost:9999"}"#,
    )
    .unwrap();

    // Push should fail because no ozzy.toml
    ozzy()
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .args(["push"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("ozzy.toml"));
}

// ── Transform scaffold ──────────────────────────────────────────

#[test]
fn test_transform_scaffold() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["transform", "scaffold", "my_transform"])
        .assert()
        .success();

    assert!(dir.path().join("transforms/my_transform.py").exists());
}

// ── Cache (offline) ─────────────────────────────────────────────

#[test]
fn test_cache_operations() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["cache", "size"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cache size"));

    ozzy()
        .current_dir(dir.path())
        .args(["cache", "ls"])
        .assert()
        .success();
}

// ── Fetch argument parsing ──────────────────────────────────────

#[test]
fn test_fetch_rejects_bad_endpoint_ref() {
    ozzy()
        .args(["fetch", "just-a-name"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Expected: owner/project/endpoint"));
}
