use core::{
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use libm::{cosf, sinf};
use spin::Mutex;
use trueos_gfx_core::{BlendDesc, RgbVertex, Rgba8, SamplerDesc};

const INTEL_RENDER_DEMO_BOOT_DELAY_MS: u64 = 0;
const INTEL_RENDER_DEMO_CLEAR_RGB: u32 = 0x10141A;
const INTEL_RENDER_DEMO_SCANOUT_BRIDGE_ENABLED: bool = true;
const INTEL_RENDER_DEMO_BENCHMARK_MODE_ENABLED: bool = false;
const INTEL_RENDER_DEMO_BENCHMARK_SKIP_CLEAR: bool = true;
const INTEL_RENDER_DEMO_DEBUG_CLEAR_MODE_ENABLED: bool = false;
const INTEL_RENDER_DEMO_DEBUG_CLEAR_RGB_A: u32 = 0x00FF00;
const INTEL_RENDER_DEMO_DEBUG_CLEAR_RGB_B: u32 = 0xFF00FF;
const INTEL_RENDER_DEMO_ENABLED: bool = true;
const INTEL_RENDER_DEMO_FRAME_MS: u64 = 16;
const INTEL_RENDER_DEMO_GUC_READY_POLL_MS: u64 = 10;
const INTEL_RENDER_DEMO_GUC_READY_TIMEOUT_MS: u64 = 3000;
const INTEL_RENDER_DEMO_PHASE_STEP_RAD: f32 = 0.18;
const INTEL_RENDER_DEMO_CLUSTER_COUNT: usize = 15;
const INTEL_RENDER_DEMO_VERTEX_COUNT: usize = INTEL_RENDER_DEMO_CLUSTER_COUNT * 3;
const INTEL_RENDER_DEMO_GPU_VA_BASE: u64 = 0x0400_0000;
const INTEL_RENDER_DEMO_GPU_VA_STRIDE: u64 = 0x0100_0000;
const INTEL_RENDER_DEMO_GPU_VA_SLOT_COUNT: usize = 16;

static INTEL_RENDER_DEMO_DISABLED: AtomicBool = AtomicBool::new(false);
static INTEL_RENDER_DEMO_DISABLE_LATCH_LOGGED: AtomicBool = AtomicBool::new(false);
static INTEL_RENDER_DEMO_GPU_VA_SLOTS: Mutex<u32> = Mutex::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RenderDemoSurfaceFormat {
    Rgba8888,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RenderDemoSurfaceGpuAddrPolicy {
    Fixed(u64),
    ReserveNext { base: u64, stride: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RenderDemoSurfaceDesc {
    pub width: u32,
    pub height: u32,
    pub align: usize,
    pub format: RenderDemoSurfaceFormat,
    pub gpu_addr_policy: RenderDemoSurfaceGpuAddrPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RenderDemoFailureKind {
    Alloc,
    Clear,
    Draw,
    Texture,
    Composite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RenderDemoStartupFailure {
    NoClaimedDevice,
    Guc(super::intel_guc::RenderDemoGucStartupFailure),
    SurfaceAlloc,
    SceneMap,
}

impl RenderDemoStartupFailure {
    const fn label(self) -> &'static str {
        match self {
            Self::NoClaimedDevice => "no-claimed-device",
            Self::Guc(reason) => reason.label(),
            Self::SurfaceAlloc => "surface-alloc-failed",
            Self::SceneMap => "scene-map-failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RenderDemoPassStatus {
    Skipped,
    Completed,
    Failed(RenderDemoFailureKind),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RenderDemoFrameReport {
    pub frame_seq: u32,
    pub clear_rgb: u32,
    pub clear: RenderDemoPassStatus,
    pub rgb: RenderDemoPassStatus,
    pub texture: RenderDemoPassStatus,
    pub composite: RenderDemoPassStatus,
    pub failure: Option<RenderDemoFailureKind>,
    pub elapsed_ms: u64,
    pub clear_ms: u64,
    pub rgb_ms: u64,
    pub composite_ms: u64,
    pub probe_before: u32,
    pub probe_after_clear: u32,
    pub probe_after_rgb: u32,
}

pub(crate) struct RenderDemoOwnedSurface {
    pub surface_phys: u64,
    pub surface_virt: *mut u8,
    pub surface_bytes: usize,
    pub desc: RenderDemoSurfaceDesc,
    pub pitch_bytes: u32,
    pub gpu_addr: u64,
    mapped_gpu_addr: Option<u64>,
    gpu_va_slot: Option<u8>,
}

const DEFAULT_RENDER_DEMO_SCENE_SURFACE_DESC: RenderDemoSurfaceDesc = RenderDemoSurfaceDesc {
    width: 800,
    height: 600,
    align: 4096,
    format: RenderDemoSurfaceFormat::Rgba8888,
    gpu_addr_policy: RenderDemoSurfaceGpuAddrPolicy::ReserveNext {
        base: INTEL_RENDER_DEMO_GPU_VA_BASE,
        stride: INTEL_RENDER_DEMO_GPU_VA_STRIDE,
    },
};

#[derive(Clone, Copy)]
struct RenderDemoGpuAddrReservation {
    gpu_addr: u64,
    slot: Option<u8>,
}

struct RenderDemoState {
    scene_surface: Option<RenderDemoOwnedSurface>,
    texture_surface: Option<RenderDemoOwnedSurface>,
    output_surface: Option<RenderDemoOwnedSurface>,
    output_shadow_surface: Option<RenderDemoOwnedSurface>,
    phase: f32,
    frame_seq: u32,
    stats: RenderDemoStats,
}

#[derive(Clone, Copy)]
struct RenderDemoStats {
    frames_started: u64,
    frames_completed: u64,
    frames_failed: u64,
    alloc_failures: u64,
    clear_failures: u64,
    draw_failures: u64,
    texture_failures: u64,
    composite_failures: u64,
    avg_frame_ms: u64,
    avg_completed_passes_milli: u32,
}

impl RenderDemoSurfaceFormat {
    const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgba8888 => 4,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Rgba8888 => "rgba8888",
        }
    }
}

impl RenderDemoSurfaceGpuAddrPolicy {
    const fn label(self) -> &'static str {
        match self {
            Self::Fixed(_) => "fixed",
            Self::ReserveNext { .. } => "reserve-next",
        }
    }
}

impl RenderDemoSurfaceDesc {
    fn min_row_bytes(self) -> Option<u32> {
        u32::try_from((self.width as usize).checked_mul(self.format.bytes_per_pixel())?).ok()
    }

    fn default_pitch_bytes(self) -> Option<u32> {
        self.min_row_bytes()
    }

    fn surface_bytes_with_pitch(self, pitch_bytes: u32) -> Option<usize> {
        let min_row_bytes = self.min_row_bytes()?;
        if pitch_bytes < min_row_bytes {
            return None;
        }
        (pitch_bytes as usize).checked_mul(self.height as usize)
    }
}

impl RenderDemoFailureKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Alloc => "alloc",
            Self::Clear => "clear",
            Self::Draw => "draw",
            Self::Texture => "texture",
            Self::Composite => "composite",
        }
    }
}

impl RenderDemoPassStatus {
    const fn label(self) -> &'static str {
        match self {
            Self::Skipped => "skipped",
            Self::Completed => "done",
            Self::Failed(reason) => reason.label(),
        }
    }

    const fn completed(self) -> bool {
        matches!(self, Self::Completed)
    }
}

impl RenderDemoFrameReport {
    const fn new(frame_seq: u32, clear_rgb: u32) -> Self {
        Self {
            frame_seq,
            clear_rgb,
            clear: RenderDemoPassStatus::Skipped,
            rgb: RenderDemoPassStatus::Skipped,
            texture: RenderDemoPassStatus::Skipped,
            composite: RenderDemoPassStatus::Skipped,
            failure: None,
            elapsed_ms: 0,
            clear_ms: 0,
            rgb_ms: 0,
            composite_ms: 0,
            probe_before: 0,
            probe_after_clear: 0,
            probe_after_rgb: 0,
        }
    }

    fn completed_passes(self) -> u32 {
        let mut completed = 0u32;
        if self.clear.completed() {
            completed = completed.saturating_add(1);
        }
        if self.rgb.completed() {
            completed = completed.saturating_add(1);
        }
        if self.texture.completed() {
            completed = completed.saturating_add(1);
        }
        if self.composite.completed() {
            completed = completed.saturating_add(1);
        }
        completed
    }

    fn finish(mut self, start: Instant) -> Self {
        self.elapsed_ms = start.elapsed().as_millis() as u64;
        self
    }
}

impl RenderDemoOwnedSurface {
    fn rgba_bytes(&self) -> Option<&[u8]> {
        if self.surface_virt.is_null() || self.surface_bytes == 0 {
            return None;
        }
        Some(unsafe {
            core::slice::from_raw_parts(self.surface_virt as *const u8, self.surface_bytes)
        })
    }

    fn rgba_bytes_mut(&mut self) -> Option<&mut [u8]> {
        if self.surface_virt.is_null() || self.surface_bytes == 0 {
            return None;
        }
        Some(unsafe { core::slice::from_raw_parts_mut(self.surface_virt, self.surface_bytes) })
    }
}

impl RenderDemoState {
    fn new(scene_desc: RenderDemoSurfaceDesc) -> Result<Self, RenderDemoFailureKind> {
        Ok(Self {
            scene_surface: Some(create_surface(scene_desc)?),
            texture_surface: None,
            output_surface: None,
            output_shadow_surface: None,
            phase: super::xelp_render_ngin::default_rgb_triangle_rotation(),
            frame_seq: 0,
            stats: RenderDemoStats::new(),
        })
    }

    fn shutdown(&mut self) {
        if let Some(surface) = self.output_shadow_surface.take() {
            destroy_surface(surface);
        }
        if let Some(surface) = self.output_surface.take() {
            destroy_surface(surface);
        }
        if let Some(surface) = self.texture_surface.take() {
            destroy_surface(surface);
        }
        if let Some(surface) = self.scene_surface.take() {
            destroy_surface(surface);
        }
    }
}

impl RenderDemoStats {
    const fn new() -> Self {
        Self {
            frames_started: 0,
            frames_completed: 0,
            frames_failed: 0,
            alloc_failures: 0,
            clear_failures: 0,
            draw_failures: 0,
            texture_failures: 0,
            composite_failures: 0,
            avg_frame_ms: 0,
            avg_completed_passes_milli: 0,
        }
    }

    fn record(&mut self, report: &RenderDemoFrameReport) {
        self.frames_started = self.frames_started.saturating_add(1);
        if let Some(reason) = report.failure {
            self.frames_failed = self.frames_failed.saturating_add(1);
            match reason {
                RenderDemoFailureKind::Alloc => {
                    self.alloc_failures = self.alloc_failures.saturating_add(1)
                }
                RenderDemoFailureKind::Clear => {
                    self.clear_failures = self.clear_failures.saturating_add(1)
                }
                RenderDemoFailureKind::Draw => {
                    self.draw_failures = self.draw_failures.saturating_add(1)
                }
                RenderDemoFailureKind::Texture => {
                    self.texture_failures = self.texture_failures.saturating_add(1)
                }
                RenderDemoFailureKind::Composite => {
                    self.composite_failures = self.composite_failures.saturating_add(1)
                }
            }
        } else {
            self.frames_completed = self.frames_completed.saturating_add(1);
        }

        update_moving_average_u64(&mut self.avg_frame_ms, self.frames_started, report.elapsed_ms);
        update_moving_average_u32(
            &mut self.avg_completed_passes_milli,
            self.frames_started,
            report.completed_passes().saturating_mul(1000),
        );
    }

    fn success_rate_milli(self) -> u32 {
        if self.frames_started == 0 {
            return 0;
        }
        ((self.frames_completed.saturating_mul(1000)) / self.frames_started) as u32
    }
}

#[inline]
pub fn render_demo_mode_active() -> bool {
    INTEL_RENDER_DEMO_ENABLED
        && super::xelp_render_ngin::isolate_rgb_triangle_proof()
        && super::intel::intel_igpu770_present()
        && !INTEL_RENDER_DEMO_DISABLED.load(Ordering::Acquire)
}

#[inline]
pub fn isolated_triangle_mode_active() -> bool {
    render_demo_mode_active()
}

fn disable_render_demo_once(reason: &'static str) {
    INTEL_RENDER_DEMO_DISABLED.store(true, Ordering::Release);
    if !INTEL_RENDER_DEMO_DISABLE_LATCH_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log!("intel/render-demo: disabled reason={} timeout_latched=1\n", reason);
    }
}

