//! Production UI3 logical-frame compositor.
//!
//! This module is deliberately separate from the experimental `ui3_frame`
//! CABI. A producer owns only a registry-backed, double-buffered logical
//! RGBA8-premultiplied surface. UI3 is the sole owner of the full-screen
//! display back buffer and the only layer allowed to commit it to scanout.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::intel::gpgpu::{
    AlphaBlendWorklistRgba8Desc, COMPOSITE_WORKLIST_FLAG_PREMUL_SRC,
    COMPOSITE_WORKLIST_FLAG_SRC_OVER, COMPOSITE_WORKLIST_FLAG_TINT_ALPHA,
    COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA, FillRectWorklistRgba8Desc, GpgpuPoint, GpgpuRect,
    GpgpuRgba8Surface, GpgpuSpriteQuadWorklistDesc,
};

pub(crate) const UI3_OUTPUT_COUNT: usize = 4;
const UI3_SURFACE_COUNT: usize = 3;
const UI3_SURFACE_BUFFER_COUNT: usize = 2;
const UI3_SURFACE_GPU_BASE: u64 = 0x2900_0000;
const UI3_SURFACE_GPU_STRIDE: u64 = 0x0100_0000;
const UI3_SCALE_SCRATCH_GPU: u64 = 0x2F00_0000;
const UI3_COMPOSITOR_GPU_LIMIT: u64 = 0x3000_0000;
const UI3_BLACK_BOX_FRAME_SIZE: u32 = 512;
const UI3_BLACK_BOX_SIZE: u32 = 160;
const UI3_BLACK_BOX_OFFSET: u32 = (UI3_BLACK_BOX_FRAME_SIZE.saturating_sub(UI3_BLACK_BOX_SIZE)) / 2;
const UI3_WINDOW_COUNT: usize = 3;
const UI3_OUTPUT_BUFFER_COUNT: usize = 2;
const UI3_WINDOW_FRAME_INTERVAL_MS: u64 = 16;
const UI3_WINDOW_BORDER: u32 = 3;
const UI3_WINDOW_TITLE_HEIGHT: u32 = 28;
const UI3_WINDOW_CLOSE_SIZE: u32 = 20;
const UI3_WINDOW_CLOSE_MARGIN: u32 = 5;
const UI3_WINDOW_CLOSE_COLOR: u32 = 0xFF46_46F0;
const UI3_RECT_WORKLIST_LANES: usize = 16;

const _: () = assert!(
    UI3_SURFACE_GPU_BASE
        + (UI3_SURFACE_COUNT * UI3_SURFACE_BUFFER_COUNT) as u64 * UI3_SURFACE_GPU_STRIDE
        <= UI3_SCALE_SCRATCH_GPU
);
const _: () = assert!(UI3_SCALE_SCRATCH_GPU + UI3_SURFACE_GPU_STRIDE <= UI3_COMPOSITOR_GPU_LIMIT);
const _: () = assert!(UI3_COMPOSITOR_GPU_LIMIT <= 0x3000_0000);

static STATE: Mutex<CompositorState> = Mutex::new(CompositorState::new());
static COMPOSE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub(crate) struct SurfaceId(u64);

impl SurfaceId {
    const DRAW3D_SCENE: Self = Self(1);
    const COMPOSITOR_PROOF: Self = Self(2);
    const SHELL_CHART: Self = Self(3);

