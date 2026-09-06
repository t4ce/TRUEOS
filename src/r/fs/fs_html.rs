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

use alloc::collections::BTreeMap;
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
    html_tree_impl(disk, max_entries, None).await
}

/// The HTTP page supplies native disclosures and controls before scripts run.
pub async fn html_browser_tree_async(
    disk: block::DeviceHandle,
    max_entries: usize,
    root_id: u32,
) -> Result<Option<String>, block::Error> {
    html_tree_impl(disk, max_entries, Some(root_id)).await
}

async fn html_tree_impl(
    disk: block::DeviceHandle,
    max_entries: usize,
    browser_root: Option<u32>,
) -> Result<Option<String>, block::Error> {
    use trueos_math::{NodeId, Tree};

    if max_entries == 0 {
        return Ok(Some(String::from("<ul></ul>")));
    }

    let Some(nodes) = super::trueosfs::index_path_snapshot_async(disk).await? else {
        return Ok(None);
    };

    const CAP: usize = super::trueosfs::TRUEOSFS_LIST_SOFT_CAP + 2;
    let effective_entries = core::cmp::min(max_entries, super::trueosfs::TRUEOSFS_LIST_SOFT_CAP);
    let cap_limit = core::cmp::min(effective_entries.saturating_add(2), CAP);
    let entry_limit = cap_limit.saturating_sub(1);
    let mut truncated = nodes.len() > effective_entries;

    let mut tree: Tree<FsEntry, CAP> = Tree::new();
    let Some(root) = tree.add_root(FsEntry {
        kind: FsKind::Root,
        name: String::from("/"),
    }) else {
        return Ok(Some(String::from("<ul><li>alloc failed</li></ul>")));
    };

    let mut dir_nodes: BTreeMap<Vec<u8>, NodeId> = BTreeMap::new();
    dir_nodes.insert(Vec::new(), root);

    for node in nodes
        .iter()
        .filter(|node| node.path.split('/').count() == 1)
    {
        let mut parts = node.path.split('/').filter(|seg| !seg.is_empty());
        let Some(first) = parts.next() else {
            continue;
        };
        if tree.len() >= entry_limit {
            truncated = true;
            break;
        }
        let kind = match node.kind {
            super::trueosfs::NodeKind::Directory => FsKind::Dir,
            super::trueosfs::NodeKind::File => FsKind::File,
        };
        let Some(node) = tree.add_child(
            root,
            FsEntry {
                kind: kind.clone(),
                name: String::from(first),
            },
        ) else {
            truncated = true;
            break;
        };
        if matches!(kind, FsKind::Dir) {
            dir_nodes.insert(first.as_bytes().to_vec(), node);
        }
    }

    let max_depth = nodes
        .iter()
        .map(|node| node.path.split('/').filter(|seg| !seg.is_empty()).count())
        .max()
        .unwrap_or(1);

    // Populate one depth across the whole filesystem at a time. This prevents
    // one large, alphabetically early subtree from consuming the complete cap
    // before sibling directories receive their immediate children.
    'levels: for depth in 2..=max_depth {
        for node in nodes.iter() {
            let parts: Vec<&str> = node.path.split('/').filter(|seg| !seg.is_empty()).collect();
            if parts.len() != depth {
                continue;
            }

            let parent_path = parts[..depth - 1].join("/").into_bytes();
            let Some(parent_node) = dir_nodes.get(&parent_path).copied() else {
                continue;
            };
            let entry_name = parts[depth - 1];

            let dir_path = parts[..depth].join("/").into_bytes();
            if dir_nodes.contains_key(&dir_path)
                && node.kind == super::trueosfs::NodeKind::Directory
            {
                continue;
            }
            if tree.len() >= entry_limit {
                truncated = true;
                break 'levels;
            }
            let node_kind = node.kind;
            let Some(tree_node) = tree.add_child(
                parent_node,
                FsEntry {
                    kind: match node_kind {
                        super::trueosfs::NodeKind::Directory => FsKind::Dir,
                        super::trueosfs::NodeKind::File => FsKind::File,
                    },
                    name: String::from(entry_name),
                },
            ) else {
                truncated = true;
                break 'levels;
            };
            if node_kind == super::trueosfs::NodeKind::Directory {
                dir_nodes.insert(dir_path, tree_node);
            }
        }
    }

    if truncated {
        crate::log_warn!(target: "filesystem";
            "trueosfs: file listing soft cap reached operation=http_tree cap={} requested={}\n",
            super::trueosfs::TRUEOSFS_LIST_SOFT_CAP,
            max_entries
        );
        let _ = tree.add_child(
            root,
            FsEntry {
                kind: FsKind::File,
                name: String::from("..."),
            },
        );
    }

    if let Some(root_id) = browser_root {
        return Ok(Some(browser_tree(&tree, root, root_id)));
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

fn escaped(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

fn browser_tree<const N: usize>(
    tree: &trueos_math::Tree<FsEntry, N>,
    root: trueos_math::NodeId,
    root_id: u32,
) -> String {
    use core::fmt::Write;
    let mut out = String::from("<ul>");
    // Explicit traversal stack keeps deep filesystem paths off the kernel stack.
    let mut stack = Vec::new();
    let children: Vec<_> = tree.children(root).collect();
    for id in children.into_iter().rev() {
        stack.push((id, String::new(), false));
    }
    while let Some((id, parent_path, close)) = stack.pop() {
        let Some(entry) = tree.get(id) else {
            continue;
        };
        let directory = entry.kind == FsKind::Dir;
        if close {
            out.push_str("</ul></details></li>");
            continue;
        }
        if entry.name == "..." {
            out.push_str("<li>...</li>");
            continue;
        }
        let path = if parent_path.is_empty() {
            entry.name.clone()
        } else {
            alloc::format!("{}/{}", parent_path, entry.name)
        };
        out.push_str("<li data-label=\"");
        escaped(&mut out, &entry.name);
        if directory {
            out.push('/');
        }
        out.push_str("\" data-path=\"");
        escaped(&mut out, &path);
        out.push_str("\">");
        if directory {
            out.push_str("<details><summary>");
            escaped(&mut out, &entry.name);
            out.push_str("<span class=\"fs-controls\"> <button type=\"button\" data-action=\"delete\">x</button> <button type=\"button\" data-action=\"upload\">upload</button> <button type=\"button\" data-action=\"mkdir\">+</button> <small></small></span></summary><ul>");
            stack.push((id, String::new(), true));
            let children: Vec<_> = tree.children(id).collect();
            for child in children.into_iter().rev() {
                stack.push((child, path.clone(), false));
            }
        } else {
            let _ = write!(
                out,
                "<button type=\"button\" data-action=\"delete\">x</button> <a download href=\"/dl/{}/",
                root_id
            );
            for byte in path.bytes() {
                if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
                    out.push(byte as char);
                } else {
                    let _ = write!(out, "%{:02X}", byte);
                }
            }
            out.push_str("\">");
            escaped(&mut out, &entry.name);
            out.push_str("</a> <small></small></li>");
        }
    }
    out.push_str("</ul>");
    out
}

#[cfg(test)]
mod browser_tree_tests {
    use super::*;

    #[test]
    fn folders_and_controls_exist_without_script_and_paths_are_escaped() {
        let mut tree = trueos_math::Tree::<FsEntry, 16>::new();
        let root = tree
            .add_root(FsEntry {
                kind: FsKind::Root,
                name: "/".into(),
            })
            .unwrap();
        let dir = tree
            .add_child(
                root,
                FsEntry {
                    kind: FsKind::Dir,
                    name: "a & \"b\"".into(),
                },
            )
            .unwrap();
        tree.add_child(
            dir,
            FsEntry {
                kind: FsKind::File,
                name: "<file>#é.txt".into(),
            },
        )
        .unwrap();
        tree.add_child(
            root,
            FsEntry {
                kind: FsKind::Dir,
                name: "empty".into(),
            },
        )
        .unwrap();
        let html = browser_tree(&tree, root, 42);
        assert_eq!(html.matches("<details>").count(), 2);
        assert!(!html.contains(" open"));
        assert_eq!(html.matches("</details>").count(), 2);
        assert!(html.contains("<details><summary>empty"));
        assert!(html.contains("data-label=\"a &amp; &quot;b&quot;/\""));
        assert!(html.contains("data-path=\"a &amp; &quot;b&quot;/&lt;file&gt;#é.txt\""));
        assert!(html.contains("href=\"/dl/42/a%20%26%20%22b%22/%3Cfile%3E%23%C3%A9.txt\""));
        assert!(html.contains(">&lt;file&gt;#é.txt</a>"));
        assert_eq!(html.matches("data-action=\"delete\"").count(), 3);
        assert_eq!(html.matches("data-action=\"upload\"").count(), 2);
        assert_eq!(html.matches("data-action=\"mkdir\"").count(), 2);
    }
}
