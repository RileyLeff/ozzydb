//! Collection management API endpoints.
//!
//! Collections are versioned, named sets of references to data atoms,
//! endpoints, or other collections. Each mutation creates a new immutable version.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use super::access::{enforce_read_access, enforce_write_access};
use super::auth::ApiError;
use crate::{
    AppState,
    auth::middleware::{AuthUser, MaybeAuthUser},
};

// ============================================================================
// Wire types
// ============================================================================

#[derive(Deserialize)]
struct CreateCollectionRequest {
    name: String,
}

#[derive(Serialize)]
struct CollectionInfo {
    id: Uuid,
    name: String,
    yanked: bool,
    created_at: DateTime<Utc>,
    latest_version: Option<i32>,
    member_count: Option<usize>,
}

#[derive(Serialize)]
struct CollectionDetail {
    id: Uuid,
    name: String,
    yanked: bool,
    yank_reason: Option<String>,
    yanked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    version: Option<VersionDetail>,
}

#[derive(Serialize)]
struct VersionDetail {
    version_number: i32,
    hash: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    members: Vec<MemberInfo>,
}

#[derive(Serialize)]
struct MemberInfo {
    member_type: String,
    member_ref: String,
    member_hash: String,
    ordinal: i32,
}

#[derive(Serialize)]
struct VersionLogEntry {
    version_number: i32,
    hash: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct AddMembersRequest {
    members: Vec<MemberInput>,
}

#[derive(Deserialize)]
struct RemoveMembersRequest {
    /// Member refs to remove (e.g. "data:readings", "collection:train-set")
    refs: Vec<String>,
}

#[derive(Deserialize)]
struct MemberInput {
    /// "data", "collection", or "endpoint"
    member_type: String,
    /// Name of the member within this project
    member_ref: String,
}

#[derive(Deserialize)]
struct YankCollectionRequest {
    reason: String,
}

#[derive(Serialize)]
struct FlattenedAtom {
    name: String,
    hash: String,
    /// Path of collection nesting, e.g. ["parent-coll", "child-coll"]
    path: Vec<String>,
}

// ============================================================================
// Helpers
// ============================================================================

/// Valid name pattern: [a-zA-Z0-9_-]+
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Resolve a member input to its hash. Returns (member_type, member_ref, member_hash).
async fn resolve_member_hash(
    state: &AppState,
    project_id: Uuid,
    input: &MemberInput,
) -> Result<(String, String, String), ApiError> {
    match input.member_type.as_str() {
        "data" => {
            let atom = state
                .db
                .get_data_atom(project_id, &input.member_ref)
                .await?
                .ok_or_else(|| {
                    ApiError::not_found(format!("Data atom '{}'", input.member_ref))
                })?;
            Ok(("data".to_string(), input.member_ref.clone(), atom.hash))
        }
        "collection" => {
            let coll = state
                .db
                .get_collection(project_id, &input.member_ref)
                .await?
                .ok_or_else(|| {
                    ApiError::not_found(format!("Collection '{}'", input.member_ref))
                })?;
            let hash = if let Some(ver) = state.db.get_latest_collection_version(coll.id).await? {
                ver.hash
            } else {
                // Empty collection: hash is the empty collection hash
                ozzy_core::hash::collection_hash(&[])
            };
            Ok(("collection".to_string(), input.member_ref.clone(), hash))
        }
        "endpoint" => {
            // Endpoint members reference the endpoint name; hash is the endpoint name itself
            // since endpoint materialized hashes are computed at execution time
            Ok((
                "endpoint".to_string(),
                input.member_ref.clone(),
                input.member_ref.clone(),
            ))
        }
        other => Err(ApiError::bad_request(format!(
            "Invalid member_type '{}': must be 'data', 'collection', or 'endpoint'",
            other
        ))),
    }
}

/// DFS cycle detection: check if adding `target_name` as a sub-collection of
/// `parent_collection_id` would create a cycle.
async fn would_create_cycle(
    state: &AppState,
    project_id: Uuid,
    parent_collection_name: &str,
    target_name: &str,
) -> Result<bool, ApiError> {
    // If we're adding a collection as a member of itself, that's a cycle
    if parent_collection_name == target_name {
        return Ok(true);
    }

    // DFS: walk the target collection's sub-collections to see if any
    // eventually reference the parent
    let mut stack = vec![target_name.to_string()];
    let mut visited = HashSet::new();

    while let Some(current) = stack.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }

