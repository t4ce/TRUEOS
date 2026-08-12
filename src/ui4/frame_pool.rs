//! UI4-owned frame storage and producer/publisher hand-off.
//!
//! A logical frame owns exactly the explicitly requested number of physical
//! buffers. Cadence and buffering are independent contract dimensions. The
//! trusted `ui_surface` allocator remains an implementation detail; producers
//! receive a checked write lease and never a display-plane address.

use alloc::vec::Vec;
use spin::Mutex;

use crate::graphics::primitives::{Error as SurfaceError, UiSurfaceFormat};
use crate::r::ui_surface::{self, UiSurfaceHandle};

use super::{
    FrameContent, FrameHandle, FramePlan, FramePlanError, FrameSpec, PremultipliedRgba8,
    ScanoutFormat,
};

const MAX_FRAMES: usize = 64;
const FRAME_BUFFER_CAPACITY: usize = super::FrameBuffering::Quad.count();

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FramePoolError {
    InvalidPlan(FramePlanError),
    InvalidHandle,
    UnsupportedFormat,
    OutOfMemory,
    Busy,
    ImmutablePublished,
    NotPublished,
    InvalidLease,
    ProducerReleaseRequired,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct FrameWriteLease {
    pub(crate) frame: FrameHandle,
    pub(crate) buffer_index: u8,
    token: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct FrameReadLease {
    pub(crate) frame: FrameHandle,
    pub(crate) buffer_index: u8,
}

/// Trusted producer-release proof carried with one published GPU-authored
/// allocation. Each variant is minted by the engine that performed the final
/// write, and both bind the proof to the exact physical surface.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FrameGpuRelease {
    ResidentScene(crate::intel::render::ResidentSceneReleaseFence),
    Compute(crate::intel::gpgpu::GpgpuRgba8ReleaseFence),
}

impl FrameGpuRelease {
    pub(crate) const fn matches(self, phys: u64, byte_len: usize) -> bool {
        match self {
            Self::ResidentScene(release) => release.matches(phys, byte_len),
            Self::Compute(release) => release.matches(phys, byte_len),
        }
    }

    pub(crate) const fn sequence(self) -> u64 {
        match self {
            Self::ResidentScene(release) => release.sequence(),
            Self::Compute(release) => release.sequence(),
        }
    }

    pub(crate) const fn producer_label(self) -> &'static str {
        match self {
            Self::ResidentScene(_) => "resident-scene",
            Self::Compute(_) => "gpgpu-compute",
        }
    }
}

#[derive(Copy, Clone)]
pub(crate) struct FrameRgbaView {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) virt: *mut u8,
    pub(crate) byte_len: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch: u32,
    /// This exact buffer has been exposed as a GPU render/compute target since
    /// it was acquired. Direct scanout additionally requires `gpu_release`.
    pub(crate) gpu_authored: bool,
    /// Retired producer release for this exact allocation. Released GPU
    /// frames may direct-scan only through their producer-specific contract.
    pub(crate) gpu_release: Option<FrameGpuRelease>,
    /// The producer cleared every pixel with alpha 255 before any subsequent
    /// source-over rendering, so the full source rectangle is opaque.
    pub(crate) fully_opaque: bool,
}

unsafe impl Send for FrameRgbaView {}
unsafe impl Sync for FrameRgbaView {}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublishedFrame {
    pub(crate) frame: FrameHandle,
    pub(crate) buffer_index: u8,
    pub(crate) publish_serial: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct FrameSnapshot {
    pub(crate) frame: FrameHandle,
    pub(crate) plan: FramePlan,
    pub(crate) buffer_count: u8,
    pub(crate) front_buffer: Option<u8>,
    pub(crate) writer_active: bool,
    pub(crate) publish_serial: u64,
}

/// One lock-consistent view of the allocation blockers seen by a streaming
/// producer. This is diagnostic state only: ownership is still transferred
/// exclusively through the checked read/write lease APIs.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct FrameBufferOwnershipProbe {
    pub(crate) buffer_count: u8,
    pub(crate) front_buffer: Option<u8>,
    pub(crate) acquired_mask: u8,
    pub(crate) reader_mask: u8,
    pub(crate) readers: [u16; FRAME_BUFFER_CAPACITY],
}

#[derive(Copy, Clone)]
struct AcquiredBuffer {
    index: u8,
    token: u64,
}

