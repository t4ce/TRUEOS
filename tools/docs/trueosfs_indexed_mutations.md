# Indexed TRUEOSFS mutations

Mounted filesystem operations must resolve live paths through the mounted
index. Reconstructing the namespace from the entire append-only log for every
mutation makes operation latency depend on unrelated historical activity.
Codex's stale-directory cleanup exposed this platform issue; it is not a
Codex-specific cleanup defect.

The mounted removal, file move and directory move paths now follow the same
sequence:

1. Ensure the index exists before acquiring the root write lease. Index
   publication may checkpoint, which itself needs that lease.
2. Acquire the lease and recheck the source, destination and parent as needed.
   If the index disappeared while waiting, release the lease and rebuild it.
3. Read and validate the indexed physical record, then append the mutation.
4. Update the mounted index before releasing the lease, invalidate affected
   file-cache entries and notify watchers.

`remove_recursive_async` uses a bounded record delete for files or one durable
tree tombstone for directories. `file_delete_async` follows the same validation
order and retains its file-only contract. Exact slash-delimited subtree
matching preserves names such as `treehouse` when removing `tree`.

`file_rename_async` appends one durable RenameTree namespace record and remaps
the existing physical file reference. It preserves the payload location, file
identity, raw content type and record key. There is no payload copy or source
deletion, and no history scan to rediscover a destination record. Replay applies
the same exact-key mapping after remount.

`dir_rename_async` validates parent/destination and subtree collisions through
the index, then appends a single RenameTree record. File payloads are not copied.
Physical record names may differ from logical paths after earlier directory
moves; indexed engine helpers validate record identity without requiring those
names to match. RenameTree paths are bounded by the replay format's 4096-byte
limit before any write.

The prevalidated engine entry points require the mounted caller's current index
and write lease. Standalone compatibility engine APIs still reconstruct the
namespace when no mounted index is supplied.

The POSIX shim, Blueprint Rust std overlay and vendored Tokio TRUEOS rename
bindings now use this native async namespace operation. Existing destinations
return EEXIST/AlreadyExists. POSIX replacement semantics remain unsupported;
there is no copy/delete fallback. Same-name renames require an existing source.
Moves across the persistent and private RAM filesystems are rejected.

Validation includes 36 host engine tests with bounded-read assertions after log
growth, replay after repeated directory moves and preservation of opaque content
identity/key metadata. The compiled-production mounted fixtures cover 7 removal
and 8 move cases, including lease races, rejected destinations, parent checks,
warm-index retention, unchanged payload LBAs and failed namespace writes. Three
compiled POSIX binding cases verify dispatch, existing-target errors and missing
same-name sources. Hardware validation is pending.