    const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui3FrameFormat {
    /// Bytes are R, G, B, A and RGB is already multiplied by A.
    Rgba8Premultiplied,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum SurfaceProducer {
    Draw3d,
    Ui3Compositor,
    Shell2Gpgpu,
}

impl SurfaceProducer {
    const fn name(self) -> &'static str {
        match self {
            Self::Draw3d => "draw3d",
            Self::Ui3Compositor => "ui3-compositor",
            Self::Shell2Gpgpu => "shell2-gpgpu-chart",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub(crate) struct Ui3OutputId(u8);

impl Ui3OutputId {
    pub(crate) const PRIMARY: Self = Self(0);

    pub(crate) const fn from_slot(slot: usize) -> Option<Self> {
        if slot < UI3_OUTPUT_COUNT {
            Some(Self(slot as u8))
        } else {
            None
        }
    }

    pub(crate) const fn slot(self) -> usize {
        self.0 as usize
    }

    pub(crate) const fn name(self) -> &'static str {
        match self.0 {
            0 => "D01",
            1 => "D02",
            2 => "D03",
            3 => "D04",
            _ => "D-invalid",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui3FramePlacement {
    pub(crate) output: Ui3OutputId,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) opacity: u8,
    pub(crate) z: i16,
    pub(crate) visible: bool,
}

impl Ui3FramePlacement {
    const fn inactive(output: Ui3OutputId, z: i16) -> Self {
        Self {
            output,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            opacity: 255,
            z,
            visible: false,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui3FrameResizeEvent {
    pub(crate) surface_id: SurfaceId,
    pub(crate) label: &'static str,
    pub(crate) output: Ui3OutputId,
    pub(crate) output_sequence: u64,
    pub(crate) old_width: u32,
    pub(crate) old_height: u32,
    pub(crate) new_width: u32,
    pub(crate) new_height: u32,
    /// Dimensions of the producer buffer used for the frame that just
    /// presented. A consumer can deliberately no-op when these already match
    /// the requested client size.
    pub(crate) presented_buffer_width: u32,
    pub(crate) presented_buffer_height: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui3FrameResizeFallback {
    /// Preserve source pixels 1:1 at the client area's top-left and clip
    /// anything outside the new bounds.
    ClipVisible,
    /// Aspect-fit the last producer buffer and fill the uncovered client area
    /// with opaque black until a correctly sized frame arrives.
    Letterbox,
}

#[derive(Copy, Clone)]
pub(crate) enum Ui3FrameResizeContract {
    /// The producer explicitly accepts its selected compatibility fallback
    /// and does not need a post-present resize notification.
    Noop { fallback: Ui3FrameResizeFallback },
    /// Invoked after a complete output frame containing the resized surface
    /// has presented. The callback must decide whether new producer work is
    /// useful; UI3 never regenerates client content on its behalf.
    AfterPresented {
        fallback: Ui3FrameResizeFallback,
        callback: fn(Ui3FrameResizeEvent),
    },
}

impl Ui3FrameResizeContract {
    const fn fallback(self) -> Ui3FrameResizeFallback {
        match self {
            Self::Noop { fallback } | Self::AfterPresented { fallback, .. } => fallback,
        }
    }
}

#[derive(Copy, Clone)]
struct SurfaceStorage {
    surface: GpgpuRgba8Surface,
    virt: *mut u8,
}

unsafe impl Send for SurfaceStorage {}
unsafe impl Sync for SurfaceStorage {}

struct SurfaceState {
    id: SurfaceId,
    label: &'static str,
    producer: SurfaceProducer,
    format: Ui3FrameFormat,
    width: u32,
    height: u32,
    gpu_slot_base: usize,
    buffers: [Option<SurfaceStorage>; UI3_SURFACE_BUFFER_COUNT],
    front: Option<usize>,
    front_generation: u64,
    acquired: Option<(usize, u64)>,
    next_generation: u64,
    pending_present_generation: Option<u64>,
    placement: Ui3FramePlacement,
    resize_contract: Ui3FrameResizeContract,
    last_presented_size: Option<(u32, u32)>,
}

impl SurfaceState {
    const fn new(
        id: SurfaceId,
        label: &'static str,
        producer: SurfaceProducer,
        gpu_slot_base: usize,
        placement: Ui3FramePlacement,
        resize_contract: Ui3FrameResizeContract,
    ) -> Self {
        Self {
            id,
            label,
            producer,
            format: Ui3FrameFormat::Rgba8Premultiplied,
            width: 0,
            height: 0,
            gpu_slot_base,
            buffers: [None; UI3_SURFACE_BUFFER_COUNT],
            front: None,
            front_generation: 0,
            acquired: None,
            next_generation: 0,
            pending_present_generation: None,
            placement,
            resize_contract,
            last_presented_size: None,
        }
    }
}

struct SurfaceRegistry {
    surfaces: [SurfaceState; UI3_SURFACE_COUNT],
}

impl SurfaceRegistry {
    const fn new() -> Self {
        Self {
            surfaces: [
                SurfaceState::new(
                    SurfaceId::DRAW3D_SCENE,
                    "draw3d-scene",
                    SurfaceProducer::Draw3d,
                    0,
                    Ui3FramePlacement::inactive(Ui3OutputId::PRIMARY, 0),
                    Ui3FrameResizeContract::Noop {
                        fallback: Ui3FrameResizeFallback::Letterbox,
                    },
                ),
                SurfaceState::new(
                    SurfaceId::COMPOSITOR_PROOF,
                    "ui3-proof-512",
                    SurfaceProducer::Ui3Compositor,
                    UI3_SURFACE_BUFFER_COUNT,
                    Ui3FramePlacement {
                        output: Ui3OutputId::PRIMARY,
                        x: 64,
                        y: 64,
                        width: UI3_BLACK_BOX_FRAME_SIZE,
                        height: UI3_BLACK_BOX_FRAME_SIZE,
                        opacity: 255,
                        z: 100,
                        visible: true,
                    },
                    Ui3FrameResizeContract::Noop {
                        fallback: Ui3FrameResizeFallback::ClipVisible,
                    },
                ),
                SurfaceState::new(
                    SurfaceId::SHELL_CHART,
                    "shell2-gpgpu-chart",
                    SurfaceProducer::Shell2Gpgpu,
                    UI3_SURFACE_BUFFER_COUNT * 2,
                    Ui3FramePlacement::inactive(Ui3OutputId::PRIMARY, 200),
                    Ui3FrameResizeContract::AfterPresented {
                        fallback: Ui3FrameResizeFallback::Letterbox,
                        callback: crate::intel::gpgpu::shell_chart_ui3_resize_after_presented,
                    },
                ),
            ],
        }
    }

    fn get(&self, id: SurfaceId) -> Option<&SurfaceState> {
        self.surfaces.iter().find(|surface| surface.id == id)
    }

    fn get_mut(&mut self, id: SurfaceId) -> Option<&mut SurfaceState> {
        self.surfaces.iter_mut().find(|surface| surface.id == id)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ManagedWindow {
    id: u8,
    label: &'static str,
    client_surface: Option<SurfaceId>,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    z: u16,
    visible: bool,
    title_color: u32,
    border_color: u32,
}

impl ManagedWindow {
    const fn new(
        id: u8,
        label: &'static str,
        client_surface: Option<SurfaceId>,
        z: u16,
        title_color: u32,
        border_color: u32,
    ) -> Self {
        Self {
            id,
            label,
            client_surface,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            z,
            visible: true,
            title_color,
            border_color,
        }
    }

    fn contains(self, x: u32, y: u32) -> bool {
        point_in_rect(x, y, self.x, self.y, self.width, self.height)
    }

    fn title_contains(self, x: u32, y: u32) -> bool {
        point_in_rect(x, y, self.x, self.y, self.width, self.height.min(UI3_WINDOW_TITLE_HEIGHT))
    }

    fn close_rect(self) -> (i32, i32, u32, u32) {
        let size = UI3_WINDOW_CLOSE_SIZE
            .min(
                self.width
                    .saturating_sub(UI3_WINDOW_CLOSE_MARGIN.saturating_mul(2)),
            )
            .min(self.height.min(UI3_WINDOW_TITLE_HEIGHT));
        let x = self.x.saturating_add(
            i32::try_from(self.width.saturating_sub(UI3_WINDOW_CLOSE_MARGIN + size))
                .unwrap_or(i32::MAX),
        );
        let y = self.y.saturating_add(
            i32::try_from(
                self.height
                    .min(UI3_WINDOW_TITLE_HEIGHT)
                    .saturating_sub(size)
                    / 2,
            )
            .unwrap_or(0),
        );
        (x, y, size, size)
    }

    fn close_contains(self, x: u32, y: u32) -> bool {
        let (close_x, close_y, close_width, close_height) = self.close_rect();
        point_in_rect(x, y, close_x, close_y, close_width, close_height)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct WindowDrag {
    window_index: usize,
    cursor_slot: u32,
    grab_x: i32,
    grab_y: i32,
    pending_x: i32,
    pending_y: i32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum WindowInputEvent {
    None,
    DragStarted {
        id: u8,
        label: &'static str,
        x: i32,
        y: i32,
    },
    DragFinished {
        id: u8,
        label: &'static str,
        x: i32,
        y: i32,
    },
    Closed { id: u8, label: &'static str },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct WindowInputUpdate {
    changed: bool,
    event: WindowInputEvent,
}

impl WindowInputUpdate {
    const NONE: Self = Self {
        changed: false,
        event: WindowInputEvent::None,
    };
}

#[derive(Copy, Clone)]
struct WindowSceneSnapshot {
    windows: [ManagedWindow; UI3_WINDOW_COUNT],
    revision: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct PresentedLayerSnapshot {
    surface_id: SurfaceId,
    generation: u64,
    source_width: u32,
    source_height: u32,
    placement: Ui3FramePlacement,
    resize_fallback: Ui3FrameResizeFallback,
}

#[derive(Copy, Clone)]
struct PresentedOutputScene {
    width: u32,
    height: u32,
    windows: Option<[ManagedWindow; UI3_WINDOW_COUNT]>,
    layers: [Option<PresentedLayerSnapshot>; UI3_SURFACE_COUNT],
}

#[derive(Copy, Clone, Debug, Default)]
struct WindowCompositionStats {
    windows: usize,
    paint_ops: usize,
    fill_descs: usize,
    submits: usize,
    proof_pixels: u64,
    proof_mismatches: u64,
}

#[derive(Copy, Clone, Debug)]
struct WindowChromeProof {
    pixels: u64,
    mismatches: u64,
    first_x: i32,
    first_y: i32,
    first_expected: u32,
    first_actual: u32,
}

impl WindowChromeProof {
    const fn exact(self) -> bool {
        self.mismatches == 0
    }
}

#[derive(Copy, Clone)]
struct ResizeNotification {
    callback: fn(Ui3FrameResizeEvent),
    event: Ui3FrameResizeEvent,
}

struct WindowManager {
    windows: [ManagedWindow; UI3_WINDOW_COUNT],
    drag: Option<WindowDrag>,
    last_buttons_down: u32,
    last_cursor_slot: u32,
    output_width: u32,
    output_height: u32,
    revision: u64,
    presented_revision: u64,
    initialized: bool,
}

impl WindowManager {
    const fn new() -> Self {
        Self {
            windows: [
                ManagedWindow::new(
                    1,
                    "gpgpu-chart",
                    Some(SurfaceId::SHELL_CHART),
                    1,
                    0xFFEB_6325,
                    0xFFEB_6325,
                ),
                ManagedWindow::new(2, "frame-two", None, 2, 0xFF81_9428, 0xFF81_9428),
                ManagedWindow::new(3, "frame-three", None, 3, 0xFF75_3FA7, 0xFF75_3FA7),
            ],
            drag: None,
            last_buttons_down: 0,
            last_cursor_slot: 0,
            output_width: 0,
            output_height: 0,
            revision: 0,
            presented_revision: 0,
            initialized: false,
        }
    }

    fn ensure_layout(&mut self, width: u32, height: u32) {
        if self.initialized && self.output_width == width && self.output_height == height {
            return;
        }
        let max_width = width.saturating_sub(16).max(1);
        let max_height = height.saturating_sub(16).max(1);
        if !self.initialized {
            let base_width = width
                .saturating_mul(2)
                .checked_div(5)
                .unwrap_or(0)
                .max(240)
                .min(520)
                .min(max_width);
            let base_height = height
                .saturating_mul(2)
                .checked_div(5)
                .unwrap_or(0)
                .max(160)
                .min(340)
                .min(max_height);
            self.windows[0].width = base_width;
            self.windows[0].height = base_height;
            self.windows[0].x = i32::try_from(width / 16).unwrap_or(0);
            self.windows[0].y = i32::try_from(height / 10).unwrap_or(0);

            self.windows[1].width = base_width.saturating_add(48).min(max_width);
            self.windows[1].height = base_height.saturating_add(32).min(max_height);
            self.windows[1].x = i32::try_from(width / 3).unwrap_or(0);
            self.windows[1].y = i32::try_from(height / 3).unwrap_or(0);

            self.windows[2].width = base_width.saturating_sub(40).max(1).min(max_width);
            self.windows[2].height = base_height.saturating_sub(24).max(1).min(max_height);
            self.windows[2].x = i32::try_from(width.saturating_mul(7) / 12).unwrap_or(0);
            self.windows[2].y = i32::try_from(height / 6).unwrap_or(0);
            self.initialized = true;
        }
        self.output_width = width;
        self.output_height = height;
        for window in &mut self.windows {
            window.width = window.width.min(max_width);
            window.height = window.height.min(max_height);
            clamp_window_to_output(window, width, height);
        }
        self.touch();
    }

    fn update_pointer(
        &mut self,
        cursor: Option<crate::ui3::ui3_hid::Ui3CursorSnapshot>,
        width: u32,
        height: u32,
    ) -> WindowInputUpdate {
        self.ensure_layout(width, height);
        let Some(cursor) = cursor else {
            let mut update = WindowInputUpdate::NONE;
            if let Some(drag) = self.drag.take() {
                let window = &mut self.windows[drag.window_index];
                let moved = (window.x, window.y) != (drag.pending_x, drag.pending_y);
                window.x = drag.pending_x;
                window.y = drag.pending_y;
                let (id, label, x, y) = (window.id, window.label, window.x, window.y);
                if moved {
                    self.touch();
                }
                update = WindowInputUpdate {
                    changed: moved,
                    event: WindowInputEvent::DragFinished { id, label, x, y },
                };
            }
            self.last_buttons_down = 0;
            self.last_cursor_slot = 0;
            return update;
        };

        let left_down = (cursor.buttons_down & crate::ui3::ui3_hid::UI3_CURSOR_BUTTON_LEFT) != 0;
        let left_was_down = (self.last_buttons_down & crate::ui3::ui3_hid::UI3_CURSOR_BUTTON_LEFT)
            != 0
            && self.last_cursor_slot == cursor.slot_id;
        let mut update = WindowInputUpdate::NONE;

        if left_down && !left_was_down {
            if let Some(index) = self.top_window_at(cursor.x_px, cursor.y_px) {
                if self.windows[index].close_contains(cursor.x_px, cursor.y_px) {
                    self.windows[index].visible = false;
                    self.drag = None;
                    self.touch();
                    update.changed = true;
                    update.event = WindowInputEvent::Closed {
                        id: self.windows[index].id,
                        label: self.windows[index].label,
                    };
                } else {
                    if self.raise(index) {
                        update.changed = true;
                    }
                    if self.windows[index].title_contains(cursor.x_px, cursor.y_px) {
                        let cursor_x = i32::try_from(cursor.x_px).unwrap_or(i32::MAX);
                        let cursor_y = i32::try_from(cursor.y_px).unwrap_or(i32::MAX);
                        self.drag = Some(WindowDrag {
                            window_index: index,
                            cursor_slot: cursor.slot_id,
                            grab_x: cursor_x.saturating_sub(self.windows[index].x),
                            grab_y: cursor_y.saturating_sub(self.windows[index].y),
                            pending_x: self.windows[index].x,
                            pending_y: self.windows[index].y,
                        });
                        update.event = WindowInputEvent::DragStarted {
                            id: self.windows[index].id,
                            label: self.windows[index].label,
                            x: self.windows[index].x,
                            y: self.windows[index].y,
                        };
                    }
                }
            }
        }

        if left_down {
            if let Some(drag) = self.drag.filter(|drag| drag.cursor_slot == cursor.slot_id) {
                let cursor_x = i32::try_from(cursor.x_px).unwrap_or(i32::MAX);
                let cursor_y = i32::try_from(cursor.y_px).unwrap_or(i32::MAX);
                let mut pending = self.windows[drag.window_index];
                pending.x = cursor_x.saturating_sub(drag.grab_x);
                pending.y = cursor_y.saturating_sub(drag.grab_y);
                clamp_window_to_output(&mut pending, width, height);
                self.drag = Some(WindowDrag {
                    pending_x: pending.x,
                    pending_y: pending.y,
                    ..drag
                });
            }
        } else if let Some(drag) = self.drag.take() {
            let window = &mut self.windows[drag.window_index];
            let moved = (window.x, window.y) != (drag.pending_x, drag.pending_y);
            window.x = drag.pending_x;
            window.y = drag.pending_y;
            let (id, label, x, y) = (window.id, window.label, window.x, window.y);
            if moved {
                self.touch();
            }
            update.changed |= moved;
            update.event = WindowInputEvent::DragFinished {
                id,
                label,
                x,
                y,
            };
        }

        self.last_buttons_down = cursor.buttons_down;
        self.last_cursor_slot = cursor.slot_id;
        update
    }

    fn top_window_at(&self, x: u32, y: u32) -> Option<usize> {
        let mut top: Option<usize> = None;
        for (index, window) in self.windows.iter().copied().enumerate() {
            if !window.visible || !window.contains(x, y) {
                continue;
            }
            if top
                .map(|top_index| window.z > self.windows[top_index].z)
                .unwrap_or(true)
            {
                top = Some(index);
            }
        }
        top
    }

    fn raise(&mut self, index: usize) -> bool {
        let Some(top_index) = self
            .windows
            .iter()
            .enumerate()
            .max_by_key(|(_, window)| (window.z, window.id))
            .map(|(index, _)| index)
        else {
            return false;
        };
        if index == top_index {
            return false;
        }
        let mut order = [0usize, 1, 2];
        order.sort_by_key(|other| (self.windows[*other].z, self.windows[*other].id));
        let mut z = 1u16;
        for other in order {
            if other == index {
                continue;
            }
            self.windows[other].z = z;
            z = z.saturating_add(1);
        }
        self.windows[index].z = z;
        self.touch();
        true
    }

    fn activate_client_surface(&mut self, surface_id: SurfaceId) -> Result<bool, &'static str> {
        let index = self
            .windows
            .iter()
            .position(|window| window.client_surface == Some(surface_id))
            .ok_or("ui3-window-client-unregistered")?;
        let mut changed = false;
        if !self.windows[index].visible {
            self.windows[index].visible = true;
            self.touch();
            changed = true;
        }
        Ok(self.raise(index) || changed)
    }

    fn snapshot(&self) -> WindowSceneSnapshot {
        WindowSceneSnapshot {
            windows: self.windows,
            revision: self.revision,
        }
    }

    fn needs_present(&self) -> bool {
        self.presented_revision < self.revision
    }

    fn mark_presented(&mut self, revision: u64) {
        self.presented_revision = self.presented_revision.max(revision.min(self.revision));
    }

    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1).max(1);
    }
}

fn clamp_window_to_output(window: &mut ManagedWindow, width: u32, height: u32) {
    let max_x = i32::try_from(width.saturating_sub(window.width)).unwrap_or(i32::MAX);
    let max_y = i32::try_from(height.saturating_sub(window.height)).unwrap_or(i32::MAX);
    window.x = window.x.clamp(0, max_x.max(0));
    window.y = window.y.clamp(0, max_y.max(0));
}

fn point_in_rect(x: u32, y: u32, rect_x: i32, rect_y: i32, width: u32, height: u32) -> bool {
    let x = i64::from(x);
    let y = i64::from(y);
    let left = i64::from(rect_x);
    let top = i64::from(rect_y);
    x >= left
        && y >= top
        && x < left.saturating_add(i64::from(width))
        && y < top.saturating_add(i64::from(height))
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct OutputFrameRecord {
    sequence: u64,
    layer_count: usize,
    pipeline_name: &'static str,
    width: u32,
    height: u32,
}

struct OutputState {
    id: Ui3OutputId,
    next_sequence: u64,
    in_flight: Option<OutputFrameRecord>,
    last_complete: Option<OutputFrameRecord>,
    last_layer_count: Option<usize>,
}

impl OutputState {
    const fn new(id: Ui3OutputId) -> Self {
        Self {
            id,
            next_sequence: 0,
            in_flight: None,
            last_complete: None,
            last_layer_count: None,
        }
    }

    fn begin_frame(
        &mut self,
        layer_count: usize,
        pipeline_name: &'static str,
        width: u32,
        height: u32,
    ) -> Result<(OutputFrameRecord, bool), &'static str> {
        if self.in_flight.is_some() {
            return Err("ui3-output-frame-in-flight");
        }
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);
        let frame = OutputFrameRecord {
            sequence: self.next_sequence,
            layer_count,
            pipeline_name,
            width,
            height,
        };
        let layer_count_changed = self.last_layer_count != Some(layer_count);
        self.last_layer_count = Some(layer_count);
        self.in_flight = Some(frame);
        Ok((frame, layer_count_changed))
    }

    fn complete_frame(
        &mut self,
        frame: OutputFrameRecord,
        presented: bool,
    ) -> Option<OutputFrameRecord> {
        if self.in_flight != Some(frame) {
            self.in_flight = None;
            return self.last_complete;
        }
        self.in_flight = None;
        if presented {
            self.last_complete = Some(frame);
        }
        self.last_complete
    }
}

struct CompositorState {
    surfaces: SurfaceRegistry,
    outputs: [OutputState; UI3_OUTPUT_COUNT],
    window_manager: WindowManager,
    proof_initializing: bool,
    scale_scratch: Option<SurfaceStorage>,
    presented_buffers:
        [[Option<PresentedOutputScene>; UI3_OUTPUT_BUFFER_COUNT]; UI3_OUTPUT_COUNT],
}

impl CompositorState {
    const fn new() -> Self {
        Self {
            surfaces: SurfaceRegistry::new(),
            outputs: [
                OutputState::new(Ui3OutputId::from_slot(0).unwrap()),
                OutputState::new(Ui3OutputId::from_slot(1).unwrap()),
                OutputState::new(Ui3OutputId::from_slot(2).unwrap()),
                OutputState::new(Ui3OutputId::from_slot(3).unwrap()),
            ],
            window_manager: WindowManager::new(),
            proof_initializing: false,
            scale_scratch: None,
            presented_buffers: [[None; UI3_OUTPUT_BUFFER_COUNT]; UI3_OUTPUT_COUNT],
        }
    }

    fn sync_window_surface_placements(&mut self) {
        let windows = self.window_manager.windows;
        for window in windows {
            let Some(surface_id) = window.client_surface else {
                continue;
            };
            let Some(surface) = self.surfaces.get_mut(surface_id) else {
                continue;
            };
            surface.placement = window_client_placement(window, surface.placement.z);
            if !window.visible {
                surface.pending_present_generation = None;
            }
        }
    }

    fn collect_presented_resize_notifications(
        &mut self,
        output: Ui3OutputId,
        output_sequence: u64,
    ) -> Vec<ResizeNotification> {
        let mut notifications = Vec::new();
        for surface in &mut self.surfaces.surfaces {
            let Some(front) = surface.front else {
                continue;
            };
            if !surface.placement.visible
                || surface.placement.opacity == 0
                || surface.placement.output != output
                || surface.placement.width == 0
                || surface.placement.height == 0
            {
                continue;
            }
            let Some(storage) = surface.buffers[front] else {
                continue;
            };
            let new_size = (surface.placement.width, surface.placement.height);
            let old_size = surface.last_presented_size.replace(new_size);
            let buffer_matches_request =
                storage.surface.width == new_size.0 && storage.surface.height == new_size.1;
            let (old_width, old_height) = old_size.unwrap_or(new_size);
            if (old_width, old_height) == new_size && buffer_matches_request {
                continue;
            }
            let Ui3FrameResizeContract::AfterPresented { callback, .. } = surface.resize_contract
            else {
                continue;
            };
            notifications.push(ResizeNotification {
                callback,
                event: Ui3FrameResizeEvent {
                    surface_id: surface.id,
                    label: surface.label,
                    output,
                    output_sequence,
                    old_width,
                    old_height,
                    new_width: new_size.0,
                    new_height: new_size.1,
                    presented_buffer_width: storage.surface.width,
                    presented_buffer_height: storage.surface.height,
                },
            });
        }
        notifications
    }

    fn mark_layer_generations_presented(&mut self, layers: &[LayerSnapshot]) {
        for layer in layers {
            let Some(surface) = self.surfaces.get_mut(layer.surface_id) else {
                continue;
            };
            if surface
                .pending_present_generation
                .is_some_and(|pending| layer.generation >= pending)
            {
                surface.pending_present_generation = None;
            }
        }
    }
}

fn window_client_placement(window: ManagedWindow, z: i16) -> Ui3FramePlacement {
    let title_height = window.height.min(UI3_WINDOW_TITLE_HEIGHT);
    let border = UI3_WINDOW_BORDER
        .min(window.width / 2)
        .min(window.height.saturating_sub(title_height));
    Ui3FramePlacement {
        output: Ui3OutputId::PRIMARY,
        x: window
            .x
            .saturating_add(i32::try_from(border).unwrap_or(i32::MAX)),
        y: window
            .y
            .saturating_add(i32::try_from(title_height).unwrap_or(i32::MAX)),
        width: window.width.saturating_sub(border.saturating_mul(2)),
        height: window
            .height
            .saturating_sub(title_height)
            .saturating_sub(border),
        opacity: u8::MAX,
        z,
        visible: window.visible,
    }
}

/// A non-copyable producer lease for one logical frame back buffer.
pub(crate) struct Ui3FrameWriteLease {
    surface_id: SurfaceId,
    buffer: usize,
    generation: u64,
    surface: GpgpuRgba8Surface,
    diagnostic_virt: *mut u8,
    format: Ui3FrameFormat,
    active: bool,
}

impl Ui3FrameWriteLease {
    pub(crate) const fn surface(&self) -> GpgpuRgba8Surface {
        self.surface
    }

    pub(crate) const fn format(&self) -> Ui3FrameFormat {
        self.format
    }

    /// Read-only CPU alias for producer retirement diagnostics. Producers must
    /// never use this as a render/upload path; the owned surface remains GPU-only.
    pub(crate) const fn diagnostic_virt(&self) -> *mut u8 {
        self.diagnostic_virt
    }
}

impl Drop for Ui3FrameWriteLease {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut state = STATE.lock();
        let Some(surface) = state.surfaces.get_mut(self.surface_id) else {
            return;
        };
        if surface.acquired == Some((self.buffer, self.generation)) {
            surface.acquired = None;
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct Ui3CompositionResult {
    pub(crate) presented: bool,
    pub(crate) output: Ui3OutputId,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) layers: usize,
    pub(crate) clear_us: u64,
    pub(crate) draw3d_us: u64,
    pub(crate) proof_us: u64,
    pub(crate) commit_us: u64,
    pub(crate) total_us: u64,
}

#[derive(Copy, Clone)]
struct LayerSnapshot {
    surface_id: SurfaceId,
    label: &'static str,
    generation: u64,
    surface: GpgpuRgba8Surface,
    placement: Ui3FramePlacement,
    resize_fallback: Ui3FrameResizeFallback,
}

#[derive(Copy, Clone, Debug, Default)]
struct LayerCompositionTiming {
    scratch_clear_us: u64,
    scale_us: u64,
    blend_us: u64,
}

#[derive(Copy, Clone, Debug)]
struct ProofSourceReadback {
    exact: bool,
    mismatches: u64,
    first_x: i32,
    first_y: i32,
    first_expected: u32,
    first_actual: u32,
    expected_black_pixels: u64,
    actual_black_pixels: u64,
    transparent: u32,
    box_first: u32,
    box_center: u32,
    box_last: u32,
    far_transparent: u32,
}

impl LayerCompositionTiming {
    const fn total_us(self) -> u64 {
        self.scratch_clear_us
            .saturating_add(self.scale_us)
            .saturating_add(self.blend_us)
    }
}

struct ComposeGuard;

impl ComposeGuard {
    fn acquire() -> Result<Self, &'static str> {
        COMPOSE_IN_FLIGHT
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| "ui3-compose-in-flight")
    }
}

impl Drop for ComposeGuard {
    fn drop(&mut self) {
        COMPOSE_IN_FLIGHT.store(false, Ordering::Release);
    }
}

/// Bring the production UI3 compositor online independently of every frame
/// producer. The worklist proof remains off-screen. Both D01 swapchain
/// buffers are initialized and presented once: the first commit arms the
/// transparent upper plane, and the second proves its steady surface-only
/// flip. Thereafter the compositor remains static until scene damage.
pub(crate) fn bootstrap_primary_output() -> Result<Ui3CompositionResult, &'static str> {
    ensure_compositor_proof()?;
    let first = compose_output(Ui3OutputId::PRIMARY)?;
    if !first.presented {
        return Err("ui3-bootstrap-first-present");
    }
    let steady = compose_output(Ui3OutputId::PRIMARY)?;
    if !steady.presented {
        return Err("ui3-bootstrap-steady-present");
    }
    Ok(steady)
}

/// Independent UI3 service and minimal window-manager loop. The task is gated
/// by the Intel presentation readiness bit, runs on AP1, and retries transient
/// route/GPU-busy failures without involving TCP or a scene command.
#[embassy_executor::task]
pub(crate) async fn ui3_compositor_task() {
    const RETRY_MS: u64 = 250;
    let upper_plane_stage = crate::intel::ui3_upper_plane_boot_stage();
    if upper_plane_stage == 1 || upper_plane_stage == 2 {
        crate::log_info!(
            target: "ui3";
            "ui3-compositor: production present held upper_plane_stage={} reason=hardware-gated-slot1-proof primary_logo_retained=1 action=idle-until-next-boot-stage\n",
            upper_plane_stage,
        );
        loop {
            Timer::after(EmbassyDuration::from_secs(1)).await;
        }
    }
    let mut attempt = 0u64;
    let initial = loop {
        attempt = attempt.saturating_add(1);
        match bootstrap_primary_output() {
            Ok(result) => {
                crate::log_info!(
                    target: "ui3";
                    "ui3-compositor: bootstrap complete=1 attempt={} output={} size={}x{} layers={} presented=2 swapchain=initialized-both-buffers steady_flip=surface-only static_until_damage=1 producer_dependency=none tcp_dependency=none milestone=static-frame-wm windows={} input=kernel-cursor-snapshot\n",
                    attempt,
                    result.output.name(),
                    result.width,
                    result.height,
                    result.layers,
                    UI3_WINDOW_COUNT,
                );
                break result;
            }
            Err(error) => {
                if attempt == 1 || attempt.is_multiple_of(20) {
                    crate::log_warn!(
                        target: "ui3";
                        "ui3-compositor: bootstrap complete=0 attempt={} potential_reason={} action=retry retry_ms={} producer_dependency=none tcp_dependency=none\n",
                        attempt,
                        error,
                        RETRY_MS,
                    );
                }
            }
        }
        Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
    };

    let mut width = initial.width;
    let mut height = initial.height;
    let mut compose_error_count = 0u64;
    loop {
        if let Some(target) =
            crate::intel::ui3_compositor_output_target(Ui3OutputId::PRIMARY.slot())
        {
            width = target.width;
            height = target.height;
        }
        let cursor = crate::ui3::ui3_hid::preferred_cursor_snapshot(width, height);
        let update = {
            let mut state = STATE.lock();
            let update = state.window_manager.update_pointer(cursor, width, height);
            state.sync_window_surface_placements();
            update
        };
        match update.event {
            WindowInputEvent::None => {}
            WindowInputEvent::DragStarted { id, label, x, y } => crate::log_info!(
                target: "ui3";
                "ui3-wm: drag start window={} label={} output={} pos={}x{} pointer=primary-button action=track-only retain-last-complete-frame=1\n",
                id,
                label,
                Ui3OutputId::PRIMARY.name(),
                x,
                y,
            ),
            WindowInputEvent::DragFinished { id, label, x, y } => crate::log_info!(
                target: "ui3";
                "ui3-wm: drag finish window={} label={} output={} pos={}x{} pointer=primary-button action=atomic-move-on-drop\n",
                id,
                label,
                Ui3OutputId::PRIMARY.name(),
                x,
                y,
            ),
            WindowInputEvent::Closed { id, label } => crate::log_info!(
                target: "ui3";
                "ui3-wm: close window={} label={} output={} action=hide\n",
                id,
                label,
                Ui3OutputId::PRIMARY.name(),
            ),
        }
        let needs_present = STATE.lock().window_manager.needs_present();
        if update.changed || needs_present {
            match compose_output(Ui3OutputId::PRIMARY) {
                Ok(result) if result.presented => {
                    compose_error_count = 0;
                }
                Ok(_) => {
                    compose_error_count = compose_error_count.saturating_add(1);
                }
                Err(error) => {
                    compose_error_count = compose_error_count.saturating_add(1);
                    if compose_error_count == 1 || compose_error_count.is_multiple_of(60) {
                        crate::log_warn!(
                            target: "ui3";
                            "ui3-wm: present deferred potential_reason={} retry_ms={} state_retained=1\n",
                            error,
                            UI3_WINDOW_FRAME_INTERVAL_MS,
                        );
                    }
                }
            }
        }
        Timer::after(EmbassyDuration::from_millis(UI3_WINDOW_FRAME_INTERVAL_MS)).await;
    }
}

/// Acquire the current D01-sized Draw3D logical back buffer.
pub(crate) fn acquire_draw3d_scene_frame() -> Result<Ui3FrameWriteLease, &'static str> {
    let output = crate::intel::ui3_compositor_output_target(Ui3OutputId::PRIMARY.slot())
        .ok_or("ui3-primary-output-unavailable")?;
    let mut state = STATE.lock();
    let surface = state
        .surfaces
        .get_mut(SurfaceId::DRAW3D_SCENE)
        .ok_or("ui3-scene-surface-unregistered")?;
    ensure_surface_buffers(surface, output.width, output.height)?;
    if surface.acquired.is_some() {
        return Err("ui3-scene-frame-already-acquired");
    }
    if surface.placement.width == 0 || surface.placement.height == 0 {
        surface.placement.x = 0;
        surface.placement.y = 0;
        surface.placement.width = output.width;
        surface.placement.height = output.height;
        surface.placement.output = Ui3OutputId::PRIMARY;
        surface.placement.opacity = 255;
        surface.placement.z = 0;
    }
    let buffer = match surface.front {
        Some(front) => (front + 1) % UI3_SURFACE_BUFFER_COUNT,
        None => 0,
    };
    let storage = surface.buffers[buffer].ok_or("ui3-scene-frame-buffer-unavailable")?;
    surface.next_generation = surface.next_generation.wrapping_add(1).max(1);
    let generation = surface.next_generation;
    surface.acquired = Some((buffer, generation));
    Ok(Ui3FrameWriteLease {
        surface_id: SurfaceId::DRAW3D_SCENE,
        buffer,
        generation,
        surface: storage.surface,
        diagnostic_virt: storage.virt,
        format: surface.format,
        active: true,
    })
}

/// Make the chart client visible for a new shell command. Closing the frame
/// suppresses it for the rest of that command; invoking `gpgpu chart` again
/// explicitly reactivates and raises it.
pub(crate) fn activate_shell_chart_window() -> Result<(), &'static str> {
    let output = crate::intel::ui3_compositor_output_target(Ui3OutputId::PRIMARY.slot())
        .ok_or("ui3-primary-output-unavailable")?;
    let mut state = STATE.lock();
    state
        .window_manager
        .ensure_layout(output.width, output.height);
    let _ = state
        .window_manager
        .activate_client_surface(SurfaceId::SHELL_CHART)?;
    state.sync_window_surface_placements();
    Ok(())
}

/// Acquire the chart window's logical client back buffer. The chart kernel
/// owns the complete opaque client area; the compositor owns its frame and
/// final D01 placement.
pub(crate) fn acquire_shell_chart_frame() -> Result<Ui3FrameWriteLease, &'static str> {
    let output = crate::intel::ui3_compositor_output_target(Ui3OutputId::PRIMARY.slot())
        .ok_or("ui3-primary-output-unavailable")?;
    let mut state = STATE.lock();
    state
        .window_manager
        .ensure_layout(output.width, output.height);
    state.sync_window_surface_placements();
    let surface = state
        .surfaces
        .get_mut(SurfaceId::SHELL_CHART)
        .ok_or("ui3-chart-surface-unregistered")?;
    if !surface.placement.visible {
        return Err("ui3-chart-window-hidden");
    }
    let width = surface.placement.width;
    let height = surface.placement.height;
    if width == 0 || height == 0 {
        return Err("ui3-chart-frame-empty");
    }
    if surface.buffers.iter().all(Option::is_none) {
        ensure_surface_buffers(surface, width, height)?;
    }
    if surface.acquired.is_some() {
        return Err("ui3-chart-frame-already-acquired");
    }
    let buffer = match surface.front {
        Some(front) => (front + 1) % UI3_SURFACE_BUFFER_COUNT,
        None => 0,
    };
    let storage = surface.buffers[buffer].ok_or("ui3-chart-frame-buffer-unavailable")?;
    surface.next_generation = surface.next_generation.wrapping_add(1).max(1);
    let generation = surface.next_generation;
    surface.acquired = Some((buffer, generation));
    Ok(Ui3FrameWriteLease {
        surface_id: SurfaceId::SHELL_CHART,
        buffer,
        generation,
        surface: storage.surface,
        diagnostic_virt: storage.virt,
        format: surface.format,
        active: true,
    })
}

/// Publish a retired chart kernel write. `request_present=false` advances only
/// the logical front. `request_present=true` marks D01 dirty for the persistent
/// compositor task; a producer never races UI3 for synchronous output ownership.
pub(crate) fn publish_shell_chart_frame(
    mut lease: Ui3FrameWriteLease,
    request_present: bool,
) -> Result<bool, &'static str> {
    if lease.surface_id != SurfaceId::SHELL_CHART {
        return Err("ui3-chart-frame-lease-kind");
    }
    {
        let mut state = STATE.lock();
        let surface = state
            .surfaces
            .get_mut(lease.surface_id)
            .ok_or("ui3-chart-surface-unregistered")?;
        if surface.acquired != Some((lease.buffer, lease.generation)) {
            return Err("ui3-chart-frame-stale-lease");
        }
        surface.front = Some(lease.buffer);
        surface.front_generation = lease.generation;
        surface.acquired = None;
        if request_present {
            surface.pending_present_generation = Some(lease.generation);
            state.window_manager.touch();
            state.sync_window_surface_placements();
        }
    }
    lease.active = false;
    Ok(request_present)
}

pub(crate) fn discard_shell_chart_frame(mut lease: Ui3FrameWriteLease) -> bool {
    if lease.surface_id != SurfaceId::SHELL_CHART {
        lease.active = false;
        return false;
    }
    let discarded = {
        let mut state = STATE.lock();
        let Some(surface) = state.surfaces.get_mut(lease.surface_id) else {
            lease.active = false;
            return false;
        };
        if surface.acquired == Some((lease.buffer, lease.generation)) {
            surface.acquired = None;
            true
        } else {
            false
        }
    };
    lease.active = false;
    discarded
}

/// Schedule one compositor retry after a resize callback regenerated chart
/// content. This does not compose recursively inside the post-present
/// callback; the persistent UI3 task consumes the dirty revision next tick.
pub(crate) fn request_shell_chart_recompose() -> bool {
    let mut state = STATE.lock();
    state.sync_window_surface_placements();
    let ready = state
        .surfaces
        .get(SurfaceId::SHELL_CHART)
        .is_some_and(|surface| surface.front.is_some() && surface.placement.visible);
    if ready {
        state.window_manager.touch();
    }
    ready
}

/// Authorize replacement of the chart's producer buffers only from its
/// post-present resize callback. Ordinary producer acquires keep using the
/// last accepted size so UI3 can clip or letterbox a stable front first.
pub(crate) fn resize_shell_chart_frame_buffers_after_presented(
    width: u32,
    height: u32,
) -> Result<(), &'static str> {
    let mut state = STATE.lock();
    state.sync_window_surface_placements();
    let surface = state
        .surfaces
        .get_mut(SurfaceId::SHELL_CHART)
        .ok_or("ui3-chart-surface-unregistered")?;
    if !surface.placement.visible {
        return Err("ui3-chart-window-hidden");
    }
    if (surface.placement.width, surface.placement.height) != (width, height) {
        return Err("ui3-chart-resize-event-stale");
    }
    ensure_surface_buffers(surface, width, height)
}

pub(crate) fn shell_chart_window_visible() -> bool {
    let state = STATE.lock();
    state
        .surfaces
        .get(SurfaceId::SHELL_CHART)
        .is_some_and(|surface| surface.placement.visible)
}

pub(crate) fn shell_chart_present_pending() -> bool {
    STATE
        .lock()
        .surfaces
        .get(SurfaceId::SHELL_CHART)
        .is_some_and(|surface| surface.pending_present_generation.is_some())
}

/// Publish a GPU-retired Draw3D source frame, then ask UI3 to build and
/// present one complete output frame.  Publishing and presentation are
/// separate facts: a failed output composition retains both the new logical
/// front and the display's previous complete front for a later retry.
pub(crate) fn commit_draw3d_scene_frame(
    mut lease: Ui3FrameWriteLease,
) -> Result<Ui3CompositionResult, &'static str> {
    if lease.surface_id != SurfaceId::DRAW3D_SCENE {
        return Err("ui3-scene-frame-lease-kind");
    }
    let output = {
        let mut state = STATE.lock();
        let surface = state
            .surfaces
            .get_mut(lease.surface_id)
            .ok_or("ui3-scene-surface-unregistered")?;
        if surface.acquired != Some((lease.buffer, lease.generation)) {
            return Err("ui3-scene-frame-stale-lease");
        }
        surface.front = Some(lease.buffer);
        surface.front_generation = lease.generation;
        surface.acquired = None;
        surface.placement.visible = true;
        surface.placement.output
    };
    lease.active = false;
    ensure_compositor_proof()?;
    compose_output(output)
}

pub(crate) fn discard_draw3d_scene_frame(mut lease: Ui3FrameWriteLease) -> bool {
    let discarded = {
        let mut state = STATE.lock();
        let Some(surface) = state.surfaces.get_mut(lease.surface_id) else {
            lease.active = false;
            return false;
        };
        if surface.acquired == Some((lease.buffer, lease.generation)) {
            surface.acquired = None;
            true
        } else {
            false
        }
    };
    lease.active = false;
    discarded
}

/// Permanent Draw3D reset removes only the scene layer. UI3 and its off-screen
/// kernel proof remain alive because display ownership belongs to the
/// compositor, not to the scene service.
pub(crate) fn reset_draw3d_scene_frame() -> bool {
    let output = {
        let mut state = STATE.lock();
        let Some(surface) = state.surfaces.get_mut(SurfaceId::DRAW3D_SCENE) else {
            return false;
        };
        surface.front = None;
        surface.front_generation = 0;
        surface.pending_present_generation = None;
        surface.acquired = None;
        surface.next_generation = surface.next_generation.wrapping_add(1);
        surface.placement.visible = false;
        surface.placement.output
    };
    match ensure_compositor_proof().and_then(|_| compose_output(output)) {
        Ok(result) => result.presented,
        Err(error) => {
            crate::log_warn!(
                target: "ui3";
                "ui3-compositor: scene reset retained previous output potential_reason={} action=retry-on-next-frame\n",
                error,
            );
            false
        }
    }
}

/// Reconfigure any production UI3 layer.  Moving between outputs recomposes
/// the old output first and the new output second; each output independently
/// retains its last complete front if the route changes mid-transaction.
#[allow(dead_code)]
pub(crate) fn configure_frame(
    id: SurfaceId,
    placement: Ui3FramePlacement,
) -> Result<bool, &'static str> {
    if placement.width == 0 || placement.height == 0 {
        return Err("ui3-frame-placement-empty");
    }
    if placement.output.slot() >= UI3_OUTPUT_COUNT {
        return Err("ui3-frame-output-range");
    }
    let (old_output, has_front) = {
        let mut state = STATE.lock();
        let surface = state
            .surfaces
            .get_mut(id)
            .ok_or("ui3-surface-unregistered")?;
        let old_output = surface.placement.output;
        surface.placement = placement;
        (old_output, surface.front.is_some())
    };
    if !has_front {
        return Ok(true);
    }
    ensure_compositor_proof()?;
    let old_ok = compose_output(old_output)
        .map(|result| result.presented)
        .unwrap_or(false);
    if old_output == placement.output {
        return Ok(old_ok);
    }
    Ok(old_ok && compose_output(placement.output)?.presented)
}

fn ensure_compositor_proof() -> Result<(), &'static str> {
    let (surface, virt) = {
        let mut state = STATE.lock();
        if state
            .surfaces
            .get(SurfaceId::COMPOSITOR_PROOF)
            .ok_or("ui3-proof-surface-unregistered")?
            .front
            .is_some()
        {
            return Ok(());
        }
        if state.proof_initializing {
            return Err("ui3-proof-initializing");
        }
        let surface = state
            .surfaces
            .get_mut(SurfaceId::COMPOSITOR_PROOF)
            .ok_or("ui3-proof-surface-unregistered")?;
        ensure_surface_buffers(surface, UI3_BLACK_BOX_FRAME_SIZE, UI3_BLACK_BOX_FRAME_SIZE)?;
        let storage = surface.buffers[0].ok_or("ui3-proof-buffer-unavailable")?;
        state.proof_initializing = true;
        (storage.surface, storage.virt)
    };

    // Two ordered GPU submissions are intentional. Overlapping fill
    // descriptors in one worklist can race between workgroups.
    let proof_started_ns = crate::chronos::monotonic_nanos();
    let clear_ok = parallel_fill_rect(
        surface,
        GpgpuRect::new(0, 0, surface.width, surface.height),
        0x0000_0000,
    );
    let clear_complete_ns = crate::chronos::monotonic_nanos();
    let box_ok = clear_ok
        && parallel_fill_rect(
            surface,
            GpgpuRect::new(
                UI3_BLACK_BOX_OFFSET as i32,
                UI3_BLACK_BOX_OFFSET as i32,
                UI3_BLACK_BOX_SIZE,
                UI3_BLACK_BOX_SIZE,
            ),
            0xFF00_0000,
        );
    let box_complete_ns = crate::chronos::monotonic_nanos();
    let source_proof = box_ok.then(|| verify_proof_source(surface, virt));

    let mut state = STATE.lock();
    state.proof_initializing = false;
    if !box_ok {
        return Err("ui3-proof-gpu-fill");
    }
    let source_proof = source_proof.ok_or("ui3-proof-source-readback")?;
    crate::log_info!(
        target: "ui3";
        "ui3-compositor: proof source exact={} mismatches={} first={}x{} first_expected=0x{:08X} first_actual=0x{:08X} expected_black_pixels={} actual_black_pixels={} transparent=0x{:08X} box_first=0x{:08X} box_center=0x{:08X} box_last=0x{:08X} far_transparent=0x{:08X} expected_transparent=0x00000000 expected_box=0xFF000000 clear_us={} box_us={} verify_us={} verification=dense presentation=offscreen-only cpu_readback=diagnostic-only cpu_flush=diagnostic-only cpu_pixel_path=none\n",
        source_proof.exact as u8,
        source_proof.mismatches,
        source_proof.first_x,
        source_proof.first_y,
        source_proof.first_expected,
        source_proof.first_actual,
        source_proof.expected_black_pixels,
        source_proof.actual_black_pixels,
        source_proof.transparent,
        source_proof.box_first,
        source_proof.box_center,
        source_proof.box_last,
        source_proof.far_transparent,
        elapsed_us(proof_started_ns, clear_complete_ns),
        elapsed_us(clear_complete_ns, box_complete_ns),
        elapsed_us(box_complete_ns, crate::chronos::monotonic_nanos()),
    );
    if !source_proof.exact {
        return Err("ui3-proof-source-mismatch");
    }
    let proof_surface = state
        .surfaces
        .get_mut(SurfaceId::COMPOSITOR_PROOF)
        .ok_or("ui3-proof-surface-unregistered")?;
    proof_surface.front = Some(0);
    proof_surface.front_generation = 1;
    proof_surface.placement.visible = false;
    crate::log_info!(
        target: "ui3";
        "ui3-compositor: surface online surface_id={} label={} format=rgba8-premultiplied content={}x{} output={} dst={}x{}+{}+{} z={} content_proof=opaque-black-box-{}x{} producer={} production=gpu-only presentation=offscreen-only\n",
        proof_surface.id.raw(),
        proof_surface.label,
        surface.width,
        surface.height,
        proof_surface.placement.output.name(),
        proof_surface.placement.width,
        proof_surface.placement.height,
        proof_surface.placement.x,
        proof_surface.placement.y,
        proof_surface.placement.z,
        UI3_BLACK_BOX_SIZE,
        UI3_BLACK_BOX_SIZE,
        proof_surface.producer.name(),
    );
    Ok(())
}

fn verify_proof_source(surface: GpgpuRgba8Surface, virt: *mut u8) -> ProofSourceReadback {
    verify_binary_proof_surface(
        surface,
        virt,
        UI3_BLACK_BOX_OFFSET,
        UI3_BLACK_BOX_OFFSET,
        UI3_BLACK_BOX_SIZE,
        UI3_BLACK_BOX_SIZE,
    )
}

fn verify_binary_proof_surface(
    surface: GpgpuRgba8Surface,
    virt: *mut u8,
    black_x: u32,
    black_y: u32,
    black_width: u32,
    black_height: u32,
) -> ProofSourceReadback {
    const INVALID: u32 = 0xDEAD_BEEF;
    const TRANSPARENT: u32 = 0x0000_0000;
    const OPAQUE_BLACK: u32 = 0xFF00_0000;
    let black_right = black_x.saturating_add(black_width);
    let black_bottom = black_y.saturating_add(black_height);
    let shape_valid = !virt.is_null()
        && surface.width != 0
        && surface.height != 0
        && black_right <= surface.width
        && black_bottom <= surface.height
        && surface.pitch_bytes as usize >= surface.width as usize * 4
        && surface.bytes >= surface.pitch_bytes as usize * surface.height as usize;
    if !shape_valid {
        return ProofSourceReadback {
            exact: false,
            mismatches: 1,
            first_x: -1,
            first_y: -1,
            first_expected: TRANSPARENT,
            first_actual: INVALID,
            expected_black_pixels: u64::from(black_width) * u64::from(black_height),
            actual_black_pixels: 0,
            transparent: INVALID,
            box_first: INVALID,
            box_center: INVALID,
            box_last: INVALID,
            far_transparent: INVALID,
        };
    }

    // The producer has retired. Invalidate the complete diagnostic view once
    // so every pixel, rather than five favorable samples, participates in the
    // proof. This is never part of ordinary frame composition.
    crate::intel::dma_flush(virt, surface.bytes);
    let pitch = surface.pitch_bytes as usize;
    let mut mismatches = 0u64;
    let mut first_x = -1i32;
    let mut first_y = -1i32;
    let mut first_expected = 0u32;
    let mut first_actual = 0u32;
    let mut actual_black_pixels = 0u64;
    let read = |x: u32, y: u32| -> u32 {
        let offset = y as usize * pitch + x as usize * 4;
        unsafe { core::ptr::read_volatile(virt.add(offset).cast::<u32>()) }
    };
    for y in 0..surface.height {
        for x in 0..surface.width {
            let expected = if x >= black_x && x < black_right && y >= black_y && y < black_bottom {
                OPAQUE_BLACK
            } else {
                TRANSPARENT
            };
            let actual = read(x, y);
            actual_black_pixels =
                actual_black_pixels.saturating_add((actual == OPAQUE_BLACK) as u64);
            if actual != expected {
                if mismatches == 0 {
                    first_x = x as i32;
                    first_y = y as i32;
                    first_expected = expected;
                    first_actual = actual;
                }
                mismatches = mismatches.saturating_add(1);
            }
        }
    }
    let box_last_x = black_right - 1;
    let box_last_y = black_bottom - 1;
    ProofSourceReadback {
        exact: mismatches == 0,
        mismatches,
        first_x,
        first_y,
        first_expected,
        first_actual,
        expected_black_pixels: u64::from(black_width) * u64::from(black_height),
        actual_black_pixels,
        transparent: read(0, 0),
        box_first: read(black_x, black_y),
        box_center: read(black_x + black_width / 2, black_y + black_height / 2),
        box_last: read(box_last_x, box_last_y),
        far_transparent: read(surface.width - 1, surface.height - 1),
    }
}

const fn elapsed_us(start_ns: u64, end_ns: u64) -> u64 {
    end_ns.saturating_sub(start_ns) / 1_000
}

fn compose_output(output: Ui3OutputId) -> Result<Ui3CompositionResult, &'static str> {
    let compose_started_ns = crate::chronos::monotonic_nanos();
    let _guard = ComposeGuard::acquire()?;
    let target = crate::intel::ui3_compositor_output_target(output.slot())
        .ok_or("ui3-output-route-unavailable")?;
    let window_scene = if output == Ui3OutputId::PRIMARY {
        let mut state = STATE.lock();
        state
            .window_manager
            .ensure_layout(target.width, target.height);
        state.sync_window_surface_placements();
        Some(state.window_manager.snapshot())
    } else {
        None
    };
    let mut layers = snapshot_layers(output);
    layers.sort_by_key(|layer| (layer.placement.z, layer.surface_id));
    let current_scene =
        PresentedOutputScene::capture(target.width, target.height, window_scene, &layers);

    let output_frame =
        crate::intel::ui3_compositor_acquire_output(target).ok_or("ui3-output-acquire")?;
    let acquire_complete_ns = crate::chronos::monotonic_nanos();
    let dst = output_frame.surface;
    let output_buffer_index = output_frame.buffer_index;
    if dst.width != target.width || dst.height != target.height {
        let _ = crate::intel::ui3_compositor_discard_output(output_frame);
        return Err("ui3-output-shape-changed");
    }
    if output_buffer_index >= UI3_OUTPUT_BUFFER_COUNT {
        let _ = crate::intel::ui3_compositor_discard_output(output_frame);
        return Err("ui3-output-buffer-index");
    }
    // The scene record belongs to this exact physical back buffer. Taking it
    // before rendering deliberately invalidates the record on every failure;
    // only a successful commit installs a new trustworthy baseline.
    let previous_scene = STATE.lock().presented_buffers[output.slot()]
        [output_buffer_index]
        .take();
    // A window move changes transparent coverage as well as painted chrome.
    // Until the compositor carries an explicit multi-rectangle damage history
    // for each swapchain image, rebuild the complete upper-plane image for
    // chrome/layout changes. This is GPU-only and occurs only on interaction;
    // a static desktop still performs no work at all. Client-only updates keep
    // their per-buffer bounded damage path below.
    let window_scene_changed = previous_scene
        .map(|previous| previous.windows != current_scene.windows)
        .unwrap_or(true);
    let damage = if window_scene_changed {
        DamageRect::full(target.width, target.height)
    } else {
        output_scene_damage(previous_scene, current_scene)
            .unwrap_or_else(|| DamageRect::full(target.width, target.height))
    };
    let damage_rect = damage.as_gpgpu_rect().ok_or("ui3-output-damage-shape")?;
    let full_redraw = damage.left == 0
        && damage.top == 0
        && damage.right == i64::from(target.width)
        && damage.bottom == i64::from(target.height);
    let opaque_damage_overwrite =
        opaque_chart_fully_overwrites_damage(previous_scene, current_scene, damage);

    let upper_plane = crate::intel::ui3_upper_plane_boot_stage() == 3;
    // Stage 3 is a true upper plane: untouched desktop pixels must be
    // transparent so the retained logo remains visible. The legacy
    // single-primary fallback still requires an opaque completed desktop.
    let clear_color = if upper_plane { 0x0000_0000 } else { 0xFF00_0000 };
    let clear_ok = opaque_damage_overwrite || parallel_fill_rect(dst, damage_rect, clear_color);
    let clear_complete_ns = crate::chronos::monotonic_nanos();
    if !clear_ok {
        let _ = crate::intel::ui3_compositor_discard_output(output_frame);
        return Err("ui3-output-gpu-clear");
    }

    let mut draw3d_us = 0u64;
    let mut proof_us = 0u64;
    let mut chart_us = 0u64;
    let mut scratch_clear_us = 0u64;
    let mut scale_us = 0u64;
    let mut blend_us = 0u64;
    for layer in &layers {
        let composed = match compose_layer_damage(*layer, dst, damage) {
            Ok(composed) => composed,
            Err(error) => {
                crate::log_warn!(
                    target: "ui3";
                    "ui3-compositor: layer rejected output={} surface_id={} label={} potential_reason={} action=discard-output-frame\n",
                    output.name(),
                    layer.surface_id.raw(),
                    layer.label,
                    error,
                );
                let _ = crate::intel::ui3_compositor_discard_output(output_frame);
                return Err(error);
            }
        };
        scratch_clear_us = scratch_clear_us.saturating_add(composed.scratch_clear_us);
        scale_us = scale_us.saturating_add(composed.scale_us);
        blend_us = blend_us.saturating_add(composed.blend_us);
        match layer.surface_id {
            SurfaceId::DRAW3D_SCENE => {
                draw3d_us = draw3d_us.saturating_add(composed.total_us());
            }
            SurfaceId::COMPOSITOR_PROOF => {
                proof_us = proof_us.saturating_add(composed.total_us());
            }
            SurfaceId::SHELL_CHART => {
                chart_us = chart_us.saturating_add(composed.total_us());
            }
            _ => {}
        }
    }

    let layers_complete_ns = crate::chronos::monotonic_nanos();
    let window_started_ns = crate::chronos::monotonic_nanos();
    let window_stats = match window_scene {
        Some(scene) => match compose_window_scene(
            scene,
            dst,
            output_frame.diagnostic_virt,
            output_buffer_index,
            damage,
        ) {
            Ok(stats) => stats,
            Err(error) => {
                let _ = crate::intel::ui3_compositor_discard_output(output_frame);
                return Err(error);
            }
        },
        None => WindowCompositionStats::default(),
    };
    let window_us = elapsed_us(window_started_ns, crate::chronos::monotonic_nanos());
    let scene_item_count = layers.len().saturating_add(window_stats.windows);

    let (logical_frame, layer_count_changed) = {
        let mut state = STATE.lock();
        let Some(output_state) = state.outputs.get_mut(output.slot()) else {
            let _ = crate::intel::ui3_compositor_discard_output(output_frame);
            return Err("ui3-output-state-range");
        };
        if output_state.id != output {
            let _ = crate::intel::ui3_compositor_discard_output(output_frame);
            return Err("ui3-output-state-identity");
        }
        match output_state.begin_frame(
            scene_item_count,
            target.pipeline_name,
            target.width,
            target.height,
        ) {
            Ok(frame) => frame,
            Err(error) => {
                let _ = crate::intel::ui3_compositor_discard_output(output_frame);
                return Err(error);
            }
        }
    };
    let commit_started_ns = crate::chronos::monotonic_nanos();
    let presented = crate::intel::ui3_compositor_commit_output(output_frame, "ui3-compositor");
    let commit_complete_ns = crate::chronos::monotonic_nanos();
    let (last_complete, resize_notifications) = {
        let mut state = STATE.lock();
        let complete = state.outputs[output.slot()].complete_frame(logical_frame, presented);
        if presented {
            state.presented_buffers[output.slot()][output_buffer_index] = Some(current_scene);
            state.mark_layer_generations_presented(&layers);
            if let Some(scene) = window_scene {
                state.window_manager.mark_presented(scene.revision);
            }
        }
        let notifications = if presented {
            state.collect_presented_resize_notifications(output, logical_frame.sequence)
        } else {
            Vec::new()
        };
        (complete, notifications)
    };
    for notification in resize_notifications {
        (notification.callback)(notification.event);
    }
    let acquire_us = elapsed_us(compose_started_ns, acquire_complete_ns);
    let clear_us = elapsed_us(acquire_complete_ns, clear_complete_ns);
    let layers_us = elapsed_us(clear_complete_ns, layers_complete_ns);
    let commit_us = elapsed_us(commit_started_ns, commit_complete_ns);
    let total_us = elapsed_us(compose_started_ns, commit_complete_ns);
    if logical_frame.sequence <= 8
        || logical_frame.sequence.is_multiple_of(60)
        || layer_count_changed
        || !presented
    {
        crate::log_info!(
            target: "ui3";
            "ui3-compositor: output frame seq={} output={} backend_output={} pipeline={} size={}x{} buffer={} layers={} windows={} last_complete_seq={} redraw={} damage={}x{}+{}+{} clear={} composition=gpu-premul-worklist wm=gpu-rect-worklist scanout={} wm_paints={} wm_fill_descs={} wm_submits={} wm_proof_pixels={} wm_proof_mismatches={} present={} acquire_us={} clear_us={} layers_us={} draw3d_us={} proof_us={} chart_us={} scratch_clear_us={} scale_us={} blend_us={} wm_us={} commit_us={} total_us={} budget_us=16667 over_budget={} cpu_pixel_path=diagnostic-read-only resize_fallback=per-surface-contract\n",
            logical_frame.sequence,
            output.name(),
            target.name,
            logical_frame.pipeline_name,
            logical_frame.width,
            logical_frame.height,
            output_buffer_index,
            logical_frame.layer_count,
            window_stats.windows,
            last_complete.map(|frame| frame.sequence).unwrap_or(0),
            if window_scene_changed {
                "full-window-change"
            } else if full_redraw {
                "full"
            } else {
                "damage"
            },
            damage_rect.width,
            damage_rect.height,
            damage_rect.x,
            damage_rect.y,
            if opaque_damage_overwrite {
                "skipped-opaque-chart"
            } else {
                "gpu-fill-worklist"
            },
            if upper_plane {
                "transparent-upper-plane"
            } else {
                "opaque-primary"
            },
            window_stats.paint_ops,
            window_stats.fill_descs,
            window_stats.submits,
            window_stats.proof_pixels,
            window_stats.proof_mismatches,
            presented as u8,
            acquire_us,
            clear_us,
            layers_us,
            draw3d_us,
            proof_us,
            chart_us,
            scratch_clear_us,
            scale_us,
            blend_us,
            window_us,
            commit_us,
            total_us,
            (total_us > 16_667) as u8,
        );
    }
    Ok(Ui3CompositionResult {
        presented,
        output,
        width: target.width,
        height: target.height,
        layers: scene_item_count,
        clear_us,
        draw3d_us,
        proof_us,
        commit_us,
        total_us,
    })
}

fn snapshot_layers(output: Ui3OutputId) -> Vec<LayerSnapshot> {
    let state = STATE.lock();
    let mut layers = Vec::with_capacity(UI3_SURFACE_COUNT);
    for surface in &state.surfaces.surfaces {
        let Some(front) = surface.front else {
            continue;
        };
        if !surface.placement.visible
            || surface.placement.opacity == 0
            || surface.placement.output != output
        {
            continue;
        }
        let Some(storage) = surface.buffers[front] else {
            continue;
        };
        if storage.surface.width != surface.width || storage.surface.height != surface.height {
            continue;
        }
        layers.push(LayerSnapshot {
            surface_id: surface.id,
            label: surface.label,
            generation: surface.front_generation,
            surface: storage.surface,
            placement: surface.placement,
            resize_fallback: surface.resize_contract.fallback(),
        });
    }
    layers
}

impl PresentedOutputScene {
    fn capture(
        width: u32,
        height: u32,
        window_scene: Option<WindowSceneSnapshot>,
        layers: &[LayerSnapshot],
    ) -> Self {
        let mut presented_layers = [None; UI3_SURFACE_COUNT];
        for (slot, layer) in layers.iter().take(UI3_SURFACE_COUNT).enumerate() {
            presented_layers[slot] = Some(PresentedLayerSnapshot {
                surface_id: layer.surface_id,
                generation: layer.generation,
                source_width: layer.surface.width,
                source_height: layer.surface.height,
                placement: layer.placement,
                resize_fallback: layer.resize_fallback,
            });
        }
        Self {
            width,
            height,
            windows: window_scene.map(|scene| scene.windows),
            layers: presented_layers,
        }
    }

