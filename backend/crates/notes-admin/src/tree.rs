use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use ulid::Ulid;

const SCHEMA_VERSION: u32 = 1;
const POSITION_GAP: i64 = 1_000;

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
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteStatus {
    Draft,
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

impl TreeManifest {
    pub fn new(ids: &mut impl IdGenerator, clock: &impl Clock) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            revision: ids.next_id(),
            updated_at: clock.now(),
            nodes: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), TreeError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(TreeError::UnsupportedSchemaVersion(self.schema_version));
        }

        let mut ids = HashSet::new();
        let mut slugs = HashSet::new();
        let mut positions = HashSet::new();

        for node in &self.nodes {
            let common = node.common();
            if common.title.trim().is_empty() {
                return Err(TreeError::BlankTitle);
            }
            if !ids.insert(common.id.clone()) {
                return Err(TreeError::DuplicateNodeId);
            }
            if !positions.insert((common.parent_id.clone(), common.position)) {
                return Err(TreeError::DuplicateSiblingPosition);
            }
            if let TreeNode::Note(note) = node {
                if !is_valid_slug(&note.slug) {
                    return Err(TreeError::InvalidSlug(note.slug.clone()));
                }
                if !slugs.insert(note.slug.clone()) {
                    return Err(TreeError::DuplicateSlug(note.slug.clone()));
                }
            }
        }

        for node in &self.nodes {
            if let Some(parent_id) = &node.common().parent_id {
                self.ensure_folder_parent(parent_id)?;
            }
        }

        for node in &self.nodes {
            self.ensure_not_descendant_parent(&node.common().id, &node.common().parent_id)?;
        }

        Ok(())
    }

    pub fn create_folder(
        &mut self,
        input: CreateFolder,
        ids: &mut impl IdGenerator,
        clock: &impl Clock,
    ) -> Result<NodeId, TreeError> {
        let title = validated_title(input.title)?;
        self.ensure_parent(&input.parent_id)?;

        let id = NodeId(ids.next_id());
        let now = clock.now();
        let position = self.append_position(&input.parent_id)?;
        self.nodes.push(TreeNode::Folder(FolderNode {
            id: id.clone(),
            parent_id: input.parent_id,
            title,
            position,
            created_at: now.clone(),
            updated_at: now,
        }));
        self.record_mutation(ids, clock);
        Ok(id)
    }

    pub fn create_note(
        &mut self,
        input: CreateNote,
        ids: &mut impl IdGenerator,
        clock: &impl Clock,
    ) -> Result<NodeId, TreeError> {
        let title = validated_title(input.title)?;
        if !is_valid_slug(&input.slug) {
            return Err(TreeError::InvalidSlug(input.slug));
        }
        if self
            .nodes
            .iter()
            .any(|node| node.slug() == Some(&input.slug))
        {
            return Err(TreeError::DuplicateSlug(input.slug));
        }
        self.ensure_parent(&input.parent_id)?;

        let id = NodeId(ids.next_id());
        let now = clock.now();
        let position = self.append_position(&input.parent_id)?;
        self.nodes.push(TreeNode::Note(NoteNode {
            id: id.clone(),
            parent_id: input.parent_id,
            title,
            slug: input.slug,
            position,
            status: NoteStatus::Draft,
            created_at: now.clone(),
            updated_at: now,
        }));
        self.record_mutation(ids, clock);
        Ok(id)
    }

    pub fn rename_node(
        &mut self,
        node_id: &NodeId,
        title: String,
        ids: &mut impl IdGenerator,
        clock: &impl Clock,
    ) -> Result<(), TreeError> {
        let title = validated_title(title)?;
        let now = clock.now();
        *self.node_mut(node_id)?.common_mut().title = title;
        *self.node_mut(node_id)?.common_mut().updated_at = now;
        self.record_mutation(ids, clock);
        Ok(())
    }

    /// Moves a node and places it immediately after `after_sibling_id`, or at
    /// the beginning of the target parent when no sibling is supplied.
    pub fn move_node(
        &mut self,
        node_id: &NodeId,
        parent_id: Option<NodeId>,
        after_sibling_id: Option<NodeId>,
        ids: &mut impl IdGenerator,
        clock: &impl Clock,
    ) -> Result<(), TreeError> {
        self.node(node_id)?;
        self.ensure_parent(&parent_id)?;
        self.ensure_not_descendant_parent(node_id, &parent_id)?;

        let position =
            self.position_for_placement(&parent_id, node_id, after_sibling_id.as_ref())?;
        let now = clock.now();
        let node = self.node_mut(node_id)?;
        let common = node.common_mut();
        *common.parent_id = parent_id;
        *common.position = position;
        *common.updated_at = now;
        self.record_mutation(ids, clock);
        Ok(())
    }

    pub fn reorder_node(
        &mut self,
        node_id: &NodeId,
        after_sibling_id: Option<NodeId>,
        ids: &mut impl IdGenerator,
        clock: &impl Clock,
    ) -> Result<(), TreeError> {
        let parent_id = self.node(node_id)?.common().parent_id.clone();
        self.move_node(node_id, parent_id, after_sibling_id, ids, clock)
    }

    pub fn sorted_nodes(&self) -> Vec<&TreeNode> {
        let mut nodes = self.nodes.iter().collect::<Vec<_>>();
        nodes.sort_by_key(|node| {
            (
                node.common().parent_id.clone(),
                node.common().position,
                node.common().id.clone(),
            )
        });
        nodes
    }

    fn ensure_parent(&self, parent_id: &Option<NodeId>) -> Result<(), TreeError> {
        if let Some(parent_id) = parent_id {
            self.ensure_folder_parent(parent_id)?;
        }
        Ok(())
    }

    fn ensure_folder_parent(&self, parent_id: &NodeId) -> Result<(), TreeError> {
        match self
            .nodes
            .iter()
            .find(|node| *node.common().id == *parent_id)
        {
            Some(TreeNode::Folder(_)) => Ok(()),
            Some(TreeNode::Note(_)) => Err(TreeError::ParentMustBeFolder(parent_id.0.clone())),
            None => Err(TreeError::ParentNotFound(parent_id.0.clone())),
        }
    }

    fn ensure_not_descendant_parent(
        &self,
        node_id: &NodeId,
        parent_id: &Option<NodeId>,
    ) -> Result<(), TreeError> {
        let requested_parent = parent_id.clone();
        let mut current_parent = parent_id.clone();
        while let Some(parent) = current_parent {
            if parent == *node_id {
                return if parent_id.as_ref() == Some(node_id) {
                    Err(TreeError::SelfParent(node_id.0.clone()))
                } else {
                    Err(TreeError::DescendantParent {
                        node: node_id.0.clone(),
                        parent: requested_parent
                            .expect("a descendant-parent error has a requested parent")
                            .0,
                    })
                };
            }
            current_parent = self.node(&parent)?.common().parent_id.clone();
        }
        Ok(())
    }

    fn append_position(&mut self, parent_id: &Option<NodeId>) -> Result<i64, TreeError> {
        let max_position = self
            .siblings(parent_id, None)
            .into_iter()
            .map(|node| node.common().position)
            .max();
        match max_position {
            Some(position) if position <= i64::MAX - POSITION_GAP => Ok(position + POSITION_GAP),
            Some(_) => {
                self.rebalance(parent_id, None);
                Ok(self
                    .siblings(parent_id, None)
                    .into_iter()
                    .map(|node| node.common().position)
                    .max()
                    .unwrap_or(0)
                    + POSITION_GAP)
            }
            None => Ok(POSITION_GAP),
        }
    }

    fn position_for_placement(
        &mut self,
        parent_id: &Option<NodeId>,
        excluded_id: &NodeId,
        after_sibling_id: Option<&NodeId>,
    ) -> Result<i64, TreeError> {
        let mut siblings = self.siblings(parent_id, Some(excluded_id));
        siblings.sort_by_key(|node| (node.common().position, node.common().id.clone()));

        if let Some(after_id) = after_sibling_id {
            let index = siblings
                .iter()
                .position(|node| *node.common().id == *after_id)
                .ok_or_else(|| {
                    if self.node(after_id).is_ok() {
                        TreeError::SiblingNotInTargetParent(after_id.0.clone())
                    } else {
                        TreeError::SiblingNotFound(after_id.0.clone())
                    }
                })?;
            let current = siblings[index].common().position;
            if let Some(next) = siblings.get(index + 1) {
                let gap = next.common().position - current;
                if gap > 1 {
                    return Ok(current + gap / 2);
                }
                self.rebalance(parent_id, Some(excluded_id));
                return self.position_for_placement(parent_id, excluded_id, after_sibling_id);
            }
            return Ok(current + POSITION_GAP);
        }

        if let Some(first) = siblings.first() {
            if first.common().position > 1 {
                return Ok(first.common().position / 2);
            }
            self.rebalance(parent_id, Some(excluded_id));
            return self.position_for_placement(parent_id, excluded_id, None);
        }

        Ok(POSITION_GAP)
    }

    fn rebalance(&mut self, parent_id: &Option<NodeId>, excluded_id: Option<&NodeId>) {
        let mut sibling_ids = self
            .siblings(parent_id, excluded_id)
            .into_iter()
            .map(|node| node.common().id.clone())
            .collect::<Vec<_>>();
        sibling_ids.sort_by_key(|id| {
            let node = self
                .node(id)
                .expect("sibling IDs are sourced from this manifest");
            (node.common().position, node.common().id.clone())
        });
        for (index, id) in sibling_ids.iter().enumerate() {
            let common = self
                .node_mut(id)
                .expect("sibling IDs are sourced from this manifest")
                .common_mut();
            *common.position = (index as i64 + 1) * POSITION_GAP;
        }
    }

    fn siblings(&self, parent_id: &Option<NodeId>, excluded_id: Option<&NodeId>) -> Vec<&TreeNode> {
        self.nodes
            .iter()
            .filter(|node| {
                *node.common().parent_id == *parent_id
                    && excluded_id.is_none_or(|id| *node.common().id != *id)
            })
            .collect()
    }

    fn node(&self, id: &NodeId) -> Result<&TreeNode, TreeError> {
        self.nodes
            .iter()
            .find(|node| *node.common().id == *id)
            .ok_or_else(|| TreeError::NodeNotFound(id.0.clone()))
    }

    fn node_mut(&mut self, id: &NodeId) -> Result<&mut TreeNode, TreeError> {
        self.nodes
            .iter_mut()
            .find(|node| *node.common().id == *id)
            .ok_or_else(|| TreeError::NodeNotFound(id.0.clone()))
    }

    fn record_mutation(&mut self, ids: &mut impl IdGenerator, clock: &impl Clock) {
        self.revision = ids.next_id();
        self.updated_at = clock.now();
    }
}

