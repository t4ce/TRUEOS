// Picasso carrier ownership.
//
// Render0 is a boot-lifetime kernel/Helio lane.  It is deliberately not a
// fallback for a VMX Picasso device.  This file owns the identity boundary for
// independently scheduled Picasso renderers (Render1 and Render2); the mutable
// rendering storage that follows is keyed by an indexed lease rather than by
// the Render0 singleton.

use crate::gpu::physical::PhysicalGpuDevice;
use crate::gpu::physical::PhysicalGpuVmHandle;
use alloc::vec::Vec as AllocVec;

// Render1's dynamic arena intentionally begins after every fixed retained
// mapping.  It must never grow out of the old 0x2000_0000 lane: that lane is
// occupied by the shared persistent-resource allocator and the warm vertex
// mapping sits immediately above it.  The upper end is the first UI4 alias.
const PICASSO_RENDERER_VA_BASE: u64 = 0x3100_0000;
const PICASSO_RENDERER_VA_LIMIT: u64 = 0x6000_0000;
const PICASSO_UI4_ALIAS_VA_BASE: u64 = 0x6000_0000;
const PICASSO_UI4_ALIAS_VA_LIMIT: u64 = 0x7000_0000;
const PICASSO_SCENE_STATE_GPU: u64 = GPU_VA_RESIDENT_SCENE_STATE_BASE;
const PICASSO_SCENE_DEPTH_GPU: u64 = GPU_VA_RESIDENT_SCENE_DEPTH_BASE;
// Keep this in sync with the authenticated Helio transform ABI.  The whole
// artifact lane ends before 0x0DC0_0000; Render1's dynamic arena is above it.
const PICASSO_HELIO_TRANSFORM_ARTIFACT_BASE: u64 = 0x0DB0_0000;
const PICASSO_HELIO_TRANSFORM_ARTIFACT_LIMIT: u64 = 0x0DC0_0000;

const _: () = {
    assert!(GPU_VA_RING_BASE + WARM_RING_BYTES as u64 <= PICASSO_RENDERER_VA_BASE);
    assert!(GPU_VA_CONTEXT_BASE + WARM_CONTEXT_BYTES as u64 <= PICASSO_RENDERER_VA_BASE);
    assert!(GPU_VA_BATCH_BASE + WARM_BATCH_BYTES as u64 <= PICASSO_RENDERER_VA_BASE);
    assert!(GPU_VA_RESULT_BASE + WARM_RESULT_BYTES as u64 <= PICASSO_RENDERER_VA_BASE);
    assert!(GPU_VA_DRAW_STATE_BASE + WARM_DRAW_STATE_BYTES as u64 <= PICASSO_RENDERER_VA_BASE);
    assert!(GPU_VA_STREAMOUT_BASE + WARM_STREAMOUT_BYTES as u64 <= PICASSO_RENDERER_VA_BASE);
    assert!(
        GPU_VA_RESIDENT_SCENE_DEPTH_BASE + RESIDENT_SCENE_DEPTH_BYTES as u64
            <= PICASSO_RENDERER_VA_BASE
    );
    assert!(GPU_VA_RESIDENT_SCENE_MSAA_COLOR_BASE + 0x0400_0000 <= PICASSO_RENDERER_VA_BASE);
    assert!(GPU_VA_RESIDENT_SCENE_MSAA_DEPTH_BASE + 0x0400_0000 <= PICASSO_RENDERER_VA_BASE);
    assert!(GPU_VA_PERSISTENT_RESOURCE_LIMIT <= PICASSO_RENDERER_VA_BASE);
    assert!(GPU_VA_VERTEX_BASE + WARM_VERTEX_BYTES as u64 <= PICASSO_RENDERER_VA_BASE);
    assert!(
        GPU_VA_RESIDENT_SCENE_STATE_BASE + RESIDENT_SCENE_STATE_BYTES as u64
            <= PICASSO_RENDERER_VA_BASE
    );
    assert!(PICASSO_HELIO_TRANSFORM_ARTIFACT_LIMIT <= PICASSO_RENDERER_VA_BASE);
    assert!(PICASSO_HELIO_TRANSFORM_ARTIFACT_BASE < PICASSO_HELIO_TRANSFORM_ARTIFACT_LIMIT);
    assert!(PICASSO_RENDERER_VA_LIMIT <= PICASSO_UI4_ALIAS_VA_BASE);
    assert!(PICASSO_UI4_ALIAS_VA_LIMIT <= GPU_VA_RESIDENT_UI4_FRAME_LIMIT);
};

// These are GGTT control addresses, not the private PPGTT addresses used by
// command packets. Render1 occupies the intentional gap before direct-RCS.
// Render2 begins at the end of HelioC's PPGTT resource ABI; only its three
// control allocations are GGTT mapped there, and the window ends before the
// persistent font coverage arena.
const PICASSO_RENDER1_GGTT_RING_BASE: u64 = 0x01A0_0000;
const PICASSO_RENDER1_GGTT_CONTEXT_BASE: u64 = 0x01A1_0000;
const PICASSO_RENDER1_GGTT_RESULT_BASE: u64 = 0x01A4_0000;
const PICASSO_RENDER2_GGTT_RING_BASE: u64 = 0x0980_0000;
const PICASSO_RENDER2_GGTT_CONTEXT_BASE: u64 = 0x0981_0000;
const PICASSO_RENDER2_GGTT_RESULT_BASE: u64 = 0x0984_0000;
const _: () =
    assert!(GPU_VA_BATCH_BASE + WARM_BATCH_BYTES as u64 <= PICASSO_RENDER1_GGTT_RING_BASE);
const _: () = assert!(
    PICASSO_RENDER1_GGTT_RING_BASE + WARM_RING_BYTES as u64 <= PICASSO_RENDER1_GGTT_CONTEXT_BASE
);
const _: () = assert!(
    PICASSO_RENDER1_GGTT_CONTEXT_BASE + WARM_CONTEXT_BYTES as u64
        <= PICASSO_RENDER1_GGTT_RESULT_BASE
);
const _: () = assert!(
    PICASSO_RENDER1_GGTT_RESULT_BASE + WARM_RESULT_BYTES as u64
        <= crate::intel::gpgpu::DIRECT_RCS_GPU_VA_RING_BASE
);
const _: () = assert!(PICASSO_RENDER2_GGTT_RING_BASE.is_multiple_of(4096));
const _: () = assert!(PICASSO_RENDER2_GGTT_CONTEXT_BASE.is_multiple_of(4096));
const _: () = assert!(PICASSO_RENDER2_GGTT_RESULT_BASE.is_multiple_of(4096));
const _: () = assert!(
    PICASSO_RENDER2_GGTT_RING_BASE + WARM_RING_BYTES as u64
        <= PICASSO_RENDER2_GGTT_CONTEXT_BASE
);
const _: () = assert!(
    PICASSO_RENDER2_GGTT_CONTEXT_BASE + WARM_CONTEXT_BYTES as u64
        <= PICASSO_RENDER2_GGTT_RESULT_BASE
);
const _: () = assert!(
    PICASSO_RENDER2_GGTT_RESULT_BASE + WARM_RESULT_BYTES as u64 <= 0x0A00_0000
);
const _: () = assert!(
    PICASSO_RENDER1_GGTT_RESULT_BASE + WARM_RESULT_BYTES as u64
        <= PICASSO_RENDER2_GGTT_RING_BASE
);