fn update_moving_average_u64(current: &mut u64, sample_count: u64, sample: u64) {
    if sample_count <= 1 {
        *current = sample;
    } else {
        *current = current.saturating_mul(7).saturating_add(sample) / 8;
    }
}

fn update_moving_average_u32(current: &mut u32, sample_count: u64, sample: u32) {
    if sample_count <= 1 {
        *current = sample;
    } else {
        *current = current.saturating_mul(7).saturating_add(sample) / 8;
    }
}

fn align_up_u64(value: u64, align: u64) -> Option<u64> {
    if align <= 1 {
        return Some(value);
    }
    let mask = align.checked_sub(1)?;
    value.checked_add(mask).map(|v| v & !mask)
}

fn reserve_surface_gpu_addr(
    desc: RenderDemoSurfaceDesc,
    surface_bytes: usize,
) -> Option<RenderDemoGpuAddrReservation> {
    match desc.gpu_addr_policy {
        RenderDemoSurfaceGpuAddrPolicy::Fixed(gpu_addr) => Some(RenderDemoGpuAddrReservation {
            gpu_addr,
            slot: None,
        }),
        RenderDemoSurfaceGpuAddrPolicy::ReserveNext { base, stride } => {
            let required_span = align_up_u64(surface_bytes as u64, 4096)?;
            if stride < required_span {
                return None;
            }
            let mut slots = INTEL_RENDER_DEMO_GPU_VA_SLOTS.lock();
            let mut slot_idx = 0usize;
            while slot_idx < INTEL_RENDER_DEMO_GPU_VA_SLOT_COUNT {
                let bit = 1u32 << slot_idx;
                if (*slots & bit) == 0 {
                    *slots |= bit;
                    let gpu_addr = base.checked_add(stride.checked_mul(slot_idx as u64)?)?;
                    return Some(RenderDemoGpuAddrReservation {
                        gpu_addr,
                        slot: Some(slot_idx as u8),
                    });
                }
                slot_idx += 1;
            }
            None
        }
    }
}

