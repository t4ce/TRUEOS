//! Embassy consumer for GridPaper's fixed snapshot format.
//!
//! Blueprint producers own their snapshot publication cadence. Spirit's one
//! retained response document uses the same resident workers through a narrow
//! kernel control plane. This service owns the accepted working copy, UI4
//! editing/focus state, GPU allocations, and presentation lifetime. No UI4
//! handles or generic drawing operations cross the Blueprint ABI.

use alloc::{collections::VecDeque, string::String, vec, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

use crate::intel::gpu_font::{
    GPU_FONT_COLOR_KEYFRAME_CAPACITY, GpuFontColorChannels, GpuFontColorIteration,
    GpuFontColorKeyframe, GpuFontColorKeyframes, GpuFontColorProgram, GpuFontColorTiming,
    GpuFontFace, GpuFontInstanceMotion, GpuFontInstanceProgram, GpuFontInstanceStyle, GpuFontRgba,
};

const COLUMN_SOFT_CAP: usize = 39;
const ROW_SOFT_CAP: usize = 55;
const COLUMNS: usize = COLUMN_SOFT_CAP;
const ROWS: usize = ROW_SOFT_CAP;
const GLYPH_UTF8_CAPACITY: usize = 4;
const CELL_BYTES: usize = 13;
pub(crate) const PAGE_BYTES: usize = COLUMNS * ROWS * CELL_BYTES;
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
const SPIRIT_RAINBOW_SELECTORS: [u8; 6] = [10, 12, 11, 15, 13, 14];
const SPIRIT_SCALE_MOTION_PERIOD_MS: u32 = 7_000;
const SPIRIT_SCALE_MOTION_AMPLITUDE_PERMILLE: i16 = 150;
// A slow seven-second breathing curve does not benefit from submitting sixty
// complete direct-scanout frames each second. Twenty samples per second are
// visually continuous here and leave substantially more RCS time for Lumen,
// UI4, and interactive work.
const SPIRIT_MOTION_FRAME_PERIOD_MS: u64 = 50;
const TEXT_ANIMATION_WIRE_VERSION_COLOR_ONLY: u8 = 1;
const TEXT_ANIMATION_WIRE_VERSION_FONT_INSTANCE: u8 = 2;
const TEXT_ANIMATION_WIRE_HEADER_BYTES: usize = 4;
const TEXT_ANIMATION_RECORD_HEADER_BYTES: usize = 12;
const TEXT_INSTANCE_RECORD_HEADER_BYTES: usize = 40;
const TEXT_ANIMATION_KEYFRAME_BYTES: usize = 8;
const MIN_ANIMATION_DURATION_MS: u32 = 16;
const MAX_ANIMATION_DURATION_MS: u32 = 600_000;
const MIN_SCALE_PERCENT: u32 = 1;
const MAX_SCALE_PERCENT: u32 = 800;
const GRIDPAPER_POOL_SOFT_CAP: usize = 10;
/// A Blueprint may expose as many local GridPaper documents as the pool has
/// resident service slots. The pool is keyed by `(owner, local_instance_id)`
/// and every slot is independent, so this is the pool's own depth rather than
/// a separate, smaller limit.
const BLUEPRINT_INSTANCE_CAPACITY: usize = GRIDPAPER_POOL_SOFT_CAP;
const PRIMARY_INSTANCE_ID: u32 = 0;
const NATIVE_SCALE_PERCENT: u16 = 100;

const DEFAULT_REGULAR_ROW_FONT_PIXELS: f32 = 24.0;
pub(crate) const A4_WIDTH_MM: u32 = 210;
pub(crate) const A4_HEIGHT_MM: u32 = 297;
const CELL_EDGE_MM: u32 = 5;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) const GRID_WIDTH_MM: u32 = COLUMNS as u32 * CELL_EDGE_MM;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) const GRID_HEIGHT_MM: u32 = ROWS as u32 * CELL_EDGE_MM;
pub(crate) const RULER_GUTTER_MM: u32 = 4;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const SURFACE_WIDTH_MM: u32 = RULER_GUTTER_MM + GRID_WIDTH_MM;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const SURFACE_HEIGHT_MM: u32 = RULER_GUTTER_MM + GRID_HEIGHT_MM;
const SMALL_TICK_LENGTH_MM: f32 = 1.25;
const CENTIMETER_TICK_LENGTH_MM: f32 = 2.5;
const THREE_CENTIMETER_TICK_LENGTH_MM: f32 = 4.0;
const DECORATION_INSET_MM: f32 = 0.5;
const UI4_OWNER: crate::ui4::WindowOwner = crate::ui4::WindowOwner::GRIDPAPER_SERVICE;
const UI4_PLANE_SLOT: usize = crate::ui4::RGB_OVERLAY_PLANE_SLOT_2;
const _: () = assert!(UI4_PLANE_SLOT == 2);
const SERVICE_PERIOD_MS: u64 = 16;
const PRIMARY_BUTTON_MASK: u32 = 1 << 0;
// One pasted Unicode scalar becomes one retained keyboard transition. Keep a
// complete page worth of text plus navigation/control transitions so paste
// cannot evict its own prefix while the GPU worker is finishing a frame.
const INPUT_QUEUE_CAPACITY_PER_INSTANCE: usize = COLUMNS * ROWS * 2;
const CELL_PATCH_BUILD_BUDGET_PER_TICK: usize = 16;
const CELL_PATCH_COVERAGE_BATCH_CAPACITY: usize = 64;
// The off-screen static base has a shorter retirement deadline than a live
// scanout surface. Submit full-page identity masks separately so one slow
// aggregate layer cannot turn a healthy sequence into a false batch timeout.
const STATIC_BASE_COVERAGE_BATCH_CAPACITY: usize = 1;
// The new C++ instance kernel currently reaches its pre-dispatch marker on
// bare metal but not its completion marker. Keep GridPaper on the proven
// identity-mask compositor until that kernel has an isolated completion proof;
// a transform feature must never suppress the document's first visible frame.
const GRIDPAPER_FONT_INSTANCE_TRANSFORMS_ENABLED: bool = false;
const GRID_CURSOR_STROKE_PX: u32 = 3;
const GRID_CURSOR_STATE_CAPACITY: usize = 32;
const PRINT_REQUEST_CAPACITY: usize = 8;
const PRINTER_MENU_CONTEXT_CAPACITY: usize = 8;
const PRINT_CAPTURE_LONG_EDGE: u32 = 1_440;

const ERROR_INVALID_SNAPSHOT: i32 = -1;
const ERROR_INVALID_SCALE: i32 = -2;
const ERROR_NOT_OWNER: i32 = -3;
const ERROR_TRANSPORT: i32 = -4;
const ERROR_INVALID_ANIMATION: i32 = -5;
const ERROR_INVALID_INSTANCE: i32 = -6;
const ERROR_POOL_FULL: i32 = -7;
const ERROR_INVALID_GRID_SIZE: i32 = -8;
pub(crate) const SIZED_SNAPSHOT_VMCALL_MARKER: u64 = 1 << 63;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GridSize {
    columns: u16,
    rows: u16,
}

impl GridSize {
    pub(crate) const FULL: Self = Self {
        columns: COLUMNS as u16,
        rows: ROWS as u16,
    };

    const fn new(columns: u32, rows: u32) -> Option<Self> {
        if columns == 0
            || columns > COLUMN_SOFT_CAP as u32
            || rows == 0
            || rows > ROW_SOFT_CAP as u32
        {
            None
        } else {
            Some(Self {
                columns: columns as u16,
                rows: rows as u16,
            })
        }
    }

    const fn columns(self) -> usize {
        self.columns as usize
    }

    const fn rows(self) -> usize {
        self.rows as usize
    }

    pub(crate) const fn columns_u32(self) -> u32 {
        self.columns as u32
    }

    pub(crate) const fn rows_u32(self) -> u32 {
        self.rows as u32
    }

    pub(crate) const fn grid_width_mm(self) -> u32 {
        self.columns_u32() * CELL_EDGE_MM
    }

    pub(crate) const fn grid_height_mm(self) -> u32 {
        self.rows_u32() * CELL_EDGE_MM
    }

    const fn surface_width_mm(self) -> u32 {
        RULER_GUTTER_MM + self.grid_width_mm()
    }

    const fn surface_height_mm(self) -> u32 {
        RULER_GUTTER_MM + self.grid_height_mm()
    }

    // Retained scenes use millimetres as their coordinate space. This keeps
    // cells, rulers, EDID sizing, and print placement on one physical unit.
    const fn scene_width(self) -> u32 {
        self.surface_width_mm()
    }

    const fn scene_height(self) -> u32 {
        self.surface_height_mm()
    }

    const fn print_capture_extent(self) -> (u32, u32) {
        let width_mm = self.surface_width_mm();
        let height_mm = self.surface_height_mm();
        if width_mm <= height_mm {
            (
                (PRINT_CAPTURE_LONG_EDGE * width_mm + height_mm / 2) / height_mm,
                PRINT_CAPTURE_LONG_EDGE,
            )
        } else {
            (
                PRINT_CAPTURE_LONG_EDGE,
                (PRINT_CAPTURE_LONG_EDGE * height_mm + width_mm / 2) / width_mm,
            )
        }
    }
}

