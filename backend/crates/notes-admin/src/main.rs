mod api;
mod error;
pub mod note;
pub mod services;
pub mod storage;
pub mod tree;

use lambda_http::{Error, run};
use std::sync::Arc;

use services::NotesService;
use storage::S3TreeManifestStore;

pub fn app(tree_operations: Arc<dyn services::TreeOperations>) -> axum::Router {
    api::router(tree_operations)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt().json().init();
    let store = S3TreeManifestStore::from_environment().await?;
    let tree_operations = Arc::new(NotesService::new(store));
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
        note::NoteDocument,
        services::{PublishedNote, TreeOperations, TreeServiceError},
        storage::StoredNoteDocument,
        tree::{CreateNote, NodeId, TreeManifest},
    };

    struct TestTreeOperations;

    #[async_trait]
    impl TreeOperations for TestTreeOperations {
        async fn load_tree(&self) -> Result<TreeManifest, TreeServiceError> {
            Ok(TreeManifest {
                schema_version: 1,
                revision: "tree-revision".to_owned(),
                updated_at: "2026-08-03T12:00:00Z".to_owned(),
                nodes: Vec::new(),
            })
        }

        async fn create_note(&self, _input: CreateNote) -> Result<NodeId, TreeServiceError> {
            Ok(NodeId::from("note-123"))
        }

        async fn load_note(&self, note_id: NodeId) -> Result<StoredNoteDocument, TreeServiceError> {
            Ok(StoredNoteDocument {
                document: NoteDocument::empty(
                    note_id,
                    "draft-revision".to_owned(),
                    "2026-08-03T12:00:00Z".to_owned(),
                ),
                etag: "etag-1".to_owned(),
            })
        }

        async fn save_note(
            &self,
            note_id: NodeId,
            document: serde_json::Value,
            _etag: String,
        ) -> Result<StoredNoteDocument, TreeServiceError> {
            Ok(StoredNoteDocument {
                document: NoteDocument::empty(
                    note_id,
                    "draft-revision-2".to_owned(),
                    "2026-08-03T12:00:01Z".to_owned(),
                )
                .with_document(
                    document,
                    "draft-revision-2".to_owned(),
                    "2026-08-03T12:00:01Z".to_owned(),
                ),
                etag: "etag-2".to_owned(),
            })
        }

        async fn publish_note(&self, note_id: NodeId) -> Result<PublishedNote, TreeServiceError> {
            Ok(PublishedNote {
                revision: "published-revision".to_owned(),
                public_path: format!("notes/{}/published-revision.json", note_id.0),
            })
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
    async fn loads_the_private_tree_while_authentication_is_disabled() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/admin/tree")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            body.as_ref(),
            br#"{"schemaVersion":1,"revision":"tree-revision","updatedAt":"2026-08-03T12:00:00Z","nodes":[]}"#
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

    #[tokio::test]
    async fn loads_saves_and_publishes_a_note_through_the_admin_api() {
        let loaded = test_app()
            .oneshot(
                Request::builder()
                    .uri("/admin/notes/note-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(loaded.status(), StatusCode::OK);
        assert_eq!(loaded.headers().get("etag").unwrap(), "etag-1");

        let saved = test_app()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/admin/notes/note-123/draft")
                    .body(Body::from(
                        r#"{"etag":"etag-1","document":{"type":"doc","content":[{"type":"paragraph"}]}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(saved.status(), StatusCode::OK);
        assert_eq!(saved.headers().get("etag").unwrap(), "etag-2");

        let published = test_app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/notes/note-123/publish")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(published.status(), StatusCode::OK);
        let body = axum::body::to_bytes(published.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            body.as_ref(),
            br#"{"revision":"published-revision","publicPath":"notes/note-123/published-revision.json"}"#
        );
    }
}
