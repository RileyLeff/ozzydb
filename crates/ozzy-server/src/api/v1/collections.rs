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
use std::collections::HashSet;
use uuid::Uuid;

use super::access::{enforce_read_access, enforce_write_access};
use super::auth::ApiError;
use crate::{
    AppState,
    auth::middleware::{AuthUser, MaybeAuthUser},
    db::queries::CollectionMutResult,
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
                .ok_or_else(|| ApiError::not_found(format!("Data atom '{}'", input.member_ref)))?;
            if atom.yanked {
                return Err(ApiError::gone(format!(
                    "Data atom '{}' has been yanked",
                    input.member_ref
                )));
            }
            Ok(("data".to_string(), input.member_ref.clone(), atom.hash))
        }
        "collection" => {
            let coll = state
                .db
                .get_collection(project_id, &input.member_ref)
                .await?
                .ok_or_else(|| ApiError::not_found(format!("Collection '{}'", input.member_ref)))?;
            if coll.yanked {
                return Err(ApiError::gone(format!(
                    "Collection '{}' has been yanked",
                    input.member_ref
                )));
            }
            let hash = if let Some(ver) = state.db.get_latest_collection_version(coll.id).await? {
                ver.hash
            } else {
                // Empty collection: hash is the empty collection hash
                ozzy_core::hash::collection_hash(&[])
            };
            Ok(("collection".to_string(), input.member_ref.clone(), hash))
        }
        // Endpoint members are deferred to a future phase. Reject for now.
        "endpoint" => Err(ApiError::bad_request(
            "Endpoint members are not supported yet. Use 'data' or 'collection'.",
        )),
        other => Err(ApiError::bad_request(format!(
            "Invalid member_type '{}': must be 'data' or 'collection'",
            other
        ))),
    }
}

/// Flatten a collection: recursively resolve all leaf data atoms.
fn flatten_collection<'a>(
    state: &'a AppState,
    project_id: Uuid,
    collection_name: &'a str,
    path: &'a [String],
    visited: &'a mut HashSet<String>,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<Vec<FlattenedAtom>, ApiError>> + Send + 'a>,
> {
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

        // Skip yanked collections during flatten
        if coll.yanked {
            return Ok(Vec::new());
        }

        let ver = match state.db.get_latest_collection_version(coll.id).await? {
            Some(v) => v,
            None => return Ok(Vec::new()),
        };

        let members = state.db.get_collection_members(ver.id).await?;
        let mut atoms = Vec::new();

        // Current path includes this collection
        let mut current_path = path.to_vec();
        current_path.push(collection_name.to_string());

        for member in members {
            match member.member_type.as_str() {
                "data" => {
                    // Skip yanked data atoms
                    if let Some(atom) = state
                        .db
                        .get_data_atom(project_id, &member.member_ref)
                        .await?
                    {
                        if atom.yanked {
                            continue;
                        }
                    }
                    atoms.push(FlattenedAtom {
                        name: member.member_ref,
                        hash: member.member_hash,
                        path: current_path.clone(),
                    });
                }
                "collection" => {
                    let child_atoms = flatten_collection(
                        state,
                        project_id,
                        &member.member_ref,
                        &current_path,
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

    // Resolve hashes for new members (read-only validation)
    let mut new_members = Vec::new();
    for input in &req.members {
        let (mtype, mref, mhash) = resolve_member_hash(&state, project.id, input).await?;
        new_members.push((mtype, mref, mhash));
    }

    // Atomically: advisory lock + yanked check + cycle check + read + merge + create version
    let (ver, members) = match state
        .db
        .add_to_collection_atomically(project.id, coll.id, &name, user.id, &new_members)
        .await?
    {
        CollectionMutResult::Ok(result) => result,
        CollectionMutResult::Yanked(coll_name) => {
            return Err(ApiError::gone(format!(
                "Collection '{}' has been yanked",
                coll_name
            )));
        }
        CollectionMutResult::CycleDetected(ref_name) => {
            return Err(ApiError::bad_request(format!(
                "Adding collection '{}' would create a circular reference",
                ref_name
            )));
        }
    };

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

    // Parse and validate removal refs: "data:name" or "collection:name"
    let valid_types = ["data", "collection"];
    let mut refs_to_remove = Vec::new();

    for r in &req.refs {
        let (mtype, mref) = r.split_once(':').ok_or_else(|| {
            ApiError::bad_request(format!(
                "Invalid ref format '{}': use 'type:name' (e.g. 'data:readings')",
                r
            ))
        })?;
        if !valid_types.contains(&mtype) {
            return Err(ApiError::bad_request(format!(
                "Invalid member type '{}': must be 'data' or 'collection'",
                mtype
            )));
        }
        refs_to_remove.push((mtype.to_string(), mref.to_string()));
    }

    // Atomically: advisory lock + yanked check + read + filter + create version
    let (ver, members) = match state
        .db
        .remove_from_collection_atomically(project.id, coll.id, user.id, &refs_to_remove)
        .await?
    {
        CollectionMutResult::Ok(Some(result)) => result,
        CollectionMutResult::Ok(None) => {
            return Err(ApiError::bad_request(format!(
                "Collection '{}' has no versions",
                name
            )));
        }
        CollectionMutResult::Yanked(coll_name) => {
            return Err(ApiError::gone(format!(
                "Collection '{}' has been yanked",
                coll_name
            )));
        }
        CollectionMutResult::CycleDetected(_) => unreachable!("remove does not check cycles"),
    };

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
