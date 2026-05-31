extern crate alloc;

use alloc::vec::Vec;

const MI_BATCH_BUFFER_START_MASK: u32 = 0xFF80_0000;
const MI_BATCH_BUFFER_START_PREFIX: u32 = 0x1880_0000;
const REPLAY_SCAN_MAX_HITS: usize = 24;
const TAR_BLOCK_BYTES: usize = 512;

#[derive(Copy, Clone, Debug)]
pub(crate) struct ReplayBoSpec {
    pub(crate) handle: u32,
    pub(crate) gpu_va: u64,
    pub(crate) size: usize,
    pub(crate) flags: u64,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ReplayPatch {
    pub(crate) handle: u32,
    pub(crate) offset: usize,
    pub(crate) bytes: &'static [u8],
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ReplaySubmit {
    pub(crate) seq: u32,
    pub(crate) batch_gpu: u64,
    pub(crate) batch_start: u64,
    pub(crate) flags: u64,
    pub(crate) patches: &'static [ReplayPatch],
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ReplayPresent {
    pub(crate) handle: u32,
    pub(crate) offset: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: usize,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum ReplayModuleLoadMode {
    VisibleTruncated,
    Full,
}

#[derive(Copy, Clone, Debug)]
struct ReplayBoBacking {
    handle: u32,
    gpu_va: u64,
    phys: u64,
    virt: *mut u8,
    size: usize,
}

pub(crate) fn submit_replay_frame(
    bos: &'static [ReplayBoSpec],
    base_patches: &'static [ReplayPatch],
    submit: ReplaySubmit,
) -> super::gpgpu::RenderReplayProof {
    submit_replay_frame_visible(bos, base_patches, submit, None)
}

pub(crate) fn submit_replay_frame_visible(
    bos: &'static [ReplayBoSpec],
    base_patches: &'static [ReplayPatch],
    submit: ReplaySubmit,
    present: Option<ReplayPresent>,
) -> super::gpgpu::RenderReplayProof {
    submit_replay_frame_visible_from_module(
        bos,
        base_patches,
        submit,
        present,
        None,
        ReplayModuleLoadMode::Full,
    )
}

pub(crate) fn submit_replay_frame_visible_from_module(
    bos: &'static [ReplayBoSpec],
    base_patches: &'static [ReplayPatch],
    submit: ReplaySubmit,
    present: Option<ReplayPresent>,
    module_string: Option<&'static [u8]>,
    load_mode: ReplayModuleLoadMode,
) -> super::gpgpu::RenderReplayProof {
    let Some(backing) = allocate_bos(bos) else {
        return replay_failure(submit.batch_gpu);
    };
    if let Some(module_string) = module_string {
        if !load_replay_module_dumps(&backing, module_string, load_mode) {
            return replay_failure(submit.batch_gpu);
        }
    }
    if !apply_patches(&backing, base_patches) || !apply_patches(&backing, submit.patches) {
        return replay_failure(submit.batch_gpu);
    }
    log_replay_artifact_scan(&backing, submit.batch_gpu);
    let ranges = ppgtt_ranges(&backing);
    let proof = super::gpgpu::submit_render_replay_probe(submit.batch_gpu, &ranges);
    log_probe_address("batch-start", submit.batch_gpu, &backing);
    let acthd = canonicalize_gen48_gpu_addr(((proof.acthd_hi as u64) << 32) | proof.acthd as u64);
    log_probe_address("acthd", acthd, &backing);
    let bbaddr =
        canonicalize_gen48_gpu_addr(((proof.bbaddr_hi as u64) << 32) | proof.bbaddr_lo as u64);
    log_probe_address("bbaddr", bbaddr, &backing);
    log_probe_address("bbaddr-bit0-clear", bbaddr & !1, &backing);
    log_probe_address("bbaddr-page", bbaddr & !0xFFF, &backing);
    log_replay_edge_probe(&backing, bbaddr);
    crate::log!(
        "intel/replay: frame seq={} batch_gpu=0x{:X} batch_start=0x{:X} flags=0x{:X} bo_count={} submitted={} retired={} fault8=0x{:08X} fault12=0x{:08X}\n",
        submit.seq,
        submit.batch_gpu,
        submit.batch_start,
        submit.flags,
        backing.len(),
        proof.submitted as u8,
        proof.retired as u8,
        proof.fault8,
        proof.fault12,
    );
    if let Some(present) = present {
        let presented = present_replay_bo(&backing, present);
        crate::log!(
            "intel/replay: present handle={} offset=0x{:X} size={}x{} pitch=0x{:X} retired={} presented={}\n",
            present.handle,
            present.offset,
            present.width,
            present.height,
            present.pitch_bytes,
            proof.retired as u8,
            presented as u8,
        );
    }
    proof
}

fn replay_failure(batch_gpu: u64) -> super::gpgpu::RenderReplayProof {
    super::gpgpu::RenderReplayProof {
        submitted: false,
        retired: false,
        pml4_phys: 0,
        table_pages: 0,
        batch_gpu,
        pre_marker: 0,
        post_marker: 0,
        head: 0,
        tail: 0,
        acthd: 0,
        acthd_hi: 0,
        bbaddr_lo: 0,
        bbaddr_hi: 0,
        ipehr: 0,
        eir: 0,
        fault8: 0,
        fault12: 0,
    }
}

fn allocate_bos(specs: &[ReplayBoSpec]) -> Option<Vec<ReplayBoBacking>> {
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        let (phys, virt) = crate::dma::alloc(spec.size, crate::intel::WARM_ALIGN)?;
        unsafe {
            core::ptr::write_bytes(virt, 0, spec.size);
        }
        out.push(ReplayBoBacking {
            handle: spec.handle,
            gpu_va: spec.gpu_va,
            phys,
            virt,
            size: spec.size,
        });
        crate::log!(
            "intel/replay: bo-alloc handle={} gpu=0x{:X} phys=0x{:X} size=0x{:X} flags=0x{:X}\n",
            spec.handle,
            spec.gpu_va,
            phys,
            spec.size,
            spec.flags,
        );
    }
    Some(out)
}

fn load_replay_module_dumps(
    backing: &[ReplayBoBacking],
    module_string: &'static [u8],
    load_mode: ReplayModuleLoadMode,
) -> bool {
    let Some(module) = crate::limine::module_bytes_by_string(module_string) else {
        crate::log!(
            "intel/replay: module-load present=0 string={} action=abort-zero-batch\n",
            core::str::from_utf8(module_string).unwrap_or("<non-utf8>"),
        );
        return false;
    };
    let mut cursor = 0usize;
    let mut loaded = 0usize;
    let mut copied = 0usize;
    while cursor + TAR_BLOCK_BYTES <= module.len() {
        let header = &module[cursor..cursor + TAR_BLOCK_BYTES];
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let name_len = header[..100].iter().position(|b| *b == 0).unwrap_or(100);
        let name = &header[..name_len];
        let Some(size) = parse_tar_octal(&header[124..136]) else {
            crate::log!("intel/replay: module-load bad-tar-size action=abort\n");
            return false;
        };
        let data_start = cursor + TAR_BLOCK_BYTES;
        let Some(data_end) = data_start.checked_add(size) else {
            return false;
        };
        if data_end > module.len() {
            crate::log!("intel/replay: module-load truncated-tar action=abort\n");
            return false;
        }
        if let Some((handle, dump_off, declared_len)) = parse_dump_name(name) {
            if should_skip_replay_dump(load_mode, handle, dump_off, declared_len) {
                crate::log!(
                    "intel/replay: module-load skip handle={} off=0x{:X} len=0x{:X} mode={:?} reason=visible-truncated-rung\n",
                    handle,
                    dump_off,
                    declared_len,
                    load_mode,
                );
                let padded = (size + TAR_BLOCK_BYTES - 1) & !(TAR_BLOCK_BYTES - 1);
                let Some(next) = data_start.checked_add(padded) else {
                    return false;
                };
                cursor = next;
                continue;
            }
            let copy_len = size.min(declared_len);
            let Some(bo) = backing.iter().find(|bo| bo.handle == handle) else {
                crate::log!(
                    "intel/replay: module-load unknown-handle handle={} name={} action=abort\n",
                    handle,
                    core::str::from_utf8(name).unwrap_or("<non-utf8>"),
                );
                return false;
            };
            let Some(copy_end) = dump_off.checked_add(copy_len) else {
                return false;
            };
            if copy_end > bo.size {
                crate::log!(
                    "intel/replay: module-load out-of-bo handle={} off=0x{:X} len=0x{:X} size=0x{:X} action=abort\n",
                    handle,
                    dump_off,
                    copy_len,
                    bo.size,
                );
                return false;
            }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    module.as_ptr().add(data_start),
                    bo.virt.add(dump_off),
                    copy_len,
                );
            }
            crate::intel::dma_flush(unsafe { bo.virt.add(dump_off) }, copy_len);
            loaded += 1;
            copied += copy_len;
            crate::log!(
                "intel/replay: module-load dump={} handle={} off=0x{:X} len=0x{:X} file_len=0x{:X}\n",
                loaded,
                handle,
                dump_off,
                copy_len,
                size,
            );
        }
        let padded = (size + TAR_BLOCK_BYTES - 1) & !(TAR_BLOCK_BYTES - 1);
        let Some(next) = data_start.checked_add(padded) else {
            return false;
        };
        cursor = next;
    }
    crate::log!(
        "intel/replay: module-load present=1 bytes=0x{:X} dumps={} copied=0x{:X}\n",
        module.len(),
        loaded,
        copied,
    );
    loaded != 0
}

fn should_skip_replay_dump(
    load_mode: ReplayModuleLoadMode,
    handle: u32,
    dump_off: usize,
    declared_len: usize,
) -> bool {
    match load_mode {
        ReplayModuleLoadMode::Full => false,
        ReplayModuleLoadMode::VisibleTruncated => handle == 8 && dump_off == 0,
    }
}

fn parse_tar_octal(bytes: &[u8]) -> Option<usize> {
    let mut value = 0usize;
    let mut seen = false;
    for b in bytes {
        match *b {
            0 | b' ' => {
                if seen {
                    break;
                }
            }
            b'0'..=b'7' => {
                seen = true;
                value = value.checked_mul(8)?.checked_add((b - b'0') as usize)?;
            }
            _ => return None,
        }
    }
    Some(value)
}

fn parse_dump_name(name: &[u8]) -> Option<(u32, usize, usize)> {
    if !name.starts_with(b"dumps/") || !name.ends_with(b".bin") {
        return None;
    }
    let handle_pos = find_bytes(name, b"handle_")? + b"handle_".len();
    let off_tag = find_bytes_from(name, b"_off_0x", handle_pos)?;
    let handle = parse_decimal_u32(&name[handle_pos..off_tag])?;
    let off_start = off_tag + b"_off_0x".len();
    let len_tag = find_bytes_from(name, b"_len_0x", off_start)?;
    let off = parse_hex_usize(&name[off_start..len_tag])?;
    let len_start = len_tag + b"_len_0x".len();
    let len_end = name.len().checked_sub(b".bin".len())?;
    let len = parse_hex_usize(&name[len_start..len_end])?;
    Some((handle, off, len))
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    find_bytes_from(haystack, needle, 0)
}

fn find_bytes_from(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || start > haystack.len() || needle.len() > haystack.len() {
        return None;
    }
    let end = haystack.len().checked_sub(needle.len())?;
    let mut pos = start;
    while pos <= end {
        if &haystack[pos..pos + needle.len()] == needle {
            return Some(pos);
        }
        pos += 1;
    }
    None
}

fn parse_decimal_u32(bytes: &[u8]) -> Option<u32> {
    let mut value = 0u32;
    if bytes.is_empty() {
        return None;
    }
    for b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(value)
}

fn parse_hex_usize(bytes: &[u8]) -> Option<usize> {
    let mut value = 0usize;
    if bytes.is_empty() {
        return None;
    }
    for b in bytes {
        let digit = match *b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        } as usize;
        value = value.checked_mul(16)?.checked_add(digit)?;
    }
    Some(value)
}