        let Some(coll) = state.db.get_collection(project_id, &current).await? else {
            continue;
        };

        let Some(ver) = state.db.get_latest_collection_version(coll.id).await? else {
            continue;
        };

        let members = state.db.get_collection_members(ver.id).await?;
        for member in members {
            if member.member_type == "collection" {
                if member.member_ref == parent_collection_name {
                    return Ok(true);
                }
                stack.push(member.member_ref);
            }
        }
    }

    Ok(false)
}

/// Flatten a collection: recursively resolve all leaf data atoms.
fn flatten_collection<'a>(
    state: &'a AppState,
    project_id: Uuid,
    collection_name: &'a str,
    path: &'a [String],
    visited: &'a mut HashSet<String>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<FlattenedAtom>, ApiError>> + Send + 'a>> {
    Box::pin(async move {
    if !visited.insert(collection_name.to_string()) {
        // Already visited — skip to avoid infinite loops from any residual cycles
        return Ok(Vec::new());
    }

    let coll = state
        .db
        .get_collection(project_id, collection_name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Collection '{}'", collection_name)))?;

    let ver = match state.db.get_latest_collection_version(coll.id).await? {
        Some(v) => v,
        None => return Ok(Vec::new()),
    };

    let members = state.db.get_collection_members(ver.id).await?;
    let mut atoms = Vec::new();

    for member in members {
        match member.member_type.as_str() {
            "data" => {
                atoms.push(FlattenedAtom {
                    name: member.member_ref,
                    hash: member.member_hash,
                    path: path.to_vec(),
                });
            }
            "collection" => {
                let mut child_path = path.to_vec();
                child_path.push(collection_name.to_string());
                let child_atoms = flatten_collection(
                    state,
                    project_id,
                    &member.member_ref,
                    &child_path,
                    visited,
                )
                .await?;
                atoms.extend(child_atoms);
            }
            _ => {
                // endpoint members are not leaf atoms, skip in flatten
            }
        }
    }

    Ok(atoms)
    })
}

// ============================================================================
// Routes
// ============================================================================

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{owner}/{project}", post(create_collection))
        .route("/{owner}/{project}", get(list_collections))
        .route("/{owner}/{project}/{name}", get(get_collection))
        .route("/{owner}/{project}/{name}/log", get(collection_log))
        .route("/{owner}/{project}/{name}/flatten", get(flatten))
        .route("/{owner}/{project}/{name}/add", post(add_members))
        .route("/{owner}/{project}/{name}/remove", post(remove_members))
        .route("/{owner}/{project}/{name}/yank", post(yank_collection))
}

/// Create a new collection.
async fn create_collection(
    AuthUser { user, scope }: AuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
    Json(req): Json<CreateCollectionRequest>,
) -> Result<Json<CollectionInfo>, ApiError> {
    if !is_valid_name(&req.name) {
        return Err(ApiError::bad_request(format!(
            "Invalid name '{}': must match [a-zA-Z0-9_-]+",
            req.name
        )));
    }

    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_write_access(&state, &project, &owner, &project_slug, &user, &scope).await?;

    let coll = state
        .db
        .create_collection(project.id, &req.name, user.id)
        .await?;

    Ok(Json(CollectionInfo {
        id: coll.id,
        name: coll.name,
        yanked: coll.yanked,
        created_at: coll.created_at,
        latest_version: None,
        member_count: None,
    }))
}

/// List collections in a project.
async fn list_collections(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
) -> Result<Json<Vec<CollectionInfo>>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let colls = state.db.list_collections(project.id).await?;

    let mut items = Vec::with_capacity(colls.len());
    for coll in colls {
        let (latest_version, member_count) =
            if let Some(ver) = state.db.get_latest_collection_version(coll.id).await? {
                let members = state.db.get_collection_members(ver.id).await?;
                (Some(ver.version_number), Some(members.len()))
            } else {
                (None, None)
            };

        items.push(CollectionInfo {
            id: coll.id,
            name: coll.name,
            yanked: coll.yanked,
            created_at: coll.created_at,
            latest_version,
            member_count,
        });
    }

    Ok(Json(items))
}

