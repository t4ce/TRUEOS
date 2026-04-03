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
}

pub(crate) struct RenderDemoOwnedSurface {
    pub surface_phys: u64,
    pub surface_virt: *mut u8,
    pub surface_bytes: usize,
    pub desc: RenderDemoSurfaceDesc,
    pub gpu_addr: u64,
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
    fn surface_bytes(self) -> Option<usize> {
        let pixels = (self.width as usize).checked_mul(self.height as usize)?;
        pixels.checked_mul(self.format.bytes_per_pixel())
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
}

impl RenderDemoState {
    fn new(scene_desc: RenderDemoSurfaceDesc) -> Result<Self, RenderDemoFailureKind> {
        Ok(Self {
            scene_surface: Some(create_surface(scene_desc)?),
            texture_surface: None,
            output_surface: None,
            phase: super::xelp_render_ngin::default_rgb_triangle_rotation(),
            frame_seq: 0,
            stats: RenderDemoStats::new(),
        })
    }

    fn shutdown(&mut self) {
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
    let Some(surface_bytes) = desc.surface_bytes() else {
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
        gpu_addr: gpu_reservation.gpu_addr,
        gpu_va_slot: gpu_reservation.slot,
    };

    crate::log!(
        "intel/render-demo: create-surface size={}x{} align={} format={} bytes=0x{:X} phys=0x{:X} gpu=0x{:X} gpu_policy={}\n",
        surface.desc.width,
        surface.desc.height,
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
        "intel/render-demo: destroy-surface size={}x{} bytes=0x{:X} phys=0x{:X} gpu=0x{:X}\n",
        surface.desc.width,
        surface.desc.height,
        surface.surface_bytes,
        surface.surface_phys,
        surface.gpu_addr
    );
    release_surface_gpu_addr(surface.gpu_va_slot);
    crate::dma::dealloc(surface.surface_virt, surface.surface_bytes);
}

pub(crate) fn clear_surface(
    surface: &RenderDemoOwnedSurface,
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

pub(crate) fn run_demo_frame(state: &mut RenderDemoState) -> RenderDemoFrameReport {
    let frame_seq = state.frame_seq.wrapping_add(1);
    state.frame_seq = frame_seq;

    let clear_rgb = render_demo_clear_rgb(frame_seq);
    let start = Instant::now();
    let Some(scene_surface) = state.scene_surface.as_ref() else {
        let mut report = RenderDemoFrameReport::new(frame_seq, clear_rgb);
        report.failure = Some(RenderDemoFailureKind::Alloc);
        return report.finish(start);
    };
    let mut report = RenderDemoFrameReport::new(frame_seq, clear_rgb);

    // Fixed pass order keeps later features slotting into passes instead of special cases.
    match clear_surface(scene_surface, clear_rgb) {
        Ok(()) => report.clear = RenderDemoPassStatus::Completed,
        Err(reason) => {
            report.clear = RenderDemoPassStatus::Failed(reason);
            report.failure = Some(reason);
            return report.finish(start);
        }
    }

    let vertices = render_demo_rgb_vertices(state.phase);
    match draw_rgb_triangles(scene_surface, rgb_vertex_bytes(&vertices)) {
        Ok(()) => report.rgb = RenderDemoPassStatus::Completed,
        Err(reason) => {
            report.rgb = RenderDemoPassStatus::Failed(reason);
            report.failure = Some(reason);
            return report.finish(start);
        }
    }

    report.texture = RenderDemoPassStatus::Skipped;
    report.composite = RenderDemoPassStatus::Skipped;

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

async fn wait_for_guc_ready() -> bool {
    if super::guc_ready() {
        crate::log!("intel/render-demo: guc-ready wait skipped status=ready\n");
        return true;
    }

    crate::log!(
        "intel/render-demo: waiting for guc-ready timeout_ms={} poll_ms={}\n",
        INTEL_RENDER_DEMO_GUC_READY_TIMEOUT_MS,
        INTEL_RENDER_DEMO_GUC_READY_POLL_MS
    );
    let mut waited_ms = 0u64;
    while waited_ms < INTEL_RENDER_DEMO_GUC_READY_TIMEOUT_MS {
        Timer::after(EmbassyDuration::from_millis(
            INTEL_RENDER_DEMO_GUC_READY_POLL_MS,
        ))
        .await;
        waited_ms = waited_ms.saturating_add(INTEL_RENDER_DEMO_GUC_READY_POLL_MS);
        if super::guc_ready() {
            crate::log!("intel/render-demo: guc-ready after {}ms\n", waited_ms);
            return true;
        }
    }

    crate::log!(
        "intel/render-demo: guc-ready timeout after {}ms\n",
        waited_ms
    );
    false
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

    super::intel_igpu770::warm_once(info);
    super::xelp_render_ngin::log_rgb_triangle_isolation();
    if !wait_for_guc_ready().await {
        disable_render_demo_once("guc-not-ready-timeout");
        crate::log!("intel/render-demo: task aborted reason=guc-not-ready-timeout\n");
        return;
    }

    let mut state = match RenderDemoState::new(DEFAULT_RENDER_DEMO_SCENE_SURFACE_DESC) {
        Ok(state) => state,
        Err(reason) => {
            disable_render_demo_once(reason.label());
            crate::log!(
                "intel/render-demo: task aborted reason={} size={}x{} align={} format={}\n",
                reason.label(),
                DEFAULT_RENDER_DEMO_SCENE_SURFACE_DESC.width,
                DEFAULT_RENDER_DEMO_SCENE_SURFACE_DESC.height,
                DEFAULT_RENDER_DEMO_SCENE_SURFACE_DESC.align,
                DEFAULT_RENDER_DEMO_SCENE_SURFACE_DESC.format.label()
            );
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

        if report.frame_seq <= 8 || report.frame_seq.is_multiple_of(240) || report.failure.is_some()
        {
            crate::log!(
                "intel/render-demo: frame={} clear={} rgb={} texture={} composite={} fail={} clear_rgb=0x{:06X} elapsed_ms={} avg_frame_ms={} avg_passes_milli={} success_milli={} scene_gpu=0x{:X}\n",
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
                report.elapsed_ms,
                state.stats.avg_frame_ms,
                state.stats.avg_completed_passes_milli,
                state.stats.success_rate_milli(),
                scene_gpu
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