fn canonicalize_gen48_gpu_addr(addr: u64) -> u64 {
    const BIT47: u64 = 1u64 << 47;
    const LOW48_MASK: u64 = (1u64 << 48) - 1;
    let low48 = addr & LOW48_MASK;
    if low48 & BIT47 != 0 {
        low48 | !LOW48_MASK
    } else {
        low48
    }
}

fn apply_patches(backing: &[ReplayBoBacking], patches: &[ReplayPatch]) -> bool {
    for patch in patches {
        let Some(bo) = backing.iter().find(|bo| bo.handle == patch.handle) else {
            return false;
        };
        let Some(end) = patch.offset.checked_add(patch.bytes.len()) else {
            return false;
        };
        if end > bo.size {
            return false;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(
                patch.bytes.as_ptr(),
                bo.virt.add(patch.offset),
                patch.bytes.len(),
            );
        }
        crate::intel::dma_flush(unsafe { bo.virt.add(patch.offset) }, patch.bytes.len());
    }
    true
}

fn log_replay_artifact_scan(backing: &[ReplayBoBacking], batch_gpu: u64) {
    log_probe_address("pre-submit-batch-start", batch_gpu, backing);
    let batch_page = batch_gpu & !0xFFF;
    if batch_page != batch_gpu {
        log_probe_address("pre-submit-batch-page", batch_page, backing);
    }

    let mut hits = 0usize;
    for bo in backing {
        let dword_count = bo.size / core::mem::size_of::<u32>();
        let dwords = unsafe { core::slice::from_raw_parts(bo.virt as *const u32, dword_count) };
        let mut index = 0usize;
        while index + 2 < dwords.len() {
            let word = unsafe { core::ptr::read_volatile(dwords.as_ptr().add(index)) };
            if (word & MI_BATCH_BUFFER_START_MASK) == MI_BATCH_BUFFER_START_PREFIX {
                let lo = unsafe { core::ptr::read_volatile(dwords.as_ptr().add(index + 1)) };
                let hi = unsafe { core::ptr::read_volatile(dwords.as_ptr().add(index + 2)) };
                let target = ((hi as u64) << 32) | lo as u64;
                let target_clear = target & !1;
                let resolved = resolve_gpu(target, backing).is_some();
                let resolved_clear = resolve_gpu(target_clear, backing).is_some();
                crate::log!(
                    "intel/replay: bbs-scan hit={} src_handle={} src_gpu=0x{:X} src_off=0x{:X} cmd=0x{:08X} target=0x{:X} target_clear=0x{:X} resolved={} resolved_clear={}\n",
                    hits,
                    bo.handle,
                    bo.gpu_va + (index * core::mem::size_of::<u32>()) as u64,
                    index * core::mem::size_of::<u32>(),
                    word,
                    target,
                    target_clear,
                    resolved as u8,
                    resolved_clear as u8,
                );
                hits += 1;
                if hits >= REPLAY_SCAN_MAX_HITS {
                    crate::log!(
                        "intel/replay: bbs-scan truncated max_hits={} note=pre-submit-patched-artifact\n",
                        REPLAY_SCAN_MAX_HITS,
                    );
                    return;
                }
            }
            index += 1;
        }
    }
    crate::log!("intel/replay: bbs-scan done hits={} note=pre-submit-patched-artifact\n", hits,);
}