    fn layer(self, surface_id: SurfaceId) -> Option<PresentedLayerSnapshot> {
        self.layers
            .iter()
            .flatten()
            .copied()
            .find(|layer| layer.surface_id == surface_id)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct DamageRect {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl DamageRect {
    fn full(width: u32, height: u32) -> Self {
        Self {
            left: 0,
            top: 0,
            right: i64::from(width),
            bottom: i64::from(height),
        }
    }

    fn include(&mut self, x: i32, y: i32, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.left = self.left.min(i64::from(x));
        self.top = self.top.min(i64::from(y));
        self.right = self
            .right
            .max(i64::from(x).saturating_add(i64::from(width)));
        self.bottom = self
            .bottom
            .max(i64::from(y).saturating_add(i64::from(height)));
    }

    fn include_window(&mut self, window: ManagedWindow) {
        if window.visible {
            self.include(window.x, window.y, window.width, window.height);
        }
    }

    fn include_layer(&mut self, layer: PresentedLayerSnapshot) {
        if layer.placement.visible {
            self.include(
                layer.placement.x,
                layer.placement.y,
                layer.placement.width,
                layer.placement.height,
            );
        }
    }

    fn clipped(self, width: u32, height: u32) -> Option<Self> {
        let clipped = Self {
            left: self.left.max(0),
            top: self.top.max(0),
            right: self.right.min(i64::from(width)),
            bottom: self.bottom.min(i64::from(height)),
        };
        (clipped.right > clipped.left && clipped.bottom > clipped.top).then_some(clipped)
    }

    fn as_gpgpu_rect(self) -> Option<GpgpuRect> {
        Some(GpgpuRect::new(
            i32::try_from(self.left).ok()?,
            i32::try_from(self.top).ok()?,
            u32::try_from(self.right - self.left).ok()?,
            u32::try_from(self.bottom - self.top).ok()?,
        ))
    }

    fn intersects(self, other: Self) -> bool {
        self.left < other.right
            && other.left < self.right
            && self.top < other.bottom
            && other.top < self.bottom
    }
}

fn output_scene_damage(
    previous: Option<PresentedOutputScene>,
    current: PresentedOutputScene,
) -> Option<DamageRect> {
    let Some(previous) = previous else {
        return Some(DamageRect::full(current.width, current.height));
    };
    if previous.width != current.width || previous.height != current.height {
        return Some(DamageRect::full(current.width, current.height));
    }

    let mut damage = DamageRect {
        left: i64::from(current.width),
        top: i64::from(current.height),
        right: 0,
        bottom: 0,
    };
    match (previous.windows, current.windows) {
        (Some(old), Some(new)) => {
            for old_window in old {
                let new_window = new
                    .iter()
                    .copied()
                    .find(|window| window.id == old_window.id);
                if new_window != Some(old_window) {
                    damage.include_window(old_window);
                    if let Some(new_window) = new_window {
                        damage.include_window(new_window);
                    }
                }
            }
            for new_window in new {
                if !old.iter().any(|window| window.id == new_window.id) {
                    damage.include_window(new_window);
                }
            }
        }
        (Some(old), None) => {
            for window in old {
                damage.include_window(window);
            }
        }
        (None, Some(new)) => {
            for window in new {
                damage.include_window(window);
            }
        }
        (None, None) => {}
    }

    for old_layer in previous.layers.iter().flatten().copied() {
        let new_layer = current.layer(old_layer.surface_id);
        if new_layer != Some(old_layer) {
            damage.include_layer(old_layer);
            if let Some(new_layer) = new_layer {
                damage.include_layer(new_layer);
            }
        }
    }
    for new_layer in current.layers.iter().flatten().copied() {
        if previous.layer(new_layer.surface_id).is_none() {
            damage.include_layer(new_layer);
        }
    }

    damage.clipped(current.width, current.height)
}

fn opaque_chart_fully_overwrites_damage(
    previous: Option<PresentedOutputScene>,
    current: PresentedOutputScene,
    damage: DamageRect,
) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    if previous.width != current.width
        || previous.height != current.height
        || previous.windows != current.windows
    {
        return false;
    }
    for layer in previous.layers.iter().flatten().copied() {
        if layer.surface_id != SurfaceId::SHELL_CHART
            && current.layer(layer.surface_id) != Some(layer)
        {
            return false;
        }
    }
    for layer in current.layers.iter().flatten().copied() {
        if layer.surface_id != SurfaceId::SHELL_CHART
            && previous.layer(layer.surface_id) != Some(layer)
        {
            return false;
        }
    }
    let Some(chart) = current.layer(SurfaceId::SHELL_CHART) else {
        return false;
    };
    if chart.placement.opacity != u8::MAX
        || chart.source_width != chart.placement.width
        || chart.source_height != chart.placement.height
    {
        return false;
    }
    if let Some(mut old_chart) = previous.layer(SurfaceId::SHELL_CHART) {
        old_chart.generation = chart.generation;
        if old_chart != chart {
            return false;
        }
    }
    DamageRect {
        left: i64::from(chart.placement.x),
        top: i64::from(chart.placement.y),
        right: i64::from(chart.placement.x).saturating_add(i64::from(chart.placement.width)),
        bottom: i64::from(chart.placement.y).saturating_add(i64::from(chart.placement.height)),
    }
    .clipped(current.width, current.height)
        == Some(damage)
}

fn compose_window_scene(
    scene: WindowSceneSnapshot,
    dst: GpgpuRgba8Surface,
    diagnostic_virt: *mut u8,
    buffer_index: usize,
    damage: DamageRect,
) -> Result<WindowCompositionStats, &'static str> {
    let mut windows = scene.windows;
    windows.sort_by_key(|window| (window.z, window.id));
    let mut paints = Vec::with_capacity(UI3_WINDOW_COUNT * 18);
    let mut visible = 0usize;
    for window in windows {
        if !window.visible {
            continue;
        }
        paint_window_frame(window, dst, &mut paints);
        visible = visible.saturating_add(1);
    }
    for paint in &mut paints {
        paint.left = paint.left.max(damage.left);
        paint.top = paint.top.max(damage.top);
        paint.right = paint.right.min(damage.right);
        paint.bottom = paint.bottom.min(damage.bottom);
    }
    paints.retain(|paint| paint.right > paint.left && paint.bottom > paint.top);
    let paint_ops = paints.len();
    let descs = resolve_window_paints(paints.as_slice());
    let fill = submit_window_fills(dst, descs.as_slice())?;
    let proof = if descs.is_empty() {
        WindowChromeProof {
            pixels: 0,
            mismatches: 0,
            first_x: -1,
            first_y: -1,
            first_expected: 0,
            first_actual: 0,
        }
    } else {
        verify_window_chrome_preflip(dst, diagnostic_virt, descs.as_slice())
    };
    if !descs.is_empty() || !proof.exact() {
        crate::log_info!(
            target: "ui3";
            "ui3-compositor: chrome-preflip-proof buffer={} gpu=0x{:X} exact={} pixels={} mismatches={} first={}x{} expected=0x{:08X} actual=0x{:08X} cache_action=clflush-read-only commit_gate={}\n",
            buffer_index,
            dst.gpu,
            proof.exact() as u8,
            proof.pixels,
            proof.mismatches,
            proof.first_x,
            proof.first_y,
            proof.first_expected,
            proof.first_actual,
            proof.exact() as u8,
        );
    }
    if !proof.exact() {
        return Err("ui3-window-preflip-readback");
    }
    Ok(WindowCompositionStats {
        windows: visible,
        paint_ops,
        fill_descs: fill.descs,
        submits: fill.submits,
        proof_pixels: proof.pixels,
        proof_mismatches: proof.mismatches,
    })
}

/// Read-only dense proof of exactly the non-overlapping rectangles submitted
/// for final chrome. The GPU completion marker has retired before this point.
/// Invalidating the WB CPU alias lets us distinguish broken render bytes from
/// a display-engine fetch/coherency failure before changing the plane SURF.
fn verify_window_chrome_preflip(
    dst: GpgpuRgba8Surface,
    diagnostic_virt: *mut u8,
    descs: &[FillRectWorklistRgba8Desc],
) -> WindowChromeProof {
    let mut proof = WindowChromeProof {
        pixels: 0,
        mismatches: 0,
        first_x: -1,
        first_y: -1,
        first_expected: 0,
        first_actual: 0,
    };
    if diagnostic_virt.is_null() || dst.pitch_bytes < 4 || dst.bytes == 0 {
        proof.mismatches = 1;
        return proof;
    }
    crate::intel::dma_flush(diagnostic_virt, dst.bytes);
    for desc in descs {
        let x0 = i32::from(desc.dst_xy as u16 as i16);
        let y0 = i32::from((desc.dst_xy >> 16) as u16 as i16);
        let width = desc.size & 0xFFFF;
        let height = desc.size >> 16;
        for y in 0..height {
            let py = y0.saturating_add(y as i32);
            for x in 0..width {
                let px = x0.saturating_add(x as i32);
                proof.pixels = proof.pixels.saturating_add(1);
                if px < 0 || py < 0 || px as u32 >= dst.width || py as u32 >= dst.height {
                    proof.mismatches = proof.mismatches.saturating_add(1);
                    if proof.first_x < 0 {
                        proof.first_x = px;
                        proof.first_y = py;
                        proof.first_expected = desc.color_rgba;
                    }
                    continue;
                }
                let offset = (py as usize)
                    .saturating_mul(dst.pitch_bytes as usize)
                    .saturating_add((px as usize).saturating_mul(4));
                if offset.saturating_add(4) > dst.bytes {
                    proof.mismatches = proof.mismatches.saturating_add(1);
                    continue;
                }
                let actual = unsafe {
                    core::ptr::read_volatile(diagnostic_virt.add(offset) as *const u32)
                };
                if actual != desc.color_rgba {
                    if proof.mismatches == 0 {
                        proof.first_x = px;
                        proof.first_y = py;
                        proof.first_expected = desc.color_rgba;
                        proof.first_actual = actual;
                    }
                    proof.mismatches = proof.mismatches.saturating_add(1);
                }
            }
        }
    }
    proof
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct WindowPaintRect {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
    color_rgba: u32,
}

fn paint_window_frame(
    window: ManagedWindow,
    dst: GpgpuRgba8Surface,
    paints: &mut Vec<WindowPaintRect>,
) {
    let title_height = window.height.min(UI3_WINDOW_TITLE_HEIGHT);
    let border = UI3_WINDOW_BORDER
        .min(window.width / 2)
        .min(window.height.saturating_sub(title_height));
    let right = window
        .x
        .saturating_add(i32::try_from(window.width).unwrap_or(i32::MAX));
    let title_bottom = window
        .y
        .saturating_add(i32::try_from(title_height).unwrap_or(i32::MAX));
    let bottom = window
        .y
        .saturating_add(i32::try_from(window.height).unwrap_or(i32::MAX));
    let (close_x, close_y, close_width, close_height) = window.close_rect();
    let close_right = close_x.saturating_add(i32::try_from(close_width).unwrap_or(i32::MAX));
    let close_bottom = close_y.saturating_add(i32::try_from(close_height).unwrap_or(i32::MAX));

    push_window_paint_rect(
        paints,
        dst,
        window.x,
        window.y,
        window.width,
        u32::try_from(close_y.saturating_sub(window.y)).unwrap_or(0),
        window.title_color,
    );
    push_window_paint_rect(
        paints,
        dst,
        window.x,
        close_y,
        u32::try_from(close_x.saturating_sub(window.x)).unwrap_or(0),
        close_height,
        window.title_color,
    );
    push_window_paint_rect(
        paints,
        dst,
        close_right,
        close_y,
        u32::try_from(right.saturating_sub(close_right)).unwrap_or(0),
        close_height,
        window.title_color,
    );
    push_window_paint_rect(
        paints,
        dst,
        window.x,
        close_bottom,
        window.width,
        u32::try_from(title_bottom.saturating_sub(close_bottom)).unwrap_or(0),
        window.title_color,
    );
    push_window_paint_rect(
        paints,
        dst,
        close_x,
        close_y,
        close_width,
        close_height,
        UI3_WINDOW_CLOSE_COLOR,
    );
    push_window_paint_rect(
        paints,
        dst,
        window.x,
        title_bottom,
        border,
        window.height.saturating_sub(title_height),
        window.border_color,
    );
    push_window_paint_rect(
        paints,
        dst,
        right.saturating_sub(i32::try_from(border).unwrap_or(0)),
        title_bottom,
        border,
        window.height.saturating_sub(title_height),
        window.border_color,
    );
    push_window_paint_rect(
        paints,
        dst,
        window
            .x
            .saturating_add(i32::try_from(border).unwrap_or(i32::MAX)),
        bottom.saturating_sub(i32::try_from(border).unwrap_or(0)),
        window.width.saturating_sub(border.saturating_mul(2)),
        border,
        window.border_color,
    );
    // The red square is deliberately the complete close icon. Keeping the
    // decoration to one non-overlapping primitive avoids turning three tiny
    // frames into dozens of long-running worklist fragments.
}

fn push_window_paint_rect(
    out: &mut Vec<WindowPaintRect>,
    dst: GpgpuRgba8Surface,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color_rgba: u32,
) {
    let left = i64::from(x).max(0);
    let top = i64::from(y).max(0);
    let right = (i64::from(x) + i64::from(width)).min(i64::from(dst.width));
    let bottom = (i64::from(y) + i64::from(height)).min(i64::from(dst.height));
    if right <= left || bottom <= top {
        return;
    }
    out.push(WindowPaintRect {
        left,
        top,
        right,
        bottom,
        color_rgba,
    });
}

/// Resolve painter order on the CPU as rectangle geometry only. The emitted
/// GPU descriptors never overlap, so all window colors and z-order remain
/// deterministic in one parallel worklist submission.
fn resolve_window_paints(paints: &[WindowPaintRect]) -> Vec<FillRectWorklistRgba8Desc> {
    let mut visible = Vec::with_capacity(paints.len());
    let mut covered = Vec::with_capacity(paints.len());
    for paint in paints.iter().rev().copied() {
        let mut fragments = Vec::from([paint]);
        for cover in &covered {
            let mut next = Vec::with_capacity(fragments.len().saturating_mul(2));
            for fragment in fragments {
                subtract_window_paint_rect(fragment, *cover, &mut next);
            }
            fragments = next;
            if fragments.is_empty() {
                break;
            }
        }
        visible.extend(fragments);
        covered.push(paint);
    }

    let mut descs = Vec::with_capacity(visible.len());
    for rect in visible {
        let Ok(x) = i16::try_from(rect.left) else {
            continue;
        };
        let Ok(y) = i16::try_from(rect.top) else {
            continue;
        };
        let Ok(width) = u16::try_from(rect.right - rect.left) else {
            continue;
        };
        let Ok(height) = u16::try_from(rect.bottom - rect.top) else {
            continue;
        };
        descs.push(FillRectWorklistRgba8Desc {
            dst_xy: u32::from(x as u16) | (u32::from(y as u16) << 16),
            size: u32::from(width) | (u32::from(height) << 16),
            color_rgba: rect.color_rgba,
        });
    }
    descs
}

fn subtract_window_paint_rect(
    rect: WindowPaintRect,
    cover: WindowPaintRect,
    out: &mut Vec<WindowPaintRect>,
) {
    let left = rect.left.max(cover.left);
    let top = rect.top.max(cover.top);
    let right = rect.right.min(cover.right);
    let bottom = rect.bottom.min(cover.bottom);
    if right <= left || bottom <= top {
        out.push(rect);
        return;
    }
    push_window_fragment(out, rect, rect.left, rect.top, rect.right, top);
    push_window_fragment(out, rect, rect.left, bottom, rect.right, rect.bottom);
    push_window_fragment(out, rect, rect.left, top, left, bottom);
    push_window_fragment(out, rect, right, top, rect.right, bottom);
}

fn push_window_fragment(
    out: &mut Vec<WindowPaintRect>,
    source: WindowPaintRect,
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
) {
    if right <= left || bottom <= top {
        return;
    }
    out.push(WindowPaintRect {
        left,
        top,
        right,
        bottom,
        color_rgba: source.color_rgba,
    });
}

fn submit_window_fills(
    dst: GpgpuRgba8Surface,
    descs: &[FillRectWorklistRgba8Desc],
) -> Result<crate::intel::gpgpu::GpgpuWorklistSubmitStats, &'static str> {
    if descs.is_empty() {
        return Ok(crate::intel::gpgpu::GpgpuWorklistSubmitStats::default());
    }
    let stats = crate::intel::gpgpu::fill_rect_worklist_rgba8_stats(dst, descs);
    if stats.descs != descs.len() || stats.submits == 0 {
        return Err("ui3-window-gpu-fill");
    }
    Ok(stats)
}

fn compose_layer_damage(
    layer: LayerSnapshot,
    dst: GpgpuRgba8Surface,
    damage: DamageRect,
) -> Result<LayerCompositionTiming, &'static str> {
    let layer_rect = DamageRect {
        left: i64::from(layer.placement.x),
        top: i64::from(layer.placement.y),
        right: i64::from(layer.placement.x).saturating_add(i64::from(layer.placement.width)),
        bottom: i64::from(layer.placement.y).saturating_add(i64::from(layer.placement.height)),
    };
    if !damage.intersects(layer_rect) {
        return Ok(LayerCompositionTiming::default());
    }
    if (layer.placement.width == layer.surface.width
        && layer.placement.height == layer.surface.height)
        || layer.resize_fallback == Ui3FrameResizeFallback::ClipVisible
    {
        let blend_started_ns = crate::chronos::monotonic_nanos();
        if !blend_premultiplied_layer_damage(layer.surface, layer.placement, dst, damage)? {
            return Err("ui3-output-gpu-damage-blend");
        }
        return Ok(LayerCompositionTiming {
            blend_us: elapsed_us(blend_started_ns, crate::chronos::monotonic_nanos()).max(1),
            ..LayerCompositionTiming::default()
        });
    }
    // The compatibility scaler is already transactional but does not yet
    // expose a source-crop contract. Recompose that one placement in full;
    // every ordinary native-size UI3 layer remains damage-clipped.
    compose_layer(layer, dst)
}

fn compose_layer(
    layer: LayerSnapshot,
    dst: GpgpuRgba8Surface,
) -> Result<LayerCompositionTiming, &'static str> {
    if layer.placement.width == layer.surface.width
        && layer.placement.height == layer.surface.height
    {
        let blend_started_ns = crate::chronos::monotonic_nanos();
        if !blend_premultiplied_layer(layer.surface, layer.placement, dst)? {
            return Err("ui3-output-gpu-blend");
        }
        return Ok(LayerCompositionTiming {
            blend_us: elapsed_us(blend_started_ns, crate::chronos::monotonic_nanos()).max(1),
            ..LayerCompositionTiming::default()
        });
    }

