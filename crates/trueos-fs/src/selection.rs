//! Bounded, content-typed recursive selection shared by TRUEOSFS and its ABI.
use alloc::{format, string::String, vec, vec::Vec};
use core::future::Future;
use crate::{ContentTypeId, DirEntry, NodeKind};

pub const DEFAULT_DEPTH: u8 = 8;
pub const MAX_DEPTH: u8 = 64;
pub const ENTRY_CAP: usize = 4096;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileSelection {
    /// Sorted paths relative to the requested folder; never filename patterns.
    pub files: Vec<String>,
    /// Some child folders lay below the requested depth.
    pub depth_limited: bool,
    /// A directory listing, traversal budget, or result budget was exhausted.
    pub truncated: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub enum SelectionError<E> { InvalidQuery, InvalidListing, Io(E) }

pub fn valid_query(content_type: ContentTypeId, max_depth: u8) -> bool {
    content_type != ContentTypeId::NONE && content_type.is_registered() && max_depth <= MAX_DEPTH
}

fn valid_relative(path: &str) -> bool {
    !path.is_empty() && !path.contains('\0') && !path.contains('\\')
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Depth zero includes only immediate files; depth eight includes files under
/// eight child folders. The caller provides mounted-root-confined directory
/// listing and optional signature probing for old BLOB records. Stored types
/// remain authoritative; probing never retags files or trusts their suffixes.
/// Any I/O failure aborts the selection instead of returning a partial success.
pub async fn select_files<E, L, LF, P, PF>(
    content_type: ContentTypeId, max_depth: u8, mut list: L, mut probe: P,
) -> Result<FileSelection, SelectionError<E>>
where
    L: FnMut(String) -> LF,
    LF: Future<Output = Result<(Vec<DirEntry>, bool), E>>,
    P: FnMut(String) -> PF,
    PF: Future<Output = Result<Option<ContentTypeId>, E>>,
{
    if !valid_query(content_type, max_depth) { return Err(SelectionError::InvalidQuery); }
    let mut result = FileSelection::default();
    let mut pending = vec![(String::new(), 0u8)];
    let mut admitted_dirs = 1;
    while let Some((dir, depth)) = pending.pop() {
        let (mut entries, truncated) = list(dir.clone()).await.map_err(SelectionError::Io)?;
        result.truncated |= truncated;
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for entry in entries {
            if !valid_relative(&entry.name) || entry.name.contains('/') {
                return Err(SelectionError::InvalidListing);
            }
            let path = if dir.is_empty() { entry.name } else { format!("{dir}/{}", entry.name) };
            if entry.kind == NodeKind::Directory {
                if depth == max_depth { result.depth_limited = true; }
                else if admitted_dirs == ENTRY_CAP { result.truncated = true; }
                else { admitted_dirs += 1; pending.push((path, depth + 1)); }
                continue;
            }
            let selected = entry.content_type == content_type || (entry.content_type == ContentTypeId::BLOB
                && content_type != ContentTypeId::BLOB
                && probe(path.clone()).await.map_err(SelectionError::Io)? == Some(content_type));
            if selected {
                if result.files.len() == ENTRY_CAP { result.truncated = true; continue; }
                result.files.push(path);
            }
        }
    }
    result.files.sort();
    result.files.dedup();
    Ok(result)
}

/// TFS1: magic, flag byte (depth/truncated), three reserved zeros, count u32,
/// then count repetitions of (UTF-8 relative-path length u16, path bytes).
pub fn encode(selection: &FileSelection) -> Option<Vec<u8>> {
    if selection.files.len() > ENTRY_CAP { return None; }
    let mut out = b"TFS1".to_vec();
    out.extend_from_slice(&[u8::from(selection.depth_limited) | (u8::from(selection.truncated) << 1), 0, 0, 0]);
    out.extend_from_slice(&(selection.files.len() as u32).to_le_bytes());
    for path in &selection.files {
        if !valid_relative(path) { return None; }
        out.extend_from_slice(&u16::try_from(path.len()).ok()?.to_le_bytes());
        out.extend_from_slice(path.as_bytes());
    }
    Some(out)
}

pub fn decode(bytes: &[u8]) -> Option<FileSelection> {
    if bytes.len() < 12 || &bytes[..4] != b"TFS1" || bytes[4] & !3 != 0 || bytes[5..8] != [0; 3] { return None; }
    let count = u32::from_le_bytes(bytes[8..12].try_into().ok()?) as usize;
    if count > ENTRY_CAP { return None; }
    let mut result = FileSelection { files: Vec::new(), depth_limited: bytes[4] & 1 != 0, truncated: bytes[4] & 2 != 0 };
    let mut rest = &bytes[12..];
    for _ in 0..count {
        let len = u16::from_le_bytes(rest.get(..2)?.try_into().ok()?) as usize;
        rest = &rest[2..];
        let path = core::str::from_utf8(rest.get(..len)?).ok()?;
        if !valid_relative(path) { return None; }
        result.files.push(String::from(path));
        rest = &rest[len..];
    }
    rest.is_empty().then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{pin::pin, task::{Context, Poll, Waker}};
    use std::{cell::RefCell, collections::BTreeMap};

    fn run<F: Future>(future: F) -> F::Output {
        let mut future = pin!(future);
        match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("test callbacks are immediately ready"),
        }
    }
    fn file(name: &str, ty: ContentTypeId) -> DirEntry {
        DirEntry { name: name.into(), kind: NodeKind::File, content_type: ty }
    }
    fn dir(name: &str) -> DirEntry {
        DirEntry { name: name.into(), kind: NodeKind::Directory, content_type: ContentTypeId::NONE }
    }

    #[test]
    fn typed_selection_ignores_suffixes_probes_only_blobs_and_obeys_depth() {
        let map = ContentTypeId::WARCRAFT3_MAP;
        let tree = BTreeMap::from([
            ("", vec![file("renamed.data", map), file("fake.w3m", ContentTypeId::PNG),
                file("old.w3m", ContentTypeId::BLOB), file("bad.w3m", ContentTypeId::BLOB), dir("sub")]),
            ("sub", vec![file("nested.w3m", map), dir("deep")]),
            ("sub/deep", vec![file("last.w3m", map)]),
        ]);
        let probes = RefCell::new(Vec::new());
        let select = |depth| run(select_files(map, depth,
            |path| core::future::ready(Ok::<_, ()>((tree[path.as_str()].clone(), false))),
            |path| {
                probes.borrow_mut().push(path.clone());
                core::future::ready(Ok(if path == "old.w3m" { Some(map) } else { None }))
            },
        )).unwrap();
        let shallow = select(0);
        assert_eq!(shallow.files, vec!["old.w3m", "renamed.data"]);
        assert!(shallow.depth_limited);
        assert!(!shallow.truncated);
        let one = select(1);
        assert_eq!(one.files, vec!["old.w3m", "renamed.data", "sub/nested.w3m"]);
        assert!(one.depth_limited);
        let all = select(8);
        assert_eq!(all.files, vec!["old.w3m", "renamed.data", "sub/deep/last.w3m", "sub/nested.w3m"]);
        assert!(!all.depth_limited);
        assert!(probes.borrow().iter().all(|p| p == "old.w3m" || p == "bad.w3m"));
        assert_eq!(decode(&encode(&all).unwrap()), Some(all));
    }

    #[test]
    fn limits_and_errors_are_not_silent() {
        let map = ContentTypeId::WARCRAFT3_MAP;
        assert!(!valid_query(ContentTypeId::NONE, 8));
        assert!(!valid_query(ContentTypeId::from_raw(u32::MAX), 8));
        assert!(!valid_query(map, 65));
        let listing = (0..ENTRY_CAP + 1).map(|i| file(&format!("{i}"), map)).collect::<Vec<_>>();
        let result = run(select_files(map, 0,
            |_| core::future::ready(Ok::<_, i32>((listing.clone(), false))),
            |_| core::future::ready(Ok(None)),
        )).unwrap();
        assert!(result.truncated);
        assert_eq!(result.files.len(), ENTRY_CAP);
        let error = run(select_files(map, 8,
            |_| core::future::ready(Err::<(Vec<DirEntry>, bool), _>(-42)),
            |_| core::future::ready(Ok(None)),
        ));
        assert_eq!(error, Err(SelectionError::Io(-42)));
        let invalid = run(select_files(map, 8,
            |_| core::future::ready(Ok::<_, ()>((vec![dir("..")], false))),
            |_| core::future::ready(Ok(None)),
        ));
        assert_eq!(invalid, Err(SelectionError::InvalidListing));
        let truncated = run(select_files(map, 0,
            |_| core::future::ready(Ok::<_, ()>((Vec::new(), true))),
            |_| core::future::ready(Ok(None)),
        )).unwrap();
        assert!(truncated.truncated);
    }

    #[test]
    fn wire_rejects_traversal_and_malformed_results() {
        let sample = FileSelection { files: vec!["a/b.w3m".into()], depth_limited: true, truncated: true };
        let wire = encode(&sample).unwrap();
        assert_eq!(decode(&wire), Some(sample));
        for len in 0..wire.len() { assert!(decode(&wire[..len]).is_none()); }
        let mut extra = wire.clone(); extra.push(0);
        assert!(decode(&extra).is_none());
        for path in ["../bad", "/absolute", "a//b", "a/./b", "a\\b", "a\0b"] {
            assert!(encode(&FileSelection { files: vec![path.into()], ..Default::default() }).is_none());
        }
    }
}
