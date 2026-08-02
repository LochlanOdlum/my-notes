use std::sync::Mutex;

use async_trait::async_trait;
use thiserror::Error;

use crate::{
    note::NoteDocument,
    storage::{
        S3TreeManifestStore, StoreError, StoredManifest, StoredNoteDocument, TreeManifestStore,
        WriteCondition,
    },
    tree::{
        Clock, CreateFolder, CreateNote, IdGenerator, NodeId, TreeError, TreeManifest,
        UlidGenerator, UtcClock,
    },
};

#[derive(Clone, Debug)]
pub struct MutationResult<T> {
    pub value: T,
    pub stored_manifest: StoredManifest,
}

#[derive(Debug, Error)]
pub enum TreeServiceError {
    #[error("the tree changed before this update could be saved")]
    Conflict,
    #[error(transparent)]
    Domain(#[from] TreeError),
    #[error("{0}")]
    InvalidDocument(String),
    #[error(transparent)]
    Storage(#[from] StoreError),
    #[error("the tree ID generator lock was poisoned")]
    IdGeneratorPoisoned,
}

pub struct TreeService<S, I, C> {
    clock: C,
    ids: Mutex<I>,
    store: S,
}

#[async_trait]
pub trait TreeOperations: Send + Sync {
    async fn create_note(&self, input: CreateNote) -> Result<NodeId, TreeServiceError>;
    async fn load_note(&self, note_id: NodeId) -> Result<StoredNoteDocument, TreeServiceError>;
    async fn save_note(
        &self,
        note_id: NodeId,
        document: serde_json::Value,
        etag: String,
    ) -> Result<StoredNoteDocument, TreeServiceError>;
    async fn publish_note(&self, note_id: NodeId) -> Result<PublishedNote, TreeServiceError>;
}

#[derive(Clone, Debug)]
pub struct PublishedNote {
    pub revision: String,
    pub public_path: String,
}

pub struct NotesService {
    tree: TreeService<S3TreeManifestStore, UlidGenerator, UtcClock>,
    store: S3TreeManifestStore,
    ids: Mutex<UlidGenerator>,
    clock: UtcClock,
}

impl NotesService {
    pub fn new(store: S3TreeManifestStore) -> Self {
        Self {
            tree: TreeService::new(store.clone(), UlidGenerator, UtcClock),
            store,
            ids: Mutex::new(UlidGenerator),
            clock: UtcClock,
        }
    }

    fn next_id(&self) -> Result<String, TreeServiceError> {
        self.ids
            .lock()
            .map(|mut ids| ids.next_id())
            .map_err(|_| TreeServiceError::IdGeneratorPoisoned)
    }

    fn now(&self) -> String {
        self.clock.now()
    }

    async fn ensure_note_exists(&self, note_id: &NodeId) -> Result<(), TreeServiceError> {
        let tree =
            self.tree.load().await?.ok_or_else(|| {
                TreeServiceError::Domain(TreeError::NodeNotFound(note_id.0.clone()))
            })?;
        if tree.manifest.contains_note(note_id) {
            Ok(())
        } else {
            Err(TreeServiceError::Domain(TreeError::NodeNotFound(
                note_id.0.clone(),
            )))
        }
    }
}

impl<S, I, C> TreeService<S, I, C>
where
    S: TreeManifestStore,
    I: IdGenerator + Send,
    C: Clock + Send + Sync,
{
    pub fn new(store: S, ids: I, clock: C) -> Self {
        Self {
            clock,
            ids: Mutex::new(ids),
            store,
        }
    }

    pub async fn load(&self) -> Result<Option<StoredManifest>, TreeServiceError> {
        self.store.load().await.map_err(map_store_error)
    }

    pub async fn create_folder(
        &self,
        input: CreateFolder,
    ) -> Result<MutationResult<NodeId>, TreeServiceError> {
        self.mutate(|manifest, ids, clock| manifest.create_folder(input, ids, clock))
            .await
    }

    pub async fn create_note(
        &self,
        input: CreateNote,
    ) -> Result<MutationResult<NodeId>, TreeServiceError> {
        self.mutate(|manifest, ids, clock| manifest.create_note(input, ids, clock))
            .await
    }

    pub async fn publish_note(
        &self,
        node_id: NodeId,
        published_revision: String,
    ) -> Result<MutationResult<()>, TreeServiceError> {
        self.mutate(|manifest, ids, clock| {
            manifest.publish_note(&node_id, published_revision, ids, clock)
        })
        .await
    }

    pub async fn rename_node(
        &self,
        node_id: NodeId,
        title: String,
    ) -> Result<MutationResult<()>, TreeServiceError> {
        self.mutate(|manifest, ids, clock| manifest.rename_node(&node_id, title, ids, clock))
            .await
    }

    pub async fn move_node(
        &self,
        node_id: NodeId,
        parent_id: Option<NodeId>,
        after_sibling_id: Option<NodeId>,
    ) -> Result<MutationResult<()>, TreeServiceError> {
        self.mutate(|manifest, ids, clock| {
            manifest.move_node(&node_id, parent_id, after_sibling_id, ids, clock)
        })
        .await
    }

    pub async fn reorder_node(
        &self,
        node_id: NodeId,
        after_sibling_id: Option<NodeId>,
    ) -> Result<MutationResult<()>, TreeServiceError> {
        self.mutate(|manifest, ids, clock| {
            manifest.reorder_node(&node_id, after_sibling_id, ids, clock)
        })
        .await
    }

    async fn mutate<T>(
        &self,
        operation: impl FnOnce(&mut TreeManifest, &mut I, &C) -> Result<T, TreeError>,
    ) -> Result<MutationResult<T>, TreeServiceError> {
        let existing = self.store.load().await.map_err(map_store_error)?;
        let condition = existing
            .as_ref()
            .map(|stored| WriteCondition::IfMatch(stored.etag.clone()))
            .unwrap_or(WriteCondition::IfAbsent);
        // IDs are generated synchronously; release this non-Send lock before
        // the asynchronous conditional S3 write below.
        let (value, manifest) = {
            let mut ids = self
                .ids
                .lock()
                .map_err(|_| TreeServiceError::IdGeneratorPoisoned)?;
            let mut manifest = existing
                .map(|stored| stored.manifest)
                .unwrap_or_else(|| TreeManifest::new(&mut *ids, &self.clock));
            let value = operation(&mut manifest, &mut *ids, &self.clock)?;
            (value, manifest)
        };

        let stored_manifest = self
            .store
            .save(&manifest, condition)
            .await
            .map_err(map_store_error)?;
        Ok(MutationResult {
            value,
            stored_manifest,
        })
    }
}

#[async_trait]
impl TreeOperations for NotesService {
    async fn create_note(&self, input: CreateNote) -> Result<NodeId, TreeServiceError> {
        let note_id = self.tree.create_note(input).await?.value;
        let draft = NoteDocument::empty(note_id.clone(), self.next_id()?, self.now());
        self.store
            .save_draft(&draft, WriteCondition::IfAbsent)
            .await
            .map_err(map_store_error)?;
        Ok(note_id)
    }

    async fn load_note(&self, note_id: NodeId) -> Result<StoredNoteDocument, TreeServiceError> {
        self.ensure_note_exists(&note_id).await?;
        self.store
            .load_draft(&note_id.0)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| {
                TreeServiceError::Storage(StoreError::InvalidManifest(
                    "note draft is missing".to_owned(),
                ))
            })
    }

    async fn save_note(
        &self,
        note_id: NodeId,
        document: serde_json::Value,
        etag: String,
    ) -> Result<StoredNoteDocument, TreeServiceError> {
        let existing = self.load_note(note_id.clone()).await?;
        if existing.etag != etag {
            return Err(TreeServiceError::Conflict);
        }
        let updated = existing
            .document
            .with_document(document, self.next_id()?, self.now());
        updated
            .validate()
            .map_err(|error| TreeServiceError::InvalidDocument(error.to_string()))?;
        self.store
            .save_draft(&updated, WriteCondition::IfMatch(etag))
            .await
            .map_err(map_store_error)
    }

    async fn publish_note(&self, note_id: NodeId) -> Result<PublishedNote, TreeServiceError> {
        let draft = self.load_note(note_id.clone()).await?;
        let revision = self.next_id()?;
        let published = draft.document.with_document(
            draft.document.document.clone(),
            revision.clone(),
            self.now(),
        );
        self.store
            .publish_note(&published)
            .await
            .map_err(map_store_error)?;

        let private_tree =
            self.tree.load().await?.ok_or_else(|| {
                TreeServiceError::Domain(TreeError::NodeNotFound(note_id.0.clone()))
            })?;
        let mut next_tree = private_tree.manifest.clone();
        next_tree.publish_note(&note_id, revision.clone(), &mut UlidGenerator, &self.clock)?;
        let public_tree = next_tree.published_view();
        let current_public = self
            .store
            .load_published_tree()
            .await
            .map_err(map_store_error)?;
        let condition = current_public
            .as_ref()
            .map(|stored| WriteCondition::IfMatch(stored.etag.clone()))
            .unwrap_or(WriteCondition::IfAbsent);
        self.store
            .save_published_tree(&public_tree, condition)
            .await
            .map_err(map_store_error)?;
        self.tree
            .publish_note(note_id.clone(), revision.clone())
            .await?;

        Ok(PublishedNote {
            public_path: format!("notes/{}/{revision}.json", note_id.0),
            revision,
        })
    }
}

fn map_store_error(error: StoreError) -> TreeServiceError {
    match error {
        StoreError::Conflict => TreeServiceError::Conflict,
        error => TreeServiceError::Storage(error),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;
    use crate::{storage::StoreError, tree::TreeNode};

    struct FixedIds {
        values: Vec<String>,
    }

    impl FixedIds {
        fn new(values: &[&str]) -> Self {
            Self {
                values: values
                    .iter()
                    .rev()
                    .map(|value| (*value).to_owned())
                    .collect(),
            }
        }
    }

    impl IdGenerator for FixedIds {
        fn next_id(&mut self) -> String {
            self.values.pop().expect("test supplied enough IDs")
        }
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> String {
            "2026-07-30T12:00:00Z".to_owned()
        }
    }

    #[derive(Default)]
    struct MemoryStore {
        value: Mutex<Option<StoredManifest>>,
        next_etag: Mutex<u64>,
    }

    struct ConflictingStore;

    #[async_trait]
    impl TreeManifestStore for ConflictingStore {
        async fn load(&self) -> Result<Option<StoredManifest>, StoreError> {
            Ok(None)
        }

        async fn save(
            &self,
            _manifest: &TreeManifest,
            _condition: WriteCondition,
        ) -> Result<StoredManifest, StoreError> {
            Err(StoreError::Conflict)
        }
    }

    #[async_trait]
    impl TreeManifestStore for MemoryStore {
        async fn load(&self) -> Result<Option<StoredManifest>, StoreError> {
            Ok(self.value.lock().unwrap().clone())
        }

        async fn save(
            &self,
            manifest: &TreeManifest,
            condition: WriteCondition,
        ) -> Result<StoredManifest, StoreError> {
            let mut value = self.value.lock().unwrap();
            match (&*value, condition) {
                (None, WriteCondition::IfAbsent) => {}
                (Some(current), WriteCondition::IfMatch(etag)) if current.etag == etag => {}
                _ => return Err(StoreError::Conflict),
            }
            let mut next_etag = self.next_etag.lock().unwrap();
            *next_etag += 1;
            let stored = StoredManifest {
                manifest: manifest.clone(),
                etag: format!("etag-{next_etag}"),
            };
            *value = Some(stored.clone());
            Ok(stored)
        }
    }

    #[tokio::test]
    async fn creates_the_manifest_on_the_first_mutation() {
        let store = MemoryStore::default();
        let service = TreeService::new(
            store,
            FixedIds::new(&["manifest-1", "folder-1", "manifest-2"]),
            FixedClock,
        );

        let result = service
            .create_folder(CreateFolder {
                parent_id: None,
                title: "Engineering".to_owned(),
            })
            .await
            .unwrap();

        assert_eq!(result.value, NodeId::from("folder-1"));
        assert_eq!(result.stored_manifest.etag, "etag-1");
        assert_eq!(result.stored_manifest.manifest.nodes.len(), 1);
    }

    #[tokio::test]
    async fn persists_follow_up_mutations_with_the_latest_etag() {
        let store = MemoryStore::default();
        let service = TreeService::new(
            store,
            FixedIds::new(&["manifest-1", "folder-1", "manifest-2", "manifest-3"]),
            FixedClock,
        );
        let created = service
            .create_folder(CreateFolder {
                parent_id: None,
                title: "Original".to_owned(),
            })
            .await
            .unwrap();

        let renamed = service
            .rename_node(created.value, "Renamed".to_owned())
            .await
            .unwrap();

        assert_eq!(renamed.stored_manifest.etag, "etag-2");
        let TreeNode::Folder(folder) = &renamed.stored_manifest.manifest.nodes[0] else {
            panic!("the created node must remain a folder");
        };
        assert_eq!(folder.title, "Renamed");
    }

    #[tokio::test]
    async fn maps_a_conditional_write_failure_to_a_service_conflict() {
        let service = TreeService::new(
            ConflictingStore,
            FixedIds::new(&["manifest-1", "folder-1", "manifest-2"]),
            FixedClock,
        );

        let error = service
            .create_folder(CreateFolder {
                parent_id: None,
                title: "Engineering".to_owned(),
            })
            .await
            .unwrap_err();

        assert!(matches!(error, TreeServiceError::Conflict));
    }
}