    if layer.resize_fallback == Ui3FrameResizeFallback::ClipVisible {
        let blend_started_ns = crate::chronos::monotonic_nanos();
        if !blend_premultiplied_layer(layer.surface, layer.placement, dst)? {
            return Err("ui3-output-gpu-clip-blend");
        }
        return Ok(LayerCompositionTiming {
            blend_us: elapsed_us(blend_started_ns, crate::chronos::monotonic_nanos()).max(1),
            ..LayerCompositionTiming::default()
        });
    }

    let scaled_placement =
        letterbox_placement(layer.surface, layer.placement).ok_or("ui3-output-letterbox-shape")?;
    let scratch_clear_started_ns = crate::chronos::monotonic_nanos();
    if !parallel_fill_placement(dst, layer.placement, 0xFF00_0000) {
        return Err("ui3-output-letterbox-fill");
    }

    if scaled_placement.width == layer.surface.width
        && scaled_placement.height == layer.surface.height
    {
        let fill_complete_ns = crate::chronos::monotonic_nanos();
        let blend_started_ns = fill_complete_ns;
        if !blend_premultiplied_layer(layer.surface, scaled_placement, dst)? {
            return Err("ui3-output-letterbox-blend");
        }
        return Ok(LayerCompositionTiming {
            scratch_clear_us: elapsed_us(scratch_clear_started_ns, fill_complete_ns).max(1),
            blend_us: elapsed_us(blend_started_ns, crate::chronos::monotonic_nanos()).max(1),
            ..LayerCompositionTiming::default()
        });
    }