fn release_surface_gpu_addr(slot: Option<u8>) {
    let Some(slot) = slot else {
        return;
    };
    let mut slots = INTEL_RENDER_DEMO_GPU_VA_SLOTS.lock();
    *slots &= !(1u32 << slot);
}

fn render_demo_clear_rgb(frame_seq: u32) -> u32 {
    if INTEL_RENDER_DEMO_DEBUG_CLEAR_MODE_ENABLED {
        if frame_seq & 1 == 0 {
            INTEL_RENDER_DEMO_DEBUG_CLEAR_RGB_A
        } else {
            INTEL_RENDER_DEMO_DEBUG_CLEAR_RGB_B
        }
    } else {
        INTEL_RENDER_DEMO_CLEAR_RGB
    }
}

#[inline]
fn render_demo_skip_clear_enabled() -> bool {
    INTEL_RENDER_DEMO_BENCHMARK_MODE_ENABLED && INTEL_RENDER_DEMO_BENCHMARK_SKIP_CLEAR
}

fn render_demo_surface_probe(surface: &RenderDemoOwnedSurface) -> u32 {
    let Some(bytes) = surface.rgba_bytes() else {
        return 0;
    };
    if bytes.len() < 4 {
        return 0;
    }

    let width = surface.desc.width as usize;
    let height = surface.desc.height as usize;
    let pitch = surface.pitch_bytes as usize;
    let sample_points = [
        (width / 2, height / 2),
        (width / 3, height / 3),
        ((width * 2) / 3, (height * 2) / 3),
        (width / 4, (height * 3) / 4),
    ];

    let mut acc = 0u32;
    for (idx, (x, y)) in sample_points.into_iter().enumerate() {
        let off = y.saturating_mul(pitch).saturating_add(x.saturating_mul(4));
        if off.saturating_add(4) <= bytes.len() {
            let px =
                u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]);
            acc ^= px.rotate_left((idx as u32) * 7);
        }
    }
    acc
}