#[derive(Copy, Clone)]
struct FrameRecord {
    generation: u32,
    active: bool,
    plan: FramePlan,
    surfaces: [Option<UiSurfaceHandle>; FRAME_BUFFER_CAPACITY],
    buffer_count: u8,
    front_buffer: Option<u8>,
    next_buffer: u8,
    // Producer leases are per backing allocation. A streaming video bridge can
    // therefore retain the live and pending-display allocations while one RCS
    // write executes and the next immutable job waits on another allocation.
    acquired: [Option<AcquiredBuffer>; FRAME_BUFFER_CAPACITY],
    readers: [u16; FRAME_BUFFER_CAPACITY],
    gpu_authored: [bool; FRAME_BUFFER_CAPACITY],
    gpu_release: [Option<FrameGpuRelease>; FRAME_BUFFER_CAPACITY],
    fully_opaque: [bool; FRAME_BUFFER_CAPACITY],
    /// Pixels are intentionally uninitialized until a full-frame GPU write.
    /// Ordinary CPU publication is forbidden for every lease in this frame.
    gpu_full_overwrite_required: bool,
    next_token: u64,
    publish_serial: u64,
}

impl FrameRecord {
    fn inactive(generation: u32) -> Self {
        Self {
            generation,
            active: false,
            plan: EMPTY_PLAN,
            surfaces: [None; FRAME_BUFFER_CAPACITY],
            buffer_count: 0,
            front_buffer: None,
            next_buffer: 0,
            acquired: [None; FRAME_BUFFER_CAPACITY],
            readers: [0; FRAME_BUFFER_CAPACITY],
            gpu_authored: [false; FRAME_BUFFER_CAPACITY],
            gpu_release: [None; FRAME_BUFFER_CAPACITY],
            fully_opaque: [false; FRAME_BUFFER_CAPACITY],
            gpu_full_overwrite_required: false,
            next_token: 0,
            publish_serial: 0,
        }
    }

    fn activate(
        &mut self,
        plan: FramePlan,
        surfaces: [Option<UiSurfaceHandle>; FRAME_BUFFER_CAPACITY],
        gpu_full_overwrite_required: bool,
    ) {
        self.generation = next_generation(self.generation);
        self.active = true;
        self.plan = plan;
        self.surfaces = surfaces;
        self.buffer_count = plan.buffering.count() as u8;
        self.front_buffer = None;
        self.next_buffer = 0;
        self.acquired = [None; FRAME_BUFFER_CAPACITY];
        self.readers = [0; FRAME_BUFFER_CAPACITY];
        self.gpu_authored = [false; FRAME_BUFFER_CAPACITY];
        self.gpu_release = [None; FRAME_BUFFER_CAPACITY];
        self.fully_opaque = [false; FRAME_BUFFER_CAPACITY];
        self.gpu_full_overwrite_required = gpu_full_overwrite_required;
        self.next_token = 0;
        self.publish_serial = 0;
    }
}

const EMPTY_PLAN: FramePlan = FramePlan {
    output: super::OutputId::from_slot(0).unwrap(),
    content: super::FrameContent::Image,
    cadence: super::FrameCadence::Immutable,
    format: ScanoutFormat::Xrgb8888,
    alpha: super::AlphaContract::Opaque,
    plane: super::PlaneAssignment::Primary { slot: 0 },
    buffering: super::FrameBuffering::Single,
    width: 1,
    height: 1,
    base_color: None,
};

struct FramePool {
    frames: Vec<FrameRecord>,
}

impl FramePool {
    const fn new() -> Self {
        Self { frames: Vec::new() }
    }

    fn active_count(&self) -> usize {
        self.frames.iter().filter(|frame| frame.active).count()
    }

    fn checked(&self, handle: FrameHandle) -> Result<&FrameRecord, FramePoolError> {
        let (slot, generation) = unpack_handle(handle)?;
        let frame = self.frames.get(slot).ok_or(FramePoolError::InvalidHandle)?;
        if !frame.active || frame.generation != generation {
            return Err(FramePoolError::InvalidHandle);
        }
        Ok(frame)
    }

    fn checked_mut(&mut self, handle: FrameHandle) -> Result<&mut FrameRecord, FramePoolError> {
        let (slot, generation) = unpack_handle(handle)?;
        let frame = self
            .frames
            .get_mut(slot)
            .ok_or(FramePoolError::InvalidHandle)?;
        if !frame.active || frame.generation != generation {
            return Err(FramePoolError::InvalidHandle);
        }
        Ok(frame)
    }
}

static FRAME_POOL: Mutex<FramePool> = Mutex::new(FramePool::new());

/// Count producer-owned frame allocations which have not completed teardown.
/// Inactive registry slots are retained for generation-safe handle reuse and
/// therefore do not contribute to this live count.
pub(super) fn active_frame_count() -> usize {
    FRAME_POOL.lock().active_count()
}

pub(crate) fn create_frame(spec: FrameSpec) -> Result<FrameHandle, FramePoolError> {
    create_frame_admitted(spec, false)
}