fn log_replay_edge_probe(backing: &[ReplayBoBacking], bbaddr: u64) {
    for bo in backing {
        let bo_end = bo.gpu_va.saturating_add(bo.size as u64);
        if bbaddr == bo_end || (bbaddr & !1) == bo_end || (bbaddr & !0xFFF) == bo_end {
            let probe_off = bo.size.saturating_sub(0x40);
            let dwords = read_probe_dwords(bo, probe_off);
            crate::log!(
                "intel/replay: edge-probe bbaddr=0x{:X} handle={} bo_gpu=0x{:X} bo_end=0x{:X} probe_off=0x{:X} tail_dw=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
                bbaddr,
                bo.handle,
                bo.gpu_va,
                bo_end,
                probe_off,
                dwords[0],
                dwords[1],
                dwords[2],
                dwords[3],
            );
        }
    }
}

fn log_probe_address(label: &'static str, gpu: u64, backing: &[ReplayBoBacking]) {
    let Some(bo) = resolve_gpu(gpu, backing) else {
        crate::log!("intel/replay: probe label={} gpu=0x{:X} resolved=0\n", label, gpu,);
        return;
    };
    let off = (gpu - bo.gpu_va) as usize;
    let dwords = read_probe_dwords(bo, off);
    crate::log!(
        "intel/replay: probe label={} gpu=0x{:X} resolved=1 handle={} bo_gpu=0x{:X} off=0x{:X} size=0x{:X} dw=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
        label,
        gpu,
        bo.handle,
        bo.gpu_va,
        off,
        bo.size,
        dwords[0],
        dwords[1],
        dwords[2],
        dwords[3],
    );
}

