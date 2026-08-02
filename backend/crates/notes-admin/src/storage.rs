use std::env;

use async_trait::async_trait;
use aws_sdk_s3::{Client, error::ProvideErrorMetadata, primitives::ByteStream};
use thiserror::Error;

use crate::{note::NoteDocument, tree::TreeManifest};

const TREE_MANIFEST_KEY: &str = "private/tree.json";
const PUBLISHED_TREE_MANIFEST_KEY: &str = "published/tree.json";

#[derive(Clone, Debug)]
pub struct StoredManifest {
    pub manifest: TreeManifest,
    pub etag: String,
}

#[derive(Clone, Debug)]
pub struct StoredNoteDocument {
    pub document: NoteDocument,
    pub etag: String,
}

#[derive(Clone, Debug)]
pub enum WriteCondition {
    IfMatch(String),
    IfAbsent,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("the tree manifest was changed by another request")]
    Conflict,
    #[error("the tree manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("CONTENT_BUCKET_NAME is not configured")]
    MissingBucketName,
    #[error("S3 {operation} failed: {message}")]
    S3 {
        operation: &'static str,
        message: String,
    },
}

#[async_trait]
pub trait TreeManifestStore: Send + Sync {
    async fn load(&self) -> Result<Option<StoredManifest>, StoreError>;

    async fn save(
        &self,
        manifest: &TreeManifest,
        condition: WriteCondition,
    ) -> Result<StoredManifest, StoreError>;
}

#[derive(Clone)]
pub struct S3TreeManifestStore {
    bucket_name: String,
    client: Client,
}

impl S3TreeManifestStore {
    pub fn new(client: Client, bucket_name: impl Into<String>) -> Self {
        Self {
            bucket_name: bucket_name.into(),
            client,
        }
    }

    pub async fn from_environment() -> Result<Self, StoreError> {
        let bucket_name =
            env::var("CONTENT_BUCKET_NAME").map_err(|_| StoreError::MissingBucketName)?;
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        Ok(Self::new(Client::new(&config), bucket_name))
    }

