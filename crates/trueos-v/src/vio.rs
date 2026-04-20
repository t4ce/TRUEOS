extern crate alloc;

pub use crate::env;
pub use crate::vcabi as cabi;
pub use crate::vshell as shell;

pub mod kfs {
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;
    use serde::Deserialize;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum FsEntryKind {
        File,
        Dir,
        Other,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct FsTreeEntry {
        pub path: String,
        pub name: String,
        pub kind: FsEntryKind,
        pub depth: usize,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct FsTreeSnapshot {
        pub version: u32,
        pub root: String,
        pub max_entries: usize,
        pub truncated: bool,
        pub entries: Vec<FsTreeEntry>,
    }

    #[derive(Clone, Debug, Deserialize)]
    struct FsTreeSnapshotWire {
        version: u32,
        root: String,
        max_entries: usize,
        truncated: bool,
        entries: Vec<FsTreeEntryWire>,
    }

    #[derive(Clone, Debug, Deserialize)]
    struct FsTreeEntryWire {
        path: String,
        name: String,
        kind: String,
        depth: usize,
    }

    impl FsEntryKind {
        fn from_wire(kind: &str) -> Self {
            match kind {
                "file" => Self::File,
                "dir" => Self::Dir,
                _ => Self::Other,
            }
        }
    }

    fn parse_snapshot(json: &str) -> Result<FsTreeSnapshot, i32> {
        let wire: FsTreeSnapshotWire = serde_json::from_str(json).map_err(|_| -1)?;
        Ok(FsTreeSnapshot {
            version: wire.version,
            root: wire.root,
            max_entries: wire.max_entries,
            truncated: wire.truncated,
            entries: wire
                .entries
                .into_iter()
                .map(|entry| FsTreeEntry {
                    path: entry.path,
                    name: entry.name,
                    kind: FsEntryKind::from_wire(entry.kind.as_str()),
                    depth: entry.depth,
                })
                .collect(),
        })
    }

    fn normalize_tree_prefix(path: &str) -> String {
        path.trim().trim_matches('/').to_string()
    }

    fn is_under_prefix(entry_path: &str, prefix: &str) -> bool {
        prefix.is_empty()
            || entry_path == prefix
            || entry_path
                .strip_prefix(prefix)
                .map(|rest| rest.starts_with('/'))
                .unwrap_or(false)
    }

    #[inline]
    pub fn read_file(path: &str) -> Result<Vec<u8>, i32> {
        crate::vfs::read_file(path.as_bytes())
    }

    #[inline]
    pub fn read_file_utf8(path: &str) -> Result<String, i32> {
        crate::vfs::read_file_utf8(path.as_bytes())
    }

    #[inline]
    pub fn exists(path: &str) -> Result<bool, i32> {
        crate::vfs::exists(path.as_bytes())
    }

    #[inline]
    pub fn write_file_begin(path: &str, total_len: u64) -> Result<u32, i32> {
        crate::vfs::write_begin(path.as_bytes(), total_len)
    }

    #[inline]
    pub fn write_file_chunk(handle: u32, data: &[u8]) -> Result<(), i32> {
        crate::vfs::write_chunk(handle, data)
    }

    #[inline]
    pub fn write_file_finish(handle: u32) -> Result<(), i32> {
        crate::vfs::write_finish(handle)
    }

    #[inline]
    pub fn write_file_abort(handle: u32) -> Result<(), i32> {
        crate::vfs::write_abort(handle)
    }

    #[inline]
    pub fn write_file(path: &str, data: &[u8]) -> Result<(), i32> {
        crate::vfs::write_file(path.as_bytes(), data)
    }

    #[inline]
    pub fn create_dir_all(path: &str) -> Result<(), i32> {
        crate::vfs::create_dir_all(path.as_bytes())
    }

    #[inline]
    pub fn write_file_utf8(path: &str, data: &str) -> Result<(), i32> {
        crate::vfs::write_file_utf8(path.as_bytes(), data)
    }

    #[inline]
    pub fn remove(path: &str) -> Result<(), i32> {
        crate::vfs::remove(path.as_bytes())
    }

    #[inline]
    pub fn html_tree(max_entries: usize) -> Result<String, i32> {
        crate::vfs::trueosfs_primary_html_tree_utf8(max_entries as u32)
    }

    #[inline]
    pub fn json_all(max_entries: usize) -> Result<String, i32> {
        crate::vfs::trueosfs_json_all_utf8(max_entries as u32)
    }

    #[inline]
    pub fn tree(max_entries: usize) -> Result<FsTreeSnapshot, i32> {
        let json = json_all(max_entries)?;
        parse_snapshot(json.as_str())
    }

    #[inline]
    pub fn list_dir(path: &str, max_entries: usize) -> Result<Vec<FsTreeEntry>, i32> {
        let prefix = normalize_tree_prefix(path);
        let base_depth = if prefix.is_empty() {
            0
        } else {
            prefix.split('/').filter(|segment| !segment.is_empty()).count()
        };

        Ok(tree(max_entries)?
            .entries
            .into_iter()
            .filter(|entry| {
                entry.depth == base_depth
                    && is_under_prefix(entry.path.as_str(), prefix.as_str())
                    && entry.path != prefix
            })
            .collect())
    }

    #[inline]
    pub fn walk_entries(path: &str, max_entries: usize) -> Result<Vec<FsTreeEntry>, i32> {
        let prefix = normalize_tree_prefix(path);
        Ok(tree(max_entries)?
            .entries
            .into_iter()
            .filter(|entry| is_under_prefix(entry.path.as_str(), prefix.as_str()))
            .collect())
    }

    #[inline]
    pub fn walk_files(path: &str, max_entries: usize) -> Result<Vec<String>, i32> {
        Ok(walk_entries(path, max_entries)?
            .into_iter()
            .filter(|entry| matches!(entry.kind, FsEntryKind::File))
            .map(|entry| entry.path)
            .collect())
    }
}