fn resolve_gpu(gpu: u64, backing: &[ReplayBoBacking]) -> Option<&ReplayBoBacking> {
    backing
        .iter()
        .find(|bo| gpu >= bo.gpu_va && gpu < bo.gpu_va.saturating_add(bo.size as u64))
}

fn read_probe_dwords(bo: &ReplayBoBacking, off: usize) -> [u32; 4] {
    let mut out = [0u32; 4];
    for (index, slot) in out.iter_mut().enumerate() {
        let byte_off = off.saturating_add(index * core::mem::size_of::<u32>());
        if byte_off + core::mem::size_of::<u32>() <= bo.size {
            *slot = unsafe { core::ptr::read_volatile(bo.virt.add(byte_off) as *const u32) };
        }
    }
    out
}

fn present_replay_bo(backing: &[ReplayBoBacking], present: ReplayPresent) -> bool {
    let Some(bo) = backing.iter().find(|bo| bo.handle == present.handle) else {
        return false;
    };
    let Some(bytes) = present
        .pitch_bytes
        .checked_mul(present.height as usize)
        .and_then(|len| present.offset.checked_add(len))
    else {
        return false;
    };
    if bytes > bo.size || present.pitch_bytes < present.width as usize * 4 {
        return false;
    }
    let src = unsafe {
        core::slice::from_raw_parts(
            bo.virt.add(present.offset) as *const u8,
            present.pitch_bytes * present.height as usize,
        )
    };
    crate::intel::present_rgba_overlay_top_right(
        src,
        present.width,
        present.height,
        present.pitch_bytes,
    ) || crate::intel::present_rgba_primary_top_right(
        src,
        present.width,
        present.height,
        present.pitch_bytes,
    )
}

fn ppgtt_ranges(backing: &[ReplayBoBacking]) -> Vec<super::ppgtt::PpgttRange> {
    let mut ranges = Vec::with_capacity(backing.len());
    for bo in backing {
        ranges.push(super::ppgtt::PpgttRange {
            gpu: bo.gpu_va,
            phys: bo.phys,
            bytes: bo.size,
        });
    }
    ranges
}