/// Create a dirty/double Blueprint frame whose allocations are never CPU
/// initialized. Publication remains impossible until a full-surface compute
/// release is attached to the exact acquired allocation.
pub(crate) fn create_gpu_full_overwrite_frame(
    spec: FrameSpec,
) -> Result<FrameHandle, FramePoolError> {
    if !matches!(
        (spec.content, spec.cadence, spec.buffering, spec.format),
        (
            FrameContent::BlueprintScene,
            super::FrameCadence::Dirty,
            super::FrameBuffering::Double,
            ScanoutFormat::Rgba8888Premultiplied,
        )
    ) || spec.base_color.is_some()
    {
        return Err(FramePoolError::UnsupportedFormat);
    }
    create_frame_admitted(spec, true)
}

fn create_frame_admitted(
    spec: FrameSpec,
    gpu_full_overwrite_required: bool,
) -> Result<FrameHandle, FramePoolError> {
    let plan = FramePlan::from_spec(spec).map_err(FramePoolError::InvalidPlan)?;
    let format = surface_format(plan.format).ok_or(FramePoolError::UnsupportedFormat)?;
    let count = plan.buffering.count();
    let mut surfaces = [None; FRAME_BUFFER_CAPACITY];
    for surface in surfaces.iter_mut().take(count) {
        let allocation = if gpu_full_overwrite_required {
            ui_surface::create_gpu_full_overwrite_surface(plan.width, plan.height, format)
        } else {
            ui_surface::create_surface(plan.width, plan.height, format)
        };
        match allocation {
            Ok(handle) => *surface = Some(handle),
            Err(error) => {
                destroy_surfaces(surfaces);
                return Err(map_surface_error(error));
            }
        }
    }
    if let Some(color) = plan.base_color
        && !initialize_rgba_surfaces(surfaces, count, color)
    {
        destroy_surfaces(surfaces);
        return Err(FramePoolError::UnsupportedFormat);
    }

    let mut pool = FRAME_POOL.lock();
    let slot = if let Some(slot) = pool.frames.iter().position(|frame| !frame.active) {
        slot
    } else {
        if pool.frames.len() >= MAX_FRAMES {
            drop(pool);
            destroy_surfaces(surfaces);
            return Err(FramePoolError::OutOfMemory);
        }
        let slot = pool.frames.len();
        pool.frames.push(FrameRecord::inactive(0));
        slot
    };
    pool.frames[slot].activate(plan, surfaces, gpu_full_overwrite_required);
    let generation = pool.frames[slot].generation;
    pack_handle(slot, generation)
}

pub(crate) fn destroy_frame(handle: FrameHandle) -> Result<(), FramePoolError> {
    let surfaces = {
        let mut pool = FRAME_POOL.lock();
        let frame = pool.checked_mut(handle)?;
        if frame_has_live_buffer_owners(frame) {
            return Err(FramePoolError::Busy);
        }
        // The Render0 alias must disappear before the physical allocation can
        // return to DMA. Keeping FRAME_POOL locked closes acquisition while the
        // no-writer/no-reader proof is consumed. Display/GGTT ownership is
        // represented by `readers`; the alias itself exists only in Render0's
        // PPGTT and is safe to recycle after this point.
        if frame.plan.content == FrameContent::RenderScene3d
            && !release_resident_scene_direct_imports(frame)
        {
            return Err(FramePoolError::Busy);
        }
        frame.active = false;
        frame.front_buffer = None;
        frame.buffer_count = 0;
        core::mem::replace(&mut frame.surfaces, [None; FRAME_BUFFER_CAPACITY])
    };
    destroy_surfaces(surfaces);
    Ok(())
}

fn frame_has_live_buffer_owners(frame: &FrameRecord) -> bool {
    buffer_owners_live(&frame.acquired, &frame.readers)
}

fn buffer_owners_live(
    acquired: &[Option<AcquiredBuffer>; FRAME_BUFFER_CAPACITY],
    readers: &[u16; FRAME_BUFFER_CAPACITY],
) -> bool {
    acquired.iter().any(Option::is_some) || readers.iter().any(|readers| *readers != 0)
}

fn release_resident_scene_direct_imports(frame: &FrameRecord) -> bool {
    frame.surfaces.iter().flatten().copied().all(|surface| {
        ui_surface::rgba_access(surface).is_some_and(|access| {
            crate::intel::render::release_resident_scene_direct_ui4_target(
                access.phys,
                access.byte_len,
            )
        })
    })
}

#[cfg(test)]
mod resident_scene_frame_destroy_tests {
    use super::{
        AcquiredBuffer, FRAME_BUFFER_CAPACITY, FramePool, FrameRecord, buffer_owners_live,
    };

    #[test]
    fn active_count_ignores_inactive_generation_slots() {
        let mut pool = FramePool::new();
        pool.frames.push(FrameRecord::inactive(3));
        pool.frames.push(FrameRecord::inactive(7));
        assert_eq!(pool.active_count(), 0);

        pool.frames[1].active = true;
        assert_eq!(pool.active_count(), 1);
    }