    // The shipped SpriteQuad artifact is a straight-alpha compositor.  Use it
    // only as a COPY scaler into a transparent intermediate; this preserves
    // premultiplied bytes exactly. The premultiplied worklist blend then
    // applies frame opacity and source-over to the real output.
    let scratch = ensure_scale_scratch(scaled_placement.width, scaled_placement.height)?;
    if !parallel_fill_rect(
        scratch,
        GpgpuRect::new(0, 0, scratch.width, scratch.height),
        0x0000_0000,
    ) {
        return Err("ui3-output-gpu-scale-clear");
    }
    let scratch_clear_complete_ns = crate::chronos::monotonic_nanos();
    let width = scratch.width as f32;
    let height = scratch.height as f32;
    let desc = GpgpuSpriteQuadWorklistDesc {
        c0_x: 0.0,
        c0_y: 0.0,
        c0_u: 0.0,
        c0_v: 0.0,
        c1_x: width,
        c1_y: 0.0,
        c1_u: 1.0,
        c1_v: 0.0,
        c2_x: width,
        c2_y: height,
        c2_u: 1.0,
        c2_v: 1.0,
        c3_x: 0.0,
        c3_y: height,
        c3_u: 0.0,
        c3_v: 1.0,
        color_rgba: COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA,
        flags: 0,
    };
    let scale_started_ns = crate::chronos::monotonic_nanos();
    let scale = crate::intel::gpgpu::sprite_quad_worklist_rgba8_over_stats(
        layer.surface,
        scratch,
        core::slice::from_ref(&desc),
    );
    let scale_complete_ns = crate::chronos::monotonic_nanos();
    if scale.descs != 1 || scale.submits != 1 {
        return Err("ui3-output-gpu-scale");
    }
    let blend_started_ns = crate::chronos::monotonic_nanos();
    if !blend_premultiplied_layer(scratch, scaled_placement, dst)? {
        return Err("ui3-output-gpu-scale-blend");
    }
    Ok(LayerCompositionTiming {
        scratch_clear_us: elapsed_us(scratch_clear_started_ns, scratch_clear_complete_ns).max(1),
        scale_us: elapsed_us(scale_started_ns, scale_complete_ns).max(1),
        blend_us: elapsed_us(blend_started_ns, crate::chronos::monotonic_nanos()).max(1),
    })
}