static GPU_COMPUTE_PRESENT_LOGGED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
static GRIDPAPER_COMPUTE_QUARANTINED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
static GRIDPAPER_FONT_INSTANCE_FALLBACK_LOGGED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
static OUTLINELESS_CELL_PATCH_WARNINGS: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);
static NEXT_KERNEL_GRID_TOKEN: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KernelGridClient {
    Shell2,
    SpiritResponse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KernelGridOwner {
    client: KernelGridClient,
    token: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GridPaperProducer {
    Blueprint(u8),
    Kernel(KernelGridOwner),
}

impl GridPaperProducer {
    const fn blueprint_owner(self) -> Option<u8> {
        match self {
            Self::Blueprint(owner) => Some(owner),
            Self::Kernel(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KernelGridLease {
    owner: KernelGridOwner,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KernelGridPresentation {
    pub(crate) window: crate::ui4::WindowId,
    pub(crate) cell_zero_x: i32,
    pub(crate) cell_zero_y: i32,
    pub(crate) published_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KernelGridError {
    InvalidSize,
    InvalidScale,
    PoolFull,
    LeaseLost,
}

#[derive(Clone, Copy)]
struct KernelGridPresentationRecord {
    owner: KernelGridOwner,
    presentation: KernelGridPresentation,
    accepted_text_cells: u64,
    accepted_keyboard_edits: u64,
    published_keyboard_edits: u64,
}

struct SnapshotStore {
    buffers: [[u8; PAGE_BYTES]; 2],
    published: usize,
    owner: Option<u8>,
    kernel_owner: Option<KernelGridOwner>,
    local_instance_id: Option<u32>,
    lease_epoch: u64,
    producer_connected: bool,
    lifecycle_paused: bool,
    generation: u64,
    scale_percent: u16,
    size: GridSize,
    serial: u64,
    text_animations: [Option<GpuFontInstanceProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_serial: u64,
}

impl SnapshotStore {
    const fn new() -> Self {
        Self {
            buffers: [[0; PAGE_BYTES]; 2],
            published: 0,
            owner: None,
            kernel_owner: None,
            local_instance_id: None,
            lease_epoch: 0,
            producer_connected: false,
            lifecycle_paused: false,
            generation: 0,
            scale_percent: 100,
            size: GridSize::FULL,
            serial: 0,
            text_animations: [None; TEXT_ANIMATION_COLOR_SLOTS],
            animation_serial: 0,
        }
    }

    fn claim(&mut self, owner: u8, local_instance_id: u32) {
        let lease_epoch = self.lease_epoch.wrapping_add(1).max(1);
        self.published = 0;
        self.owner = Some(owner);
        self.kernel_owner = None;
        self.local_instance_id = Some(local_instance_id);
        self.lease_epoch = lease_epoch;
        self.producer_connected = true;
        self.lifecycle_paused = false;
        self.generation = 0;
        self.scale_percent = NATIVE_SCALE_PERCENT;
        self.size = GridSize::FULL;
        self.serial = 0;
        self.text_animations = [None; TEXT_ANIMATION_COLOR_SLOTS];
        self.animation_serial = 0;
    }

    fn claim_kernel(
        &mut self,
        owner: KernelGridOwner,
        size: GridSize,
        scale_percent: u16,
        visible: bool,
    ) {
        let lease_epoch = self.lease_epoch.wrapping_add(1).max(1);
        self.buffers = [[0; PAGE_BYTES]; 2];
        self.published = 0;
        self.owner = None;
        self.kernel_owner = Some(owner);
        self.local_instance_id = Some(owner.token);
        self.lease_epoch = lease_epoch;
        self.producer_connected = true;
        self.lifecycle_paused = !visible;
        self.generation = 1;
        self.scale_percent = scale_percent;
        self.size = size;
        self.serial = 1;
        self.text_animations = [None; TEXT_ANIMATION_COLOR_SLOTS];
        self.animation_serial = 0;
    }

    fn release(&mut self) {
        self.owner = None;
        self.kernel_owner = None;
        self.local_instance_id = None;
        self.lease_epoch = self.lease_epoch.wrapping_add(1).max(1);
        self.producer_connected = false;
        self.lifecycle_paused = false;
        self.generation = 0;
        self.size = GridSize::FULL;
        self.serial = 0;
        self.text_animations = [None; TEXT_ANIMATION_COLOR_SLOTS];
        self.animation_serial = 0;
    }
}

static SNAPSHOTS: Mutex<[SnapshotStore; GRIDPAPER_POOL_SOFT_CAP]> =
    Mutex::new([const { SnapshotStore::new() }; GRIDPAPER_POOL_SOFT_CAP]);
static KERNEL_GRID_PRESENTATIONS: Mutex<
    [Option<KernelGridPresentationRecord>; GRIDPAPER_POOL_SOFT_CAP],
> = Mutex::new([None; GRIDPAPER_POOL_SOFT_CAP]);

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
    let Some(slot) = stores
        .iter()
        .position(|store| store.owner.is_none() && store.kernel_owner.is_none())
    else {
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

fn next_kernel_grid_token() -> u32 {
    use core::sync::atomic::Ordering;

    loop {
        let token = NEXT_KERNEL_GRID_TOKEN.fetch_add(1, Ordering::Relaxed);
        if token != 0 {
            return token;
        }
    }
}

fn find_kernel_pool_slot(
    stores: &[SnapshotStore; GRIDPAPER_POOL_SOFT_CAP],
    owner: KernelGridOwner,
) -> Option<usize> {
    stores
        .iter()
        .position(|store| store.kernel_owner == Some(owner))
}

fn request_kernel_grid(
    client: KernelGridClient,
    columns: u32,
    rows: u32,
    scale_percent: u16,
) -> Result<KernelGridLease, KernelGridError> {
    let size = GridSize::new(columns, rows).ok_or(KernelGridError::InvalidSize)?;
    if !(MIN_SCALE_PERCENT..=MAX_SCALE_PERCENT).contains(&u32::from(scale_percent)) {
        return Err(KernelGridError::InvalidScale);
    }
    let mut stores = SNAPSHOTS.lock();
    if client == KernelGridClient::SpiritResponse
        && let Some((slot, store)) = stores.iter_mut().enumerate().find(|(_, store)| {
            store
                .kernel_owner
                .is_some_and(|owner| owner.client == KernelGridClient::SpiritResponse)
        })
    {
        let owner = store
            .kernel_owner
            .expect("matched Spirit Gridpaper kernel owner");
        if store.size != size {
            store.size = size;
            store.lease_epoch = store.lease_epoch.wrapping_add(1).max(1);
        }
        store.scale_percent = scale_percent;
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: kernel lease reused slot={} client={:?} token={} grid={}x{} scale={} residency=retained\n",
            slot,
            owner.client,
            owner.token,
            size.columns(),
            size.rows(),
            scale_percent,
        );
        return Ok(KernelGridLease { owner });
    }

    let Some(slot) = stores
        .iter()
        .position(|store| store.owner.is_none() && store.kernel_owner.is_none())
    else {
        return Err(KernelGridError::PoolFull);
    };
    let owner = KernelGridOwner {
        client,
        token: next_kernel_grid_token(),
    };
    stores[slot].claim_kernel(owner, size, scale_percent, client == KernelGridClient::Shell2);
    crate::log_info!(
        target: "gridpaper";
        "gridpaper: kernel lease claimed slot={} client={:?} token={} grid={}x{} scale={} soft_cap={}\n",
        slot,
        owner.client,
        owner.token,
        size.columns(),
        size.rows(),
        scale_percent,
        GRIDPAPER_POOL_SOFT_CAP,
    );
    Ok(KernelGridLease { owner })
}

/// Open a user-requested Gridpaper document directly in one resident kernel
/// service slot. No Blueprint guest, archive, or virtual container participates.
/// Each request claims a new slot; the shared pool holds ten independent scenes
/// in total across shell, Spirit, and Blueprint producers.
pub(crate) fn request_shell_grid(
    columns: u32,
    rows: u32,
    scale_percent: u16,
) -> Result<KernelGridLease, KernelGridError> {
    request_kernel_grid(KernelGridClient::Shell2, columns, rows, scale_percent)
}

/// Obtain Spirit's one stable Gridpaper document. Repeated calls return the
/// same lease so its GPU scene can stay warm while its UI4 presentation is
/// hidden between replies.
pub(crate) fn request_spirit_response_grid(
    columns: u32,
    rows: u32,
    scale_percent: u16,
) -> Result<KernelGridLease, KernelGridError> {
    request_kernel_grid(KernelGridClient::SpiritResponse, columns, rows, scale_percent)
}

fn with_kernel_grid_store<R>(
    lease: KernelGridLease,
    update: impl FnOnce(usize, &mut SnapshotStore) -> R,
) -> Result<R, KernelGridError> {
    let mut stores = SNAPSHOTS.lock();
    let slot = find_kernel_pool_slot(&stores, lease.owner).ok_or(KernelGridError::LeaseLost)?;
    Ok(update(slot, &mut stores[slot]))
}

/// Clear the retained document and request a fresh UI4 presentation. The
/// worker and GPU allocations remain the same when the logical size is stable.
pub(crate) fn reset_and_show_kernel_grid(lease: KernelGridLease) -> Result<u64, KernelGridError> {
    with_kernel_grid_store(lease, |slot, store| {
        let next = store.published ^ 1;
        store.buffers[next].fill(0);
        store.published = next;
        if lease.owner.client == KernelGridClient::SpiritResponse {
            // Spirit types the next reply through the ordinary identity cell
            // patch path first. Motion is enabled once the complete,
            // word-wrapped response is resident, avoiding a full topology
            // rebuild for every keystroke.
            store.text_animations = [None; TEXT_ANIMATION_COLOR_SLOTS];
            store.animation_serial = store.animation_serial.wrapping_add(1).max(1);
        }
        store.generation = store.generation.wrapping_add(1).max(1);
        store.serial = store.serial.wrapping_add(1).max(1);
        store.lifecycle_paused = false;
        store.producer_connected = true;
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: kernel document reset+show slot={} client={:?} token={} generation={} serial={} retained-runtime=1\n",
            slot,
            lease.owner.client,
            lease.owner.token,
            store.generation,
            store.serial,
        );
        store.generation
    })
}

/// Request Spirit's retained rainbow text presentation after the complete
/// reply has been typed. Palette color stays fixed per cell; motion remains an
/// identity fallback until the C++ font-instance kernel is completion-proven.
pub(crate) fn enable_spirit_response_rainbow_motion(
    lease: KernelGridLease,
) -> Result<u64, KernelGridError> {
    if lease.owner.client != KernelGridClient::SpiritResponse {
        return Err(KernelGridError::LeaseLost);
    }
    with_kernel_grid_store(lease, |slot, store| {
        let mut programs = [None; TEXT_ANIMATION_COLOR_SLOTS];
        for (rainbow_index, selector) in SPIRIT_RAINBOW_SELECTORS.iter().copied().enumerate() {
            programs[usize::from(selector)] = Some(GpuFontInstanceProgram {
                color: None,
                style: GpuFontInstanceStyle::IDENTITY,
                motion: GpuFontInstanceMotion {
                    period_ms: SPIRIT_SCALE_MOTION_PERIOD_MS,
                    phase_permille: (rainbow_index as u16 * 1_000)
                        / SPIRIT_RAINBOW_SELECTORS.len() as u16,
                    rotation_amplitude_centidegrees: 0,
                    scale_amplitude_permille: SPIRIT_SCALE_MOTION_AMPLITUDE_PERMILLE,
                    opacity_amplitude_permille: 0,
                    translation_x_tenths_px: 0,
                    translation_y_tenths_px: 0,
                },
            });
        }
        store.text_animations = programs;
        store.animation_serial = store.animation_serial.wrapping_add(1).max(1);
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: Spirit rainbow motion requested slot={} token={} serial={} selectors={:?} scale=0.85..1.15 period_ms={} phase=palette-staggered evaluator=glyph-mask-compat affine_motion=identity\n",
            slot,
            lease.owner.token,
            store.animation_serial,
            SPIRIT_RAINBOW_SELECTORS,
            SPIRIT_SCALE_MOTION_PERIOD_MS,
        );
        store.animation_serial
    })
}

/// Detach only the UI4 window. The document, compute scene, frame, and pool
/// lease stay resident for a later low-latency show.
pub(crate) fn hide_kernel_grid(lease: KernelGridLease) -> Result<(), KernelGridError> {
    with_kernel_grid_store(lease, |slot, store| {
        store.lifecycle_paused = true;
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: kernel document hide requested slot={} client={:?} token={} action=detach-ui4+retain-gpu-scene\n",
            slot,
            lease.owner.client,
            lease.owner.token,
        );
    })
}

pub(crate) fn kernel_grid_presentation(lease: KernelGridLease) -> Option<KernelGridPresentation> {
    KERNEL_GRID_PRESENTATIONS
        .lock()
        .iter()
        .flatten()
        .find(|record| record.owner == lease.owner)
        .map(|record| record.presentation)
}

pub(crate) fn kernel_grid_accepted_text_cells(lease: KernelGridLease) -> Option<u64> {
    KERNEL_GRID_PRESENTATIONS
        .lock()
        .iter()
        .flatten()
        .find(|record| record.owner == lease.owner)
        .map(|record| record.accepted_text_cells)
}

pub(crate) fn kernel_grid_accepted_keyboard_edits(lease: KernelGridLease) -> Option<u64> {
    KERNEL_GRID_PRESENTATIONS
        .lock()
        .iter()
        .flatten()
        .find(|record| record.owner == lease.owner)
        .map(|record| record.accepted_keyboard_edits)
}

pub(crate) fn kernel_grid_published_keyboard_edits(lease: KernelGridLease) -> Option<u64> {
    KERNEL_GRID_PRESENTATIONS
        .lock()
        .iter()
        .flatten()
        .find(|record| record.owner == lease.owner)
        .map(|record| record.published_keyboard_edits)
}

pub(crate) fn is_spirit_response_grid_window(window: crate::ui4::WindowId) -> bool {
    KERNEL_GRID_PRESENTATIONS
        .lock()
        .iter()
        .flatten()
        .any(|record| {
            record.owner.client == KernelGridClient::SpiritResponse
                && record.presentation.window == window
        })
}

fn mark_kernel_grid_generation_published(pool_slot: usize, generation: u64) {
    let mut presentations = KERNEL_GRID_PRESENTATIONS.lock();
    if let Some(record) = presentations[pool_slot].as_mut() {
        record.presentation.published_generation = generation;
    }
}

fn mark_kernel_grid_keyboard_edits_published(pool_slot: usize, edits: u64) {
    let mut presentations = KERNEL_GRID_PRESENTATIONS.lock();
    if let Some(record) = presentations[pool_slot].as_mut() {
        record.published_keyboard_edits = record.published_keyboard_edits.max(edits);
    }
}

#[derive(Clone)]
struct OwnedSnapshot {
    raw: Vec<u8>,
    producer: GridPaperProducer,
    generation: u64,
    scale_percent: u16,
    size: GridSize,
    serial: u64,
}

struct GridPaperPrintRequest {
    instance_id: u32,
    owner: u8,
    token: u32,
    generation: u64,
    size: GridSize,
    raw: Vec<u8>,
}

struct PrintRenderRequest {
    job_id: u32,
    generation: u64,
    size: GridSize,
    raw: Vec<u8>,
}

struct PrinterMenuContext {
    id: u64,
    snapshot: OwnedSnapshot,
    printers: Vec<crate::r::net::printer::PrinterSnapshot>,
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
static NEXT_PRINTER_MENU_CONTEXT: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(1);
static PRINTER_MENU_CONTEXTS: Mutex<VecDeque<PrinterMenuContext>> = Mutex::new(VecDeque::new());

fn next_print_request_token() -> u32 {
    use core::sync::atomic::Ordering;

    loop {
        let token = NEXT_PRINT_REQUEST_TOKEN.fetch_add(1, Ordering::Relaxed);
        if token != 0 {
            return token;
        }
    }
}

fn next_printer_menu_context() -> u64 {
    use core::sync::atomic::Ordering;

    loop {
        let context = NEXT_PRINTER_MENU_CONTEXT.fetch_add(1, Ordering::Relaxed);
        if context != 0 {
            return context;
        }
    }
}

fn queue_print_request(instance_id: u32, snapshot: &OwnedSnapshot) -> Option<u32> {
    let owner = snapshot.producer.blueprint_owner()?;
    let token = next_print_request_token();
    let mut requests = GRIDPAPER_PRINT_REQUESTS.lock();
    if requests.len() >= PRINT_REQUEST_CAPACITY {
        return None;
    }
    requests.push_back(GridPaperPrintRequest {
        instance_id,
        owner,
        token,
        generation: snapshot.generation,
        size: snapshot.size,
        raw: snapshot.raw.clone(),
    });
    drop(requests);
    crate::log_os::gridpaper_print_requested(owner, token, snapshot.generation);
    Some(token)
}

pub(crate) fn take_print_request_for_owner(owner: u8, instance_id: u32) -> Option<(u32, u64)> {
    let requests = GRIDPAPER_PRINT_REQUESTS.lock();
    let request = requests
        .iter()
        .find(|request| request.owner == owner && request.instance_id == instance_id)?;
    Some((request.token, request.generation))
}

pub(crate) fn consume_print_request(owner: u8, token: u32) -> Option<(u64, GridSize, Vec<u8>)> {
    let mut requests = GRIDPAPER_PRINT_REQUESTS.lock();
    let index = requests
        .iter()
        .position(|request| request.owner == owner && request.token == token)?;
    let request = requests.remove(index)?;
    Some((request.generation, request.size, request.raw))
}

pub(crate) fn valid_print_snapshot(raw: &[u8]) -> bool {
    raw.len() == PAGE_BYTES && validate_page(raw).is_ok()
}

pub(crate) fn request_print_render(
    job_id: u32,
    generation: u64,
    size: GridSize,
    raw: Vec<u8>,
) -> bool {
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
        size,
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn edit_snapshot_from_keyboard(
    snapshot: &mut OwnedSnapshot,
    selection: &mut GridCellSelection,
    input_field: &mut CellInputField,
    event: crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> KeyboardGridOutcome {
    edit_snapshot_from_keyboard_with_foreground(
        snapshot,
        selection,
        input_field,
        event,
        COLOR_DEFAULT,
    )
}

fn edit_snapshot_from_keyboard_with_foreground(
    snapshot: &mut OwnedSnapshot,
    selection: &mut GridCellSelection,
    input_field: &mut CellInputField,
    event: crate::r::keyboard::TrueosKeyboardOutputEvent,
    default_foreground: u8,
) -> KeyboardGridOutcome {
    let mut outcome = KeyboardGridOutcome::default();
    let columns = snapshot.size.columns();
    let rows = snapshot.size.rows();
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
            cell[FOREGROUND_OFFSET] = default_foreground;
        }
        outcome.content_changed = true;
        outcome.edited_cell = Some(edited_cell);
        if *input_field == CellInputField::Primary {
            let linear = selection
                .row
                .saturating_mul(columns)
                .saturating_add(selection.column);
            let next_linear = linear.saturating_add(1).min(columns * rows - 1);
            let next = GridCellSelection {
                column: next_linear % columns,
                row: next_linear / columns,
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
        crate::r::keyboard::KEYBOARD_KEY_BACKSPACE => {
            let next = selection.column.saturating_sub(1);
            outcome.selection_changed = next != selection.column;
            selection.column = next;
            clear_selected_cell(snapshot, *selection, *input_field, &mut outcome);
        }
        crate::r::keyboard::KEYBOARD_KEY_DELETE => {
            clear_selected_cell(snapshot, *selection, *input_field, &mut outcome);
        }
        crate::r::keyboard::KEYBOARD_KEY_ARROW_LEFT => {
            let next = selection.column.saturating_sub(1);
            outcome.selection_changed = next != selection.column;
            selection.column = next;
        }
        crate::r::keyboard::KEYBOARD_KEY_ARROW_RIGHT => {
            let next = selection.column.saturating_add(1).min(columns - 1);
            outcome.selection_changed = next != selection.column;
            selection.column = next;
        }
        crate::r::keyboard::KEYBOARD_KEY_ARROW_UP => {
            let next = selection.row.saturating_sub(1);
            outcome.selection_changed = next != selection.row;
            selection.row = next;
        }
        crate::r::keyboard::KEYBOARD_KEY_ARROW_DOWN => {
            let next = selection.row.saturating_add(1).min(rows - 1);
            outcome.selection_changed = next != selection.row;
            selection.row = next;
        }
        crate::r::keyboard::KEYBOARD_KEY_ENTER => {
            let next_row = selection.row.saturating_add(1).min(rows - 1);
            outcome.selection_changed = selection.column != 0 || next_row != selection.row;
            selection.column = 0;
            selection.row = next_row;
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
            outcome.selection_changed = selection.column != columns - 1;
            selection.column = columns - 1;
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

fn clear_selected_cell(
    snapshot: &mut OwnedSnapshot,
    selection: GridCellSelection,
    input_field: CellInputField,
    outcome: &mut KeyboardGridOutcome,
) {
    let offset = (selection.row * COLUMNS + selection.column) * CELL_BYTES;
    let cell = &mut snapshot.raw[offset..offset + CELL_BYTES];
    let had_content = match input_field {
        CellInputField::Primary => {
            cell[PRIMARY_LENGTH_OFFSET] != 0 || cell[UPPER_LENGTH_OFFSET] != 0
        }
        CellInputField::Upper => cell[UPPER_LENGTH_OFFSET] != 0,
    };
    if !had_content {
        return;
    }
    clear_cell_glyph(cell, input_field);
    if input_field == CellInputField::Primary {
        clear_cell_glyph(cell, CellInputField::Upper);
    }
    if cell[PRIMARY_LENGTH_OFFSET] == 0 && cell[UPPER_LENGTH_OFFSET] == 0 {
        // Color is authorship-like state only while a cell has content. Once
        // both fields are empty, let the next cursor establish its own
        // default foreground instead of inheriting the previous editor.
        cell[FOREGROUND_OFFSET] = COLOR_TRANSPARENT;
    }
    outcome.content_changed = true;
    outcome.edited_cell = Some(selection);
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ScenePan {
    x: f32,
    y: f32,
}

impl ScenePan {
    const ZERO: Self = Self { x: 0.0, y: 0.0 };

    fn clamped(self, scale_percent: u16, size: GridSize) -> Self {
        let scale = f32::from(scale_percent) / 100.0;
        let min_x = (size.scene_width() as f32 * (1.0 - scale)).min(0.0);
        let min_y = (size.scene_height() as f32 * (1.0 - scale)).min(0.0);
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
        size: GridSize,
    ) -> bool {
        let previous = *self;
        self.x += dx as f32 * size.scene_width() as f32 / raster_width.max(1) as f32;
        self.y += dy as f32 * size.scene_height() as f32 / raster_height.max(1) as f32;
        *self = self.clamped(scale_percent, size);
        *self != previous
    }
}

#[derive(Clone, Copy)]
struct OwnedTextAnimations {
    programs: [Option<GpuFontInstanceProgram>; TEXT_ANIMATION_COLOR_SLOTS],
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
    submit_sized_snapshot_for_owner(
        owner,
        instance_id,
        generation,
        scale_percent,
        GridSize::FULL.columns_u32(),
        GridSize::FULL.rows_u32(),
        raw,
    )
}

/// Accept a fixed-capacity snapshot with a smaller logical grid extent. The
/// backing image and row stride stay homogeneous at the soft caps.
pub(crate) fn submit_sized_snapshot_for_owner(
    owner: u8,
    instance_id: u32,
    generation: u64,
    scale_percent: u32,
    columns: u32,
    rows: u32,
    raw: &[u8],
) -> i32 {
    if raw.len() != PAGE_BYTES || validate_page(raw).is_err() {
        return ERROR_INVALID_SNAPSHOT;
    }
    if !(MIN_SCALE_PERCENT..=MAX_SCALE_PERCENT).contains(&scale_percent) {
        return ERROR_INVALID_SCALE;
    }
    let Some(size) = GridSize::new(columns, rows) else {
        return ERROR_INVALID_GRID_SIZE;
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
    let next = snapshots.published ^ 1;
    snapshots.buffers[next].copy_from_slice(raw);
    if snapshots.size != size {
        snapshots.size = size;
        // Scene extent is immutable inside one UI4 frame allocation. Wake the
        // slot worker through the existing lease-epoch teardown/rebuild path.
        snapshots.lease_epoch = snapshots.lease_epoch.wrapping_add(1).max(1);
    }
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
        "gridpaper: font-instance-table accepted pool_slot={} owner={} local_instance={} serial={} programs={} wire_version={} wire_bytes={} ownership=producer-scoped descriptor_residency=gpu-vm geometry_uploads=0\n",
        instance,
        owner,
        instance_id,
        snapshots.animation_serial,
        snapshots.text_animations.iter().flatten().count(),
        raw.first().copied().unwrap_or(0),
        raw.len(),
    );
    0
}

/// Checkpoint the latest app data and atomically release its kernel projection.
///
/// UI4 cell edits and this operation serialize through `SnapshotStore`: an
/// edit is either included in `out`, or observes the released lease and cannot
/// become post-checkpoint app state.
pub(crate) fn checkpoint_snapshot_for_owner(owner: u8, instance_id: u32, out: &mut [u8]) -> i32 {
    if out.len() != PAGE_BYTES {
        return ERROR_INVALID_SNAPSHOT;
    }
    if !valid_local_instance(instance_id) {
        return ERROR_INVALID_INSTANCE;
    }
    let mut stores = SNAPSHOTS.lock();
    let Some(instance) = find_pool_slot(&stores, owner, instance_id) else {
        return ERROR_NOT_OWNER;
    };
    let snapshot = &mut stores[instance];
    if snapshot.owner != Some(owner) {
        return ERROR_NOT_OWNER;
    }
    out.copy_from_slice(&snapshot.buffers[snapshot.published]);
    snapshot.release();
    drop(stores);

    GRIDPAPER_PRINT_REQUESTS
        .lock()
        .retain(|request| request.owner != owner || request.instance_id != instance_id);
    PRINTER_MENU_CONTEXTS
        .lock()
        .retain(|context| context.snapshot.producer.blueprint_owner() != Some(owner));
    crate::log_info!(
        target: "gridpaper";
        "gridpaper: checkpoint captured and pool lease released slot={} owner={} local_instance={} bytes={} action=destroy-ui4+gpu-scene+frame\n",
        instance,
        owner,
        instance_id,
        out.len(),
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
    let result = match snapshots.owner {
        Some(active) if active == owner => {
            snapshots.release();
            0
        }
        Some(_) => ERROR_NOT_OWNER,
        None => 0,
    };
    drop(stores);
    if result == 0 {
        GRIDPAPER_PRINT_REQUESTS
            .lock()
            .retain(|request| request.owner != owner || request.instance_id != instance_id);
        PRINTER_MENU_CONTEXTS
            .lock()
            .retain(|context| context.snapshot.producer.blueprint_owner() != Some(owner));
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: pool lease released slot={} owner={} local_instance={} soft_cap={}\n",
            instance,
            owner,
            instance_id,
            GRIDPAPER_POOL_SOFT_CAP,
        );
    }
    result
}

/// Release every Gridpaper lease owned by a VM.
///
/// This is the host-side fallback for stop/crash paths where Blueprint code
/// cannot cooperatively call `gridpaper::close`.
pub(crate) fn release_owner_lifecycle(owner: u8) -> usize {
    let mut stores = SNAPSHOTS.lock();
    let mut released = 0usize;
    for snapshot in stores.iter_mut() {
        if snapshot.owner == Some(owner) {
            snapshot.release();
            released = released.saturating_add(1);
        }
    }
    drop(stores);

    GRIDPAPER_PRINT_REQUESTS
        .lock()
        .retain(|request| request.owner != owner);
    PRINTER_MENU_CONTEXTS
        .lock()
        .retain(|context| context.snapshot.producer.blueprint_owner() != Some(owner));

    if released != 0 {
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: lifecycle release owner={} released_scenes={} action=destroy-ui4+gpu-scene+frame\n",
            owner,
            released,
        );
    }
    released
}

/// Detach every Gridpaper presentation owned by a VM while keeping its page,
/// retained compute inputs, print-only resident resources, GPU allocations,
/// and last front buffer available for a same-slot resume.
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
            "gridpaper: lifecycle pause owner={} retained_scenes={} action=detach-ui4-preserve-compute-inputs+print-resources\n",
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
        producer: snapshots
            .owner
            .map(GridPaperProducer::Blueprint)
            .or_else(|| snapshots.kernel_owner.map(GridPaperProducer::Kernel))?,
        generation: snapshots.generation,
        scale_percent: snapshots.scale_percent,
        size: snapshots.size,
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
) -> Result<[Option<GpuFontInstanceProgram>; TEXT_ANIMATION_COLOR_SLOTS], ()> {
    if raw.len() < TEXT_ANIMATION_WIRE_HEADER_BYTES || raw[2] != 0 || raw[3] != 0 {
        return Err(());
    }
    let version = raw[0];
    let record_header_bytes = match version {
        TEXT_ANIMATION_WIRE_VERSION_COLOR_ONLY => TEXT_ANIMATION_RECORD_HEADER_BYTES,
        TEXT_ANIMATION_WIRE_VERSION_FONT_INSTANCE => TEXT_INSTANCE_RECORD_HEADER_BYTES,
        _ => return Err(()),
    };
    let count = usize::from(raw[1]);
    if count > TEXT_ANIMATION_COLOR_SLOTS {
        return Err(());
    }
    let mut programs = [None; TEXT_ANIMATION_COLOR_SLOTS];
    let mut cursor = TEXT_ANIMATION_WIRE_HEADER_BYTES;
    for _ in 0..count {
        let header_end = cursor.checked_add(record_header_bytes).ok_or(())?;
        let header = raw.get(cursor..header_end).ok_or(())?;
        let selector = usize::from(header[0]);
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
            || header[9..12] != [0, 0, 0]
        {
            return Err(());
        }
        let style = if version == TEXT_ANIMATION_WIRE_VERSION_COLOR_ONLY {
            GpuFontInstanceStyle::IDENTITY
        } else {
            let rotation_centidegrees = i16::from_le_bytes([header[12], header[13]]);
            let scale_permille = u16::from_le_bytes([header[14], header[15]]);
            let opacity_permille = u16::from_le_bytes([header[16], header[17]]);
            if !(-18_000..=18_000).contains(&rotation_centidegrees)
                || !(125..=8_000).contains(&scale_permille)
                || opacity_permille > 1_000
            {
                return Err(());
            }
            GpuFontInstanceStyle {
                rotation_centidegrees,
                scale_permille,
                opacity_permille,
                background: GpuFontRgba::new(header[18], header[19], header[20], header[21]),
            }
        };
        let motion = if version == TEXT_ANIMATION_WIRE_VERSION_COLOR_ONLY {
            GpuFontInstanceMotion::NONE
        } else {
            let period_ms = u32::from_le_bytes([header[22], header[23], header[24], header[25]]);
            let phase_permille = u16::from_le_bytes([header[26], header[27]]);
            let rotation_amplitude_centidegrees = i16::from_le_bytes([header[28], header[29]]);
            let scale_amplitude_permille = i16::from_le_bytes([header[30], header[31]]);
            let opacity_amplitude_permille = i16::from_le_bytes([header[32], header[33]]);
            let translation_x_tenths_px = i16::from_le_bytes([header[34], header[35]]);
            let translation_y_tenths_px = i16::from_le_bytes([header[36], header[37]]);
            let has_amplitude = rotation_amplitude_centidegrees != 0
                || scale_amplitude_permille != 0
                || opacity_amplitude_permille != 0
                || translation_x_tenths_px != 0
                || translation_y_tenths_px != 0;
            if header[38..40] != [0, 0]
                || phase_permille > 1_000
                || !(-18_000..=18_000).contains(&rotation_amplitude_centidegrees)
                || !(-875..=4_000).contains(&scale_amplitude_permille)
                || !(-1_000..=1_000).contains(&opacity_amplitude_permille)
                || (period_ms == 0 && (has_amplitude || phase_permille != 0))
                || (period_ms != 0
                    && !(MIN_ANIMATION_DURATION_MS..=MAX_ANIMATION_DURATION_MS)
                        .contains(&period_ms))
            {
                return Err(());
            }
            GpuFontInstanceMotion {
                period_ms,
                phase_permille,
                rotation_amplitude_centidegrees,
                scale_amplitude_permille,
                opacity_amplitude_permille,
                translation_x_tenths_px,
                translation_y_tenths_px,
            }
        };
        let color = if frame_count == 0 {
            if version == TEXT_ANIMATION_WIRE_VERSION_COLOR_ONLY
                || header[1] != 0
                || header[2] != 0
                || header[3] != 0
                || duration_ms != 0
            {
                return Err(());
            }
            None
        } else {
            if !(MIN_ANIMATION_DURATION_MS..=MAX_ANIMATION_DURATION_MS).contains(&duration_ms)
                || !(2..=GPU_FONT_COLOR_KEYFRAME_CAPACITY).contains(&frame_count)
            {
                return Err(());
            }
            Some(GpuFontColorChannels::from_bits(header[1]).ok_or(())?)
        };
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
        if frame_count != 0
            && (frames[0].offset_permille != 0 || frames[frame_count - 1].offset_permille != 1_000)
        {
            return Err(());
        }
        programs[selector] = Some(GpuFontInstanceProgram {
            color: color.map(|channels| {
                GpuFontColorProgram::Keyframes(GpuFontColorKeyframes {
                    frames,
                    frame_count: frame_count as u8,
                    channels,
                    duration_ms,
                    timing,
                    iteration,
                })
            }),
            style,
            motion,
        });
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
pub unsafe extern "C" fn trueos_cabi_gridpaper_snapshot_checkpoint(
    out_ptr: *mut u8,
    out_len: usize,
) -> i32 {
    unsafe {
        trueos_cabi_gridpaper_snapshot_checkpoint_instance(PRIMARY_INSTANCE_ID, out_ptr, out_len)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gridpaper_snapshot_checkpoint_instance(
    instance_id: u32,
    out_ptr: *mut u8,
    out_len: usize,
) -> i32 {
    if out_ptr.is_null() || out_len != PAGE_BYTES {
        return ERROR_INVALID_SNAPSHOT;
    }
    // SAFETY: checked non-null above; the ABI caller promises writable bytes.
    let out = unsafe { core::slice::from_raw_parts_mut(out_ptr, out_len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_GRIDPAPER_SNAPSHOT_CHECKPOINT,
            u64::from(instance_id),
            0,
            &[],
            out,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    crate::hv::current_guest_execution_context_vm_id()
        .map(|owner| checkpoint_snapshot_for_owner(owner, instance_id, out))
        .unwrap_or(ERROR_NOT_OWNER)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gridpaper_snapshot_submit_sized(
    generation: u64,
    scale_percent: u32,
    columns: u32,
    rows: u32,
    raw_ptr: *const u8,
    raw_len: usize,
) -> i32 {
    unsafe {
        trueos_cabi_gridpaper_snapshot_submit_instance_sized(
            PRIMARY_INSTANCE_ID,
            generation,
            scale_percent,
            columns,
            rows,
            raw_ptr,
            raw_len,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gridpaper_snapshot_submit_instance_sized(
    instance_id: u32,
    generation: u64,
    scale_percent: u32,
    columns: u32,
    rows: u32,
    raw_ptr: *const u8,
    raw_len: usize,
) -> i32 {
    if GridSize::new(columns, rows).is_none() {
        return ERROR_INVALID_GRID_SIZE;
    }
    if !(MIN_SCALE_PERCENT..=MAX_SCALE_PERCENT).contains(&scale_percent) {
        return ERROR_INVALID_SCALE;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        if raw_ptr.is_null() || raw_len != PAGE_BYTES {
            return ERROR_INVALID_SNAPSHOT;
        }
        // The marker preserves the original B9 vmcall packing for already
        // packaged producers while fitting the bounded logical extent into
        // the otherwise-unused high bits.
        let packed = SIZED_SNAPSHOT_VMCALL_MARKER
            | (u64::from(instance_id & 0x7fff_ffff) << 32)
            | (u64::from(rows & 0xff) << 24)
            | (u64::from(columns & 0xff) << 16)
            | u64::from(scale_percent & 0xffff);
        // SAFETY: the ABI caller promises `raw_len` readable bytes.
        let raw = unsafe { core::slice::from_raw_parts(raw_ptr, raw_len) };
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_GRIDPAPER_SNAPSHOT_SUBMIT,
            generation,
            packed,
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
    submit_sized_snapshot_for_owner(
        owner,
        instance_id,
        generation,
        scale_percent,
        columns,
        rows,
        raw,
    )
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
const _: unsafe extern "C" fn(*mut u8, usize) -> i32 = trueos_cabi_gridpaper_snapshot_checkpoint;
const _: unsafe extern "C" fn(u32, *mut u8, usize) -> i32 =
    trueos_cabi_gridpaper_snapshot_checkpoint_instance;
const _: unsafe extern "C" fn(u32, u64, u32, *const u8, usize) -> i32 =
    trueos_cabi_gridpaper_snapshot_submit_instance;
const _: unsafe extern "C" fn(u64, u32, u32, u32, *const u8, usize) -> i32 =
    trueos_cabi_gridpaper_snapshot_submit_sized;
const _: unsafe extern "C" fn(u32, u64, u32, u32, u32, *const u8, usize) -> i32 =
    trueos_cabi_gridpaper_snapshot_submit_instance_sized;
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
    producer: GridPaperProducer,
    session: crate::ui4::WindowSessionId,
    window: crate::ui4::WindowId,
}

struct GridPaperSurface {
    pool_slot: usize,
    instance_id: u32,
    frame: crate::ui4::FrameHandle,
    presentation: Option<GridPaperPresentation>,
    size: GridSize,
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
    let metrics = GridSceneMetrics::new(surface.size, scale_percent, surface.width, surface.height);
    let scene_width = surface.size.scene_width();
    let scene_height = surface.size.scene_height();
    let scene_x = local_x as f32 * scene_width as f32 / surface.width.max(1) as f32 - pan.x;
    let scene_y = local_y as f32 * scene_height as f32 / surface.height.max(1) as f32 - pan.y;
    let grid_right = metrics.grid_left + surface.size.columns() as f32 * metrics.cell_width;
    let grid_bottom = metrics.grid_top + surface.size.rows() as f32 * metrics.cell_height;
    if scene_x < metrics.grid_left
        || scene_y < metrics.grid_top
        || scene_x >= grid_right
        || scene_y >= grid_bottom
    {
        return None;
    }
    Some(GridCellSelection {
        column: ((scene_x - metrics.grid_left) / metrics.cell_width) as usize,
        row: ((scene_y - metrics.grid_top) / metrics.cell_height) as usize,
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
    size: GridSize,
) -> Result<GridPaperSurface, ServiceError> {
    let (width, height, extent_source) =
        crate::intel::physical_extent_pixels(size.surface_width_mm(), size.surface_height_mm())
            .map(|(width, height)| (width, height, "edid-physical-mm"))
            .unwrap_or((size.scene_width(), size.scene_height(), "logical-fallback"));
    let output = crate::ui4::OutputId::from_slot(0).expect("UI4 D01 must exist");
    let frame = crate::ui4::create_frame(crate::ui4::FrameSpec {
        output,
        content: crate::ui4::FrameContent::BlueprintScene,
        cadence: crate::ui4::FrameCadence::Streaming,
        buffering: crate::ui4::FrameBuffering::Triple,
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
        size,
        width,
        height,
        extent_source,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PoolLeaseState {
    epoch: u64,
    producer: Option<GridPaperProducer>,
    local_instance_id: Option<u32>,
    presentable_producer: Option<GridPaperProducer>,
    size: GridSize,
    scale_percent: u16,
}

fn pool_lease_state(pool_slot: usize) -> PoolLeaseState {
    let mut stores = SNAPSHOTS.lock();
    // GridPaper does not impose a second window policy. Every live lease may
    // request presentation; UI4's broker is the sole authority which isolates
    // non-single-buffer windows across the four ordinary application planes.
    let snapshots = &mut stores[pool_slot];
    let producer = snapshots
        .owner
        .map(GridPaperProducer::Blueprint)
        .or_else(|| snapshots.kernel_owner.map(GridPaperProducer::Kernel));
    let presentable_producer = producer
        .filter(|_| !snapshots.lifecycle_paused && snapshots.producer_connected)
        .and_then(|candidate| match candidate {
            GridPaperProducer::Blueprint(owner) => {
                let state = crate::hv::vm_state(owner);
                if state.running || state.starting {
                    Some(candidate)
                } else {
                    snapshots.producer_connected = false;
                    None
                }
            }
            GridPaperProducer::Kernel(_) => Some(candidate),
        });
    PoolLeaseState {
        epoch: snapshots.lease_epoch,
        producer,
        local_instance_id: snapshots.local_instance_id,
        presentable_producer,
        size: snapshots.size,
        scale_percent: snapshots.scale_percent,
    }
}

fn attach_presentation(
    surface: &mut GridPaperSurface,
    producer: GridPaperProducer,
    session: crate::ui4::WindowSessionId,
    expose_retained_front: bool,
    scale_percent: u16,
) -> Result<GridPaperPresentation, ServiceError> {
    let grid_width_mm = surface.size.grid_width_mm();
    let grid_height_mm = surface.size.grid_height_mm();
    let (grid_width, grid_height) =
        crate::intel::physical_extent_pixels(grid_width_mm, grid_height_mm)
            .unwrap_or((grid_width_mm, grid_height_mm));
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
    // Every producer owns one same-sized scene. Cascade separately leased
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
        interaction: crate::ui4::WindowInteraction::APPLICATION_FIXED_FRAME,
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
    if let GridPaperProducer::Kernel(owner) = producer {
        let metrics =
            GridSceneMetrics::new(surface.size, scale_percent, surface.width, surface.height);
        let cell_zero = metrics.cell_rect(GridCellSelection { column: 0, row: 0 });
        let cell_zero_x = x as i32
            + libm::roundf(
                (cell_zero.left + cell_zero.right) * 0.5 * surface.width as f32
                    / surface.size.scene_width().max(1) as f32,
            ) as i32;
        let cell_zero_y = y as i32
            + libm::roundf(
                (cell_zero.top + cell_zero.bottom) * 0.5 * surface.height as f32
                    / surface.size.scene_height().max(1) as f32,
            ) as i32;
        KERNEL_GRID_PRESENTATIONS.lock()[surface.pool_slot] = Some(KernelGridPresentationRecord {
            owner,
            presentation: KernelGridPresentation {
                window,
                cell_zero_x,
                cell_zero_y,
                published_generation: 0,
            },
            accepted_text_cells: 0,
            accepted_keyboard_edits: 0,
            published_keyboard_edits: 0,
        });
    }
    Ok(presentation)
}

fn release_presentation(surface: &mut GridPaperSurface) -> Option<GridPaperPresentation> {
    let presentation = surface.presentation.take()?;
    Some(presentation)
}

struct ResidentLayer {
    base_color: [u8; 4],
    text_color_selector: Option<u8>,
    logical_rects: Vec<SceneRect>,
    mesh: crate::intel::render::ResidentTriangleMesh,
    coverage: Option<crate::intel::gpu_font::GpuFontCoverageMask>,
    font_instance_descriptor: Option<usize>,
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
    size: GridSize,
    pan: ScenePan,
    layers: Vec<ResidentLayer>,
    font_instance_state: Option<crate::intel::gpgpu::GpgpuOwnedFontInstanceState>,
    font_instance_animation_serial: core::sync::atomic::AtomicU64,
    static_base: Option<crate::intel::gpgpu::GpgpuOwnedRgba8Surface>,
    static_base_animation_serial: core::sync::atomic::AtomicU64,
    cell_patches: Vec<ResidentCellPatch>,
    cell_patch_serial: u64,
    static_base_cell_patch_serial: core::sync::atomic::AtomicU64,
}

impl ResidentPage {
    fn invalidate_static_base(&self) {
        self.static_base_animation_serial
            .store(u64::MAX, core::sync::atomic::Ordering::Release);
    }

    fn install_cell_patch(&mut self, mut patch: ResidentCellPatch) -> Result<(), &'static str> {
        self.cell_patch_serial = self.cell_patch_serial.wrapping_add(1).max(1);
        patch.serial = self.cell_patch_serial;
        if let Some(existing) = self
            .cell_patches
            .iter_mut()
            .find(|existing| existing.selection == patch.selection)
        {
            *existing = patch;
        } else {
            self.cell_patches.push(patch);
        }
        Ok(())
    }
}

struct Geometry {
    size: GridSize,
    vertices: Vec<[f32; 3]>,
    indices: Vec<u32>,
    logical_rects: Vec<SceneRect>,
}

#[derive(Copy, Clone)]
struct SceneRect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

struct ResidentCellCoverage {
    mask: crate::intel::gpu_font::GpuFontCoverageMask,
    color_rgba: u32,
}

struct ResidentCellPatch {
    serial: u64,
    selection: GridCellSelection,
    paper: SceneRect,
    background: Option<(SceneRect, u32)>,
    grid: [SceneRect; 4],
    decorations: Vec<(SceneRect, u32)>,
    coverage: Vec<ResidentCellCoverage>,
}

#[derive(Copy, Clone)]
struct GridSceneMetrics {
    scene_width: u32,
    scene_height: u32,
    scale: f32,
    scene_units_per_mm_x: f32,
    scene_units_per_mm_y: f32,
    cell_width: f32,
    cell_height: f32,
    grid_left: f32,
    grid_top: f32,
    visible_scene_x: f32,
    visible_scene_y: f32,
}

impl GridSceneMetrics {
    fn new(size: GridSize, scale_percent: u16, raster_width: u32, raster_height: u32) -> Self {
        let scene_width = size.scene_width();
        let scene_height = size.scene_height();
        let scale = f32::from(scale_percent) / 100.0;
        let scene_units_per_mm_x = scene_width as f32 / size.surface_width_mm() as f32;
        let scene_units_per_mm_y = scene_height as f32 / size.surface_height_mm() as f32;
        Self {
            scene_width,
            scene_height,
            scale,
            scene_units_per_mm_x,
            scene_units_per_mm_y,
            cell_width: CELL_EDGE_MM as f32 * scene_units_per_mm_x * scale,
            cell_height: CELL_EDGE_MM as f32 * scene_units_per_mm_y * scale,
            grid_left: RULER_GUTTER_MM as f32 * scene_units_per_mm_x * scale,
            grid_top: RULER_GUTTER_MM as f32 * scene_units_per_mm_y * scale,
            visible_scene_x: scene_width as f32 / raster_width as f32,
            visible_scene_y: scene_height as f32 / raster_height as f32,
        }
    }

    fn cell_rect(self, selection: GridCellSelection) -> SceneRect {
        let left = self.grid_left + selection.column as f32 * self.cell_width;
        let top = self.grid_top + selection.row as f32 * self.cell_height;
        SceneRect {
            left,
            top,
            right: left + self.cell_width,
            bottom: top + self.cell_height,
        }
    }

    fn cell_grid(self, cell: SceneRect) -> [SceneRect; 4] {
        let vertical_line = self.visible_scene_x * self.scale;
        let horizontal_line = self.visible_scene_y * self.scale;
        [
            SceneRect {
                left: cell.left - vertical_line * 0.5,
                top: cell.top,
                right: cell.left + vertical_line * 0.5,
                bottom: cell.bottom,
            },
            SceneRect {
                left: cell.right - vertical_line * 0.5,
                top: cell.top,
                right: cell.right + vertical_line * 0.5,
                bottom: cell.bottom,
            },
            SceneRect {
                left: cell.left,
                top: cell.top - horizontal_line * 0.5,
                right: cell.right,
                bottom: cell.top + horizontal_line * 0.5,
            },
            SceneRect {
                left: cell.left,
                top: cell.bottom - horizontal_line * 0.5,
                right: cell.right,
                bottom: cell.bottom + horizontal_line * 0.5,
            },
        ]
    }
}

impl Geometry {
    fn new(size: GridSize) -> Self {
        Self {
            size,
            vertices: Vec::new(),
            indices: Vec::new(),
            logical_rects: Vec::new(),
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
            clip_vertex(left, top, z, self.size),
            clip_vertex(left, bottom, z, self.size),
            clip_vertex(right, bottom, z, self.size),
            clip_vertex(right, top, z, self.size),
        ]);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        self.logical_rects.push(SceneRect {
            left,
            top,
            right,
            bottom,
        });
    }

    fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

fn clip_vertex(x: f32, y: f32, z: f32, size: GridSize) -> [f32; 3] {
    [
        x * 2.0 / size.scene_width() as f32 - 1.0,
        1.0 - y * 2.0 / size.scene_height() as f32,
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

struct CellSceneContent {
    foreground: u8,
    background: Option<(u8, SceneRect)>,
    decorations: Vec<(u8, SceneRect)>,
    texts: Vec<TextCell>,
    outline_less_glyphs: u8,
}

fn build_cell_scene_content(
    instance_id: u32,
    cell: &[u8],
    bounds: SceneRect,
    metrics: GridSceneMetrics,
) -> Result<CellSceneContent, &'static str> {
    let primary_len = usize::from(cell[PRIMARY_LENGTH_OFFSET]);
    let upper_len = usize::from(cell[UPPER_LENGTH_OFFSET]);
    let foreground = cell[FOREGROUND_OFFSET];
    let background_selector = cell[BACKGROUND_OFFSET];
    let style = cell[STYLE_OFFSET];
    let background = (background_selector != COLOR_DEFAULT
        && background_selector != COLOR_TRANSPARENT)
        .then(|| {
            (
                background_selector,
                SceneRect {
                    left: bounds.left + metrics.visible_scene_x * metrics.scale * 0.5,
                    top: bounds.top + metrics.visible_scene_y * metrics.scale * 0.5,
                    right: bounds.right - metrics.visible_scene_x * metrics.scale * 0.5,
                    bottom: bounds.bottom - metrics.visible_scene_y * metrics.scale * 0.5,
                },
            )
        });
    let mut content = CellSceneContent {
        foreground,
        background,
        decorations: Vec::new(),
        texts: Vec::new(),
        outline_less_glyphs: 0,
    };
    if foreground == COLOR_TRANSPARENT || primary_len == 0 {
        return Ok(content);
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
    let font_pixels = (DEFAULT_REGULAR_ROW_FONT_PIXELS * metrics.visible_scene_y * metrics.scale)
        .clamp(metrics.visible_scene_y, 256.0);
    let baseline = bounds.top + metrics.cell_height * 0.72;
    let primary_font = visible_font_for_glyph(instance_id, primary)?;
    let upper_font = upper
        .map(|upper| visible_font_for_glyph(instance_id, upper))
        .transpose()?
        .flatten();
    content.outline_less_glyphs = u8::from(primary_font.is_none())
        .saturating_add(u8::from(upper.is_some() && upper_font.is_none()));
    let has_upper = upper_font.is_some();
    if let Some(font) = primary_font {
        content.texts.push(TextCell {
            text: String::from(primary),
            font,
            color: foreground,
            center_x: (bounds.left + bounds.right) * 0.5
                - if has_upper {
                    metrics.cell_width * 0.10
                } else {
                    0.0
                },
            center_y: (bounds.top + bounds.bottom) * 0.5
                + if has_upper {
                    metrics.cell_height * 0.08
                } else {
                    0.0
                },
            font_pixels: if has_upper {
                font_pixels * 0.82
            } else {
                font_pixels
            },
            bold: style & STYLE_BOLD != 0,
            italic: style & STYLE_ITALIC != 0,
        });
    }
    if let (Some(upper), Some(font)) = (upper, upper_font) {
        content.texts.push(TextCell {
            text: String::from(upper),
            font,
            color: foreground,
            center_x: (bounds.left + bounds.right) * 0.5 + metrics.cell_width * 0.24,
            center_y: (bounds.top + bounds.bottom) * 0.5 - metrics.cell_height * 0.24,
            font_pixels: font_pixels * 0.52,
            bold: style & STYLE_BOLD != 0,
            italic: style & STYLE_ITALIC != 0,
        });
    }
    if style & STYLE_UNDERLINE != 0 {
        let thickness = (font_pixels / 14.0).max(metrics.visible_scene_y);
        let inset = DECORATION_INSET_MM * metrics.scene_units_per_mm_x * metrics.scale;
        content.decorations.push((
            foreground,
            SceneRect {
                left: bounds.left + inset,
                top: baseline + thickness,
                right: bounds.right - inset,
                bottom: baseline + thickness * 2.0,
            },
        ));
    }
    if style & STYLE_STRIKEOUT != 0 {
        let thickness = (font_pixels / 14.0).max(metrics.visible_scene_y);
        let y = baseline - font_pixels * 0.32;
        let inset = DECORATION_INSET_MM * metrics.scene_units_per_mm_x * metrics.scale;
        content.decorations.push((
            foreground,
            SceneRect {
                left: bounds.left + inset,
                top: y,
                right: bounds.right - inset,
                bottom: y + thickness,
            },
        ));
    }
    Ok(content)
}

fn spread_u16_bits(mut value: u32) -> u32 {
    value &= 0x0000_FFFF;
    value = (value | value << 8) & 0x00FF_00FF;
    value = (value | value << 4) & 0x0F0F_0F0F;
    value = (value | value << 2) & 0x3333_3333;
    (value | value << 1) & 0x5555_5555
}

fn font_entry_spatial_key(
    entry: crate::intel::gpu_font::GpuFontJobEntry<'_>,
    scene_width: u32,
    scene_height: u32,
) -> u32 {
    let quantize = |value: f32, extent: u32| {
        let normalized = (value / extent.max(1) as f32).clamp(0.0, 1.0);
        libm::roundf(normalized * u16::MAX as f32) as u32
    };
    let x = quantize(entry.position[0], scene_width);
    let y = quantize(entry.position[1], scene_height);
    spread_u16_bits(x) | (spread_u16_bits(y) << 1)
}

fn partition_font_entries<'a>(
    entries: &[crate::intel::gpu_font::GpuFontJobEntry<'a>],
    partition_count: usize,
    scene_width: u32,
    scene_height: u32,
) -> Vec<Vec<crate::intel::gpu_font::GpuFontJobEntry<'a>>> {
    let mut ordered = entries.to_vec();
    ordered.sort_unstable_by_key(|entry| font_entry_spatial_key(*entry, scene_width, scene_height));
    let partition_count = partition_count.clamp(1, ordered.len());
    let mut partitions = Vec::with_capacity(partition_count);
    let mut cursor = 0usize;
    for partition in 0..partition_count {
        let remaining_entries = ordered.len() - cursor;
        let remaining_partitions = partition_count - partition;
        let take = remaining_entries.div_ceil(remaining_partitions);
        partitions.push(ordered[cursor..cursor + take].to_vec());
        cursor += take;
    }
    partitions
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

fn visible_font_for_glyph(
    instance_id: u32,
    glyph: &str,
) -> Result<Option<GpuFontFace>, &'static str> {
    // A space is real Gridpaper content because it advances the selected
    // cell, but it intentionally has no outline to submit. Keep it out of the
    // font batches: both the resident mesh and analytical coverage builders
    // reject outline-less jobs as malformed rendering work.
    if glyph.chars().all(char::is_whitespace) {
        return Ok(None);
    }
    let font = font_for_glyph(instance_id, glyph);
    match crate::graphics::font::gpu_outline_for_text(font.registry_name(), glyph) {
        Ok(_) => Ok(Some(font)),
        // Default-ignorable or unsupported scalars can also legitimately
        // resolve to no ink. They must behave like a blank cell rather than
        // poisoning every later snapshot rebuild.
        Err("outline-empty") => Ok(None),
        Err(error) => Err(error),
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
    instance_id: u32,
    snapshot: &OwnedSnapshot,
    text_animations: &[Option<GpuFontInstanceProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    raster_width: u32,
    raster_height: u32,
    pan: ScenePan,
) -> Result<ResidentPage, &'static str> {
    use crate::intel::gpu_font::{
        GpuFontJobEntry, GpuFontTextRequest, create_gpu_font_centered_coverage_mask_at_raster,
        create_resident_font_centered_scene_mesh_at_raster,
        gpu_font_entries_use_analytical_coverage,
    };

    // Only glyphs present in this snapshot consult the already-registered
    // fallback faces below. A geometry-only page must not preflight a future
    // filesystem face merely because that face is in Gridpaper's fallback
    // order.
    let mut layers = Vec::new();

    let mut backgrounds: Vec<(u8, Geometry)> = Vec::new();
    let mut decorations: Vec<(u8, Geometry)> = Vec::new();
    let mut texts = Vec::new();
    let size = snapshot.size;
    let metrics = GridSceneMetrics::new(size, snapshot.scale_percent, raster_width, raster_height);
    let scene_width = metrics.scene_width;
    let scene_height = metrics.scene_height;
    let scale = metrics.scale;
    let scene_units_per_mm_x = metrics.scene_units_per_mm_x;
    let scene_units_per_mm_y = metrics.scene_units_per_mm_y;
    let cell_width = metrics.cell_width;
    let cell_height = metrics.cell_height;
    let grid_width = size.columns() as f32 * cell_width;
    let grid_height = size.rows() as f32 * cell_height;
    let pan = pan.clamped(snapshot.scale_percent, size);
    let grid_left = metrics.grid_left;
    let grid_top = metrics.grid_top;
    let grid_right = grid_left + grid_width;
    let grid_bottom = grid_top + grid_height;
    let visible_scene_x = metrics.visible_scene_x;
    let visible_scene_y = metrics.visible_scene_y;

    // Only the grid owns paper. The ruler gutters remain transparent, and
    // there is no unused A4 margin on the right or bottom of the frame.
    let mut paper = Geometry::new(size);
    paper.quad(grid_left, grid_top, grid_right, grid_bottom, 0.9);
    push_geometry_layer(&mut layers, paper, palette(COLOR_DEFAULT, true))?;

    for row in 0..size.rows() {
        for column in 0..size.columns() {
            let offset = (row * COLUMNS + column) * CELL_BYTES;
            let cell = &snapshot.raw[offset..offset + CELL_BYTES];
            let bounds = metrics.cell_rect(GridCellSelection { column, row });
            let content = build_cell_scene_content(instance_id, cell, bounds, metrics)?;
            if let Some((color, rect)) = content.background {
                geometry_for_color(&mut backgrounds, color, size).quad(
                    rect.left,
                    rect.top,
                    rect.right,
                    rect.bottom,
                    0.8,
                );
            }
            for (color, rect) in content.decorations {
                geometry_for_color(&mut decorations, color, size).quad(
                    rect.left,
                    rect.top,
                    rect.right,
                    rect.bottom,
                    0.4,
                );
            }
            texts.extend(content.texts);
        }
    }

    for (color, geometry) in backgrounds {
        push_geometry_layer(&mut layers, geometry, palette(color, true))?;
    }

    let mut grid = Geometry::new(size);
    let vertical_line = visible_scene_x * scale;
    let horizontal_line = visible_scene_y * scale;
    for column in 0..=size.columns() {
        let x = grid_left + column as f32 * cell_width;
        grid.quad(x - vertical_line * 0.5, grid_top, x + vertical_line * 0.5, grid_bottom, 0.6);
    }
    for row in 0..=size.rows() {
        let y = grid_top + row as f32 * cell_height;
        grid.quad(grid_left, y - horizontal_line * 0.5, grid_right, y + horizontal_line * 0.5, 0.6);
    }
    push_geometry_layer(&mut layers, grid, [188, 205, 224, 255])?;

    let mut rulers = Geometry::new(size);
    for column in 0..=size.columns() {
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
    for row in 0..=size.rows() {
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

    let mut font_groups = Vec::new();
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
            font_groups.push((font, color, entries));
        }
    }

    if font_groups.len() > crate::intel::gpgpu::GPGPU_FONT_INSTANCE_MAX_LAYERS {
        return Err("gridpaper-font-instance-group-capacity");
    }
    let font_group_count = font_groups.len();
    let mut partition_counts = vec![1usize; font_groups.len()];
    let mut partition_total = font_groups.len();
    while partition_total < crate::intel::gpgpu::GPGPU_FONT_INSTANCE_MAX_LAYERS {
        let mut best_index = None;
        let mut best_score = 1usize;
        for (index, (_, color, entries)) in font_groups.iter().enumerate() {
            if text_animations
                .get(usize::from(*color))
                .copied()
                .flatten()
                .is_none()
            {
                continue;
            }
            if partition_counts[index] >= entries.len() {
                continue;
            }
            let score = entries.len().div_ceil(partition_counts[index]);
            if score > best_score {
                best_score = score;
                best_index = Some(index);
            }
        }
        let Some(index) = best_index else {
            break;
        };
        partition_counts[index] += 1;
        partition_total += 1;
    }

    let font_entry_count = font_groups
        .iter()
        .map(|(_, _, entries)| entries.len())
        .sum::<usize>();
    let dynamic_group_count = font_groups
        .iter()
        .filter(|(_, color, _)| {
            text_animations
                .get(usize::from(*color))
                .copied()
                .flatten()
                .is_some()
        })
        .count();
    let dynamic_entry_count = font_groups
        .iter()
        .filter(|(_, color, _)| {
            text_animations
                .get(usize::from(*color))
                .copied()
                .flatten()
                .is_some()
        })
        .map(|(_, _, entries)| entries.len())
        .sum::<usize>();
    for ((font, color, entries), partition_count) in
        font_groups.into_iter().zip(partition_counts.into_iter())
    {
        for entries in partition_font_entries(&entries, partition_count, scene_width, scene_height)
        {
            let coverage = if gpu_font_entries_use_analytical_coverage(
                &entries,
                scene_width,
                scene_height,
                raster_width,
                raster_height,
            ) {
                match create_gpu_font_centered_coverage_mask_at_raster(
                    &entries,
                    font,
                    scene_width,
                    scene_height,
                    raster_width,
                    raster_height,
                ) {
                    Ok(coverage) => coverage,
                    Err(reason) => {
                        crate::log_warn!(
                            target: "gridpaper";
                            "gridpaper: analytical font coverage unavailable instance={} scale={} font={} color={} entries={} reason={} action=retry-complete-page\n",
                            instance_id,
                            snapshot.scale_percent,
                            font.registry_name(),
                            color,
                            entries.len(),
                            reason,
                        );
                        return Err(reason);
                    }
                }
            } else {
                return Err("font-coverage-ineligible");
            };
            let mesh = create_resident_font_centered_scene_mesh_at_raster(
                &entries,
                font,
                scene_width,
                scene_height,
                raster_width,
                raster_height,
            )?;
            layers.push(ResidentLayer {
                base_color: palette(color, false),
                text_color_selector: Some(color),
                logical_rects: Vec::new(),
                mesh,
                coverage: Some(coverage),
                font_instance_descriptor: None,
            });
        }
    }

    let coverage_layer_count = layers
        .iter()
        .filter(|layer| layer.coverage.is_some())
        .count();
    crate::log_info!(
        target: "gridpaper";
        "gridpaper: font coverage partitioned instance={} groups={} entries={} dynamic_groups={} dynamic_entries={} masks={} assigned_partitions={} descriptor_capacity={} order=morton-2d policy=animated-priority-static-aggregate\n",
        instance_id,
        font_group_count,
        font_entry_count,
        dynamic_group_count,
        dynamic_entry_count,
        coverage_layer_count,
        partition_total,
        crate::intel::gpgpu::GPGPU_FONT_INSTANCE_MAX_LAYERS,
    );
    let font_instance_state = if coverage_layer_count == 0 {
        None
    } else {
        let state = crate::intel::gpgpu::allocate_font_instance_state(coverage_layer_count)
            .ok_or("gridpaper-font-instance-state-allocation")?;
        let mut descriptor_index = 0usize;
        for layer in &mut layers {
            if layer.coverage.is_some() {
                layer.font_instance_descriptor = Some(descriptor_index);
                descriptor_index += 1;
            }
        }
        Some(state)
    };
    let static_base =
        crate::intel::gpgpu::allocate_font_instance_rgba8_surface(raster_width, raster_height);

    Ok(ResidentPage {
        serial: snapshot.serial,
        generation: snapshot.generation,
        scale_percent: snapshot.scale_percent,
        size,
        pan,
        layers,
        font_instance_state,
        font_instance_animation_serial: core::sync::atomic::AtomicU64::new(u64::MAX),
        static_base,
        static_base_animation_serial: core::sync::atomic::AtomicU64::new(u64::MAX),
        cell_patches: Vec::new(),
        cell_patch_serial: 0,
        static_base_cell_patch_serial: core::sync::atomic::AtomicU64::new(0),
    })
}

fn build_resident_cell_patch(
    instance_id: u32,
    snapshot: &OwnedSnapshot,
    selection: GridCellSelection,
    raster_width: u32,
    raster_height: u32,
) -> Result<ResidentCellPatch, &'static str> {
    use crate::intel::gpu_font::{
        GpuFontJobEntry, GpuFontTextRequest, create_gpu_font_centered_coverage_mask_at_raster,
    };

    if selection.column >= snapshot.size.columns() || selection.row >= snapshot.size.rows() {
        return Err("gridpaper-cell-patch-selection");
    }
    let size = snapshot.size;
    let metrics = GridSceneMetrics::new(size, snapshot.scale_percent, raster_width, raster_height);
    let scene_width = metrics.scene_width;
    let scene_height = metrics.scene_height;
    let scale = metrics.scale;
    let visible_scene_x = metrics.visible_scene_x;
    let paper = metrics.cell_rect(selection);
    let grid = metrics.cell_grid(paper);
    let offset = (selection.row * COLUMNS + selection.column) * CELL_BYTES;
    let cell = &snapshot.raw[offset..offset + CELL_BYTES];
    let content = build_cell_scene_content(instance_id, cell, paper, metrics)?;
    let foreground = content.foreground;
    let outline_less_glyphs = content.outline_less_glyphs;
    let background = content
        .background
        .map(|(color, rect)| (rect, u32::from_le_bytes(palette(color, true))));
    let decorations = content
        .decorations
        .into_iter()
        .map(|(color, rect)| (rect, u32::from_le_bytes(palette(color, false))))
        .collect::<Vec<_>>();
    let texts = content.texts;

    if outline_less_glyphs != 0 {
        let warning = OUTLINELESS_CELL_PATCH_WARNINGS
            .fetch_add(1, core::sync::atomic::Ordering::AcqRel)
            .saturating_add(1);
        if warning <= 8 || warning.is_multiple_of(120) {
            crate::log_warn!(
                target: "gridpaper";
                "gridpaper: outline-less cell content retained instance={} column={} row={} warning_seq={} primary_utf8_bytes={} upper_utf8_bytes={} outline_less_glyphs={} reason=whitespace-or-no-font-outline action=early-return-before-font-coverage+retain-intentional-blank-cell future=advance-text-layout-contract log_policy=first-8+each-120\n",
                instance_id,
                selection.column,
                selection.row,
                warning,
                cell[PRIMARY_LENGTH_OFFSET],
                cell[UPPER_LENGTH_OFFSET],
                outline_less_glyphs,
            );
        }
        if texts.is_empty() {
            return Ok(ResidentCellPatch {
                serial: 0,
                selection,
                paper,
                background,
                grid,
                decorations,
                coverage: Vec::new(),
            });
        }
    }

    let mut coverage = Vec::new();
    for font in [
        GpuFontFace::Inconsolata,
        GpuFontFace::NotoSansSc,
        GpuFontFace::Default,
    ] {
        let mut entries = Vec::new();
        for text in texts.iter().filter(|text| text.font == font) {
            let bold_center_offset = if text.bold {
                visible_scene_x * 0.5 * scale
            } else {
                0.0
            };
            entries.push(GpuFontJobEntry {
                text: GpuFontTextRequest::SingleLine(text.text.as_str()),
                position: [text.center_x - bold_center_offset, text.center_y],
                font_pixels: text.font_pixels,
                slant: if text.italic { 0.22 } else { 0.0 },
            });
            if text.bold {
                entries.push(GpuFontJobEntry {
                    text: GpuFontTextRequest::SingleLine(text.text.as_str()),
                    position: [text.center_x + bold_center_offset, text.center_y],
                    font_pixels: text.font_pixels,
                    slant: if text.italic { 0.22 } else { 0.0 },
                });
            }
        }
        if entries.is_empty() {
            continue;
        }
        coverage.push(ResidentCellCoverage {
            mask: create_gpu_font_centered_coverage_mask_at_raster(
                entries.as_slice(),
                font,
                scene_width,
                scene_height,
                raster_width,
                raster_height,
            )?,
            color_rgba: u32::from_le_bytes(palette(foreground, false)),
        });
    }

    Ok(ResidentCellPatch {
        serial: 0,
        selection,
        paper,
        background,
        grid,
        decorations,
        coverage,
    })
}

fn geometry_for_color(
    layers: &mut Vec<(u8, Geometry)>,
    color: u8,
    size: GridSize,
) -> &mut Geometry {
    if let Some(index) = layers.iter().position(|(candidate, _)| *candidate == color) {
        return &mut layers[index].1;
    }
    layers.push((color, Geometry::new(size)));
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
        logical_rects: geometry.logical_rects,
        mesh,
        coverage: None,
        font_instance_descriptor: None,
    });
    Ok(())
}

fn palette(color: u8, background: bool) -> [u8; 4] {
    match color {
        // Default paper is true opaque white on screen and in the shared
        // print scene. Explicit per-cell backgrounds still layer above it.
        0 if background => [255, 255, 255, 255],
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

fn ui4_cursor_rgba(source: crate::ui4::Ui4CursorSource) -> [u8; 4] {
    let color = crate::ui4::cursor_color(source);
    [color.r, color.g, color.b, color.a]
}

fn grid_foreground_for_cursor(source: crate::ui4::Ui4CursorSource) -> u8 {
    let cursor = ui4_cursor_rgba(source);
    (1..COLOR_TRANSPARENT)
        .min_by_key(|selector| {
            let candidate = palette(*selector, false);
            let channel_delta = |index: usize| {
                let delta = i32::from(cursor[index]) - i32::from(candidate[index]);
                delta * delta
            };
            channel_delta(0) + channel_delta(1) + channel_delta(2)
        })
        .unwrap_or(COLOR_DEFAULT)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum GridPaperComputeFailure {
    Unavailable(&'static str),
    SubmittedIncomplete(&'static str),
}

impl GridPaperComputeFailure {
    const fn reason(self) -> &'static str {
        match self {
            Self::Unavailable(reason) | Self::SubmittedIncomplete(reason) => reason,
        }
    }

    const fn submitted_incomplete(self) -> bool {
        matches!(self, Self::SubmittedIncomplete(_))
    }
}

struct GridPaperFrameResult {
    changed_pixels: usize,
    static_base_rebuilt: bool,
    frame_us: u64,
    geometry_us: u64,
    geometry_rects: usize,
    geometry_submits: usize,
    coverage_us: u64,
    coverage_submits: usize,
    coverage_walkers: usize,
    present_copy_us: u64,
    release: crate::intel::gpgpu::GpgpuRgba8ReleaseFence,
}

fn publish_page(
    surface: &GridPaperSurface,
    page: &ResidentPage,
    cursor_inputs: &[GridCursorInputState],
    damage: crate::ui4::DamageRect,
    text_animations: &[Option<GpuFontInstanceProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_serial: u64,
    animation_elapsed_ms: u64,
) -> Result<GridPaperFrameResult, ServiceError> {
    use core::sync::atomic::Ordering;

    if GRIDPAPER_COMPUTE_QUARANTINED.load(Ordering::Acquire) {
        return Err(ServiceError::Render("gridpaper-compute-producer-quarantined"));
    }
    if page
        .layers
        .iter()
        .any(|layer| layer.text_color_selector.is_some() && layer.coverage.is_none())
    {
        return Err(ServiceError::Render("gridpaper-compute-text-coverage-required"));
    }
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
    let mut final_rects = Vec::with_capacity(cursor_inputs.len().saturating_mul(4));
    for (cursor_index, cursor) in cursor_inputs.iter().enumerate() {
        let Some(selection) = cursor.selection else {
            continue;
        };
        let same_outline = |candidate: &&GridCursorInputState| {
            candidate.selection == Some(selection) && candidate.input_field == cursor.input_field
        };
        let ring_index = cursor_inputs[..cursor_index]
            .iter()
            .filter(same_outline)
            .count();
        let ring_count = cursor_inputs.iter().filter(same_outline).count();
        let color_rgba = u32::from_le_bytes(ui4_cursor_rgba(cursor.source));
        if let Some(rects) = grid_cursor_rects(
            surface,
            page,
            selection,
            cursor.input_field,
            color_rgba,
            ring_index,
            ring_count,
        ) {
            final_rects.extend(rects);
        }
    }
    let result = match render_compute_page_frame(
        page,
        text_animations,
        animation_serial,
        animation_elapsed_ms,
        final_rects.as_slice(),
        destination,
    ) {
        Ok(result) => result,
        Err(failure) => {
            if failure.submitted_incomplete() {
                GRIDPAPER_COMPUTE_QUARANTINED.store(true, Ordering::Release);
                crate::log_error!(
                    target: "gridpaper";
                    "gridpaper: compute producer quarantined instance={} frame={} buffer={} reason={} action=retain-write-lease-no-reuse+disable-gridpaper-compute-until-reboot\n",
                    surface.instance_id,
                    lease.frame.raw(),
                    lease.buffer_index,
                    failure.reason(),
                );
            } else {
                let _ = crate::ui4::cancel_frame_buffer(lease);
            }
            return Err(ServiceError::Render(failure.reason()));
        }
    };
    if !GPU_COMPUTE_PRESENT_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: live frame path=gpgpu-compute-blueprint-scene-to-ui4-triple size={}x{} pitch={} target_gpu=0x{:X} buffers=3 plane_slot={} cpu_readback=0 cpu_frame_copy=0 gpu_frame_copy=1 rectangles={} rectangle_submits={} coverage_submits={} coverage_walkers={} static_base_rebuilt={} static_base_cache=pat0-wb present_cache=pat3-uc cursor_overlay=gpgpu-worklist final_release=compute-pat3-uc+pipe-control-post-sync publish=exact-surface surflive=display-ownership resident3d_ui4=disabled\n",
            destination.width,
            destination.height,
            destination.pitch_bytes,
            destination.gpu,
            UI4_PLANE_SLOT,
            result.geometry_rects,
            result.geometry_submits,
            result.coverage_submits,
            result.coverage_walkers,
            result.static_base_rebuilt,
        );
    }
    if let Err(error) = crate::ui4::publish_gpgpu_scene_frame_buffer(lease, result.release) {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(error.into());
    }
    crate::ui4::publish_window_frame(UI4_OWNER, presentation.window, damage)?;
    Ok(result)
}

fn rgba_u32(rgba: GpuFontRgba) -> u32 {
    u32::from_le_bytes([rgba.r, rgba.g, rgba.b, rgba.a])
}

fn instance_program_for_layer(
    layer: &ResidentLayer,
    programs: &[Option<GpuFontInstanceProgram>; TEXT_ANIMATION_COLOR_SLOTS],
) -> Option<GpuFontInstanceProgram> {
    layer
        .text_color_selector
        .and_then(|selector| programs.get(usize::from(selector)).copied().flatten())
}

fn update_font_instance_descriptors(
    page: &ResidentPage,
    programs: &[Option<GpuFontInstanceProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_serial: u64,
) -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;

    let Some(state) = page.font_instance_state.as_ref() else {
        return if page.layers.iter().any(|layer| layer.coverage.is_some()) {
            Err("gridpaper-font-instance-state-missing")
        } else {
            Ok(())
        };
    };
    if page.font_instance_animation_serial.load(Ordering::Acquire) == animation_serial {
        return Ok(());
    }

    for layer in &page.layers {
        let Some(coverage) = layer.coverage.as_ref() else {
            continue;
        };
        let descriptor_index = layer
            .font_instance_descriptor
            .ok_or("gridpaper-font-instance-descriptor-missing")?;
        let base = GpuFontRgba::new(
            layer.base_color[0],
            layer.base_color[1],
            layer.base_color[2],
            layer.base_color[3],
        );
        let program = instance_program_for_layer(layer, programs);
        let foreground = match program.and_then(|program| program.color) {
            Some(GpuFontColorProgram::Static(rgba)) => rgba,
            Some(GpuFontColorProgram::Transition(transition)) => transition.from,
            Some(GpuFontColorProgram::Keyframes(keyframes)) => keyframes.frames[0].rgba,
            None => base,
        };
        let mut descriptor = crate::intel::gpgpu::GpgpuFontInstanceDescriptor::new(
            coverage.surface(),
            coverage.full_rect(),
            rgba_u32(foreground),
        )
        .ok_or("gridpaper-font-instance-descriptor-invalid")?;
        let style = program.map_or(GpuFontInstanceStyle::IDENTITY, |program| program.style);
        descriptor.set_transform(
            f32::from(style.scale_permille) / 1_000.0,
            f32::from(style.rotation_centidegrees) * core::f32::consts::PI / 18_000.0,
            f32::from(style.opacity_permille) / 1_000.0,
        );
        descriptor.set_background(rgba_u32(style.background));

        let motion = program.map_or(GpuFontInstanceMotion::NONE, |program| program.motion);
        if motion.period_ms != 0 {
            descriptor.set_motion(
                motion.period_ms as f32 / 1_000.0,
                f32::from(motion.phase_permille) / 1_000.0,
                f32::from(motion.rotation_amplitude_centidegrees) * core::f32::consts::PI
                    / 18_000.0,
                f32::from(motion.scale_amplitude_permille) / 1_000.0,
                f32::from(motion.opacity_amplitude_permille) / 1_000.0,
                [
                    f32::from(motion.translation_x_tenths_px) / 10.0,
                    f32::from(motion.translation_y_tenths_px) / 10.0,
                ],
            );
        }

        let color_ok = match program.and_then(|program| program.color) {
            Some(GpuFontColorProgram::Transition(transition)) => {
                let frames = [
                    (0, rgba_u32(transition.from)),
                    (1_000, rgba_u32(transition.to)),
                ];
                descriptor.set_color_animation(
                    transition.channels.bits(),
                    transition.timing == GpuFontColorTiming::EaseInOutSine,
                    match transition.iteration {
                        GpuFontColorIteration::Once => 0,
                        GpuFontColorIteration::Loop => 1,
                        GpuFontColorIteration::Alternate => 2,
                    },
                    transition.duration_ms as f32 / 1_000.0,
                    &frames,
                )
            }
            Some(GpuFontColorProgram::Keyframes(keyframes)) => {
                let count = usize::from(keyframes.frame_count);
                let mut frames = [(0u16, 0u32); GPU_FONT_COLOR_KEYFRAME_CAPACITY];
                for (output, input) in frames.iter_mut().zip(keyframes.frames.iter()).take(count) {
                    *output = (input.offset_permille, rgba_u32(input.rgba));
                }
                descriptor.set_color_animation(
                    keyframes.channels.bits(),
                    keyframes.timing == GpuFontColorTiming::EaseInOutSine,
                    match keyframes.iteration {
                        GpuFontColorIteration::Once => 0,
                        GpuFontColorIteration::Loop => 1,
                        GpuFontColorIteration::Alternate => 2,
                    },
                    keyframes.duration_ms as f32 / 1_000.0,
                    &frames[..count],
                )
            }
            _ => true,
        };
        if !color_ok || !state.write(descriptor_index, &descriptor) {
            return Err("gridpaper-font-instance-descriptor-write");
        }
    }
    page.font_instance_animation_serial
        .store(animation_serial, Ordering::Release);
    Ok(())
}

#[derive(Default)]
struct StaticBaseBuildStats {
    geometry_rects: usize,
    geometry_submits: usize,
    coverage_submits: usize,
    coverage_walkers: usize,
}

fn fill_static_patch_rects(
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    rects: &[crate::intel::gpgpu::GpgpuSolidRect],
    incomplete_reason: &'static str,
    unavailable_reason: &'static str,
) -> Result<(usize, usize), GridPaperComputeFailure> {
    use crate::intel::gpgpu::GpgpuSubmissionOutcome;

    if rects.is_empty() {
        return Ok((0, 0));
    }
    let rendered = crate::intel::gpgpu::fill_solid_rects_rgba8_result(destination, rects);
    match rendered.outcome {
        GpgpuSubmissionOutcome::Complete => Ok((rendered.stats.descs, rendered.stats.submits)),
        GpgpuSubmissionOutcome::SubmittedIncomplete => {
            Err(GridPaperComputeFailure::SubmittedIncomplete(incomplete_reason))
        }
        GpgpuSubmissionOutcome::Unavailable => {
            Err(GridPaperComputeFailure::Unavailable(unavailable_reason))
        }
    }
}

fn apply_resident_cell_patches(
    page: &ResidentPage,
    after_serial: u64,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    viewport_translation_px: [f32; 2],
) -> Result<StaticBaseBuildStats, GridPaperComputeFailure> {
    let patches = page
        .cell_patches
        .iter()
        .filter(|patch| patch.serial > after_serial)
        .collect::<Vec<_>>();
    if patches.is_empty() {
        return Ok(StaticBaseBuildStats::default());
    }
    let lower = |rect: SceneRect, color_rgba: u32| {
        scene_rect_to_surface(rect, page.size, destination, viewport_translation_px, color_rgba)
    };
    let paper = patches
        .iter()
        .filter_map(|patch| lower(patch.paper, u32::from_le_bytes(palette(COLOR_DEFAULT, true))))
        .collect::<Vec<_>>();
    let backgrounds = patches
        .iter()
        .filter_map(|patch| {
            patch
                .background
                .and_then(|(rect, color)| lower(rect, color))
        })
        .collect::<Vec<_>>();
    let grid_color = u32::from_le_bytes([188, 205, 224, 255]);
    let grid = patches
        .iter()
        .flat_map(|patch| patch.grid)
        .filter_map(|rect| lower(rect, grid_color))
        .collect::<Vec<_>>();
    let decorations = patches
        .iter()
        .flat_map(|patch| patch.decorations.iter().copied())
        .filter_map(|(rect, color)| lower(rect, color))
        .collect::<Vec<_>>();
    let mut stats = StaticBaseBuildStats::default();
    for (rects, incomplete, unavailable) in [
        (
            paper.as_slice(),
            "gridpaper-cell-patch-paper-incomplete",
            "gridpaper-cell-patch-paper-unavailable",
        ),
        (
            backgrounds.as_slice(),
            "gridpaper-cell-patch-background-incomplete",
            "gridpaper-cell-patch-background-unavailable",
        ),
        (
            grid.as_slice(),
            "gridpaper-cell-patch-grid-incomplete",
            "gridpaper-cell-patch-grid-unavailable",
        ),
        (
            decorations.as_slice(),
            "gridpaper-cell-patch-decoration-incomplete",
            "gridpaper-cell-patch-decoration-unavailable",
        ),
    ] {
        let (descs, submits) =
            fill_static_patch_rects(destination, rects, incomplete, unavailable)?;
        stats.geometry_rects = stats.geometry_rects.saturating_add(descs);
        stats.geometry_submits = stats.geometry_submits.saturating_add(submits);
    }

    let translation = [
        libm::roundf(viewport_translation_px[0]) as i32,
        libm::roundf(viewport_translation_px[1]) as i32,
    ];
    let coverage = patches
        .iter()
        .flat_map(|patch| patch.coverage.iter())
        .collect::<Vec<_>>();
    // A cell reaches this incremental path only when it has no transform or
    // animation. Composite that identity coverage directly: the C++ instance
    // engine remains authoritative for CSS-like dynamic layers, while plain
    // typing avoids mutable descriptor indirection entirely.
    //
    // Multi-walker indirect payload starts use 256-byte slots, satisfying the
    // Gen12 64-byte start-address contract. Keep the complete patch set in one
    // ordered submission instead of paying one GuC scheduling quantum per
    // typed glyph.
    for chunk in coverage.chunks(CELL_PATCH_COVERAGE_BATCH_CAPACITY) {
        let mut layers = Vec::with_capacity(chunk.len());
        for coverage in chunk {
            let origin = coverage.mask.origin_px();
            let left = origin[0].saturating_add(translation[0]);
            let top = origin[1].saturating_add(translation[1]);
            layers.push(crate::intel::gpgpu::GpgpuGlyphMaskLayer {
                mask: coverage.mask.surface(),
                mask_rect: coverage.mask.full_rect(),
                dst_xy: crate::intel::gpgpu::GpgpuPoint::new(left, top),
                color_rgba: coverage.color_rgba,
            });
        }
        let result = crate::intel::gpgpu::glyph_mask_layers_rgba8_2d_mode(
            layers.as_slice(),
            destination,
            false,
        );
        if !result.ok {
            return Err(if result.submitted {
                GridPaperComputeFailure::SubmittedIncomplete(
                    "gridpaper-cell-patch-batch-incomplete",
                )
            } else {
                GridPaperComputeFailure::Unavailable("gridpaper-cell-patch-batch-unavailable")
            });
        }
        stats.coverage_submits = stats.coverage_submits.saturating_add(result.submits);
        stats.coverage_walkers = stats.coverage_walkers.saturating_add(result.active_walkers);
    }
    Ok(stats)
}

fn lower_resident_font_instance_layer(
    layer: &ResidentLayer,
    programs: &[Option<GpuFontInstanceProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    viewport_translation_px: [f32; 2],
) -> Option<crate::intel::gpgpu::GpgpuFontInstanceLayer> {
    let coverage = layer.coverage.as_ref()?;
    let origin = coverage.origin_px();
    let descriptor_index = layer.font_instance_descriptor?;
    let program = instance_program_for_layer(layer, programs);
    let style = program.map_or(GpuFontInstanceStyle::IDENTITY, |program| program.style);
    let motion = program.map_or(GpuFontInstanceMotion::NONE, |program| program.motion);
    let affine =
        style.scale_permille != 1_000 || style.rotation_centidegrees != 0 || motion.is_active();
    let translation = if affine {
        viewport_translation_px
    } else {
        [
            libm::roundf(viewport_translation_px[0]),
            libm::roundf(viewport_translation_px[1]),
        ]
    };
    let center = [
        origin[0] as f32 + coverage.full_rect().width as f32 * 0.5 + translation[0],
        origin[1] as f32 + coverage.full_rect().height as f32 * 0.5 + translation[1],
    ];
    let (left, top, right, bottom) = if affine {
        let base_scale = f32::from(style.scale_permille) / 1_000.0;
        let max_scale =
            base_scale * (1.0 + (f32::from(motion.scale_amplitude_permille) / 1_000.0).abs());
        let width = coverage.full_rect().width as f32;
        let height = coverage.full_rect().height as f32;
        let radius = libm::sqrtf(width * width + height * height) * 0.5 * max_scale;
        let extent_x = radius + (f32::from(motion.translation_x_tenths_px) / 10.0).abs() + 2.0;
        let extent_y = radius + (f32::from(motion.translation_y_tenths_px) / 10.0).abs() + 2.0;
        (
            libm::floorf(center[0] - extent_x) as i32,
            libm::floorf(center[1] - extent_y) as i32,
            libm::ceilf(center[0] + extent_x) as i32,
            libm::ceilf(center[1] + extent_y) as i32,
        )
    } else {
        let left = origin[0].saturating_add(translation[0] as i32);
        let top = origin[1].saturating_add(translation[1] as i32);
        (
            left,
            top,
            left.saturating_add(coverage.full_rect().width as i32),
            top.saturating_add(coverage.full_rect().height as i32),
        )
    };
    Some(crate::intel::gpgpu::GpgpuFontInstanceLayer {
        mask: coverage.surface(),
        mask_rect: coverage.full_rect(),
        dst_center: center,
        dispatch_rect: crate::intel::gpgpu::GpgpuRect::new(
            left,
            top,
            right.saturating_sub(left) as u32,
            bottom.saturating_sub(top) as u32,
        ),
        descriptor_index,
    })
}

fn lower_resident_identity_mask_layer(
    layer: &ResidentLayer,
    viewport_translation_px: [f32; 2],
    color_rgba: u32,
) -> Option<crate::intel::gpgpu::GpgpuGlyphMaskLayer> {
    let coverage = layer.coverage.as_ref()?;
    let origin = coverage.origin_px();
    let translation = [
        libm::roundf(viewport_translation_px[0]) as i32,
        libm::roundf(viewport_translation_px[1]) as i32,
    ];
    Some(crate::intel::gpgpu::GpgpuGlyphMaskLayer {
        mask: coverage.surface(),
        mask_rect: coverage.full_rect(),
        dst_xy: crate::intel::gpgpu::GpgpuPoint::new(
            origin[0].saturating_add(translation[0]),
            origin[1].saturating_add(translation[1]),
        ),
        color_rgba,
    })
}

fn rebuild_static_font_base(
    page: &ResidentPage,
    text_animations: &[Option<GpuFontInstanceProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    viewport_translation_px: [f32; 2],
) -> Result<StaticBaseBuildStats, GridPaperComputeFailure> {
    use crate::intel::gpgpu::GpgpuSubmissionOutcome;

    let destination = page
        .static_base
        .as_ref()
        .ok_or(GridPaperComputeFailure::Unavailable("gridpaper-static-base-missing"))?
        .surface();
    let clear = crate::intel::gpgpu::GpgpuSolidRect {
        rect: destination.bounds(),
        color_rgba: 0,
    };
    let clear_result = crate::intel::gpgpu::fill_solid_rects_rgba8_result(
        destination,
        core::slice::from_ref(&clear),
    );
    match clear_result.outcome {
        GpgpuSubmissionOutcome::Complete => {}
        GpgpuSubmissionOutcome::SubmittedIncomplete => {
            return Err(GridPaperComputeFailure::SubmittedIncomplete(
                "gridpaper-static-base-clear-incomplete",
            ));
        }
        GpgpuSubmissionOutcome::Unavailable => {
            return Err(GridPaperComputeFailure::Unavailable(
                "gridpaper-static-base-clear-unavailable",
            ));
        }
    }

    let mut stats = StaticBaseBuildStats {
        geometry_rects: 1,
        geometry_submits: clear_result.stats.submits,
        ..StaticBaseBuildStats::default()
    };
    for layer in &page.layers {
        if layer.logical_rects.is_empty() {
            continue;
        }
        let color_rgba = u32::from_le_bytes(layer.base_color);
        let rects = layer
            .logical_rects
            .iter()
            .filter_map(|rect| {
                scene_rect_to_surface(
                    *rect,
                    page.size,
                    destination,
                    viewport_translation_px,
                    color_rgba,
                )
            })
            .collect::<Vec<_>>();
        if rects.is_empty() {
            continue;
        }
        let rendered =
            crate::intel::gpgpu::fill_solid_rects_rgba8_result(destination, rects.as_slice());
        match rendered.outcome {
            GpgpuSubmissionOutcome::Complete => {
                stats.geometry_rects = stats.geometry_rects.saturating_add(rendered.stats.descs);
                stats.geometry_submits = stats
                    .geometry_submits
                    .saturating_add(rendered.stats.submits);
            }
            GpgpuSubmissionOutcome::SubmittedIncomplete => {
                return Err(GridPaperComputeFailure::SubmittedIncomplete(
                    "gridpaper-static-base-rectangles-incomplete",
                ));
            }
            GpgpuSubmissionOutcome::Unavailable => {
                return Err(GridPaperComputeFailure::Unavailable(
                    "gridpaper-static-base-rectangles-unavailable",
                ));
            }
        }
    }

    let static_layers = page
        .layers
        .iter()
        .filter(|layer| instance_program_for_layer(layer, text_animations).is_none())
        .filter_map(|layer| {
            lower_resident_identity_mask_layer(
                layer,
                viewport_translation_px,
                u32::from_le_bytes(layer.base_color),
            )
        })
        .collect::<Vec<_>>();
    for chunk in static_layers.chunks(STATIC_BASE_COVERAGE_BATCH_CAPACITY) {
        let coverage =
            crate::intel::gpgpu::glyph_mask_layers_rgba8_2d_mode(chunk, destination, false);
        if !coverage.ok {
            return Err(if coverage.submitted {
                GridPaperComputeFailure::SubmittedIncomplete(
                    "gridpaper-static-base-identity-mask-incomplete",
                )
            } else {
                GridPaperComputeFailure::Unavailable(
                    "gridpaper-static-base-identity-mask-unavailable",
                )
            });
        }
        stats.coverage_submits = stats.coverage_submits.saturating_add(coverage.submits);
        stats.coverage_walkers = stats
            .coverage_walkers
            .saturating_add(coverage.active_walkers);
    }
    let patch_stats = apply_resident_cell_patches(page, 0, destination, viewport_translation_px)?;
    stats.geometry_rects = stats
        .geometry_rects
        .saturating_add(patch_stats.geometry_rects);
    stats.geometry_submits = stats
        .geometry_submits
        .saturating_add(patch_stats.geometry_submits);
    stats.coverage_submits = stats
        .coverage_submits
        .saturating_add(patch_stats.coverage_submits);
    stats.coverage_walkers = stats
        .coverage_walkers
        .saturating_add(patch_stats.coverage_walkers);
    page.static_base_cell_patch_serial
        .store(page.cell_patch_serial, core::sync::atomic::Ordering::Release);
    Ok(stats)
}

fn render_compute_page_frame(
    page: &ResidentPage,
    text_animations: &[Option<GpuFontInstanceProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_serial: u64,
    animation_elapsed_ms: u64,
    final_rects: &[crate::intel::gpgpu::GpgpuSolidRect],
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> Result<GridPaperFrameResult, GridPaperComputeFailure> {
    use crate::intel::gpgpu::GpgpuSubmissionOutcome;

    let frame_started_ns = crate::chronos::monotonic_nanos();
    let viewport_translation_px = [
        page.pan.x * destination.width as f32 / page.size.scene_width() as f32,
        page.pan.y * destination.height as f32 / page.size.scene_height() as f32,
    ];
    if GRIDPAPER_FONT_INSTANCE_TRANSFORMS_ENABLED {
        update_font_instance_descriptors(page, text_animations, animation_serial)
            .map_err(GridPaperComputeFailure::Unavailable)?;
    }
    let static_base_rebuilt = page
        .static_base_animation_serial
        .load(core::sync::atomic::Ordering::Acquire)
        != animation_serial;
    let mut base_stats = if static_base_rebuilt {
        let stats = rebuild_static_font_base(page, text_animations, viewport_translation_px)?;
        page.static_base_animation_serial
            .store(animation_serial, core::sync::atomic::Ordering::Release);
        stats
    } else {
        StaticBaseBuildStats::default()
    };
    let static_base = page
        .static_base
        .as_ref()
        .ok_or(GridPaperComputeFailure::Unavailable("gridpaper-static-base-missing"))?
        .surface();
    let applied_patch_serial = page
        .static_base_cell_patch_serial
        .load(core::sync::atomic::Ordering::Acquire);
    if !static_base_rebuilt && applied_patch_serial != page.cell_patch_serial {
        let patch_stats = apply_resident_cell_patches(
            page,
            applied_patch_serial,
            static_base,
            viewport_translation_px,
        )?;
        base_stats.geometry_rects = base_stats
            .geometry_rects
            .saturating_add(patch_stats.geometry_rects);
        base_stats.geometry_submits = base_stats
            .geometry_submits
            .saturating_add(patch_stats.geometry_submits);
        base_stats.coverage_submits = base_stats
            .coverage_submits
            .saturating_add(patch_stats.coverage_submits);
        base_stats.coverage_walkers = base_stats
            .coverage_walkers
            .saturating_add(patch_stats.coverage_walkers);
        page.static_base_cell_patch_serial
            .store(page.cell_patch_serial, core::sync::atomic::Ordering::Release);
    }
    if !crate::intel::gpgpu::copy_rect_rgba8_complete_mode(
        static_base,
        static_base.bounds(),
        destination,
        crate::intel::gpgpu::GpgpuPoint::new(0, 0),
        true,
    ) {
        return Err(GridPaperComputeFailure::SubmittedIncomplete(
            "gridpaper-static-base-copy-incomplete",
        ));
    }
    let geometry_rects = base_stats.geometry_rects;
    let geometry_submits = base_stats.geometry_submits.saturating_add(1);
    let geometry_finished_ns = crate::chronos::monotonic_nanos();

    let (dynamic_coverage_submits, dynamic_coverage_walkers) =
        if GRIDPAPER_FONT_INSTANCE_TRANSFORMS_ENABLED {
            let coverage_layers = page
                .layers
                .iter()
                .filter(|layer| instance_program_for_layer(layer, text_animations).is_some())
                .filter_map(|layer| {
                    lower_resident_font_instance_layer(
                        layer,
                        text_animations,
                        viewport_translation_px,
                    )
                })
                .collect::<Vec<_>>();
            if coverage_layers.is_empty() {
                (0, 0)
            } else {
                let descriptor_state = page.font_instance_state.as_ref().ok_or(
                    GridPaperComputeFailure::Unavailable("gridpaper-font-instance-state-missing"),
                )?;
                let coverage = crate::intel::gpgpu::font_instance_layers_rgba8_2d_mode(
                    coverage_layers.as_slice(),
                    descriptor_state,
                    destination,
                    true,
                    animation_elapsed_ms as f32 / 1_000.0,
                );
                if !coverage.ok {
                    return Err(if coverage.submitted {
                        GridPaperComputeFailure::SubmittedIncomplete(
                            "gridpaper-font-instance-incomplete",
                        )
                    } else {
                        GridPaperComputeFailure::Unavailable("gridpaper-font-instance-unavailable")
                    });
                }
                (coverage.submits, coverage.active_walkers)
            }
        } else {
            if !GRIDPAPER_FONT_INSTANCE_FALLBACK_LOGGED
                .swap(true, core::sync::atomic::Ordering::AcqRel)
            {
                crate::log_warn!(
                    target: "gridpaper";
                    "gridpaper: font-instance transforms isolated reason=baremetal-completion-marker-timeout renderer=glyph-mask-compat color_animation=enabled affine_motion=identity action=preserve-visible-first-frame\n",
                );
            }
            let coverage_layers = page
                .layers
                .iter()
                .filter(|layer| instance_program_for_layer(layer, text_animations).is_some())
                .filter_map(|layer| {
                    lower_resident_identity_mask_layer(
                        layer,
                        viewport_translation_px,
                        u32::from_le_bytes(resident_layer_color(
                            layer,
                            text_animations,
                            animation_elapsed_ms,
                        )),
                    )
                })
                .collect::<Vec<_>>();
            if coverage_layers.is_empty() {
                (0, 0)
            } else {
                let coverage = crate::intel::gpgpu::glyph_mask_layers_rgba8_2d_mode(
                    coverage_layers.as_slice(),
                    destination,
                    true,
                );
                if !coverage.ok {
                    return Err(if coverage.submitted {
                        GridPaperComputeFailure::SubmittedIncomplete(
                            "gridpaper-dynamic-identity-mask-incomplete",
                        )
                    } else {
                        GridPaperComputeFailure::Unavailable(
                            "gridpaper-dynamic-identity-mask-unavailable",
                        )
                    });
                }
                (coverage.submits, coverage.active_walkers)
            }
        };
    let coverage_submits = base_stats
        .coverage_submits
        .saturating_add(dynamic_coverage_submits);
    let coverage_walkers = base_stats
        .coverage_walkers
        .saturating_add(dynamic_coverage_walkers);
    let coverage_finished_ns = crate::chronos::monotonic_nanos();

    if !final_rects.is_empty() {
        let cursor =
            crate::intel::gpgpu::fill_solid_rects_rgba8_scanout_result(destination, final_rects);
        match cursor.outcome {
            GpgpuSubmissionOutcome::Complete => {}
            GpgpuSubmissionOutcome::SubmittedIncomplete => {
                return Err(GridPaperComputeFailure::SubmittedIncomplete(
                    "gridpaper-compute-cursor-incomplete",
                ));
            }
            GpgpuSubmissionOutcome::Unavailable => {
                return Err(GridPaperComputeFailure::Unavailable(
                    "gridpaper-compute-cursor-unavailable",
                ));
            }
        }
    }
    let finalizer = crate::intel::gpgpu::release_rgba8_surface_for_scanout(destination);
    if !finalizer.ok {
        return Err(if finalizer.submitted {
            GridPaperComputeFailure::SubmittedIncomplete(
                "gridpaper-compute-final-release-incomplete",
            )
        } else {
            GridPaperComputeFailure::Unavailable("gridpaper-compute-final-release-unavailable")
        });
    }
    let Some(release) = finalizer.release else {
        return Err(GridPaperComputeFailure::Unavailable(
            "gridpaper-compute-final-release-missing",
        ));
    };
    let finished_ns = crate::chronos::monotonic_nanos();
    Ok(GridPaperFrameResult {
        changed_pixels: destination.width as usize * destination.height as usize,
        static_base_rebuilt,
        frame_us: finished_ns.saturating_sub(frame_started_ns) / 1_000,
        geometry_us: geometry_finished_ns.saturating_sub(frame_started_ns) / 1_000,
        geometry_rects,
        geometry_submits,
        coverage_us: coverage_finished_ns.saturating_sub(geometry_finished_ns) / 1_000,
        coverage_submits,
        coverage_walkers,
        present_copy_us: finished_ns.saturating_sub(coverage_finished_ns) / 1_000,
        release,
    })
}

fn scene_rect_to_surface(
    rect: SceneRect,
    size: GridSize,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    viewport_translation_px: [f32; 2],
    color_rgba: u32,
) -> Option<crate::intel::gpgpu::GpgpuSolidRect> {
    let scale_x = destination.width as f32 / size.scene_width() as f32;
    let scale_y = destination.height as f32 / size.scene_height() as f32;
    let left = libm::floorf(rect.left * scale_x + viewport_translation_px[0]) as i32;
    let top = libm::floorf(rect.top * scale_y + viewport_translation_px[1]) as i32;
    let right = libm::ceilf(rect.right * scale_x + viewport_translation_px[0]) as i32;
    let bottom = libm::ceilf(rect.bottom * scale_y + viewport_translation_px[1]) as i32;
    let clipped_left = left.clamp(0, destination.width as i32);
    let clipped_top = top.clamp(0, destination.height as i32);
    let clipped_right = right.clamp(0, destination.width as i32);
    let clipped_bottom = bottom.clamp(0, destination.height as i32);
    if clipped_right <= clipped_left || clipped_bottom <= clipped_top {
        return None;
    }
    Some(crate::intel::gpgpu::GpgpuSolidRect {
        rect: crate::intel::gpgpu::GpgpuRect::new(
            clipped_left,
            clipped_top,
            (clipped_right - clipped_left) as u32,
            (clipped_bottom - clipped_top) as u32,
        ),
        color_rgba,
    })
}

fn resident_layer_color(
    layer: &ResidentLayer,
    text_animations: &[Option<GpuFontInstanceProgram>; TEXT_ANIMATION_COLOR_SLOTS],
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
    let rgba = program.sample_color(
        GpuFontRgba::new(
            layer.base_color[0],
            layer.base_color[1],
            layer.base_color[2],
            layer.base_color[3],
        ),
        animation_elapsed_ms,
    );
    [rgba.r, rgba.g, rgba.b, rgba.a]
}

fn render_print_page(
    request: PrintRenderRequest,
    text_animations: &[Option<GpuFontInstanceProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_elapsed_ms: u64,
) -> PrintRenderResult {
    use core::sync::atomic::Ordering;

    let job_id = request.job_id;
    let generation = request.generation;
    let size = request.size;
    let (print_capture_width, print_capture_height) = size.print_capture_extent();
    let snapshot = OwnedSnapshot {
        raw: request.raw,
        producer: GridPaperProducer::Blueprint(0),
        generation,
        scale_percent: 100,
        size,
        serial: u64::from(job_id),
    };
    let result = (|| {
        if GRIDPAPER_COMPUTE_QUARANTINED.load(Ordering::Acquire) {
            return Err("gridpaper-compute-producer-quarantined");
        }
        let page = build_resident_page(
            PRIMARY_INSTANCE_ID,
            &snapshot,
            text_animations,
            print_capture_width,
            print_capture_height,
            ScenePan::ZERO,
        )?;
        if page
            .layers
            .iter()
            .any(|layer| layer.text_color_selector.is_some() && layer.coverage.is_none())
        {
            return Err("gridpaper-compute-text-coverage-required");
        }
        let destination = crate::intel::gpgpu::allocate_font_instance_rgba8_surface(
            print_capture_width,
            print_capture_height,
        )
        .ok_or("gridpaper-print-raster-allocation")?;
        let rendered = render_compute_page_frame(
            &page,
            text_animations,
            generation.max(1),
            animation_elapsed_ms,
            &[],
            destination.surface(),
        )
        .map_err(|failure| {
            if failure.submitted_incomplete() {
                GRIDPAPER_COMPUTE_QUARANTINED.store(true, Ordering::Release);
                crate::log_error!(
                    target: "gridpaper";
                    "gridpaper: print compute producer quarantined job={} generation={} extent={}x{} reason={} action=reject-future-gridpaper-compute-until-reboot\n",
                    job_id,
                    generation,
                    print_capture_width,
                    print_capture_height,
                    failure.reason(),
                );
            }
            failure.reason()
        })?;
        let rgba_premultiplied = destination
            .readback_tight_rgba()
            .ok_or("gridpaper-print-raster-readback")?;
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: print raster ready job={} generation={} grid={}x{} extent={}x{} rgba_bytes={} rectangle_descs={} rectangle_submits={} coverage_submits={} coverage_walkers={} static_base_rebuilt={} frame_us={} font_path=skrifa-r8-identity-mask renderer=gpgpu-compute-blueprint-scene readback=one-shot-after-release next=pwg-raster\n",
            job_id,
            generation,
            size.columns(),
            size.rows(),
            print_capture_width,
            print_capture_height,
            rgba_premultiplied.len(),
            rendered.geometry_rects,
            rendered.geometry_submits,
            rendered.coverage_submits,
            rendered.coverage_walkers,
            rendered.static_base_rebuilt,
            rendered.frame_us,
        );
        Ok(PrintRasterFrame {
            width: print_capture_width,
            height: print_capture_height,
            rgba_premultiplied,
        })
    })();
    if let Err(reason) = &result {
        crate::log_warn!(
            target: "gridpaper";
            "gridpaper: print raster failed job={} generation={} grid={}x{} extent={}x{} reason={} renderer=gpgpu-compute-blueprint-scene next=print2d-failed\n",
            job_id,
            generation,
            size.columns(),
            size.rows(),
            print_capture_width,
            print_capture_height,
            reason,
        );
    }
    PrintRenderResult { job_id, result }
}

fn sampled_text_colors(
    page: &ResidentPage,
    text_animations: &[Option<GpuFontInstanceProgram>; TEXT_ANIMATION_COLOR_SLOTS],
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
    color_rgba: u32,
    ring_index: usize,
    ring_count: usize,
) -> Option<[crate::intel::gpgpu::GpgpuSolidRect; 4]> {
    let metrics =
        GridSceneMetrics::new(page.size, page.scale_percent, surface.width, surface.height);
    let scene_width = page.size.scene_width();
    let scene_height = page.size.scene_height();
    let cell = metrics.cell_rect(selection);
    let mut scene_left = cell.left + page.pan.x;
    let scene_top = cell.top + page.pan.y;
    let scene_right = cell.right + page.pan.x;
    let mut scene_bottom = cell.bottom + page.pan.y;
    if input_field == CellInputField::Upper {
        scene_left += metrics.cell_width * 0.5;
        scene_bottom -= metrics.cell_height * 0.5;
    }
    let left = libm::floorf(scene_left * surface.width as f32 / scene_width as f32) as i32;
    let top = libm::floorf(scene_top * surface.height as f32 / scene_height as f32) as i32;
    let right = libm::ceilf(scene_right * surface.width as f32 / scene_width as f32) as i32;
    let bottom = libm::ceilf(scene_bottom * surface.height as f32 / scene_height as f32) as i32;
    let mut clipped_left = left.clamp(0, surface.width as i32) as u32;
    let mut clipped_top = top.clamp(0, surface.height as i32) as u32;
    let mut clipped_right = right.clamp(0, surface.width as i32) as u32;
    let mut clipped_bottom = bottom.clamp(0, surface.height as i32) as u32;
    if clipped_left >= clipped_right || clipped_top >= clipped_bottom {
        return None;
    }
    let available_width = clipped_right - clipped_left;
    let available_height = clipped_bottom - clipped_top;
    // Shared-cell selections become nested color bands. Adapt the stroke to
    // the number of owners so the common two/three-cursor case remains fully
    // visible rather than allowing the last submitted outline to hide all
    // earlier cursor colors.
    let ring_count = u32::try_from(ring_count.max(1)).unwrap_or(u32::MAX);
    let stroke = GRID_CURSOR_STROKE_PX
        .min(available_width / ring_count.saturating_mul(2).max(1))
        .min(available_height / ring_count.saturating_mul(2).max(1))
        .max(1);
    let inset = u32::try_from(ring_index)
        .unwrap_or(u32::MAX)
        .saturating_mul(stroke);
    clipped_left = clipped_left.saturating_add(inset);
    clipped_top = clipped_top.saturating_add(inset);
    clipped_right = clipped_right.saturating_sub(inset);
    clipped_bottom = clipped_bottom.saturating_sub(inset);
    if clipped_left >= clipped_right || clipped_top >= clipped_bottom {
        return None;
    }
    let width = clipped_right - clipped_left;
    let height = clipped_bottom - clipped_top;
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

fn grid_cell_damage_rect(
    surface: &GridPaperSurface,
    page: &ResidentPage,
    selection: GridCellSelection,
) -> Option<crate::ui4::DamageRect> {
    if selection.column >= page.size.columns() || selection.row >= page.size.rows() {
        return None;
    }
    let metrics =
        GridSceneMetrics::new(page.size, page.scale_percent, surface.width, surface.height);
    let scene_width = page.size.scene_width();
    let scene_height = page.size.scene_height();
    let cell = metrics.cell_rect(selection);
    let scene_left = cell.left + page.pan.x;
    let scene_top = cell.top + page.pan.y;
    let left = libm::floorf(scene_left * surface.width as f32 / scene_width as f32) as i32;
    let top = libm::floorf(scene_top * surface.height as f32 / scene_height as f32) as i32;
    let right =
        libm::ceilf((scene_left + metrics.cell_width) * surface.width as f32 / scene_width as f32)
            as i32;
    let bottom = libm::ceilf(
        (scene_top + metrics.cell_height) * surface.height as f32 / scene_height as f32,
    ) as i32;
    let clipped_left = left.clamp(0, surface.width as i32) as u32;
    let clipped_top = top.clamp(0, surface.height as i32) as u32;
    let clipped_right = right.clamp(0, surface.width as i32) as u32;
    let clipped_bottom = bottom.clamp(0, surface.height as i32) as u32;
    (clipped_right > clipped_left && clipped_bottom > clipped_top).then(|| {
        crate::ui4::DamageRect::new(
            clipped_left,
            clipped_top,
            clipped_right - clipped_left,
            clipped_bottom - clipped_top,
        )
    })
}

fn union_grid_cell_damage(
    surface: &GridPaperSurface,
    page: &ResidentPage,
    selections: impl Iterator<Item = GridCellSelection>,
) -> Option<crate::ui4::DamageRect> {
    selections
        .filter_map(|selection| grid_cell_damage_rect(surface, page, selection))
        .reduce(crate::ui4::DamageRect::union)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GridCursorInputState {
    source: crate::ui4::Ui4CursorSource,
    selection: Option<GridCellSelection>,
    input_field: CellInputField,
}

impl GridCursorInputState {
    const fn new(source: crate::ui4::Ui4CursorSource) -> Self {
        Self {
            source,
            selection: None,
            input_field: CellInputField::Primary,
        }
    }
}

struct GridPaperRuntime {
    surface: GridPaperSurface,
    observed_serial: u64,
    observed_animation_serial: u64,
    text_animations: [Option<GpuFontInstanceProgram>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_started_ms: u64,
    animation_dirty: bool,
    last_sampled_text_colors: [Option<[u8; 4]>; TEXT_ANIMATION_COLOR_SLOTS],
    animation_frames: u64,
    last_animation_frame_ms: u64,
    latest_snapshot: Option<OwnedSnapshot>,
    queued_snapshot: Option<OwnedSnapshot>,
    pending: Option<ResidentPage>,
    active: Option<ResidentPage>,
    pan: ScenePan,
    pan_dirty: bool,
    hot_pan_frames: u64,
    active_pan_source: Option<crate::ui4::Ui4CursorSource>,
    pending_pan_pixels: (i32, i32),
    cursor_inputs: Vec<GridCursorInputState>,
    cursor_damage_cells: Vec<GridCellSelection>,
    cursor_dirty: bool,
    dirty_cells: VecDeque<GridCellSelection>,
    presented_cell_patch_serial: u64,
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
            last_animation_frame_ms: 0,
            latest_snapshot: None,
            queued_snapshot: None,
            pending: None,
            active: None,
            pan: ScenePan::ZERO,
            pan_dirty: false,
            hot_pan_frames: 0,
            active_pan_source: None,
            pending_pan_pixels: (0, 0),
            cursor_inputs: Vec::new(),
            cursor_damage_cells: Vec::new(),
            cursor_dirty: false,
            dirty_cells: VecDeque::new(),
            presented_cell_patch_serial: 0,
            keyboard_edits: 0,
            last_build_error: None,
            last_render_error: None,
        }
    }

    fn presented_producer(&self) -> Option<GridPaperProducer> {
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
        if self
            .cursor_inputs
            .iter()
            .any(|cursor| cursor.selection.is_some())
        {
            self.cursor_dirty = true;
        }
        self.cursor_inputs.clear();
        self.cursor_damage_cells.clear();
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
    producer: GridPaperProducer,
    scale_percent: u16,
) -> Result<crate::ui4::WindowSessionId, ServiceError> {
    let session = crate::ui4::begin_additional_window_session(UI4_OWNER)?;
    let snapshot_is_current =
        snapshot_after(runtime.surface.pool_slot, runtime.observed_serial).is_none();
    let expose_retained_front = runtime.active.is_some()
        && runtime.pending.is_none()
        && runtime.queued_snapshot.is_none()
        && snapshot_is_current;
    if let Err(error) = attach_presentation(
        &mut runtime.surface,
        producer,
        session,
        expose_retained_front,
        scale_percent,
    ) {
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
        crate::ui4::WindowSessionCloseRequest::default().direct_plane_animate_and_retire_frames()
    } else {
        crate::ui4::WindowSessionCloseRequest::default().direct_plane_animate()
    };
    let release = crate::ui4::finish_window_session_with_request(UI4_OWNER, session, close_request);
    let frame_transferred = retire_frame && release.is_ok();
    set_input_route(runtime.surface.pool_slot, None);
    let Some(presentation) = release_presentation(&mut runtime.surface) else {
        return frame_transferred;
    };
    if matches!(presentation.producer, GridPaperProducer::Kernel(_)) {
        KERNEL_GRID_PRESENTATIONS.lock()[runtime.surface.pool_slot] = None;
    }
    runtime.reset_detached_input();
    match release {
        Ok(closed_windows) => crate::log_info!(
            target: "gridpaper";
            "gridpaper: presentation released pool_slot={} instance={} producer={:?} session={} window={} frame={} closed_windows={} retained_gpu_scene=1 retained_frame=1\n",
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
            "gridpaper: presentation release pool_slot={} instance={} producer={:?} session={} window={} frame={} error={:?} action=consider-detached retained_gpu_scene=1 retained_frame=1\n",
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
        let topology_changed = runtime
            .text_animations
            .iter()
            .zip(update.programs.iter())
            .any(|(before, after)| before.is_some() != after.is_some());
        runtime.observed_animation_serial = update.serial;
        runtime.text_animations = update.programs;
        runtime.animation_started_ms = Instant::now().as_millis();
        runtime.animation_dirty = true;
        let animation_cadence_ms = if matches!(
            runtime.presented_producer(),
            Some(GridPaperProducer::Kernel(KernelGridOwner {
                client: KernelGridClient::SpiritResponse,
                ..
            }))
        ) {
            SPIRIT_MOTION_FRAME_PERIOD_MS
        } else {
            SERVICE_PERIOD_MS
        };
        if topology_changed && let Some(snapshot) = runtime.latest_snapshot.as_ref() {
            runtime.pending = None;
            runtime.queued_snapshot = Some(snapshot.clone());
            runtime.dirty_cells.clear();
        }
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: font-instance-table activated pool_slot={} instance={} serial={} programs={} cadence_ms={} clock=monotonic-elapsed evaluator=glyph-mask-compat color_animation=enabled affine_motion=identity topology_changed={} topology_rebuild={} geometry_uploads=0\n",
            pool_slot,
            instance_id,
            runtime.observed_animation_serial,
            runtime.text_animations.iter().flatten().count(),
            animation_cadence_ms,
            topology_changed,
            topology_changed && runtime.latest_snapshot.is_some(),
        );
    }

    if let Some(snapshot) = snapshot_after(pool_slot, runtime.observed_serial) {
        runtime.observed_serial = snapshot.serial;
        let clamped_pan = runtime.pan.clamped(snapshot.scale_percent, snapshot.size);
        if clamped_pan != runtime.pan {
            runtime.pan = clamped_pan;
            if let Some(page) = runtime.active.as_mut() {
                page.pan = runtime.pan;
                page.invalidate_static_base();
            }
            runtime.pan_dirty = true;
        }
        let mut invalid_cursor_selections = Vec::new();
        for cursor in &mut runtime.cursor_inputs {
            if cursor.selection.is_some_and(|selection| {
                selection.column >= snapshot.size.columns() || selection.row >= snapshot.size.rows()
            }) {
                invalid_cursor_selections.extend(cursor.selection);
                cursor.selection = None;
                cursor.input_field = CellInputField::Primary;
            }
        }
        if !invalid_cursor_selections.is_empty() {
            for selection in invalid_cursor_selections {
                queue_cursor_damage(runtime, Some(selection));
            }
            runtime.cursor_dirty = true;
        }
        runtime.pending = None;
        runtime.dirty_cells.clear();
        runtime.latest_snapshot = Some(snapshot.clone());
        runtime.queued_snapshot = Some(snapshot);
    }
}

fn queue_cursor_damage(runtime: &mut GridPaperRuntime, selection: Option<GridCellSelection>) {
    let Some(selection) = selection else {
        return;
    };
    if !runtime.cursor_damage_cells.contains(&selection) {
        runtime.cursor_damage_cells.push(selection);
    }
}

fn grid_cursor_input_index(
    runtime: &mut GridPaperRuntime,
    source: crate::ui4::Ui4CursorSource,
) -> Option<usize> {
    if let Some(index) = runtime
        .cursor_inputs
        .iter()
        .position(|cursor| cursor.source == source)
    {
        return Some(index);
    }
    if runtime.cursor_inputs.len() >= GRID_CURSOR_STATE_CAPACITY {
        crate::log_warn!(
            target: "gridpaper";
            "gridpaper: cursor input state rejected instance={} controller={} slot={} ep={} capacity={} action=retain-existing-cursors\n",
            runtime.surface.instance_id,
            source.controller_id,
            source.slot_id,
            source.ep_target,
            GRID_CURSOR_STATE_CAPACITY,
        );
        return None;
    }
    runtime
        .cursor_inputs
        .push(GridCursorInputState::new(source));
    Some(runtime.cursor_inputs.len() - 1)
}

fn remove_grid_cursor_input(runtime: &mut GridPaperRuntime, source: crate::ui4::Ui4CursorSource) {
    let Some(index) = runtime
        .cursor_inputs
        .iter()
        .position(|cursor| cursor.source == source)
    else {
        return;
    };
    let previous = runtime.cursor_inputs[index].selection;
    runtime.cursor_inputs.remove(index);
    queue_cursor_damage(runtime, previous);
    runtime.cursor_dirty |= previous.is_some();
}

fn prune_grid_cursor_inputs(runtime: &mut GridPaperRuntime) {
    let Some(window) = runtime.presented_window() else {
        return;
    };
    let frame = crate::ui4::CursorFrameKey::new(UI4_OWNER, window);
    let mut index = 0usize;
    while index < runtime.cursor_inputs.len() {
        let cursor = runtime.cursor_inputs[index];
        if crate::ui4::selected_frame_for_source(cursor.source) == Some(frame) {
            index += 1;
            continue;
        }
        runtime.cursor_inputs.remove(index);
        queue_cursor_damage(runtime, cursor.selection);
        runtime.cursor_dirty |= cursor.selection.is_some();
    }
}

fn select_gridpaper_cell(
    runtime: &mut GridPaperRuntime,
    source: crate::ui4::Ui4CursorSource,
    local_x: i32,
    local_y: i32,
) {
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
    let Some(cursor_index) = grid_cursor_input_index(runtime, source) else {
        return;
    };
    let previous = runtime.cursor_inputs[cursor_index].selection;
    if next == previous {
        return;
    }
    runtime.cursor_inputs[cursor_index].selection = next;
    runtime.cursor_inputs[cursor_index].input_field = CellInputField::Primary;
    queue_cursor_damage(runtime, previous);
    queue_cursor_damage(runtime, next);
    runtime.cursor_dirty = true;
    if let Some(selected) = next {
        let color = ui4_cursor_rgba(source);
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: cell selected instance={} column={} row={} local={},{} scale={} pan_scene={:.3},{:.3} controller={} slot={} ep={} cursor_rgba={:02X}{:02X}{:02X}{:02X} active_cursors={} input=ui4-primary-click\n",
            runtime.surface.instance_id,
            selected.column,
            selected.row,
            local_x,
            local_y,
            scale_percent.unwrap_or(0),
            runtime.pan.x,
            runtime.pan.y,
            source.controller_id,
            source.slot_id,
            source.ep_target,
            color[0],
            color[1],
            color[2],
            color[3],
            runtime.cursor_inputs.len(),
        );
    } else {
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: cell selection cleared instance={} local={},{} controller={} slot={} ep={} active_cursors={} input=ui4-primary-click-outside-grid\n",
            runtime.surface.instance_id,
            local_x,
            local_y,
            source.controller_id,
            source.slot_id,
            source.ep_target,
            runtime.cursor_inputs.len(),
        );
    }
}

fn queue_dirty_cell(dirty_cells: &mut VecDeque<GridCellSelection>, selection: GridCellSelection) {
    if let Some(index) = dirty_cells
        .iter()
        .position(|candidate| *candidate == selection)
    {
        dirty_cells.remove(index);
    }
    dirty_cells.push_back(selection);
}

fn edit_gridpaper_cell(
    runtime: &mut GridPaperRuntime,
    source: crate::ui4::Ui4CursorSource,
    event: crate::r::keyboard::TrueosKeyboardOutputEvent,
    combo_id: u32,
    virtual_keyboard: bool,
) {
    let Some(cursor_index) = grid_cursor_input_index(runtime, source) else {
        return;
    };
    let Some(mut selected) = runtime.cursor_inputs[cursor_index].selection else {
        return;
    };
    let mut input_field = runtime.cursor_inputs[cursor_index].input_field;
    let default_foreground = grid_foreground_for_cursor(source);
    let (outcome, edited_state) = {
        let Some(snapshot) = runtime.latest_snapshot.as_mut() else {
            return;
        };
        let outcome = edit_snapshot_from_keyboard_with_foreground(
            snapshot,
            &mut selected,
            &mut input_field,
            event,
            default_foreground,
        );
        if let Some(edited) = outcome.edited_cell
            && matches!(
                snapshot.producer,
                GridPaperProducer::Kernel(KernelGridOwner {
                    client: KernelGridClient::SpiritResponse,
                    ..
                })
            )
        {
            let offset = (edited.row * COLUMNS + edited.column) * CELL_BYTES;
            let rainbow_index = edited
                .row
                .saturating_mul(snapshot.size.columns())
                .saturating_add(edited.column)
                % SPIRIT_RAINBOW_SELECTORS.len();
            snapshot.raw[offset + FOREGROUND_OFFSET] = SPIRIT_RAINBOW_SELECTORS[rainbow_index];
        }
        let edited_state = outcome.edited_cell.map(|edited| {
            let offset = (edited.row * COLUMNS + edited.column) * CELL_BYTES;
            (
                edited,
                usize::from(snapshot.raw[offset + PRIMARY_LENGTH_OFFSET]),
                usize::from(snapshot.raw[offset + UPPER_LENGTH_OFFSET]),
                snapshot.raw[offset + FOREGROUND_OFFSET],
            )
        });
        (outcome, edited_state)
    };
    if outcome.capacity_rejected {
        crate::log_warn!(
            target: "gridpaper";
            "gridpaper: cell input rejected instance={} column={} row={} field={} rule=one-unicode-scalar-and-upper-requires-primary input=ui4-keyboard\n",
            runtime.surface.instance_id,
            selected.column,
            selected.row,
            input_field.name(),
        );
    }
    if outcome.input_field_changed {
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: cell input field toggled instance={} column={} row={} field={} key=tab input=ui4-focused-keyboard\n",
            runtime.surface.instance_id,
            selected.column,
            selected.row,
            input_field.name(),
        );
    }
    let previous_selection = runtime.cursor_inputs[cursor_index].selection;
    if outcome.clear_selection {
        runtime.cursor_inputs[cursor_index].selection = None;
        runtime.cursor_inputs[cursor_index].input_field = CellInputField::Primary;
        queue_cursor_damage(runtime, previous_selection);
        runtime.cursor_dirty = true;
    } else {
        runtime.cursor_inputs[cursor_index].selection = Some(selected);
        runtime.cursor_inputs[cursor_index].input_field = input_field;
        if outcome.selection_changed || outcome.input_field_changed {
            queue_cursor_damage(runtime, previous_selection);
            queue_cursor_damage(runtime, Some(selected));
        }
        runtime.cursor_dirty |= outcome.selection_changed || outcome.input_field_changed;
    }
    if !outcome.content_changed {
        return;
    }
    runtime.keyboard_edits = runtime.keyboard_edits.saturating_add(1);
    let mut presentations = KERNEL_GRID_PRESENTATIONS.lock();
    if let Some(record) = presentations[runtime.surface.pool_slot].as_mut()
        && record.owner.client == KernelGridClient::SpiritResponse
    {
        record.accepted_keyboard_edits = record.accepted_keyboard_edits.saturating_add(1);
        if event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_TEXT {
            record.accepted_text_cells = record.accepted_text_cells.saturating_add(1);
        }
    }
    let (edited, primary_len, upper_len, foreground) =
        edited_state.unwrap_or((selected, 0, 0, COLOR_TRANSPARENT));
    let snapshot = runtime
        .latest_snapshot
        .as_ref()
        .expect("GridPaper edited snapshot remains resident");
    mirror_blueprint_cell_edit(runtime.surface.pool_slot, snapshot, edited);
    let active_matches = runtime.active.as_ref().is_some_and(|page| {
        page.serial == snapshot.serial
            && page.generation == snapshot.generation
            && page.scale_percent == snapshot.scale_percent
            && page.size == snapshot.size
    });
    let animated_or_transformed = runtime
        .text_animations
        .get(usize::from(foreground))
        .copied()
        .flatten()
        .is_some();
    let cell_patch = active_matches
        && runtime.pending.is_none()
        && runtime.queued_snapshot.is_none()
        && !animated_or_transformed;
    let action = if cell_patch {
        queue_dirty_cell(&mut runtime.dirty_cells, edited);
        "queue-cell-patch"
    } else {
        runtime.queued_snapshot = Some(snapshot.clone());
        runtime.pending = None;
        runtime.dirty_cells.clear();
        "rebuild-page-fallback"
    };
    if runtime.keyboard_edits <= 64 || runtime.keyboard_edits.is_multiple_of(120) || !cell_patch {
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: cell edited instance={} edit_seq={} ring_seq={} device_seq={} column={} row={} field={} primary_utf8_bytes={} upper_utf8_bytes={} key_kind={} codepoint={} controller={} slot={} ep={} combo={} virtual_keyboard={} cursor_foreground={} input=ui4-keyboard action={} animated_or_transformed={} queued_cells={} log_policy=first-64+each-120+fallback\n",
            runtime.surface.instance_id,
            runtime.keyboard_edits,
            event.seq,
            event.device_seq,
            edited.column,
            edited.row,
            input_field.name(),
            primary_len,
            upper_len,
            event.kind,
            event.codepoint,
            event.controller_id,
            event.slot_id,
            event.ep_target,
            combo_id,
            virtual_keyboard,
            default_foreground,
            action,
            animated_or_transformed,
            runtime.dirty_cells.len(),
        );
    }
}

fn mirror_blueprint_cell_edit(
    pool_slot: usize,
    snapshot: &OwnedSnapshot,
    selection: GridCellSelection,
) {
    let GridPaperProducer::Blueprint(owner) = snapshot.producer else {
        return;
    };
    let offset = (selection.row * COLUMNS + selection.column) * CELL_BYTES;
    let end = offset + CELL_BYTES;
    let mut stores = SNAPSHOTS.lock();
    let Some(store) = stores.get_mut(pool_slot) else {
        return;
    };
    if store.owner != Some(owner)
        || store.serial != snapshot.serial
        || store.generation != snapshot.generation
    {
        return;
    }
    store.buffers[store.published][offset..end].copy_from_slice(&snapshot.raw[offset..end]);
}

fn build_dirty_cell_patches(runtime: &mut GridPaperRuntime) {
    for _ in 0..CELL_PATCH_BUILD_BUDGET_PER_TICK {
        let Some(selection) = runtime.dirty_cells.pop_front() else {
            break;
        };
        let Some(snapshot) = runtime.latest_snapshot.as_ref() else {
            runtime.dirty_cells.clear();
            break;
        };
        let result = build_resident_cell_patch(
            runtime.surface.instance_id,
            snapshot,
            selection,
            runtime.surface.width,
            runtime.surface.height,
        );
        match result {
            Ok(patch) => {
                let Some(page) = runtime.active.as_mut() else {
                    runtime.queued_snapshot = Some(snapshot.clone());
                    runtime.dirty_cells.clear();
                    break;
                };
                if let Err(error) = page.install_cell_patch(patch) {
                    crate::log_warn!(
                        target: "gridpaper";
                        "gridpaper: cell patch state unavailable instance={} column={} row={} reason={} action=rebuild-page-fallback\n",
                        runtime.surface.instance_id,
                        selection.column,
                        selection.row,
                        error,
                    );
                    runtime.queued_snapshot = Some(snapshot.clone());
                    runtime.pending = None;
                    runtime.dirty_cells.clear();
                    runtime.last_build_error = None;
                    break;
                }
            }
            Err(error) if retryable_font_coverage_error(error) => {
                runtime.dirty_cells.push_front(selection);
                crate::log_warn!(
                    target: "gridpaper";
                    "gridpaper: cell patch deferred instance={} column={} row={} reason={} queued_cells={} action=retain-dirty-cell+retry-gpgpu-coverage\n",
                    runtime.surface.instance_id,
                    selection.column,
                    selection.row,
                    error,
                    runtime.dirty_cells.len(),
                );
                break;
            }
            Err(error) => {
                crate::log_warn!(
                    target: "gridpaper";
                    "gridpaper: cell patch unavailable instance={} column={} row={} reason={} action=rebuild-page-fallback\n",
                    runtime.surface.instance_id,
                    selection.column,
                    selection.row,
                    error,
                );
                runtime.queued_snapshot = Some(snapshot.clone());
                runtime.pending = None;
                runtime.dirty_cells.clear();
                runtime.last_build_error = None;
                break;
            }
        }
    }
}

fn retryable_font_coverage_error(error: &str) -> bool {
    error == "font-coverage-dispatch"
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
                snapshot.size,
            ) {
                if let Some(page) = runtime.pending.as_mut() {
                    page.pan = runtime.pan;
                    page.invalidate_static_base();
                }
                if let Some(page) = runtime.active.as_mut() {
                    page.pan = runtime.pan;
                    page.invalidate_static_base();
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

fn is_gridpaper_print_key(event: crate::r::keyboard::TrueosKeyboardOutputEvent) -> bool {
    event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
        && event.key_code == crate::r::keyboard::KEYBOARD_KEY_PRINT_SCREEN
}

fn is_gridpaper_printer_menu_key(event: crate::r::keyboard::TrueosKeyboardOutputEvent) -> bool {
    event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
        && event.key_code == crate::r::keyboard::KEYBOARD_KEY_F10
}

fn remove_printer_menu_context(context: u64) -> Option<PrinterMenuContext> {
    let mut contexts = PRINTER_MENU_CONTEXTS.lock();
    let index = contexts
        .iter()
        .position(|candidate| candidate.id == context)?;
    contexts.remove(index)
}

fn complete_gridpaper_printer_menu(result: crate::ui4::ContextMenuResult) {
    let Some(context) = remove_printer_menu_context(result.context) else {
        return;
    };
    if result.owner != UI4_OWNER {
        return;
    }
    if result.reason != crate::ui4::ContextMenuCloseReason::Selected {
        crate::log_info!(
            target: "gridpaper";
            "gridpaper: printer menu closed context={} window={} reason={:?} action=none\n",
            result.context,
            result.window.raw(),
            result.reason,
        );
        return;
    }
    let Some(action) = result.selected_action else {
        crate::log_warn!(
            target: "gridpaper";
            "gridpaper: printer menu selection rejected context={} window={} reason={:?} action=none\n",
            result.context,
            result.window.raw(),
            result.reason,
        );
        return;
    };
    let Some(printer) = usize::try_from(action)
        .ok()
        .and_then(|index| context.printers.get(index))
        .filter(|printer| crate::r::net::printer::supports_gridpaper_print(printer))
    else {
        crate::log_warn!(
            target: "gridpaper";
            "gridpaper: printer menu selection rejected context={} action={} reason=missing-or-unsupported-printer\n",
            result.context,
            action,
        );
        return;
    };
    let Some(owner) = context.snapshot.producer.blueprint_owner() else {
        return;
    };
    let printer_name = printer.name.clone();
    let printer_uri = printer.uri.clone();
    match crate::r::print2d::submit_gridpaper_to_printer(
        owner,
        context.snapshot.generation,
        context.snapshot.size,
        context.snapshot.raw,
        printer_uri.as_str(),
    ) {
        Ok(job_id) => {
            crate::log_info!(
                target: "gridpaper";
                "gridpaper: printer menu selection accepted context={} action={} printer={} uri={} job={} owner={} trigger=F10\n",
                result.context,
                action,
                printer_name,
                printer_uri,
                job_id,
                owner,
            );
        }
        Err(error) => {
            crate::log_warn!(
                target: "gridpaper";
                "gridpaper: printer menu selection rejected context={} action={} printer={} error={} trigger=F10\n",
                result.context,
                action,
                printer_name,
                error,
            );
        }
    }
}

fn show_gridpaper_printer_menu(runtime: &GridPaperRuntime, event: crate::ui4::Ui4KeyboardEvent) {
    let Some(window) = runtime.presented_window() else {
        return;
    };
    let Some(snapshot) = runtime
        .latest_snapshot
        .as_ref()
        .filter(|snapshot| snapshot.producer.blueprint_owner().is_some())
        .cloned()
    else {
        return;
    };
    let mut printers = crate::r::net::printer::snapshot()
        .into_iter()
        .filter(crate::r::net::printer::supports_gridpaper_print)
        .collect::<Vec<_>>();
    if printers.len() > crate::ui4::MAX_CONTEXT_MENU_ENTRIES {
        crate::log_warn!(
            target: "gridpaper";
            "gridpaper: printer menu truncated discovered={} capacity={}\n",
            printers.len(),
            crate::ui4::MAX_CONTEXT_MENU_ENTRIES,
        );
        printers.truncate(crate::ui4::MAX_CONTEXT_MENU_ENTRIES);
    }
    let entries = if printers.is_empty() {
        vec![crate::ui4::ContextMenuEntry::disabled("NO PRINTERS")]
    } else {
        printers
            .iter()
            .enumerate()
            .map(|(index, printer)| {
                crate::ui4::ContextMenuEntry::action(&printer.name, index as u32)
            })
            .collect()
    };
    let context_id = next_printer_menu_context();
    {
        let mut contexts = PRINTER_MENU_CONTEXTS.lock();
        if contexts.len() >= PRINTER_MENU_CONTEXT_CAPACITY {
            crate::log_warn!(
                target: "gridpaper";
                "gridpaper: printer menu request rejected context={} reason=context-capacity capacity={}\n",
                context_id,
                PRINTER_MENU_CONTEXT_CAPACITY,
            );
            return;
        }
        contexts.push_back(PrinterMenuContext {
            id: context_id,
            snapshot,
            printers,
        });
    }
    let request = crate::ui4::ContextMenuRequest {
        entries,
        context: context_id,
        callback: complete_gridpaper_printer_menu,
    };
    if let Err(error) = crate::ui4::show_context_menu(event.source, UI4_OWNER, window, request) {
        remove_printer_menu_context(context_id);
        crate::log_warn!(
            target: "gridpaper";
            "gridpaper: printer menu request rejected context={} window={} error={:?} trigger=F10\n",
            context_id,
            window.raw(),
            error,
        );
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
            select_gridpaper_cell(runtime, event.source, event.local_x, event.local_y);
        }
        crate::ui4::Ui4InputEvent::Keyboard(event) if is_gridpaper_print_key(event.event) => {
            if let Some(snapshot) = runtime.latest_snapshot.as_ref()
                && queue_print_request(runtime.surface.instance_id, snapshot).is_none()
            {
                crate::log_os::print2d_job_state(
                    0,
                    "request-dropped",
                    "gridpaper-PrintScreen-queue-full",
                );
            }
        }
        crate::ui4::Ui4InputEvent::Keyboard(event)
            if is_gridpaper_printer_menu_key(event.event) =>
        {
            show_gridpaper_printer_menu(runtime, event);
        }
        crate::ui4::Ui4InputEvent::Keyboard(event) => {
            edit_gridpaper_cell(
                runtime,
                event.source,
                event.event,
                event.combo_id,
                event.virtual_keyboard,
            );
        }
        crate::ui4::Ui4InputEvent::Pan(event) => {
            pan_gridpaper(runtime, event);
        }
        crate::ui4::Ui4InputEvent::Focus(event) if !event.focused => {
            remove_grid_cursor_input(runtime, event.source);
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
        &runtime.text_animations,
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
    if page.cell_patch_serial != runtime.presented_cell_patch_serial {
        return true;
    }
    if runtime.pan_dirty || runtime.cursor_dirty || runtime.animation_dirty {
        return true;
    }
    if GRIDPAPER_FONT_INSTANCE_TRANSFORMS_ENABLED
        && runtime
            .text_animations
            .iter()
            .flatten()
            .any(|program| program.motion.is_active())
    {
        let spirit_response = matches!(
            runtime.presented_producer(),
            Some(GridPaperProducer::Kernel(KernelGridOwner {
                client: KernelGridClient::SpiritResponse,
                ..
            }))
        );
        return !spirit_response
            || now_ms.saturating_sub(runtime.last_animation_frame_ms)
                >= SPIRIT_MOTION_FRAME_PERIOD_MS;
    }
    let elapsed_ms = now_ms.saturating_sub(runtime.animation_started_ms);
    sampled_text_colors(page, &runtime.text_animations, elapsed_ms)
        != runtime.last_sampled_text_colors
}

fn runtime_needs_update(runtime: &GridPaperRuntime, now_ms: u64) -> bool {
    !runtime.dirty_cells.is_empty()
        || runtime.queued_snapshot.is_some()
        || runtime_needs_render(runtime, now_ms)
}

fn incomplete_pending_page_error(error: ServiceError) -> bool {
    matches!(error, ServiceError::Render("gridpaper-compute-text-coverage-required"))
}

fn publish_runtime(runtime: &mut GridPaperRuntime, now_ms: u64) {
    let animation_elapsed_ms = now_ms.saturating_sub(runtime.animation_started_ms);
    let mut published_page_this_tick = false;
    if runtime.surface.presentation.is_some()
        && let Some(candidate) = runtime.pending.as_ref()
    {
        let candidate_serial = candidate.serial;
        let candidate_generation = candidate.generation;
        match publish_page(
            &runtime.surface,
            candidate,
            runtime.cursor_inputs.as_slice(),
            crate::ui4::DamageRect::FULL,
            &runtime.text_animations,
            runtime.observed_animation_serial,
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
                    "gridpaper: frame published instance={} serial={} generation={} scale={} pan_scene={:.3},{:.3} layers={} coverage_masks={} rectangle_descs={} rectangle_submits={} coverage_submits={} coverage_walkers={} static_base_rebuilt={} changed_pixels={} frame_us={} rectangles_us={} coverage_us={} release_us={} font_path=skrifa-r8-to-glyph-mask-compat persistence=coverage+static-rgba-base-retained-until-next-snapshot static_base_cache=pat0-wb present_cache=pat3-uc affine_motion=identity frame_path=gpgpu-compute-blueprint-scene cpu_readback=0 cpu_frame_copy=0 gpu_frame_copy=1 resident3d_ui4=disabled\n",
                    runtime.surface.instance_id,
                    published.serial,
                    published.generation,
                    published.scale_percent,
                    published.pan.x,
                    published.pan.y,
                    published.layers.len(),
                    coverage_masks,
                    result.geometry_rects,
                    result.geometry_submits,
                    result.coverage_submits,
                    result.coverage_walkers,
                    result.static_base_rebuilt,
                    result.changed_pixels,
                    result.frame_us,
                    result.geometry_us,
                    result.coverage_us,
                    result.present_copy_us,
                );
                let published_cell_patch_serial = published.cell_patch_serial;
                let published_generation = published.generation;
                let retired = runtime.active.replace(published);
                drop(retired);
                mark_kernel_grid_generation_published(
                    runtime.surface.pool_slot,
                    published_generation,
                );
                runtime.animation_dirty = false;
                if GRIDPAPER_FONT_INSTANCE_TRANSFORMS_ENABLED
                    && runtime
                        .text_animations
                        .iter()
                        .flatten()
                        .any(|program| program.motion.is_active())
                {
                    runtime.last_animation_frame_ms = now_ms;
                }
                runtime.pan_dirty = false;
                runtime.cursor_dirty = false;
                runtime.cursor_damage_cells.clear();
                runtime.presented_cell_patch_serial = published_cell_patch_serial;
                if runtime.dirty_cells.is_empty()
                    && runtime.queued_snapshot.is_none()
                    && runtime.pending.is_none()
                {
                    mark_kernel_grid_keyboard_edits_published(
                        runtime.surface.pool_slot,
                        runtime.keyboard_edits,
                    );
                }
                published_page_this_tick = true;
                runtime.last_render_error = None;
            }
            Err(error) => {
                let incomplete = incomplete_pending_page_error(error);
                if incomplete {
                    runtime.pending = None;
                    runtime.queued_snapshot = runtime.latest_snapshot.clone();
                    runtime.last_build_error = None;
                }
                if runtime.last_render_error != Some(error) {
                    crate::log_warn!(
                        target: "gridpaper";
                        "gridpaper: frame pending instance={} serial={} generation={} error={:?} action={}\n",
                        runtime.surface.instance_id,
                        candidate_serial,
                        candidate_generation,
                        error,
                        if incomplete {
                            "discard-incomplete-page+rebuild-latest-snapshot"
                        } else {
                            "retain-front-and-retry"
                        },
                    );
                    runtime.last_render_error = Some(error);
                }
            }
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
    let gpu_motion_active = GRIDPAPER_FONT_INSTANCE_TRANSFORMS_ENABLED
        && runtime
            .text_animations
            .iter()
            .flatten()
            .any(|program| program.motion.is_active());
    let animation_changed =
        runtime.animation_dirty || sampled != runtime.last_sampled_text_colors || gpu_motion_active;
    let hot_pan_frame = runtime.pan_dirty;
    let selection_frame = runtime.cursor_dirty;
    let cell_patch_changed = page.cell_patch_serial != runtime.presented_cell_patch_serial;
    if !animation_changed && !hot_pan_frame && !selection_frame && !cell_patch_changed {
        return;
    }
    // TODO(perf, late-stage only): A color-only animation tick can eventually
    // coalesce to its latest sample and recolor retained text coverage through
    // a palette/mask fast path instead of rerasterizing the complete page.
    // Keep the retained 3D scene canonical; this must not become a separate
    // correctness, admission, or feature-semantics path.
    let damage = if animation_changed || hot_pan_frame {
        crate::ui4::DamageRect::FULL
    } else {
        let patch_cells = page
            .cell_patches
            .iter()
            .filter(|patch| patch.serial > runtime.presented_cell_patch_serial)
            .map(|patch| patch.selection);
        let cursor_cells = runtime.cursor_damage_cells.iter().copied();
        union_grid_cell_damage(&runtime.surface, page, patch_cells.chain(cursor_cells))
            .unwrap_or(crate::ui4::DamageRect::FULL)
    };
    match publish_page(
        &runtime.surface,
        page,
        runtime.cursor_inputs.as_slice(),
        damage,
        &runtime.text_animations,
        runtime.observed_animation_serial,
        animation_elapsed_ms,
    ) {
        Ok(result) => {
            runtime.last_sampled_text_colors = sampled;
            runtime.animation_dirty = false;
            runtime.pan_dirty = false;
            runtime.cursor_dirty = false;
            runtime.cursor_damage_cells.clear();
            if cell_patch_changed {
                crate::log_info!(
                    target: "gridpaper";
                    "gridpaper: cell-patch-frame instance={} edit_seq={} retained_cells={} queued_cells={} patch_serial={} ui4_damage={},{}+{}x{} rectangle_descs={} rectangle_submits={} coverage_submits={} coverage_walkers={} static_base_rebuilt={} frame_us={} font_path=skrifa-r8-identity-mask submission=aligned-multi-walker-batch static_base_cache=pat0-wb present_cache=pat3-uc action=retain-page-topology+patch-static-vm\n",
                    runtime.surface.instance_id,
                    runtime.keyboard_edits,
                    page.cell_patches.len(),
                    runtime.dirty_cells.len(),
                    page.cell_patch_serial,
                    damage.x,
                    damage.y,
                    damage.width,
                    damage.height,
                    result.geometry_rects,
                    result.geometry_submits,
                    result.coverage_submits,
                    result.coverage_walkers,
                    result.static_base_rebuilt,
                    result.frame_us,
                );
            }
            runtime.presented_cell_patch_serial = page.cell_patch_serial;
            if runtime.dirty_cells.is_empty()
                && runtime.queued_snapshot.is_none()
                && runtime.pending.is_none()
            {
                mark_kernel_grid_keyboard_edits_published(
                    runtime.surface.pool_slot,
                    runtime.keyboard_edits,
                );
            }
            if hot_pan_frame {
                runtime.hot_pan_frames = runtime.hot_pan_frames.saturating_add(1);
                if runtime.hot_pan_frames <= 8 || runtime.hot_pan_frames.is_multiple_of(120) {
                    crate::log_info!(
                        target: "gridpaper";
                        "gridpaper: hot-pan-frame instance={} seq={} pan_scene={:.3},{:.3} rectangle_descs={} rectangle_submits={} coverage_submits={} coverage_walkers={} static_base_rebuilt={} changed_pixels={} frame_us={} rectangles_us={} coverage_us={} release_us={} geometry_uploads=0 resident_mesh_rebuilds=0 transform=compute-dst-translation final_clip=compute-surface frame_path=gpgpu-compute-blueprint-scene cpu_readback=0 cpu_frame_copy=0 gpu_frame_copy=1\n",
                        runtime.surface.instance_id,
                        runtime.hot_pan_frames,
                        page.pan.x,
                        page.pan.y,
                        result.geometry_rects,
                        result.geometry_submits,
                        result.coverage_submits,
                        result.coverage_walkers,
                        result.static_base_rebuilt,
                        result.changed_pixels,
                        result.frame_us,
                        result.geometry_us,
                        result.coverage_us,
                        result.present_copy_us,
                    );
                }
            }
            if animation_changed {
                runtime.animation_frames = runtime.animation_frames.saturating_add(1);
                runtime.last_animation_frame_ms = now_ms;
            }
            if animation_changed
                && (runtime.animation_frames <= 8 || runtime.animation_frames.is_multiple_of(120))
            {
                crate::log_info!(
                    target: "gridpaper";
                    "gridpaper: text-animation-frame instance={} seq={} animation_serial={} elapsed_ms={} programs={} rectangle_descs={} rectangle_submits={} coverage_submits={} coverage_walkers={} static_base_rebuilt={} changed_pixels={} frame_us={} rectangles_us={} coverage_us={} release_us={} geometry_uploads=0 resident_mesh_rebuilds=0 frame_path=gpgpu-compute-blueprint-scene cpu_readback=0 cpu_frame_copy=0 gpu_frame_copy=1\n",
                    runtime.surface.instance_id,
                    runtime.animation_frames,
                    runtime.observed_animation_serial,
                    animation_elapsed_ms,
                    runtime.text_animations.iter().flatten().count(),
                    result.geometry_rects,
                    result.geometry_submits,
                    result.coverage_submits,
                    result.coverage_walkers,
                    result.static_base_rebuilt,
                    result.changed_pixels,
                    result.frame_us,
                    result.geometry_us,
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
    let mut observed_lease_epoch = 0u64;
    let mut last_unpresentable_epoch = 0u64;
    let mut runtime: Option<GridPaperRuntime> = None;
    let mut presentation_session = None;
    let mut last_init_error = None;
    let mut last_presentation_error = None;
    let mut presentation_retry_after_ms = 0u64;
    loop {
        let lease = pool_lease_state(pool_slot);
        if lease.epoch != observed_lease_epoch {
            if let Some(old_runtime) = runtime.take() {
                destroy_runtime(old_runtime, &mut presentation_session);
            }
            observed_lease_epoch = lease.epoch;
            last_unpresentable_epoch = 0;
            last_init_error = None;
            last_presentation_error = None;
            presentation_retry_after_ms = 0;
        }

        let Some(instance_id) = lease.local_instance_id else {
            Timer::after(EmbassyDuration::from_millis(250)).await;
            continue;
        };

        if lease.presentable_producer.is_none()
            && let Some(GridPaperProducer::Blueprint(owner)) = lease.producer
            && last_unpresentable_epoch != lease.epoch
        {
            let state = crate::hv::vm_state(owner);
            crate::log_warn!(
                target: "gridpaper";
                "gridpaper: presentation withheld pool_slot={} owner={} local_instance={} lease_epoch={} vm_running={} vm_starting={} vm_pause_latched={} action=wait-for-vm-lifecycle\n",
                pool_slot,
                owner,
                instance_id,
                lease.epoch,
                state.running as u8,
                state.starting as u8,
                state.pause_latched as u8,
            );
            last_unpresentable_epoch = lease.epoch;
        }

        if runtime.is_none() {
            match initialize_surface(pool_slot, instance_id, lease.size) {
                Ok(surface) => {
                    crate::log_info!(
                        target: "gridpaper";
                        "gridpaper: pool runtime activated pool_slot={} producer={:?} local_instance={} grid={}x{} soft_caps={}x{} worker_slot={} ui4={}x{} extent_source={} configured_scale={}\n",
                        pool_slot,
                        lease.producer,
                        instance_id,
                        lease.size.columns(),
                        lease.size.rows(),
                        COLUMN_SOFT_CAP,
                        ROW_SOFT_CAP,
                        crate::percpu::current_slot(),
                        surface.width,
                        surface.height,
                        surface.extent_source,
                        lease.scale_percent,
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
        let presentation_now_ms = Instant::now().as_millis();
        if lease.presentable_producer != runtime_ref.presented_producer()
            && presentation_now_ms >= presentation_retry_after_ms
        {
            if let Some(session) = presentation_session.take() {
                let _ = release_runtime_presentation(runtime_ref, session, false);
            }
            if let Some(producer) = lease.presentable_producer {
                match attach_runtime_presentation(runtime_ref, producer, lease.scale_percent) {
                    Ok(session) => {
                        presentation_session = Some(session);
                        let presentation = runtime_ref
                            .surface
                            .presentation
                            .expect("attached GridPaper presentation");
                        crate::log_info!(
                            target: "gridpaper";
                            "gridpaper: presentation attached pool_slot={} instance={} producer={:?} session={} window={} frame={} retained_front={} persistent_compute_scene=1 resident3d_ui4=disabled\n",
                            pool_slot,
                            runtime_ref.surface.instance_id,
                            presentation.producer,
                            presentation.session.raw(),
                            presentation.window.raw(),
                            runtime_ref.surface.frame.raw(),
                            u8::from(runtime_ref.active.is_some()),
                        );
                        last_presentation_error = None;
                        presentation_retry_after_ms = 0;
                    }
                    Err(error) => {
                        presentation_retry_after_ms = presentation_now_ms.saturating_add(250);
                        if last_presentation_error != Some(error) {
                            crate::log_warn!(
                                target: "gridpaper";
                                "gridpaper: presentation attach pending pool_slot={} instance={} error={:?} retry_ms=250 retained_compute_scene=1\n",
                                pool_slot,
                                runtime_ref.surface.instance_id,
                                error,
                            );
                        }
                        last_presentation_error = Some(error);
                    }
                }
            } else {
                last_presentation_error = None;
                presentation_retry_after_ms = 0;
            }
        }

        refresh_runtime(runtime_ref);
        for event in take_routed_input_events(pool_slot) {
            dispatch_gridpaper_input(runtime_ref, event);
        }
        prune_grid_cursor_inputs(runtime_ref);
        let now_ms = Instant::now().as_millis();
        if runtime_needs_update(runtime_ref, now_ms) {
            if pool_lease_state(pool_slot).epoch == observed_lease_epoch {
                build_dirty_cell_patches(runtime_ref);
                build_queued_page(runtime_ref);
                let publish_now_ms = Instant::now().as_millis();
                if runtime_needs_render(runtime_ref, publish_now_ms) {
                    publish_runtime(runtime_ref, publish_now_ms);
                }
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

/// Kernel controller for the Gridpaper worker pool. Blueprint documents and
/// Spirit's single retained response document share up to ten isolated worker
/// slots, each retaining its own UI4 frame and scene worker. Compute
/// presentation and print-resolution rendering are admitted by their own GuC
/// context rather than through the unrelated font service.
#[embassy_executor::task]
pub async fn gridpaper_service_task() {
    let spawned = spawn_gridpaper_instance_pool();
    crate::log_info!(
        target: "gridpaper";
        "gridpaper: pool initialized workers={} soft_cap={} producer_lane=system-service-compute-retired+shared-async-serialized resident3d_ui4=disabled\n",
        spawned,
        GRIDPAPER_POOL_SOFT_CAP,
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
    fn kernel_grid_claim_preserves_requested_scale() {
        let owner = KernelGridOwner {
            client: KernelGridClient::SpiritResponse,
            token: 1,
        };
        let size = GridSize::new(19, 13).expect("Spirit response grid size is valid");
        let mut store = SnapshotStore::new();

        store.claim_kernel(owner, size, 150, false);

        assert_eq!(store.kernel_owner, Some(owner));
        assert_eq!(store.size, size);
        assert_eq!(store.scale_percent, 150);
        assert!(store.lifecycle_paused);
    }

    #[test]
    fn shell_grid_claim_is_immediately_visible_and_keeps_native_scale() {
        let owner = KernelGridOwner {
            client: KernelGridClient::Shell2,
            token: 2,
        };
        let size = GridSize::new(39, 55).expect("full shell Gridpaper size is valid");
        let mut store = SnapshotStore::new();

        store.claim_kernel(owner, size, NATIVE_SCALE_PERCENT, true);

        assert_eq!(store.kernel_owner, Some(owner));
        assert_eq!(store.size, GridSize::FULL);
        assert_eq!(store.scale_percent, NATIVE_SCALE_PERCENT);
        assert!(!store.lifecycle_paused);
        assert!(store.producer_connected);
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
        assert_eq!((GridSize::FULL.scene_width(), GridSize::FULL.scene_height()), (199, 279));
    }

    #[test]
    fn default_gridpaper_background_is_opaque_white() {
        assert_eq!(palette(COLOR_DEFAULT, true), [255, 255, 255, 255]);
    }

    #[test]
    fn every_positive_grid_within_the_soft_caps_is_valid() {
        for columns in 1..=COLUMN_SOFT_CAP as u32 {
            for rows in 1..=ROW_SOFT_CAP as u32 {
                assert_eq!(
                    GridSize::new(columns, rows),
                    Some(GridSize {
                        columns: columns as u16,
                        rows: rows as u16,
                    })
                );
            }
        }
        for (columns, rows) in [
            (0, 1),
            (1, 0),
            (0, 0),
            (COLUMN_SOFT_CAP as u32 + 1, 1),
            (1, ROW_SOFT_CAP as u32 + 1),
        ] {
            assert_eq!(GridSize::new(columns, rows), None);
        }
    }

    #[test]
    fn print_capture_caps_the_long_edge_for_tall_and_wide_grids() {
        assert_eq!(GridSize::FULL.print_capture_extent(), (1_027, 1_440));
        assert_eq!(GridSize::new(1, 55).unwrap().print_capture_extent().1, 1_440);
        assert_eq!(GridSize::new(39, 1).unwrap().print_capture_extent().0, 1_440);
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
        assert!(!pan.drag_pixels(100, 100, 853, 1_196, 150, GridSize::FULL));
        assert!(pan.drag_pixels(-10_000, -10_000, 853, 1_196, 150, GridSize::FULL));
        assert_eq!(pan.x, -(GridSize::FULL.scene_width() as f32 * 0.5));
        assert_eq!(pan.y, -(GridSize::FULL.scene_height() as f32 * 0.5));
        assert!(pan.drag_pixels(10_000, 10_000, 853, 1_196, 150, GridSize::FULL));
        assert_eq!(pan, ScenePan::ZERO);

        assert!(!pan.drag_pixels(-100, -100, 853, 1_196, 100, GridSize::FULL));
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
            producer: GridPaperProducer::Blueprint(0),
            generation: 1,
            scale_percent: 100,
            size: GridSize::FULL,
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
    fn blank_cell_uses_the_editing_cursor_foreground_and_releases_it_when_cleared() {
        let mut snapshot = OwnedSnapshot {
            raw: Vec::from([0u8; PAGE_BYTES]),
            producer: GridPaperProducer::Blueprint(0),
            generation: 1,
            scale_percent: 100,
            size: GridSize::new(19, 13).unwrap(),
            serial: 1,
        };
        let target = GridCellSelection { column: 2, row: 3 };
        let offset = (target.row * COLUMNS + target.column) * CELL_BYTES;
        let mut selection = target;
        let mut input_field = CellInputField::Primary;
        let mut text = crate::r::keyboard::TrueosKeyboardOutputEvent::default();
        text.kind = crate::r::keyboard::KEYBOARD_OUTPUT_KIND_TEXT;
        text.utf8_len = 1;
        text.codepoint = 'x' as u32;
        text.utf8[0] = b'x';

        let first = edit_snapshot_from_keyboard_with_foreground(
            &mut snapshot,
            &mut selection,
            &mut input_field,
            text,
            13,
        );
        assert!(first.content_changed);
        assert_eq!(snapshot.raw[offset + FOREGROUND_OFFSET], 13);

        selection = target;
        let mut delete = crate::r::keyboard::TrueosKeyboardOutputEvent::default();
        delete.kind = crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY;
        delete.key_code = crate::r::keyboard::KEYBOARD_KEY_DELETE;
        let cleared = edit_snapshot_from_keyboard_with_foreground(
            &mut snapshot,
            &mut selection,
            &mut input_field,
            delete,
            13,
        );
        assert!(cleared.content_changed);
        assert_eq!(snapshot.raw[offset + FOREGROUND_OFFSET], COLOR_TRANSPARENT);

        let second = edit_snapshot_from_keyboard_with_foreground(
            &mut snapshot,
            &mut selection,
            &mut input_field,
            text,
            4,
        );
        assert!(second.content_changed);
        assert_eq!(snapshot.raw[offset + FOREGROUND_OFFSET], 4);
    }

    #[test]
    fn cursor_selection_and_tab_field_are_independent_per_source() {
        let first_source = crate::ui4::Ui4CursorSource {
            controller_id: 1,
            slot_id: 2,
            ep_target: 3,
            hid_kind: 1,
        };
        let second_source = crate::ui4::Ui4CursorSource {
            controller_id: 1,
            slot_id: 4,
            ep_target: 5,
            hid_kind: 1,
        };
        let first_cell = GridCellSelection { column: 2, row: 3 };
        let second_cell = GridCellSelection { column: 7, row: 11 };
        let mut cursors = [
            GridCursorInputState::new(first_source),
            GridCursorInputState::new(second_source),
        ];
        cursors[0].selection = Some(first_cell);
        cursors[0].input_field = CellInputField::Upper;
        cursors[1].selection = Some(second_cell);

        assert_eq!(cursors[0].selection, Some(first_cell));
        assert_eq!(cursors[0].input_field, CellInputField::Upper);
        assert_eq!(cursors[1].selection, Some(second_cell));
        assert_eq!(cursors[1].input_field, CellInputField::Primary);
    }

    #[test]
    fn keyboard_backspace_moves_left_and_clears_the_active_field() {
        let mut snapshot = OwnedSnapshot {
            raw: Vec::from([0u8; PAGE_BYTES]),
            producer: GridPaperProducer::Blueprint(0),
            generation: 1,
            scale_percent: 100,
            size: GridSize::new(19, 13).unwrap(),
            serial: 1,
        };
        let target = GridCellSelection { column: 2, row: 3 };
        let offset = (target.row * COLUMNS + target.column) * CELL_BYTES;
        let cell = &mut snapshot.raw[offset..offset + CELL_BYTES];
        write_cell_glyph(cell, CellInputField::Primary, b"x");
        write_cell_glyph(cell, CellInputField::Upper, "²".as_bytes());

        let mut selection = GridCellSelection { column: 3, row: 3 };
        let mut input_field = CellInputField::Upper;
        let mut backspace = crate::r::keyboard::TrueosKeyboardOutputEvent::default();
        backspace.kind = crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY;
        backspace.key_code = crate::r::keyboard::KEYBOARD_KEY_BACKSPACE;

        let upper =
            edit_snapshot_from_keyboard(&mut snapshot, &mut selection, &mut input_field, backspace);
        assert!(upper.content_changed);
        assert!(upper.selection_changed);
        assert_eq!(upper.edited_cell, Some(target));
        assert_eq!(selection, target);
        let cell = &snapshot.raw[offset..offset + CELL_BYTES];
        assert_eq!(cell[PRIMARY_LENGTH_OFFSET], 1);
        assert_eq!(cell[UPPER_LENGTH_OFFSET], 0);

        let cell = &mut snapshot.raw[offset..offset + CELL_BYTES];
        write_cell_glyph(cell, CellInputField::Upper, "²".as_bytes());
        selection.column = 3;
        input_field = CellInputField::Primary;
        let primary =
            edit_snapshot_from_keyboard(&mut snapshot, &mut selection, &mut input_field, backspace);
        assert!(primary.content_changed);
        assert!(primary.selection_changed);
        assert_eq!(primary.edited_cell, Some(target));
        assert_eq!(selection, target);
        let cell = &snapshot.raw[offset..offset + CELL_BYTES];
        assert_eq!(cell[PRIMARY_LENGTH_OFFSET], 0);
        assert_eq!(cell[UPPER_LENGTH_OFFSET], 0);
        assert_eq!(validate_page(&snapshot.raw), Ok(()));
    }

    #[test]
    fn keyboard_enter_moves_to_the_first_cell_of_the_next_row() {
        let mut snapshot = OwnedSnapshot {
            raw: Vec::from([0u8; PAGE_BYTES]),
            producer: GridPaperProducer::Blueprint(0),
            generation: 1,
            scale_percent: 100,
            size: GridSize::new(19, 13).unwrap(),
            serial: 1,
        };
        let mut selection = GridCellSelection { column: 16, row: 4 };
        let mut input_field = CellInputField::Primary;
        let mut enter = crate::r::keyboard::TrueosKeyboardOutputEvent::default();
        enter.kind = crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY;
        enter.key_code = crate::r::keyboard::KEYBOARD_KEY_ENTER;

        let outcome =
            edit_snapshot_from_keyboard(&mut snapshot, &mut selection, &mut input_field, enter);
        assert!(outcome.selection_changed);
        assert_eq!(selection, GridCellSelection { column: 0, row: 5 });
    }

    #[test]
    fn pasted_cell_work_is_page_sized_and_latest_edit_wins() {
        assert!(INPUT_QUEUE_CAPACITY_PER_INSTANCE >= COLUMNS * ROWS);
        let first = GridCellSelection { column: 2, row: 3 };
        let second = GridCellSelection { column: 3, row: 3 };
        let mut dirty = VecDeque::new();
        queue_dirty_cell(&mut dirty, first);
        queue_dirty_cell(&mut dirty, second);
        queue_dirty_cell(&mut dirty, first);
        assert_eq!(dirty.len(), 2);
        assert_eq!(dirty.pop_front(), Some(second));
        assert_eq!(dirty.pop_front(), Some(first));
    }

    #[test]
    fn space_is_cell_content_without_a_font_submission() {
        let mut cell = [0u8; CELL_BYTES];
        cell[PRIMARY_LENGTH_OFFSET] = 1;
        cell[PRIMARY_OFFSET] = b' ';
        let metrics = GridSceneMetrics::new(GridSize::FULL, 100, 853, 1_196);
        let content = build_cell_scene_content(
            PRIMARY_INSTANCE_ID,
            &cell,
            metrics.cell_rect(GridCellSelection { column: 2, row: 3 }),
            metrics,
        )
        .expect("space cell is valid");
        assert!(content.texts.is_empty());
        assert_eq!(content.outline_less_glyphs, 1);
    }

    #[test]
    fn print_screen_and_f10_have_distinct_print_paths() {
        let mut event = crate::r::keyboard::TrueosKeyboardOutputEvent {
            kind: crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY,
            key_code: crate::r::keyboard::KEYBOARD_KEY_PRINT_SCREEN,
            ..Default::default()
        };
        assert!(is_gridpaper_print_key(event));

        event.key_code = crate::r::keyboard::KEYBOARD_KEY_F10;
        assert!(!is_gridpaper_print_key(event));
        assert!(is_gridpaper_printer_menu_key(event));
    }

    #[test]
    fn transient_coverage_busy_and_incomplete_pages_have_recovery_paths() {
        assert!(retryable_font_coverage_error("font-coverage-dispatch"));
        assert!(!retryable_font_coverage_error("outline-empty"));
        assert!(incomplete_pending_page_error(ServiceError::Render(
            "gridpaper-compute-text-coverage-required"
        )));
        assert!(!incomplete_pending_page_error(ServiceError::InvalidFrame));
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
        let fallback = GpuFontRgba::new(1, 2, 3, 4);
        assert_eq!(program.sample_color(fallback, 0), GpuFontRgba::new(255, 0, 0, 255));
        assert_eq!(program.sample_color(fallback, 250), GpuFontRgba::new(128, 128, 0, 255));
        assert_eq!(program.sample_color(fallback, 500), GpuFontRgba::new(0, 255, 0, 255));
        assert_eq!(program.sample_color(fallback, 1_000), GpuFontRgba::new(255, 0, 0, 255));
    }

    #[test]
    fn font_instance_v2_wire_accepts_transform_background_and_gpu_motion() {
        let mut wire = Vec::from([2, 1, 0, 0]);
        let mut record = [0u8; TEXT_INSTANCE_RECORD_HEADER_BYTES];
        record[0] = 5;
        record[12..14].copy_from_slice(&(-325i16).to_le_bytes());
        record[14..16].copy_from_slice(&1_250u16.to_le_bytes());
        record[16..18].copy_from_slice(&875u16.to_le_bytes());
        record[18..22].copy_from_slice(&[12, 24, 48, 96]);
        record[22..26].copy_from_slice(&4_000u32.to_le_bytes());
        record[26..28].copy_from_slice(&250u16.to_le_bytes());
        record[28..30].copy_from_slice(&180i16.to_le_bytes());
        record[30..32].copy_from_slice(&60i16.to_le_bytes());
        record[32..34].copy_from_slice(&(-120i16).to_le_bytes());
        record[34..36].copy_from_slice(&15i16.to_le_bytes());
        record[36..38].copy_from_slice(&(-10i16).to_le_bytes());
        wire.extend_from_slice(&record);

        let programs = decode_text_animations(&wire).expect("valid v2 font-instance wire");
        let program = programs[5].expect("selector installed");
        assert_eq!(program.color, None);
        assert_eq!(program.style.rotation_centidegrees, -325);
        assert_eq!(program.style.scale_permille, 1_250);
        assert_eq!(program.style.opacity_permille, 875);
        assert_eq!(program.style.background, GpuFontRgba::new(12, 24, 48, 96));
        assert!(program.motion.is_active());
        assert_eq!(program.motion.period_ms, 4_000);
        assert_eq!(program.motion.phase_permille, 250);
        assert_eq!(program.motion.rotation_amplitude_centidegrees, 180);
        assert_eq!(program.motion.scale_amplitude_permille, 60);
        assert_eq!(program.motion.opacity_amplitude_permille, -120);
        assert_eq!(program.motion.translation_x_tenths_px, 15);
        assert_eq!(program.motion.translation_y_tenths_px, -10);
    }
}
