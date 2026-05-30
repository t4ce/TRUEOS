extern crate alloc;

use alloc::vec::Vec;

const MI_BATCH_BUFFER_START_MASK: u32 = 0xFF80_0000;
const MI_BATCH_BUFFER_START_PREFIX: u32 = 0x1880_0000;
const REPLAY_SCAN_MAX_HITS: usize = 24;

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
    let Some(backing) = allocate_bos(bos) else {
        return replay_failure(submit.batch_gpu);
    };
    if !apply_patches(&backing, base_patches) || !apply_patches(&backing, submit.patches) {
        return replay_failure(submit.batch_gpu);
    }
    log_replay_artifact_scan(&backing, submit.batch_gpu);
    let ranges = ppgtt_ranges(&backing);
    let proof = super::gpgpu::submit_render_replay_probe(submit.batch_gpu, &ranges);
    log_probe_address("batch-start", submit.batch_gpu, &backing);
    log_probe_address("acthd", proof.acthd as u64, &backing);
    let bbaddr = ((proof.bbaddr_hi as u64) << 32) | proof.bbaddr_lo as u64;
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
    crate::log!(
        "intel/replay: bbs-scan done hits={} note=pre-submit-patched-artifact\n",
        hits,
    );
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