fn blend_premultiplied_layer(
    src: GpgpuRgba8Surface,
    placement: Ui3FramePlacement,
    dst: GpgpuRgba8Surface,
) -> Result<bool, &'static str> {
    let Some((src_rect, dst_xy)) = clipped_unscaled_rect(src, placement, dst) else {
        return Ok(true);
    };
    Ok(parallel_blend_rect(src, src_rect, dst, dst_xy, placement.opacity))
}

fn blend_premultiplied_layer_damage(
    src: GpgpuRgba8Surface,
    placement: Ui3FramePlacement,
    dst: GpgpuRgba8Surface,
    damage: DamageRect,
) -> Result<bool, &'static str> {
    let Some((mut src_rect, mut dst_xy)) = clipped_unscaled_rect(src, placement, dst) else {
        return Ok(true);
    };
    let dst_left = i64::from(dst_xy.x);
    let dst_top = i64::from(dst_xy.y);
    let left = dst_left.max(damage.left);
    let top = dst_top.max(damage.top);
    let right = dst_left
        .saturating_add(i64::from(src_rect.width))
        .min(damage.right);
    let bottom = dst_top
        .saturating_add(i64::from(src_rect.height))
        .min(damage.bottom);
    if right <= left || bottom <= top {
        return Ok(true);
    }
    let x_skip = i32::try_from(left - dst_left).map_err(|_| "ui3-damage-x-range")?;
    let y_skip = i32::try_from(top - dst_top).map_err(|_| "ui3-damage-y-range")?;
    src_rect.x = src_rect.x.saturating_add(x_skip);
    src_rect.y = src_rect.y.saturating_add(y_skip);
    src_rect.width = u32::try_from(right - left).map_err(|_| "ui3-damage-width-range")?;
    src_rect.height = u32::try_from(bottom - top).map_err(|_| "ui3-damage-height-range")?;
    dst_xy.x = i32::try_from(left).map_err(|_| "ui3-damage-x-range")?;
    dst_xy.y = i32::try_from(top).map_err(|_| "ui3-damage-y-range")?;
    Ok(parallel_blend_rect(
        src,
        src_rect,
        dst,
        dst_xy,
        placement.opacity,
    ))
}

