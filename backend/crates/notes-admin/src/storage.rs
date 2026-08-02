use std::env;

use async_trait::async_trait;
use aws_sdk_s3::{Client, error::ProvideErrorMetadata, primitives::ByteStream};
use thiserror::Error;

use crate::tree::TreeManifest;

const TREE_MANIFEST_KEY: &str = "private/tree.json";

#[derive(Clone, Debug)]
pub struct StoredManifest {
    pub manifest: TreeManifest,
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
}

#[async_trait]
impl TreeManifestStore for S3TreeManifestStore {
    async fn load(&self) -> Result<Option<StoredManifest>, StoreError> {
        let response = match self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(TREE_MANIFEST_KEY)
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
            .into_bytes();
        let manifest = serde_json::from_slice::<TreeManifest>(&bytes)
            .map_err(|error| StoreError::InvalidManifest(error.to_string()))?;
        manifest
            .validate()
            .map_err(|error| StoreError::InvalidManifest(error.to_string()))?;

        Ok(Some(StoredManifest { manifest, etag }))
    }

    async fn save(
        &self,
        manifest: &TreeManifest,
        condition: WriteCondition,
    ) -> Result<StoredManifest, StoreError> {
        manifest
            .validate()
            .map_err(|error| StoreError::InvalidManifest(error.to_string()))?;
        let body = serde_json::to_vec(manifest)
            .map_err(|error| StoreError::InvalidManifest(error.to_string()))?;
        let request = self
            .client
            .put_object()
            .bucket(&self.bucket_name)
            .key(TREE_MANIFEST_KEY)
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
        let etag = response.e_tag().map(str::to_owned).ok_or_else(|| {
            StoreError::InvalidManifest("S3 response did not include an ETag".to_owned())
        })?;

        Ok(StoredManifest {
            manifest: manifest.clone(),
            etag,
        })
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
