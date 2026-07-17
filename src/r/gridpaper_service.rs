//! Embassy consumer for GridPaper's fixed snapshot format.
//!
//! The Blueprint owns edits and publication cadence. This service owns the
//! accepted copy, UI4 surface, GPU allocations, and presentation lifetime.
//! No UI4 handles or generic drawing operations cross the ABI.

use alloc::{string::String, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

const COLUMNS: usize = 21;
const ROWS: usize = 30;
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
const MIN_SCALE_PERCENT: u32 = 1;
const MAX_SCALE_PERCENT: u32 = 800;

const VIEWPORT_WIDTH: u32 = 700;
const VIEWPORT_HEIGHT: u32 = 990;
const UI4_OWNER: crate::ui4::WindowOwner = crate::ui4::WindowOwner::KernelApp(4);
const SERVICE_PERIOD_MS: u64 = 16;

const ERROR_INVALID_SNAPSHOT: i32 = -1;
const ERROR_INVALID_SCALE: i32 = -2;
const ERROR_NOT_OWNER: i32 = -3;
const ERROR_TRANSPORT: i32 = -4;

struct SnapshotStore {
    buffers: [[u8; PAGE_BYTES]; 2],
    published: usize,
    owner: Option<u8>,
    generation: u64,
    scale_percent: u16,
    serial: u64,
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
pub extern "C" fn trueos_cabi_gridpaper_close() -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_GRIDPAPER_CLOSE,
            0,
            0,
        );
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
    let output = crate::ui4::OutputId::from_slot(0).expect("UI4 D01 must exist");
    let frame = crate::ui4::create_frame(crate::ui4::FrameSpec {
        output,
        content: crate::ui4::FrameContent::RenderScene3d,
        cadence: crate::ui4::FrameCadence::Dirty,
        format: crate::ui4::ScanoutFormat::Rgba8888Premultiplied,
        width: VIEWPORT_WIDTH,
        height: VIEWPORT_HEIGHT,
        base_color: Some(crate::ui4::PremultipliedRgba8::TRANSPARENT),
    })?;
    let session = match crate::ui4::begin_window_session(UI4_OWNER) {
        Ok(session) => session,
        Err(error) => {
            let _ = crate::ui4::destroy_frame(frame);
            return Err(error.into());
        }
    };
    let (scanout_width, scanout_height) = crate::intel::active_scanout_dimensions()
        .unwrap_or((VIEWPORT_WIDTH, VIEWPORT_HEIGHT));
    let window = match crate::ui4::create_window(crate::ui4::WindowCreate {
        owner: UI4_OWNER,
        session,
        frame,
        output,
        plane: crate::ui4::WindowPlane::Universal(crate::ui4::RGB_OVERLAY_PLANE_SLOT_3 as u8),
        placement: crate::ui4::WindowPlacement {
            x: (scanout_width.saturating_sub(VIEWPORT_WIDTH) / 2) as i32,
            y: (scanout_height.saturating_sub(VIEWPORT_HEIGHT) / 2) as i32,
            width: VIEWPORT_WIDTH,
            height: VIEWPORT_HEIGHT,
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
    Ok(GridPaperSurface { frame, window })
}

struct ResidentLayer {
    color: [u8; 4],
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
        x * 2.0 / VIEWPORT_WIDTH as f32 - 1.0,
        1.0 - y * 2.0 / VIEWPORT_HEIGHT as f32,
        z,
    ]
}

struct TextCell {
    text: String,
    color: u8,
    x: f32,
    baseline: f32,
    font_pixels: f32,
    bold: bool,
}

fn build_resident_page(snapshot: &OwnedSnapshot) -> Result<ResidentPage, &'static str> {
    use crate::intel::gpu_font::{
        GpuFontFace, GpuFontJobEntry, GpuFontTextRequest, create_resident_font_scene_mesh,
        ensure_font_face_available,
    };

    ensure_font_face_available(GpuFontFace::Default)?;
    let mut layers = Vec::new();

    let mut paper = Geometry::new();
    paper.quad(0.0, 0.0, VIEWPORT_WIDTH as f32, VIEWPORT_HEIGHT as f32, 0.9);
    push_geometry_layer(&mut layers, paper, palette(COLOR_DEFAULT, true))?;

    let mut backgrounds: Vec<(u8, Geometry)> = Vec::new();
    let mut decorations: Vec<(u8, Geometry)> = Vec::new();
    let mut texts = Vec::new();
    let cell_width = VIEWPORT_WIDTH as f32 / COLUMNS as f32;
    let millimeter = VIEWPORT_HEIGHT as f32 / 297.0;
    let scale = f32::from(snapshot.scale_percent) / 100.0;

    for row in 0..ROWS {
        let top_mm = (row * 10) as f32;
        let bottom_mm = if row + 1 == ROWS { 297.0 } else { top_mm + 10.0 };
        let top = top_mm * millimeter;
        let bottom = bottom_mm * millimeter;
        let cell_height = bottom - top;
        for column in 0..COLUMNS {
            let offset = (row * COLUMNS + column) * CELL_BYTES;
            let cell = &snapshot.raw[offset..offset + CELL_BYTES];
            let text_len = usize::from(cell[0]);
            let foreground = cell[1];
            let background = cell[2];
            let style = cell[3];
            let left = column as f32 * cell_width;
            let right = left + cell_width;

            if background != COLOR_DEFAULT && background != COLOR_TRANSPARENT {
                geometry_for_color(&mut backgrounds, background)
                    .quad(left + 0.5, top + 0.5, right - 0.5, bottom - 0.5, 0.8);
            }

            if foreground == COLOR_TRANSPARENT || text_len == 0 {
                continue;
            }
            let text = core::str::from_utf8(&cell[TEXT_OFFSET..TEXT_OFFSET + text_len])
                .map_err(|_| "gridpaper-utf8")?;
            let font_pixels = (cell_height * 0.58 * scale).clamp(1.0, 256.0);
            let baseline = top + cell_height * 0.72;
            texts.push(TextCell {
                text: String::from(text),
                color: foreground,
                x: left + (2.5 * scale).min(cell_width * 0.2),
                baseline,
                font_pixels,
                bold: style & STYLE_BOLD != 0,
            });
            if style & STYLE_UNDERLINE != 0 {
                let thickness = (font_pixels / 14.0).max(1.0);
                geometry_for_color(&mut decorations, foreground).quad(
                    left + 2.0,
                    baseline + thickness,
                    right - 2.0,
                    baseline + thickness * 2.0,
                    0.4,
                );
            }
            if style & STYLE_STRIKEOUT != 0 {
                let thickness = (font_pixels / 14.0).max(1.0);
                let y = baseline - font_pixels * 0.32;
                geometry_for_color(&mut decorations, foreground).quad(
                    left + 2.0,
                    y,
                    right - 2.0,
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
    let line = 1.0;
    for column in 0..=COLUMNS {
        let x = column as f32 * cell_width;
        grid.quad(
            (x - line * 0.5).max(0.0),
            0.0,
            (x + line * 0.5).min(VIEWPORT_WIDTH as f32),
            VIEWPORT_HEIGHT as f32,
            0.6,
        );
    }
    for row in 0..=ROWS {
        let y = if row == ROWS {
            VIEWPORT_HEIGHT as f32
        } else {
            (row * 10) as f32 * millimeter
        };
        grid.quad(
            0.0,
            (y - line * 0.5).max(0.0),
            VIEWPORT_WIDTH as f32,
            (y + line * 0.5).min(VIEWPORT_HEIGHT as f32),
            0.6,
        );
    }
    push_geometry_layer(&mut layers, grid, [188, 205, 224, 255])?;

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
                position: [cell.x, cell.baseline],
                font_pixels: cell.font_pixels,
            });
            if cell.bold {
                entries.push(GpuFontJobEntry {
                    text: GpuFontTextRequest::SingleLine(cell.text.as_str()),
                    position: [cell.x + 0.75, cell.baseline],
                    font_pixels: cell.font_pixels,
                });
            }
        }
        let mesh = create_resident_font_scene_mesh(
            &entries,
            GpuFontFace::Default,
            VIEWPORT_WIDTH,
            VIEWPORT_HEIGHT,
        )?;
        layers.push(ResidentLayer {
            color: palette(color, false),
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
    let mesh = crate::intel::render::create_resident_triangle_mesh(
        &geometry.vertices,
        &geometry.indices,
    )?;
    layers.push(ResidentLayer { color, mesh });
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
) -> Result<crate::intel::render::ResidentSceneFrameResult, ServiceError> {
    let draws = page
        .layers
        .iter()
        .map(|layer| crate::intel::render::ResidentSceneDraw {
            mesh: &layer.mesh,
            rgba: layer.color,
        })
        .collect::<Vec<_>>();
    let result = crate::intel::render::capture_resident_triangle_scene_frame_premultiplied_at_extent(
        &draws,
        Some([0, 0, 0, 0]),
        VIEWPORT_WIDTH,
        VIEWPORT_HEIGHT,
        false,
    )
    .map_err(ServiceError::Render)?;
    if result.completed_draws != result.requested_draws || result.rgba.is_none() {
        return Err(ServiceError::Render("incomplete-frame"));
    }
    publish_pixels(surface, &result)?;
    Ok(result)
}

fn publish_pixels(
    surface: &GridPaperSurface,
    result: &crate::intel::render::ResidentSceneFrameResult,
) -> Result<(), ServiceError> {
    if result.width != VIEWPORT_WIDTH || result.height != VIEWPORT_HEIGHT {
        return Err(ServiceError::InvalidFrame);
    }
    let source = result.rgba.as_deref().ok_or(ServiceError::InvalidFrame)?;
    let source_pitch = VIEWPORT_WIDTH as usize * 4;
    if source.len() < source_pitch * VIEWPORT_HEIGHT as usize {
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
    if view.width != VIEWPORT_WIDTH
        || view.height != VIEWPORT_HEIGHT
        || view.pitch < VIEWPORT_WIDTH * 4
    {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(ServiceError::InvalidFrame);
    }
    let destination = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
    for row in 0..VIEWPORT_HEIGHT as usize {
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
        "gridpaper: embassy service ready page_bytes={} cells={} snapshot_buffers=2 ui4_buffers=2 extent={}x{} owner=kernel-app-4 persistent_gpu_scene=1\n",
        PAGE_BYTES,
        COLUMNS * ROWS,
        VIEWPORT_WIDTH,
        VIEWPORT_HEIGHT,
    );

    let mut observed_serial = 0u64;
    let mut pending: Option<ResidentPage> = None;
    let mut active: Option<ResidentPage> = None;
    let mut last_build_error = None;
    let mut last_render_error = None;

    loop {
        if let Some(snapshot) = snapshot_after(observed_serial) {
            observed_serial = snapshot.serial;
            match build_resident_page(&snapshot) {
                Ok(page) => {
                    pending = Some(page);
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

        if let Some(candidate) = pending.as_ref() {
            match publish_page(&surface, candidate) {
                Ok(result) => {
                    let published = pending.take().expect("gridpaper pending page exists");
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
                    active = Some(published);
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

        let _ = active.as_ref();
        Timer::after(EmbassyDuration::from_millis(SERVICE_PERIOD_MS)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_wire_size_matches_a4_gridpaper() {
        assert_eq!(PAGE_BYTES, 12_600);
        assert_eq!(COLUMNS * ROWS, 630);
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
}