const PICASSO_CARRIER_COUNT: usize = 2;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PicassoCarrierId {
    Render1,
    Render2,
}

#[derive(Copy, Clone)]
struct PicassoControlGgtt {
    ring: u64,
    context: u64,
    result: u64,
}

impl PicassoCarrierId {
    const ALL: [Self; PICASSO_CARRIER_COUNT] = [Self::Render1, Self::Render2];

    const fn index(self) -> usize {
        match self {
            Self::Render1 => 0,
            Self::Render2 => 1,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Render1 => "Render1",
            Self::Render2 => "Render2",
        }
    }

    const fn control(self) -> PicassoControlGgtt {
        match self {
            Self::Render1 => PicassoControlGgtt {
                ring: PICASSO_RENDER1_GGTT_RING_BASE,
                context: PICASSO_RENDER1_GGTT_CONTEXT_BASE,
                result: PICASSO_RENDER1_GGTT_RESULT_BASE,
            },
            Self::Render2 => PicassoControlGgtt {
                ring: PICASSO_RENDER2_GGTT_RING_BASE,
                context: PICASSO_RENDER2_GGTT_CONTEXT_BASE,
                result: PICASSO_RENDER2_GGTT_RESULT_BASE,
            },
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct PicassoCarrierMapping {
    gpu: u64,
    phys: u64,
    bytes: usize,
    scanout: bool,
}

struct PicassoCarrierState {
    lease: PicassoCarrierLease,
    mappings: AllocVec<PicassoCarrierMapping>,
    next_renderer_va: u64,
    next_ui4_alias_va: u64,
    lrc_initialized: bool,
    published_tail: usize,
    frame_active: bool,
    mapping_in_flight: bool,
    quarantined: bool,
    scene_state: Option<PicassoCarrierAllocation>,
    scene_depth: Option<PicassoCarrierAllocation>,
}

#[derive(Copy, Clone)]
struct PicassoCarrierAllocation {
    gpu: u64,
    phys: u64,
    virt: *mut u8,
    bytes: usize,
}

struct PicassoCarrierMappingGuard {
    lease: PicassoCarrierLease,
}

impl Drop for PicassoCarrierMappingGuard {
    fn drop(&mut self) {
        if let Some(state) = picasso_carrier_slot(self.lease.carrier())
            .lock()
            .state
            .as_mut()
            .filter(|state| state.lease == self.lease)
        {
            state.mapping_in_flight = false;
        }
    }
}

unsafe impl Send for PicassoCarrierAllocation {}

/// Private scene allocations returned to the primary encoder.  Their GPU VAs
/// are fixed command ABI values, but their pages and mappings are owned by the
/// Render1 lease, never by Render0's PPGTT caches.
#[derive(Copy, Clone)]
pub(crate) struct PicassoCarrierSceneStorage {
    pub(crate) state_phys: u64,
    pub(crate) state_virt: *mut u8,
    pub(crate) depth_phys: u64,
    pub(crate) depth_virt: *mut u8,
    pub(crate) depth_bytes: usize,
}

unsafe impl Send for PicassoCarrierSceneStorage {}

/// The physical Render1 control backing is boot-lifetime, exactly like the
/// other GuC lanes.  Only the PPGTT mappings and VMX ownership edge below are
/// tenant/epoch scoped.
struct PicassoCarrierSlot {
    boot_ready: bool,
    warm: Option<RenderWarmState>,
    state: Option<PicassoCarrierState>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct PicassoCarrierLease {
    carrier: PicassoCarrierId,
    device_raw: u64,
    epoch: u64,
    gpuvm: PhysicalGpuVmHandle,
    root_phys: u64,
}

/// A carrier claim distinguishes the first owner of the slot from later
/// retained meshes of that same VMX device.  Only the former may install or
/// tear down the warm PPGTT mappings.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct PicassoCarrierClaim {
    lease: PicassoCarrierLease,
    newly_claimed: bool,
}

impl PicassoCarrierClaim {
    pub(crate) const fn lease(self) -> PicassoCarrierLease {
        self.lease
    }

    pub(crate) const fn newly_claimed(self) -> bool {
        self.newly_claimed
    }
}

impl PicassoCarrierLease {
    pub(crate) const fn carrier(self) -> PicassoCarrierId {
        self.carrier
    }

    pub(crate) const fn device_raw(self) -> u64 {
        self.device_raw
    }

    pub(crate) const fn epoch(self) -> u64 {
        self.epoch
    }

    pub(crate) const fn gpuvm(self) -> PhysicalGpuVmHandle {
        self.gpuvm
    }

    pub(crate) const fn root_phys(self) -> u64 {
        self.root_phys
    }

    const fn matches(self, device_raw: u64, epoch: u64) -> bool {
        self.device_raw == device_raw && self.epoch == epoch
    }
}

/// This lock protects only the software ownership token.  Mapping, allocation,
/// LRC initialization and GuC calls happen after the broker lease has been
/// staged and therefore never under `gpu::vgpu::BROKER`.
const fn empty_picasso_carrier_slot() -> PicassoCarrierSlot {
    PicassoCarrierSlot {
        boot_ready: false,
        warm: None,
        state: None,
    }
}

static PICASSO_CARRIER_SLOTS: [Mutex<PicassoCarrierSlot>; PICASSO_CARRIER_COUNT] = [
    Mutex::new(empty_picasso_carrier_slot()),
    Mutex::new(empty_picasso_carrier_slot()),
];
static PICASSO_CARRIER_GGTT_PREWARM: spin::Once<bool> = spin::Once::new();
static PICASSO_CARRIER_CLAIM_LOCK: Mutex<()> = Mutex::new(());

fn picasso_carrier_slot(carrier: PicassoCarrierId) -> &'static Mutex<PicassoCarrierSlot> {
    &PICASSO_CARRIER_SLOTS[carrier.index()]
}

fn warm_backing_is_distinct(a: RenderWarmState, b: RenderWarmState) -> bool {
    let a_ranges = [
        (a.ring_phys, a.ring_len),
        (a.context_phys, a.context_len),
        (a.batch_phys, a.batch_len),
        (a.draw_state_phys, a.draw_state_len),
        (a.vertex_phys, a.vertex_len),
        (a.result_phys, a.result_len),
        (a.streamout_phys, a.streamout_len),
    ];
    let b_ranges = [
        (b.ring_phys, b.ring_len),
        (b.context_phys, b.context_len),
        (b.batch_phys, b.batch_len),
        (b.draw_state_phys, b.draw_state_len),
        (b.vertex_phys, b.vertex_len),
        (b.result_phys, b.result_len),
        (b.streamout_phys, b.streamout_len),
    ];
    a_ranges.iter().all(|(a_phys, a_bytes)| {
        let a_end = a_phys.saturating_add(*a_bytes as u64);
        b_ranges.iter().all(|(b_phys, b_bytes)| {
            let b_end = b_phys.saturating_add(*b_bytes as u64);
            a_end <= *b_phys || b_end <= *a_phys
        })
    })
}

/// Prewarm both process-global carrier control windows exactly once while GT
/// bring-up owns GGTT mutation. The per-VM path later maps only its carrier's
/// data backing into its owned PPGTT; it never maps or repairs GGTT at runtime.
pub(crate) fn prewarm_picasso_carrier_control_ggtt_for_boot(dev: crate::intel::Dev) -> bool {
    *PICASSO_CARRIER_GGTT_PREWARM.call_once(|| {
        if !crate::intel::physical_gt_ready(dev) {
            return false;
        }
        let Some(render1_warm) = allocate_picasso_warm_state_for_boot(dev) else {
            crate::log_error!(target: "render";
                "picasso-carrier prewarm accepted=0 carrier=Render1 reason=control-backing-allocation-failed runtime-ggtt-repair=forbidden\n"
            );
            return false;
        };
        let Some(render2_warm) = allocate_picasso_warm_state_for_boot(dev) else {
            crate::log_error!(target: "render";
                "picasso-carrier prewarm accepted=0 carrier=Render2 reason=control-backing-allocation-failed runtime-ggtt-repair=forbidden\n"
            );
            return false;
        };
        if !warm_backing_is_distinct(render1_warm, render2_warm) {
            crate::log_error!(target: "render";
                "picasso-carrier prewarm accepted=0 carriers=Render1,Render2 reason=physical-backing-alias runtime-ggtt-repair=forbidden\n"
            );
            return false;
        }
        let warms = [render1_warm, render2_warm];
        for carrier in PicassoCarrierId::ALL {
            let warm = warms[carrier.index()];
            let control = carrier.control();
            let mapped = crate::intel::map_ggtt(dev, warm.ring_phys, warm.ring_len, control.ring)
                && crate::intel::map_ggtt(
                    dev,
                    warm.context_phys,
                    warm.context_len,
                    control.context,
                )
                && crate::intel::map_ggtt(
                    dev,
                    warm.result_phys,
                    warm.result_len,
                    control.result,
                );
            if !mapped {
                crate::log_error!(target: "render";
                    "picasso-carrier prewarm accepted=0 carrier={} reason=ggtt-control-map-failed runtime-ggtt-repair=forbidden\n",
                    carrier.label(),
                );
                return false;
            }
        }
        crate::intel::ggtt_invalidate(dev);
        for carrier in PicassoCarrierId::ALL {
            let control = carrier.control();
            let mut slot = picasso_carrier_slot(carrier).lock();
            slot.warm = Some(warms[carrier.index()]);
            slot.boot_ready = true;
            crate::log_info!(target: "render";
                "picasso-carrier prewarm accepted=1 carrier={} ggtt_ring=0x{:X} ggtt_hwlrca=0x{:X} ggtt_result=0x{:X} ownership=boot-lifetime runtime_ggtt_remap=forbidden\n",
                carrier.label(), control.ring, control.context, control.result,
            );
        }
        true
    })
}

pub(crate) fn picasso_carrier_control_ggtt_ready() -> bool {
    PICASSO_CARRIER_GGTT_PREWARM.get().copied() == Some(true)
        && PicassoCarrierId::ALL
            .iter()
            .all(|carrier| picasso_carrier_slot(*carrier).lock().boot_ready)
}

pub(crate) fn picasso_render1_warm_state(lease: PicassoCarrierLease) -> Option<RenderWarmState> {
    let slot = picasso_carrier_slot(lease.carrier()).lock();
    slot.state
        .as_ref()
        .filter(|state| state.lease == lease)
        .and(slot.warm)
}

/// Bind Render1's immutable control/data backing into the selected VMX
/// device's owned PPGTT.  Ring and HWLRCA stay GGTT-only; every address used
/// by the retained scene command stream is mapped through this device root.
pub(crate) fn bind_picasso_render1_warm_ppgtt(
    lease: PicassoCarrierLease,
    physical: &'static dyn PhysicalGpuDevice,
) -> bool {
    if physical.gpuvm_root_phys(lease.gpuvm()).ok() != Some(lease.root_phys()) {
        return false;
    }
    let Some(warm) = picasso_render1_warm_state(lease) else {
        return false;
    };
    let mappings = [
        (GPU_VA_BATCH_BASE, warm.batch_phys, warm.batch_len),
        (GPU_VA_DRAW_STATE_BASE, warm.draw_state_phys, warm.draw_state_len),
        (GPU_VA_VERTEX_BASE, warm.vertex_phys, warm.vertex_len),
        (GPU_VA_RESULT_BASE, warm.result_phys, warm.result_len),
        (GPU_VA_STREAMOUT_BASE, warm.streamout_phys, warm.streamout_len),
    ];
    let mut installed = 0usize;
    for (gpu, phys, bytes) in mappings {
        if !map_picasso_render1_resource_range(lease, physical, gpu, phys, bytes) {
            for (old_gpu, _, old_bytes) in mappings[..installed].iter().rev() {
                let _ = unmap_picasso_render1_range(lease, physical, *old_gpu, *old_bytes);
            }
            return false;
        }
        installed += 1;
    }
    crate::log_info!(target: "render";
        "picasso-carrier ppgtt-bind carrier={} device=0x{:X} epoch={} gpuvm={} root=0x{:X} mappings={}\n",
        lease.carrier().label(), lease.device_raw(), lease.epoch(), lease.gpuvm().raw(),
        lease.root_phys(), installed,
    );
    true
}

const fn picasso_carrier_hwlrca_ggtt(lease: PicassoCarrierLease) -> u64 {
    lease.carrier().control().context
}

/// Result writes in retained batches use MI_STORE_DATA_IMM with the GGTT bit
/// set. They therefore must target this carrier's boot-mapped result page, not
/// the numerically identical Render0 control window.
pub(crate) const fn picasso_render1_result_ggtt(lease: PicassoCarrierLease) -> u64 {
    lease.carrier().control().result
}

/// Initialize the carrier's HWLRCA against the immutable VMX PPGTT root and
/// construct the only descriptor that may be registered for this carrier.
/// The HWLRCA is a distinct boot-mapped GGTT address while the ring/batch
/// addresses encoded in its image remain private PPGTT translations.
pub(crate) fn prepare_picasso_render1_context(
    lease: PicassoCarrierLease,
) -> Option<crate::gpu::physical::PhysicalContextDescriptor> {
    let control = lease.carrier().control();
    let mut slot = picasso_carrier_slot(lease.carrier()).lock();
    let warm = slot.warm?;
    let state = slot.state.as_mut().filter(|state| state.lease == lease)?;
    if !state.lrc_initialized {
        let ring_ctl = ring_ctl_value(warm.ring_len)?;
        if !init_gen12_lrc_context_image(
            warm,
            control.ring as u32,
            state.published_tail as u32,
            ring_ctl,
            lease.root_phys(),
        ) {
            return None;
        }
        state.lrc_initialized = true;
    }
    let (hwlrca_lo, hwlrca_hi) =
        build_guc_context_descriptor(picasso_carrier_hwlrca_ggtt(lease));
    Some(crate::gpu::physical::PhysicalContextDescriptor {
        engine: crate::gpu::physical::PhysicalEngineId::RCS0,
        hwlrca_lo,
        hwlrca_hi,
        gpuvm_root_phys: lease.root_phys(),
    })
}

pub(crate) fn picasso_carrier_descriptor_matches(
    lease: PicassoCarrierLease,
    descriptor: crate::gpu::physical::PhysicalContextDescriptor,
) -> bool {
    let (hwlrca_lo, hwlrca_hi) =
        build_guc_context_descriptor(picasso_carrier_hwlrca_ggtt(lease));
    descriptor.engine == crate::gpu::physical::PhysicalEngineId::RCS0
        && descriptor.gpuvm_root_phys == lease.root_phys()
        && descriptor.hwlrca_lo == hwlrca_lo
        && descriptor.hwlrca_hi == hwlrca_hi
}

/// Acquire the carrier-local CPU encoder gate.  This is intentionally a
/// different bit from `PRIMARY_PROBE_IN_FLIGHT`: a Picasso frame must never
/// be serialized by Render0's primary path merely because both eventually
/// schedule on physical RCS0.
pub(crate) fn try_begin_picasso_render1_frame(lease: PicassoCarrierLease) -> bool {
    // This is the same lock used by map/reserve/unmap.  A separate CAS left
    // a gap in which a mapper could observe `frame_active == false` after
    // admission had begun but before the bit was published.
    let mut slot = picasso_carrier_slot(lease.carrier()).lock();
    let Some(state) = slot.state.as_mut().filter(|state| state.lease == lease) else {
        return false;
    };
    if state.quarantined || state.frame_active || state.mapping_in_flight {
        return false;
    }
    // All target/depth/state mappings must have been installed before this
    // transition.  map/unmap reject while this bit is set.
    state.frame_active = true;
    true
}

fn begin_picasso_render1_mapping(lease: PicassoCarrierLease) -> Option<PicassoCarrierMappingGuard> {
    let mut slot = picasso_carrier_slot(lease.carrier()).lock();
    let state = slot.state.as_mut().filter(|state| state.lease == lease)?;
    if state.quarantined || state.frame_active || state.mapping_in_flight {
        return None;
    }
    state.mapping_in_flight = true;
    Some(PicassoCarrierMappingGuard { lease })
}

pub(crate) fn finish_picasso_render1_frame(lease: PicassoCarrierLease) {
    if let Some(state) = picasso_carrier_slot(lease.carrier())
        .lock()
        .state
        .as_mut()
        .filter(|state| state.lease == lease)
    {
        state.frame_active = false;
    }
}

fn allocate_picasso_scene_range(
    lease: PicassoCarrierLease,
    physical: &'static dyn PhysicalGpuDevice,
    gpu: u64,
    bytes: usize,
) -> Option<PicassoCarrierAllocation> {
    let bytes = aligned_carrier_bytes(bytes)?;
    // This backing is never placed in GGTT: its only device mapping is the
    // tenant's PPGTT leaf below.  Do not impose the generic below-4GiB DMA
    // ceiling here; fragmented low memory otherwise makes the first retained
    // frame fail before it can reach carrier admission.
    let Some((phys, virt)) = crate::dma::alloc_ppgtt(bytes, crate::intel::WARM_ALIGN) else {
        let pmm = crate::phys::pmm_stats();
        crate::log_error!(target: "render";
            "picasso-carrier reject carrier={} device=0x{:X} epoch={} stage=scene-ppgtt-alloc bytes=0x{:X} pmm_free_bytes={} pmm_largest_free_region={} pmm_free_regions={}\n",
            lease.carrier().label(), lease.device_raw(), lease.epoch(), bytes,
            pmm.map_or(0, |stats| stats.free_bytes),
            pmm.map_or(0, |stats| stats.largest_free_region),
            pmm.map_or(0, |stats| stats.free_regions),
        );
        return None;
    };
    unsafe { core::ptr::write_bytes(virt, 0, bytes) };
    crate::intel::dma_flush(virt, bytes);
    if !map_picasso_render1_resource_range(lease, physical, gpu, phys, bytes) {
        crate::log_error!(target: "render";
            "picasso-carrier reject carrier={} device=0x{:X} epoch={} stage=scene-ppgtt-map bytes=0x{:X}\n",
            lease.carrier().label(), lease.device_raw(), lease.epoch(), bytes,
        );
        crate::dma::dealloc(virt, bytes);
        return None;
    }
    Some(PicassoCarrierAllocation {
        gpu,
        phys,
        virt,
        bytes,
    })
}

/// Lazily install the carrier-local state/depth caches.  The command stream
/// uses stable ABI VAs, but every leaf is in the VMX owner's GPUVM and the CPU
/// backing is retained only by the Render1 lease.
pub(crate) fn prepare_picasso_render1_scene_storage(
    lease: PicassoCarrierLease,
    physical: &'static dyn PhysicalGpuDevice,
) -> Option<PicassoCarrierSceneStorage> {
    let existing = {
        let slot = picasso_carrier_slot(lease.carrier()).lock();
        slot.state.as_ref().and_then(|state| {
            (state.lease == lease && !state.quarantined)
                .then(|| state.scene_state.zip(state.scene_depth))
        })
    };
    if let Some((state, depth)) = existing.flatten() {
        return Some(PicassoCarrierSceneStorage {
            state_phys: state.phys,
            state_virt: state.virt,
            depth_phys: depth.phys,
            depth_virt: depth.virt,
            depth_bytes: depth.bytes,
        });
    }
    // State and depth are a single pre-admission mapping transaction.  The
    // guard keeps frame admission out until both fixed leaves are present (or
    // the partial leaf has been rolled back).
    let _mapping = begin_picasso_render1_mapping(lease)?;
    let existing = {
        let slot = picasso_carrier_slot(lease.carrier()).lock();
        slot.state.as_ref().and_then(|state| {
            (state.lease == lease && !state.quarantined)
                .then(|| state.scene_state.zip(state.scene_depth))
        })
    };
    let (state, depth) = match existing.flatten() {
        Some(existing) => existing,
        None => {
            let state = allocate_picasso_scene_range(
                lease,
                physical,
                PICASSO_SCENE_STATE_GPU,
                RESIDENT_SCENE_STATE_BYTES,
            )?;
            let depth = match allocate_picasso_scene_range(
                lease,
                physical,
                PICASSO_SCENE_DEPTH_GPU,
                RESIDENT_SCENE_DEPTH_BYTES,
            ) {
                Some(depth) => depth,
                None => {
                    let _ = unmap_picasso_render1_range_inner(
                        lease,
                        physical,
                        state.gpu,
                        state.bytes,
                        true,
                    );
                    crate::dma::dealloc(state.virt, state.bytes);
                    return None;
                }
            };
            let mut slot = picasso_carrier_slot(lease.carrier()).lock();
            let owner = slot.state.as_mut().filter(|owner| owner.lease == lease)?;
            if owner.quarantined || owner.scene_state.is_some() || owner.scene_depth.is_some() {
                drop(slot);
                let _ = unmap_picasso_render1_range_inner(
                    lease,
                    physical,
                    depth.gpu,
                    depth.bytes,
                    true,
                );
                let _ = unmap_picasso_render1_range_inner(
                    lease,
                    physical,
                    state.gpu,
                    state.bytes,
                    true,
                );
                crate::dma::dealloc(depth.virt, depth.bytes);
                crate::dma::dealloc(state.virt, state.bytes);
                return None;
            }
            owner.scene_state = Some(state);
            owner.scene_depth = Some(depth);
            (state, depth)
        }
    };
    Some(PicassoCarrierSceneStorage {
        state_phys: state.phys,
        state_virt: state.virt,
        depth_phys: depth.phys,
        depth_virt: depth.virt,
        depth_bytes: depth.bytes,
    })
}

fn quarantine_picasso_render1(lease: PicassoCarrierLease, reason: &'static str) {
    {
        let mut slot = picasso_carrier_slot(lease.carrier()).lock();
        let Some(state) = slot.state.as_mut().filter(|state| state.lease == lease) else {
            return;
        };
        state.quarantined = true;
    }
    crate::log_error!(target: "render";
        "picasso-carrier quarantine carrier={} device=0x{:X} epoch={} reason={} action=retain-context-and-mappings\n",
        lease.carrier().label(), lease.device_raw(), lease.epoch(), reason,
    );
    crate::gpu::vgpu::quarantine_picasso_carrier(lease);
}

/// Publish one already-flushed primary batch through the carrier's own HWLRCA and
/// physical mediated GuC context.  Success requires both the release cookie
/// and the exact context-saved HEAD equality; anything ambiguous quarantines
/// the carrier rather than allowing a tail rollback or reuse.
pub(crate) fn submit_picasso_render1_batch(
    lease: PicassoCarrierLease,
    batch_gpu: u64,
    expected_result: u32,
    expected_result_slot_dword: usize,
) -> Result<(), &'static str> {
    let _physical = crate::gpu::physical::physical_device().ok_or("picasso-physical-gpu")?;
    let warm = picasso_render1_warm_state(lease).ok_or("picasso-render1-warm")?;
    let (old_tail, initialized, quarantined) = {
        let slot = picasso_carrier_slot(lease.carrier()).lock();
        let state = slot
            .state
            .as_ref()
            .filter(|state| state.lease == lease)
            .ok_or("picasso-render1-lease")?;
        (state.published_tail, state.lrc_initialized, state.quarantined)
    };
    if quarantined {
        return Err("picasso-render1-quarantined");
    }
    if initialized {
        let mask = warm.ring_len.saturating_sub(1) as u32;
        let started = crate::chronos::monotonic_nanos();
        let mut spins = 0u64;
        loop {
            if crate::intel::render::read_gen12_lrc_ring_head(warm) & mask == old_tail as u32 {
                break;
            }
            if spins >= 5_000_000
                || (spins.is_multiple_of(256)
                    && crate::chronos::monotonic_nanos().saturating_sub(started) >= 2_000_000_000)
            {
                quarantine_picasso_render1(lease, "saved-head-before-reuse");
                return Err("picasso-render1-saved-head");
            }
            spins = spins.saturating_add(1);
            core::hint::spin_loop();
        }
    }
    // The numeric batch VA intentionally matches Render0's established ABI,
    // but its translation is owned by this VMX GPUVM. Selecting PPGTT here
    // is the isolation boundary: a GGTT fetch would execute Render0's mutable
    // batch at the same address.
    let tail = append_ring_batch_start(warm, old_tail, batch_gpu, true)
        .ok_or("picasso-render1-ring")?;
    {
        let mut slot = picasso_carrier_slot(lease.carrier()).lock();
        let state = slot
            .state
            .as_mut()
            .filter(|state| state.lease == lease)
            .ok_or("picasso-render1-lease")?;
        state.published_tail = tail;
    }
    let descriptor = match prepare_picasso_render1_context(lease) {
        Some(descriptor) => descriptor,
        None => {
            quarantine_picasso_render1(lease, "lrc-prepare");
            return Err("picasso-render1-lrc");
        }
    };
    if initialized && !write_gen12_lrc_ring_tail(warm, tail as u32) {
        quarantine_picasso_render1(lease, "lrc-tail-publish");
        return Err("picasso-render1-lrc-tail");
    }
    let submission = match crate::gpu::vgpu::submit_picasso_carrier_context(lease, descriptor) {
        Ok(submission) => submission,
        Err(_) => {
            quarantine_picasso_render1(lease, "guc-register-or-submit-ambiguous");
            return Err("picasso-render1-submit");
        }
    };
    crate::log_trace!(target: "render";
        "picasso-carrier submit carrier={} device=0x{:X} epoch={} context={} serial={} old_tail={} published_tail={} batch=0x{:X} fetch=ppgtt\n",
        lease.carrier().label(), lease.device_raw(), lease.epoch(), submission.context.raw(), submission.serial, old_tail, tail, batch_gpu,
    );
    let started = crate::chronos::monotonic_nanos();
    let mut spins = 0u64;
    loop {
        let (lo, hi) = read_result_qword_coherent(warm, expected_result_slot_dword);
        let head = read_gen12_lrc_ring_head(warm) & (warm.ring_len.saturating_sub(1) as u32);
        if lo == expected_result
            && hi == RCS_EXEC_RESULT_SCENE_RCS_RELEASE_DONE_HI
            && head == tail as u32
        {
            crate::log_trace!(target: "render";
                "picasso-carrier retire carrier={} device=0x{:X} epoch={} context={} saved_head={} published_tail={} release=1 wait_us={} wait_iters={}\n",
                lease.carrier().label(), lease.device_raw(), lease.epoch(), submission.context.raw(), head, tail,
                crate::chronos::monotonic_nanos().saturating_sub(started) / 1_000, spins,
            );
            return Ok(());
        }
        if spins >= 5_000_000
            || (spins.is_multiple_of(256)
                && crate::chronos::monotonic_nanos().saturating_sub(started) >= 2_000_000_000)
        {
            let debug_bytes = RESULT_DEBUG_DWORD_COUNT
                .saturating_mul(core::mem::size_of::<u32>())
                .min(warm.result_len);
            crate::intel::dma_flush(warm.result_virt, debug_bytes);
            let (scene_lo, scene_hi) =
                read_result_qword_coherent(warm, RESULT_SLOT_SCENE_FRAME_DWORD);
            crate::log_important!(target: "render";
                "picasso-carrier-timeout-proof carrier={} accepted=0 saved_head={} published_tail={} stage_markers=[entry:0x{:08X},opening:0x{:08X},vf:0x{:08X},vs:0x{:08X},clip:0x{:08X},raster:0x{:08X},ps_state:0x{:08X},pre3d:0x{:08X},post3d:0x{:08X},final:0x{:08X}] secondary_return=0x{:08X} scene_release=0x{:08X}/0x{:08X} interpretation=highest-secondary-index-and-last-command-frontier\n",
                lease.carrier().label(),
                head,
                tail,
                read_result_dword(warm, RESULT_SLOT_BATCH_ENTRY_DWORD),
                read_result_dword(warm, RESULT_SLOT_POST_OPENING_DWORD),
                read_result_dword(warm, RESULT_SLOT_POST_VF_DWORD),
                read_result_dword(warm, RESULT_SLOT_POST_VS_DWORD),
                read_result_dword(warm, RESULT_SLOT_POST_CLIP_DWORD),
                read_result_dword(warm, RESULT_SLOT_POST_RASTER_DWORD),
                read_result_dword(warm, RESULT_SLOT_POST_PS_STATE_DWORD),
                read_result_dword(warm, RESULT_SLOT_PRE3D_DWORD),
                read_result_dword(warm, RESULT_SLOT_POST3D_DWORD),
                read_result_dword(warm, RESULT_SLOT_FINAL_DWORD),
                read_result_dword(warm, RESULT_SLOT_SECONDARY_RETURN_DWORD),
                scene_lo,
                scene_hi,
            );
            quarantine_picasso_render1(lease, "release-or-saved-head-timeout");
            return Err("picasso-render1-retire");
        }
        spins = spins.saturating_add(1);
        core::hint::spin_loop();
    }
}

/// Claim one Picasso carrier for a live VMX device.
///
/// `root_phys` is obtained by the broker from this device's already-owned
/// physical GPUVM.  Keeping it in the immutable lease means a replacement
/// handle generation cannot inherit a prior tenant's address-space identity.
pub(crate) fn claim_picasso_render1(
    device_raw: u64,
    epoch: u64,
    gpuvm: PhysicalGpuVmHandle,
    root_phys: u64,
) -> Result<PicassoCarrierClaim, &'static str> {
    if device_raw == 0 || epoch == 0 || root_phys == 0 {
        return Err("picasso-carrier-identity");
    }
    let _claims = PICASSO_CARRIER_CLAIM_LOCK.lock();
    for carrier in PicassoCarrierId::ALL {
        let slot = picasso_carrier_slot(carrier).lock();
        if let Some(existing) = slot
            .state
            .as_ref()
            .filter(|state| state.lease.matches(device_raw, epoch))
        {
            if existing.lease.gpuvm() != gpuvm || existing.lease.root_phys() != root_phys {
                return Err("picasso-carrier-identity");
            }
            return Ok(PicassoCarrierClaim {
                lease: existing.lease,
                newly_claimed: false,
            });
        }
    }
    for carrier in PicassoCarrierId::ALL {
        let mut slot = picasso_carrier_slot(carrier).lock();
        if !slot.boot_ready || slot.warm.is_none() {
            return Err("picasso-carrier-not-prewarmed");
        }
        if slot.state.is_some() {
            continue;
        }
        let lease = PicassoCarrierLease {
            carrier,
            device_raw,
            epoch,
            gpuvm,
            root_phys,
        };
        slot.state = Some(PicassoCarrierState {
            lease,
            mappings: AllocVec::new(),
            next_renderer_va: PICASSO_RENDERER_VA_BASE,
            next_ui4_alias_va: PICASSO_UI4_ALIAS_VA_BASE,
            lrc_initialized: false,
            published_tail: 0,
            frame_active: false,
            mapping_in_flight: false,
            quarantined: false,
            scene_state: None,
            scene_depth: None,
        });
        crate::log_info!(target: "render";
            "picasso-carrier claim carrier={} device=0x{:X} epoch={} gpuvm={} root=0x{:X} max_vmx_domains={}\n",
            carrier.label(), device_raw, epoch, gpuvm.raw(), root_phys, PICASSO_CARRIER_COUNT,
        );
        return Ok(PicassoCarrierClaim {
            lease,
            newly_claimed: true,
        });
    }
    Err("picasso-carrier-capacity")
}

