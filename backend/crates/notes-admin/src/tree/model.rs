use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use ulid::Ulid;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub String);

impl From<&str> for NodeId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

pub trait IdGenerator {
    fn next_id(&mut self) -> String;
}

pub struct UlidGenerator;

impl IdGenerator for UlidGenerator {
    fn next_id(&mut self) -> String {
        Ulid::new().to_string()
    }
}

pub trait Clock {
    fn now(&self) -> String;
}

pub struct UtcClock;

impl Clock for UtcClock {
    fn now(&self) -> String {
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .expect("UTC timestamps always format as RFC 3339")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeManifest {
    pub schema_version: u32,
    pub revision: String,
    pub updated_at: String,
    pub nodes: Vec<TreeNode>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TreeNode {
    Folder(FolderNode),
    Note(NoteNode),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderNode {
    pub id: NodeId,
    pub parent_id: Option<NodeId>,
    pub title: String,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteNode {
    pub id: NodeId,
    pub parent_id: Option<NodeId>,
    pub title: String,
    pub slug: String,
    pub position: i64,
    pub status: NoteStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_revision: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteStatus {
    Draft,
    Published,
}

#[derive(Clone, Debug)]
pub struct CreateFolder {
    pub parent_id: Option<NodeId>,
    pub title: String,
}

#[derive(Clone, Debug)]
pub struct CreateNote {
    pub parent_id: Option<NodeId>,
    pub title: String,
    pub slug: String,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum TreeError {
    #[error("a node title cannot be blank")]
    BlankTitle,
    #[error("slug `{0}` is invalid")]
    InvalidSlug(String),
    #[error("slug `{0}` is already in use")]
    DuplicateSlug(String),
    #[error("node `{0}` does not exist")]
    NodeNotFound(String),
    #[error("parent `{0}` does not exist")]
    ParentNotFound(String),
    #[error("parent `{0}` is not a folder")]
    ParentMustBeFolder(String),
    #[error("node `{0}` cannot be its own parent")]
    SelfParent(String),
    #[error("node `{node}` cannot be moved into its descendant `{parent}`")]
    DescendantParent { node: String, parent: String },
    #[error("sibling `{0}` does not exist")]
    SiblingNotFound(String),
    #[error("sibling `{0}` is not in the target parent")]
    SiblingNotInTargetParent(String),
    #[error("node IDs must be unique")]
    DuplicateNodeId,
    #[error("sibling positions must be unique")]
    DuplicateSiblingPosition,
    #[error("manifest schema version `{0}` is unsupported")]
    UnsupportedSchemaVersion(u32),
}