    #[test]
    fn render_alias_release_requires_every_owner_to_be_gone() {
        let mut acquired = [None; FRAME_BUFFER_CAPACITY];
        let mut readers = [0; FRAME_BUFFER_CAPACITY];
        assert!(!buffer_owners_live(&acquired, &readers));

        acquired[1] = Some(AcquiredBuffer { index: 1, token: 7 });
        assert!(buffer_owners_live(&acquired, &readers));
        acquired[1] = None;

        readers[2] = 1;
        assert!(buffer_owners_live(&acquired, &readers));
        readers[2] = 0;
        assert!(!buffer_owners_live(&acquired, &readers));
    }
}

pub(crate) fn acquire_frame_buffer(handle: FrameHandle) -> Result<FrameWriteLease, FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(handle)?;
    if frame.buffer_count == 1 && frame.front_buffer.is_some() {
        return Err(FramePoolError::ImmutablePublished);
    }

    let count = frame.buffer_count;
    let index = (0..count)
        .map(|offset| (frame.next_buffer + offset) % count)
        .find(|index| {
            (count == 1 || frame.front_buffer != Some(*index))
                && frame.acquired[*index as usize].is_none()
                && frame.readers[*index as usize] == 0
                && frame.surfaces[*index as usize].is_some()
        })
        .ok_or(FramePoolError::Busy)?;

    frame.next_buffer = (index + 1) % count;
    frame.gpu_authored[index as usize] = false;
    frame.gpu_release[index as usize] = None;
    frame.fully_opaque[index as usize] = false;
    frame.next_token = next_serial(frame.next_token);
    let acquired = AcquiredBuffer {
        index,
        token: frame.next_token,
    };
    frame.acquired[index as usize] = Some(acquired);
    Ok(FrameWriteLease {
        frame: handle,
        buffer_index: index,
        token: acquired.token,
    })
}

pub(crate) fn frame_buffer_ownership_probe(
    handle: FrameHandle,
) -> Result<FrameBufferOwnershipProbe, FramePoolError> {
    let pool = FRAME_POOL.lock();
    let frame = pool.checked(handle)?;
    let mut acquired_mask = 0u8;
    let mut reader_mask = 0u8;
    for index in 0..frame.buffer_count as usize {
        if frame.acquired[index].is_some() {
            acquired_mask |= 1u8 << index;
        }
        if frame.readers[index] != 0 {
            reader_mask |= 1u8 << index;
        }
    }
    Ok(FrameBufferOwnershipProbe {
        buffer_count: frame.buffer_count,
        front_buffer: frame.front_buffer,
        acquired_mask,
        reader_mask,
        readers: frame.readers,
    })
}

pub(crate) fn acquire_published_frame(
    handle: FrameHandle,
) -> Result<FrameReadLease, FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(handle)?;
    let index = frame.front_buffer.ok_or(FramePoolError::NotPublished)?;
    let readers = &mut frame.readers[index as usize];
    *readers = readers.checked_add(1).ok_or(FramePoolError::Busy)?;
    Ok(FrameReadLease {
        frame: handle,
        buffer_index: index,
    })
}

/// Retain the exact buffer already pinned by `lease`.
///
/// A direct-scanout presenter uses this to transfer ownership of a published
/// front buffer from the compositor transaction to the display plane.  It
/// must not reacquire `frame.front_buffer`: a streaming producer may have
/// published a newer buffer while the compositor was waiting for SURFLIVE.
pub(crate) fn retain_published_frame(
    lease: FrameReadLease,
) -> Result<FrameReadLease, FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    let index = lease.buffer_index as usize;
    if index >= frame.buffer_count as usize || frame.readers[index] == 0 {
        return Err(FramePoolError::InvalidLease);
    }
    frame.readers[index] = frame.readers[index]
        .checked_add(1)
        .ok_or(FramePoolError::Busy)?;
    Ok(lease)
}

pub(crate) fn published_rgba_view(lease: FrameReadLease) -> Result<FrameRgbaView, FramePoolError> {
    let (surface, gpu_authored, gpu_release, fully_opaque) = {
        let pool = FRAME_POOL.lock();
        let frame = pool.checked(lease.frame)?;
        if frame.readers[lease.buffer_index as usize] == 0 {
            return Err(FramePoolError::InvalidLease);
        }
        if frame.plan.format != ScanoutFormat::Rgba8888Premultiplied {
            return Err(FramePoolError::UnsupportedFormat);
        }
        (
            frame.surfaces[lease.buffer_index as usize].ok_or(FramePoolError::InvalidLease)?,
            frame.gpu_authored[lease.buffer_index as usize],
            frame.gpu_release[lease.buffer_index as usize],
            frame.fully_opaque[lease.buffer_index as usize],
        )
    };
    let access = ui_surface::rgba_access(surface).ok_or(FramePoolError::InvalidLease)?;
    Ok(FrameRgbaView {
        phys: access.phys,
        gpu: access.gpu,
        virt: access.virt,
        byte_len: access.byte_len,
        width: access.width,
        height: access.height,
        pitch: access.pitch,
        gpu_authored,
        gpu_release,
        fully_opaque,
    })
}