/// Split one logical rectangle evenly over the SIMD16 worklist lanes.  This
/// keeps the BSP on the retained descriptor kernel without leaving a single
/// descriptor (and therefore a single lane) to walk a large surface alone.
fn parallel_fill_rect(dst: GpgpuRgba8Surface, rect: GpgpuRect, color: u32) -> bool {
    let right = i64::from(rect.x) + i64::from(rect.width);
    let bottom = i64::from(rect.y) + i64::from(rect.height);
    if rect.x < 0
        || rect.y < 0
        || rect.width == 0
        || rect.height == 0
        || right > i64::from(dst.width)
        || bottom > i64::from(dst.height)
    {
        return false;
    }
    let Ok(dst_x) = i16::try_from(rect.x) else {
        return false;
    };
    let Ok(dst_y) = i16::try_from(rect.y) else {
        return false;
    };
    let Ok(width) = u16::try_from(rect.width) else {
        return false;
    };
    let Ok(height) = u16::try_from(rect.height) else {
        return false;
    };
    let band_count = usize::from(height).min(UI3_RECT_WORKLIST_LANES);
    let mut descs = [FillRectWorklistRgba8Desc::default(); UI3_RECT_WORKLIST_LANES];
    for (index, desc) in descs[..band_count].iter_mut().enumerate() {
        let y0 = u32::from(height) * index as u32 / band_count as u32;
        let y1 = u32::from(height) * (index as u32 + 1) / band_count as u32;
        let Ok(band_y) = i16::try_from(i32::from(dst_y) + y0 as i32) else {
            return false;
        };
        *desc = FillRectWorklistRgba8Desc {
            dst_xy: u32::from(dst_x as u16) | (u32::from(band_y as u16) << 16),
            size: u32::from(width) | ((y1 - y0) << 16),
            color_rgba: color,
        };
    }
    let stats = crate::intel::gpgpu::fill_rect_worklist_rgba8_stats(dst, &descs[..band_count]);
    stats.descs == band_count && stats.submits == 1
}

