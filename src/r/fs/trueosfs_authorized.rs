//! Explicit caller authorization at the filesystem publication boundary.
//! Host callers use Host; guests carry their captured VM binding epoch.
//! Every check observes records under the same root lease as its operation.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Actor {
    Host,
    Public,
    Vm(u8, u64),
}
pub(crate) fn actor_for_vm(vm: u8) -> Result<Actor, block::Error> {
    let epoch = crate::access_key_db::principal_epoch(vm).ok_or(block::Error::PermissionDenied)?;
    Ok(Actor::Vm(vm, epoch))
}

pub(crate) fn actor_from_owner(owner: u32) -> Result<Actor, block::Error> {
    if owner & 0x8000_0000 == 0 {
        return Ok(Actor::Host);
    }
    actor_for_vm(u8::try_from(owner & !0x8000_0000).map_err(|_| block::Error::PermissionDenied)?)
}

fn canonical_path(path: &str) -> Result<&str, block::Error> {
    let path = path.trim_matches('/');
    if path
        .bytes()
        .any(|byte| byte == 0 || byte == b'\\' || byte < 32)
        || path
            .split('/')
            .any(|part| matches!(part, "." | "..") || part.is_empty() && !path.is_empty())
    {
        return Err(block::Error::InvalidParam);
    }
    Ok(path)
}