fn render_demo_rgb_vertices(phase: f32) -> [RgbVertex; INTEL_RENDER_DEMO_VERTEX_COUNT] {
    const BASE_TRIANGLE: [(f32, f32); 3] = [(0.0, -0.0325), (-0.035, 0.0275), (0.035, 0.0275)];
    const COLORS: [Rgba8; 3] = [
        Rgba8::new(0xFF, 0x52, 0x52, 0xFF),
        Rgba8::new(0x40, 0xE3, 0x92, 0xFF),
        Rgba8::new(0x5A, 0x9C, 0xFF, 0xFF),
    ];
    const CENTERS: [(f32, f32); INTEL_RENDER_DEMO_CLUSTER_COUNT] = [
        (-0.32, 0.00),
        (-0.16, 0.00),
        (0.00, 0.00),
        (0.16, 0.00),
        (0.32, 0.00),
        (-0.48, -0.24),
        (-0.48, 0.00),
        (-0.48, 0.24),
        (0.48, -0.24),
        (0.48, 0.00),
        (0.48, 0.24),
        (-0.70, -0.18),
        (-0.70, 0.18),
        (0.70, -0.18),
        (0.70, 0.18),
    ];

    let frames = phase / INTEL_RENDER_DEMO_PHASE_STEP_RAD.max(0.0001);
    let frame_dt = INTEL_RENDER_DEMO_FRAME_MS as f32 / 1000.0;
    let mut vertices = [RgbVertex {
        x: 0.0,
        y: 0.0,
        color: Rgba8::new(0, 0, 0, 0xFF),
    }; INTEL_RENDER_DEMO_VERTEX_COUNT];

    let mut tri_idx = 0usize;
    while tri_idx < INTEL_RENDER_DEMO_CLUSTER_COUNT {
        let (center_x, center_y) = CENTERS[tri_idx];
        let rpm = 1.0 + (179.0 * tri_idx as f32 / (INTEL_RENDER_DEMO_CLUSTER_COUNT - 1) as f32);
        let radians_per_frame = (rpm * core::f32::consts::TAU / 60.0) * frame_dt;
        let signed_angle = if tri_idx.is_multiple_of(2) {
            frames * radians_per_frame
        } else {
            -(frames * radians_per_frame)
        };
        let cos_p = cosf(signed_angle);
        let sin_p = sinf(signed_angle);

        let mut vtx_idx = 0usize;
        while vtx_idx < 3 {
            let (base_x, base_y) = BASE_TRIANGLE[vtx_idx];
            let rot_x = (base_x * cos_p) - (base_y * sin_p);
            let rot_y = (base_x * sin_p) + (base_y * cos_p);
            vertices[tri_idx * 3 + vtx_idx] = RgbVertex {
                x: center_x + rot_x,
                y: center_y + rot_y,
                color: COLORS[vtx_idx],
            };
            vtx_idx += 1;
        }

        tri_idx += 1;
    }

    vertices
}

fn rgb_vertex_bytes(vertices: &[RgbVertex; INTEL_RENDER_DEMO_VERTEX_COUNT]) -> &[u8] {
    unsafe {
        core::slice::from_raw_parts(
            vertices.as_ptr() as *const u8,
            core::mem::size_of_val(vertices),
        )
    }
}

pub(crate) fn create_surface(
    desc: RenderDemoSurfaceDesc,
) -> Result<RenderDemoOwnedSurface, RenderDemoFailureKind> {
    let Some(pitch_bytes) = desc.default_pitch_bytes() else {
        return Err(RenderDemoFailureKind::Alloc);
    };
    create_surface_with_pitch(desc, pitch_bytes)
}

fn create_surface_with_pitch(
    desc: RenderDemoSurfaceDesc,
    pitch_bytes: u32,
) -> Result<RenderDemoOwnedSurface, RenderDemoFailureKind> {
    let Some(surface_bytes) = desc.surface_bytes_with_pitch(pitch_bytes) else {
        return Err(RenderDemoFailureKind::Alloc);
    };
    let Some(gpu_reservation) = reserve_surface_gpu_addr(desc, surface_bytes) else {
        return Err(RenderDemoFailureKind::Alloc);
    };
    let Some((surface_phys, surface_virt)) = crate::dma::alloc(surface_bytes, desc.align) else {
        release_surface_gpu_addr(gpu_reservation.slot);
        return Err(RenderDemoFailureKind::Alloc);
    };
    unsafe {
        ptr::write_bytes(surface_virt, 0, surface_bytes);
    }

    let surface = RenderDemoOwnedSurface {
        surface_phys,
        surface_virt,
        surface_bytes,
        desc,
        pitch_bytes,
        gpu_addr: gpu_reservation.gpu_addr,
        mapped_gpu_addr: None,
        gpu_va_slot: gpu_reservation.slot,
    };

    crate::log!(
        "intel/render-demo: create-surface size={}x{} pitch=0x{:X} align={} format={} bytes=0x{:X} phys=0x{:X} gpu=0x{:X} gpu_policy={}\n",
        surface.desc.width,
        surface.desc.height,
        surface.pitch_bytes,
        surface.desc.align,
        surface.desc.format.label(),
        surface.surface_bytes,
        surface.surface_phys,
        surface.gpu_addr,
        surface.desc.gpu_addr_policy.label()
    );

    Ok(surface)
}

fn destroy_surface(surface: RenderDemoOwnedSurface) {
    crate::log!(
        "intel/render-demo: destroy-surface size={}x{} pitch=0x{:X} bytes=0x{:X} phys=0x{:X} gpu=0x{:X}\n",
        surface.desc.width,
        surface.desc.height,
        surface.pitch_bytes,
        surface.surface_bytes,
        surface.surface_phys,
        surface.gpu_addr
    );
    release_surface_gpu_addr(surface.gpu_va_slot);
    crate::dma::dealloc(surface.surface_virt, surface.surface_bytes);
}

pub(crate) fn clear_surface(
    surface: &mut RenderDemoOwnedSurface,
    rgb: u32,
) -> Result<(), RenderDemoFailureKind> {
    let Some(target_rgba) = surface.rgba_bytes() else {
        return Err(RenderDemoFailureKind::Alloc);
    };
    if super::intel_igpu770::rcs_clear_rgba_surface(
        target_rgba,
        surface.desc.width,
        surface.desc.height,
        surface.gpu_addr,
        rgb,
    ) {
        Ok(())
    } else {
        Err(RenderDemoFailureKind::Clear)
    }
}

