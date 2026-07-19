//! Embassy consumer for GridPaper's fixed snapshot format.
//!
//! The Blueprint owns snapshot publication cadence. This service owns the
//! accepted working copy, UI4 editing/focus state, GPU allocations, and
//! presentation lifetime. No UI4 handles or generic drawing operations cross
//! the ABI.

use alloc::{collections::VecDeque, string::String, vec::Vec};

use embassy_sync::mutex::Mutex as AsyncMutex;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

use crate::intel::gpu_font::{
    GPU_FONT_COLOR_KEYFRAME_CAPACITY, GpuFontColorChannels, GpuFontColorIteration,
    GpuFontColorKeyframe, GpuFontColorKeyframes, GpuFontColorProgram, GpuFontColorTiming,
    GpuFontFace, GpuFontRgba,
};

const COLUMNS: usize = 39;
const ROWS: usize = 55;
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
/// Each Blueprint exposes one local GridPaper document. The kernel leases
/// those local documents onto this many independent resident service slots.
const BLUEPRINT_INSTANCE_CAPACITY: usize = 1;
const GRIDPAPER_POOL_SOFT_CAP: usize = 10;
const PRIMARY_INSTANCE_ID: u32 = 0;
const NATIVE_SCALE_PERCENT: u16 = 100;

const DEFAULT_REGULAR_ROW_FONT_PIXELS: f32 = 24.0;
pub(crate) const A4_WIDTH_MM: u32 = 210;
pub(crate) const A4_HEIGHT_MM: u32 = 297;
const CELL_EDGE_MM: u32 = 5;
pub(crate) const GRID_WIDTH_MM: u32 = COLUMNS as u32 * CELL_EDGE_MM;
pub(crate) const GRID_HEIGHT_MM: u32 = ROWS as u32 * CELL_EDGE_MM;
pub(crate) const RULER_GUTTER_MM: u32 = 4;
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
const UI4_OWNER: crate::ui4::WindowOwner = crate::ui4::WindowOwner::GRIDPAPER_SERVICE;
const UI4_PLANE_SLOT: usize = crate::ui4::RGB_OVERLAY_PLANE_SLOT_2;
const _: () = assert!(UI4_PLANE_SLOT == 2);
const SERVICE_PERIOD_MS: u64 = 16;
const PRIMARY_BUTTON_MASK: u32 = 1 << 0;
const INPUT_QUEUE_CAPACITY_PER_INSTANCE: usize = 64;
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
const ERROR_INVALID_INSTANCE: i32 = -6;
const ERROR_POOL_FULL: i32 = -7;

static COVERAGE_COMPOSITE_FALLBACK_LOGGED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
static GPU_DIRECT_PRESENT_LOGGED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

struct SnapshotStore {
    buffers: [[u8; PAGE_BYTES]; 2],
    published: usize,
    owner: Option<u8>,
    local_instance_id: Option<u32>,
    lease_epoch: u64,
    producer_connected: bool,
    lifecycle_paused: bool,
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
            local_instance_id: None,
            lease_epoch: 0,
            producer_connected: false,
            lifecycle_paused: false,
            generation: 0,
            scale_percent: 100,
            serial: 0,
            text_animations: [None; TEXT_ANIMATION_COLOR_SLOTS],
            animation_serial: 0,
        }
    }

    fn claim(&mut self, owner: u8, local_instance_id: u32) {
        let lease_epoch = self.lease_epoch.wrapping_add(1).max(1);
        self.published = 0;
        self.owner = Some(owner);
        self.local_instance_id = Some(local_instance_id);
        self.lease_epoch = lease_epoch;
        self.producer_connected = true;
        self.lifecycle_paused = false;
        self.generation = 0;
        self.scale_percent = NATIVE_SCALE_PERCENT;
        self.serial = 0;
        self.text_animations = [None; TEXT_ANIMATION_COLOR_SLOTS];
        self.animation_serial = 0;
    }

    fn release(&mut self) {
        self.owner = None;
        self.local_instance_id = None;
        self.lease_epoch = self.lease_epoch.wrapping_add(1).max(1);
        self.producer_connected = false;
        self.lifecycle_paused = false;
        self.generation = 0;
        self.serial = 0;
        self.text_animations = [None; TEXT_ANIMATION_COLOR_SLOTS];
        self.animation_serial = 0;
    }
}

static SNAPSHOTS: Mutex<[SnapshotStore; GRIDPAPER_POOL_SOFT_CAP]> =
    Mutex::new([const { SnapshotStore::new() }; GRIDPAPER_POOL_SOFT_CAP]);

fn valid_local_instance(instance_id: u32) -> bool {
    usize::try_from(instance_id).is_ok_and(|index| index < BLUEPRINT_INSTANCE_CAPACITY)
}

fn find_pool_slot(
    stores: &[SnapshotStore; GRIDPAPER_POOL_SOFT_CAP],
    owner: u8,
    local_instance_id: u32,
) -> Option<usize> {
    stores.iter().position(|store| {
        store.owner == Some(owner) && store.local_instance_id == Some(local_instance_id)
    })
}

fn resolve_pool_slot(owner: u8, local_instance_id: u32) -> Result<usize, i32> {
    if !valid_local_instance(local_instance_id) {
        return Err(ERROR_INVALID_INSTANCE);
    }
    let stores = SNAPSHOTS.lock();
    find_pool_slot(&stores, owner, local_instance_id).ok_or(ERROR_NOT_OWNER)
}