/// Get collection detail with current version and members.
async fn get_collection(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, name)): Path<(String, String, String)>,
) -> Result<Json<CollectionDetail>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let coll = state
        .db
        .get_collection(project.id, &name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Collection '{}'", name)))?;

    let version = if let Some(ver) = state.db.get_latest_collection_version(coll.id).await? {
        let members = state.db.get_collection_members(ver.id).await?;
        Some(VersionDetail {
            version_number: ver.version_number,
            hash: ver.hash,
            created_by: ver.created_by,
            created_at: ver.created_at,
            members: members
                .into_iter()
                .map(|m| MemberInfo {
                    member_type: m.member_type,
                    member_ref: m.member_ref,
                    member_hash: m.member_hash,
                    ordinal: m.ordinal,
                })
                .collect(),
        })
    } else {
        None
    };

    Ok(Json(CollectionDetail {
        id: coll.id,
        name: coll.name,
        yanked: coll.yanked,
        yank_reason: coll.yank_reason,
        yanked_at: coll.yanked_at,
        created_at: coll.created_at,
        version,
    }))
}

/// Get version history for a collection.
async fn collection_log(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, name)): Path<(String, String, String)>,
) -> Result<Json<Vec<VersionLogEntry>>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let coll = state
        .db
        .get_collection(project.id, &name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Collection '{}'", name)))?;

    let versions = state.db.list_collection_versions(coll.id).await?;

    let entries: Vec<VersionLogEntry> = versions
        .into_iter()
        .map(|v| VersionLogEntry {
            version_number: v.version_number,
            hash: v.hash,
            created_by: v.created_by,
            created_at: v.created_at,
        })
        .collect();

    Ok(Json(entries))
}

/// Flatten a collection to its leaf-level data atoms.
async fn flatten(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, name)): Path<(String, String, String)>,
) -> Result<Json<Vec<FlattenedAtom>>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    // Verify collection exists
    state
        .db
        .get_collection(project.id, &name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Collection '{}'", name)))?;

    let mut visited = HashSet::new();
    let atoms = flatten_collection(&state, project.id, &name, &[], &mut visited).await?;

    Ok(Json(atoms))
}