pub(crate) fn draw_rgb_triangles(
    surface: &RenderDemoOwnedSurface,
    vertices: &[u8],
) -> Result<(), RenderDemoFailureKind> {
    let Some(target_rgba) = surface.rgba_bytes() else {
        return Err(RenderDemoFailureKind::Alloc);
    };
    if super::intel_igpu770::rcs_draw_rgba_rgb_triangles(
        target_rgba,
        vertices,
        surface.desc.width,
        surface.desc.height,
        surface.gpu_addr,
        None,
        BlendDesc::disabled(),
    ) {
        Ok(())
    } else {
        Err(RenderDemoFailureKind::Draw)
    }
}

pub(crate) fn draw_tex_triangles(
    surface: &RenderDemoOwnedSurface,
    texture: &RenderDemoOwnedSurface,
    vertices: &[u8],
) -> Result<(), RenderDemoFailureKind> {
    let Some(target_rgba) = surface.rgba_bytes() else {
        return Err(RenderDemoFailureKind::Alloc);
    };
    let Some(texture_rgba) = texture.rgba_bytes() else {
        return Err(RenderDemoFailureKind::Alloc);
    };
    if super::intel_igpu770::rcs_draw_screen_tex_triangles(
        target_rgba,
        texture_rgba,
        texture.desc.width,
        texture.desc.height,
        vertices,
        surface.desc.width,
        surface.desc.height,
        surface.gpu_addr,
        None,
        BlendDesc::disabled(),
        SamplerDesc::default_2d(),
        super::xelp_render_ngin::TextureStoreSampleKind::Rgba,
    ) {
        Ok(())
    } else {
        Err(RenderDemoFailureKind::Texture)
    }
}

fn map_surface_to_gpu(surface: &mut RenderDemoOwnedSurface) -> bool {
    let Some(target_rgba) = surface.rgba_bytes() else {
        return false;
    };
    if super::intel_igpu770::ggtt_map_rgba_surface_pitch(
        target_rgba,
        surface.desc.width,
        surface.desc.height,
        surface.pitch_bytes,
        surface.gpu_addr,
    ) {
        surface.mapped_gpu_addr = Some(surface.gpu_addr);
        true
    } else {
        false
    }
}

fn bridge_scene_to_scanout_surface(
    scene_surface: &RenderDemoOwnedSurface,
    output_surface: &mut RenderDemoOwnedSurface,
    clear_rgb: u32,
    clear_before_copy: bool,
) -> Result<(), RenderDemoFailureKind> {
    if output_surface.mapped_gpu_addr != Some(output_surface.gpu_addr)
        && !map_surface_to_gpu(output_surface)
    {
        return Err(RenderDemoFailureKind::Composite);
    }

    if scene_surface.desc.format != output_surface.desc.format {
        return Err(RenderDemoFailureKind::Composite);
    }
    if !super::intel_igpu770::bcs_composite_rgba_surface(
        scene_surface.gpu_addr,
        scene_surface.desc.width,
        scene_surface.desc.height,
        scene_surface.pitch_bytes,
        output_surface.gpu_addr,
        output_surface.desc.width,
        output_surface.desc.height,
        output_surface.pitch_bytes,
        clear_rgb,
        clear_before_copy,
    ) {
        return Err(RenderDemoFailureKind::Composite);
    }

    if super::primary_present_surface(
        output_surface.gpu_addr,
        output_surface.desc.width,
        output_surface.desc.height,
        output_surface.pitch_bytes,
    ) {
        Ok(())
    } else {
        Err(RenderDemoFailureKind::Composite)
    }
}