/// Fill the visible part of a logical placement. A fully clipped placement is
/// already satisfied and deliberately performs no GPU submission.
fn parallel_fill_placement(
    dst: GpgpuRgba8Surface,
    placement: Ui3FramePlacement,
    color: u32,
) -> bool {
    let left = i64::from(placement.x).max(0);
    let top = i64::from(placement.y).max(0);
    let right = (i64::from(placement.x) + i64::from(placement.width)).min(i64::from(dst.width));
    let bottom = (i64::from(placement.y) + i64::from(placement.height)).min(i64::from(dst.height));
    if right <= left || bottom <= top {
        return true;
    }
    let Ok(x) = i32::try_from(left) else {
        return false;
    };
    let Ok(y) = i32::try_from(top) else {
        return false;
    };
    let Ok(width) = u32::try_from(right - left) else {
        return false;
    };
    let Ok(height) = u32::try_from(bottom - top) else {
        return false;
    };
    parallel_fill_rect(dst, GpgpuRect::new(x, y, width, height), color)
}

fn parallel_blend_rect(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
    opacity: u8,
) -> bool {
    let src_right = i64::from(src_rect.x) + i64::from(src_rect.width);
    let src_bottom = i64::from(src_rect.y) + i64::from(src_rect.height);
    let dst_right = i64::from(dst_xy.x) + i64::from(src_rect.width);
    let dst_bottom = i64::from(dst_xy.y) + i64::from(src_rect.height);
    if src_rect.x < 0
        || src_rect.y < 0
        || src_rect.width == 0
        || src_rect.height == 0
        || dst_xy.x < 0
        || dst_xy.y < 0
        || src_right > i64::from(src.width)
        || src_bottom > i64::from(src.height)
        || dst_right > i64::from(dst.width)
        || dst_bottom > i64::from(dst.height)
    {
        return false;
    }
    let Ok(src_x) = u16::try_from(src_rect.x) else {
        return false;
    };
    let Ok(src_y) = u16::try_from(src_rect.y) else {
        return false;
    };
    let Ok(dst_x) = i16::try_from(dst_xy.x) else {
        return false;
    };
    let Ok(dst_y) = i16::try_from(dst_xy.y) else {
        return false;
    };
    let Ok(width) = u16::try_from(src_rect.width) else {
        return false;
    };
    let Ok(height) = u16::try_from(src_rect.height) else {
        return false;
    };
    let band_count = usize::from(height).min(UI3_RECT_WORKLIST_LANES);
    let mut flags = COMPOSITE_WORKLIST_FLAG_SRC_OVER | COMPOSITE_WORKLIST_FLAG_PREMUL_SRC;
    let color_rgba = if opacity == u8::MAX {
        COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA
    } else {
        flags |= COMPOSITE_WORKLIST_FLAG_TINT_ALPHA;
        (u32::from(opacity) << 24) | 0x00FF_FFFF
    };
    let mut descs = [AlphaBlendWorklistRgba8Desc::default(); UI3_RECT_WORKLIST_LANES];
    for (index, desc) in descs[..band_count].iter_mut().enumerate() {
        let y0 = u32::from(height) * index as u32 / band_count as u32;
        let y1 = u32::from(height) * (index as u32 + 1) / band_count as u32;
        let Ok(band_src_y) = u16::try_from(u32::from(src_y) + y0) else {
            return false;
        };
        let Ok(band_dst_y) = i16::try_from(i32::from(dst_y) + y0 as i32) else {
            return false;
        };
        *desc = AlphaBlendWorklistRgba8Desc {
            src_xy: u32::from(src_x) | (u32::from(band_src_y) << 16),
            dst_xy: u32::from(dst_x as u16) | (u32::from(band_dst_y as u16) << 16),
            size: u32::from(width) | ((y1 - y0) << 16),
            flags,
            color_rgba,
        };
    }
    let stats = crate::intel::gpgpu::alpha_blend_worklist_rgba8_over_stats(
        src,
        dst,
        &descs[..band_count],
    );
    stats.descs == band_count && stats.submits == 1
}

fn clipped_unscaled_rect(
    src: GpgpuRgba8Surface,
    placement: Ui3FramePlacement,
    dst: GpgpuRgba8Surface,
) -> Option<(GpgpuRect, GpgpuPoint)> {
    let visible_width = src.width.min(placement.width);
    let visible_height = src.height.min(placement.height);
    let left = i64::from(placement.x).max(0);
    let top = i64::from(placement.y).max(0);
    let right = (i64::from(placement.x) + i64::from(visible_width)).min(i64::from(dst.width));
    let bottom = (i64::from(placement.y) + i64::from(visible_height)).min(i64::from(dst.height));
    if right <= left || bottom <= top {
        return None;
    }
    let src_x = left.saturating_sub(i64::from(placement.x));
    let src_y = top.saturating_sub(i64::from(placement.y));
    let width = u32::try_from(right - left).ok()?;
    let height = u32::try_from(bottom - top).ok()?;
    Some((
        GpgpuRect::new(i32::try_from(src_x).ok()?, i32::try_from(src_y).ok()?, width, height),
        GpgpuPoint::new(i32::try_from(left).ok()?, i32::try_from(top).ok()?),
    ))
}

fn letterbox_placement(
    src: GpgpuRgba8Surface,
    placement: Ui3FramePlacement,
) -> Option<Ui3FramePlacement> {
    if src.width == 0 || src.height == 0 || placement.width == 0 || placement.height == 0 {
        return None;
    }

    let available_width = u64::from(placement.width);
    let available_height = u64::from(placement.height);
    let source_width = u64::from(src.width);
    let source_height = u64::from(src.height);
    let (width, height) = if available_width.saturating_mul(source_height)
        <= available_height.saturating_mul(source_width)
    {
        (
            placement.width,
            u32::try_from(available_width.saturating_mul(source_height) / source_width)
                .ok()?
                .max(1)
                .min(placement.height),
        )
    } else {
        (
            u32::try_from(available_height.saturating_mul(source_width) / source_height)
                .ok()?
                .max(1)
                .min(placement.width),
            placement.height,
        )
    };
    let x_offset = i32::try_from(placement.width.saturating_sub(width) / 2).ok()?;
    let y_offset = i32::try_from(placement.height.saturating_sub(height) / 2).ok()?;
    Some(Ui3FramePlacement {
        x: placement.x.saturating_add(x_offset),
        y: placement.y.saturating_add(y_offset),
        width,
        height,
        ..placement
    })
}

fn ensure_surface_buffers(
    surface: &mut SurfaceState,
    width: u32,
    height: u32,
) -> Result<(), &'static str> {
    let shape_changed = surface
        .buffers
        .iter()
        .flatten()
        .any(|storage| storage.surface.width != width || storage.surface.height != height);
    if shape_changed {
        if surface.acquired.is_some() {
            return Err("ui3-logical-frame-mode-resize-in-flight");
        }
        // Logical sources are never scanned out. All producer/compositor
        // submissions retire synchronously, so replacing both buffers is safe
        // here; the producer render PPGTT and direct-RCS PPGTT overwrite their
        // fixed-VA mappings before either new backing is consumed.
        for storage in surface.buffers.iter_mut().filter_map(Option::take) {
            crate::dma::dealloc(storage.virt, storage.surface.bytes);
        }
        surface.front = None;
        surface.front_generation = 0;
        surface.pending_present_generation = None;
    }
    for buffer in 0..UI3_SURFACE_BUFFER_COUNT {
        if surface.buffers[buffer].is_some() {
            continue;
        }
        let slot = surface.gpu_slot_base.saturating_add(buffer);
        if slot >= UI3_SURFACE_COUNT * UI3_SURFACE_BUFFER_COUNT {
            return Err("ui3-surface-gpu-slot-range");
        }
        let gpu = UI3_SURFACE_GPU_BASE
            .checked_add((slot as u64).saturating_mul(UI3_SURFACE_GPU_STRIDE))
            .ok_or("ui3-logical-frame-gpu-overflow")?;
        surface.buffers[buffer] = Some(allocate_surface(width, height, gpu)?);
        crate::log_info!(
            target: "ui3";
            "ui3-compositor: surface buffer registered surface_id={} label={} producer={} format=rgba8-premultiplied buffer={} gpu_slot={} gpu=0x{:X} size={}x{}\n",
            surface.id.raw(),
            surface.label,
            surface.producer.name(),
            buffer,
            slot,
            gpu,
            width,
            height,
        );
    }
    surface.width = width;
    surface.height = height;
    Ok(())
}

fn ensure_scale_scratch(width: u32, height: u32) -> Result<GpgpuRgba8Surface, &'static str> {
    let mut state = STATE.lock();
    if let Some(storage) = state.scale_scratch {
        if storage.surface.width == width && storage.surface.height == height {
            return Ok(storage.surface);
        }
        // Every compositor kernel submission is synchronously retired before
        // return, so the old scratch backing is no longer in flight.  The next
        // direct-RCS submission overwrites this fixed VA's PTE.
        crate::dma::dealloc(storage.virt, storage.surface.bytes);
        state.scale_scratch = None;
    }
    let storage = allocate_surface(width, height, UI3_SCALE_SCRATCH_GPU)?;
    let surface = storage.surface;
    state.scale_scratch = Some(storage);
    Ok(surface)
}

fn allocate_surface(width: u32, height: u32, gpu: u64) -> Result<SurfaceStorage, &'static str> {
    if width == 0 || height == 0 || (gpu & 0xFFF) != 0 {
        return Err("ui3-logical-frame-shape");
    }
    let row_bytes = (width as usize)
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or("ui3-logical-frame-size")?;
    let pitch = crate::intel::align_up(row_bytes, 64).ok_or("ui3-logical-frame-pitch")?;
    let raw_bytes = pitch
        .checked_mul(height as usize)
        .ok_or("ui3-logical-frame-size")?;
    let bytes = crate::intel::align_up(raw_bytes, crate::intel::WARM_ALIGN)
        .ok_or("ui3-logical-frame-size")?;
    if bytes as u64 > UI3_SURFACE_GPU_STRIDE
        || gpu.saturating_add(bytes as u64) > UI3_COMPOSITOR_GPU_LIMIT
    {
        return Err("ui3-logical-frame-capacity");
    }
    let (phys, virt) =
        crate::dma::alloc(bytes, crate::intel::WARM_ALIGN).ok_or("ui3-logical-frame-alloc")?;
    let Some(surface) = GpgpuRgba8Surface::new(
        phys,
        gpu,
        bytes,
        width,
        height,
        u32::try_from(pitch).map_err(|_| "ui3-logical-frame-pitch")?,
    ) else {
        crate::dma::dealloc(virt, bytes);
        return Err("ui3-logical-frame-surface");
    };
    crate::log_info!(
        target: "ui3";
        "ui3-compositor: source buffer allocated gpu=0x{:X} phys=0x{:X} size={}x{} pitch=0x{:X} bytes=0x{:X} initialization=gpu-only\n",
        gpu,
        phys,
        width,
        height,
        pitch,
        bytes,
    );
    Ok(SurfaceStorage { surface, virt })
}
