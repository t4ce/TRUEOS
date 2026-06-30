use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec::Vec;

use crate::disc::block;

/// Best-effort: build an HTML `<ul>/<li>` tree of the TRUEOSFS directory structure.
///
/// Returns `Ok(None)` if the disk does not contain TRUEOSFS.
///
/// Notes:
/// - Traversal is capped (`max_entries`) to keep this usable for tiny HTTP responses.
/// - Root-level entries are inserted before descendants, so top-level files remain visible
///   even when a large directory subtree would otherwise consume the cap.
/// - Uses the same HTML escaping guarantees as `trueos_math::Tree::html_tree_string`.
pub async fn html_tree_async(
    disk: block::DeviceHandle,
    max_entries: usize,
) -> Result<Option<String>, block::Error> {
    use trueos_math::{NodeId, Tree};

    if max_entries == 0 {
        return Ok(Some(String::from("<ul></ul>")));
    }

    let Some(paths) = super::trueosfs::index_path_snapshot_async(disk).await? else {
        return Ok(None);
    };

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum FsKind {
        Root,
        Dir,
        File,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct FsEntry {
        kind: FsKind,
        name: String,
    }

    const CAP: usize = 1024;
    let cap_limit = core::cmp::min(max_entries.saturating_add(1), CAP);

    let mut tree: Tree<FsEntry, CAP> = Tree::new();
    let Some(root) = tree.add_root(FsEntry {
        kind: FsKind::Root,
        name: String::from("/"),
    }) else {
        return Ok(Some(String::from("<ul><li>alloc failed</li></ul>")));
    };

    let mut dir_nodes: BTreeMap<Vec<u8>, NodeId> = BTreeMap::new();
    let mut root_files: BTreeSet<String> = BTreeSet::new();
    dir_nodes.insert(Vec::new(), root);

    for path in paths.iter() {
        let mut parts = path.split('/').filter(|seg| !seg.is_empty());
        let Some(first) = parts.next() else {
            continue;
        };

        if parts.next().is_none() {
            root_files.insert(String::from(first));
            continue;
        }

        let first_path = first.as_bytes().to_vec();
        if dir_nodes.contains_key(&first_path) {
            continue;
        }
        if tree.len() >= cap_limit {
            break;
        }
        let Some(node) = tree.add_child(
            root,
            FsEntry {
                kind: FsKind::Dir,
                name: String::from(first),
            },
        ) else {
            break;
        };
        dir_nodes.insert(first_path, node);
    }

    for file in root_files.iter() {
        if tree.len() >= cap_limit {
            break;
        }
        if tree
            .add_child(
                root,
                FsEntry {
                    kind: FsKind::File,
                    name: file.clone(),
                },
            )
            .is_none()
        {
            break;
        }
    }

    'files: for path in paths.iter() {
        if path
            .split('/')
            .filter(|seg| !seg.is_empty())
            .take(2)
            .count()
            <= 1
        {
            continue;
        }

        let mut parent_node = root;
        let mut dir_path: Vec<u8> = Vec::new();
        let mut parts = path.split('/').filter(|seg| !seg.is_empty()).peekable();
        while let Some(seg) = parts.next() {
            let is_last = parts.peek().is_none();
            if is_last {
                if tree.len() >= cap_limit {
                    break 'files;
                }
                if tree
                    .add_child(
                        parent_node,
                        FsEntry {
                            kind: FsKind::File,
                            name: String::from(seg),
                        },
                    )
                    .is_none()
                {
                    break 'files;
                }
                continue;
            }

            if !dir_path.is_empty() {
                dir_path.push(b'/');
            }
            dir_path.extend_from_slice(seg.as_bytes());

            if let Some(existing) = dir_nodes.get(&dir_path).copied() {
                parent_node = existing;
                continue;
            }

            if tree.len() >= cap_limit {
                break 'files;
            }
            let Some(node) = tree.add_child(
                parent_node,
                FsEntry {
                    kind: FsKind::Dir,
                    name: String::from(seg),
                },
            ) else {
                break 'files;
            };
            dir_nodes.insert(dir_path.clone(), node);
            parent_node = node;
        }
    }

    Ok(Some(tree.html_tree_string(root, |entry, out| match entry.kind {
        FsKind::Root => out.push('/'),
        FsKind::Dir => {
            out.push_str(entry.name.as_str());
            out.push('/');
        }
        FsKind::File => out.push_str(entry.name.as_str()),
    })))
}
