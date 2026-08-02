use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    routing::post,
};
use lambda_http::RequestExt;
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    services::{TreeOperations, TreeServiceError},
    tree::{CreateNote, NodeId},
};

pub fn router(tree_operations: Arc<dyn TreeOperations>) -> Router {
    Router::new()
        .route("/notes", post(create_note))
        .fallback(not_implemented)
        .with_state(tree_operations)
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
