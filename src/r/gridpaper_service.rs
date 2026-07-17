//! Embassy consumer for GridPaper's fixed snapshot format.
//!
//! The Blueprint owns snapshot publication cadence. This service owns the
//! accepted working copy, UI4 editing/focus state, GPU allocations, and
//! presentation lifetime. No UI4 handles or generic drawing operations cross
//! the ABI.

use alloc::{collections::VecDeque, string::String, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

use crate::intel::gpu_font::{
    GPU_FONT_COLOR_KEYFRAME_CAPACITY, GpuFontColorChannels, GpuFontColorIteration,
    GpuFontColorKeyframe, GpuFontColorKeyframes, GpuFontColorProgram, GpuFontColorTiming,
    GpuFontFace, GpuFontRgba,
};

const COLUMNS: usize = 37;
const ROWS: usize = 53;
const GLYPH_UTF8_CAPACITY: usize = 4;
const CELL_BYTES: usize = 13;
const PAGE_BYTES: usize = COLUMNS * ROWS * CELL_BYTES;
const PRIMARY_LENGTH_OFFSET: usize = 0;
const UPPER_LENGTH_OFFSET: usize = 1;
const FOREGROUND_OFFSET: usize = 2;
const BACKGROUND_OFFSET: usize = 3;
const STYLE_OFFSET: usize = 4;
const PRIMARY_OFFSET: usize = 5;
const UPPER_OFFSET: usize = PRIMARY_OFFSET + GLYPH_UTF8_CAPACITY;
const VALID_STYLE_BITS: u8 = 0x0f;
const STYLE_BOLD: u8 = 1 << 0;
const STYLE_STRIKEOUT: u8 = 1 << 1;
const STYLE_UNDERLINE: u8 = 1 << 2;
const STYLE_ITALIC: u8 = 1 << 3;
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
const DECORATION_INSET_MM: f32 = 0.5;
const UI4_OWNER: crate::ui4::WindowOwner = crate::ui4::WindowOwner::KernelApp(4);
const SERVICE_PERIOD_MS: u64 = 16;
const PRIMARY_BUTTON_MASK: u32 = 1 << 0;
const GRID_CURSOR_STROKE_PX: u32 = 3;
const GRID_CURSOR_RGBA: [u8; 4] = [255, 96, 32, 255];
const PRINT_REQUEST_CAPACITY: usize = 8;
const PRINT_CAPTURE_HEIGHT: u32 = 1_440;
const PRINT_CAPTURE_WIDTH: u32 =
    (PRINT_CAPTURE_HEIGHT * SURFACE_WIDTH_MM + SURFACE_HEIGHT_MM / 2) / SURFACE_HEIGHT_MM;

const ERROR_INVALID_SNAPSHOT: i32 = -1;
const ERROR_INVALID_SCALE: i32 = -2;
const ERROR_NOT_OWNER: i32 = -3;
const ERROR_TRANSPORT: i32 = -4;
const ERROR_INVALID_ANIMATION: i32 = -5;

struct SnapshotStore {
    buffers: [[u8; PAGE_BYTES]; 2],
    published: usize,
    owner: Option<u8>,
    producer_connected: bool,
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
            producer_connected: false,
            generation: 0,
            scale_percent: 100,
            serial: 0,
            text_animations: [None; TEXT_ANIMATION_COLOR_SLOTS],
            animation_serial: 0,
        }
    }
}

static SNAPSHOTS: Mutex<SnapshotStore> = Mutex::new(SnapshotStore::new());

fn producer_ownership_conflicts(
    active: Option<u8>,
    producer_connected: bool,
    requester: u8,
) -> bool {
    producer_connected
        && active.is_some_and(|active| active != requester && crate::hv::vm_state(active).running)
}

#[derive(Clone)]
struct OwnedSnapshot {
    raw: Vec<u8>,
    owner: u8,
    generation: u64,
    scale_percent: u16,
    serial: u64,
}

struct GridPaperPrintRequest {
    owner: u8,
    token: u32,
    generation: u64,
    raw: Vec<u8>,
}

struct PrintRenderRequest {
    job_id: u32,
    generation: u64,
    raw: Vec<u8>,
}

pub(crate) struct PrintRasterFrame {
    pub width: u32,
    pub height: u32,
    pub rgba_premultiplied: Vec<u8>,
}

pub(crate) struct PrintRenderResult {
    pub job_id: u32,
    pub result: Result<PrintRasterFrame, &'static str>,
}

static NEXT_PRINT_REQUEST_TOKEN: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(1);
static GRIDPAPER_PRINT_REQUESTS: Mutex<VecDeque<GridPaperPrintRequest>> =
    Mutex::new(VecDeque::new());
static PRINT_RENDER_REQUESTS: Mutex<VecDeque<PrintRenderRequest>> = Mutex::new(VecDeque::new());
static PRINT_RENDER_RESULTS: Mutex<VecDeque<PrintRenderResult>> = Mutex::new(VecDeque::new());

fn next_print_request_token() -> u32 {
    use core::sync::atomic::Ordering;

    loop {
        let token = NEXT_PRINT_REQUEST_TOKEN.fetch_add(1, Ordering::Relaxed);
        if token != 0 {
            return token;
        }
    }
}

fn queue_print_request(snapshot: &OwnedSnapshot) -> Option<u32> {
    let token = next_print_request_token();
    let mut requests = GRIDPAPER_PRINT_REQUESTS.lock();
    if requests.len() >= PRINT_REQUEST_CAPACITY {
        return None;
    }
    requests.push_back(GridPaperPrintRequest {
        owner: snapshot.owner,
        token,
        generation: snapshot.generation,
        raw: snapshot.raw.clone(),
    });
    drop(requests);
    crate::log_os::gridpaper_print_requested(snapshot.owner, token, snapshot.generation);
    Some(token)
}

pub(crate) fn take_print_request_for_owner(owner: u8) -> Option<(u32, u64)> {
    let requests = GRIDPAPER_PRINT_REQUESTS.lock();
    let request = requests.iter().find(|request| request.owner == owner)?;
    Some((request.token, request.generation))
}

pub(crate) fn consume_print_request(owner: u8, token: u32) -> Option<(u64, Vec<u8>)> {
    let mut requests = GRIDPAPER_PRINT_REQUESTS.lock();
    let index = requests
        .iter()
        .position(|request| request.owner == owner && request.token == token)?;
    let request = requests.remove(index)?;
    Some((request.generation, request.raw))
}

pub(crate) fn valid_print_snapshot(raw: &[u8]) -> bool {
    raw.len() == PAGE_BYTES && validate_page(raw).is_ok()
}

pub(crate) fn request_print_render(job_id: u32, generation: u64, raw: Vec<u8>) -> bool {
    if !valid_print_snapshot(&raw) {
        return false;
    }
    let mut requests = PRINT_RENDER_REQUESTS.lock();
    if requests.len() >= PRINT_REQUEST_CAPACITY {
        return false;
    }
    requests.push_back(PrintRenderRequest {
        job_id,
        generation,
        raw,
    });
    true
}

