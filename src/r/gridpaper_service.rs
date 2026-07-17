//! Embassy consumer for GridPaper's fixed snapshot format.
//!
//! The Blueprint owns edits and publication cadence. This service owns the
//! accepted copy, UI4 surface, GPU allocations, and presentation lifetime.
//! No UI4 handles or generic drawing operations cross the ABI.

use alloc::{string::String, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

use crate::intel::gpu_font::{
    GPU_FONT_COLOR_KEYFRAME_CAPACITY, GpuFontColorChannels, GpuFontColorIteration,
    GpuFontColorKeyframe, GpuFontColorKeyframes, GpuFontColorProgram, GpuFontColorTiming,
    GpuFontRgba,
};

const COLUMNS: usize = 37;
const ROWS: usize = 53;
const CELL_BYTES: usize = 20;
const CELL_TEXT_CAPACITY: usize = 16;
const PAGE_BYTES: usize = COLUMNS * ROWS * CELL_BYTES;
const TEXT_OFFSET: usize = 4;
const VALID_STYLE_BITS: u8 = 0x0f;
const STYLE_BOLD: u8 = 1 << 0;
const STYLE_STRIKEOUT: u8 = 1 << 1;
const STYLE_UNDERLINE: u8 = 1 << 2;
const COLOR_COUNT: usize = 18;
const COLOR_DEFAULT: u8 = 0;
const COLOR_TRANSPARENT: u8 = 17;
const TEXT_ANIMATION_COLOR_SLOTS: usize = COLOR_TRANSPARENT as usize;
const TEXT_ANIMATION_WIRE_VERSION: u8 = 1;
const TEXT_ANIMATION_WIRE_HEADER_BYTES: usize = 4;
const TEXT_ANIMATION_RECORD_HEADER_BYTES: usize = 12;
const TEXT_ANIMATION_KEYFRAME_BYTES: usize = 8;
const MIN_ANIMATION_DURATION_MS: u32 = 16;
const MAX_ANIMATION_DURATION_MS: u32 = 600_000;
const MIN_SCALE_PERCENT: u32 = 1;
const MAX_SCALE_PERCENT: u32 = 800;

const DEFAULT_REGULAR_ROW_FONT_PIXELS: f32 = 24.0;
const A4_WIDTH_MM: u32 = 210;
const A4_HEIGHT_MM: u32 = 297;
const CELL_EDGE_MM: u32 = 5;
const GRID_WIDTH_MM: u32 = COLUMNS as u32 * CELL_EDGE_MM;
const GRID_HEIGHT_MM: u32 = ROWS as u32 * CELL_EDGE_MM;
const RULER_GUTTER_MM: u32 = 4;
const SURFACE_WIDTH_MM: u32 = RULER_GUTTER_MM + GRID_WIDTH_MM;
const SURFACE_HEIGHT_MM: u32 = RULER_GUTTER_MM + GRID_HEIGHT_MM;
// The retained scene uses millimetres as its coordinate space. This makes the
// grid, rulers, and EDID-sized raster share one physical unit directly.
const SCENE_WIDTH: u32 = SURFACE_WIDTH_MM;
const SCENE_HEIGHT: u32 = SURFACE_HEIGHT_MM;
const SMALL_TICK_LENGTH_MM: f32 = 1.25;
const CENTIMETER_TICK_LENGTH_MM: f32 = 2.5;
const THREE_CENTIMETER_TICK_LENGTH_MM: f32 = 4.0;
const TEXT_LEFT_INSET_MM: f32 = 0.75;
const DECORATION_INSET_MM: f32 = 0.5;
const UI4_OWNER: crate::ui4::WindowOwner = crate::ui4::WindowOwner::KernelApp(4);
const SERVICE_PERIOD_MS: u64 = 16;

const ERROR_INVALID_SNAPSHOT: i32 = -1;
const ERROR_INVALID_SCALE: i32 = -2;
const ERROR_NOT_OWNER: i32 = -3;
const ERROR_TRANSPORT: i32 = -4;
const ERROR_INVALID_ANIMATION: i32 = -5;

struct SnapshotStore {
    buffers: [[u8; PAGE_BYTES]; 2],
    published: usize,
    owner: Option<u8>,
    generation: u64,
    scale_percent: u16,
    serial: u64,
    text_animations: [Option<GpuFontColorProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_serial: u64,
}

impl SnapshotStore {
    const fn new() -> Self {
        Self {
            buffers: [[0; PAGE_BYTES]; 2],
            published: 0,
            owner: None,
            generation: 0,
            scale_percent: 100,
            serial: 0,
            text_animations: [None; TEXT_ANIMATION_COLOR_SLOTS],
            animation_serial: 0,
        }
    }
}

static SNAPSHOTS: Mutex<SnapshotStore> = Mutex::new(SnapshotStore::new());

struct OwnedSnapshot {
    raw: Vec<u8>,
    generation: u64,
    scale_percent: u16,
    serial: u64,
}

#[derive(Clone, Copy)]
struct OwnedTextAnimations {
    programs: [Option<GpuFontColorProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    serial: u64,
}

/// Accept a snapshot from a vmcall after its producer identity is known.
pub(crate) fn submit_snapshot_for_owner(
    owner: u8,
    generation: u64,
    scale_percent: u32,
    raw: &[u8],
) -> i32 {
    if raw.len() != PAGE_BYTES || validate_page(raw).is_err() {
        return ERROR_INVALID_SNAPSHOT;
    }
    if !(MIN_SCALE_PERCENT..=MAX_SCALE_PERCENT).contains(&scale_percent) {
        return ERROR_INVALID_SCALE;
    }

    let mut snapshots = SNAPSHOTS.lock();
    if snapshots.owner.is_some_and(|active| active != owner) {
        return ERROR_NOT_OWNER;
    }
    let next = snapshots.published ^ 1;
    snapshots.buffers[next].copy_from_slice(raw);
    snapshots.published = next;
    snapshots.owner = Some(owner);
    snapshots.generation = generation;
    snapshots.scale_percent = scale_percent as u16;
    snapshots.serial = snapshots.serial.wrapping_add(1).max(1);
    0
}

/// Replace the complete CSS-like text animation table for one producer.
/// Palette indices 0..16 act as stable selectors for foreground text layers.
pub(crate) fn submit_text_animations_for_owner(owner: u8, raw: &[u8]) -> i32 {
    let Ok(programs) = decode_text_animations(raw) else {
        return ERROR_INVALID_ANIMATION;
    };
    let mut snapshots = SNAPSHOTS.lock();
    if snapshots.owner.is_some_and(|active| active != owner) {
        return ERROR_NOT_OWNER;
    }
    snapshots.owner = Some(owner);
    snapshots.text_animations = programs;
    snapshots.animation_serial = snapshots.animation_serial.wrapping_add(1).max(1);
    crate::log_info!(
        target: "gridpaper";
        "gridpaper: text-animation-table accepted serial={} programs={} wire_bytes={} ownership=producer-scoped geometry_uploads=0\n",
        snapshots.animation_serial,
        snapshots.text_animations.iter().flatten().count(),
        raw.len(),
    );
    0
}

/// Relinquish producer authority without destroying the last persistent scene.
pub(crate) fn close_owner(owner: u8) -> i32 {
    let mut snapshots = SNAPSHOTS.lock();
    match snapshots.owner {
        Some(active) if active == owner => {
            snapshots.owner = None;
            0
        }
        Some(_) => ERROR_NOT_OWNER,
        None => 0,
    }
}

fn snapshot_after(serial: u64) -> Option<OwnedSnapshot> {
    let snapshots = SNAPSHOTS.lock();
    if snapshots.serial == 0 || snapshots.serial == serial {
        return None;
    }
    Some(OwnedSnapshot {
        raw: snapshots.buffers[snapshots.published].to_vec(),
        generation: snapshots.generation,
        scale_percent: snapshots.scale_percent,
        serial: snapshots.serial,
    })
}

fn text_animations_after(serial: u64) -> Option<OwnedTextAnimations> {
    let snapshots = SNAPSHOTS.lock();
    if snapshots.animation_serial == serial {
        return None;
    }
    Some(OwnedTextAnimations {
        programs: snapshots.text_animations,
        serial: snapshots.animation_serial,
    })
}

fn decode_text_animations(
    raw: &[u8],
) -> Result<[Option<GpuFontColorProgram>; TEXT_ANIMATION_COLOR_SLOTS], ()> {
    if raw.len() < TEXT_ANIMATION_WIRE_HEADER_BYTES
        || raw[0] != TEXT_ANIMATION_WIRE_VERSION
        || raw[2] != 0
        || raw[3] != 0
    {
        return Err(());
    }
    let count = usize::from(raw[1]);
    if count > TEXT_ANIMATION_COLOR_SLOTS {
        return Err(());
    }
    let mut programs = [None; TEXT_ANIMATION_COLOR_SLOTS];
    let mut cursor = TEXT_ANIMATION_WIRE_HEADER_BYTES;
    for _ in 0..count {
        let header_end = cursor
            .checked_add(TEXT_ANIMATION_RECORD_HEADER_BYTES)
            .ok_or(())?;
        let header = raw.get(cursor..header_end).ok_or(())?;
        let selector = usize::from(header[0]);
        let channels = GpuFontColorChannels::from_bits(header[1]).ok_or(())?;
        let timing = match header[2] {
            0 => GpuFontColorTiming::Linear,
            1 => GpuFontColorTiming::EaseInOutSine,
            _ => return Err(()),
        };
        let iteration = match header[3] {
            0 => GpuFontColorIteration::Once,
            1 => GpuFontColorIteration::Loop,
            2 => GpuFontColorIteration::Alternate,
            _ => return Err(()),
        };
        let duration_ms = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let frame_count = usize::from(header[8]);
        if selector >= TEXT_ANIMATION_COLOR_SLOTS
            || programs[selector].is_some()
            || !(MIN_ANIMATION_DURATION_MS..=MAX_ANIMATION_DURATION_MS).contains(&duration_ms)
            || !(2..=GPU_FONT_COLOR_KEYFRAME_CAPACITY).contains(&frame_count)
            || header[9..12] != [0, 0, 0]
        {
            return Err(());
        }
        cursor = header_end;
        let mut frames = [GpuFontColorKeyframe::EMPTY; GPU_FONT_COLOR_KEYFRAME_CAPACITY];
        let mut previous_offset = None;
        for frame in frames.iter_mut().take(frame_count) {
            let frame_end = cursor
                .checked_add(TEXT_ANIMATION_KEYFRAME_BYTES)
                .ok_or(())?;
            let encoded = raw.get(cursor..frame_end).ok_or(())?;
            let offset_permille = u16::from_le_bytes([encoded[0], encoded[1]]);
            if encoded[2] != 0
                || encoded[3] != 0
                || offset_permille > 1_000
                || previous_offset.is_some_and(|previous| offset_permille <= previous)
            {
                return Err(());
            }
            *frame = GpuFontColorKeyframe {
                offset_permille,
                rgba: GpuFontRgba::new(encoded[4], encoded[5], encoded[6], encoded[7]),
            };
            previous_offset = Some(offset_permille);
            cursor = frame_end;
        }
        if frames[0].offset_permille != 0 || frames[frame_count - 1].offset_permille != 1_000 {
            return Err(());
        }
        programs[selector] = Some(GpuFontColorProgram::Keyframes(GpuFontColorKeyframes {
            frames,
            frame_count: frame_count as u8,
            channels,
            duration_ms,
            timing,
            iteration,
        }));
    }
    if cursor != raw.len() {
        return Err(());
    }
    Ok(programs)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gridpaper_snapshot_submit(
    generation: u64,
    scale_percent: u32,
    raw_ptr: *const u8,
    raw_len: usize,
) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        if raw_ptr.is_null() || raw_len != PAGE_BYTES {
            return ERROR_INVALID_SNAPSHOT;
        }
        // SAFETY: the ABI caller promises `raw_len` readable bytes.
        let raw = unsafe { core::slice::from_raw_parts(raw_ptr, raw_len) };
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_GRIDPAPER_SNAPSHOT_SUBMIT,
            generation,
            u64::from(scale_percent),
            raw,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    if raw_ptr.is_null() || raw_len != PAGE_BYTES {
        return ERROR_INVALID_SNAPSHOT;
    }
    let Some(owner) = crate::hv::current_guest_execution_context_vm_id() else {
        return ERROR_NOT_OWNER;
    };
    // SAFETY: checked non-null above; the ABI caller promises readable bytes.
    let raw = unsafe { core::slice::from_raw_parts(raw_ptr, raw_len) };
    submit_snapshot_for_owner(owner, generation, scale_percent, raw)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gridpaper_text_animations_submit(
    raw_ptr: *const u8,
    raw_len: usize,
) -> i32 {
    if raw_ptr.is_null() || raw_len < TEXT_ANIMATION_WIRE_HEADER_BYTES {
        return ERROR_INVALID_ANIMATION;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        // SAFETY: the ABI caller promises `raw_len` readable bytes.
        let raw = unsafe { core::slice::from_raw_parts(raw_ptr, raw_len) };
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_GRIDPAPER_TEXT_ANIMATIONS_SUBMIT,
            0,
            0,
            raw,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    let Some(owner) = crate::hv::current_guest_execution_context_vm_id() else {
        return ERROR_NOT_OWNER;
    };
    // SAFETY: checked non-null above; the ABI caller promises readable bytes.
    let raw = unsafe { core::slice::from_raw_parts(raw_ptr, raw_len) };
    submit_text_animations_for_owner(owner, raw)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_gridpaper_close() -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_GRIDPAPER_CLOSE, 0, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    crate::hv::current_guest_execution_context_vm_id()
        .map(close_owner)
        .unwrap_or(ERROR_NOT_OWNER)
}

fn validate_page(raw: &[u8]) -> Result<(), ()> {
    if raw.len() != PAGE_BYTES {
        return Err(());
    }
    for cell in raw.chunks_exact(CELL_BYTES) {
        let text_len = usize::from(cell[0]);
        if text_len > CELL_TEXT_CAPACITY
            || cell[1] >= COLOR_COUNT as u8
            || cell[2] >= COLOR_COUNT as u8
            || cell[3] & !VALID_STYLE_BITS != 0
            || core::str::from_utf8(&cell[TEXT_OFFSET..TEXT_OFFSET + text_len]).is_err()
        {
            return Err(());
        }
    }
    Ok(())
}

struct GridPaperSurface {
    frame: crate::ui4::FrameHandle,
    window: crate::ui4::WindowId,
    width: u32,
    height: u32,
    extent_source: &'static str,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ServiceError {
    Frame(crate::ui4::FramePoolError),
    Window(crate::ui4::WindowBrokerError),
    Render(&'static str),
    InvalidFrame,
}

impl From<crate::ui4::FramePoolError> for ServiceError {
    fn from(error: crate::ui4::FramePoolError) -> Self {
        Self::Frame(error)
    }
}

impl From<crate::ui4::WindowBrokerError> for ServiceError {
    fn from(error: crate::ui4::WindowBrokerError) -> Self {
        Self::Window(error)
    }
}

fn initialize_surface() -> Result<GridPaperSurface, ServiceError> {
    let (width, height, extent_source) =
        crate::intel::physical_extent_pixels(SURFACE_WIDTH_MM, SURFACE_HEIGHT_MM)
            .map(|(width, height)| (width, height, "edid-physical-mm"))
            .unwrap_or((SCENE_WIDTH, SCENE_HEIGHT, "logical-fallback"));
    let (grid_width, grid_height) =
        crate::intel::physical_extent_pixels(GRID_WIDTH_MM, GRID_HEIGHT_MM)
            .unwrap_or((GRID_WIDTH_MM, GRID_HEIGHT_MM));
    let output = crate::ui4::OutputId::from_slot(0).expect("UI4 D01 must exist");
    let frame = crate::ui4::create_frame(crate::ui4::FrameSpec {
        output,
        content: crate::ui4::FrameContent::RenderScene3d,
        cadence: crate::ui4::FrameCadence::Dirty,
        format: crate::ui4::ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(crate::ui4::PremultipliedRgba8::TRANSPARENT),
    })?;
    let session = match crate::ui4::begin_window_session(UI4_OWNER) {
        Ok(session) => session,
        Err(error) => {
            let _ = crate::ui4::destroy_frame(frame);
            return Err(error.into());
        }
    };
    let (scanout_width, scanout_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((width, height));
    // Keep the useful grid centered. The surface itself extends only far
    // enough above and to the left to carry the two ruler axes.
    let x = scanout_width
        .saturating_sub(grid_width)
        .saturating_div(2)
        .saturating_sub(width.saturating_sub(grid_width));
    let y = scanout_height
        .saturating_sub(grid_height)
        .saturating_div(2)
        .saturating_sub(height.saturating_sub(grid_height));
    let window = match crate::ui4::create_window(crate::ui4::WindowCreate {
        owner: UI4_OWNER,
        session,
        frame,
        output,
        plane: crate::ui4::WindowPlane::Universal(crate::ui4::RGB_OVERLAY_PLANE_SLOT_3 as u8),
        placement: crate::ui4::WindowPlacement {
            x: x as i32,
            y: y as i32,
            width,
            height,
            z: 70,
            opacity: u8::MAX,
            visible: true,
        },
    }) {
        Ok(window) => window,
        Err(error) => {
            let _ = crate::ui4::finish_window_session(UI4_OWNER, session);
            let _ = crate::ui4::destroy_frame(frame);
            return Err(error.into());
        }
    };
    Ok(GridPaperSurface {
        frame,
        window,
        width,
        height,
        extent_source,
    })
}

struct ResidentLayer {
    base_color: [u8; 4],
    text_color_selector: Option<u8>,
    mesh: crate::intel::render::ResidentTriangleMesh,
}

impl Drop for ResidentLayer {
    fn drop(&mut self) {
        if !crate::intel::render::release_resident_triangle_mesh(&self.mesh) {
            crate::log_warn!(
                target: "gridpaper";
                "gridpaper: resident layer release deferred gpu=0x{:X} bytes={}\n",
                self.mesh.gpu_base,
                self.mesh.storage_bytes,
            );
        }
    }
}

struct ResidentPage {
    serial: u64,
    generation: u64,
    scale_percent: u16,
    layers: Vec<ResidentLayer>,
}

struct Geometry {
    vertices: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

impl Geometry {
    fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    fn quad(&mut self, left: f32, top: f32, right: f32, bottom: f32, z: f32) {
        if right <= left || bottom <= top {
            return;
        }
        let Ok(base) = u32::try_from(self.vertices.len()) else {
            return;
        };
        self.vertices.extend_from_slice(&[
            clip_vertex(left, top, z),
            clip_vertex(left, bottom, z),
            clip_vertex(right, bottom, z),
            clip_vertex(right, top, z),
        ]);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

fn clip_vertex(x: f32, y: f32, z: f32) -> [f32; 3] {
    [
        x * 2.0 / SCENE_WIDTH as f32 - 1.0,
        1.0 - y * 2.0 / SCENE_HEIGHT as f32,
        z,
    ]
}

struct TextCell {
    text: String,
    color: u8,
    x: f32,
    y: f32,
    font_pixels: f32,
    bold: bool,
}

fn axis_tick_length_mm(cell_index: usize) -> f32 {
    let distance_mm = cell_index as u32 * CELL_EDGE_MM;
    if distance_mm % 30 == 0 {
        THREE_CENTIMETER_TICK_LENGTH_MM
    } else if distance_mm % 10 == 0 {
        CENTIMETER_TICK_LENGTH_MM
    } else {
        SMALL_TICK_LENGTH_MM
    }
}

fn build_resident_page(
    snapshot: &OwnedSnapshot,
    raster_width: u32,
    raster_height: u32,
) -> Result<ResidentPage, &'static str> {
    use crate::intel::gpu_font::{
        GpuFontFace, GpuFontJobEntry, GpuFontTextRequest, create_resident_font_scene_mesh,
        ensure_font_face_available,
    };

    ensure_font_face_available(GpuFontFace::Default)?;
    let mut layers = Vec::new();

    let mut backgrounds: Vec<(u8, Geometry)> = Vec::new();
    let mut decorations: Vec<(u8, Geometry)> = Vec::new();
    let mut texts = Vec::new();
    let scene_units_per_mm_x = SCENE_WIDTH as f32 / SURFACE_WIDTH_MM as f32;
    let scene_units_per_mm_y = SCENE_HEIGHT as f32 / SURFACE_HEIGHT_MM as f32;
    let cell_width = CELL_EDGE_MM as f32 * scene_units_per_mm_x;
    let cell_height = CELL_EDGE_MM as f32 * scene_units_per_mm_y;
    let grid_width = COLUMNS as f32 * cell_width;
    let grid_height = ROWS as f32 * cell_height;
    let grid_left = RULER_GUTTER_MM as f32 * scene_units_per_mm_x;
    let grid_top = RULER_GUTTER_MM as f32 * scene_units_per_mm_y;
    let grid_right = grid_left + grid_width;
    let grid_bottom = grid_top + grid_height;
    let visible_scene_x = SCENE_WIDTH as f32 / raster_width as f32;
    let visible_scene_y = SCENE_HEIGHT as f32 / raster_height as f32;
    let scale = f32::from(snapshot.scale_percent) / 100.0;

    // Only the grid owns paper. The ruler gutters remain transparent, and
    // there is no unused A4 margin on the right or bottom of the frame.
    let mut paper = Geometry::new();
    paper.quad(grid_left, grid_top, grid_right, grid_bottom, 0.9);
    push_geometry_layer(&mut layers, paper, palette(COLOR_DEFAULT, true))?;

    for row in 0..ROWS {
        let top = grid_top + row as f32 * cell_height;
        let bottom = top + cell_height;
        for column in 0..COLUMNS {
            let offset = (row * COLUMNS + column) * CELL_BYTES;
            let cell = &snapshot.raw[offset..offset + CELL_BYTES];
            let text_len = usize::from(cell[0]);
            let foreground = cell[1];
            let background = cell[2];
            let style = cell[3];
            let left = grid_left + column as f32 * cell_width;
            let right = left + cell_width;

            if background != COLOR_DEFAULT && background != COLOR_TRANSPARENT {
                geometry_for_color(&mut backgrounds, background).quad(
                    left + visible_scene_x * 0.5,
                    top + visible_scene_y * 0.5,
                    right - visible_scene_x * 0.5,
                    bottom - visible_scene_y * 0.5,
                    0.8,
                );
            }

            if foreground == COLOR_TRANSPARENT || text_len == 0 {
                continue;
            }
            let text = core::str::from_utf8(&cell[TEXT_OFFSET..TEXT_OFFSET + text_len])
                .map_err(|_| "gridpaper-utf8")?;
            // Font size is specified in output pixels. Convert it into the
            // logical scene units consumed by the resident font mesh so 100%
            // remains an actual 24 px regardless of the physical raster extent.
            let font_pixels = (DEFAULT_REGULAR_ROW_FONT_PIXELS * visible_scene_y * scale)
                .clamp(visible_scene_y, 256.0);
            let baseline = top + cell_height * 0.72;
            // The font tessellator's single-line mesh already starts with its
            // baseline one em below the supplied position. Pass the true mesh
            // origin here; passing `baseline` caused the observed one-row slip.
            let text_y = baseline - font_pixels;
            texts.push(TextCell {
                text: String::from(text),
                color: foreground,
                x: left + (TEXT_LEFT_INSET_MM * scene_units_per_mm_x * scale).min(cell_width * 0.2),
                y: text_y,
                font_pixels,
                bold: style & STYLE_BOLD != 0,
            });
            if style & STYLE_UNDERLINE != 0 {
                let thickness = (font_pixels / 14.0).max(visible_scene_y);
                let inset = DECORATION_INSET_MM * scene_units_per_mm_x;
                geometry_for_color(&mut decorations, foreground).quad(
                    left + inset,
                    baseline + thickness,
                    right - inset,
                    baseline + thickness * 2.0,
                    0.4,
                );
            }
            if style & STYLE_STRIKEOUT != 0 {
                let thickness = (font_pixels / 14.0).max(visible_scene_y);
                let y = baseline - font_pixels * 0.32;
                let inset = DECORATION_INSET_MM * scene_units_per_mm_x;
                geometry_for_color(&mut decorations, foreground).quad(
                    left + inset,
                    y,
                    right - inset,
                    y + thickness,
                    0.4,
                );
            }
        }
    }

    for (color, geometry) in backgrounds {
        push_geometry_layer(&mut layers, geometry, palette(color, true))?;
    }

    let mut grid = Geometry::new();
    let vertical_line = visible_scene_x;
    let horizontal_line = visible_scene_y;
    for column in 0..=COLUMNS {
        let x = grid_left + column as f32 * cell_width;
        grid.quad(x - vertical_line * 0.5, grid_top, x + vertical_line * 0.5, grid_bottom, 0.6);
    }
    for row in 0..=ROWS {
        let y = grid_top + row as f32 * cell_height;
        grid.quad(grid_left, y - horizontal_line * 0.5, grid_right, y + horizontal_line * 0.5, 0.6);
    }
    push_geometry_layer(&mut layers, grid, [188, 205, 224, 255])?;

    let mut rulers = Geometry::new();
    for column in 0..=COLUMNS {
        let x = grid_left + column as f32 * cell_width;
        let length = axis_tick_length_mm(column) * scene_units_per_mm_y;
        rulers.quad(
            x - vertical_line * 0.5,
            grid_top - length,
            x + vertical_line * 0.5,
            grid_top,
            0.55,
        );
    }
    for row in 0..=ROWS {
        let y = grid_top + row as f32 * cell_height;
        let length = axis_tick_length_mm(row) * scene_units_per_mm_x;
        rulers.quad(
            grid_left - length,
            y - horizontal_line * 0.5,
            grid_left,
            y + horizontal_line * 0.5,
            0.55,
        );
    }
    push_geometry_layer(&mut layers, rulers, [91, 101, 115, 255])?;

    for (color, geometry) in decorations {
        push_geometry_layer(&mut layers, geometry, palette(color, false))?;
    }

    for color in 0..COLOR_COUNT as u8 {
        if color == COLOR_TRANSPARENT || !texts.iter().any(|cell| cell.color == color) {
            continue;
        }
        let mut entries = Vec::new();
        for cell in texts.iter().filter(|cell| cell.color == color) {
            entries.push(GpuFontJobEntry {
                text: GpuFontTextRequest::SingleLine(cell.text.as_str()),
                position: [cell.x, cell.y],
                font_pixels: cell.font_pixels,
            });
            if cell.bold {
                entries.push(GpuFontJobEntry {
                    text: GpuFontTextRequest::SingleLine(cell.text.as_str()),
                    position: [cell.x + visible_scene_x, cell.y],
                    font_pixels: cell.font_pixels,
                });
            }
        }
        let mesh = create_resident_font_scene_mesh(
            &entries,
            GpuFontFace::Default,
            SCENE_WIDTH,
            SCENE_HEIGHT,
        )?;
        layers.push(ResidentLayer {
            base_color: palette(color, false),
            text_color_selector: Some(color),
            mesh,
        });
    }

    Ok(ResidentPage {
        serial: snapshot.serial,
        generation: snapshot.generation,
        scale_percent: snapshot.scale_percent,
        layers,
    })
}

fn geometry_for_color(layers: &mut Vec<(u8, Geometry)>, color: u8) -> &mut Geometry {
    if let Some(index) = layers.iter().position(|(candidate, _)| *candidate == color) {
        return &mut layers[index].1;
    }
    layers.push((color, Geometry::new()));
    let last = layers.len() - 1;
    &mut layers[last].1
}

fn push_geometry_layer(
    layers: &mut Vec<ResidentLayer>,
    geometry: Geometry,
    color: [u8; 4],
) -> Result<(), &'static str> {
    if geometry.is_empty() {
        return Ok(());
    }
    let mesh =
        crate::intel::render::create_resident_triangle_mesh(&geometry.vertices, &geometry.indices)?;
    layers.push(ResidentLayer {
        base_color: color,
        text_color_selector: None,
        mesh,
    });
    Ok(())
}

fn palette(color: u8, background: bool) -> [u8; 4] {
    match color {
        0 if background => [250, 252, 255, 255],
        0 | 1 => [20, 25, 32, 255],
        2 => [190, 45, 55, 255],
        3 => [36, 138, 72, 255],
        4 => [190, 145, 20, 255],
        5 => [40, 91, 190, 255],
        6 => [159, 54, 170, 255],
        7 => [30, 145, 155, 255],
        8 => [238, 241, 245, 255],
        9 => [91, 101, 115, 255],
        10 => [255, 94, 104, 255],
        11 => [85, 213, 120, 255],
        12 => [250, 207, 68, 255],
        13 => [92, 146, 255, 255],
        14 => [231, 105, 239, 255],
        15 => [79, 216, 226, 255],
        16 => [255, 255, 255, 255],
        _ => [0, 0, 0, 0],
    }
}

fn publish_page(
    surface: &GridPaperSurface,
    page: &ResidentPage,
    text_animations: &[Option<GpuFontColorProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_elapsed_ms: u64,
) -> Result<crate::intel::render::ResidentSceneFrameResult, ServiceError> {
    let draws = page
        .layers
        .iter()
        .map(|layer| crate::intel::render::ResidentSceneDraw {
            mesh: &layer.mesh,
            rgba: resident_layer_color(layer, text_animations, animation_elapsed_ms),
        })
        .collect::<Vec<_>>();
    let result =
        crate::intel::render::capture_resident_triangle_scene_frame_premultiplied_at_extent(
            &draws,
            Some([0, 0, 0, 0]),
            surface.width,
            surface.height,
            false,
        )
        .map_err(ServiceError::Render)?;
    if result.completed_draws != result.requested_draws || result.rgba.is_none() {
        return Err(ServiceError::Render("incomplete-frame"));
    }
    publish_pixels(surface, &result)?;
    Ok(result)
}

fn resident_layer_color(
    layer: &ResidentLayer,
    text_animations: &[Option<GpuFontColorProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_elapsed_ms: u64,
) -> [u8; 4] {
    let Some(selector) = layer.text_color_selector else {
        return layer.base_color;
    };
    let Some(program) = text_animations
        .get(usize::from(selector))
        .copied()
        .flatten()
    else {
        return layer.base_color;
    };
    let rgba = program.sample(animation_elapsed_ms);
    [rgba.r, rgba.g, rgba.b, rgba.a]
}

fn sampled_text_colors(
    page: &ResidentPage,
    text_animations: &[Option<GpuFontColorProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_elapsed_ms: u64,
) -> [Option<[u8; 4]>; TEXT_ANIMATION_COLOR_SLOTS] {
    let mut colors = [None; TEXT_ANIMATION_COLOR_SLOTS];
    for layer in &page.layers {
        let Some(selector) = layer.text_color_selector else {
            continue;
        };
        colors[usize::from(selector)] =
            Some(resident_layer_color(layer, text_animations, animation_elapsed_ms));
    }
    colors
}

fn publish_pixels(
    surface: &GridPaperSurface,
    result: &crate::intel::render::ResidentSceneFrameResult,
) -> Result<(), ServiceError> {
    if result.width != surface.width || result.height != surface.height {
        return Err(ServiceError::InvalidFrame);
    }
    let source = result.rgba.as_deref().ok_or(ServiceError::InvalidFrame)?;
    let source_pitch = surface.width as usize * 4;
    if source.len() < source_pitch * surface.height as usize {
        return Err(ServiceError::InvalidFrame);
    }
    let lease = crate::ui4::acquire_frame_buffer(surface.frame)?;
    let view = match crate::ui4::writable_rgba_view(lease) {
        Ok(view) => view,
        Err(error) => {
            let _ = crate::ui4::cancel_frame_buffer(lease);
            return Err(error.into());
        }
    };
    if view.width != surface.width
        || view.height != surface.height
        || view.pitch < surface.width * 4
    {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(ServiceError::InvalidFrame);
    }
    let destination = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
    for row in 0..surface.height as usize {
        let source_start = row * source_pitch;
        let destination_start = row * view.pitch as usize;
        destination[destination_start..destination_start + source_pitch]
            .copy_from_slice(&source[source_start..source_start + source_pitch]);
    }
    crate::intel::dma_flush(view.virt, view.byte_len);
    if let Err(error) = crate::ui4::publish_frame_buffer(lease) {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(error.into());
    }
    crate::ui4::publish_window_frame(UI4_OWNER, surface.window, crate::ui4::DamageRect::FULL)?;
    Ok(())
}

/// Persistent GridPaper scene consumer. Geometry is rebuilt only for a new
/// accepted snapshot and remains GPU-owned between publications.
#[embassy_executor::task]
pub async fn gridpaper_service_task() {
    crate::intel::wait_hw_logo_sequence_done().await;
    let mut last_init_error = None;
    let surface = loop {
        match initialize_surface() {
            Ok(surface) => break surface,
            Err(error) => {
                if last_init_error != Some(error) {
                    crate::log_warn!(
                        target: "gridpaper";
                        "gridpaper: UI4 surface pending error={:?} action=retry\n",
                        error,
                    );
                    last_init_error = Some(error);
                }
                Timer::after(EmbassyDuration::from_millis(250)).await;
            }
        }
    };
    crate::log_info!(
        target: "gridpaper";
        "gridpaper: embassy service ready page_bytes={} cells={} snapshot_buffers=2 ui4_buffers=2 scene={}x{} ui4={}x{} extent_source={} document_mm={}x{} grid_mm={}x{} surface_mm={}x{} ruler_gutter_mm={} target_cell_mm={} font_px_at_100=24 owner=kernel-app-4 persistent_gpu_scene=1\n",
        PAGE_BYTES,
        COLUMNS * ROWS,
        SCENE_WIDTH,
        SCENE_HEIGHT,
        surface.width,
        surface.height,
        surface.extent_source,
        A4_WIDTH_MM,
        A4_HEIGHT_MM,
        GRID_WIDTH_MM,
        GRID_HEIGHT_MM,
        SURFACE_WIDTH_MM,
        SURFACE_HEIGHT_MM,
        RULER_GUTTER_MM,
        CELL_EDGE_MM,
    );

    let mut observed_serial = 0u64;
    let mut observed_animation_serial = 0u64;
    let mut text_animations = [None; TEXT_ANIMATION_COLOR_SLOTS];
    let mut animation_started_ms = Instant::now().as_millis();
    let mut animation_dirty = false;
    let mut last_sampled_text_colors = [None; TEXT_ANIMATION_COLOR_SLOTS];
    let mut animation_frames = 0u64;
    let mut queued_snapshot: Option<OwnedSnapshot> = None;
    let mut pending: Option<ResidentPage> = None;
    let mut active: Option<ResidentPage> = None;
    let mut last_build_error = None;
    let mut last_render_error = None;

    loop {
        if let Some(update) = text_animations_after(observed_animation_serial) {
            observed_animation_serial = update.serial;
            text_animations = update.programs;
            animation_started_ms = Instant::now().as_millis();
            animation_dirty = true;
            crate::log_info!(
                target: "gridpaper";
                "gridpaper: text-animation-table activated serial={} programs={} cadence_ms={} clock=monotonic-elapsed geometry_uploads=0\n",
                observed_animation_serial,
                text_animations.iter().flatten().count(),
                SERVICE_PERIOD_MS,
            );
        }

        if let Some(snapshot) = snapshot_after(observed_serial) {
            observed_serial = snapshot.serial;
            pending = None;
            queued_snapshot = Some(snapshot);
        }

        if let Some(snapshot) = queued_snapshot.as_ref() {
            match build_resident_page(&snapshot, surface.width, surface.height) {
                Ok(page) => {
                    pending = Some(page);
                    queued_snapshot = None;
                    last_build_error = None;
                }
                Err(error) => {
                    if last_build_error != Some(error) {
                        crate::log_warn!(
                            target: "gridpaper";
                            "gridpaper: snapshot build pending serial={} generation={} reason={} action=retain-front\n",
                            snapshot.serial,
                            snapshot.generation,
                            error,
                        );
                        last_build_error = Some(error);
                    }
                }
            }
        }

        let animation_elapsed_ms = Instant::now()
            .as_millis()
            .saturating_sub(animation_started_ms);
        let mut published_page_this_tick = false;
        if let Some(candidate) = pending.as_ref() {
            match publish_page(&surface, candidate, &text_animations, animation_elapsed_ms) {
                Ok(result) => {
                    let published = pending.take().expect("gridpaper pending page exists");
                    last_sampled_text_colors =
                        sampled_text_colors(&published, &text_animations, animation_elapsed_ms);
                    crate::log_info!(
                        target: "gridpaper";
                        "gridpaper: frame published serial={} generation={} scale={} layers={} changed_pixels={} frame_us={} persistence=resident-until-next-snapshot\n",
                        published.serial,
                        published.generation,
                        published.scale_percent,
                        published.layers.len(),
                        result.changed_pixels,
                        result.frame_us,
                    );
                    let retired = active.replace(published);
                    drop(retired);
                    animation_dirty = false;
                    published_page_this_tick = true;
                    last_render_error = None;
                }
                Err(error) if last_render_error != Some(error) => {
                    crate::log_warn!(
                        target: "gridpaper";
                        "gridpaper: frame pending serial={} generation={} error={:?} action=retain-front-and-retry\n",
                        candidate.serial,
                        candidate.generation,
                        error,
                    );
                    last_render_error = Some(error);
                }
                Err(_) => {}
            }
        }

        if !published_page_this_tick
            && pending.is_none()
            && let Some(page) = active.as_ref()
        {
            let sampled = sampled_text_colors(page, &text_animations, animation_elapsed_ms);
            if animation_dirty || sampled != last_sampled_text_colors {
                match publish_page(&surface, page, &text_animations, animation_elapsed_ms) {
                    Ok(result) => {
                        last_sampled_text_colors = sampled;
                        animation_dirty = false;
                        animation_frames = animation_frames.saturating_add(1);
                        if animation_frames <= 8 || animation_frames.is_multiple_of(120) {
                            crate::log_info!(
                                target: "gridpaper";
                                "gridpaper: text-animation-frame seq={} animation_serial={} elapsed_ms={} programs={} changed_pixels={} frame_us={} geometry_uploads=0 resident_mesh_rebuilds=0\n",
                                animation_frames,
                                observed_animation_serial,
                                animation_elapsed_ms,
                                text_animations.iter().flatten().count(),
                                result.changed_pixels,
                                result.frame_us,
                            );
                        }
                        last_render_error = None;
                    }
                    Err(error) if last_render_error != Some(error) => {
                        crate::log_warn!(
                            target: "gridpaper";
                            "gridpaper: text-animation-frame pending serial={} elapsed_ms={} error={:?} action=retain-front-and-retry\n",
                            observed_animation_serial,
                            animation_elapsed_ms,
                            error,
                        );
                        last_render_error = Some(error);
                    }
                    Err(_) => {}
                }
            }
        }

        Timer::after(EmbassyDuration::from_millis(SERVICE_PERIOD_MS)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_wire_size_matches_a4_gridpaper() {
        assert_eq!(PAGE_BYTES, 39_220);
        assert_eq!((COLUMNS, ROWS), (37, 53));
        assert_eq!(COLUMNS * ROWS, 1_961);
        assert_eq!((A4_WIDTH_MM, A4_HEIGHT_MM), (210, 297));
        assert_eq!(COLUMNS as u32 * CELL_EDGE_MM, 185);
        assert_eq!(ROWS as u32 * CELL_EDGE_MM, 265);
        assert_eq!((GRID_WIDTH_MM, GRID_HEIGHT_MM), (185, 265));
        assert_eq!((SURFACE_WIDTH_MM, SURFACE_HEIGHT_MM), (189, 269));
        assert_eq!((SCENE_WIDTH, SCENE_HEIGHT), (189, 269));
    }

    #[test]
    fn axis_ticks_mark_half_centimeters_centimeters_and_three_centimeters() {
        assert_eq!(axis_tick_length_mm(0), THREE_CENTIMETER_TICK_LENGTH_MM);
        assert_eq!(axis_tick_length_mm(1), SMALL_TICK_LENGTH_MM);
        assert_eq!(axis_tick_length_mm(2), CENTIMETER_TICK_LENGTH_MM);
        assert_eq!(axis_tick_length_mm(6), THREE_CENTIMETER_TICK_LENGTH_MM);
    }

    #[test]
    fn validator_rejects_non_utf8_and_unknown_style_bits() {
        let mut raw = [0u8; PAGE_BYTES];
        assert_eq!(validate_page(&raw), Ok(()));
        raw[0] = 1;
        raw[TEXT_OFFSET] = 0xff;
        assert_eq!(validate_page(&raw), Err(()));
        raw[0] = 0;
        raw[3] = 0x80;
        assert_eq!(validate_page(&raw), Err(()));
    }

    #[test]
    fn css_keyframe_wire_decodes_and_samples_without_geometry_state() {
        let mut wire = Vec::from([1, 1, 0, 0]);
        wire.extend_from_slice(&[
            13, // BrightBlue selector.
            GpuFontColorChannels::RGB.bits(),
            0, // linear
            1, // loop
        ]);
        wire.extend_from_slice(&1_000u32.to_le_bytes());
        wire.extend_from_slice(&[3, 0, 0, 0]);
        for (offset, rgba) in [
            (0u16, [255, 0, 0, 255]),
            (500u16, [0, 255, 0, 255]),
            (1_000u16, [255, 0, 0, 255]),
        ] {
            wire.extend_from_slice(&offset.to_le_bytes());
            wire.extend_from_slice(&[0, 0]);
            wire.extend_from_slice(&rgba);
        }

        let programs = decode_text_animations(&wire).expect("valid keyframe wire");
        let program = programs[13].expect("selector installed");
        assert_eq!(program.sample(0), GpuFontRgba::new(255, 0, 0, 255));
        assert_eq!(program.sample(250), GpuFontRgba::new(128, 128, 0, 255));
        assert_eq!(program.sample(500), GpuFontRgba::new(0, 255, 0, 255));
        assert_eq!(program.sample(1_000), GpuFontRgba::new(255, 0, 0, 255));
    }
}