impl TreeNode {
    fn common(&self) -> CommonNodeRef<'_> {
        match self {
            Self::Folder(node) => CommonNodeRef::from(node),
            Self::Note(node) => CommonNodeRef::from(node),
        }
    }

    fn common_mut(&mut self) -> CommonNodeMut<'_> {
        match self {
            Self::Folder(node) => CommonNodeMut::from(node),
            Self::Note(node) => CommonNodeMut::from(node),
        }
    }

    fn slug(&self) -> Option<&String> {
        match self {
            Self::Folder(_) => None,
            Self::Note(node) => Some(&node.slug),
        }
    }
}

struct CommonNodeRef<'a> {
    id: &'a NodeId,
    parent_id: &'a Option<NodeId>,
    title: &'a String,
    position: i64,
}

impl<'a> From<&'a FolderNode> for CommonNodeRef<'a> {
    fn from(node: &'a FolderNode) -> Self {
        Self {
            id: &node.id,
            parent_id: &node.parent_id,
            title: &node.title,
            position: node.position,
        }
    }
}

impl<'a> From<&'a NoteNode> for CommonNodeRef<'a> {
    fn from(node: &'a NoteNode) -> Self {
        Self {
            id: &node.id,
            parent_id: &node.parent_id,
            title: &node.title,
            position: node.position,
        }
    }
}