pub(crate) fn writable_rgba_view(lease: FrameWriteLease) -> Result<FrameRgbaView, FramePoolError> {
    let surface = {
        let pool = FRAME_POOL.lock();
        let frame = pool.checked(lease.frame)?;
        checked_lease(frame, lease)?;
        if frame.plan.format != ScanoutFormat::Rgba8888Premultiplied {
            return Err(FramePoolError::UnsupportedFormat);
        }
        frame.surfaces[lease.buffer_index as usize].ok_or(FramePoolError::InvalidLease)?
    };
    let access = ui_surface::rgba_access(surface).ok_or(FramePoolError::InvalidLease)?;
    Ok(FrameRgbaView {
        phys: access.phys,
        gpu: access.gpu,
        virt: access.virt,
        byte_len: access.byte_len,
        width: access.width,
        height: access.height,
        pitch: access.pitch,
        gpu_authored: false,
        gpu_release: None,
        fully_opaque: false,
    })
}

pub(crate) fn release_published_frame(lease: FrameReadLease) -> Result<(), FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    let readers = &mut frame.readers[lease.buffer_index as usize];
    if *readers == 0 {
        return Err(FramePoolError::InvalidLease);
    }
    *readers -= 1;
    Ok(())
}

pub(crate) fn cancel_frame_buffer(lease: FrameWriteLease) -> Result<(), FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    checked_lease(frame, lease)?;
    frame.acquired[lease.buffer_index as usize] = None;
    Ok(())
}

pub(crate) fn gpgpu_rgba_surface(
    lease: FrameWriteLease,
) -> Result<crate::intel::gpgpu::GpgpuRgba8Surface, FramePoolError> {
    let surface = {
        let mut pool = FRAME_POOL.lock();
        let frame = pool.checked_mut(lease.frame)?;
        checked_lease(frame, lease)?;
        if frame.plan.format != ScanoutFormat::Rgba8888Premultiplied {
            return Err(FramePoolError::UnsupportedFormat);
        }
        let index = lease.buffer_index as usize;
        let surface = frame.surfaces[index].ok_or(FramePoolError::InvalidLease)?;
        frame.gpu_authored[index] = true;
        surface
    };
    ui_surface::gpgpu_rgba_surface(surface).ok_or(FramePoolError::InvalidLease)
}

pub(crate) fn publish_frame_buffer(
    lease: FrameWriteLease,
) -> Result<PublishedFrame, FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    checked_lease(frame, lease)?;
    if frame.gpu_full_overwrite_required {
        return Err(FramePoolError::ProducerReleaseRequired);
    }
    if matches!(
        frame.plan.content,
        FrameContent::FontScene2d
            | FrameContent::RenderScene3d
            | FrameContent::BlueprintScene
            | FrameContent::Video
    ) && frame.gpu_authored[lease.buffer_index as usize]
    {
        return Err(FramePoolError::ProducerReleaseRequired);
    }
    publish_checked_frame(frame, lease)
}

/// Reclassify a leased RGBA allocation for a complete CPU overwrite path. This
/// is used only when a compute dispatch was never admitted; an accepted GPU
/// submission must instead retire or quarantine its exact allocation.
pub(crate) fn mark_frame_buffer_cpu_authored(lease: FrameWriteLease) -> Result<(), FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    checked_lease(frame, lease)?;
    if frame.gpu_full_overwrite_required {
        return Err(FramePoolError::ProducerReleaseRequired);
    }
    let index = lease.buffer_index as usize;
    frame.gpu_authored[index] = false;
    frame.gpu_release[index] = None;
    Ok(())
}

/// Mark a leased frame as fully opaque after the producer has overwritten the
/// complete allocation with alpha 255. Later GPU source-over rendering retains
/// this invariant, enabling an ordered copy fallback for the slot-0 stack.
pub(crate) fn mark_frame_buffer_fully_opaque(
    lease: FrameWriteLease,
) -> Result<(), FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    checked_lease(frame, lease)?;
    frame.fully_opaque[lease.buffer_index as usize] = true;
    Ok(())
}

/// Publish one GPU-authored resident-scene allocation only after its actual
/// final writer's release packet has retired. No pixel is read, copied, or
/// cache-flushed by the CPU here; this transfers ownership metadata to UI4.
pub(crate) fn publish_gpu_frame_buffer(
    lease: FrameWriteLease,
    release: crate::intel::render::ResidentSceneReleaseFence,
) -> Result<PublishedFrame, FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    checked_lease(frame, lease)?;
    let index = lease.buffer_index as usize;
    if frame.plan.content != FrameContent::RenderScene3d
        || frame.plan.cadence != super::FrameCadence::Streaming
        || frame.plan.buffering != super::FrameBuffering::Triple
        || frame.plan.format != ScanoutFormat::Rgba8888Premultiplied
        || !frame.gpu_authored[index]
    {
        return Err(FramePoolError::ProducerReleaseRequired);
    }
    let surface = frame.surfaces[index].ok_or(FramePoolError::InvalidLease)?;
    let access = ui_surface::rgba_access(surface).ok_or(FramePoolError::InvalidLease)?;
    if !release.matches(access.phys, access.byte_len) {
        return Err(FramePoolError::InvalidLease);
    }
    frame.gpu_release[index] = Some(FrameGpuRelease::ResidentScene(release));
    publish_checked_frame(frame, lease)
}

