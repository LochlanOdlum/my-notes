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

    for node in &mut tree.nodes {
        let TreeNode::Folder(folder) = node else {
            continue;
        };
        if folder.id == first {
            folder.position = 1;
        } else if folder.id == second {
            folder.position = 2;
        }
    }
    tree.move_node(&third, None, None, &mut ids, &clock)
        .unwrap();

    let positions = tree
        .sorted_nodes()
        .into_iter()
        .map(|node| match node {
            TreeNode::Folder(folder) => folder.position,
            TreeNode::Note(note) => note.position,
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, vec![500, 1_000, 2_000]);
    assert!(tree.validate().is_ok());
}