/// Look up an already-claimed carrier without creating an ownership edge.
pub(crate) fn picasso_render1_for(device_raw: u64, epoch: u64) -> Option<PicassoCarrierLease> {
    PicassoCarrierId::ALL.iter().find_map(|carrier| {
        picasso_carrier_slot(*carrier)
            .lock()
            .state
            .as_ref()
            .map(|state| state.lease)
            .filter(|lease| lease.matches(device_raw, epoch))
    })
}

fn aligned_carrier_bytes(bytes: usize) -> Option<usize> {
    (bytes != 0)
        .then(|| crate::intel::align_up(bytes, 4096))
        .flatten()
}

/// Reserve a low renderer VA from Render1's private resource window.  VMX
/// client buffers start at 4GiB, so this cannot alias a guest-selected GPU VA.
pub(crate) fn reserve_picasso_render1_resource_va(
    lease: PicassoCarrierLease,
    bytes: usize,
) -> Option<u64> {
    let bytes = aligned_carrier_bytes(bytes)? as u64;
    let mut slot = picasso_carrier_slot(lease.carrier()).lock();
    let state = slot.state.as_mut().filter(|state| state.lease == lease)?;
    if state.frame_active || state.mapping_in_flight || state.quarantined {
        return None;
    }
    let start = (state.next_renderer_va + 4095) & !4095;
    let end = start.checked_add(bytes)?;
    if end > PICASSO_RENDERER_VA_LIMIT {
        return None;
    }
    state.next_renderer_va = end;
    Some(start)
}