/// This catalog defines ownership and grants; no guest scope bypasses it.
fn reserved_path(path: &str) -> bool {
    path == "users"
        || path.starts_with("users/")
        || ["trueos/uncrypted.blob", "trueos/users-ever-persisted"]
            .iter()
            .any(|reserved| {
                path == *reserved
                    || path
                        .strip_prefix(reserved)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
}

/// Reject known forbidden namespaces without waiting for mount replay or a
/// directory listing that currently owns the root lease. The same checks run
/// again under the lease; this is admission, not cached authorization.
fn preflight_path(actor: Actor, path: &str) -> Result<(), block::Error> {
    if actor == Actor::Host {
        return Ok(());
    }
    authorize_keys(actor, &[])?;
    if reserved_path(canonical_path(path)?) {
        return Err(block::Error::PermissionDenied);
    }
    Ok(())
}

/// Physical records can be shared by all path checks in one read-only lease.
/// This value never escapes that lease. Authorization is deliberately repeated:
/// a cached FFA record must still reject an actor whose VM binding has retired.
#[derive(Default)]
struct LeaseRecords {
    nodes: BTreeMap<String, Option<trueos_fs::NodeRecordRef>>,
}

impl LeaseRecords {
    async fn node(
        &mut self,
        disk: block::DeviceHandle,
        params: &trueos_fs::FsParams,
        path: &str,
    ) -> Result<Option<trueos_fs::NodeRecordRef>, block::Error> {
        if let Some(node) = self.nodes.get(path) {
            return Ok(*node);
        }
        let node = node_record_under_lease(disk, params, path).await?;
        self.nodes.insert(String::from(path), node);
        Ok(node)
    }

    async fn check_path(
        &mut self,
        actor: Actor,
        disk: block::DeviceHandle,
        params: &trueos_fs::FsParams,
        path: &str,
    ) -> Result<(), block::Error> {
        preflight_path(actor, path)?;
        if actor == Actor::Host {
            return Ok(());
        }
        let path = canonical_path(path)?;
        for end in path
            .bytes()
            .enumerate()
            .filter_map(|(index, byte)| (byte == b'/').then_some(index))
            .chain(core::iter::once(path.len()))
        {
            if end == 0 {
                continue;
            }
            match self.node(disk, params, &path[..end]).await? {
                Some(node) => {
                    if end != path.len() && node.kind != NodeKind::Directory {
                        return Err(block::Error::InvalidParam);
                    }
                    // Do not inspect descendants of an already denied root.
                    authorize_keys(actor, &[node.record_key])?;
                }
                None if end == path.len() => {}
                None => return Err(block::Error::NotReady),
            }
        }
        authorize_keys(actor, &[])
    }
}

pub(crate) async fn keys_under_lease(
    disk: block::DeviceHandle,
    params: &trueos_fs::FsParams,
    path: &str,
) -> Result<Vec<RecordKey>, block::Error> {
    let path = canonical_path(path)?;
    let mut keys = Vec::new();
    for end in path
        .bytes()
        .enumerate()
        .filter_map(|(index, byte)| (byte == b'/').then_some(index))
        .chain(core::iter::once(path.len()))
    {
        if end == 0 {
            continue;
        }
        match node_record_under_lease(disk, params, &path[..end]).await? {
            Some(node) => {
                if end != path.len() && node.kind != NodeKind::Directory {
                    return Err(block::Error::InvalidParam);
                }
                if node.record_key != RecordKey::Ffa && !keys.contains(&node.record_key) {
                    keys.push(node.record_key);
                }
            }
            None if end == path.len() => {}
            None => return Err(block::Error::NotReady),
        }
    }
    Ok(keys)
}

pub(crate) async fn check_path_under_lease(
    actor: Actor,
    disk: block::DeviceHandle,
    params: &trueos_fs::FsParams,
    path: &str,
) -> Result<(), block::Error> {
    preflight_path(actor, path)?;
    if actor == Actor::Host {
        return Ok(());
    }
    authorize_keys(actor, &keys_under_lease(disk, params, path).await?)
}

pub(crate) fn authorize_keys(actor: Actor, keys: &[RecordKey]) -> Result<(), block::Error> {
    if let Actor::Vm(vm, epoch) = actor {
        crate::access_key_db::authorize_record_at(vm, epoch, RecordKey::Ffa)
            .map_err(|_| block::Error::PermissionDenied)?;
    }
    for &key in keys {
        if key == RecordKey::Ffa {
            continue;
        }
        match actor {
            Actor::Vm(vm, epoch) => crate::access_key_db::authorize_record_at(vm, epoch, key)
                .map_err(|_| block::Error::PermissionDenied)?,
            Actor::Public => return Err(block::Error::PermissionDenied),
            Actor::Host => {}
        }
    }
    Ok(())
}

/// Whole-tree changes may affect separately keyed descendants of an FFA node.
pub(crate) async fn check_tree_under_lease(
    actor: Actor,
    disk: block::DeviceHandle,
    params: &trueos_fs::FsParams,
    path: &str,
) -> Result<(), block::Error> {
    if actor == Actor::Host {
        return Ok(());
    }
    let path = canonical_path(path)?;
    if path.is_empty() || path == "users" {
        return Err(block::Error::PermissionDenied);
    }
    let mut records = LeaseRecords::default();
    records.check_path(actor, disk, params, path).await?;
    let prefix = normalized_dir_prefix(path);
    let paths = {
        let roots = ROOTS.lock();
        let index = roots
            .iter()
            .find(|mount| mount.disk_id == disk.id())
            .and_then(|mount| mount.index.as_ref())
            .ok_or(block::Error::NotReady)?;
        let mut paths = Vec::new();
        for (key, _) in index.range(prefix.as_bytes().to_vec()..) {
            if !key.starts_with(prefix.as_bytes()) {
                break;
            }
            paths.push(String::from(
                core::str::from_utf8(key).map_err(|_| block::Error::Corrupted)?,
            ));
        }
        paths
    };
    for child in paths {
        records.check_path(actor, disk, params, &child).await?;
    }
    Ok(())
}

pub(super) async fn lease(
    disk: block::DeviceHandle,
) -> Result<
    (
        crate::disc::access::Activity,
        write_gate::RootWriteLease,
        trueos_fs::FsParams,
    ),
    block::Error,
> {
    lease_observed(disk, &mut |_| {}).await
}

async fn lease_observed(
    disk: block::DeviceHandle,
    observe: &mut impl FnMut(ReadRangePhase),
) -> Result<
    (
        crate::disc::access::Activity,
        write_gate::RootWriteLease,
        trueos_fs::FsParams,
    ),
    block::Error,
> {
    let activity = crate::disc::access::Activity::begin(disk)?;
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    observe(ReadRangePhase::Placement);
    let placement = placement_for_io_async(disk)
        .await?
        .ok_or(block::Error::NotReady)?;
    loop {
        observe(ReadRangePhase::Index);
        ensure_index_async(disk, &placement).await?;
        let lease = write_gate::RootWriteLease::acquire(disk.id()).await;
        if ROOTS
            .lock()
            .iter()
            .any(|mount| mount.disk_id == disk.id() && mount.index.is_some())
        {
            return Ok((
                activity,
                lease,
                trueos_fs::FsParams {
                    super_lba: placement.super_lba,
                    data_lba: placement.data_lba,
                    data_end_lba_exclusive: placement.data_end_lba_exclusive,
                },
            ));
        }
        drop(lease);
    }
}

/// A legacy FFA node below a keyed ancestor must not become public merely by
/// moving it outside that ancestor. Its own durable key travels with the node;
/// every remaining source key must still be present at the destination.
pub(crate) async fn check_move_protection_under_lease(
    actor: Actor,
    disk: block::DeviceHandle,
    params: &trueos_fs::FsParams,
    src: &str,
    dst: &str,
) -> Result<(), block::Error> {
    if actor == Actor::Host {
        return Ok(());
    }
    let Some(source) = node_record_under_lease(disk, params, src).await? else {
        return Ok(());
    };
    let source_keys = keys_under_lease(disk, params, src).await?;
    let parent = dst.rsplit_once('/').map_or("", |(parent, _)| parent);
    let destination_keys = keys_under_lease(disk, params, parent).await?;
    if source_keys
        .iter()
        .any(|key| *key != source.record_key && !destination_keys.contains(key))
    {
        return Err(block::Error::PermissionDenied);
    }
    Ok(())
}

pub(crate) async fn node_info(
    actor: Actor,
    disk: block::DeviceHandle,
    path: &str,
) -> Result<Option<NodeInfo>, block::Error> {
    preflight_path(actor, path)?;
    let (_activity, _lease, params) = lease(disk).await?;
    check_path_under_lease(actor, disk, &params, path).await?;
    if path.is_empty() || path == "/" {
        return Ok(Some(NodeInfo {
            revision: 0,
            file_id: 0,
            kind: NodeKind::Directory,
            data_len: 0,
            content_type: ContentTypeId::NONE,
            record_key: RecordKey::Ffa,
        }));
    }
    Ok(node_record_under_lease(disk, &params, path)
        .await?
        .map(|node| NodeInfo {
            revision: node.entry_lba,
            file_id: node.file_id,
            kind: node.kind,
            data_len: node.data_len,
            content_type: node.content_type,
            record_key: node.record_key,
        }))
}

pub(crate) async fn read(
    actor: Actor,
    disk: block::DeviceHandle,
    path: &str,
    max_len: u64,
) -> Result<Option<Vec<u8>>, block::Error> {
    Ok(read_snapshot(actor, disk, path, max_len)
        .await?
        .map(|(bytes, _)| bytes))
}

/// Cached consumers retain these keys with the bytes, including after unlink or
/// rename. Re-authorize the retained identities before serving resident data.
pub(crate) async fn read_snapshot(
    actor: Actor,
    disk: block::DeviceHandle,
    path: &str,
    max_len: u64,
) -> Result<Option<(Vec<u8>, Vec<RecordKey>)>, block::Error> {
    preflight_path(actor, path)?;
    let (_activity, _lease, params) = lease(disk).await?;
    check_path_under_lease(actor, disk, &params, path).await?;
    let keys = keys_under_lease(disk, &params, path).await?;
    let Some(node) = node_record_under_lease(disk, &params, path).await? else {
        return Ok(None);
    };
    if node.kind != NodeKind::File {
        return Err(block::Error::InvalidParam);
    }
    if node.data_len > max_len {
        return Err(block::Error::OutOfBounds);
    }
    Ok(
        trueos_fs::read_file_at_record(&KernelBlockIo::new(disk), &params, &file_record(node))
            .await
            .map_err(map_engine_err)?
            .map(|bytes| (bytes, keys)),
    )
}

fn file_record(node: trueos_fs::NodeRecordRef) -> trueos_fs::FileRecordRef {
    trueos_fs::FileRecordRef {
        file_id: node.file_id,
        entry_lba: node.entry_lba,
        data_lba: node.data_lba,
        data_len: node.data_len,
        content_type: node.content_type,
        record_key: node.record_key,
    }
}

/// Pin an immutable read record as a capability at an authorized open boundary.
/// This does not expose a guest-supplied physical record or a Unix descriptor.
pub(crate) async fn open_read(
    actor: Actor,
    disk: block::DeviceHandle,
    path: &str,
) -> Result<Option<FileReadHandle>, block::Error> {
    preflight_path(actor, path)?;
    let (_activity, _lease, params) = lease(disk).await?;
    check_path_under_lease(actor, disk, &params, path).await?;
    let Some(node) = node_record_under_lease(disk, &params, path).await? else {
        return Ok(None);
    };
    if node.kind != NodeKind::File {
        return Err(block::Error::InvalidParam);
    }
    let mount_seq = ROOTS
        .lock()
        .iter()
        .find(|mount| mount.disk_id == disk.id())
        .map(|mount| mount.seq)
        .ok_or(block::Error::NotReady)?;
    Ok(Some(FileReadHandle {
        actor,
        disk,
        params,
        record: file_record(node),
        mount_seq,
    }))
}

pub(crate) async fn read_range(
    actor: Actor,
    disk: block::DeviceHandle,
    path: &str,
    offset: u64,
    out: &mut [u8],
) -> Result<Option<usize>, block::Error> {
    read_range_observed(actor, disk, path, offset, out, |_| {}).await
}

pub(crate) async fn read_range_observed(
    actor: Actor,
    disk: block::DeviceHandle,
    path: &str,
    offset: u64,
    out: &mut [u8],
    mut observe: impl FnMut(ReadRangePhase),
) -> Result<Option<usize>, block::Error> {
    preflight_path(actor, path)?;
    let (_activity, _lease, params) = lease_observed(disk, &mut observe).await?;
    check_path_under_lease(actor, disk, &params, path).await?;
    let Some(node) = node_record_under_lease(disk, &params, path).await? else {
        return Ok(None);
    };
    if node.kind != NodeKind::File {
        return Err(block::Error::InvalidParam);
    }
    observe(ReadRangePhase::Read);
    trueos_fs::read_file_range_at(
        &KernelBlockIo::new(disk),
        &params,
        &file_record(node),
        offset,
        out,
    )
    .await
    .map_err(map_engine_err)
}

pub(crate) async fn list(
    actor: Actor,
    disk: block::DeviceHandle,
    dir: &str,
    entry_cap: usize,
) -> Result<Option<DirListing>, block::Error> {
    preflight_path(actor, dir)?;
    let (_activity, _lease, params) = lease(disk).await?;
    let mut records = LeaseRecords::default();
    records.check_path(actor, disk, &params, dir).await?;
    let prefix = normalized_dir_prefix(dir);
    if !prefix.is_empty() {
        match records
            .node(disk, &params, prefix.trim_end_matches('/'))
            .await?
        {
            None => return Ok(None),
            Some(node) if node.kind != NodeKind::Directory => {
                return Err(block::Error::InvalidParam);
            }
            _ => {}
        }
    }
    // Snapshot names under the lease, then authorize each child before returning
    // any name/type or applying the public result budget.
    let candidates = {
        let roots = ROOTS.lock();
        let index = roots
            .iter()
            .find(|mount| mount.disk_id == disk.id())
            .and_then(|mount| mount.index.as_ref())
            .ok_or(block::Error::NotReady)?;
        let mut entries = Vec::new();
        for (key, entry) in index.range(prefix.as_bytes().to_vec()..) {
            if !key.starts_with(prefix.as_bytes()) {
                break;
            }
            let name =
                core::str::from_utf8(&key[prefix.len()..]).map_err(|_| block::Error::Corrupted)?;
            if name.is_empty() || name.contains('/') {
                continue;
            }
            let kind = match entry.kind {
                trueos_fs::LogKind::Put => NodeKind::File,
                trueos_fs::LogKind::Directory => NodeKind::Directory,
                _ => continue,
            };
            entries.push(DirEntry {
                name: String::from(name),
                kind,
                content_type: entry.content_type,
            });
        }
        entries
    };
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in candidates {
        let path = alloc::format!("{prefix}{}", entry.name);
        match records.check_path(actor, disk, &params, &path).await {
            Err(block::Error::PermissionDenied) => {
                authorize_keys(actor, &[])?;
                continue;
            }
            Err(error) => return Err(error),
            Ok(()) => {}
        }
        if entries.len() == entry_cap {
            truncated = true;
            break;
        }
        entries.push(entry);
    }
    Ok(Some(DirListing { entries, truncated }))
}

/// Candidate ordering uses only the already resident namespace index. No record
/// headers are read until the output budget and breadth-first order are known.
fn ordered_index_candidates(disk: block::DeviceHandle) -> Result<Vec<IndexNode>, block::Error> {
    let roots = ROOTS.lock();
    let index = roots
        .iter()
        .find(|mount| mount.disk_id == disk.id())
        .and_then(|mount| mount.index.as_ref())
        .ok_or(block::Error::NotReady)?;
    let mut nodes = Vec::new();
    for (key, entry) in index.iter() {
        let kind = match entry.kind {
            trueos_fs::LogKind::Put => NodeKind::File,
            trueos_fs::LogKind::Directory => NodeKind::Directory,
            _ => continue,
        };
        nodes.push(IndexNode {
            path: String::from(core::str::from_utf8(key).map_err(|_| block::Error::Corrupted)?),
            kind,
        });
    }
    drop(roots);
    nodes.sort_by(|a, b| {
        let depth = |node: &IndexNode| node.path.bytes().filter(|byte| *byte == b'/').count();
        depth(a).cmp(&depth(b)).then_with(|| a.path.cmp(&b.path))
    });
    Ok(nodes)
}

async fn filter_index_under_lease(
    actor: Actor,
    disk: block::DeviceHandle,
    params: &trueos_fs::FsParams,
    entry_cap: usize,
    records: &mut LeaseRecords,
) -> Result<(Vec<IndexNode>, bool), block::Error> {
    let mut nodes = Vec::new();
    for node in ordered_index_candidates(disk)? {
        match records.check_path(actor, disk, params, &node.path).await {
            Err(block::Error::PermissionDenied) => {
                authorize_keys(actor, &[])?;
                continue;
            }
            Err(error) => return Err(error),
            Ok(()) => {}
        }
        // One further authorized entry proves truncation. Hidden names never
        // consume the result budget or make the public view appear truncated.
        if nodes.len() == entry_cap {
            authorize_keys(actor, &[])?;
            return Ok((nodes, true));
        }
        nodes.push(node);
    }
    authorize_keys(actor, &[])?;
    Ok((nodes, false))
}

pub(crate) async fn index_snapshot_limited(
    actor: Actor,
    disk: block::DeviceHandle,
    entry_cap: usize,
) -> Result<Option<(Vec<IndexNode>, bool)>, block::Error> {
    authorize_keys(actor, &[])?;
    let (_activity, _lease, params) = lease(disk).await?;
    let mut records = LeaseRecords::default();
    filter_index_under_lease(actor, disk, &params, entry_cap, &mut records)
        .await
        .map(Some)
}

pub(crate) async fn index_snapshot(
    actor: Actor,
    disk: block::DeviceHandle,
) -> Result<Option<Vec<IndexNode>>, block::Error> {
    Ok(index_snapshot_limited(actor, disk, usize::MAX)
        .await?
        .map(|(nodes, _)| nodes))
}

pub(crate) async fn json_all(
    actor: Actor,
    disk: block::DeviceHandle,
    max_entries: usize,
) -> Result<Option<String>, block::Error> {
    authorize_keys(actor, &[])?;
    let (_activity, _lease, params) = lease(disk).await?;
    let limit = if max_entries == 0 {
        TRUEOSFS_LIST_SOFT_CAP
    } else {
        max_entries.min(TRUEOSFS_LIST_SOFT_CAP)
    };
    let mut records = LeaseRecords::default();
    let (candidates, truncated) =
        filter_index_under_lease(actor, disk, &params, limit, &mut records).await?;
    let mut entries = Vec::new();
    for candidate in candidates {
        let path = candidate.path;
        if let Some(node) = records.node(disk, &params, &path).await? {
            entries.push((
                path.bytes().filter(|byte| *byte == b'/').count(),
                path,
                node,
            ));
        }
    }
    authorize_keys(actor, &[])?;
    let visible_limit = if truncated {
        limit.saturating_sub(1)
    } else {
        limit
    };
    let mut out = alloc::format!(
        "{{\"version\":2,\"root\":\"/\",\"max_entries\":{limit},\"truncated\":{},\"entries\":[",
        if truncated { "true" } else { "false" }
    );
    let mut first = true;
    for (depth, path, node) in entries.into_iter().take(visible_limit) {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str("{\"path\":");
        push_json_string_escaped(&mut out, &path);
        out.push_str(",\"name\":");
        push_json_string_escaped(&mut out, path.rsplit('/').next().unwrap_or(""));
        out.push_str(",\"kind\":");
        push_json_string_escaped(
            &mut out,
            if node.kind == NodeKind::Directory {
                "dir"
            } else {
                "file"
            },
        );
        out.push_str(&alloc::format!(
            ",\"depth\":{depth},\"id\":{}",
            node.entry_lba
        ));
        push_record_key_json(&mut out, node.record_key);
        out.push('}');
    }
    if truncated {
        if !first {
            out.push(',');
        }
        out.push_str("{\"path\":\"...\",\"name\":\"...\",\"kind\":\"more\",\"depth\":0,\"id\":0,\"key\":{\"kind\":\"ffa\"}}");
    }
    out.push_str("]}");
    Ok(Some(out))
}

pub(crate) async fn create_dir(
    actor: Actor,
    disk: block::DeviceHandle,
    path: &str,
    exclusive: bool,
) -> Result<bool, CreateDirectoryError> {
    create_directory_with_policy_async(disk, path, exclusive, actor).await
}
pub(crate) async fn create_dir_all(
    actor: Actor,
    disk: block::DeviceHandle,
    path: &str,
) -> Result<bool, block::Error> {
    authorize_keys(actor, &[])?;
    canonical_path(path)?;
    let mut prefix = String::new();
    for part in path.split('/').filter(|part| !part.is_empty()) {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        match create_dir(actor, disk, &prefix, false).await {
            Ok(false) => return Ok(false),
            Ok(true) => {}
            Err(CreateDirectoryError::Device(error)) => return Err(error),
            Err(_) => return Err(block::Error::InvalidParam),
        }
    }
    Ok(true)
}

pub(crate) async fn write(
    actor: Actor,
    disk: block::DeviceHandle,
    path: &str,
    bytes: &[u8],
    content_type: ContentTypeId,
    legacy_blob: bool,
) -> Result<bool, block::Error> {
    file_in_with_metadata_async_for_actor(
        disk,
        path,
        bytes,
        content_type,
        RecordKey::Ffa,
        legacy_blob,
        actor,
    )
    .await
}
pub(crate) async fn write_checked(
    actor: Actor,
    disk: block::DeviceHandle,
    path: &str,
    bytes: &[u8],
    content_type: ContentTypeId,
    create_new: bool,
    expected: Option<[u8; 32]>,
    infer_blob: bool,
    reset_blob: bool,
) -> Result<bool, CheckedWriteError> {
    file_in_checked_with_type_policy_async(
        disk,
        path,
        bytes,
        content_type,
        create_new,
        expected,
        if reset_blob {
            CheckedWriteTypePolicy::ResetBlob
        } else if infer_blob {
            CheckedWriteTypePolicy::InferBlob
        } else {
            CheckedWriteTypePolicy::Retain
        },
        actor,
    )
    .await
}
pub(crate) async fn write_begin(
    actor: Actor,
    disk: block::DeviceHandle,
    path: &str,
    len: u64,
    content_type: ContentTypeId,
    legacy_blob: bool,
) -> Result<Option<u32>, block::Error> {
    file_write_begin_with_metadata_async_for_actor(
        disk,
        path,
        len,
        content_type,
        RecordKey::Ffa,
        legacy_blob,
        actor,
    )
    .await
}
pub(crate) async fn remove(
    actor: Actor,
    disk: block::DeviceHandle,
    path: &str,
    leaf_kind: Option<NodeKind>,
) -> Result<bool, RemoveNodeError> {
    remove_node_with_policy_async(disk, path, leaf_kind, actor).await
}
pub(crate) async fn rename(
    actor: Actor,
    disk: block::DeviceHandle,
    src: &str,
    dst: &str,
    replace_file: bool,
    directory: bool,
) -> Result<bool, block::Error> {
    if directory {
        dir_rename_for_actor_async(disk, src, dst, actor).await
    } else {
        file_rename_with_policy_async(disk, src, dst, replace_file, actor).await
    }
}

pub(crate) async fn select(
    actor: Actor,
    disk: block::DeviceHandle,
    folder: &str,
    content_type: ContentTypeId,
    max_depth: u8,
) -> Result<trueos_fs::selection::FileSelection, block::Error> {
    let base = canonical_path(folder)?;
    let full_path = |relative: String| {
        if relative.is_empty() {
            String::from(base)
        } else if base.is_empty() {
            relative
        } else {
            alloc::format!("{base}/{relative}")
        }
    };
    trueos_fs::selection::select_files(
        content_type,
        max_depth,
        |relative| {
            let path = full_path(relative);
            async move {
                let listing = list(actor, disk, &path, trueos_fs::selection::ENTRY_CAP)
                    .await?
                    .ok_or(block::Error::InvalidParam)?;
                Ok((listing.entries, listing.truncated))
            }
        },
        |relative| {
            let path = full_path(relative);
            async move {
                let mut prefix = [0; 4096];
                let len = read_range(actor, disk, &path, 0, &mut prefix)
                    .await?
                    .ok_or(block::Error::InvalidParam)?;
                Ok(infer::get(&prefix[..len])
                    .map(|kind| kind.content_type_id())
                    .filter(|id| *id != ContentTypeId::NONE))
            }
        },
    )
    .await
    .map_err(|error| match error {
        trueos_fs::selection::SelectionError::Io(error) => error,
        _ => block::Error::InvalidParam,
    })
}

pub(crate) async fn assert_private_path(
    actor: Actor,
    disk: block::DeviceHandle,
    path: &str,
) -> Result<(), block::Error> {
    let Actor::Vm(vm, epoch) = actor else {
        return Err(block::Error::PermissionDenied);
    };
    let path = canonical_path(path)?;
    let workspace = crate::access_key_db::workspace_for_vm_at(vm, epoch)
        .ok_or(block::Error::PermissionDenied)?;
    let workspace = canonical_path(&workspace)?;
    if path != workspace
        && !path
            .strip_prefix(workspace)
            .is_some_and(|suffix| suffix.starts_with('/'))
    {
        return Err(block::Error::PermissionDenied);
    }
    let (_activity, _lease, params) = lease(disk).await?;
    let root = node_record_under_lease(disk, &params, workspace)
        .await?
        .ok_or(block::Error::PermissionDenied)?;
    if root.kind != NodeKind::Directory || root.record_key == RecordKey::Ffa {
        return Err(block::Error::PermissionDenied);
    }
    crate::access_key_db::authorize_protected_workspace_at(vm, epoch, root.record_key)
        .map_err(|_| block::Error::PermissionDenied)?;
    check_path_under_lease(actor, disk, &params, path).await
}