    pub const fn key() -> &'static str {
        TREE_MANIFEST_KEY
    }

    pub async fn load_draft(
        &self,
        note_id: &str,
    ) -> Result<Option<StoredNoteDocument>, StoreError> {
        self.load_document(&format!("private/drafts/{note_id}.json"))
            .await
    }

    pub async fn save_draft(
        &self,
        document: &NoteDocument,
        condition: WriteCondition,
    ) -> Result<StoredNoteDocument, StoreError> {
        self.save_document(
            &format!("private/drafts/{}.json", document.note_id.0),
            document,
            condition,
        )
        .await
    }

    pub async fn publish_note(&self, document: &NoteDocument) -> Result<(), StoreError> {
        self.save_document(
            &format!(
                "published/notes/{}/{}.json",
                document.note_id.0, document.revision
            ),
            document,
            WriteCondition::IfAbsent,
        )
        .await
        .map(|_| ())
    }

    pub async fn load_published_tree(&self) -> Result<Option<StoredManifest>, StoreError> {
        self.load_manifest(PUBLISHED_TREE_MANIFEST_KEY).await
    }

    pub async fn save_published_tree(
        &self,
        manifest: &TreeManifest,
        condition: WriteCondition,
    ) -> Result<StoredManifest, StoreError> {
        self.save_manifest(PUBLISHED_TREE_MANIFEST_KEY, manifest, condition)
            .await
    }

    async fn load_document(&self, key: &str) -> Result<Option<StoredNoteDocument>, StoreError> {
        let Some((bytes, etag)) = self.load_bytes(key).await? else {
            return Ok(None);
        };
        let document = serde_json::from_slice::<NoteDocument>(&bytes)
            .map_err(|error| StoreError::InvalidManifest(error.to_string()))?;
        document
            .validate()
            .map_err(|error| StoreError::InvalidManifest(error.to_string()))?;
        Ok(Some(StoredNoteDocument { document, etag }))
    }

    async fn save_document(
        &self,
        key: &str,
        document: &NoteDocument,
        condition: WriteCondition,
    ) -> Result<StoredNoteDocument, StoreError> {
        document
            .validate()
            .map_err(|error| StoreError::InvalidManifest(error.to_string()))?;
        let etag = self.save_json(key, document, condition).await?;
        Ok(StoredNoteDocument {
            document: document.clone(),
            etag,
        })
    }

    async fn load_manifest(&self, key: &str) -> Result<Option<StoredManifest>, StoreError> {
        let Some((bytes, etag)) = self.load_bytes(key).await? else {
            return Ok(None);
        };
        let manifest = serde_json::from_slice::<TreeManifest>(&bytes)
            .map_err(|error| StoreError::InvalidManifest(error.to_string()))?;
        manifest
            .validate()
            .map_err(|error| StoreError::InvalidManifest(error.to_string()))?;
        Ok(Some(StoredManifest { manifest, etag }))
    }

    async fn load_bytes(&self, key: &str) -> Result<Option<(Vec<u8>, String)>, StoreError> {
        let response = match self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) if is_not_found(&error) => return Ok(None),
            Err(error) => return Err(s3_error("GetObject", error)),
        };
        let etag = response.e_tag().map(str::to_owned).ok_or_else(|| {
            StoreError::InvalidManifest("S3 response did not include an ETag".to_owned())
        })?;
        let bytes = response
            .body
            .collect()
            .await
            .map_err(|error| s3_error("GetObject body read", error))?
            .into_bytes()
            .to_vec();
        Ok(Some((bytes, etag)))
    }

    async fn save_manifest(
        &self,
        key: &str,
        manifest: &TreeManifest,
        condition: WriteCondition,
    ) -> Result<StoredManifest, StoreError> {
        manifest
            .validate()
            .map_err(|error| StoreError::InvalidManifest(error.to_string()))?;
        let etag = self.save_json(key, manifest, condition).await?;
        Ok(StoredManifest {
            manifest: manifest.clone(),
            etag,
        })
    }

    async fn save_json<T: serde::Serialize>(
        &self,
        key: &str,
        value: &T,
        condition: WriteCondition,
    ) -> Result<String, StoreError> {
        let body = serde_json::to_vec(value)
            .map_err(|error| StoreError::InvalidManifest(error.to_string()))?;
        let request = self
            .client
            .put_object()
            .bucket(&self.bucket_name)
            .key(key)
            .content_type("application/json")
            .body(ByteStream::from(body));
        let request = match condition {
            WriteCondition::IfMatch(etag) => request.if_match(etag),
            WriteCondition::IfAbsent => request.if_none_match("*"),
        };
        let response = request.send().await.map_err(|error| {
            if is_conflict(&error) {
                StoreError::Conflict
            } else {
                s3_error("PutObject", error)
            }
        })?;
        response.e_tag().map(str::to_owned).ok_or_else(|| {
            StoreError::InvalidManifest("S3 response did not include an ETag".to_owned())
        })
    }
}

#[async_trait]
impl TreeManifestStore for S3TreeManifestStore {
    async fn load(&self) -> Result<Option<StoredManifest>, StoreError> {
        self.load_manifest(TREE_MANIFEST_KEY).await
    }

    async fn save(
        &self,
        manifest: &TreeManifest,
        condition: WriteCondition,
    ) -> Result<StoredManifest, StoreError> {
        self.save_manifest(TREE_MANIFEST_KEY, manifest, condition)
            .await
    }
}

fn is_not_found<E: ProvideErrorMetadata>(error: &E) -> bool {
    matches!(error.code(), Some("NoSuchKey") | Some("NotFound"))
}

fn is_conflict<E: ProvideErrorMetadata>(error: &E) -> bool {
    matches!(
        error.code(),
        Some("PreconditionFailed") | Some("ConditionalRequestConflict")
    )
}

fn s3_error<E: std::fmt::Display>(operation: &'static str, error: E) -> StoreError {
    StoreError::S3 {
        operation,
        message: error.to_string(),
    }
}