/// Reserve one carrier-local direct UI4 alias.  The mapping itself must use
/// `map_picasso_render1_scanout_range`, which installs PAT3/UC.
pub(crate) fn reserve_picasso_render1_ui4_alias_va(
    lease: PicassoCarrierLease,
    bytes: usize,
) -> Option<u64> {
    let bytes = aligned_carrier_bytes(bytes)? as u64;
    let mut slot = picasso_carrier_slot(lease.carrier()).lock();
    let state = slot.state.as_mut().filter(|state| state.lease == lease)?;
    if state.frame_active || state.mapping_in_flight || state.quarantined {
        return None;
    }
    let start = (state.next_ui4_alias_va + 4095) & !4095;
    let end = start.checked_add(bytes)?;
    if end > PICASSO_UI4_ALIAS_VA_LIMIT {
        return None;
    }
    state.next_ui4_alias_va = end;
    Some(start)
}

fn map_picasso_render1_range_inner(
    lease: PicassoCarrierLease,
    physical: &'static dyn PhysicalGpuDevice,
    gpu: u64,
    phys: u64,
    bytes: usize,
    scanout: bool,
) -> bool {
    let Some(bytes) = aligned_carrier_bytes(bytes) else {
        return false;
    };
    let Some(end) = gpu.checked_add(bytes as u64) else {
        return false;
    };
    if !gpu.is_multiple_of(4096) || !phys.is_multiple_of(4096) || end > (1u64 << 32) {
        return false;
    }
    if physical.gpuvm_root_phys(lease.gpuvm()).ok() != Some(lease.root_phys()) {
        return false;
    }
    let mut slot = picasso_carrier_slot(lease.carrier()).lock();
    let Some(state) = slot.state.as_mut().filter(|state| state.lease == lease) else {
        return false;
    };
    if state.frame_active || state.quarantined {
        return false;
    }
    let mapping = PicassoCarrierMapping {
        gpu,
        phys,
        bytes,
        scanout,
    };
    if state.mappings.iter().any(|existing| *existing == mapping) {
        return true;
    }
    if state.mappings.iter().any(|existing| {
        let existing_end = existing.gpu.saturating_add(existing.bytes as u64);
        gpu < existing_end && existing.gpu < end
    }) {
        return false;
    }
    let mapped = if scanout {
        physical.map_gpuvm_scanout(lease.gpuvm(), gpu, phys, bytes)
    } else {
        physical.map_gpuvm(lease.gpuvm(), gpu, phys, bytes)
    };
    if mapped.is_err() {
        crate::log_error!(target: "render";
            "picasso-carrier reject carrier={} device=0x{:X} epoch={} stage=ppgtt-map mapping={} bytes=0x{:X}\n",
            lease.carrier().label(), lease.device_raw(), lease.epoch(), if scanout { "ui4-pat3-uc" } else { "resource" }, bytes,
        );
        return false;
    }
    state.mappings.push(mapping);
    true
}