pub(crate) fn run_demo_frame(state: &mut RenderDemoState) -> RenderDemoFrameReport {
    let frame_seq = state.frame_seq.wrapping_add(1);
    state.frame_seq = frame_seq;

    let clear_rgb = render_demo_clear_rgb(frame_seq);
    let start = Instant::now();
    if state.scene_surface.is_none() {
        let mut report = RenderDemoFrameReport::new(frame_seq, clear_rgb);
        report.failure = Some(RenderDemoFailureKind::Alloc);
        return report.finish(start);
    }
    let mut report = RenderDemoFrameReport::new(frame_seq, clear_rgb);
    report.probe_before = state
        .scene_surface
        .as_ref()
        .map(render_demo_surface_probe)
        .unwrap_or(0);

    // Fixed pass order keeps later features slotting into passes instead of special cases.
    if render_demo_skip_clear_enabled() {
        report.clear = RenderDemoPassStatus::Skipped;
        report.probe_after_clear = report.probe_before;
    } else {
        let clear_start = Instant::now();
        let Some(scene_surface) = state.scene_surface.as_mut() else {
            report.failure = Some(RenderDemoFailureKind::Alloc);
            return report.finish(start);
        };
        match clear_surface(scene_surface, clear_rgb) {
            Ok(()) => {
                report.clear = RenderDemoPassStatus::Completed;
                report.clear_ms = clear_start.elapsed().as_millis() as u64;
                report.probe_after_clear = render_demo_surface_probe(scene_surface);
            }
            Err(reason) => {
                report.clear = RenderDemoPassStatus::Failed(reason);
                report.clear_ms = clear_start.elapsed().as_millis() as u64;
                report.probe_after_clear = render_demo_surface_probe(scene_surface);
                report.failure = Some(reason);
                return report.finish(start);
            }
        }
    }

    let vertices = render_demo_rgb_vertices(state.phase);
    let rgb_start = Instant::now();
    {
        let Some(scene_surface) = state.scene_surface.as_ref() else {
            report.failure = Some(RenderDemoFailureKind::Alloc);
            return report.finish(start);
        };
        match draw_rgb_triangles(scene_surface, rgb_vertex_bytes(&vertices)) {
            Ok(()) => {
                report.rgb = RenderDemoPassStatus::Completed;
                report.rgb_ms = rgb_start.elapsed().as_millis() as u64;
                report.probe_after_rgb = render_demo_surface_probe(scene_surface);
            }
            Err(reason) => {
                report.rgb = RenderDemoPassStatus::Failed(reason);
                report.rgb_ms = rgb_start.elapsed().as_millis() as u64;
                report.probe_after_rgb = render_demo_surface_probe(scene_surface);
                report.failure = Some(reason);
                return report.finish(start);
            }
        }
    }

    let Some(scene_surface_ref) = state.scene_surface.as_ref() else {
        report.failure = Some(RenderDemoFailureKind::Alloc);
        return report.finish(start);
    };
    let scene_surface = RenderDemoOwnedSurface {
        surface_phys: scene_surface_ref.surface_phys,
        surface_virt: scene_surface_ref.surface_virt,
        surface_bytes: scene_surface_ref.surface_bytes,
        desc: scene_surface_ref.desc,
        pitch_bytes: scene_surface_ref.pitch_bytes,
        gpu_addr: scene_surface_ref.gpu_addr,
        mapped_gpu_addr: scene_surface_ref.mapped_gpu_addr,
        gpu_va_slot: scene_surface_ref.gpu_va_slot,
    };

    report.texture = RenderDemoPassStatus::Skipped;
    match next_output_surface_mut(state) {
        Some(output_surface) => {
            let composite_start = Instant::now();
            match bridge_scene_to_scanout_surface(
                &scene_surface,
                output_surface,
                clear_rgb,
                !render_demo_skip_clear_enabled(),
            ) {
                Ok(()) => {
                    report.composite = RenderDemoPassStatus::Completed;
                    report.composite_ms = composite_start.elapsed().as_millis() as u64;
                }
                Err(reason) => {
                    report.composite = RenderDemoPassStatus::Failed(reason);
                    report.composite_ms = composite_start.elapsed().as_millis() as u64;
                    report.failure = Some(reason);
                    return report.finish(start);
                }
            }
        }
        None => {
            report.composite = RenderDemoPassStatus::Skipped;
        }
    }

    state.phase += INTEL_RENDER_DEMO_PHASE_STEP_RAD;
    if state.phase > core::f32::consts::TAU {
        state.phase -= core::f32::consts::TAU;
    }

    report.finish(start)
}

fn log_task_start(desc: RenderDemoSurfaceDesc) {
    match desc.gpu_addr_policy {
        RenderDemoSurfaceGpuAddrPolicy::Fixed(gpu_addr) => crate::log!(
            "intel/render-demo: task start mode=headless-rcs-dma size={}x{} align={} format={} frame_ms={} gpu_policy={} gpu=0x{:X}\n",
            desc.width,
            desc.height,
            desc.align,
            desc.format.label(),
            INTEL_RENDER_DEMO_FRAME_MS,
            desc.gpu_addr_policy.label(),
            gpu_addr
        ),
        RenderDemoSurfaceGpuAddrPolicy::ReserveNext { base, stride } => crate::log!(
            "intel/render-demo: task start mode=headless-rcs-dma size={}x{} align={} format={} frame_ms={} gpu_policy={} base=0x{:X} stride=0x{:X}\n",
            desc.width,
            desc.height,
            desc.align,
            desc.format.label(),
            INTEL_RENDER_DEMO_FRAME_MS,
            desc.gpu_addr_policy.label(),
            base,
            stride
        ),
    }
}

fn active_display_surface_gpu_addr(state: &RenderDemoState) -> u64 {
    super::primary_present_surface_gpu_addr()
        .or_else(|| state.output_surface.as_ref().map(|surface| surface.gpu_addr))
        .or_else(|| state.output_shadow_surface.as_ref().map(|surface| surface.gpu_addr))
        .unwrap_or(0)
}

fn next_output_surface_mut(state: &mut RenderDemoState) -> Option<&mut RenderDemoOwnedSurface> {
    let target_gpu_addr = super::primary_present_surface_gpu_addr();
    match target_gpu_addr {
        Some(addr) => {
            if matches!(state.output_surface.as_ref(), Some(surface) if surface.gpu_addr == addr) {
                return state.output_surface.as_mut();
            }
            if matches!(
                state.output_shadow_surface.as_ref(),
                Some(surface) if surface.gpu_addr == addr
            ) {
                return state.output_shadow_surface.as_mut();
            }
            state.output_surface
                .as_mut()
                .or(state.output_shadow_surface.as_mut())
        }
        None => state
            .output_surface
            .as_mut()
            .or(state.output_shadow_surface.as_mut()),
    }
}

fn map_scene_surface(surface: &RenderDemoOwnedSurface) -> Result<(), RenderDemoStartupFailure> {
    let mut mapped_surface = RenderDemoOwnedSurface {
        surface_phys: surface.surface_phys,
        surface_virt: surface.surface_virt,
        surface_bytes: surface.surface_bytes,
        desc: surface.desc,
        pitch_bytes: surface.pitch_bytes,
        gpu_addr: surface.gpu_addr,
        mapped_gpu_addr: surface.mapped_gpu_addr,
        gpu_va_slot: surface.gpu_va_slot,
    };
    if map_surface_to_gpu(&mut mapped_surface) {
        crate::log!(
            "intel/render-demo: startup phase=scene-surface-map ok gpu=0x{:X} size={}x{} pitch=0x{:X}\n",
            mapped_surface.gpu_addr,
            mapped_surface.desc.width,
            mapped_surface.desc.height,
            mapped_surface.pitch_bytes
        );
        Ok(())
    } else {
        Err(RenderDemoStartupFailure::SceneMap)
    }
}