struct CommonNodeMut<'a> {
    parent_id: &'a mut Option<NodeId>,
    title: &'a mut String,
    position: &'a mut i64,
    updated_at: &'a mut String,
}

impl<'a> From<&'a mut FolderNode> for CommonNodeMut<'a> {
    fn from(node: &'a mut FolderNode) -> Self {
        Self {
            parent_id: &mut node.parent_id,
            title: &mut node.title,
            position: &mut node.position,
            updated_at: &mut node.updated_at,
        }
    }
}

impl<'a> From<&'a mut NoteNode> for CommonNodeMut<'a> {
    fn from(node: &'a mut NoteNode) -> Self {
        Self {
            parent_id: &mut node.parent_id,
            title: &mut node.title,
            position: &mut node.position,
            updated_at: &mut node.updated_at,
        }
    }
}

fn validated_title(title: String) -> Result<String, TreeError> {
    let title = title.trim().to_owned();
    if title.is_empty() {
        return Err(TreeError::BlankTitle);
    }
    Ok(title)
}

fn is_valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
        && slug.bytes().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == b'-'
        })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn manifest() -> (TreeManifest, FixedIds, FixedClock) {
        (
            TreeManifest::new(&mut FixedIds::new(&["manifest-1"]), &FixedClock),
            FixedIds::new(&[
                "folder-1",
                "manifest-2",
                "note-1",
                "manifest-3",
                "folder-2",
                "manifest-4",
                "note-2",
                "manifest-5",
            ]),
            FixedClock,
        )
    }

    #[test]
    fn serializes_a_flat_manifest_and_creates_nodes() {
        let (mut tree, mut ids, clock) = manifest();
        let folder = tree
            .create_folder(
                CreateFolder {
                    parent_id: None,
                    title: " Engineering ".to_owned(),
                },
                &mut ids,
                &clock,
            )
            .unwrap();
        let note = tree
            .create_note(
                CreateNote {
                    parent_id: Some(folder.clone()),
                    title: "Lambda notes".to_owned(),
                    slug: "lambda-notes".to_owned(),
                },
                &mut ids,
                &clock,
            )
            .unwrap();

        assert_eq!(folder.0, "folder-1");
        assert_eq!(note.0, "note-1");
        assert!(tree.validate().is_ok());
        let json = serde_json::to_value(&tree).unwrap();
        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["nodes"][0]["type"], "folder");
        assert_eq!(json["nodes"][1]["type"], "note");
        assert_eq!(
            serde_json::from_value::<TreeManifest>(json)
                .unwrap()
                .nodes
                .len(),
            2
        );
    }

    #[test]
    fn rejects_invalid_nodes_and_slugs() {
        let (mut tree, mut ids, clock) = manifest();
        assert_eq!(
            tree.create_folder(
                CreateFolder {
                    parent_id: None,
                    title: "  ".to_owned()
                },
                &mut ids,
                &clock
            ),
            Err(TreeError::BlankTitle)
        );
        assert_eq!(
            tree.create_note(
                CreateNote {
                    parent_id: None,
                    title: "Note".to_owned(),
                    slug: "Not A Slug".to_owned()
                },
                &mut ids,
                &clock,
            ),
            Err(TreeError::InvalidSlug("Not A Slug".to_owned()))
        );
    }

    #[test]
    fn prevents_cycles() {
        let (mut tree, mut ids, clock) = manifest();
        let parent = tree
            .create_folder(
                CreateFolder {
                    parent_id: None,
                    title: "Parent".to_owned(),
                },
                &mut ids,
                &clock,
            )
            .unwrap();
        let child = tree
            .create_folder(
                CreateFolder {
                    parent_id: Some(parent.clone()),
                    title: "Child".to_owned(),
                },
                &mut ids,
                &clock,
            )
            .unwrap();
        assert_eq!(
            tree.move_node(&parent, Some(child.clone()), None, &mut ids, &clock),
            Err(TreeError::DescendantParent {
                node: parent.0.clone(),
                parent: child.0.clone()
            })
        );
    }

    #[test]
    fn rebalances_siblings_when_the_position_gap_is_exhausted() {
        let clock = FixedClock;
        let mut ids = FixedIds::new(&[
            "manifest-1",
            "folder-1",
            "manifest-2",
            "folder-2",
            "manifest-3",
            "folder-3",
            "manifest-4",
            "manifest-5",
        ]);
        let mut tree = TreeManifest::new(&mut ids, &clock);
        let first = tree
            .create_folder(
                CreateFolder {
                    parent_id: None,
                    title: "First".to_owned(),
                },
                &mut ids,
                &clock,
            )
            .unwrap();
        let second = tree
            .create_folder(
                CreateFolder {
                    parent_id: None,
                    title: "Second".to_owned(),
                },
                &mut ids,
                &clock,
            )
            .unwrap();
        let third = tree
            .create_folder(
                CreateFolder {
                    parent_id: None,
                    title: "Third".to_owned(),
                },
                &mut ids,
                &clock,
            )
            .unwrap();

        *tree.node_mut(&first).unwrap().common_mut().position = 1;
        *tree.node_mut(&second).unwrap().common_mut().position = 2;
        tree.move_node(&third, None, None, &mut ids, &clock)
            .unwrap();

        let positions = tree
            .sorted_nodes()
            .into_iter()
            .map(|node| node.common().position)
            .collect::<Vec<_>>();
        assert_eq!(positions, vec![500, 1_000, 2_000]);
        assert!(tree.validate().is_ok());
    }
}
