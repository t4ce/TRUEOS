extern crate alloc;

use alloc::vec::Vec;

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
    let Some(backing) = allocate_bos(bos) else {
        return replay_failure(submit.batch_gpu);
    };
    if !apply_patches(&backing, base_patches) || !apply_patches(&backing, submit.patches) {
        return replay_failure(submit.batch_gpu);
    }
    let ranges = ppgtt_ranges(&backing);
    let proof = super::gpgpu::submit_render_replay_probe(submit.batch_gpu, &ranges);
    crate::log!(
        "intel/replay: frame seq={} batch_gpu=0x{:X} batch_start=0x{:X} flags=0x{:X} bo_count={} submitted={} retired={}\n",
        submit.seq,
        submit.batch_gpu,
        submit.batch_start,
        submit.flags,
        backing.len(),
        proof.submitted as u8,
        proof.retired as u8,
    );
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
        ipehr: 0,
        eir: 0,
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
