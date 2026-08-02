use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::tree::NodeId;

pub const NOTE_DOCUMENT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDocument {
    pub schema_version: u32,
    pub note_id: NodeId,
    pub revision: String,
    pub document: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Error)]
pub enum NoteDocumentError {
    #[error("note document schema version `{0}` is unsupported")]
    UnsupportedSchemaVersion(u32),
    #[error("note document root must be an object with type `doc`")]
    InvalidRoot,
    #[error("note document root content must be an array")]
    InvalidContent,
}

impl NoteDocument {
    pub fn empty(note_id: NodeId, revision: String, now: String) -> Self {
        Self {
            schema_version: NOTE_DOCUMENT_SCHEMA_VERSION,
            note_id,
            revision,
            document: serde_json::json!({ "type": "doc", "content": [] }),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    pub fn with_document(&self, document: Value, revision: String, now: String) -> Self {
        Self {
            schema_version: NOTE_DOCUMENT_SCHEMA_VERSION,
            note_id: self.note_id.clone(),
            revision,
            document,
            created_at: self.created_at.clone(),
            updated_at: now,
        }
    }

    pub fn validate(&self) -> Result<(), NoteDocumentError> {
        if self.schema_version != NOTE_DOCUMENT_SCHEMA_VERSION {
            return Err(NoteDocumentError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        let Some(root) = self.document.as_object() else {
            return Err(NoteDocumentError::InvalidRoot);
        };
        if root.get("type").and_then(Value::as_str) != Some("doc") {
            return Err(NoteDocumentError::InvalidRoot);
        }
        if !root.get("content").is_some_and(Value::is_array) {
            return Err(NoteDocumentError::InvalidContent);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_a_tiptap_document_root() {
        let document = NoteDocument::empty(
            NodeId::from("note-1"),
            "revision-1".to_owned(),
            "2026-08-03T12:00:00Z".to_owned(),
        );
        assert!(document.validate().is_ok());
    }

    #[test]
    fn rejects_invalid_document_roots() {
        let mut document = NoteDocument::empty(
            NodeId::from("note-1"),
            "revision-1".to_owned(),
            "2026-08-03T12:00:00Z".to_owned(),
        );
        document.document = serde_json::json!({ "type": "paragraph" });
        assert!(matches!(
            document.validate(),
            Err(NoteDocumentError::InvalidRoot)
        ));
    }
}
