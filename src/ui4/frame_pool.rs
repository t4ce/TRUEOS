//! UI4-owned frame storage and producer/publisher hand-off.
//!
//! A logical frame owns exactly the explicitly requested number of physical
//! buffers. Cadence and buffering are independent contract dimensions. The
//! trusted `ui_surface` allocator remains an implementation detail; producers
//! receive a checked write lease and never a display-plane address.

use alloc::vec::Vec;
use embassy_sync::signal::Signal;
use spin::Mutex;

use crate::graphics::primitives::{Error as SurfaceError, UiSurfaceFormat};
use crate::r::ui_surface::{self, UiSurfaceHandle};

use super::{
    FrameContent, FrameHandle, FramePlan, FramePlanError, FrameSpec, PremultipliedRgba8,
    ScanoutFormat,
};

const MAX_FRAMES: usize = 64;

/// Coalesced notification for the single supported streaming producer.  The
/// frame handle lets an awakened producer ignore releases belonging to another
/// logical frame without polling the pool.
static FRAME_BUFFER_RELEASED: Signal<crate::wait::EmbassySpinRawMutex, FrameHandle> = Signal::new();

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
    surfaces: [Option<UiSurfaceHandle>; 3],
    buffer_count: u8,
    front_buffer: Option<u8>,
    next_buffer: u8,
    acquired: Option<AcquiredBuffer>,
    readers: [u16; 3],
    gpu_authored: [bool; 3],
    gpu_release: [Option<FrameGpuRelease>; 3],
    next_token: u64,
    publish_serial: u64,
}

impl FrameRecord {
    fn inactive(generation: u32) -> Self {
        Self {
            generation,
            active: false,
            plan: EMPTY_PLAN,
            surfaces: [None; 3],
            buffer_count: 0,
            front_buffer: None,
            next_buffer: 0,
            acquired: None,
            readers: [0; 3],
            gpu_authored: [false; 3],
            gpu_release: [None; 3],
            next_token: 0,
            publish_serial: 0,
        }
    }