pub(crate) fn map_picasso_render1_resource_range(
    lease: PicassoCarrierLease,
    physical: &'static dyn PhysicalGpuDevice,
    gpu: u64,
    phys: u64,
    bytes: usize,
) -> bool {
    map_picasso_render1_range_inner(lease, physical, gpu, phys, bytes, false)
}

pub(crate) fn map_picasso_render1_scanout_range(
    lease: PicassoCarrierLease,
    physical: &'static dyn PhysicalGpuDevice,
    gpu: u64,
    phys: u64,
    bytes: usize,
) -> bool {
    map_picasso_render1_range_inner(lease, physical, gpu, phys, bytes, true)
}

/// Return a stable Render1 PAT3/UC alias for one leased UI4 producer surface.
/// The alias belongs to the carrier, never to Render0, and remains mapped
/// until device teardown so a later frame cannot retarget a still-live leaf.
pub(crate) fn prepare_picasso_render1_ui4_target(
    lease: PicassoCarrierLease,
    physical: &'static dyn PhysicalGpuDevice,
    phys: u64,
    bytes: usize,
) -> Option<u64> {
    let bytes = aligned_carrier_bytes(bytes)?;
    if let Some(gpu) = picasso_carrier_slot(lease.carrier())
        .lock()
        .state
        .as_ref()
        .and_then(|state| {
            (state.lease == lease)
                .then(|| {
                    state
                        .mappings
                        .iter()
                        .find(|mapping| {
                            mapping.scanout && mapping.phys == phys && mapping.bytes == bytes
                        })
                        .map(|mapping| mapping.gpu)
                })
                .flatten()
        })
    {
        return Some(gpu);
    }
    let gpu = reserve_picasso_render1_ui4_alias_va(lease, bytes)?;
    map_picasso_render1_scanout_range(lease, physical, gpu, phys, bytes).then_some(gpu)
}