async fn try_prepare_scanout_bridge(
    info: super::intel::IntelDeviceInfo,
    state: &mut RenderDemoState,
) {
    if !INTEL_RENDER_DEMO_SCANOUT_BRIDGE_ENABLED {
        crate::log!("intel/render-demo: startup phase=scanout-bridge skipped reason=disabled\n");
        return;
    }
    if !super::intel::ensure_display_kickoff("render-demo-scanout-bridge").await {
        crate::log!(
            "intel/render-demo: startup phase=scanout-bridge skipped reason=display-kickoff-unavailable bdf={:02X}:{:02X}.{}\n",
            info.bus,
            info.slot,
            info.function
        );
        return;
    }

    let Some(draft) = super::xelp_display_ngin::draft_workload(
        super::xelp_display_ngin::DisplayWorkloadKind::PrimaryPresent,
    ) else {
        crate::log!(
            "intel/render-demo: startup phase=scanout-bridge skipped reason=primary-present-draft-unavailable\n"
        );
        return;
    };
    let Some(scene_surface) = state.scene_surface.as_ref() else {
        crate::log!(
            "intel/render-demo: startup phase=scanout-bridge skipped reason=no-scene-surface\n"
        );
        return;
    };
    let output_gpu = draft.surface.windows.staging_gpu_addr;
    let output_shadow_gpu = draft.surface.windows.shadow_state_gpu_addr;
    if output_gpu == 0
        || output_shadow_gpu == 0
        || draft.surface.width == 0
        || draft.surface.height == 0
        || draft.surface.pitch_bytes == 0
    {
        crate::log!(
            "intel/render-demo: startup phase=scanout-bridge skipped reason=invalid-display-surface staging_gpu=0x{:X} shadow_gpu=0x{:X} size={}x{} pitch=0x{:X}\n",
            output_gpu,
            output_shadow_gpu,
            draft.surface.width,
            draft.surface.height,
            draft.surface.pitch_bytes
        );
        return;
    }

    let output_desc = RenderDemoSurfaceDesc {
        width: draft.surface.width,
        height: draft.surface.height,
        align: usize::try_from(draft.surface.alignment_bytes)
            .ok()
            .filter(|align| *align != 0)
            .unwrap_or(scene_surface.desc.align),
        format: scene_surface.desc.format,
        gpu_addr_policy: RenderDemoSurfaceGpuAddrPolicy::Fixed(output_gpu),
    };
    let mut output_surface = match create_surface_with_pitch(output_desc, draft.surface.pitch_bytes)
    {
        Ok(surface) => surface,
        Err(reason) => {
            crate::log!(
                "intel/render-demo: startup phase=scanout-bridge skipped reason={} scene_gpu=0x{:X} staging_gpu=0x{:X}\n",
                reason.label(),
                scene_surface.gpu_addr,
                output_gpu
            );
            return;
        }
    };
    let shadow_desc = RenderDemoSurfaceDesc {
        gpu_addr_policy: RenderDemoSurfaceGpuAddrPolicy::Fixed(output_shadow_gpu),
        ..output_desc
    };
    let mut output_shadow_surface = match create_surface_with_pitch(shadow_desc, draft.surface.pitch_bytes)
    {
        Ok(surface) => surface,
        Err(reason) => {
            crate::log!(
                "intel/render-demo: startup phase=scanout-bridge skipped reason={} scene_gpu=0x{:X} shadow_gpu=0x{:X}\n",
                reason.label(),
                scene_surface.gpu_addr,
                output_shadow_gpu
            );
            destroy_surface(output_surface);
            return;
        }
    };
    if !map_surface_to_gpu(&mut output_surface) {
        crate::log!(
            "intel/render-demo: startup phase=scanout-bridge skipped reason=display-surface-map-failed scene_gpu=0x{:X} staging_gpu=0x{:X}\n",
            scene_surface.gpu_addr,
            output_surface.gpu_addr
        );
        destroy_surface(output_surface);
        return;
    }
    if !map_surface_to_gpu(&mut output_shadow_surface) {
        crate::log!(
            "intel/render-demo: startup phase=scanout-bridge skipped reason=display-surface-map-failed scene_gpu=0x{:X} shadow_gpu=0x{:X}\n",
            scene_surface.gpu_addr,
            output_shadow_surface.gpu_addr
        );
        destroy_surface(output_shadow_surface);
        destroy_surface(output_surface);
        return;
    }
    if draft.descriptor.name == "pipe-a" {
        let _ = super::owned_triangle_disable_non_primary_planes_pipe_a();
    }
    crate::log!(
        "intel/render-demo: startup phase=scanout-bridge armed route=display.present.primary pipe={} scene_gpu=0x{:X} staging_gpu=0x{:X} shadow_gpu=0x{:X} scene_size={}x{} display_size={}x{} display_pitch=0x{:X}\n",
        draft.descriptor.name,
        scene_surface.gpu_addr,
        output_surface.gpu_addr,
        output_shadow_surface.gpu_addr,
        scene_surface.desc.width,
        scene_surface.desc.height,
        output_surface.desc.width,
        output_surface.desc.height,
        output_surface.pitch_bytes
    );
    state.output_surface = Some(output_surface);
    state.output_shadow_surface = Some(output_shadow_surface);
}

