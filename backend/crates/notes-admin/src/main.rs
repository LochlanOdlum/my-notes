mod api;
mod error;
pub mod services;
pub mod storage;
pub mod tree;

use lambda_http::{Error, run};
use std::sync::Arc;

use services::TreeService;
use storage::S3TreeManifestStore;
use tree::{UlidGenerator, UtcClock};

pub fn app(tree_operations: Arc<dyn services::TreeOperations>) -> axum::Router {
    api::router(tree_operations)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt().json().init();
    let store = S3TreeManifestStore::from_environment().await?;
    let tree_operations = Arc::new(TreeService::new(store, UlidGenerator, UtcClock));
    run(app(tree_operations)).await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    use super::{
        app,
        services::{TreeOperations, TreeServiceError},
        tree::{CreateNote, NodeId},
    };

    struct TestTreeOperations;

    #[async_trait]
    impl TreeOperations for TestTreeOperations {
        async fn create_note(&self, _input: CreateNote) -> Result<NodeId, TreeServiceError> {
            Ok(NodeId::from("note-123"))
        }
    }

    fn test_app() -> axum::Router {
        app(Arc::new(TestTreeOperations))
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), br#"{"status":"ok"}"#);
    }

    #[tokio::test]
    async fn admin_operations_are_available_while_authentication_is_disabled() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/admin/tree")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            body.as_ref(),
            br#"{"error":{"code":"not_implemented","message":"This admin operation is not implemented yet."}}"#
        );
    }

    #[tokio::test]
    async fn creates_a_note_through_the_admin_api() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/notes")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"First note","slug":"first-note"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), br#"{"id":"note-123"}"#);
    }
}