/// Publish one Blueprint scene written directly by the resident Render0
/// producer. This is the graphics-pipeline counterpart to
/// `publish_gpgpu_scene_frame_buffer`: both retain the Blueprint frame's
/// cadence-selected ring, but the release proof identifies the engine that
/// performed the final write.
pub(crate) fn publish_resident_scene_frame_buffer(
    lease: FrameWriteLease,
    release: crate::intel::render::ResidentSceneReleaseFence,
) -> Result<PublishedFrame, FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    checked_lease(frame, lease)?;
    let index = lease.buffer_index as usize;
    if frame.plan.content != FrameContent::BlueprintScene
        || !blueprint_gpu_release_plan(frame.plan.cadence, frame.plan.buffering)
        || frame.plan.format != ScanoutFormat::Rgba8888Premultiplied
        || !frame.gpu_authored[index]
    {
        return Err(FramePoolError::ProducerReleaseRequired);
    }
    let surface = frame.surfaces[index].ok_or(FramePoolError::InvalidLease)?;
    let access = ui_surface::rgba_access(surface).ok_or(FramePoolError::InvalidLease)?;
    if !release.matches(access.phys, access.byte_len) {
        return Err(FramePoolError::InvalidLease);
    }
    frame.gpu_release[index] = Some(FrameGpuRelease::ResidentScene(release));
    publish_checked_frame(frame, lease)
}

/// Publish a streaming render-scene frame whose final writer was the compute
/// sprite path rather than the retained 3D renderer.
pub(crate) fn publish_gpgpu_render_frame_buffer(
    lease: FrameWriteLease,
    release: crate::intel::gpgpu::GpgpuRgba8ReleaseFence,
) -> Result<PublishedFrame, FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    checked_lease(frame, lease)?;
    let index = lease.buffer_index as usize;
    if frame.plan.content != FrameContent::RenderScene3d
        || frame.plan.cadence != super::FrameCadence::Streaming
        || frame.plan.buffering != super::FrameBuffering::Triple
        || frame.plan.format != ScanoutFormat::Rgba8888Premultiplied
        || !frame.gpu_authored[index]
    {
        return Err(FramePoolError::ProducerReleaseRequired);
    }
    let surface = frame.surfaces[index].ok_or(FramePoolError::InvalidLease)?;
    let access = ui_surface::rgba_access(surface).ok_or(FramePoolError::InvalidLease)?;
    if !release.matches(access.phys, access.byte_len) {
        return Err(FramePoolError::InvalidLease);
    }
    frame.gpu_release[index] = Some(FrameGpuRelease::Compute(release));
    publish_checked_frame(frame, lease)
}

/// Publish one full-surface compute allocation only after its final
/// PIPE_CONTROL and post-sync marker retired. This is the double-buffered
/// counterpart to resident-scene publication: metadata changes ownership, while the
/// CPU neither reads nor copies the pixels.
pub(crate) fn publish_gpgpu_frame_buffer(
    lease: FrameWriteLease,
    release: crate::intel::gpgpu::GpgpuRgba8ReleaseFence,
) -> Result<PublishedFrame, FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    checked_lease(frame, lease)?;
    let index = lease.buffer_index as usize;
    if frame.plan.content != FrameContent::Image
        || frame.plan.buffering != super::FrameBuffering::Double
        || !frame.gpu_authored[index]
    {
        return Err(FramePoolError::ProducerReleaseRequired);
    }
    let surface = frame.surfaces[index].ok_or(FramePoolError::InvalidLease)?;
    let access = ui_surface::rgba_access(surface).ok_or(FramePoolError::InvalidLease)?;
    if !release.matches(access.phys, access.byte_len) {
        return Err(FramePoolError::InvalidLease);
    }
    frame.gpu_release[index] = Some(FrameGpuRelease::Compute(release));
    publish_checked_frame(frame, lease)
}