pub(crate) fn take_print_render_result(job_id: u32) -> Option<PrintRenderResult> {
    let mut results = PRINT_RENDER_RESULTS.lock();
    let index = results.iter().position(|result| result.job_id == job_id)?;
    results.remove(index)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GridCellSelection {
    column: usize,
    row: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CellInputField {
    #[default]
    Primary,
    Upper,
}

impl CellInputField {
    const fn toggled(self) -> Self {
        match self {
            Self::Primary => Self::Upper,
            Self::Upper => Self::Primary,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Upper => "upper",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct KeyboardGridOutcome {
    content_changed: bool,
    selection_changed: bool,
    input_field_changed: bool,
    clear_selection: bool,
    capacity_rejected: bool,
    edited_cell: Option<GridCellSelection>,
}

fn edit_snapshot_from_keyboard(
    snapshot: &mut OwnedSnapshot,
    selection: &mut GridCellSelection,
    input_field: &mut CellInputField,
    event: crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> KeyboardGridOutcome {
    let mut outcome = KeyboardGridOutcome::default();
    if event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_TEXT {
        let utf8_len = usize::from(event.utf8_len);
        if utf8_len == 0
            || utf8_len > event.utf8.len()
            || event.codepoint < 0x20
            || event.codepoint == 0x7f
            || core::str::from_utf8(&event.utf8[..utf8_len])
                .ok()
                .is_none_or(|glyph| glyph.chars().count() != 1)
        {
            return outcome;
        }
        let offset = (selection.row * COLUMNS + selection.column) * CELL_BYTES;
        let cell = &mut snapshot.raw[offset..offset + CELL_BYTES];
        if *input_field == CellInputField::Upper && cell[PRIMARY_LENGTH_OFFSET] == 0 {
            outcome.capacity_rejected = true;
            return outcome;
        }
        let edited_cell = *selection;
        write_cell_glyph(cell, *input_field, &event.utf8[..utf8_len]);
        if cell[FOREGROUND_OFFSET] == COLOR_TRANSPARENT {
            cell[FOREGROUND_OFFSET] = COLOR_DEFAULT;
        }
        outcome.content_changed = true;
        outcome.edited_cell = Some(edited_cell);
        if *input_field == CellInputField::Primary {
            let linear = selection
                .row
                .saturating_mul(COLUMNS)
                .saturating_add(selection.column);
            let next_linear = linear.saturating_add(1).min(COLUMNS * ROWS - 1);
            let next = GridCellSelection {
                column: next_linear % COLUMNS,
                row: next_linear / COLUMNS,
            };
            outcome.selection_changed = next != *selection;
            *selection = next;
        }
        return outcome;
    }

    if event.kind != crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY {
        return outcome;
    }
    match event.key_code {
        crate::r::keyboard::KEYBOARD_KEY_BACKSPACE | crate::r::keyboard::KEYBOARD_KEY_DELETE => {
            let offset = (selection.row * COLUMNS + selection.column) * CELL_BYTES;
            let cell = &mut snapshot.raw[offset..offset + CELL_BYTES];
            let had_content = match *input_field {
                CellInputField::Primary => {
                    cell[PRIMARY_LENGTH_OFFSET] != 0 || cell[UPPER_LENGTH_OFFSET] != 0
                }
                CellInputField::Upper => cell[UPPER_LENGTH_OFFSET] != 0,
            };
            if had_content {
                clear_cell_glyph(cell, *input_field);
                if *input_field == CellInputField::Primary {
                    clear_cell_glyph(cell, CellInputField::Upper);
                }
                outcome.content_changed = true;
                outcome.edited_cell = Some(*selection);
            }
        }
        crate::r::keyboard::KEYBOARD_KEY_ARROW_LEFT => {
            let next = selection.column.saturating_sub(1);
            outcome.selection_changed = next != selection.column;
            selection.column = next;
        }
        crate::r::keyboard::KEYBOARD_KEY_ARROW_RIGHT => {
            let next = selection.column.saturating_add(1).min(COLUMNS - 1);
            outcome.selection_changed = next != selection.column;
            selection.column = next;
        }
        crate::r::keyboard::KEYBOARD_KEY_ARROW_UP => {
            let next = selection.row.saturating_sub(1);
            outcome.selection_changed = next != selection.row;
            selection.row = next;
        }
        crate::r::keyboard::KEYBOARD_KEY_ARROW_DOWN | crate::r::keyboard::KEYBOARD_KEY_ENTER => {
            let next = selection.row.saturating_add(1).min(ROWS - 1);
            outcome.selection_changed = next != selection.row;
            selection.row = next;
        }
        crate::r::keyboard::KEYBOARD_KEY_TAB => {
            *input_field = input_field.toggled();
            outcome.input_field_changed = true;
        }
        crate::r::keyboard::KEYBOARD_KEY_HOME => {
            outcome.selection_changed = selection.column != 0;
            selection.column = 0;
        }
        crate::r::keyboard::KEYBOARD_KEY_END => {
            outcome.selection_changed = selection.column != COLUMNS - 1;
            selection.column = COLUMNS - 1;
        }
        crate::r::keyboard::KEYBOARD_KEY_ESCAPE => outcome.clear_selection = true,
        _ => {}
    }
    outcome
}

fn glyph_offsets(input_field: CellInputField) -> (usize, usize) {
    match input_field {
        CellInputField::Primary => (PRIMARY_LENGTH_OFFSET, PRIMARY_OFFSET),
        CellInputField::Upper => (UPPER_LENGTH_OFFSET, UPPER_OFFSET),
    }
}

fn write_cell_glyph(cell: &mut [u8], input_field: CellInputField, encoded: &[u8]) {
    debug_assert!(encoded.len() <= GLYPH_UTF8_CAPACITY);
    let (length_offset, glyph_offset) = glyph_offsets(input_field);
    cell[glyph_offset..glyph_offset + GLYPH_UTF8_CAPACITY].fill(0);
    cell[glyph_offset..glyph_offset + encoded.len()].copy_from_slice(encoded);
    cell[length_offset] = encoded.len() as u8;
}

fn clear_cell_glyph(cell: &mut [u8], input_field: CellInputField) {
    let (length_offset, glyph_offset) = glyph_offsets(input_field);
    cell[length_offset] = 0;
    cell[glyph_offset..glyph_offset + GLYPH_UTF8_CAPACITY].fill(0);
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ScenePan {
    x: f32,
    y: f32,
}

impl ScenePan {
    const ZERO: Self = Self { x: 0.0, y: 0.0 };

    fn clamped(self, scale_percent: u16) -> Self {
        let scale = f32::from(scale_percent) / 100.0;
        let min_x = (SCENE_WIDTH as f32 * (1.0 - scale)).min(0.0);
        let min_y = (SCENE_HEIGHT as f32 * (1.0 - scale)).min(0.0);
        Self {
            x: self.x.clamp(min_x, 0.0),
            y: self.y.clamp(min_y, 0.0),
        }
    }

    fn drag_pixels(
        &mut self,
        dx: i32,
        dy: i32,
        raster_width: u32,
        raster_height: u32,
        scale_percent: u16,
    ) -> bool {
        let previous = *self;
        self.x += dx as f32 * SCENE_WIDTH as f32 / raster_width.max(1) as f32;
        self.y += dy as f32 * SCENE_HEIGHT as f32 / raster_height.max(1) as f32;
        *self = self.clamped(scale_percent);
        *self != previous
    }
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
    if producer_ownership_conflicts(snapshots.owner, snapshots.producer_connected, owner) {
        return ERROR_NOT_OWNER;
    }
    let next = snapshots.published ^ 1;
    snapshots.buffers[next].copy_from_slice(raw);
    snapshots.published = next;
    snapshots.owner = Some(owner);
    snapshots.producer_connected = true;
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
    if producer_ownership_conflicts(snapshots.owner, snapshots.producer_connected, owner) {
        return ERROR_NOT_OWNER;
    }
    snapshots.owner = Some(owner);
    snapshots.producer_connected = true;
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

/// Relinquish producer authority. The service releases its UI4 presentation,
/// while the kernel-owned frame and last persistent scene remain resident.
pub(crate) fn close_owner(owner: u8) -> i32 {
    let mut snapshots = SNAPSHOTS.lock();
    match snapshots.owner {
        Some(active) if active == owner => {
            snapshots.owner = None;
            snapshots.producer_connected = false;
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
        owner: snapshots.owner?,
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

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_gridpaper_print_request_take() -> u64 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_GRIDPAPER_PRINT_REQUEST_TAKE, 0, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            data
        } else {
            0
        };
    }
    let Some(owner) = crate::hv::current_guest_execution_context_vm_id() else {
        return 0;
    };
    take_print_request_for_owner(owner)
        .map(|(token, _generation)| u64::from(token))
        .unwrap_or(0)
}

fn validate_page(raw: &[u8]) -> Result<(), ()> {
    if raw.len() != PAGE_BYTES {
        return Err(());
    }
    for cell in raw.chunks_exact(CELL_BYTES) {
        let primary_len = usize::from(cell[PRIMARY_LENGTH_OFFSET]);
        let upper_len = usize::from(cell[UPPER_LENGTH_OFFSET]);
        if primary_len > GLYPH_UTF8_CAPACITY
            || upper_len > GLYPH_UTF8_CAPACITY
            || (primary_len == 0 && upper_len != 0)
            || cell[FOREGROUND_OFFSET] >= COLOR_COUNT as u8
            || cell[BACKGROUND_OFFSET] >= COLOR_COUNT as u8
            || cell[STYLE_OFFSET] & !VALID_STYLE_BITS != 0
            || !valid_single_glyph(&cell[PRIMARY_OFFSET..PRIMARY_OFFSET + primary_len])
            || !valid_single_glyph(&cell[UPPER_OFFSET..UPPER_OFFSET + upper_len])
        {
            return Err(());
        }
    }
    Ok(())
}

fn valid_single_glyph(encoded: &[u8]) -> bool {
    core::str::from_utf8(encoded).is_ok_and(|glyph| glyph.is_empty() || glyph.chars().count() == 1)
}

#[derive(Copy, Clone)]
struct GridPaperPresentation {
    producer: u8,
    session: crate::ui4::WindowSessionId,
    window: crate::ui4::WindowId,
}

struct GridPaperSurface {
    frame: crate::ui4::FrameHandle,
    presentation: Option<GridPaperPresentation>,
    width: u32,
    height: u32,
    extent_source: &'static str,
}

fn grid_cell_at_local_point(
    surface: &GridPaperSurface,
    local_x: i32,
    local_y: i32,
    scale_percent: u16,
    pan: ScenePan,
) -> Option<GridCellSelection> {
    if local_x < 0
        || local_y < 0
        || local_x >= surface.width as i32
        || local_y >= surface.height as i32
    {
        return None;
    }
    let scale = f32::from(scale_percent) / 100.0;
    let scene_x = local_x as f32 * SCENE_WIDTH as f32 / surface.width.max(1) as f32 - pan.x;
    let scene_y = local_y as f32 * SCENE_HEIGHT as f32 / surface.height.max(1) as f32 - pan.y;
    let scene_units_per_mm_x = SCENE_WIDTH as f32 / SURFACE_WIDTH_MM as f32;
    let scene_units_per_mm_y = SCENE_HEIGHT as f32 / SURFACE_HEIGHT_MM as f32;
    let grid_left = RULER_GUTTER_MM as f32 * scene_units_per_mm_x * scale;
    let grid_top = RULER_GUTTER_MM as f32 * scene_units_per_mm_y * scale;
    let cell_width = CELL_EDGE_MM as f32 * scene_units_per_mm_x * scale;
    let cell_height = CELL_EDGE_MM as f32 * scene_units_per_mm_y * scale;
    let grid_right = grid_left + COLUMNS as f32 * cell_width;
    let grid_bottom = grid_top + ROWS as f32 * cell_height;
    if scene_x < grid_left || scene_y < grid_top || scene_x >= grid_right || scene_y >= grid_bottom
    {
        return None;
    }
    Some(GridCellSelection {
        column: ((scene_x - grid_left) / cell_width) as usize,
        row: ((scene_y - grid_top) / cell_height) as usize,
    })
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
    Ok(GridPaperSurface {
        frame,
        presentation: None,
        width,
        height,
        extent_source,
    })
}

fn connected_owner() -> Option<u8> {
    let mut snapshots = SNAPSHOTS.lock();
    let owner = snapshots.owner?;
    if !snapshots.producer_connected {
        return None;
    }
    if crate::hv::vm_state(owner).running {
        Some(owner)
    } else {
        snapshots.producer_connected = false;
        None
    }
}

fn attach_presentation(
    surface: &mut GridPaperSurface,
    producer: u8,
    expose_retained_front: bool,
) -> Result<GridPaperPresentation, ServiceError> {
    let (grid_width, grid_height) =
        crate::intel::physical_extent_pixels(GRID_WIDTH_MM, GRID_HEIGHT_MM)
            .unwrap_or((GRID_WIDTH_MM, GRID_HEIGHT_MM));
    let output = crate::ui4::OutputId::from_slot(0).expect("UI4 D01 must exist");
    let session = crate::ui4::begin_window_session(UI4_OWNER)?;
    let (scanout_width, scanout_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((surface.width, surface.height));
    // Keep the useful grid centered. The surface itself extends only far
    // enough above and to the left to carry the two ruler axes.
    let x = scanout_width
        .saturating_sub(grid_width)
        .saturating_div(2)
        .saturating_sub(surface.width.saturating_sub(grid_width));
    let y = scanout_height
        .saturating_sub(grid_height)
        .saturating_div(2)
        .saturating_sub(surface.height.saturating_sub(grid_height));
    let window = match crate::ui4::create_window(crate::ui4::WindowCreate {
        owner: UI4_OWNER,
        session,
        frame: surface.frame,
        output,
        plane: crate::ui4::WindowPlane::Universal(crate::ui4::RGB_OVERLAY_PLANE_SLOT_3 as u8),
        placement: crate::ui4::WindowPlacement {
            x: x as i32,
            y: y as i32,
            width: surface.width,
            height: surface.height,
            z: 70,
            opacity: u8::MAX,
            visible: true,
        },
    }) {
        Ok(window) => window,
        Err(error) => {
            let _ = crate::ui4::finish_window_session(UI4_OWNER, session);
            return Err(error.into());
        }
    };

    if expose_retained_front
        && let Err(error) =
            crate::ui4::publish_window_frame(UI4_OWNER, window, crate::ui4::DamageRect::FULL)
    {
        let _ = crate::ui4::finish_window_session(UI4_OWNER, session);
        return Err(error.into());
    }

    let presentation = GridPaperPresentation {
        producer,
        session,
        window,
    };
    surface.presentation = Some(presentation);
    Ok(presentation)
}

fn release_presentation(surface: &mut GridPaperSurface) -> Option<GridPaperPresentation> {
    let presentation = surface.presentation.take()?;
    match crate::ui4::finish_window_session(UI4_OWNER, presentation.session) {
        Ok(closed_windows) => crate::log_info!(
            target: "gridpaper";
            "gridpaper: presentation released producer={} session={} window={} frame={} closed_windows={} retained_gpu_scene=1 retained_frame=1\n",
            presentation.producer,
            presentation.session.raw(),
            presentation.window.raw(),
            surface.frame.raw(),
            closed_windows,
        ),
        Err(error) => crate::log_warn!(
            target: "gridpaper";
            "gridpaper: presentation release producer={} session={} window={} frame={} error={:?} action=consider-detached retained_gpu_scene=1 retained_frame=1\n",
            presentation.producer,
            presentation.session.raw(),
            presentation.window.raw(),
            surface.frame.raw(),
            error,
        ),
    }
    Some(presentation)
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
    pan: ScenePan,
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
    font: GpuFontFace,
    color: u8,
    center_x: f32,
    center_y: f32,
    font_pixels: f32,
    bold: bool,
    italic: bool,
}

fn font_for_glyph(glyph: &str) -> GpuFontFace {
    if crate::intel::gpu_font::font_face_supports_text(GpuFontFace::Inconsolata, glyph) {
        GpuFontFace::Inconsolata
    } else if crate::intel::gpu_font::font_face_supports_text(GpuFontFace::NotoSansSc, glyph) {
        GpuFontFace::NotoSansSc
    } else {
        GpuFontFace::Default
    }
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
    pan: ScenePan,
) -> Result<ResidentPage, &'static str> {
    use crate::intel::gpu_font::{
        GpuFontJobEntry, GpuFontTextRequest, create_resident_font_centered_scene_mesh_at_raster,
        ensure_font_face_available,
    };

    ensure_font_face_available(GpuFontFace::Default)?;
    ensure_font_face_available(GpuFontFace::Inconsolata)?;
    ensure_font_face_available(GpuFontFace::NotoSansSc)?;
    let mut layers = Vec::new();

    let mut backgrounds: Vec<(u8, Geometry)> = Vec::new();
    let mut decorations: Vec<(u8, Geometry)> = Vec::new();
    let mut texts = Vec::new();
    let scale = f32::from(snapshot.scale_percent) / 100.0;
    let scene_units_per_mm_x = SCENE_WIDTH as f32 / SURFACE_WIDTH_MM as f32;
    let scene_units_per_mm_y = SCENE_HEIGHT as f32 / SURFACE_HEIGHT_MM as f32;
    let cell_width = CELL_EDGE_MM as f32 * scene_units_per_mm_x * scale;
    let cell_height = CELL_EDGE_MM as f32 * scene_units_per_mm_y * scale;
    let grid_width = COLUMNS as f32 * cell_width;
    let grid_height = ROWS as f32 * cell_height;
    let pan = pan.clamped(snapshot.scale_percent);
    let grid_left = RULER_GUTTER_MM as f32 * scene_units_per_mm_x * scale;
    let grid_top = RULER_GUTTER_MM as f32 * scene_units_per_mm_y * scale;
    let grid_right = grid_left + grid_width;
    let grid_bottom = grid_top + grid_height;
    let visible_scene_x = SCENE_WIDTH as f32 / raster_width as f32;
    let visible_scene_y = SCENE_HEIGHT as f32 / raster_height as f32;

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
            let primary_len = usize::from(cell[PRIMARY_LENGTH_OFFSET]);
            let upper_len = usize::from(cell[UPPER_LENGTH_OFFSET]);
            let foreground = cell[FOREGROUND_OFFSET];
            let background = cell[BACKGROUND_OFFSET];
            let style = cell[STYLE_OFFSET];
            let left = grid_left + column as f32 * cell_width;
            let right = left + cell_width;

            if background != COLOR_DEFAULT && background != COLOR_TRANSPARENT {
                geometry_for_color(&mut backgrounds, background).quad(
                    left + visible_scene_x * scale * 0.5,
                    top + visible_scene_y * scale * 0.5,
                    right - visible_scene_x * scale * 0.5,
                    bottom - visible_scene_y * scale * 0.5,
                    0.8,
                );
            }

            if foreground == COLOR_TRANSPARENT || primary_len == 0 {
                continue;
            }
            let primary = core::str::from_utf8(&cell[PRIMARY_OFFSET..PRIMARY_OFFSET + primary_len])
                .map_err(|_| "gridpaper-utf8")?;
            let upper = if upper_len == 0 {
                None
            } else {
                Some(
                    core::str::from_utf8(&cell[UPPER_OFFSET..UPPER_OFFSET + upper_len])
                        .map_err(|_| "gridpaper-upper-utf8")?,
                )
            };
            // Font size is specified in output pixels. Convert it into the
            // logical scene units consumed by the resident font mesh so 100%
            // remains an actual 24 px regardless of the physical raster extent.
            let font_pixels = (DEFAULT_REGULAR_ROW_FONT_PIXELS * visible_scene_y * scale)
                .clamp(visible_scene_y, 256.0);
            let baseline = top + cell_height * 0.72;
            let has_upper = upper.is_some();
            texts.push(TextCell {
                text: String::from(primary),
                font: font_for_glyph(primary),
                color: foreground,
                center_x: (left + right) * 0.5 - if has_upper { cell_width * 0.10 } else { 0.0 },
                center_y: (top + bottom) * 0.5 + if has_upper { cell_height * 0.08 } else { 0.0 },
                font_pixels: if has_upper {
                    font_pixels * 0.82
                } else {
                    font_pixels
                },
                bold: style & STYLE_BOLD != 0,
                italic: style & STYLE_ITALIC != 0,
            });
            if let Some(upper) = upper {
                texts.push(TextCell {
                    text: String::from(upper),
                    font: font_for_glyph(upper),
                    color: foreground,
                    center_x: (left + right) * 0.5 + cell_width * 0.24,
                    center_y: (top + bottom) * 0.5 - cell_height * 0.24,
                    font_pixels: font_pixels * 0.52,
                    bold: style & STYLE_BOLD != 0,
                    italic: style & STYLE_ITALIC != 0,
                });
            }
            if style & STYLE_UNDERLINE != 0 {
                let thickness = (font_pixels / 14.0).max(visible_scene_y);
                let inset = DECORATION_INSET_MM * scene_units_per_mm_x * scale;
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
                let inset = DECORATION_INSET_MM * scene_units_per_mm_x * scale;
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
    let vertical_line = visible_scene_x * scale;
    let horizontal_line = visible_scene_y * scale;
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
        let length = axis_tick_length_mm(column) * scene_units_per_mm_y * scale;
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
        let length = axis_tick_length_mm(row) * scene_units_per_mm_x * scale;
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

    for font in [
        GpuFontFace::Inconsolata,
        GpuFontFace::NotoSansSc,
        GpuFontFace::Default,
    ] {
        for color in 0..COLOR_COUNT as u8 {
            if color == COLOR_TRANSPARENT
                || !texts
                    .iter()
                    .any(|cell| cell.color == color && cell.font == font)
            {
                continue;
            }
            let mut entries = Vec::new();
            for cell in texts
                .iter()
                .filter(|cell| cell.color == color && cell.font == font)
            {
                let bold_center_offset = if cell.bold {
                    visible_scene_x * 0.5 * scale
                } else {
                    0.0
                };
                entries.push(GpuFontJobEntry {
                    text: GpuFontTextRequest::SingleLine(cell.text.as_str()),
                    position: [cell.center_x - bold_center_offset, cell.center_y],
                    font_pixels: cell.font_pixels,
                    slant: if cell.italic { 0.22 } else { 0.0 },
                });
                if cell.bold {
                    entries.push(GpuFontJobEntry {
                        text: GpuFontTextRequest::SingleLine(cell.text.as_str()),
                        position: [cell.center_x + bold_center_offset, cell.center_y],
                        font_pixels: cell.font_pixels,
                        slant: if cell.italic { 0.22 } else { 0.0 },
                    });
                }
            }
            let mesh = create_resident_font_centered_scene_mesh_at_raster(
                &entries,
                font,
                SCENE_WIDTH,
                SCENE_HEIGHT,
                raster_width,
                raster_height,
            )?;
            layers.push(ResidentLayer {
                base_color: palette(color, false),
                text_color_selector: Some(color),
                mesh,
            });
        }
    }

    Ok(ResidentPage {
        serial: snapshot.serial,
        generation: snapshot.generation,
        scale_percent: snapshot.scale_percent,
        pan,
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
    selection: Option<GridCellSelection>,
    input_field: CellInputField,
    text_animations: &[Option<GpuFontColorProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_elapsed_ms: u64,
) -> Result<crate::intel::render::ResidentSceneFrameResult, ServiceError> {
    let viewport_translation_px = [
        page.pan.x * surface.width as f32 / SCENE_WIDTH as f32,
        page.pan.y * surface.height as f32 / SCENE_HEIGHT as f32,
    ];
    let draws = page
        .layers
        .iter()
        .map(|layer| crate::intel::render::ResidentSceneDraw {
            mesh: &layer.mesh,
            rgba: resident_layer_color(layer, text_animations, animation_elapsed_ms),
            viewport_translation_px,
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
    publish_pixels(surface, page, selection, input_field, &result)?;
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

fn render_print_page(
    request: PrintRenderRequest,
    text_animations: &[Option<GpuFontColorProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_elapsed_ms: u64,
) -> PrintRenderResult {
    let snapshot = OwnedSnapshot {
        raw: request.raw,
        owner: 0,
        generation: request.generation,
        scale_percent: 100,
        serial: u64::from(request.job_id),
    };
    let result = (|| {
        let page = build_resident_page(
            &snapshot,
            PRINT_CAPTURE_WIDTH,
            PRINT_CAPTURE_HEIGHT,
            ScenePan::ZERO,
        )?;
        let draws = page
            .layers
            .iter()
            .map(|layer| crate::intel::render::ResidentSceneDraw {
                mesh: &layer.mesh,
                rgba: resident_layer_color(layer, text_animations, animation_elapsed_ms),
                viewport_translation_px: [0.0, 0.0],
            })
            .collect::<Vec<_>>();
        let captured =
            crate::intel::render::capture_resident_triangle_scene_frame_premultiplied_at_extent(
                &draws,
                Some([0, 0, 0, 0]),
                PRINT_CAPTURE_WIDTH,
                PRINT_CAPTURE_HEIGHT,
                false,
            )?;
        if captured.completed_draws != captured.requested_draws {
            return Err("incomplete-print-frame");
        }
        let rgba_premultiplied = captured.rgba.ok_or("missing-print-frame")?;
        Ok(PrintRasterFrame {
            width: PRINT_CAPTURE_WIDTH,
            height: PRINT_CAPTURE_HEIGHT,
            rgba_premultiplied,
        })
    })();
    PrintRenderResult {
        job_id: request.job_id,
        result,
    }
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
    page: &ResidentPage,
    selection: Option<GridCellSelection>,
    input_field: CellInputField,
    result: &crate::intel::render::ResidentSceneFrameResult,
) -> Result<(), ServiceError> {
    let presentation = surface
        .presentation
        .ok_or(ServiceError::Window(crate::ui4::WindowBrokerError::SessionClosed))?;
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
    if let Some(selection) = selection {
        paint_grid_cursor(destination, view.pitch, surface, page, selection, input_field);
    }
    crate::intel::dma_flush(view.virt, view.byte_len);
    if let Err(error) = crate::ui4::publish_frame_buffer(lease) {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(error.into());
    }
    crate::ui4::publish_window_frame(UI4_OWNER, presentation.window, crate::ui4::DamageRect::FULL)?;
    Ok(())
}

fn paint_grid_cursor(
    destination: &mut [u8],
    pitch: u32,
    surface: &GridPaperSurface,
    page: &ResidentPage,
    selection: GridCellSelection,
    input_field: CellInputField,
) {
    let scale = f32::from(page.scale_percent) / 100.0;
    let scene_units_per_mm_x = SCENE_WIDTH as f32 / SURFACE_WIDTH_MM as f32;
    let scene_units_per_mm_y = SCENE_HEIGHT as f32 / SURFACE_HEIGHT_MM as f32;
    let cell_width = CELL_EDGE_MM as f32 * scene_units_per_mm_x * scale;
    let cell_height = CELL_EDGE_MM as f32 * scene_units_per_mm_y * scale;
    let mut scene_left = RULER_GUTTER_MM as f32 * scene_units_per_mm_x * scale
        + selection.column as f32 * cell_width
        + page.pan.x;
    let scene_top = RULER_GUTTER_MM as f32 * scene_units_per_mm_y * scale
        + selection.row as f32 * cell_height
        + page.pan.y;
    let scene_right = scene_left + cell_width;
    let mut scene_bottom = scene_top + cell_height;
    if input_field == CellInputField::Upper {
        scene_left += cell_width * 0.5;
        scene_bottom -= cell_height * 0.5;
    }
    let left = libm::floorf(scene_left * surface.width as f32 / SCENE_WIDTH as f32) as i32;
    let top = libm::floorf(scene_top * surface.height as f32 / SCENE_HEIGHT as f32) as i32;
    let right = libm::ceilf(scene_right * surface.width as f32 / SCENE_WIDTH as f32) as i32;
    let bottom = libm::ceilf(scene_bottom * surface.height as f32 / SCENE_HEIGHT as f32) as i32;
    let clipped_left = left.clamp(0, surface.width as i32) as u32;
    let clipped_top = top.clamp(0, surface.height as i32) as u32;
    let clipped_right = right.clamp(0, surface.width as i32) as u32;
    let clipped_bottom = bottom.clamp(0, surface.height as i32) as u32;
    if clipped_left >= clipped_right || clipped_top >= clipped_bottom {
        return;
    }
    let stroke = GRID_CURSOR_STROKE_PX
        .min(clipped_right - clipped_left)
        .min(clipped_bottom - clipped_top);
    let pitch = pitch as usize;
    for y in clipped_top..clipped_bottom {
        for x in clipped_left..clipped_right {
            if x - clipped_left >= stroke
                && clipped_right - x > stroke
                && y - clipped_top >= stroke
                && clipped_bottom - y > stroke
            {
                continue;
            }
            let offset = y as usize * pitch + x as usize * 4;
            if let Some(pixel) = destination.get_mut(offset..offset + 4) {
                pixel.copy_from_slice(&GRID_CURSOR_RGBA);
            }
        }
    }
}

/// Persistent GridPaper scene consumer. Geometry is rebuilt for accepted
/// snapshots and keyboard edits. Selection is composited over the rendered
/// page, while middle-button pan is hot-applied through the 3D viewport
/// transform and keeps the font and grid meshes GPU-owned.
#[embassy_executor::task]
pub async fn gridpaper_service_task() {
    crate::intel::wait_hw_logo_sequence_done().await;
    let mut last_init_error = None;
    let mut surface = loop {
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
        "gridpaper: embassy service ready page_bytes={} cells={} snapshot_buffers=2 ui4_buffers=2 scene={}x{} ui4={}x{} extent_source={} document_mm={}x{} grid_mm={}x{} surface_mm={}x{} ruler_gutter_mm={} target_cell_mm={} font_px_at_100=24 owner=kernel-app-4 input=left-click-cell+focused-keyboard+middle-pan pan_mode=hot-viewport-transform persistent_gpu_scene=1 presentation=vm-owner-gated initial_presentation=detached\n",
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
    let mut latest_snapshot: Option<OwnedSnapshot> = None;
    let mut queued_snapshot: Option<OwnedSnapshot> = None;
    let mut pending: Option<ResidentPage> = None;
    let mut active: Option<ResidentPage> = None;
    let mut pan = ScenePan::ZERO;
    let mut pan_dirty = false;
    let mut hot_pan_frames = 0u64;
    let mut active_pan_source = None;
    let mut pending_pan_pixels = (0i32, 0i32);
    let mut selection: Option<GridCellSelection> = None;
    let mut input_field = CellInputField::Primary;
    let mut cursor_dirty = false;
    let mut keyboard_edits = 0u64;
    let mut last_build_error = None;
    let mut last_render_error = None;
    let mut last_presentation_error: Option<(u8, ServiceError)> = None;

    loop {
        let desired_owner = connected_owner();
        let presented_owner = surface
            .presentation
            .map(|presentation| presentation.producer);
        if desired_owner != presented_owner {
            if surface.presentation.is_some() {
                release_presentation(&mut surface);
                if selection.take().is_some() {
                    cursor_dirty = true;
                }
                input_field = CellInputField::Primary;
                active_pan_source = None;
                pending_pan_pixels = (0, 0);
            }

            if let Some(producer) = desired_owner {
                match attach_presentation(&mut surface, producer, active.is_some()) {
                    Ok(presentation) => {
                        crate::log_info!(
                            target: "gridpaper";
                            "gridpaper: presentation attached producer={} session={} window={} frame={} retained_front={} persistent_gpu_scene=1\n",
                            producer,
                            presentation.session.raw(),
                            presentation.window.raw(),
                            surface.frame.raw(),
                            u8::from(active.is_some()),
                        );
                        last_presentation_error = None;
                    }
                    Err(error) if last_presentation_error != Some((producer, error)) => {
                        crate::log_warn!(
                            target: "gridpaper";
                            "gridpaper: presentation attach pending producer={} frame={} error={:?} action=retry retained_gpu_scene=1\n",
                            producer,
                            surface.frame.raw(),
                            error,
                        );
                        last_presentation_error = Some((producer, error));
                    }
                    Err(_) => {}
                }
            } else {
                last_presentation_error = None;
            }
        }

        let presented_window = surface.presentation.map(|presentation| presentation.window);

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
            let clamped_pan = pan.clamped(snapshot.scale_percent);
            if clamped_pan != pan {
                pan = clamped_pan;
                if let Some(page) = active.as_mut() {
                    page.pan = pan;
                }
                pan_dirty = true;
            }
            pending = None;
            latest_snapshot = Some(snapshot.clone());
            queued_snapshot = Some(snapshot);
        }

        for event in crate::ui4::take_owner_input_events(UI4_OWNER) {
            match event {
                crate::ui4::Ui4InputEvent::Button(event)
                    if presented_window == Some(event.window)
                        && event.phase == crate::ui4::Ui4ButtonPhase::Down
                        && event.changed_buttons & PRIMARY_BUTTON_MASK != 0 =>
                {
                    let scale_percent = active
                        .as_ref()
                        .map(|page| page.scale_percent)
                        .or_else(|| pending.as_ref().map(|page| page.scale_percent))
                        .or_else(|| {
                            latest_snapshot
                                .as_ref()
                                .map(|snapshot| snapshot.scale_percent)
                        });
                    let next = scale_percent.and_then(|scale_percent| {
                        grid_cell_at_local_point(
                            &surface,
                            event.local_x,
                            event.local_y,
                            scale_percent,
                            pan,
                        )
                    });
                    if next != selection {
                        selection = next;
                        input_field = CellInputField::Primary;
                        cursor_dirty = true;
                        if let Some(selected) = selection {
                            crate::log_info!(
                                target: "gridpaper";
                                "gridpaper: cell selected column={} row={} local={},{} scale={} pan_scene={:.3},{:.3} input=ui4-primary-click\n",
                                selected.column,
                                selected.row,
                                event.local_x,
                                event.local_y,
                                scale_percent.unwrap_or(0),
                                pan.x,
                                pan.y,
                            );
                        } else {
                            crate::log_info!(
                                target: "gridpaper";
                                "gridpaper: cell selection cleared local={},{} input=ui4-primary-click-outside-grid\n",
                                event.local_x,
                                event.local_y,
                            );
                        }
                    }
                }
                crate::ui4::Ui4InputEvent::Keyboard(event)
                    if presented_window == Some(event.window)
                        && event.event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
                        && event.event.key_code == crate::r::keyboard::KEYBOARD_KEY_F10 =>
                {
                    if let Some(snapshot) = latest_snapshot.as_ref()
                        && queue_print_request(snapshot).is_none()
                    {
                        crate::log_os::print2d_job_state(
                            0,
                            "request-dropped",
                            "gridpaper-F10-queue-full",
                        );
                    }
                }
                crate::ui4::Ui4InputEvent::Keyboard(event)
                    if presented_window == Some(event.window) && selection.is_some() =>
                {
                    let Some(snapshot) = latest_snapshot.as_mut() else {
                        continue;
                    };
                    let mut selected = selection.expect("gridpaper selection checked");
                    let outcome = edit_snapshot_from_keyboard(
                        snapshot,
                        &mut selected,
                        &mut input_field,
                        event.event,
                    );
                    if outcome.capacity_rejected {
                        crate::log_warn!(
                            target: "gridpaper";
                            "gridpaper: cell input rejected column={} row={} field={} rule=one-unicode-scalar-and-upper-requires-primary input=ui4-keyboard\n",
                            selected.column,
                            selected.row,
                            input_field.name(),
                        );
                    }
                    if outcome.input_field_changed {
                        crate::log_info!(
                            target: "gridpaper";
                            "gridpaper: cell input field toggled column={} row={} field={} key=tab input=ui4-focused-keyboard\n",
                            selected.column,
                            selected.row,
                            input_field.name(),
                        );
                    }
                    if outcome.clear_selection {
                        selection = None;
                        input_field = CellInputField::Primary;
                        cursor_dirty = true;
                    } else {
                        selection = Some(selected);
                        cursor_dirty |= outcome.selection_changed || outcome.input_field_changed;
                    }
                    if outcome.content_changed {
                        keyboard_edits = keyboard_edits.saturating_add(1);
                        let edited = outcome.edited_cell.unwrap_or(selected);
                        let offset = (edited.row * COLUMNS + edited.column) * CELL_BYTES;
                        let primary_len = usize::from(snapshot.raw[offset + PRIMARY_LENGTH_OFFSET]);
                        let upper_len = usize::from(snapshot.raw[offset + UPPER_LENGTH_OFFSET]);
                        queued_snapshot = Some(snapshot.clone());
                        pending = None;
                        crate::log_info!(
                            target: "gridpaper";
                            "gridpaper: cell edited seq={} column={} row={} field={} primary_utf8_bytes={} upper_utf8_bytes={} key_kind={} codepoint={} input=ui4-focused-keyboard action=rebuild-page\n",
                            keyboard_edits,
                            edited.column,
                            edited.row,
                            input_field.name(),
                            primary_len,
                            upper_len,
                            event.event.kind,
                            event.event.codepoint,
                        );
                    }
                }
                crate::ui4::Ui4InputEvent::Pan(event) if presented_window == Some(event.window) => {
                    match event.phase {
                        crate::ui4::Ui4PanPhase::Begin => {
                            active_pan_source = Some(event.source);
                            pending_pan_pixels = (0, 0);
                        }
                        crate::ui4::Ui4PanPhase::Update
                            if active_pan_source == Some(event.source) =>
                        {
                            pending_pan_pixels.0 = pending_pan_pixels.0.saturating_add(event.dx);
                            pending_pan_pixels.1 = pending_pan_pixels.1.saturating_add(event.dy);
                            let Some(snapshot) = latest_snapshot.as_ref() else {
                                continue;
                            };
                            if pan.drag_pixels(
                                event.dx,
                                event.dy,
                                surface.width,
                                surface.height,
                                snapshot.scale_percent,
                            ) {
                                if let Some(page) = pending.as_mut() {
                                    page.pan = pan;
                                }
                                if let Some(page) = active.as_mut() {
                                    page.pan = pan;
                                }
                                pan_dirty = true;
                            }
                        }
                        crate::ui4::Ui4PanPhase::End if active_pan_source == Some(event.source) => {
                            active_pan_source = None;
                            let (drag_x, drag_y) = pending_pan_pixels;
                            pending_pan_pixels = (0, 0);
                            if drag_x != 0 || drag_y != 0 {
                                crate::log_info!(
                                    target: "gridpaper";
                                    "gridpaper: middle-pan ended drag_px={},{} pan_scene={:.3},{:.3} hot_frames_total={} action=retain-resident-meshes\n",
                                    drag_x,
                                    drag_y,
                                    pan.x,
                                    pan.y,
                                    hot_pan_frames,
                                );
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        if let Some(snapshot) = queued_snapshot.as_ref() {
            match build_resident_page(&snapshot, surface.width, surface.height, pan) {
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
        if let Some(request) = PRINT_RENDER_REQUESTS.lock().pop_front() {
            let result = render_print_page(request, &text_animations, animation_elapsed_ms);
            PRINT_RENDER_RESULTS.lock().push_back(result);
        }
        let mut published_page_this_tick = false;
        if surface.presentation.is_some()
            && let Some(candidate) = pending.as_ref()
        {
            match publish_page(
                &surface,
                candidate,
                selection,
                input_field,
                &text_animations,
                animation_elapsed_ms,
            ) {
                Ok(result) => {
                    let published = pending.take().expect("gridpaper pending page exists");
                    last_sampled_text_colors =
                        sampled_text_colors(&published, &text_animations, animation_elapsed_ms);
                    crate::log_info!(
                        target: "gridpaper";
                        "gridpaper: frame published serial={} generation={} scale={} pan_scene={:.3},{:.3} layers={} changed_pixels={} frame_us={} persistence=resident-until-next-snapshot pan_transform=sf-viewport\n",
                        published.serial,
                        published.generation,
                        published.scale_percent,
                        published.pan.x,
                        published.pan.y,
                        published.layers.len(),
                        result.changed_pixels,
                        result.frame_us,
                    );
                    let retired = active.replace(published);
                    drop(retired);
                    animation_dirty = false;
                    pan_dirty = false;
                    cursor_dirty = false;
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
            && surface.presentation.is_some()
            && pending.is_none()
            && let Some(page) = active.as_ref()
        {
            let sampled = sampled_text_colors(page, &text_animations, animation_elapsed_ms);
            let animation_changed = animation_dirty || sampled != last_sampled_text_colors;
            let hot_pan_frame = pan_dirty;
            let selection_frame = cursor_dirty;
            if animation_changed || hot_pan_frame || selection_frame {
                match publish_page(
                    &surface,
                    page,
                    selection,
                    input_field,
                    &text_animations,
                    animation_elapsed_ms,
                ) {
                    Ok(result) => {
                        last_sampled_text_colors = sampled;
                        animation_dirty = false;
                        pan_dirty = false;
                        cursor_dirty = false;
                        if hot_pan_frame {
                            hot_pan_frames = hot_pan_frames.saturating_add(1);
                            if hot_pan_frames <= 8 || hot_pan_frames.is_multiple_of(120) {
                                crate::log_info!(
                                    target: "gridpaper";
                                    "gridpaper: hot-pan-frame seq={} pan_scene={:.3},{:.3} changed_pixels={} frame_us={} geometry_uploads=0 resident_mesh_rebuilds=0 transform=sf-viewport preclip=translated-bypass final_clip=scissor\n",
                                    hot_pan_frames,
                                    page.pan.x,
                                    page.pan.y,
                                    result.changed_pixels,
                                    result.frame_us,
                                );
                            }
                        }
                        if animation_changed {
                            animation_frames = animation_frames.saturating_add(1);
                        }
                        if animation_changed
                            && (animation_frames <= 8 || animation_frames.is_multiple_of(120))
                        {
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
        assert_eq!(CELL_BYTES, 13);
        assert_eq!(PAGE_BYTES, 25_493);
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
    fn middle_pan_tracks_drag_and_clamps_to_scaled_document() {
        let mut pan = ScenePan::ZERO;
        assert!(!pan.drag_pixels(100, 100, 810, 1_153, 150));
        assert!(pan.drag_pixels(-10_000, -10_000, 810, 1_153, 150));
        assert_eq!(pan.x, -(SCENE_WIDTH as f32 * 0.5));
        assert_eq!(pan.y, -(SCENE_HEIGHT as f32 * 0.5));
        assert!(pan.drag_pixels(10_000, 10_000, 810, 1_153, 150));
        assert_eq!(pan, ScenePan::ZERO);

        assert!(!pan.drag_pixels(-100, -100, 810, 1_153, 100));
        assert_eq!(pan, ScenePan::ZERO);
    }

    #[test]
    fn validator_rejects_non_utf8_and_unknown_style_bits() {
        let mut raw = [0u8; PAGE_BYTES];
        assert_eq!(validate_page(&raw), Ok(()));
        raw[PRIMARY_LENGTH_OFFSET] = 1;
        raw[PRIMARY_OFFSET] = 0xff;
        assert_eq!(validate_page(&raw), Err(()));
        raw[PRIMARY_LENGTH_OFFSET] = 0;
        raw[PRIMARY_OFFSET] = 0;
        raw[STYLE_OFFSET] = 0x80;
        assert_eq!(validate_page(&raw), Err(()));
    }

    #[test]
    fn keyboard_primary_advances_while_upper_stays_and_delete_restores_primary_only() {
        let mut snapshot = OwnedSnapshot {
            raw: Vec::from([0u8; PAGE_BYTES]),
            generation: 1,
            scale_percent: 100,
            serial: 1,
        };
        let original = GridCellSelection { column: 2, row: 3 };
        let mut selection = original;
        let mut input_field = CellInputField::Primary;
        let mut primary_event = crate::r::keyboard::TrueosKeyboardOutputEvent::default();
        primary_event.kind = crate::r::keyboard::KEYBOARD_OUTPUT_KIND_TEXT;
        primary_event.utf8_len = 1;
        primary_event.codepoint = 'x' as u32;
        primary_event.utf8[0] = b'x';

        let primary = edit_snapshot_from_keyboard(
            &mut snapshot,
            &mut selection,
            &mut input_field,
            primary_event,
        );
        assert!(primary.content_changed);
        assert!(primary.selection_changed);
        assert_eq!(selection, GridCellSelection { column: 3, row: 3 });

        selection = original;
        let mut tab = crate::r::keyboard::TrueosKeyboardOutputEvent::default();
        tab.kind = crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY;
        tab.key_code = crate::r::keyboard::KEYBOARD_KEY_TAB;
        let toggled =
            edit_snapshot_from_keyboard(&mut snapshot, &mut selection, &mut input_field, tab);
        assert!(toggled.input_field_changed);
        assert_eq!(input_field, CellInputField::Upper);

        let mut upper_event = crate::r::keyboard::TrueosKeyboardOutputEvent::default();
        let mut encoded = [0u8; 4];
        let upper = '²'.encode_utf8(&mut encoded);
        upper_event.kind = crate::r::keyboard::KEYBOARD_OUTPUT_KIND_TEXT;
        upper_event.utf8_len = upper.len() as u8;
        upper_event.codepoint = '²' as u32;
        upper_event.utf8[..upper.len()].copy_from_slice(upper.as_bytes());
        let upper = edit_snapshot_from_keyboard(
            &mut snapshot,
            &mut selection,
            &mut input_field,
            upper_event,
        );
        assert!(upper.content_changed);
        assert!(!upper.selection_changed);
        assert_eq!(selection, original);

        let offset = (original.row * COLUMNS + original.column) * CELL_BYTES;
        let cell = &snapshot.raw[offset..offset + CELL_BYTES];
        assert_eq!(cell[PRIMARY_LENGTH_OFFSET], 1);
        assert_eq!(&cell[PRIMARY_OFFSET..PRIMARY_OFFSET + 1], b"x");
        assert_eq!(cell[UPPER_LENGTH_OFFSET], 2);
        assert_eq!(&cell[UPPER_OFFSET..UPPER_OFFSET + 2], "²".as_bytes());
        assert_eq!(validate_page(&snapshot.raw), Ok(()));

        let mut delete = crate::r::keyboard::TrueosKeyboardOutputEvent::default();
        delete.kind = crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY;
        delete.key_code = crate::r::keyboard::KEYBOARD_KEY_DELETE;
        let deleted =
            edit_snapshot_from_keyboard(&mut snapshot, &mut selection, &mut input_field, delete);
        assert!(deleted.content_changed);
        let cell = &snapshot.raw[offset..offset + CELL_BYTES];
        assert_eq!(cell[PRIMARY_LENGTH_OFFSET], 1);
        assert_eq!(cell[UPPER_LENGTH_OFFSET], 0);
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
