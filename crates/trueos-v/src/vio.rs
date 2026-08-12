//! Kernel-internal compatibility facade for filesystem-oriented services.

extern crate alloc;

pub use crate::env;
pub use crate::legacy_fs_abi as cabi;
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
        pub id: u64,
        pub path: String,
        pub name: String,
        pub kind: FsEntryKind,
        pub depth: usize,
        pub record_key: trueos_fs::RecordKey,
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
        #[serde(default)]
        id: u64,
        path: String,
        name: String,
        kind: String,
        depth: usize,
        #[serde(default)]
        key: Option<FsRecordKeyWire>,
    }

    #[derive(Clone, Debug, Deserialize)]
    struct FsRecordKeyWire {
        kind: String,
        #[serde(default)]
        provider: String,
        #[serde(default)]
        handle: String,
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

    fn decode_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
        if text.len() != N * 2 {
            return None;
        }
        let mut out = [0u8; N];
        for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
            let high = (pair[0] as char).to_digit(16)? as u8;
            let low = (pair[1] as char).to_digit(16)? as u8;
            out[index] = (high << 4) | low;
        }
        Some(out)
    }

    fn record_key_from_wire(key: Option<FsRecordKeyWire>) -> Result<trueos_fs::RecordKey, i32> {
        let Some(key) = key else {
            // Version-1 listings predate record-key metadata.
            return Ok(trueos_fs::RecordKey::Ffa);
        };
        match key.kind.as_str() {
            "ffa" => Ok(trueos_fs::RecordKey::Ffa),
            "key" => {
                let provider = trueos_crypto::ProviderId::new(
                    decode_hex::<16>(key.provider.as_str()).ok_or(-1)?,
                )
                .ok_or(-1)?;
                let handle =
                    trueos_crypto::KeyHandle::new(decode_hex::<32>(key.handle.as_str()).ok_or(-1)?)
                        .ok_or(-1)?;
                Ok(trueos_fs::RecordKey::Key(trueos_crypto::KeyRef::new(provider, handle)))
            }
            _ => Err(-1),
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
                .map(|entry| {
                    Ok(FsTreeEntry {
                        id: entry.id,
                        path: entry.path,
                        name: entry.name,
                        kind: FsEntryKind::from_wire(entry.kind.as_str()),
                        depth: entry.depth,
                        record_key: record_key_from_wire(entry.key)?,
                    })
                })
                .collect::<Result<Vec<_>, i32>>()?,
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
            prefix
                .split('/')
                .filter(|segment| !segment.is_empty())
                .count()
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn version_one_tree_entries_default_to_ffa() {
            let snapshot = parse_snapshot(
                r#"{"version":1,"root":"/","max_entries":1,"truncated":false,"entries":[{"id":8,"path":"one","name":"one","kind":"file","depth":0}]}"#,
            )
            .unwrap();
            assert_eq!(snapshot.entries[0].record_key, trueos_fs::RecordKey::Ffa);
        }

        #[test]
        fn keyed_tree_entry_decodes_full_key_ref() {
            let snapshot = parse_snapshot(
                r#"{"version":2,"root":"/","max_entries":1,"truncated":false,"entries":[{"id":8,"path":"one","name":"one","kind":"file","depth":0,"key":{"kind":"key","provider":"11111111111111111111111111111111","handle":"abababababababababababababababababababababababababababababababab"}}]}"#,
            )
            .unwrap();
            let expected = trueos_fs::RecordKey::Key(trueos_crypto::KeyRef::new(
                trueos_crypto::ProviderId::new([0x11; 16]).unwrap(),
                trueos_crypto::KeyHandle::new([0xab; 32]).unwrap(),
            ));
            assert_eq!(snapshot.entries[0].record_key, expected);
        }
    }
}