fn resolve_or_claim_pool_slot(owner: u8, local_instance_id: u32) -> Result<usize, i32> {
    if !valid_local_instance(local_instance_id) {
        return Err(ERROR_INVALID_INSTANCE);
    }
    let mut stores = SNAPSHOTS.lock();
    if let Some(slot) = find_pool_slot(&stores, owner, local_instance_id) {
        return Ok(slot);
    }
    let Some(slot) = stores.iter().position(|store| store.owner.is_none()) else {
        return Err(ERROR_POOL_FULL);
    };
    stores[slot].claim(owner, local_instance_id);
    crate::log_info!(
        target: "gridpaper";
        "gridpaper: pool lease claimed slot={} owner={} local_instance={} soft_cap={}\n",
        slot,
        owner,
        local_instance_id,
        GRIDPAPER_POOL_SOFT_CAP,
    );
    Ok(slot)
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
    instance_id: u32,
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

fn queue_print_request(instance_id: u32, snapshot: &OwnedSnapshot) -> Option<u32> {
    let token = next_print_request_token();
    let mut requests = GRIDPAPER_PRINT_REQUESTS.lock();
    if requests.len() >= PRINT_REQUEST_CAPACITY {
        return None;
    }
    requests.push_back(GridPaperPrintRequest {
        instance_id,
        owner: snapshot.owner,
        token,
        generation: snapshot.generation,
        raw: snapshot.raw.clone(),
    });
    drop(requests);
    crate::log_os::gridpaper_print_requested(snapshot.owner, token, snapshot.generation);
    Some(token)
}

pub(crate) fn take_print_request_for_owner(owner: u8, instance_id: u32) -> Option<(u32, u64)> {
    let requests = GRIDPAPER_PRINT_REQUESTS.lock();
    let request = requests
        .iter()
        .find(|request| request.owner == owner && request.instance_id == instance_id)?;
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
    instance_id: u32,
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
    let instance = match resolve_or_claim_pool_slot(owner, instance_id) {
        Ok(instance) => instance,
        Err(error) => return error,
    };

    let mut stores = SNAPSHOTS.lock();
    let snapshots = &mut stores[instance];
    if !crate::hv::vm_state(owner).pause_latched {
        snapshots.lifecycle_paused = false;
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
pub(crate) fn submit_text_animations_for_owner(owner: u8, instance_id: u32, raw: &[u8]) -> i32 {
    let Ok(programs) = decode_text_animations(raw) else {
        return ERROR_INVALID_ANIMATION;
    };
    let instance = match resolve_or_claim_pool_slot(owner, instance_id) {
        Ok(instance) => instance,
        Err(error) => return error,
    };
    let mut stores = SNAPSHOTS.lock();
    let snapshots = &mut stores[instance];
    if !crate::hv::vm_state(owner).pause_latched {
        snapshots.lifecycle_paused = false;
    }
    snapshots.owner = Some(owner);
    snapshots.producer_connected = true;
    snapshots.text_animations = programs;
    snapshots.animation_serial = snapshots.animation_serial.wrapping_add(1).max(1);
    crate::log_info!(
        target: "gridpaper";
        "gridpaper: text-animation-table accepted pool_slot={} owner={} local_instance={} serial={} programs={} wire_bytes={} ownership=producer-scoped geometry_uploads=0\n",
        instance,
        owner,
        instance_id,
        snapshots.animation_serial,
        snapshots.text_animations.iter().flatten().count(),
        raw.len(),
    );
    0
}

/// Relinquish producer authority and return its kernel pool slot. Lifecycle
/// pause is the separate operation that retains a scene for resume.
pub(crate) fn close_owner(owner: u8, instance_id: u32) -> i32 {
    let instance = match resolve_pool_slot(owner, instance_id) {
        Ok(instance) => instance,
        Err(ERROR_NOT_OWNER) => return 0,
        Err(error) => return error,
    };
    let mut stores = SNAPSHOTS.lock();
    let snapshots = &mut stores[instance];
    match snapshots.owner {
        Some(active) if active == owner => {
            snapshots.release();
            crate::log_info!(
                target: "gridpaper";
                "gridpaper: pool lease released slot={} owner={} local_instance={} soft_cap={}\n",
                instance,
                owner,
                instance_id,
                GRIDPAPER_POOL_SOFT_CAP,
            );
            0
        }
        Some(_) => ERROR_NOT_OWNER,
        None => 0,
    }
}

/// Detach every Gridpaper presentation owned by a VM while keeping its page,
/// resident 3D scene, GPU allocations, and last front buffer available for a
/// same-slot resume.
pub(crate) fn pause_owner_lifecycle(owner: u8) -> usize {
    let mut stores = SNAPSHOTS.lock();
    let mut retained = 0usize;
    for snapshot in stores.iter_mut() {
        if snapshot.owner == Some(owner) {
            snapshot.lifecycle_paused = true;
            retained = retained.saturating_add(1);
        }
    }
    if retained != 0 {
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: lifecycle pause owner={} retained_scenes={} action=detach-ui4-preserve-resident-3d\n",
            owner,
            retained,
        );
    }
    retained
}

/// Re-arm retained Gridpaper producers after their VM slot has been restored.
/// UI4 creates a fresh presentation session; no snapshotted window or GPU
/// handle is reused.
pub(crate) fn resume_owner_lifecycle(owner: u8) -> usize {
    let mut stores = SNAPSHOTS.lock();
    let mut resumed = 0usize;
    for snapshot in stores.iter_mut() {
        if snapshot.owner == Some(owner) {
            snapshot.lifecycle_paused = false;
            snapshot.producer_connected = true;
            resumed = resumed.saturating_add(1);
        }
    }
    if resumed != 0 {
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: lifecycle resume owner={} retained_scenes={} action=reattach-fresh-ui4-session\n",
            owner,
            resumed,
        );
    }
    resumed
}

fn snapshot_after(pool_slot: usize, serial: u64) -> Option<OwnedSnapshot> {
    let stores = SNAPSHOTS.lock();
    let snapshots = stores.get(pool_slot)?;
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

fn text_animations_after(pool_slot: usize, serial: u64) -> Option<OwnedTextAnimations> {
    let stores = SNAPSHOTS.lock();
    let snapshots = stores.get(pool_slot)?;
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
    // Preserve the original portal ABI for already-packaged GridPaper
    // Blueprints. New producers use the instance-aware symbol below.
    unsafe {
        trueos_cabi_gridpaper_snapshot_submit_instance(
            PRIMARY_INSTANCE_ID,
            generation,
            scale_percent,
            raw_ptr,
            raw_len,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gridpaper_snapshot_submit_instance(
    instance_id: u32,
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
            u64::from(scale_percent) | (u64::from(instance_id) << 32),
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
    submit_snapshot_for_owner(owner, instance_id, generation, scale_percent, raw)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gridpaper_text_animations_submit(
    raw_ptr: *const u8,
    raw_len: usize,
) -> i32 {
    // Preserve the original single-instance portal ABI.
    unsafe {
        trueos_cabi_gridpaper_text_animations_submit_instance(PRIMARY_INSTANCE_ID, raw_ptr, raw_len)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gridpaper_text_animations_submit_instance(
    instance_id: u32,
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
            u64::from(instance_id),
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
    submit_text_animations_for_owner(owner, instance_id, raw)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_gridpaper_close() -> i32 {
    trueos_cabi_gridpaper_close_instance(PRIMARY_INSTANCE_ID)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_gridpaper_close_instance(instance_id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_GRIDPAPER_CLOSE,
            u64::from(instance_id),
            0,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    crate::hv::current_guest_execution_context_vm_id()
        .map(|owner| close_owner(owner, instance_id))
        .unwrap_or(ERROR_NOT_OWNER)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_gridpaper_print_request_take() -> u64 {
    trueos_cabi_gridpaper_print_request_take_instance(PRIMARY_INSTANCE_ID)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_gridpaper_print_request_take_instance(instance_id: u32) -> u64 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_GRIDPAPER_PRINT_REQUEST_TAKE,
            u64::from(instance_id),
            0,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data
        } else {
            0
        };
    }
    let Some(owner) = crate::hv::current_guest_execution_context_vm_id() else {
        return 0;
    };
    take_print_request_for_owner(owner, instance_id)
        .map(|(token, _generation)| u64::from(token))
        .unwrap_or(0)
}

// Portal ABI regression guards. The legacy signatures are intentionally kept
// distinct from the instance-aware signatures so an old Blueprint cannot have
// its pointer/length registers reinterpreted again.
const _: unsafe extern "C" fn(u64, u32, *const u8, usize) -> i32 =
    trueos_cabi_gridpaper_snapshot_submit;
const _: unsafe extern "C" fn(u32, u64, u32, *const u8, usize) -> i32 =
    trueos_cabi_gridpaper_snapshot_submit_instance;
const _: unsafe extern "C" fn(*const u8, usize) -> i32 =
    trueos_cabi_gridpaper_text_animations_submit;
const _: unsafe extern "C" fn(u32, *const u8, usize) -> i32 =
    trueos_cabi_gridpaper_text_animations_submit_instance;
const _: extern "C" fn() -> i32 = trueos_cabi_gridpaper_close;
const _: extern "C" fn(u32) -> i32 = trueos_cabi_gridpaper_close_instance;
const _: extern "C" fn() -> u64 = trueos_cabi_gridpaper_print_request_take;
const _: extern "C" fn(u32) -> u64 = trueos_cabi_gridpaper_print_request_take_instance;

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
    pool_slot: usize,
    instance_id: u32,
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

fn initialize_surface(
    pool_slot: usize,
    instance_id: u32,
) -> Result<GridPaperSurface, ServiceError> {
    let (width, height, extent_source) =
        crate::intel::physical_extent_pixels(SURFACE_WIDTH_MM, SURFACE_HEIGHT_MM)
            .map(|(width, height)| (width, height, "edid-physical-mm"))
            .unwrap_or((SCENE_WIDTH, SCENE_HEIGHT, "logical-fallback"));
    let output = crate::ui4::OutputId::from_slot(0).expect("UI4 D01 must exist");
    let frame = crate::ui4::create_frame(crate::ui4::FrameSpec {
        output,
        content: crate::ui4::FrameContent::RenderScene3d,
        cadence: crate::ui4::FrameCadence::Streaming,
        format: crate::ui4::ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(crate::ui4::PremultipliedRgba8::TRANSPARENT),
    })?;
    Ok(GridPaperSurface {
        pool_slot,
        instance_id,
        frame,
        presentation: None,
        width,
        height,
        extent_source,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PoolLeaseState {
    epoch: u64,
    owner: Option<u8>,
    local_instance_id: Option<u32>,
    presentable_owner: Option<u8>,
}

fn pool_lease_state(pool_slot: usize) -> PoolLeaseState {
    let mut stores = SNAPSHOTS.lock();
    // The current direct-present iteration reserves one hardware plane for
    // GridPaper. Keep every leased scene resident, but attach only the first
    // live producer; otherwise two exact-release windows on slot 2 would be
    // rejected rather than silently copied/composited.
    let mut presentation_slot = None;
    for (candidate_slot, candidate) in stores.iter_mut().enumerate() {
        let Some(candidate_owner) = candidate.owner else {
            continue;
        };
        if candidate.lifecycle_paused || !candidate.producer_connected {
            continue;
        }
        let state = crate::hv::vm_state(candidate_owner);
        if state.running || state.starting {
            if presentation_slot.is_none() {
                presentation_slot = Some(candidate_slot);
            }
        } else {
            candidate.producer_connected = false;
        }
    }
    let snapshots = &stores[pool_slot];
    let owner = snapshots.owner;
    let presentable_owner = (presentation_slot == Some(pool_slot))
        .then_some(owner)
        .flatten();
    PoolLeaseState {
        epoch: snapshots.lease_epoch,
        owner,
        local_instance_id: snapshots.local_instance_id,
        presentable_owner,
    }
}

fn attach_presentation(
    surface: &mut GridPaperSurface,
    producer: u8,
    session: crate::ui4::WindowSessionId,
    expose_retained_front: bool,
) -> Result<GridPaperPresentation, ServiceError> {
    let (grid_width, grid_height) =
        crate::intel::physical_extent_pixels(GRID_WIDTH_MM, GRID_HEIGHT_MM)
            .unwrap_or((GRID_WIDTH_MM, GRID_HEIGHT_MM));
    let output = crate::ui4::OutputId::from_slot(0).expect("UI4 D01 must exist");
    let (scanout_width, scanout_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((surface.width, surface.height));
    // Keep the useful grid centered. The surface itself extends only far
    // enough above and to the left to carry the two ruler axes.
    let primary_x = scanout_width
        .saturating_sub(grid_width)
        .saturating_div(2)
        .saturating_sub(surface.width.saturating_sub(grid_width));
    let y = scanout_height
        .saturating_sub(grid_height)
        .saturating_div(2)
        .saturating_sub(surface.height.saturating_sub(grid_height));
    // Every Blueprint owns one same-sized scene. Cascade separately leased
    // windows just enough that another instance remains reachable for drag.
    let cascade = (surface.pool_slot as u32 % 5).saturating_mul(24);
    let x = primary_x
        .saturating_add(cascade)
        .min(scanout_width.saturating_sub(surface.width));
    let y = y
        .saturating_add(cascade)
        .min(scanout_height.saturating_sub(surface.height));
    let window = crate::ui4::create_window(crate::ui4::WindowCreate {
        owner: UI4_OWNER,
        session,
        frame: surface.frame,
        output,
        plane: crate::ui4::WindowPlane::Universal(UI4_PLANE_SLOT as u8),
        placement: crate::ui4::WindowPlacement {
            x: x as i32,
            y: y as i32,
            width: surface.width,
            height: surface.height,
            z: 70,
            opacity: u8::MAX,
            visible: true,
        },
        interaction: crate::ui4::WindowInteraction::APPLICATION,
    })?;

    if expose_retained_front
        && let Err(error) =
            crate::ui4::publish_window_frame(UI4_OWNER, window, crate::ui4::DamageRect::FULL)
    {
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
    Some(presentation)
}

struct ResidentLayer {
    base_color: [u8; 4],
    text_color_selector: Option<u8>,
    mesh: crate::intel::render::ResidentTriangleMesh,
    coverage: Option<crate::intel::gpu_font::GpuFontCoverageMask>,
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

fn font_preferences(_instance_id: u32) -> [GpuFontFace; 3] {
    // Preserve the former native/100% debug scene as the sole Blueprint
    // document contract.
    [
        GpuFontFace::Default,
        GpuFontFace::NotoSansSc,
        GpuFontFace::Inconsolata,
    ]
}

fn font_for_glyph(instance_id: u32, glyph: &str) -> GpuFontFace {
    for font in font_preferences(instance_id) {
        if crate::intel::gpu_font::font_face_supports_text(font, glyph) {
            return font;
        }
    }
    GpuFontFace::Default
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
    instance_id: u32,
    snapshot: &OwnedSnapshot,
    raster_width: u32,
    raster_height: u32,
    pan: ScenePan,
) -> Result<ResidentPage, &'static str> {
    use crate::intel::gpu_font::{
        GpuFontJobEntry, GpuFontTextRequest, create_gpu_font_centered_coverage_mask_at_raster,
        create_resident_font_centered_scene_mesh_at_raster, ensure_font_face_available,
        gpu_font_entries_use_analytical_coverage,
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
                font: font_for_glyph(instance_id, primary),
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
                    font: font_for_glyph(instance_id, upper),
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
            let coverage = if gpu_font_entries_use_analytical_coverage(
                &entries,
                SCENE_WIDTH,
                SCENE_HEIGHT,
                raster_width,
                raster_height,
            ) {
                match create_gpu_font_centered_coverage_mask_at_raster(
                    &entries,
                    font,
                    SCENE_WIDTH,
                    SCENE_HEIGHT,
                    raster_width,
                    raster_height,
                ) {
                    Ok(coverage) => Some(coverage),
                    Err(reason) => {
                        crate::log_warn!(
                            target: "gridpaper";
                            "gridpaper: analytical font coverage unavailable instance={} scale={} font={} color={} entries={} reason={} action=resident-triangle-fallback\n",
                            instance_id,
                            snapshot.scale_percent,
                            font.registry_name(),
                            color,
                            entries.len(),
                            reason,
                        );
                        None
                    }
                }
            } else {
                None
            };
            layers.push(ResidentLayer {
                base_color: palette(color, false),
                text_color_selector: Some(color),
                mesh,
                coverage,
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
        coverage: None,
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
    let presentation = surface
        .presentation
        .ok_or(ServiceError::Window(crate::ui4::WindowBrokerError::SessionClosed))?;
    let lease = crate::ui4::acquire_frame_buffer(surface.frame)?;
    let destination = match crate::ui4::gpgpu_rgba_surface(lease) {
        Ok(destination) => destination,
        Err(error) => {
            let _ = crate::ui4::cancel_frame_buffer(lease);
            return Err(error.into());
        }
    };
    if destination.width != surface.width || destination.height != surface.height {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(ServiceError::InvalidFrame);
    }
    let viewport_translation_px = [
        page.pan.x * surface.width as f32 / SCENE_WIDTH as f32,
        page.pan.y * surface.height as f32 / SCENE_HEIGHT as f32,
    ];
    let cursor_rects = selection.and_then(|selection| {
        grid_cursor_rects(surface, page, selection, input_field)
    });
    let final_rects = cursor_rects.as_ref().map_or(&[][..], |rects| &rects[..]);
    let result = match capture_resident_page_frame(
        page,
        text_animations,
        animation_elapsed_ms,
        viewport_translation_px,
        surface.width,
        surface.height,
        final_rects,
        Some(destination),
    ) {
        Ok(result) => result,
        Err(reason) => {
            let _ = crate::ui4::cancel_frame_buffer(lease);
            return Err(ServiceError::Render(reason));
        }
    };
    if !GPU_DIRECT_PRESENT_LOGGED.swap(true, core::sync::atomic::Ordering::AcqRel) {
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: live frame path=gpu-direct-msaa-resolve-to-ui4-triple size={}x{} pitch={} target_gpu=0x{:X} buffers=3 plane_slot={} cpu_readback={} cpu_frame_copy=0 cursor_overlay=gpgpu-worklist retained_scene=1 coverage_submits={} coverage_walkers={} final_release=pat3-uc+pipe-control-post-sync publish=exact-surface surflive=display-ownership\n",
            destination.width,
            destination.height,
            destination.pitch_bytes,
            destination.gpu,
            UI4_PLANE_SLOT,
            result.rgba.is_some() as u8,
            result.coverage_submits,
            result.coverage_walkers,
        );
    }
    let Some(release) = result.release_fence else {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(ServiceError::Render("missing-gridpaper-release-fence"));
    };
    if let Err(error) = crate::ui4::publish_gpu_frame_buffer(lease, release) {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(error.into());
    }
    crate::ui4::publish_window_frame(UI4_OWNER, presentation.window, crate::ui4::DamageRect::FULL)?;
    Ok(result)
}

fn capture_resident_page_frame(
    page: &ResidentPage,
    text_animations: &[Option<GpuFontColorProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_elapsed_ms: u64,
    viewport_translation_px: [f32; 2],
    width: u32,
    height: u32,
    final_rects: &[crate::intel::gpgpu::GpgpuSolidRect],
    destination: Option<crate::intel::gpgpu::GpgpuRgba8Surface>,
) -> Result<crate::intel::render::ResidentSceneFrameResult, &'static str> {
    let triangle_draws = page
        .layers
        .iter()
        .filter(|layer| layer.coverage.is_none())
        .map(|layer| crate::intel::render::ResidentSceneDraw {
            mesh: &layer.mesh,
            rgba: resident_layer_color(layer, text_animations, animation_elapsed_ms),
            viewport_translation_px,
        })
        .collect::<Vec<_>>();
    let pan_px = [
        libm::roundf(viewport_translation_px[0]) as i32,
        libm::roundf(viewport_translation_px[1]) as i32,
    ];
    let coverage_draws = page
        .layers
        .iter()
        .filter_map(|layer| {
            let coverage = layer.coverage.as_ref()?;
            let origin = coverage.origin_px();
            Some(crate::intel::render::ResidentSceneCoverageDraw {
                mask: coverage.surface(),
                mask_rect: coverage.full_rect(),
                dst_xy: crate::intel::gpgpu::GpgpuPoint::new(
                    origin[0].saturating_add(pan_px[0]),
                    origin[1].saturating_add(pan_px[1]),
                ),
                color_rgba: u32::from_le_bytes(resident_layer_color(
                    layer,
                    text_animations,
                    animation_elapsed_ms,
                )),
            })
        })
        .collect::<Vec<_>>();
    let captured = if let Some(destination) = destination {
        crate::intel::render::render_resident_triangle_scene_frame_premultiplied_msaa4_with_coverage_and_rects_to_surface(
            &triangle_draws,
            &coverage_draws,
            final_rects,
            Some([0, 0, 0, 0]),
            destination,
            false,
        )
    } else {
        crate::intel::render::capture_resident_triangle_scene_frame_premultiplied_at_extent_msaa4_with_coverage(
            &triangle_draws,
            &coverage_draws,
            Some([0, 0, 0, 0]),
            width,
            height,
            false,
        )
    };
    match captured {
        Ok(result)
            if result.completed_draws == result.requested_draws
                && (destination.is_some() || result.rgba.is_some()) =>
        {
            Ok(result)
        }
        Ok(_) if coverage_draws.is_empty() => Err("incomplete-frame"),
        Err(reason) if coverage_draws.is_empty() => Err(reason),
        failed => {
            if !COVERAGE_COMPOSITE_FALLBACK_LOGGED.swap(true, core::sync::atomic::Ordering::AcqRel)
            {
                let reason = match failed {
                    Ok(_) => "incomplete-coverage-frame",
                    Err(reason) => reason,
                };
                crate::log_warn!(
                    target: "gridpaper";
                    "gridpaper: analytical coverage composite failed reason={} masks={} action=rerender-resident-triangle-fallback\n",
                    reason,
                    coverage_draws.len(),
                );
            }
            let fallback_draws = page
                .layers
                .iter()
                .map(|layer| crate::intel::render::ResidentSceneDraw {
                    mesh: &layer.mesh,
                    rgba: resident_layer_color(layer, text_animations, animation_elapsed_ms),
                    viewport_translation_px,
                })
                .collect::<Vec<_>>();
            let fallback = if let Some(destination) = destination {
                crate::intel::render::render_resident_triangle_scene_frame_premultiplied_msaa4_with_coverage_and_rects_to_surface(
                    &fallback_draws,
                    &[],
                    final_rects,
                    Some([0, 0, 0, 0]),
                    destination,
                    false,
                )
            } else {
                crate::intel::render::capture_resident_triangle_scene_frame_premultiplied_at_extent_msaa4(
                    &fallback_draws,
                    Some([0, 0, 0, 0]),
                    width,
                    height,
                    false,
                )
            }?;
            if fallback.completed_draws != fallback.requested_draws
                || (destination.is_none() && fallback.rgba.is_none())
            {
                return Err("incomplete-fallback-frame");
            }
            Ok(fallback)
        }
    }
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
            PRIMARY_INSTANCE_ID,
            &snapshot,
            PRINT_CAPTURE_WIDTH,
            PRINT_CAPTURE_HEIGHT,
            ScenePan::ZERO,
        )?;
        let captured = capture_resident_page_frame(
            &page,
            text_animations,
            animation_elapsed_ms,
            [0.0, 0.0],
            PRINT_CAPTURE_WIDTH,
            PRINT_CAPTURE_HEIGHT,
            &[],
            None,
        )?;
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

fn grid_cursor_rects(
    surface: &GridPaperSurface,
    page: &ResidentPage,
    selection: GridCellSelection,
    input_field: CellInputField,
) -> Option<[crate::intel::gpgpu::GpgpuSolidRect; 4]> {
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
        return None;
    }
    let stroke = GRID_CURSOR_STROKE_PX
        .min(clipped_right - clipped_left)
        .min(clipped_bottom - clipped_top);
    let width = clipped_right - clipped_left;
    let height = clipped_bottom - clipped_top;
    let color_rgba = u32::from_le_bytes(GRID_CURSOR_RGBA);
    let solid = |rect| crate::intel::gpgpu::GpgpuSolidRect { rect, color_rgba };
    Some([
        solid(crate::intel::gpgpu::GpgpuRect::new(
            clipped_left as i32,
            clipped_top as i32,
            width,
            stroke,
        )),
        solid(crate::intel::gpgpu::GpgpuRect::new(
            clipped_left as i32,
            clipped_bottom.saturating_sub(stroke) as i32,
            width,
            stroke,
        )),
        solid(crate::intel::gpgpu::GpgpuRect::new(
            clipped_left as i32,
            clipped_top as i32,
            stroke,
            height,
        )),
        solid(crate::intel::gpgpu::GpgpuRect::new(
            clipped_right.saturating_sub(stroke) as i32,
            clipped_top as i32,
            stroke,
            height,
        )),
    ])
}

struct GridPaperRuntime {
    surface: GridPaperSurface,
    observed_serial: u64,
    observed_animation_serial: u64,
    text_animations: [Option<GpuFontColorProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_started_ms: u64,
    animation_dirty: bool,
    last_sampled_text_colors: [Option<[u8; 4]>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_frames: u64,
    latest_snapshot: Option<OwnedSnapshot>,
    queued_snapshot: Option<OwnedSnapshot>,
    pending: Option<ResidentPage>,
    active: Option<ResidentPage>,
    pan: ScenePan,
    pan_dirty: bool,
    hot_pan_frames: u64,
    active_pan_source: Option<crate::ui4::Ui4CursorSource>,
    pending_pan_pixels: (i32, i32),
    selection: Option<GridCellSelection>,
    input_field: CellInputField,
    cursor_dirty: bool,
    keyboard_edits: u64,
    last_build_error: Option<&'static str>,
    last_render_error: Option<ServiceError>,
}

impl GridPaperRuntime {
    fn new(surface: GridPaperSurface) -> Self {
        Self {
            surface,
            observed_serial: 0,
            observed_animation_serial: 0,
            text_animations: [None; TEXT_ANIMATION_COLOR_SLOTS],
            animation_started_ms: Instant::now().as_millis(),
            animation_dirty: false,
            last_sampled_text_colors: [None; TEXT_ANIMATION_COLOR_SLOTS],
            animation_frames: 0,
            latest_snapshot: None,
            queued_snapshot: None,
            pending: None,
            active: None,
            pan: ScenePan::ZERO,
            pan_dirty: false,
            hot_pan_frames: 0,
            active_pan_source: None,
            pending_pan_pixels: (0, 0),
            selection: None,
            input_field: CellInputField::Primary,
            cursor_dirty: false,
            keyboard_edits: 0,
            last_build_error: None,
            last_render_error: None,
        }
    }

    fn presented_owner(&self) -> Option<u8> {
        self.surface
            .presentation
            .map(|presentation| presentation.producer)
    }

    fn presented_window(&self) -> Option<crate::ui4::WindowId> {
        self.surface
            .presentation
            .map(|presentation| presentation.window)
    }

    fn reset_detached_input(&mut self) {
        if self.selection.take().is_some() {
            self.cursor_dirty = true;
        }
        self.input_field = CellInputField::Primary;
        self.active_pan_source = None;
        self.pending_pan_pixels = (0, 0);
    }
}

struct InputRoute {
    window: Option<crate::ui4::WindowId>,
    events: VecDeque<crate::ui4::Ui4InputEvent>,
}

impl InputRoute {
    const fn new() -> Self {
        Self {
            window: None,
            events: VecDeque::new(),
        }
    }

    fn push_event(&mut self, event: crate::ui4::Ui4InputEvent) {
        use crate::ui4::Ui4InputEvent;

        // Pointer motion is state-like: only the newest absolute position is
        // useful before the worker drains this route. Do not let it crowd out
        // focus, button, or keyboard transitions while a GPU frame is busy.
        if let Ui4InputEvent::Pointer(next) = event
            && next.wheel == 0
            && next.buttons_pressed == 0
            && next.buttons_released == 0
            && let Some(Ui4InputEvent::Pointer(previous)) = self.events.back_mut()
            && previous.window == next.window
            && previous.source == next.source
            && previous.buttons_down == next.buttons_down
            && previous.wheel == 0
            && previous.buttons_pressed == 0
            && previous.buttons_released == 0
        {
            *previous = next;
            return;
        }

        if self.events.len() == INPUT_QUEUE_CAPACITY_PER_INSTANCE {
            let replaceable = self.events.iter().position(|queued| match queued {
                Ui4InputEvent::Pointer(pointer) => {
                    pointer.wheel == 0
                        && pointer.buttons_pressed == 0
                        && pointer.buttons_released == 0
                }
                Ui4InputEvent::Pan(pan) => pan.phase == crate::ui4::Ui4PanPhase::Update,
                _ => false,
            });
            if let Some(index) = replaceable {
                self.events.remove(index);
            } else if matches!(
                event,
                Ui4InputEvent::Pointer(_)
                    | Ui4InputEvent::Pan(crate::ui4::Ui4PanEvent {
                        phase: crate::ui4::Ui4PanPhase::Update,
                        ..
                    })
            ) {
                return;
            } else {
                self.events.pop_front();
            }
        }
        self.events.push_back(event);
    }
}

static INPUT_ROUTES: Mutex<[InputRoute; GRIDPAPER_POOL_SOFT_CAP]> =
    Mutex::new([const { InputRoute::new() }; GRIDPAPER_POOL_SOFT_CAP]);
static GPU_RENDER_LANE: AsyncMutex<crate::wait::EmbassySpinRawMutex, ()> = AsyncMutex::new(());

fn set_input_route(pool_slot: usize, window: Option<crate::ui4::WindowId>) {
    let mut routes = INPUT_ROUTES.lock();
    let route = &mut routes[pool_slot];
    route.window = window;
    route.events.clear();
}

fn input_event_window(event: crate::ui4::Ui4InputEvent) -> crate::ui4::WindowId {
    match event {
        crate::ui4::Ui4InputEvent::Pointer(event) => event.window,
        crate::ui4::Ui4InputEvent::Button(event) => event.window,
        crate::ui4::Ui4InputEvent::Pan(event) => event.window,
        crate::ui4::Ui4InputEvent::Resize(event) => event.window,
        crate::ui4::Ui4InputEvent::Keyboard(event) => event.window,
        crate::ui4::Ui4InputEvent::Focus(event) => event.window,
    }
}

fn route_input_events() {
    let events = crate::ui4::take_owner_input_events(UI4_OWNER);
    if events.is_empty() {
        return;
    }
    let mut routes = INPUT_ROUTES.lock();
    for event in events {
        let window = input_event_window(event);
        let Some(route) = routes.iter_mut().find(|route| route.window == Some(window)) else {
            continue;
        };
        route.push_event(event);
    }
}

fn take_routed_input_events(pool_slot: usize) -> VecDeque<crate::ui4::Ui4InputEvent> {
    let mut routes = INPUT_ROUTES.lock();
    core::mem::take(&mut routes[pool_slot].events)
}

fn attach_runtime_presentation(
    runtime: &mut GridPaperRuntime,
    producer: u8,
) -> Result<crate::ui4::WindowSessionId, ServiceError> {
    let session = crate::ui4::begin_window_session(UI4_OWNER)?;
    if let Err(error) =
        attach_presentation(&mut runtime.surface, producer, session, runtime.active.is_some())
    {
        let _ = crate::ui4::finish_window_session(UI4_OWNER, session);
        runtime.surface.presentation = None;
        return Err(error);
    }
    set_input_route(runtime.surface.pool_slot, runtime.presented_window());
    Ok(session)
}

fn release_runtime_presentation(
    runtime: &mut GridPaperRuntime,
    session: crate::ui4::WindowSessionId,
    retire_frame: bool,
) -> bool {
    let close_request = if retire_frame {
        crate::ui4::WindowSessionCloseRequest::default()
            .direct_plane_animate_and_retire_frames()
    } else {
        crate::ui4::WindowSessionCloseRequest::default().direct_plane_animate()
    };
    let release = crate::ui4::finish_window_session_with_request(UI4_OWNER, session, close_request);
    let frame_transferred = retire_frame && release.is_ok();
    set_input_route(runtime.surface.pool_slot, None);
    let Some(presentation) = release_presentation(&mut runtime.surface) else {
        return frame_transferred;
    };
    runtime.reset_detached_input();
    match release {
        Ok(closed_windows) => crate::log_info!(
            target: "gridpaper";
            "gridpaper: presentation released pool_slot={} instance={} producer={} session={} window={} frame={} closed_windows={} retained_gpu_scene=1 retained_frame=1\n",
            runtime.surface.pool_slot,
            runtime.surface.instance_id,
            presentation.producer,
            presentation.session.raw(),
            presentation.window.raw(),
            runtime.surface.frame.raw(),
            closed_windows,
        ),
        Err(error) => crate::log_warn!(
            target: "gridpaper";
            "gridpaper: presentation release pool_slot={} instance={} producer={} session={} window={} frame={} error={:?} action=consider-detached retained_gpu_scene=1 retained_frame=1\n",
            runtime.surface.pool_slot,
            runtime.surface.instance_id,
            presentation.producer,
            presentation.session.raw(),
            presentation.window.raw(),
            runtime.surface.frame.raw(),
            error,
        ),
    }
    frame_transferred
}

fn destroy_runtime(
    mut runtime: GridPaperRuntime,
    session: &mut Option<crate::ui4::WindowSessionId>,
) {
    let frame_transferred = session.take().is_some_and(|active_session| {
        release_runtime_presentation(&mut runtime, active_session, true)
    });
    set_input_route(runtime.surface.pool_slot, None);
    if !frame_transferred {
        let _ = crate::ui4::destroy_frame(runtime.surface.frame);
    }
}

fn refresh_runtime(runtime: &mut GridPaperRuntime) {
    let pool_slot = runtime.surface.pool_slot;
    let instance_id = runtime.surface.instance_id;
    if let Some(update) = text_animations_after(pool_slot, runtime.observed_animation_serial) {
        runtime.observed_animation_serial = update.serial;
        runtime.text_animations = update.programs;
        runtime.animation_started_ms = Instant::now().as_millis();
        runtime.animation_dirty = true;
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: text-animation-table activated pool_slot={} instance={} serial={} programs={} cadence_ms={} clock=monotonic-elapsed geometry_uploads=0\n",
            pool_slot,
            instance_id,
            runtime.observed_animation_serial,
            runtime.text_animations.iter().flatten().count(),
            SERVICE_PERIOD_MS,
        );
    }

    if let Some(snapshot) = snapshot_after(pool_slot, runtime.observed_serial) {
        runtime.observed_serial = snapshot.serial;
        let clamped_pan = runtime.pan.clamped(snapshot.scale_percent);
        if clamped_pan != runtime.pan {
            runtime.pan = clamped_pan;
            if let Some(page) = runtime.active.as_mut() {
                page.pan = runtime.pan;
            }
            runtime.pan_dirty = true;
        }
        runtime.pending = None;
        runtime.latest_snapshot = Some(snapshot.clone());
        runtime.queued_snapshot = Some(snapshot);
    }
}

fn select_gridpaper_cell(runtime: &mut GridPaperRuntime, local_x: i32, local_y: i32) {
    let scale_percent = runtime
        .active
        .as_ref()
        .map(|page| page.scale_percent)
        .or_else(|| runtime.pending.as_ref().map(|page| page.scale_percent))
        .or_else(|| {
            runtime
                .latest_snapshot
                .as_ref()
                .map(|snapshot| snapshot.scale_percent)
        });
    let next = scale_percent.and_then(|scale_percent| {
        grid_cell_at_local_point(&runtime.surface, local_x, local_y, scale_percent, runtime.pan)
    });
    if next == runtime.selection {
        return;
    }
    runtime.selection = next;
    runtime.input_field = CellInputField::Primary;
    runtime.cursor_dirty = true;
    if let Some(selected) = runtime.selection {
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: cell selected instance={} column={} row={} local={},{} scale={} pan_scene={:.3},{:.3} input=ui4-primary-click\n",
            runtime.surface.instance_id,
            selected.column,
            selected.row,
            local_x,
            local_y,
            scale_percent.unwrap_or(0),
            runtime.pan.x,
            runtime.pan.y,
        );
    } else {
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: cell selection cleared instance={} local={},{} input=ui4-primary-click-outside-grid\n",
            runtime.surface.instance_id,
            local_x,
            local_y,
        );
    }
}

fn edit_gridpaper_cell(
    runtime: &mut GridPaperRuntime,
    event: crate::r::keyboard::TrueosKeyboardOutputEvent,
) {
    let Some(mut selected) = runtime.selection else {
        return;
    };
    let Some(snapshot) = runtime.latest_snapshot.as_mut() else {
        return;
    };
    let outcome =
        edit_snapshot_from_keyboard(snapshot, &mut selected, &mut runtime.input_field, event);
    if outcome.capacity_rejected {
        crate::log_warn!(
            target: "gridpaper";
            "gridpaper: cell input rejected instance={} column={} row={} field={} rule=one-unicode-scalar-and-upper-requires-primary input=ui4-keyboard\n",
            runtime.surface.instance_id,
            selected.column,
            selected.row,
            runtime.input_field.name(),
        );
    }
    if outcome.input_field_changed {
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: cell input field toggled instance={} column={} row={} field={} key=tab input=ui4-focused-keyboard\n",
            runtime.surface.instance_id,
            selected.column,
            selected.row,
            runtime.input_field.name(),
        );
    }
    if outcome.clear_selection {
        runtime.selection = None;
        runtime.input_field = CellInputField::Primary;
        runtime.cursor_dirty = true;
    } else {
        runtime.selection = Some(selected);
        runtime.cursor_dirty |= outcome.selection_changed || outcome.input_field_changed;
    }
    if !outcome.content_changed {
        return;
    }
    runtime.keyboard_edits = runtime.keyboard_edits.saturating_add(1);
    let edited = outcome.edited_cell.unwrap_or(selected);
    let offset = (edited.row * COLUMNS + edited.column) * CELL_BYTES;
    let primary_len = usize::from(snapshot.raw[offset + PRIMARY_LENGTH_OFFSET]);
    let upper_len = usize::from(snapshot.raw[offset + UPPER_LENGTH_OFFSET]);
    runtime.queued_snapshot = Some(snapshot.clone());
    runtime.pending = None;
    crate::log_info!(
        target: "gridpaper";
        "gridpaper: cell edited instance={} seq={} column={} row={} field={} primary_utf8_bytes={} upper_utf8_bytes={} key_kind={} codepoint={} input=ui4-keyboard action=rebuild-page\n",
        runtime.surface.instance_id,
        runtime.keyboard_edits,
        edited.column,
        edited.row,
        runtime.input_field.name(),
        primary_len,
        upper_len,
        event.kind,
        event.codepoint,
    );
}

fn pan_gridpaper(runtime: &mut GridPaperRuntime, event: crate::ui4::Ui4PanEvent) {
    match event.phase {
        crate::ui4::Ui4PanPhase::Begin => {
            runtime.active_pan_source = Some(event.source);
            runtime.pending_pan_pixels = (0, 0);
        }
        crate::ui4::Ui4PanPhase::Update if runtime.active_pan_source == Some(event.source) => {
            runtime.pending_pan_pixels.0 = runtime.pending_pan_pixels.0.saturating_add(event.dx);
            runtime.pending_pan_pixels.1 = runtime.pending_pan_pixels.1.saturating_add(event.dy);
            let Some(snapshot) = runtime.latest_snapshot.as_ref() else {
                return;
            };
            if runtime.pan.drag_pixels(
                event.dx,
                event.dy,
                runtime.surface.width,
                runtime.surface.height,
                snapshot.scale_percent,
            ) {
                if let Some(page) = runtime.pending.as_mut() {
                    page.pan = runtime.pan;
                }
                if let Some(page) = runtime.active.as_mut() {
                    page.pan = runtime.pan;
                }
                runtime.pan_dirty = true;
            }
        }
        crate::ui4::Ui4PanPhase::End if runtime.active_pan_source == Some(event.source) => {
            runtime.active_pan_source = None;
            let (drag_x, drag_y) = runtime.pending_pan_pixels;
            runtime.pending_pan_pixels = (0, 0);
            if drag_x != 0 || drag_y != 0 {
                crate::log_info!(
                    target: "gridpaper";
                    "gridpaper: middle-pan ended instance={} drag_px={},{} pan_scene={:.3},{:.3} hot_frames_total={} action=retain-resident-meshes\n",
                    runtime.surface.instance_id,
                    drag_x,
                    drag_y,
                    runtime.pan.x,
                    runtime.pan.y,
                    runtime.hot_pan_frames,
                );
            }
        }
        _ => {}
    }
}

fn dispatch_gridpaper_input(runtime: &mut GridPaperRuntime, event: crate::ui4::Ui4InputEvent) {
    if runtime.presented_window() != Some(input_event_window(event)) {
        return;
    }
    match event {
        crate::ui4::Ui4InputEvent::Button(event)
            if event.phase == crate::ui4::Ui4ButtonPhase::Down
                && event.changed_buttons & PRIMARY_BUTTON_MASK != 0 =>
        {
            select_gridpaper_cell(runtime, event.local_x, event.local_y);
        }
        crate::ui4::Ui4InputEvent::Keyboard(event)
            if event.event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
                && event.event.key_code == crate::r::keyboard::KEYBOARD_KEY_F10 =>
        {
            if let Some(snapshot) = runtime.latest_snapshot.as_ref()
                && queue_print_request(runtime.surface.instance_id, snapshot).is_none()
            {
                crate::log_os::print2d_job_state(0, "request-dropped", "gridpaper-F10-queue-full");
            }
        }
        crate::ui4::Ui4InputEvent::Keyboard(event) => {
            edit_gridpaper_cell(runtime, event.event);
        }
        crate::ui4::Ui4InputEvent::Pan(event) => {
            pan_gridpaper(runtime, event);
        }
        _ => {}
    }
}

fn build_queued_page(runtime: &mut GridPaperRuntime) {
    let Some(snapshot) = runtime.queued_snapshot.as_ref() else {
        return;
    };
    match build_resident_page(
        runtime.surface.instance_id,
        snapshot,
        runtime.surface.width,
        runtime.surface.height,
        runtime.pan,
    ) {
        Ok(page) => {
            runtime.pending = Some(page);
            runtime.queued_snapshot = None;
            runtime.last_build_error = None;
        }
        Err(error) if runtime.last_build_error != Some(error) => {
            crate::log_warn!(
                target: "gridpaper";
                "gridpaper: snapshot build pending instance={} serial={} generation={} reason={} action=retain-front\n",
                runtime.surface.instance_id,
                snapshot.serial,
                snapshot.generation,
                error,
            );
            runtime.last_build_error = Some(error);
        }
        Err(_) => {}
    }
}

fn runtime_needs_render(runtime: &GridPaperRuntime, now_ms: u64) -> bool {
    if runtime.surface.presentation.is_none() {
        return false;
    }
    if runtime.pending.is_some() {
        return true;
    }
    let Some(page) = runtime.active.as_ref() else {
        return false;
    };
    if runtime.pan_dirty || runtime.cursor_dirty || runtime.animation_dirty {
        return true;
    }
    let elapsed_ms = now_ms.saturating_sub(runtime.animation_started_ms);
    sampled_text_colors(page, &runtime.text_animations, elapsed_ms)
        != runtime.last_sampled_text_colors
}

fn publish_runtime(runtime: &mut GridPaperRuntime, now_ms: u64) {
    let animation_elapsed_ms = now_ms.saturating_sub(runtime.animation_started_ms);
    let mut published_page_this_tick = false;
    if runtime.surface.presentation.is_some()
        && let Some(candidate) = runtime.pending.as_ref()
    {
        match publish_page(
            &runtime.surface,
            candidate,
            runtime.selection,
            runtime.input_field,
            &runtime.text_animations,
            animation_elapsed_ms,
        ) {
            Ok(result) => {
                let published = runtime
                    .pending
                    .take()
                    .expect("gridpaper pending page exists");
                runtime.last_sampled_text_colors =
                    sampled_text_colors(&published, &runtime.text_animations, animation_elapsed_ms);
                let coverage_masks = published
                    .layers
                    .iter()
                    .filter(|layer| layer.coverage.is_some())
                    .count();
                crate::log_info!(
                    target: "gridpaper";
                    "gridpaper: frame published instance={} serial={} generation={} scale={} pan_scene={:.3},{:.3} layers={} coverage_masks={} coverage_submits={} coverage_walkers={} changed_pixels={} frame_us={} geometry_us={} resolve_us={} coverage_us={} present_copy_us={} font_path=kernel-font-stamp-default/skrifa-gpgpu-r8-or-triangle-fallback persistence=resident-until-next-snapshot pan_transform=sf-viewport frame_path=gpu-direct cpu_readback=0 cpu_frame_copy=0\n",
                    runtime.surface.instance_id,
                    published.serial,
                    published.generation,
                    published.scale_percent,
                    published.pan.x,
                    published.pan.y,
                    published.layers.len(),
                    coverage_masks,
                    result.coverage_submits,
                    result.coverage_walkers,
                    result.changed_pixels,
                    result.frame_us,
                    result.geometry_us,
                    result.resolve_us,
                    result.coverage_us,
                    result.present_copy_us,
                );
                let retired = runtime.active.replace(published);
                drop(retired);
                runtime.animation_dirty = false;
                runtime.pan_dirty = false;
                runtime.cursor_dirty = false;
                published_page_this_tick = true;
                runtime.last_render_error = None;
            }
            Err(error) if runtime.last_render_error != Some(error) => {
                crate::log_warn!(
                    target: "gridpaper";
                    "gridpaper: frame pending instance={} serial={} generation={} error={:?} action=retain-front-and-retry\n",
                    runtime.surface.instance_id,
                    candidate.serial,
                    candidate.generation,
                    error,
                );
                runtime.last_render_error = Some(error);
            }
            Err(_) => {}
        }
    }

    if published_page_this_tick
        || runtime.surface.presentation.is_none()
        || runtime.pending.is_some()
    {
        return;
    }
    let Some(page) = runtime.active.as_ref() else {
        return;
    };
    let sampled = sampled_text_colors(page, &runtime.text_animations, animation_elapsed_ms);
    let animation_changed = runtime.animation_dirty || sampled != runtime.last_sampled_text_colors;
    let hot_pan_frame = runtime.pan_dirty;
    let selection_frame = runtime.cursor_dirty;
    if !animation_changed && !hot_pan_frame && !selection_frame {
        return;
    }
    match publish_page(
        &runtime.surface,
        page,
        runtime.selection,
        runtime.input_field,
        &runtime.text_animations,
        animation_elapsed_ms,
    ) {
        Ok(result) => {
            runtime.last_sampled_text_colors = sampled;
            runtime.animation_dirty = false;
            runtime.pan_dirty = false;
            runtime.cursor_dirty = false;
            if hot_pan_frame {
                runtime.hot_pan_frames = runtime.hot_pan_frames.saturating_add(1);
                if runtime.hot_pan_frames <= 8 || runtime.hot_pan_frames.is_multiple_of(120) {
                    crate::log_info!(
                        target: "gridpaper";
                        "gridpaper: hot-pan-frame instance={} seq={} pan_scene={:.3},{:.3} coverage_submits={} coverage_walkers={} changed_pixels={} frame_us={} geometry_us={} resolve_us={} coverage_us={} present_copy_us={} geometry_uploads=0 resident_mesh_rebuilds=0 transform=sf-viewport preclip=translated-bypass final_clip=scissor frame_path=gpu-direct cpu_readback=0 cpu_frame_copy=0\n",
                        runtime.surface.instance_id,
                        runtime.hot_pan_frames,
                        page.pan.x,
                        page.pan.y,
                        result.coverage_submits,
                        result.coverage_walkers,
                        result.changed_pixels,
                        result.frame_us,
                        result.geometry_us,
                        result.resolve_us,
                        result.coverage_us,
                        result.present_copy_us,
                    );
                }
            }
            if animation_changed {
                runtime.animation_frames = runtime.animation_frames.saturating_add(1);
            }
            if animation_changed
                && (runtime.animation_frames <= 8 || runtime.animation_frames.is_multiple_of(120))
            {
                crate::log_info!(
                    target: "gridpaper";
                    "gridpaper: text-animation-frame instance={} seq={} animation_serial={} elapsed_ms={} programs={} coverage_submits={} coverage_walkers={} changed_pixels={} frame_us={} geometry_us={} resolve_us={} coverage_us={} present_copy_us={} geometry_uploads=0 resident_mesh_rebuilds=0 frame_path=gpu-direct cpu_readback=0 cpu_frame_copy=0\n",
                    runtime.surface.instance_id,
                    runtime.animation_frames,
                    runtime.observed_animation_serial,
                    animation_elapsed_ms,
                    runtime.text_animations.iter().flatten().count(),
                    result.coverage_submits,
                    result.coverage_walkers,
                    result.changed_pixels,
                    result.frame_us,
                    result.geometry_us,
                    result.resolve_us,
                    result.coverage_us,
                    result.present_copy_us,
                );
            }
            runtime.last_render_error = None;
        }
        Err(error) if runtime.last_render_error != Some(error) => {
            crate::log_warn!(
                target: "gridpaper";
                "gridpaper: text-animation-frame pending instance={} serial={} elapsed_ms={} error={:?} action=retain-front-and-retry\n",
                runtime.surface.instance_id,
                runtime.observed_animation_serial,
                animation_elapsed_ms,
                error,
            );
            runtime.last_render_error = Some(error);
        }
        Err(_) => {}
    }
}

#[embassy_executor::task(pool_size = GRIDPAPER_POOL_SOFT_CAP)]
async fn gridpaper_instance_worker_task(pool_slot: usize) {
    crate::intel::wait_hw_logo_sequence_done().await;
    crate::log_info!(
        target: "gridpaper";
        "gridpaper: pool worker online pool_slot={} carrier_slot={} render_lane=shared-async-serialized\n",
        pool_slot,
        crate::percpu::current_slot(),
    );

    let mut observed_lease_epoch = 0u64;
    let mut runtime: Option<GridPaperRuntime> = None;
    let mut presentation_session = None;
    let mut last_init_error = None;
    let mut last_presentation_error = None;
    loop {
        let lease = pool_lease_state(pool_slot);
        if lease.epoch != observed_lease_epoch {
            if let Some(old_runtime) = runtime.take() {
                destroy_runtime(old_runtime, &mut presentation_session);
            }
            observed_lease_epoch = lease.epoch;
            last_init_error = None;
            last_presentation_error = None;
        }

        let Some(instance_id) = lease.local_instance_id else {
            Timer::after(EmbassyDuration::from_millis(250)).await;
            continue;
        };

        if runtime.is_none() {
            match initialize_surface(pool_slot, instance_id) {
                Ok(surface) => {
                    crate::log_info!(
                        target: "gridpaper";
                        "gridpaper: pool runtime activated pool_slot={} owner={} local_instance={} worker_slot={} ui4={}x{} extent_source={} default_scale={}\n",
                        pool_slot,
                        lease.owner.unwrap_or(u8::MAX),
                        instance_id,
                        crate::percpu::current_slot(),
                        surface.width,
                        surface.height,
                        surface.extent_source,
                        NATIVE_SCALE_PERCENT,
                    );
                    runtime = Some(GridPaperRuntime::new(surface));
                    last_init_error = None;
                }
                Err(error) => {
                    if last_init_error != Some(error) {
                        crate::log_warn!(
                            target: "gridpaper";
                            "gridpaper: UI4 surface pending pool_slot={} instance={} error={:?} action=retry\n",
                            pool_slot,
                            instance_id,
                            error,
                        );
                        last_init_error = Some(error);
                    }
                    Timer::after(EmbassyDuration::from_millis(250)).await;
                    continue;
                }
            }
        }

        let runtime_ref = runtime
            .as_mut()
            .expect("leased GridPaper runtime initialized");
        if lease.presentable_owner != runtime_ref.presented_owner() {
            if let Some(session) = presentation_session.take() {
                let _ = release_runtime_presentation(runtime_ref, session, false);
            }
            if let Some(producer) = lease.presentable_owner {
                match attach_runtime_presentation(runtime_ref, producer) {
                    Ok(session) => {
                        presentation_session = Some(session);
                        let presentation = runtime_ref
                            .surface
                            .presentation
                            .expect("attached GridPaper presentation");
                        crate::log_info!(
                            target: "gridpaper";
                            "gridpaper: presentation attached pool_slot={} instance={} producer={} session={} window={} frame={} retained_front={} persistent_gpu_scene=1\n",
                            pool_slot,
                            runtime_ref.surface.instance_id,
                            presentation.producer,
                            presentation.session.raw(),
                            presentation.window.raw(),
                            runtime_ref.surface.frame.raw(),
                            u8::from(runtime_ref.active.is_some()),
                        );
                        last_presentation_error = None;
                    }
                    Err(error) if last_presentation_error != Some(error) => {
                        crate::log_warn!(
                            target: "gridpaper";
                            "gridpaper: presentation attach pending pool_slot={} instance={} error={:?} action=retry retained_gpu_scene=1\n",
                            pool_slot,
                            runtime_ref.surface.instance_id,
                            error,
                        );
                        last_presentation_error = Some(error);
                    }
                    Err(_) => {}
                }
            } else {
                last_presentation_error = None;
            }
        }

        refresh_runtime(runtime_ref);
        for event in take_routed_input_events(pool_slot) {
            dispatch_gridpaper_input(runtime_ref, event);
        }
        build_queued_page(runtime_ref);

        let now_ms = Instant::now().as_millis();
        if runtime_needs_render(runtime_ref, now_ms) {
            let _render_lane = GPU_RENDER_LANE.lock().await;
            if pool_lease_state(pool_slot).epoch == observed_lease_epoch {
                publish_runtime(runtime_ref, Instant::now().as_millis());
            }
        }

        Timer::after(EmbassyDuration::from_millis(SERVICE_PERIOD_MS)).await;
    }
}

fn spawn_gridpaper_instance_pool() -> usize {
    let mut spawned = 0usize;
    for pool_slot in 0..GRIDPAPER_POOL_SOFT_CAP {
        let Some(spawner) = crate::workers::pick_background_spawner() else {
            break;
        };
        match gridpaper_instance_worker_task(pool_slot) {
            Ok(token) => {
                spawner.spawn(token);
                spawned += 1;
            }
            Err(error) => crate::log_warn!(
                target: "gridpaper";
                "gridpaper: pool worker spawn failed pool_slot={} error={:?}\n",
                pool_slot,
                error,
            ),
        }
    }
    spawned
}

/// Kernel controller for the GridPaper Blueprint worker pool. Each Blueprint
/// contributes one local document; up to ten owner-local leases retain their
/// own UI4 frame and scene worker. Only the current single physical RCS render
/// context is serialized across those workers.
#[embassy_executor::task]
pub async fn gridpaper_service_task() {
    crate::intel::wait_hw_logo_sequence_done().await;
    let spawned = spawn_gridpaper_instance_pool();
    crate::log_info!(
        target: "gridpaper";
        "gridpaper: embassy service ready controller_slot={} pool_workers={} soft_cap={} blueprint_instances=1 page_bytes={} cells={} snapshot_buffers_per_instance=2 ui4_buffers_per_active_instance=3 scene={}x{} document_mm={}x{} grid_mm={}x{} surface_mm={}x{} target_cell_mm={} default_scale={} scheduling=ap2+-worker-pool render_lane=single-rcs-async-fair input=window-routed presentation=ui4-triple-direct-slot2 direct_visible_capacity=1 retained_scene_capacity=10 release=pat3-uc+pipe-control-post-sync+surflive\n",
        crate::percpu::current_slot(),
        spawned,
        GRIDPAPER_POOL_SOFT_CAP,
        PAGE_BYTES,
        COLUMNS * ROWS,
        SCENE_WIDTH,
        SCENE_HEIGHT,
        A4_WIDTH_MM,
        A4_HEIGHT_MM,
        GRID_WIDTH_MM,
        GRID_HEIGHT_MM,
        SURFACE_WIDTH_MM,
        SURFACE_HEIGHT_MM,
        CELL_EDGE_MM,
        NATIVE_SCALE_PERCENT,
    );
    if spawned != GRIDPAPER_POOL_SOFT_CAP {
        crate::log_warn!(
            target: "gridpaper";
            "gridpaper: worker pool below soft cap spawned={} requested={} action=serve-available-slots\n",
            spawned,
            GRIDPAPER_POOL_SOFT_CAP,
        );
    }

    loop {
        route_input_events();
        if let Some(request) = PRINT_RENDER_REQUESTS.lock().pop_front() {
            let animations = SNAPSHOTS
                .lock()
                .iter()
                .find(|snapshot| snapshot.owner.is_some())
                .map(|snapshot| snapshot.text_animations)
                .unwrap_or([None; TEXT_ANIMATION_COLOR_SLOTS]);
            let _render_lane = GPU_RENDER_LANE.lock().await;
            let result = render_print_page(request, &animations, Instant::now().as_millis());
            PRINT_RENDER_RESULTS.lock().push_back(result);
        }
        Timer::after(EmbassyDuration::from_millis(SERVICE_PERIOD_MS)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_local_documents_lease_independent_pool_slots() {
        assert!(valid_local_instance(PRIMARY_INSTANCE_ID));
        assert!(!valid_local_instance(BLUEPRINT_INSTANCE_CAPACITY as u32));

        let mut stores = [const { SnapshotStore::new() }; GRIDPAPER_POOL_SOFT_CAP];
        stores[0].claim(3, PRIMARY_INSTANCE_ID);
        stores[1].claim(7, PRIMARY_INSTANCE_ID);
        stores[0].scale_percent = 125;
        stores[0].serial = 7;
        assert_eq!(find_pool_slot(&stores, 3, PRIMARY_INSTANCE_ID), Some(0));
        assert_eq!(find_pool_slot(&stores, 7, PRIMARY_INSTANCE_ID), Some(1));
        assert_eq!(stores[0].scale_percent, 125);
        assert_eq!(stores[0].serial, 7);
        assert_eq!(stores[1].scale_percent, NATIVE_SCALE_PERCENT);
        assert_eq!(stores[1].serial, 0);
    }

    #[test]
    fn sole_blueprint_instance_keeps_native_font_preference() {
        assert_eq!(font_preferences(PRIMARY_INSTANCE_ID)[0], GpuFontFace::Default);
    }

    #[test]
    fn fixed_wire_size_matches_a4_gridpaper() {
        assert_eq!(CELL_BYTES, 13);
        assert_eq!(PAGE_BYTES, 27_885);
        assert_eq!((COLUMNS, ROWS), (39, 55));
        assert_eq!(COLUMNS * ROWS, 2_145);
        assert_eq!((A4_WIDTH_MM, A4_HEIGHT_MM), (210, 297));
        assert_eq!(COLUMNS as u32 * CELL_EDGE_MM, 195);
        assert_eq!(ROWS as u32 * CELL_EDGE_MM, 275);
        assert_eq!((GRID_WIDTH_MM, GRID_HEIGHT_MM), (195, 275));
        assert_eq!((SURFACE_WIDTH_MM, SURFACE_HEIGHT_MM), (199, 279));
        assert_eq!((SCENE_WIDTH, SCENE_HEIGHT), (199, 279));
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
        assert!(!pan.drag_pixels(100, 100, 853, 1_196, 150));
        assert!(pan.drag_pixels(-10_000, -10_000, 853, 1_196, 150));
        assert_eq!(pan.x, -(SCENE_WIDTH as f32 * 0.5));
        assert_eq!(pan.y, -(SCENE_HEIGHT as f32 * 0.5));
        assert!(pan.drag_pixels(10_000, 10_000, 853, 1_196, 150));
        assert_eq!(pan, ScenePan::ZERO);

        assert!(!pan.drag_pixels(-100, -100, 853, 1_196, 100));
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