/// Publish one font scene written directly into its exact UI4 allocation.
///
/// The font worker retains the frame write lease indirectly through its
/// caller, writes the premultiplied RGBA8 canvas in place, and returns a
/// release proof bound to this exact surface. No staging allocation, CPU
/// readback, or frame copy crosses this boundary. One-shot presenters use an
/// immutable/single frame; bounded live probes may reuse dirty/double back
/// buffers after explicitly clearing the acquired destination.
pub(crate) fn publish_gpu_font_frame_buffer(
    lease: FrameWriteLease,
    release: crate::intel::gpgpu::GpgpuRgba8ReleaseFence,
) -> Result<PublishedFrame, FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    checked_lease(frame, lease)?;
    let index = lease.buffer_index as usize;
    let valid_lifecycle = matches!(
        (frame.plan.cadence, frame.plan.buffering),
        (super::FrameCadence::Immutable, super::FrameBuffering::Single)
            | (super::FrameCadence::Dirty, super::FrameBuffering::Double)
    );
    if frame.plan.content != FrameContent::FontScene2d
        || !valid_lifecycle
        || frame.plan.format != ScanoutFormat::Rgba8888Premultiplied
        || !frame.gpu_authored[index]
    {
        return Err(FramePoolError::ProducerReleaseRequired);
    }
    let surface = frame.surfaces[index].ok_or(FramePoolError::InvalidLease)?;
    let access = ui_surface::rgba_access(surface).ok_or(FramePoolError::InvalidLease)?;
    if !release.matches(access.phys, access.byte_len) {
        return Err(FramePoolError::InvalidLease);
    }
    frame.gpu_release[index] = Some(FrameGpuRelease::Compute(release));
    publish_checked_frame(frame, lease)
}

/// Publish one decoder-converted RGBA allocation. GuC completion proves the
/// native NV12 source is no longer read; only this exact streaming Frame
/// surface and its compute release cross the broker boundary. Four allocations
/// cover live scanout, its pending replacement, and both immutable RCS slots.
pub(crate) fn publish_gpgpu_video_frame_buffer(
    lease: FrameWriteLease,
    release: crate::intel::gpgpu::GpgpuRgba8ReleaseFence,
) -> Result<PublishedFrame, FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    checked_lease(frame, lease)?;
    let index = lease.buffer_index as usize;
    if frame.plan.content != FrameContent::Video
        || frame.plan.cadence != super::FrameCadence::Streaming
        || frame.plan.buffering != super::FrameBuffering::Quad
        || frame.plan.format != ScanoutFormat::Rgba8888Premultiplied
        || !super::video_frame_extent_admitted(frame.plan.width, frame.plan.height)
        || !frame.gpu_authored[index]
    {
        return Err(FramePoolError::ProducerReleaseRequired);
    }
    let surface = frame.surfaces[index].ok_or(FramePoolError::InvalidLease)?;
    let access = ui_surface::rgba_access(surface).ok_or(FramePoolError::InvalidLease)?;
    if !release.matches(access.phys, access.byte_len) {
        return Err(FramePoolError::InvalidLease);
    }
    frame.gpu_release[index] = Some(FrameGpuRelease::Compute(release));
    publish_checked_frame(frame, lease)
}

/// Publish one Blueprint scene written directly by a compute producer.
///
/// Blueprint cadence selects its ring size: immutable scenes use one buffer,
/// dirty scenes use two, and streaming scenes use three. The release binds the
/// completed write to this exact allocation in every case; display ownership
/// still ends only at SURFLIVE.
pub(crate) fn publish_gpgpu_scene_frame_buffer(
    lease: FrameWriteLease,
    release: crate::intel::gpgpu::GpgpuRgba8ReleaseFence,
) -> Result<PublishedFrame, FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    checked_lease(frame, lease)?;
    let index = lease.buffer_index as usize;
    if frame.plan.content != FrameContent::BlueprintScene
        || !blueprint_gpu_release_plan(frame.plan.cadence, frame.plan.buffering)
        || !frame.gpu_authored[index]
    {
        return Err(FramePoolError::ProducerReleaseRequired);
    }
    let surface = frame.surfaces[index].ok_or(FramePoolError::InvalidLease)?;
    let access = ui_surface::rgba_access(surface).ok_or(FramePoolError::InvalidLease)?;
    if !release.matches(access.phys, access.byte_len) {
        return Err(FramePoolError::InvalidLease);
    }
    frame.gpu_release[index] = Some(FrameGpuRelease::Compute(release));
    publish_checked_frame(frame, lease)
}

const fn blueprint_gpu_release_plan(
    cadence: super::FrameCadence,
    buffering: super::FrameBuffering,
) -> bool {
    matches!(
        (cadence, buffering),
        (super::FrameCadence::Immutable, super::FrameBuffering::Single)
            | (super::FrameCadence::Dirty, super::FrameBuffering::Double)
            | (super::FrameCadence::Streaming, super::FrameBuffering::Triple)
    )
}