fn unmap_picasso_render1_range_inner(
    lease: PicassoCarrierLease,
    physical: &'static dyn PhysicalGpuDevice,
    gpu: u64,
    bytes: usize,
    mapping_guard_owner: bool,
) -> bool {
    let Some(bytes) = aligned_carrier_bytes(bytes) else {
        return false;
    };
    if physical.gpuvm_root_phys(lease.gpuvm()).ok() != Some(lease.root_phys()) {
        return false;
    }
    let mut slot = picasso_carrier_slot(lease.carrier()).lock();
    let Some(state) = slot.state.as_mut().filter(|state| state.lease == lease) else {
        return false;
    };
    if state.frame_active || state.quarantined || (state.mapping_in_flight && !mapping_guard_owner)
    {
        return false;
    }
    let Some(index) = state
        .mappings
        .iter()
        .position(|mapping| mapping.gpu == gpu && mapping.bytes == bytes)
    else {
        return false;
    };
    if physical.unmap_gpuvm(lease.gpuvm(), gpu, bytes).is_err() {
        // A physical unmap result is an execution-ownership boundary.  Keep
        // every carrier leaf pinned and make the VMX device fail closed; in
        // contrast, a frame-active refusal above is ordinary backpressure.
        state.quarantined = true;
        drop(slot);
        quarantine_picasso_render1(lease, "ppgtt-unmap");
        return false;
    }
    state.mappings.swap_remove(index);
    true
}

