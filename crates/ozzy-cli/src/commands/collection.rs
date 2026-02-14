//! `ozzy collection` — manage collections (versioned sets of data references).

use anyhow::{Result, bail};
use serde::Deserialize;

use super::shared;

// ── Response types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CollectionInfo {
    name: String,
    yanked: bool,
    created_at: String,
    latest_version: Option<i32>,
    member_count: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct CollectionDetail {
    name: String,
    yanked: bool,
    yank_reason: Option<String>,
    created_at: String,
    version: Option<VersionDetail>,
}

#[derive(Debug, Deserialize)]
struct VersionDetail {
    version_number: i32,
    hash: String,
    created_by: String,
    #[allow(dead_code)]
    created_at: String,
    members: Vec<MemberInfo>,
}

#[derive(Debug, Deserialize)]
struct MemberInfo {
    member_type: String,
    member_ref: String,
    member_hash: String,
    #[allow(dead_code)]
    ordinal: i32,
}

#[derive(Debug, Deserialize)]
struct VersionLogEntry {
    version_number: i32,
    hash: String,
    created_by: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct FlattenedAtom {
    name: String,
    hash: String,
    path: Vec<String>,
}

// ── Commands ────────────────────────────────────────────────────

/// Create a new collection.
pub async fn create(name: &str) -> Result<()> {
    shared::validate_name(name, "collection")?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    let body = serde_json::json!({ "name": name });

    let resp = client
        .post(format!(
            "{}/api/v1/collections/{}/{}",
            registry_url, project.owner, project.slug
        ))
        .bearer_auth(&creds.token)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to create collection: {}", err);
    }

    let info: CollectionInfo = resp.json().await?;
    println!("Created collection '{}'.", info.name);

    Ok(())
}

/// Add members to a collection.
///
/// Members are specified as "type:name" (e.g., "data:readings", "collection:train-set").
pub async fn add(name: &str, members: &[String]) -> Result<()> {
    shared::validate_name(name, "collection")?;
    if members.is_empty() {
        bail!("No members specified. Usage: ozzy collection add <name> data:x [collection:y ...]");
    }

    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    let parsed: Result<Vec<serde_json::Value>> = members
        .iter()
        .map(|m| {
            let (mtype, mref) = m.split_once(':').ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid member format '{}': use 'type:name' (e.g., 'data:readings')",
                    m
                )
            })?;
            if mtype != "data" && mtype != "collection" {
                bail!(
                    "Invalid member type '{}' in '{}': must be 'data' or 'collection'",
                    mtype,
                    m
                );
            }
            shared::validate_name(mref, "member reference")?;
            Ok(serde_json::json!({
                "member_type": mtype,
                "member_ref": mref,
            }))
        })
        .collect();

    let body = serde_json::json!({ "members": parsed? });

    let resp = client
        .post(format!(
            "{}/api/v1/collections/{}/{}/{}/add",
            registry_url, project.owner, project.slug, name
        ))
        .bearer_auth(&creds.token)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to add members to '{}': {}", name, err);
    }

    let ver: VersionDetail = resp.json().await?;
    println!(
        "Updated collection '{}' (v{}, {} members).",
        name,
        ver.version_number,
        ver.members.len()
    );

    Ok(())
}

/// Remove members from a collection.
///
/// Members are specified as "type:name" (e.g., "data:readings").
pub async fn rm(name: &str, members: &[String]) -> Result<()> {
    shared::validate_name(name, "collection")?;
    if members.is_empty() {
        bail!("No members specified. Usage: ozzy collection rm <name> data:x [collection:y ...]");
    }

    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    // Validate format
    for m in members {
        let (mtype, mref) = m.split_once(':').ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid member format '{}': use 'type:name' (e.g., 'data:readings')",
                m
            )
        })?;
        if mtype != "data" && mtype != "collection" {
            bail!(
                "Invalid member type '{}' in '{}': must be 'data' or 'collection'",
                mtype,
                m
            );
        }
        shared::validate_name(mref, "member reference")?;
    }

    let body = serde_json::json!({ "refs": members });

    let resp = client
        .post(format!(
            "{}/api/v1/collections/{}/{}/{}/remove",
            registry_url, project.owner, project.slug, name
        ))
        .bearer_auth(&creds.token)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to remove members from '{}': {}", name, err);
    }

    let ver: VersionDetail = resp.json().await?;
    println!(
        "Updated collection '{}' (v{}, {} members).",
        name,
        ver.version_number,
        ver.members.len()
    );

    Ok(())
}

