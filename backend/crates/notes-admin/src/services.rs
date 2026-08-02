use std::sync::Mutex;

use thiserror::Error;

use crate::{
    storage::{StoreError, StoredManifest, TreeManifestStore, WriteCondition},
    tree::{
        Clock, CreateFolder, CreateNote, IdGenerator, NodeId, TreeError, TreeManifest,
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
        let mut ids = self
            .ids
            .lock()
            .map_err(|_| TreeServiceError::IdGeneratorPoisoned)?;
        let mut manifest = existing
            .map(|stored| stored.manifest)
            .unwrap_or_else(|| TreeManifest::new(&mut *ids, &self.clock));
        let value = operation(&mut manifest, &mut *ids, &self.clock)?;
        drop(ids);

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
    use crate::storage::StoreError;

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