pub(crate) fn unmap_picasso_render1_range(
    lease: PicassoCarrierLease,
    physical: &'static dyn PhysicalGpuDevice,
    gpu: u64,
    bytes: usize,
) -> bool {
    unmap_picasso_render1_range_inner(lease, physical, gpu, bytes, false)
}

/// Return this carrier to the pool only after the owner has destroyed its
/// GuC context and unmapped every carrier-local resource.  A mismatched epoch
/// is intentionally a no-op: it denotes a stale close racing a newer device.
pub(crate) fn release_picasso_render1(lease: PicassoCarrierLease) -> bool {
    let mut slot = picasso_carrier_slot(lease.carrier()).lock();
    if slot.state.as_ref().is_some_and(|current| {
        current.lease == lease
            && !current.frame_active
            && !current.mapping_in_flight
            && !current.quarantined
            && current.mappings.is_empty()
    }) {
        slot.state = None;
        true
    } else {
        false
    }
}

/// Final carrier teardown, called only after the broker has destroyed every
/// tracked GuC context and released every resident mesh.  This drains warm,
/// scene-cache and direct-target leaves before the VMX GPUVM itself is
/// destroyed; no `BROKER` lock is held while renderer mappings mutate.
pub(crate) fn teardown_picasso_render1(
    lease: PicassoCarrierLease,
    physical: &'static dyn PhysicalGpuDevice,
) -> bool {
    if picasso_carrier_slot(lease.carrier())
        .lock()
        .state
        .as_ref()
        .is_none_or(|state| {
            state.lease != lease
                || state.frame_active
                || state.mapping_in_flight
                || state.quarantined
        })
    {
        return false;
    }
    loop {
        let mapping = {
            let slot = picasso_carrier_slot(lease.carrier()).lock();
            slot.state
                .as_ref()
                .filter(|state| state.lease == lease)
                .and_then(|state| state.mappings.last().copied())
        };
        let Some(mapping) = mapping else {
            break;
        };
        if physical
            .unmap_gpuvm(lease.gpuvm(), mapping.gpu, mapping.bytes)
            .is_err()
        {
            quarantine_picasso_render1(lease, "teardown-unmap");
            return false;
        }
        let mut slot = picasso_carrier_slot(lease.carrier()).lock();
        let Some(state) = slot.state.as_mut().filter(|state| state.lease == lease) else {
            return false;
        };
        let Some(index) = state.mappings.iter().rposition(|entry| *entry == mapping) else {
            state.quarantined = true;
            drop(slot);
            quarantine_picasso_render1(lease, "teardown-mapping-race");
            return false;
        };
        state.mappings.swap_remove(index);
    }
    let (scene_state, scene_depth) = {
        let mut slot = picasso_carrier_slot(lease.carrier()).lock();
        let Some(state) = slot.state.as_mut().filter(|state| state.lease == lease) else {
            return false;
        };
        (state.scene_state.take(), state.scene_depth.take())
    };
    for allocation in [scene_state, scene_depth].into_iter().flatten() {
        crate::dma::dealloc(allocation.virt, allocation.bytes);
    }
    let mut slot = picasso_carrier_slot(lease.carrier()).lock();
    let Some(state) = slot.state.as_ref() else {
        return false;
    };
    if state.lease != lease || !state.mappings.is_empty() {
        return false;
    }
    slot.state = None;
    true
}