/// List collections or show a specific collection's members.
pub async fn ls(name: Option<&str>) -> Result<()> {
    if let Some(n) = name {
        shared::validate_name(n, "collection")?;
    }
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    if let Some(name) = name {
        // Show specific collection detail
        let resp = client
            .get(format!(
                "{}/api/v1/collections/{}/{}/{}",
                registry_url, project.owner, project.slug, name
            ))
            .bearer_auth(&creds.token)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = shared::extract_error(resp).await;
            bail!("Failed to get collection '{}': {}", name, err);
        }

        let detail: CollectionDetail = resp.json().await?;

        println!("Collection: {}", detail.name);
        println!("Created:    {}", detail.created_at);

        if detail.yanked {
            println!("Status:     YANKED");
            if let Some(reason) = &detail.yank_reason {
                println!("Reason:     {}", reason);
            }
        }

        if let Some(ver) = &detail.version {
            println!("Version:    {} (by {})", ver.version_number, ver.created_by);
            println!("Hash:       {}", ver.hash.get(..12).unwrap_or(&ver.hash));

            if ver.members.is_empty() {
                println!("\n  (empty)");
            } else {
                println!("\nMembers:");
                for m in &ver.members {
                    println!(
                        "  {}:{} ({})",
                        m.member_type,
                        m.member_ref,
                        m.member_hash.get(..12).unwrap_or(&m.member_hash)
                    );
                }
            }
        } else {
            println!("\n  (no versions yet)");
        }
    } else {
        // List all collections
        let resp = client
            .get(format!(
                "{}/api/v1/collections/{}/{}",
                registry_url, project.owner, project.slug
            ))
            .bearer_auth(&creds.token)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = shared::extract_error(resp).await;
            bail!("Failed to list collections: {}", err);
        }

        let colls: Vec<CollectionInfo> = resp.json().await?;

        if colls.is_empty() {
            println!("No collections. Create one with `ozzy collection create <name>`.");
            return Ok(());
        }

        println!(
            "{:<24} {:<8} {:<10} {:<8} {}",
            "NAME", "VERSION", "MEMBERS", "YANKED", "CREATED"
        );
        for c in &colls {
            println!(
                "{:<24} {:<8} {:<10} {:<8} {}",
                c.name,
                c.latest_version
                    .map(|v| format!("v{}", v))
                    .unwrap_or_else(|| "-".to_string()),
                c.member_count
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                if c.yanked { "yes" } else { "" },
                c.created_at.get(..10).unwrap_or(&c.created_at),
            );
        }
    }

    Ok(())
}

/// Show collection version history.
pub async fn log(name: &str) -> Result<()> {
    shared::validate_name(name, "collection")?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    let resp = client
        .get(format!(
            "{}/api/v1/collections/{}/{}/{}/log",
            registry_url, project.owner, project.slug, name
        ))
        .bearer_auth(&creds.token)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to get log for '{}': {}", name, err);
    }

    let entries: Vec<VersionLogEntry> = resp.json().await?;

    if entries.is_empty() {
        println!("No versions for collection '{}'.", name);
        return Ok(());
    }

    println!(
        "{:<8} {:<14} {:<16} {}",
        "VERSION", "HASH", "AUTHOR", "DATE"
    );
    for e in &entries {
        println!(
            "v{:<7} {:<14} {:<16} {}",
            e.version_number,
            e.hash.get(..12).unwrap_or(&e.hash),
            e.created_by,
            e.created_at.get(..10).unwrap_or(&e.created_at),
        );
    }

    Ok(())
}

/// Show all leaf-level data atoms in a collection (recursive flatten).
pub async fn flatten(name: &str) -> Result<()> {
    shared::validate_name(name, "collection")?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    let resp = client
        .get(format!(
            "{}/api/v1/collections/{}/{}/{}/flatten",
            registry_url, project.owner, project.slug, name
        ))
        .bearer_auth(&creds.token)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to flatten '{}': {}", name, err);
    }

    let atoms: Vec<FlattenedAtom> = resp.json().await?;

    if atoms.is_empty() {
        println!("No leaf atoms in collection '{}'.", name);
        return Ok(());
    }

    println!("{:<24} {:<14} {}", "NAME", "HASH", "PATH");
    for a in &atoms {
        println!(
            "{:<24} {:<14} {}",
            a.name,
            a.hash.get(..12).unwrap_or(&a.hash),
            a.path.join("/"),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collection_info_deserialization() {
        let json = serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000000",
            "name": "train-set",
            "yanked": false,
            "created_at": "2025-01-01T00:00:00Z",
            "latest_version": 2,
            "member_count": 5,
        });
        let info: CollectionInfo = serde_json::from_value(json).unwrap();
        assert_eq!(info.name, "train-set");
        assert_eq!(info.latest_version, Some(2));
        assert_eq!(info.member_count, Some(5));
    }

    #[test]
    fn test_version_detail_deserialization() {
        let json = serde_json::json!({
            "version_number": 1,
            "hash": "abc123",
            "created_by": "alice",
            "created_at": "2025-01-01T00:00:00Z",
            "members": [{
                "member_type": "data",
                "member_ref": "readings",
                "member_hash": "def456",
                "ordinal": 0,
            }],
        });
        let ver: VersionDetail = serde_json::from_value(json).unwrap();
        assert_eq!(ver.version_number, 1);
        assert_eq!(ver.members.len(), 1);
        assert_eq!(ver.members[0].member_type, "data");
    }

    #[test]
    fn test_flattened_atom_deserialization() {
        let json = serde_json::json!({
            "name": "readings",
            "hash": "abc123",
            "path": ["parent", "child"],
        });
        let atom: FlattenedAtom = serde_json::from_value(json).unwrap();
        assert_eq!(atom.name, "readings");
        assert_eq!(atom.path, vec!["parent", "child"]);
    }
}