/// Add members to a collection (creates a new version).
async fn add_members(
    AuthUser { user, scope }: AuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, name)): Path<(String, String, String)>,
    Json(req): Json<AddMembersRequest>,
) -> Result<Json<VersionDetail>, ApiError> {
    if req.members.is_empty() {
        return Err(ApiError::bad_request("No members to add"));
    }

    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_write_access(&state, &project, &owner, &project_slug, &user, &scope).await?;

    let coll = state
        .db
        .get_collection(project.id, &name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Collection '{}'", name)))?;

    if coll.yanked {
        return Err(ApiError::gone(format!(
            "Collection '{}' has been yanked",
            name
        )));
    }

    // Resolve hashes for new members and check for cycles
    let mut new_members = Vec::new();
    for input in &req.members {
        // Cycle detection for collection members
        if input.member_type == "collection" {
            if would_create_cycle(&state, project.id, &name, &input.member_ref).await? {
                return Err(ApiError::bad_request(format!(
                    "Adding collection '{}' would create a circular reference",
                    input.member_ref
                )));
            }
        }

        let (mtype, mref, mhash) =
            resolve_member_hash(&state, project.id, input).await?;
        new_members.push((mtype, mref, mhash));
    }

    // Get current members
    let current_members =
        if let Some(latest_ver) = state.db.get_latest_collection_version(coll.id).await? {
            state.db.get_collection_members(latest_ver.id).await?
        } else {
            Vec::new()
        };

    // Build combined member list
    let mut all_members: Vec<(String, String, String, i32)> = current_members
        .iter()
        .map(|m| {
            (
                m.member_type.clone(),
                m.member_ref.clone(),
                m.member_hash.clone(),
                m.ordinal,
            )
        })
        .collect();

    let next_ordinal = all_members.iter().map(|(_, _, _, o)| *o).max().unwrap_or(-1) + 1;
    for (i, (mtype, mref, mhash)) in new_members.into_iter().enumerate() {
        // Skip duplicates (same type + ref already in collection)
        if all_members.iter().any(|(t, r, _, _)| t == &mtype && r == &mref) {
            continue;
        }
        all_members.push((mtype, mref, mhash, next_ordinal + i as i32));
    }

    // Compute new collection hash
    let member_hashes: Vec<&str> = all_members.iter().map(|(_, _, h, _)| h.as_str()).collect();
    let coll_hash = ozzy_core::hash::collection_hash(&member_hashes);

    let (ver, members) = state
        .db
        .create_collection_version_with_members(coll.id, &coll_hash, user.id, &all_members)
        .await?;

    Ok(Json(VersionDetail {
        version_number: ver.version_number,
        hash: ver.hash,
        created_by: ver.created_by,
        created_at: ver.created_at,
        members: members
            .into_iter()
            .map(|m| MemberInfo {
                member_type: m.member_type,
                member_ref: m.member_ref,
                member_hash: m.member_hash,
                ordinal: m.ordinal,
            })
            .collect(),
    }))
}

/// Remove members from a collection (creates a new version).
async fn remove_members(
    AuthUser { user, scope }: AuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, name)): Path<(String, String, String)>,
    Json(req): Json<RemoveMembersRequest>,
) -> Result<Json<VersionDetail>, ApiError> {
    if req.refs.is_empty() {
        return Err(ApiError::bad_request("No member refs to remove"));
    }

    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_write_access(&state, &project, &owner, &project_slug, &user, &scope).await?;

    let coll = state
        .db
        .get_collection(project.id, &name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Collection '{}'", name)))?;

    if coll.yanked {
        return Err(ApiError::gone(format!(
            "Collection '{}' has been yanked",
            name
        )));
    }

    // Parse removal refs: "data:name" or "collection:name" or "endpoint:name"
    let removal_set: HashMap<(&str, &str), bool> = req
        .refs
        .iter()
        .filter_map(|r| {
            let (mtype, mref) = r.split_once(':')?;
            Some(((mtype, mref), true))
        })
        .collect();

    if removal_set.is_empty() {
        return Err(ApiError::bad_request(
            "Invalid ref format: use 'type:name' (e.g. 'data:readings')",
        ));
    }

    // Get current members
    let latest_ver = state
        .db
        .get_latest_collection_version(coll.id)
        .await?
        .ok_or_else(|| {
            ApiError::bad_request(format!("Collection '{}' has no versions", name))
        })?;

    let current_members = state.db.get_collection_members(latest_ver.id).await?;

    // Filter out removed members, re-number ordinals
    let remaining: Vec<(String, String, String, i32)> = current_members
        .iter()
        .filter(|m| {
            !removal_set.contains_key(&(m.member_type.as_str(), m.member_ref.as_str()))
        })
        .enumerate()
        .map(|(i, m)| {
            (
                m.member_type.clone(),
                m.member_ref.clone(),
                m.member_hash.clone(),
                i as i32,
            )
        })
        .collect();

    // Compute new hash
    let member_hashes: Vec<&str> = remaining.iter().map(|(_, _, h, _)| h.as_str()).collect();
    let coll_hash = ozzy_core::hash::collection_hash(&member_hashes);

    let (ver, members) = state
        .db
        .create_collection_version_with_members(coll.id, &coll_hash, user.id, &remaining)
        .await?;

    Ok(Json(VersionDetail {
        version_number: ver.version_number,
        hash: ver.hash,
        created_by: ver.created_by,
        created_at: ver.created_at,
        members: members
            .into_iter()
            .map(|m| MemberInfo {
                member_type: m.member_type,
                member_ref: m.member_ref,
                member_hash: m.member_hash,
                ordinal: m.ordinal,
            })
            .collect(),
    }))
}

/// Yank a collection (soft delete with reason).
async fn yank_collection(
    AuthUser { user, scope }: AuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, name)): Path<(String, String, String)>,
    Json(req): Json<YankCollectionRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if req.reason.is_empty() {
        return Err(ApiError::bad_request("Yank reason cannot be empty"));
    }

    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_write_access(&state, &project, &owner, &project_slug, &user, &scope).await?;

    let yanked = state
        .db
        .yank_collection(project.id, &name, &req.reason)
        .await?;
    if !yanked {
        return Err(ApiError::not_found(format!("Collection '{}'", name)));
    }

    Ok(Json(serde_json::json!({ "yanked": true, "name": name })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_name() {
        assert!(is_valid_name("train-set"));
        assert!(is_valid_name("my_collection"));
        assert!(is_valid_name("v2"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("has space"));
        assert!(!is_valid_name("has.dot"));
        assert!(!is_valid_name("has/slash"));
    }
}