/// Roll back the first claim before it has crossed the GuC boundary.  An
/// existing device carrier is never passed here, and a submitted/quarantined
/// carrier is deliberately retained until reboot instead of being guessed at.
pub(crate) fn teardown_unsubmitted_picasso_render1(
    lease: PicassoCarrierLease,
    physical: &'static dyn PhysicalGpuDevice,
) -> bool {
    let safe = picasso_carrier_slot(lease.carrier())
        .lock()
        .state
        .as_ref()
        .is_some_and(|state| {
            state.lease == lease
                && !state.frame_active
                && !state.mapping_in_flight
                && !state.lrc_initialized
                && state.published_tail == 0
                && !state.quarantined
        });
    safe && teardown_picasso_render1(lease, physical)
}

#[cfg(test)]
mod picasso_carrier_tests {
    use super::*;

    #[test]
    fn lease_match_requires_the_same_device_generation() {
        let lease = PicassoCarrierLease {
            carrier: PicassoCarrierId::Render1,
            device_raw: 0x1000_0001,
            epoch: 7,
            gpuvm: PhysicalGpuVmHandle::from_raw(3),
            root_phys: 0xCAFE_0000,
        };
        assert!(lease.matches(0x1000_0001, 7));
        assert!(!lease.matches(0x1000_0001, 8));
        assert!(!lease.matches(0x2000_0001, 7));
    }

    #[test]
    fn render1_lease_uses_the_owned_gpuvm_root() {
        let lease = PicassoCarrierLease {
            carrier: PicassoCarrierId::Render1,
            device_raw: 1,
            epoch: 1,
            gpuvm: PhysicalGpuVmHandle::from_raw(9),
            root_phys: 0xBEEF_0000,
        };
        assert_eq!(lease.gpuvm().raw(), 9);
        assert_eq!(lease.root_phys(), 0xBEEF_0000);
    }

    #[test]
    fn carrier_identity_and_control_windows_are_distinct() {
        let render1 = PicassoCarrierLease {
            carrier: PicassoCarrierId::Render1,
            device_raw: 1,
            epoch: 1,
            gpuvm: PhysicalGpuVmHandle::from_raw(9),
            root_phys: 0xBEEF_0000,
        };
        let render2 = PicassoCarrierLease {
            carrier: PicassoCarrierId::Render2,
            ..render1
        };
        assert_ne!(render1, render2);
        assert_eq!(render1.carrier().label(), "Render1");
        assert_eq!(render2.carrier().label(), "Render2");
        let render1_control = render1.carrier().control();
        let render2_control = render2.carrier().control();
        assert_ne!(render1_control.ring, render2_control.ring);
        assert_ne!(render1_control.context, render2_control.context);
        assert_ne!(render1_control.result, render2_control.result);
        assert!(
            render1_control.result + WARM_RESULT_BYTES as u64 <= render2_control.ring
        );
    }

    #[test]
    fn renderer_and_guest_virtual_ranges_are_disjoint() {
        assert!(PICASSO_RENDERER_VA_LIMIT <= 0x1_0000_0000);
        assert!(PICASSO_UI4_ALIAS_VA_LIMIT <= 0x1_0000_0000);
        assert!(PICASSO_RENDERER_VA_LIMIT <= PICASSO_UI4_ALIAS_VA_BASE);
        assert!(GPU_VA_PERSISTENT_RESOURCE_LIMIT <= PICASSO_RENDERER_VA_BASE);
        assert!(GPU_VA_VERTEX_BASE + WARM_VERTEX_BYTES as u64 <= PICASSO_RENDERER_VA_BASE);
        assert!(
            GPU_VA_RESIDENT_SCENE_STATE_BASE + RESIDENT_SCENE_STATE_BYTES as u64
                <= PICASSO_RENDERER_VA_BASE
        );
        assert!(PICASSO_HELIO_TRANSFORM_ARTIFACT_LIMIT <= PICASSO_RENDERER_VA_BASE);
        assert!(PICASSO_HELIO_TRANSFORM_ARTIFACT_BASE < PICASSO_HELIO_TRANSFORM_ARTIFACT_LIMIT);
    }
}