async fn run_render_demo_startup(
    info: super::intel::IntelDeviceInfo,
) -> Result<RenderDemoState, RenderDemoStartupFailure> {
    super::intel_igpu770::warm_once(info);
    crate::log!("intel/render-demo: startup phase=warm ok\n");
    super::xelp_render_ngin::log_rgb_triangle_isolation();

    crate::log!("intel/render-demo: startup phase=ggtt-bootstrap-objects begin\n");
    super::intel_igpu770::ggtt_map_smoke_objects_once();
    crate::log!("intel/render-demo: startup phase=ggtt-bootstrap-objects issued\n");

    let Some(warm) = super::warm_state() else {
        return Err(RenderDemoStartupFailure::Guc(
            super::intel_guc::RenderDemoGucStartupFailure::ReadyTimeout,
        ));
    };
    crate::log!("intel/render-demo: startup phase=guc-bootstrap begin\n");
    super::intel_guc::ensure_ready_for_render_demo(
        warm,
        INTEL_RENDER_DEMO_GUC_READY_POLL_MS,
        INTEL_RENDER_DEMO_GUC_READY_TIMEOUT_MS,
    )
    .await
    .map_err(RenderDemoStartupFailure::Guc)?;
    crate::log!("intel/render-demo: startup phase=guc-bootstrap ok\n");
    crate::log!(
        "intel/render-demo: startup phase=guc-ready ok raw=0x{:08X}\n",
        super::intel_guc::status(warm)
    );

    let mut state = RenderDemoState::new(DEFAULT_RENDER_DEMO_SCENE_SURFACE_DESC)
        .map_err(|_| RenderDemoStartupFailure::SurfaceAlloc)?;
    {
        let scene_surface = state
            .scene_surface
            .as_mut()
            .ok_or(RenderDemoStartupFailure::SurfaceAlloc)?;
        crate::log!(
            "intel/render-demo: startup phase=scene-surface-alloc ok phys=0x{:X} gpu=0x{:X} bytes=0x{:X} pitch=0x{:X}\n",
            scene_surface.surface_phys,
            scene_surface.gpu_addr,
            scene_surface.surface_bytes,
            scene_surface.pitch_bytes
        );
        if !map_surface_to_gpu(scene_surface) {
            return Err(RenderDemoStartupFailure::SceneMap);
        }
        crate::log!(
            "intel/render-demo: startup phase=scene-surface-map ok gpu=0x{:X} size={}x{} pitch=0x{:X}\n",
            scene_surface.gpu_addr,
            scene_surface.desc.width,
            scene_surface.desc.height,
            scene_surface.pitch_bytes
        );
    }
    try_prepare_scanout_bridge(info, &mut state).await;
    Ok(state)
}

#[embassy_executor::task]
pub async fn intel_render_demo_task() {
    let Some(info) = super::intel::first_claimed_device() else {
        crate::log!("intel/render-demo: task skipped reason=no-claimed-device\n");
        return;
    };
    if !render_demo_mode_active() {
        crate::log!("intel/render-demo: task skipped reason=mode-inactive\n");
        return;
    }

    log_task_start(DEFAULT_RENDER_DEMO_SCENE_SURFACE_DESC);
    if INTEL_RENDER_DEMO_BOOT_DELAY_MS != 0 {
        crate::log!(
            "intel/render-demo: boot delay={}ms before first frame\n",
            INTEL_RENDER_DEMO_BOOT_DELAY_MS
        );
    }
    Timer::after(EmbassyDuration::from_millis(INTEL_RENDER_DEMO_BOOT_DELAY_MS)).await;

    let mut state = match run_render_demo_startup(info).await {
        Ok(state) => state,
        Err(reason) => {
            disable_render_demo_once(reason.label());
            crate::log!("intel/render-demo: task aborted reason={}\n", reason.label());
            return;
        }
    };

    loop {
        let report = run_demo_frame(&mut state);
        state.stats.record(&report);
        let scene_gpu = state
            .scene_surface
            .as_ref()
            .map(|surface| surface.gpu_addr)
            .unwrap_or(0);
        let display_gpu = active_display_surface_gpu_addr(&state);

        if report.frame_seq <= 8 || report.frame_seq.is_multiple_of(240) || report.failure.is_some()
        {
            crate::log!(
                "intel/render-demo: frame={} clear={} rgb={} texture={} composite={} fail={} clear_rgb=0x{:06X} clear_ms={} rgb_ms={} composite_ms={} elapsed_ms={} probe_before=0x{:08X} probe_after_clear=0x{:08X} probe_after_rgb=0x{:08X} avg_frame_ms={} avg_passes_milli={} success_milli={} scene_gpu=0x{:X} display_gpu=0x{:X} scanout_bridge={} skip_clear={}\n",
                report.frame_seq,
                report.clear.label(),
                report.rgb.label(),
                report.texture.label(),
                report.composite.label(),
                report
                    .failure
                    .map(|reason| reason.label())
                    .unwrap_or("none"),
                report.clear_rgb & 0x00FF_FFFF,
                report.clear_ms,
                report.rgb_ms,
                report.composite_ms,
                report.elapsed_ms,
                report.probe_before,
                report.probe_after_clear,
                report.probe_after_rgb,
                state.stats.avg_frame_ms,
                state.stats.avg_completed_passes_milli,
                state.stats.success_rate_milli(),
                scene_gpu,
                display_gpu,
                (state.output_surface.is_some() || state.output_shadow_surface.is_some()) as u8,
                render_demo_skip_clear_enabled() as u8
            );
        }

        if let Some(reason) = report.failure {
            disable_render_demo_once(reason.label());
            break;
        }

        Timer::after(EmbassyDuration::from_millis(INTEL_RENDER_DEMO_FRAME_MS)).await;
    }

    crate::log!(
        "intel/render-demo: stop frames={} ok={} failed={} alloc={} clear={} draw={} texture={} composite={} avg_frame_ms={} avg_passes_milli={}\n",
        state.stats.frames_started,
        state.stats.frames_completed,
        state.stats.frames_failed,
        state.stats.alloc_failures,
        state.stats.clear_failures,
        state.stats.draw_failures,
        state.stats.texture_failures,
        state.stats.composite_failures,
        state.stats.avg_frame_ms,
        state.stats.avg_completed_passes_milli
    );
    state.shutdown();
}