const _: () = {
    assert!(blueprint_gpu_release_plan(
        super::FrameCadence::Immutable,
        super::FrameBuffering::Single,
    ));
    assert!(blueprint_gpu_release_plan(
        super::FrameCadence::Dirty,
        super::FrameBuffering::Double,
    ));
    assert!(blueprint_gpu_release_plan(
        super::FrameCadence::Streaming,
        super::FrameBuffering::Triple,
    ));
    assert!(!blueprint_gpu_release_plan(
        super::FrameCadence::Immutable,
        super::FrameBuffering::Triple,
    ));
};

fn publish_checked_frame(
    frame: &mut FrameRecord,
    lease: FrameWriteLease,
) -> Result<PublishedFrame, FramePoolError> {
    frame.front_buffer = Some(lease.buffer_index);
    frame.acquired[lease.buffer_index as usize] = None;
    frame.publish_serial = next_serial(frame.publish_serial);
    Ok(PublishedFrame {
        frame: lease.frame,
        buffer_index: lease.buffer_index,
        publish_serial: frame.publish_serial,
    })
}

pub(crate) fn frame_snapshot(handle: FrameHandle) -> Result<FrameSnapshot, FramePoolError> {
    let pool = FRAME_POOL.lock();
    let frame = pool.checked(handle)?;
    Ok(FrameSnapshot {
        frame: handle,
        plan: frame.plan,
        buffer_count: frame.buffer_count,
        front_buffer: frame.front_buffer,
        writer_active: frame.acquired.iter().any(Option::is_some),
        publish_serial: frame.publish_serial,
    })
}

fn checked_lease(frame: &FrameRecord, lease: FrameWriteLease) -> Result<(), FramePoolError> {
    match frame.acquired[lease.buffer_index as usize] {
        Some(acquired) if acquired.index == lease.buffer_index && acquired.token == lease.token => {
            Ok(())
        }
        _ => Err(FramePoolError::InvalidLease),
    }
}

fn surface_format(format: ScanoutFormat) -> Option<UiSurfaceFormat> {
    match format {
        ScanoutFormat::Xrgb8888 => Some(UiSurfaceFormat::Xrgb8888),
        ScanoutFormat::Xbgr8888 => Some(UiSurfaceFormat::Xbgr8888),
        ScanoutFormat::Rgba8888Premultiplied => Some(UiSurfaceFormat::Rgba8888),
    }
}

fn destroy_surfaces(surfaces: [Option<UiSurfaceHandle>; FRAME_BUFFER_CAPACITY]) {
    for surface in surfaces.into_iter().flatten() {
        let _ = ui_surface::destroy_surface(surface);
    }
}

/// Initialize the complete cadence-selected ring before its handle becomes
/// visible. In particular, dirty/triple-buffer rotation can never expose a
/// zeroed backing buffer when the consumer requested an opaque or translucent
/// base color.
fn initialize_rgba_surfaces(
    surfaces: [Option<UiSurfaceHandle>; FRAME_BUFFER_CAPACITY],
    count: usize,
    color: PremultipliedRgba8,
) -> bool {
    let pixel = u32::from_le_bytes(color.to_native_bytes());
    let mut initialized = 0usize;
    for surface in surfaces.into_iter().take(count) {
        let Some(surface) = surface else {
            return false;
        };
        let Some(access) = ui_surface::rgba_access(surface) else {
            return false;
        };
        if access.virt.is_null() || access.byte_len % core::mem::size_of::<u32>() != 0 {
            return false;
        }
        let words = access.byte_len / core::mem::size_of::<u32>();
        let dst = access.virt.cast::<u32>();
        for offset in 0..words {
            unsafe {
                core::ptr::write(dst.add(offset), pixel);
            }
        }
        crate::intel::dma_flush(access.virt, access.byte_len);
        initialized += 1;
    }
    initialized == count
}

fn map_surface_error(error: SurfaceError) -> FramePoolError {
    match error {
        SurfaceError::OutOfMemory => FramePoolError::OutOfMemory,
        SurfaceError::Unsupported => FramePoolError::UnsupportedFormat,
        SurfaceError::Invalid | SurfaceError::NotFound => FramePoolError::InvalidHandle,
    }
}

fn next_generation(generation: u32) -> u32 {
    generation.wrapping_add(1).max(1)
}

fn next_serial(serial: u64) -> u64 {
    serial.wrapping_add(1).max(1)
}

fn pack_handle(slot: usize, generation: u32) -> Result<FrameHandle, FramePoolError> {
    let slot = u32::try_from(slot)
        .ok()
        .and_then(|slot| slot.checked_add(1))
        .ok_or(FramePoolError::OutOfMemory)?;
    Ok(FrameHandle((u64::from(generation) << 32) | u64::from(slot)))
}

fn unpack_handle(handle: FrameHandle) -> Result<(usize, u32), FramePoolError> {
    let raw = handle.raw();
    let generation = (raw >> 32) as u32;
    let slot = raw as u32;
    if generation == 0 || slot == 0 {
        return Err(FramePoolError::InvalidHandle);
    }
    Ok(((slot - 1) as usize, generation))
}
