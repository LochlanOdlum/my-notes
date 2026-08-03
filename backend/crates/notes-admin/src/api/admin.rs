use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    routing::{get, options, post, put},
};
use lambda_http::RequestExt;
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    note::NoteDocument,
    services::{PublishedNote, TreeOperations, TreeServiceError},
    tree::{CreateNote, NodeId},
};

pub fn router(tree_operations: Arc<dyn TreeOperations>) -> Router {
    Router::new()
        .route("/tree", get(get_tree))
        .route("/notes", post(create_note))
        .route("/notes/{note_id}", get(get_note))
        .route("/notes/{note_id}/draft", put(save_note))
        .route("/notes/{note_id}/publish", post(publish_note))
        .route("/{*path}", options(preflight))
        .fallback(not_implemented)
        .with_state(tree_operations)
}

async fn preflight() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn get_tree(
    State(tree_operations): State<Arc<dyn TreeOperations>>,
    request: Request,
) -> Result<Json<crate::tree::TreeManifest>, ApiError> {
    authorize(&request)?;
    Ok(Json(
        tree_operations.load_tree().await.map_err(map_tree_error)?,
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateNoteRequest {
    parent_id: Option<NodeId>,
    title: String,
    slug: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateNoteResponse {
    id: NodeId,
}

#[derive(Deserialize)]
struct SaveDraftRequest {
    document: serde_json::Value,
    etag: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishNoteResponse {
    revision: String,
    public_path: String,
}

async fn create_note(
    State(tree_operations): State<Arc<dyn TreeOperations>>,
    request: Request,
) -> Result<(StatusCode, Json<CreateNoteResponse>), ApiError> {
    authorize(&request)?;
    let body = axum::body::to_bytes(request.into_body(), 64 * 1024)
        .await
        .map_err(|_| ApiError::BadRequest("request body is too large".to_owned()))?;
    let input: CreateNoteRequest = serde_json::from_slice(&body)
        .map_err(|_| ApiError::BadRequest("request body must be valid JSON".to_owned()))?;
    let id = tree_operations
        .create_note(CreateNote {
            parent_id: input.parent_id,
            title: input.title,
            slug: input.slug,
        })
        .await
        .map_err(map_tree_error)?;

    Ok((StatusCode::CREATED, Json(CreateNoteResponse { id })))
}

async fn get_note(
    State(tree_operations): State<Arc<dyn TreeOperations>>,
    Path(note_id): Path<NodeId>,
    request: Request,
) -> Result<(HeaderMap, Json<NoteDocument>), ApiError> {
    authorize(&request)?;
    let stored = tree_operations
        .load_note(note_id)
        .await
        .map_err(map_tree_error)?;
    Ok((etag_header(&stored.etag)?, Json(stored.document)))
}

async fn save_note(
    State(tree_operations): State<Arc<dyn TreeOperations>>,
    Path(note_id): Path<NodeId>,
    request: Request,
) -> Result<(HeaderMap, Json<NoteDocument>), ApiError> {
    authorize(&request)?;
    let header_etag = request
        .headers()
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = axum::body::to_bytes(request.into_body(), 1024 * 1024)
        .await
        .map_err(|_| ApiError::BadRequest("request body is too large".to_owned()))?;
    let input: SaveDraftRequest = serde_json::from_slice(&body)
        .map_err(|_| ApiError::BadRequest("request body must be valid JSON".to_owned()))?;
    let etag = header_etag.or(input.etag).ok_or_else(|| {
        ApiError::BadRequest("If-Match header or etag body field is required".to_owned())
    })?;
    let stored = tree_operations
        .save_note(note_id, input.document, etag)
        .await
        .map_err(map_tree_error)?;
    Ok((etag_header(&stored.etag)?, Json(stored.document)))
}

async fn publish_note(
    State(tree_operations): State<Arc<dyn TreeOperations>>,
    Path(note_id): Path<NodeId>,
    request: Request,
) -> Result<Json<PublishNoteResponse>, ApiError> {
    authorize(&request)?;
    let PublishedNote {
        revision,
        public_path,
    } = tree_operations
        .publish_note(note_id)
        .await
        .map_err(map_tree_error)?;
    Ok(Json(PublishNoteResponse {
        revision,
        public_path,
    }))
}

fn etag_header(etag: &str) -> Result<HeaderMap, ApiError> {
    let mut headers = HeaderMap::new();
    let value = HeaderValue::from_str(etag).map_err(|_| ApiError::Internal)?;
    headers.insert(header::ETAG, value);
    Ok(headers)
}

async fn not_implemented(request: Request) -> Result<ApiError, ApiError> {
    authorize(&request)?;

    Ok(ApiError::NotImplemented)
}

fn authorize(request: &Request) -> Result<(), ApiError> {
    if auth_is_enabled() && !is_admin(request) {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}

fn map_tree_error(error: TreeServiceError) -> ApiError {
    match error {
        TreeServiceError::Domain(error) => ApiError::BadRequest(error.to_string()),
        TreeServiceError::InvalidDocument(error) => ApiError::BadRequest(error),
        TreeServiceError::Conflict => ApiError::Conflict,
        TreeServiceError::Storage(_) | TreeServiceError::IdGeneratorPoisoned => ApiError::Internal,
    }
}

fn auth_is_enabled() -> bool {
    auth_is_enabled_from(std::env::var("ADMIN_AUTH_ENABLED").ok().as_deref())
}

fn auth_is_enabled_from(value: Option<&str>) -> bool {
    value == Some("true")
}

fn is_admin(request: &Request) -> bool {
    request
        .request_context_ref()
        .and_then(|context| context.authorizer())
        .and_then(|authorizer| authorizer.jwt.as_ref())
        .and_then(|jwt| jwt.claims.get("cognito:groups"))
        .is_some_and(|groups| is_member_of_admin_group(groups))
}

fn is_member_of_admin_group(groups: &str) -> bool {
    groups
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|group| group.trim().trim_matches('"'))
        .any(|group| group == "admins")
}

#[cfg(test)]
mod tests {
    use super::{auth_is_enabled_from, is_member_of_admin_group};

    #[test]
    fn recognises_the_admins_cognito_group() {
        assert!(is_member_of_admin_group("[admins]"));
        assert!(is_member_of_admin_group("[\"editors\", \"admins\"]"));
        assert!(!is_member_of_admin_group("[editors]"));
    }

    #[test]
    fn authentication_is_disabled_unless_explicitly_enabled() {
        assert!(!auth_is_enabled_from(None));
        assert!(auth_is_enabled_from(Some("true")));
        assert!(!auth_is_enabled_from(Some("false")));
    }
}