    fn activate(&mut self, plan: FramePlan, surfaces: [Option<UiSurfaceHandle>; 3]) {
        self.generation = next_generation(self.generation);
        self.active = true;
        self.plan = plan;
        self.surfaces = surfaces;
        self.buffer_count = plan.buffering.count() as u8;
        self.front_buffer = None;
        self.next_buffer = 0;
        self.acquired = None;
        self.readers = [0; 3];
        self.gpu_authored = [false; 3];
        self.gpu_release = [None; 3];
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

pub(crate) fn create_frame(spec: FrameSpec) -> Result<FrameHandle, FramePoolError> {
    let plan = FramePlan::from_spec(spec).map_err(FramePoolError::InvalidPlan)?;
    let format = surface_format(plan.format).ok_or(FramePoolError::UnsupportedFormat)?;
    let count = plan.buffering.count();
    let mut surfaces = [None; 3];
    for surface in surfaces.iter_mut().take(count) {
        match ui_surface::create_surface(plan.width, plan.height, format) {
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
    pool.frames[slot].activate(plan, surfaces);
    let generation = pool.frames[slot].generation;
    pack_handle(slot, generation)
}

pub(crate) fn destroy_frame(handle: FrameHandle) -> Result<(), FramePoolError> {
    let surfaces = {
        let mut pool = FRAME_POOL.lock();
        let frame = pool.checked_mut(handle)?;
        if frame.acquired.is_some() || frame.readers.iter().any(|readers| *readers != 0) {
            return Err(FramePoolError::Busy);
        }
        frame.active = false;
        frame.front_buffer = None;
        frame.buffer_count = 0;
        core::mem::replace(&mut frame.surfaces, [None; 3])
    };
    destroy_surfaces(surfaces);
    Ok(())
}

pub(crate) fn acquire_frame_buffer(handle: FrameHandle) -> Result<FrameWriteLease, FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(handle)?;
    if frame.acquired.is_some() {
        return Err(FramePoolError::Busy);
    }
    if frame.buffer_count == 1 && frame.front_buffer.is_some() {
        return Err(FramePoolError::ImmutablePublished);
    }

    let count = frame.buffer_count;
    let index = (0..count)
        .map(|offset| (frame.next_buffer + offset) % count)
        .find(|index| {
            (count == 1 || frame.front_buffer != Some(*index))
                && frame.readers[*index as usize] == 0
                && frame.surfaces[*index as usize].is_some()
        })
        .ok_or(FramePoolError::Busy)?;

    frame.next_buffer = (index + 1) % count;
    frame.gpu_authored[index as usize] = false;
    frame.gpu_release[index as usize] = None;
    frame.next_token = next_serial(frame.next_token);
    let acquired = AcquiredBuffer {
        index,
        token: frame.next_token,
    };
    frame.acquired = Some(acquired);
    Ok(FrameWriteLease {
        frame: handle,
        buffer_index: index,
        token: acquired.token,
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
    let (surface, gpu_authored, gpu_release) = {
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
    let became_available = *readers == 0 && frame.front_buffer != Some(lease.buffer_index);
    drop(pool);
    if became_available {
        FRAME_BUFFER_RELEASED.signal(lease.frame);
    }
    Ok(())
}

/// Wait until a reader releases a non-front buffer of `handle`.
///
/// The signal is deliberately coalescing: a producer only needs to know that
/// retrying acquisition may now succeed, not how many read leases retired.
pub(crate) async fn wait_frame_buffer_release(handle: FrameHandle) {
    loop {
        if FRAME_BUFFER_RELEASED.wait().await == handle {
            return;
        }
    }
}

pub(crate) fn cancel_frame_buffer(lease: FrameWriteLease) -> Result<(), FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    checked_lease(frame, lease)?;
    frame.acquired = None;
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
    if matches!(
        frame.plan.content,
        FrameContent::RenderScene3d | FrameContent::BlueprintScene | FrameContent::Video
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
    let index = lease.buffer_index as usize;
    frame.gpu_authored[index] = false;
    frame.gpu_release[index] = None;
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
    if frame.plan.content != FrameContent::RenderScene3d || !frame.gpu_authored[index] {
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

/// Publish one full-surface compute allocation only after its final
/// PIPE_CONTROL and post-sync marker retired. This is the double-buffered
/// counterpart to Draw3D publication: metadata changes ownership, while the
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

/// Publish one decoder-converted RGBA allocation. GuC completion proves the
/// native NV12 source is no longer read; only this exact double-buffered Frame
/// surface and its compute release cross the broker boundary.
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
        || frame.plan.buffering != super::FrameBuffering::Double
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

/// Publish one triple-buffered Blueprint scene shaded directly by a compute
/// kernel. The release binds the completed shader write to this exact Frame
/// allocation; display ownership still ends only at SURFLIVE.
pub(crate) fn publish_gpgpu_scene_frame_buffer(
    lease: FrameWriteLease,
    release: crate::intel::gpgpu::GpgpuRgba8ReleaseFence,
) -> Result<PublishedFrame, FramePoolError> {
    let mut pool = FRAME_POOL.lock();
    let frame = pool.checked_mut(lease.frame)?;
    checked_lease(frame, lease)?;
    let index = lease.buffer_index as usize;
    if frame.plan.content != FrameContent::BlueprintScene
        || frame.plan.buffering != super::FrameBuffering::Triple
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

fn publish_checked_frame(
    frame: &mut FrameRecord,
    lease: FrameWriteLease,
) -> Result<PublishedFrame, FramePoolError> {
    frame.front_buffer = Some(lease.buffer_index);
    frame.acquired = None;
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
        writer_active: frame.acquired.is_some(),
        publish_serial: frame.publish_serial,
    })
}

fn checked_lease(frame: &FrameRecord, lease: FrameWriteLease) -> Result<(), FramePoolError> {
    match frame.acquired {
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

fn destroy_surfaces(surfaces: [Option<UiSurfaceHandle>; 3]) {
    for surface in surfaces.into_iter().flatten() {
        let _ = ui_surface::destroy_surface(surface);
    }
}

/// Initialize the complete cadence-selected ring before its handle becomes
/// visible. In particular, dirty/triple-buffer rotation can never expose a
/// zeroed backing buffer when the consumer requested an opaque or translucent
/// base color.
fn initialize_rgba_surfaces(
    surfaces: [Option<UiSurfaceHandle>; 3],
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
