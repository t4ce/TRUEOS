// Edit's fixed, independently scheduled glyph-driven Conway compute producer.
// The generated ABI and immutable executable come from the pinned bakery.
include!("../kernels/artifacts/adls/cpp/edit_life_rgba8.contract.rs");
embedded_compute_artifact!(pub(crate) EDIT_LIFE_RGBA8_ADLS_BIN, EDIT_LIFE_RGBA8_ADLS_SPV,
    EDIT_LIFE_RGBA8_ADLS_BIN_STORAGE,
    "../kernels/artifacts/adls/cpp/edit_life_rgba8.bin"
);

const EDIT_LIFE_RGBA8_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact::contracted(
    "edit_life_rgba8", EDIT_LIFE_RGBA8_ADLS_BIN, EDIT_LIFE_RGBA8_ADLS_SPV,
    &EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT,
);
// Dedicated instruction window between ParticleCraft and Helio; persistent
// font/page allocations skip the entire 0x0d00_0000..0x0e00_0000 interval.
const EDIT_LIFE_RGBA8_GPU: u64 = 0x0D90_0000;
const EDIT_LIFE_IDD: usize = 0x800;
const EDIT_LIFE_BINDING: usize = 0x840;
const EDIT_LIFE_SURFACES: usize = 0x880;
const EDIT_LIFE_PAYLOAD: usize = 0xB00;
const EDIT_LIFE_UNIFORMS: usize = 0xC40;
const EDIT_LIFE_UNIFORM_WORDS: usize = 32;
const EDIT_LIFE_UNIFORM_BYTES: usize = EDIT_LIFE_UNIFORM_WORDS * 4;
const EDIT_LIFE_BATCH_USED_BYTES: usize = (EDIT_LIFE_UNIFORMS + EDIT_LIFE_UNIFORM_BYTES + 63) & !63;
const EDIT_LIFE_RESULT_USED_BYTES: usize = 256;
const EDIT_LIFE_MAX_PAGE_PIXELS: u64 = 4096 * 2160;
const EDIT_LIFE_MAX_CANONICAL_PIXELS: u64 = 2 * 1024 * 1024;
const EDIT_LIFE_BATCH_ENTER_SLOT: usize = 40;
const EDIT_LIFE_PRE_WALKER_SLOT: usize = 42;
const EDIT_LIFE_POST_WALKER_SLOT: usize = 44;
const EDIT_LIFE_POST_RELEASE_SLOT: usize = 46;
const EDIT_LIFE_CROSS_BYTES: usize = 192;
const EDIT_LIFE_INDIRECT_BYTES: usize = 288;
const EDIT_LIFE_POST_SLOT: usize = 50;
const EDIT_LIFE_POST_MARKER: u32 = 0xC0DE_E102;
static EDIT_LIFE_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static EDIT_LIFE_PPGTT_ROOT: AtomicU64 = AtomicU64::new(0);
static EDIT_LIFE_PPGTT_ATTEMPTED: AtomicBool = AtomicBool::new(false);
// IDD thread preemption is disabled by the proven compute descriptor. Bound
// each independently retired walker so an editor page cannot monopolize RCS.
const EDIT_LIFE_SEED_MAX_LANES: u64 = 64 * 1024;
const EDIT_LIFE_STEP_MAX_LANES: u64 = 64 * 1024;
const EDIT_LIFE_COMPOSITE_MAX_LANES: u64 = 128 * 1024;

fn edit_life_dispatch_rows(mode: u32, width: u32, height: u32, first_row: u32) -> Option<u32> {
    if width == 0 || first_row >= height { return None; }
    let maximum = match mode {
        0 => EDIT_LIFE_SEED_MAX_LANES,
        1 | 3 | 4 | 5 => EDIT_LIFE_STEP_MAX_LANES,
        2 => EDIT_LIFE_COMPOSITE_MAX_LANES,
        _ => return None,
    };
    let launched_width = u64::from(width).div_ceil(16) * 16;
    let rows = maximum / launched_width;
    if rows == 0 { return None; }
    Some((height - first_row).min(rows as u32))
}

/// For n <= MAX_PAGE_PIXELS and d <= 256, n*(d-1) < 2^32.
/// Therefore mul_hi(n, ceil(2^32/d)) is exactly floor(n/d). A zero
/// reciprocal denotes d=1, which the shader handles without a multiply.
fn edit_life_reciprocal(divisor: u32) -> Option<u32> {
    if divisor == 0 || divisor > 256 { return None; }
    Some(if divisor == 1 { 0 } else { (1u64 << 32).div_ceil(u64::from(divisor)) as u32 })
}

fn edit_life_expansion_boundary(row: u32, cell_height: u32, viewport_height: u32) -> u32 {
    (u64::from(row / 11) * u64::from(cell_height)
        + (u64::from(row % 11) * u64::from(cell_height)).div_ceil(11))
        .min(u64::from(viewport_height)) as u32
}

/// A canonical row can expand into many physical rows. Bound both actual
/// writes and padded launched lanes before admitting a nonpreemptible chunk.
fn edit_life_expansion_rows(viewport: GpgpuRect, cell_height: u32,
    grid_width: u32, grid_height: u32, first_row: u32) -> Option<u32>
{
    if viewport.is_empty() || cell_height < 11 || cell_height > 256 { return None; }
    let mut high = edit_life_dispatch_rows(2, grid_width, grid_height, first_row)?;
    let start = edit_life_expansion_boundary(first_row, cell_height, viewport.height);
    let mut low = 0;
    while low < high {
        let candidate = low + (high - low).div_ceil(2);
        let end = edit_life_expansion_boundary(first_row + candidate, cell_height, viewport.height);
        if u64::from(end - start) * u64::from(viewport.width) <= EDIT_LIFE_COMPOSITE_MAX_LANES {
            low = candidate;
        } else { high = candidate - 1; }
    }
    (low != 0).then_some(low)
}

fn edit_life_chrome_rects(width: u32, height: u32, viewport: GpgpuRect) -> [GpgpuRect; 4] {
    let x = viewport.x as u32;
    let y = viewport.y as u32;
    let right = x + viewport.width;
    let bottom = y + viewport.height;
    [GpgpuRect { x: 0, y: 0, width, height: y },
        GpgpuRect { x: 0, y: bottom as i32, width, height: height - bottom },
        GpgpuRect { x: 0, y: y as i32, width: x, height: viewport.height },
        GpgpuRect { x: right as i32, y: y as i32, width: width - right, height: viewport.height }]
}

fn edit_life_canonical_geometry(viewport: GpgpuRect, cell_width: u32, cell_height: u32)
    -> Option<(u32, u32, u32, u32)>
{
    if viewport.is_empty() || cell_width < 6 || cell_height < 11
        || cell_width > 256 || cell_height > 256 { return None; }
    let columns = viewport.width.div_ceil(cell_width);
    let rows = viewport.height.div_ceil(cell_height);
    let grid_width = columns.checked_mul(6)?;
    let grid_height = rows.checked_mul(11)?;
    if u64::from(grid_width) * u64::from(grid_height) > EDIT_LIFE_MAX_CANONICAL_PIXELS
        || u64::from(grid_width).div_ceil(16) * 16 > EDIT_LIFE_SEED_MAX_LANES
        || u64::from(viewport.width) * u64::from(viewport.height.min(cell_height.div_ceil(11)))
            > EDIT_LIFE_COMPOSITE_MAX_LANES
    { return None; }
    Some((columns, rows, grid_width, grid_height))
}

fn edit_life_geometry_uniforms(viewport: GpgpuRect, grid_width: u32, grid_height: u32,
    cell_width: u32, cell_height: u32) -> Option<Vec<u32>>
{
    let (_, _, expected_width, expected_height) = edit_life_canonical_geometry(viewport, cell_width, cell_height)?;
    if (grid_width, grid_height) != (expected_width, expected_height) { return None; }
    let mut uniforms = Vec::new();
    uniforms.try_reserve_exact(EDIT_LIFE_UNIFORM_WORDS).ok()?;
    uniforms.resize(EDIT_LIFE_UNIFORM_WORDS, 0);
    uniforms[16] = edit_life_reciprocal(cell_width)?;
    uniforms[17] = edit_life_reciprocal(cell_height)?;
    uniforms[19] = ((viewport.width - 1) / cell_width) * 6
        + (((viewport.width - 1) % cell_width) * 6 / cell_width) + 1;
    uniforms[20] = ((viewport.height - 1) / cell_height) * 11
        + (((viewport.height - 1) % cell_height) * 11 / cell_height) + 1;
    Some(uniforms)
}

/// Called only while EDIT_LIFE_RCS_SUBMIT_LOCK is held. The lane has one
/// immutable physical root for its lifetime; never rebuild it between frames.
fn edit_life_prepare_ppgtt(state: DirectRcsState) -> bool {
    let root = EDIT_LIFE_PPGTT_ROOT.load(Ordering::Acquire);
    if root != 0 {
        if root == state.ppgtt_phys { return true; }
        quarantine_direct_rcs_lane(DirectRcsLane::EditLife, "edit-life-ppgtt-root-changed");
        return false;
    }
    if EDIT_LIFE_PPGTT_ATTEMPTED.swap(true, Ordering::AcqRel) { return false; }
    if !direct_rcs_init_ppgtt(state) {
        quarantine_direct_rcs_lane(DirectRcsLane::EditLife, "edit-life-ppgtt-initialization-failed");
        return false;
    }
    EDIT_LIFE_PPGTT_ROOT.store(state.ppgtt_phys, Ordering::Release);
    true
}

/// Borrowed UI4 addresses may be recycled between source/PAT0 and destination/
/// PAT3 roles. Retire those exact leaves after the final context-save proof;
/// persistent kernel/world/font leaves remain in this lane's private PPGTT.
/// The caller holds EDIT_LIFE_RCS_SUBMIT_LOCK and retains both frame leases.
fn edit_life_retire_imports(state: DirectRcsState, imports: &[GpgpuRgba8Surface]) -> bool {
    if edit_life_rcs_context_is_quarantined() || EDIT_LIFE_RCS_SUBMIT_RUNTIME.lock().pending.is_some() {
        return false;
    }
    for surface in imports {
        let Some(bytes) = surface.bytes.checked_add(4095).map(|bytes| bytes & !4095)
            .filter(|bytes| *bytes != 0) else { return false; };
        if !direct_rcs_unmap_ppgtt_region_exact(state, surface.gpu, surface.phys, bytes)
            || !direct_rcs_flush_ppgtt_pte_range(state, surface.gpu, bytes)
        { return false; }
    }
    true
}

#[derive(Clone, Copy, Default)]
struct EditLifePassTiming {
    chunks: u32,
    samples: u32,
    launched_workitems: u64,
    encode_us: u64,
    admit_cpu_us: u64,
    queue_us: u64,
    prologue_us: u64,
    walker_us: u64,
    release_us: u64,
    marker_us: u64,
    save_us: u64,
}

fn edit_life_phase_us(start: u64, end: u64, frequency_hz: u64) -> Option<u64> {
    if start == 0 || end == 0 || frequency_hz == 0 { return None; }
    if start == end { return Some(0); }
    direct_rcs_timestamp_interval_us(start, end, frequency_hz).map(|(_, us)| us)
}

const _: () = {
    assert!(matches!(EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
    assert!(EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.cross_thread_data_bytes == 192);
    assert!(EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.per_thread_data_bytes == 96);
    assert!(EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.bindings.len() == 8);
    assert!(edit_life_abi_matches());
    assert!(EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.simd_width == 16);
    assert!(EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.scratch_bytes == 0);
    assert!(EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.slm_bytes == 0);
    assert!(PARTICLE_CRAFT_ADLS_GPU + PARTICLE_CRAFT_ADLS_BIN_BYTES as u64 <= EDIT_LIFE_RGBA8_GPU);
    assert!(EDIT_LIFE_RGBA8_GPU + EDIT_LIFE_RGBA8_ADLS_BIN.len() as u64 <= HELIO_RETAINED_TRANSFORM_ADLS_GPU);
    assert!(EDIT_LIFE_SURFACES + 8 * 64 <= EDIT_LIFE_PAYLOAD);
    assert!(EDIT_LIFE_PAYLOAD + EDIT_LIFE_INDIRECT_BYTES <= EDIT_LIFE_UNIFORMS);
    assert!(EDIT_LIFE_BATCH_USED_BYTES == 0xCC0);
    assert!(EDIT_LIFE_BATCH_USED_BYTES <= DIRECT_RCS_BATCH_BYTES);
    assert!((EDIT_LIFE_POST_SLOT + 2) * 4 <= EDIT_LIFE_RESULT_USED_BYTES);
    assert!(EDIT_LIFE_MAX_PAGE_PIXELS * 255 < (1u64 << 32));
};

/// Hardware compatibility only: no upload, allocation, submission or retry.
/// A compatible native initialization failure must not become CPU fallback.
pub(crate) fn edit_life_native_supported() -> bool {
    super::claimed_device().is_some_and(|dev| edit_life_native_target_supported(dev.device_id, dev.revision_id))
}

pub(crate) const fn edit_life_native_target_supported(device_id: u16, revision_id: u8) -> bool {
    EDIT_LIFE_RGBA8_ARTIFACT.target_policy.supports(device_id, revision_id)
}

fn upload_edit_life_kernel() -> Option<UploadedKernelArtifact> {
    let mut cached = EDIT_LIFE_UPLOAD.lock();
    if let Some(upload) = *cached { return Some(upload); }
    let dev = super::claimed_device()?;
    let upload = upload_ppgtt_resident_artifact(dev, EDIT_LIFE_RGBA8_ARTIFACT, EDIT_LIFE_RGBA8_GPU)?;
    crate::log_info!(target: "ui4/edit-life";
        "edit-life: artifact lane=edit-life bin_sha256={} entry_bytes={} simd={} grfs={} scratch_bytes={} slm_bytes={}\n",
        digest_hex(&upload.bin_sha256), EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.entry_size,
        EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.simd_width, EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.grf_count,
        EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.scratch_bytes, EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.slm_bytes);
    *cached = Some(upload);
    Some(upload)
}

/// Compact glyph and color inputs belong to this effect task. The source frame
/// is borrowed only during a synchronous render; its caller keeps the exact
/// published front pinned through retirement (or quarantines that read lease).
/// Generations reuse the uploaded cells and baked glyph masks.
pub(crate) struct EditLifeState {
    width: u32,
    height: u32,
    viewport: GpgpuRect,
    grid_width: u32,
    grid_height: u32,
    cell_width: u32,
    cell_height: u32,
    columns: u32,
    rows: u32,
    cells: [GpgpuOwnedRgba8Surface; 2],
    glyphs: GpgpuOwnedRgba8Surface,
    previous: GpgpuOwnedRgba8Surface,
    font: GpgpuOwnedRgba8Surface,
    canonical: GpgpuOwnedRgba8Surface,
    geometry_uniforms: Vec<u32>,
    canonical_valid: bool,
    read: usize,
    uploaded: bool,
    pending_snapshot: bool,
    initialized: bool,
    quarantined: bool,
    timing_kinds: u8,
    renders: u64,
    slow_renders: u64,
}

impl EditLifeState {
    pub(crate) fn new(width: u32, height: u32, viewport: GpgpuRect,
        cell_width: u32, cell_height: u32) -> Option<Self>
    {
        if width == 0 || height == 0 || cell_width == 0 || cell_height == 0
            || cell_width > 256 || cell_height > 256
            || u64::from(width) * u64::from(height) > EDIT_LIFE_MAX_PAGE_PIXELS
            || u64::from(width).div_ceil(16) * 16 > EDIT_LIFE_COMPOSITE_MAX_LANES
            || viewport.x < 0 || viewport.y < 0
            || viewport.is_empty()
            || (viewport.x as u32).checked_add(viewport.width).is_none_or(|right| right > width)
            || (viewport.y as u32).checked_add(viewport.height).is_none_or(|bottom| bottom > height)
            || edit_life_rcs_context_is_quarantined()
        { return None; }
        let (columns, rows, grid_width, grid_height) = edit_life_canonical_geometry(viewport, cell_width, cell_height)?;
        upload_edit_life_kernel()?;
        let cell_words = grid_width.checked_mul(grid_height)?.checked_mul(2)?;
        let geometry_uniforms = edit_life_geometry_uniforms(viewport, grid_width, grid_height, cell_width, cell_height)?;
        let glyph_count = width.div_ceil(cell_width).checked_mul(height.div_ceil(cell_height))?;
        let font_masks = &crate::r::services::shell3_font_service::hex::MASKS;
        let font = edit_life_allocation(u32::try_from(font_masks.len()).ok()?.checked_mul(4)?, 1)?;
        unsafe { core::ptr::copy_nonoverlapping(font_masks.as_ptr().cast::<u8>(), font.virt,
            core::mem::size_of_val(font_masks)); }
        super::dma_flush(font.virt, core::mem::size_of_val(font_masks));
        Some(Self {
            width, height, viewport, grid_width, grid_height, cell_width, cell_height,
            columns, rows,
            // A one-row allocation keeps uint2 cells tightly packed without
            // inserting the RGBA helper's 64-byte padding between cell rows.
            cells: [edit_life_allocation(cell_words, 1)?, edit_life_allocation(cell_words, 1)?],
            // uint4 terminal cells and uint2 previous glyph/flags pairs. Full
            // terminal capacity lets gutter shifts reuse the same inputs.
            glyphs: edit_life_allocation(glyph_count.checked_mul(4)?, 1)?,
            previous: edit_life_allocation(glyph_count.checked_mul(2)?, 1)?,
            font, canonical: edit_life_allocation(grid_width, grid_height)?, geometry_uniforms,
            canonical_valid: false,
            read: 0, uploaded: false, pending_snapshot: false,
            initialized: false, quarantined: false,
            timing_kinds: 0, renders: 0, slow_renders: 0,
        })
    }

    pub(crate) fn extent(&self) -> (u32, u32) { (self.width, self.height) }
    pub(crate) fn viewport(&self) -> GpgpuRect { self.viewport }

    /// Gutter changes often move or slightly shrink the editor viewport while
    /// its page extent stays fixed. Reuse the retained storage when it fits;
    /// the next seed overwrites both the world and its comparison baseline.
    pub(crate) fn set_viewport(&mut self, viewport: GpgpuRect) -> bool {
        if self.quarantined || edit_life_rcs_context_is_quarantined()
            || viewport.x < 0 || viewport.y < 0 || viewport.is_empty()
            || (viewport.x as u32).checked_add(viewport.width).is_none_or(|right| right > self.width)
            || (viewport.y as u32).checked_add(viewport.height).is_none_or(|bottom| bottom > self.height)
        { return false; }
        if viewport == self.viewport { return true; }
        let Some((columns, rows, grid_width, grid_height)) =
            edit_life_canonical_geometry(viewport, self.cell_width, self.cell_height)
            else { return false; };
        let Some(cell_bytes) = (grid_width as usize).checked_mul(grid_height as usize)
            .and_then(|cells| cells.checked_mul(8)) else { return false; };
        let Some(glyph_bytes) = (columns as usize).checked_mul(rows as usize)
            .and_then(|cells| cells.checked_mul(8)) else { return false; };
        if self.cells.iter().any(|cells| cells.surface().bytes < cell_bytes)
            || self.previous.surface().bytes < glyph_bytes
            || self.glyphs.surface().bytes < glyph_bytes.saturating_mul(2)
            || grid_width > self.canonical.surface().width
            || grid_height > self.canonical.surface().height
        { return false; }
        let Some(geometry_uniforms) = edit_life_geometry_uniforms(viewport, grid_width, grid_height,
            self.cell_width, self.cell_height) else { return false; };
        self.geometry_uniforms = geometry_uniforms;
        self.viewport = viewport;
        self.grid_width = grid_width;
        self.grid_height = grid_height;
        self.columns = columns;
        self.rows = rows;
        self.initialized = false;
        self.canonical_valid = false;
        self.pending_snapshot = true;
        true
    }

    /// Transfer one compact terminal cell, not expanded glyph pixels. The shared
    /// baked 6x11 hex masks are resolved by the kernel for seeding and drawing.
    pub(crate) fn upload(&mut self, cells: &[[u32; 4]]) -> bool {
        let Some(count) = (self.columns as usize).checked_mul(self.rows as usize) else { return false; };
        if self.quarantined || edit_life_rcs_context_is_quarantined() || cells.len() != count
            || core::mem::size_of_val(cells) > self.glyphs.surface().bytes
            || count.saturating_mul(8) > self.previous.surface().bytes
        { return false; }
        unsafe {
            core::ptr::copy_nonoverlapping(cells.as_ptr().cast::<u8>(), self.glyphs.virt,
                core::mem::size_of_val(cells));
        }
        super::dma_flush(self.glyphs.virt, core::mem::size_of_val(cells));
        self.uploaded = true;
        self.pending_snapshot = true;
        self.canonical_valid = false;
        true
    }

    /// `steps` is a bounded clock debt, normally zero or one at speed 1/10 Hz.
    /// A fresh snapshot always advances its comparison baseline; `emit` only
    /// chooses whether positive new glyph ink joins the current world.
    pub(crate) fn render(&mut self, dst: GpgpuRgba8Surface, source: GpgpuRgba8Surface,
        reset: bool, emit: bool, steps: u32)
        -> GpgpuRgba8KernelResult
    {
        if self.quarantined || !self.uploaded || !dst.is_valid()
            || dst.storage_order != GpgpuRgba8StorageOrder::Rgba || !source.is_valid()
            || source.storage_order != GpgpuRgba8StorageOrder::Rgba
            || (source.width, source.height) != self.extent()
            || gpu_ranges_overlap(source.gpu, source.bytes, dst.gpu, dst.bytes)
            || gpu_ranges_overlap(source.phys, source.bytes, dst.phys, dst.bytes)
            || (dst.width, dst.height) != self.extent() || steps > 5
            || edit_life_rcs_context_is_quarantined()
        { return GpgpuRgba8KernelResult::default(); }
        let started = direct_rcs_now_tick();
        let _guard = EDIT_LIFE_RCS_SUBMIT_LOCK.lock();
        let lock_ms = direct_rcs_elapsed_ms_since(started);
        let setup_started = direct_rcs_now_tick();
        let Some(dev) = super::claimed_device() else { return GpgpuRgba8KernelResult::default(); };
        let Some(upload) = upload_edit_life_kernel() else { return GpgpuRgba8KernelResult::default(); };
        let Some(state) = edit_life_rcs_state_once(dev) else { return GpgpuRgba8KernelResult::default(); };
        // Prepare the dedicated editor PPGTT once per render. Keep mappings
        // stable across all bounded walkers; each chunk retires completely
        // before the shared batch/payload is rewritten for the next one.
        let mut prepared = !edit_life_rcs_context_is_quarantined()
            && direct_rcs_forcewake(dev) && direct_rcs_map_state(dev, state)
            && edit_life_prepare_ppgtt(state)
            && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
        for resource in [self.cells[0].surface(), self.cells[1].surface(),
            self.glyphs.surface(), self.previous.surface(), self.font.surface(), self.canonical.surface()]
        {
            prepared = prepared && direct_rcs_map_ppgtt_kernel(state, resource.gpu, resource.phys, resource.bytes);
        }
        let source_mapped = prepared
            && direct_rcs_map_ppgtt_kernel(state, source.gpu, source.phys, source.bytes);
        prepared = source_mapped
            && direct_rcs_map_ppgtt_destination(state, dst.gpu, dst.phys, dst.bytes, true);
        if !prepared {
            // A destination publication failure can follow its PTE writes.
            // Exact retirement also accepts absent leaves, so clean both
            // imports while their leases are still held and no job was sent.
            if source_mapped && !edit_life_retire_imports(state, &[source, dst]) {
                quarantine_direct_rcs_lane(DirectRcsLane::EditLife, "edit-life-source-import-retirement-failed");
            }
            return GpgpuRgba8KernelResult::default();
        }
        let setup_ms = direct_rcs_elapsed_ms_since(setup_started);
        let (kind, kind_bit) = if !self.initialized && self.renders == 0 { ("startup", 1) }
            else if !self.initialized { ("viewport-reset", 16) }
            else if reset { ("scroll-reset", 2) }
            else if self.pending_snapshot { ("page-update", 4) }
            else { ("generation", 8) };
        let dispatch_started = direct_rcs_now_tick();
        let mut submitted = false;
        let mut marker = 0;
        let mut passes = 0u32;
        let mut pass_ms = [0u64; 6];
        // Sample startup/input/reset and occasional steady frames. Production
        // frames otherwise keep the ordinary release packets unchanged.
        let probe = self.timing_kinds & kind_bit == 0 || self.renders < 2 || self.renders % 120 == 0;
        let mut pass_timing = [EditLifePassTiming::default(); 6];
        let mut run = |this: &mut Self, mode: u32, reseed: bool, rect: GpgpuRect| {
            let pass_started = direct_rcs_now_tick();
            let outcome = this.dispatch(dev, state, upload, dst, source, mode, reseed, rect,
                if probe { Some(&mut pass_timing[mode as usize]) } else { None });
            passes += 1;
            pass_ms[mode as usize] += direct_rcs_elapsed_ms_since(pass_started);
            submitted |= outcome.submitted;
            marker = outcome.observed;
            outcome.observed == EDIT_LIFE_POST_MARKER
        };
        let reseed = reset || !self.initialized;
        let body = GpgpuRect::default();
        let mut ok = true;
        if self.pending_snapshot || reseed {
            if reseed || emit {
                ok = run(self, 0, reseed, body);
                if ok { self.read = 1 - self.read; self.canonical_valid = false; }
            }
            if ok { ok = run(self, 3, false, body); }
            if ok { self.pending_snapshot = false; self.initialized = true; }
        }
        for _ in 0..steps {
            if !ok { break; }
            ok = run(self, 1, false, body);
            if ok { self.read = 1 - self.read; self.canonical_valid = false; }
        }
        if ok && !self.canonical_valid {
            ok = run(self, 4, false, body);
            if ok { self.canonical_valid = true; }
        }
        if ok { ok = run(self, 2, false, body); }
        for rect in edit_life_chrome_rects(self.width, self.height, self.viewport) {
            if !ok { break; }
            if !rect.is_empty() { ok = run(self, 5, false, rect); }
        }
        if ok || !submitted {
            if !edit_life_retire_imports(state, &[source, dst]) {
                quarantine_direct_rcs_lane(DirectRcsLane::EditLife, "edit-life-import-retirement-failed");
                ok = false;
            }
        }
        if !ok && submitted {
            // A later chunk can reject after earlier chunks were admitted.
            // Keep all imported/owned backing fenced until reboot rather than
            // leaving a still-admissible lane with retained borrowed aliases.
            quarantine_direct_rcs_lane(DirectRcsLane::EditLife, "edit-life-render-incomplete");
            self.quarantine();
        }
        let submit_ms = direct_rcs_elapsed_ms_since(dispatch_started);
        let total_ms = direct_rcs_elapsed_ms_since(started);
        self.renders = self.renders.saturating_add(1);
        if total_ms >= 100 { self.slow_renders = self.slow_renders.saturating_add(1); }
        // First timings distinguish startup/reset/input from steady animation.
        // Power-of-two overrun reporting keeps a slow rig's log bounded.
        if self.timing_kinds & kind_bit == 0 || !ok
            || (total_ms >= 100 && self.slow_renders.is_power_of_two())
            || self.renders % 600 == 0
        {
            self.timing_kinds |= kind_bit;
            crate::log_info!(target: "ui4/edit-life";
                "intel/gpgpu: edit-life timing kind={} ok={} extent={}x{} grid={}x{} steps={} passes={} ppgtt_prepares=1 lock_ms={} setup_ms={} seed_ms={} baseline_ms={} step_ms={} resolve_ms={} upscale_ms={} chrome_ms={} submit_ms={} total_ms={} renders={} slow_renders={}\n",
                kind, ok as u8, self.width, self.height, self.grid_width, self.grid_height,
                steps, passes, lock_ms, setup_ms, pass_ms[0], pass_ms[3], pass_ms[1], pass_ms[4], pass_ms[2], pass_ms[5],
                submit_ms, total_ms, self.renders, self.slow_renders);
        }
        if probe {
            for (mode, timing) in pass_timing.iter().enumerate().filter(|(_, timing)| timing.chunks != 0) {
                crate::log_info!(target: "ui4/edit-life";
                    "edit-life: GPU probe kind={} mode={} chunks={} samples={} launched_workitems={} encode_cpu_us={} admit_cpu_us={} submit_to_batch_us={} prologue_us={} walker_us={} release_us={} release_to_marker_us={} marker_to_saved_us={} timestamp_hz={} batch_bytes={} result_bytes={} ppgtt=retained\n",
                    kind, mode, timing.chunks, timing.samples, timing.launched_workitems, timing.encode_us, timing.admit_cpu_us,
                    timing.queue_us, timing.prologue_us, timing.walker_us, timing.release_us,
                    timing.marker_us, timing.save_us, direct_rcs_timestamp_frequency_hz(dev),
                    EDIT_LIFE_BATCH_USED_BYTES, EDIT_LIFE_RESULT_USED_BYTES);
            }
        }
        GpgpuRgba8KernelResult { ok, submitted, marker,
            submit_ms: total_ms,
            release: ok.then(|| gpgpu_rgba8_release(dst)) }
    }

    fn quarantine(&mut self) {
        self.quarantined = true;
        for resource in [&self.cells[0], &self.cells[1], &self.glyphs, &self.previous, &self.font, &self.canonical] {
            resource.quarantine_backing();
        }
    }

    fn dispatch(&self, dev: super::Dev, state: DirectRcsState, upload: UploadedKernelArtifact,
        dst: GpgpuRgba8Surface, source: GpgpuRgba8Surface, mode: u32, reset: bool,
        rect: GpgpuRect, mut timing: Option<&mut EditLifePassTiming>) -> DirectRcsDispatchOutcome
    {
        let (width, height) = match mode {
            0 | 1 | 2 | 4 => (self.grid_width, self.grid_height),
            3 => (self.columns, self.rows),
            5 => (rect.width.div_ceil(2), rect.height),
            _ => return DirectRcsDispatchOutcome::default(),
        };
        let mut submitted = false;
        let mut first_row = 0;
        while first_row < height {
            let rows = if mode == 2 {
                edit_life_expansion_rows(self.viewport, self.cell_height, width, height, first_row)
            } else { edit_life_dispatch_rows(mode, width, height, first_row) };
            let Some(rows) = rows
                else { return DirectRcsDispatchOutcome { submitted, observed: 0 }; };
            let probe = timing.is_some();
            let encode_started = if probe { direct_rcs_now_tick() } else { 0 };
            let encoded = encode_edit_life_batch(state, upload, self, dst, source,
                mode, reset, rect, width, first_row, rows, probe);
            if let Some(timing) = timing.as_deref_mut() {
                timing.chunks += 1;
                timing.encode_us += direct_rcs_elapsed_us_since(encode_started);
            }
            let host_submit_gpu = if probe { direct_rcs_read_render_timestamp(dev) } else { 0 };
            let admit_started = if probe { direct_rcs_now_tick() } else { 0 };
            let admission = if encoded {
                direct_rcs_submit_batch_on_lane_state(dev, state, DirectRcsLane::EditLife)
            } else { DirectRcsSubmissionState::Rejected };
            if let Some(timing) = timing.as_deref_mut() {
                timing.admit_cpu_us += direct_rcs_elapsed_us_since(admit_started);
                if admission.may_have_submitted() {
                    timing.launched_workitems += u64::from(width).div_ceil(16) * 16 * u64::from(rows);
                }
            }
            submitted |= admission.may_have_submitted();
            let (observed, marker_observed_gpu) = if admission.can_poll() && probe {
                direct_rcs_poll_result_slot_timeout_ms_on_lane_with_timestamp(state, EDIT_LIFE_POST_SLOT,
                    EDIT_LIFE_POST_MARKER, UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
                    DirectRcsLane::EditLife, Some(dev))
            } else if admission.can_poll() {
                (direct_rcs_poll_result_slot_timeout_ms_on_lane(state, EDIT_LIFE_POST_SLOT,
                    EDIT_LIFE_POST_MARKER, UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS, DirectRcsLane::EditLife), 0)
            } else { (0, 0) };
            if observed != EDIT_LIFE_POST_MARKER {
                if admission.may_have_submitted() {
                    quarantine_direct_rcs_lane(DirectRcsLane::EditLife, "edit-life-marker-timeout");
                }
                crate::log_error!(target: "gpgpu";
                    "intel/gpgpu: edit-life failed lane=edit-life mode={} encoded={} submitted={} marker=0x{:08X} width={} first_row={} rows={} height={} action=stop-effect-no-retry\n",
                    mode, encoded as u8, submitted as u8, observed, width, first_row, rows, height);
                return DirectRcsDispatchOutcome { submitted, observed };
            }
            if let Some(timing) = timing.as_deref_mut() {
                let saved_gpu = direct_rcs_read_render_timestamp(dev);
                let enter = direct_rcs_read_result_qword(state, EDIT_LIFE_BATCH_ENTER_SLOT);
                let pre = direct_rcs_read_result_qword(state, EDIT_LIFE_PRE_WALKER_SLOT);
                let post = direct_rcs_read_result_qword(state, EDIT_LIFE_POST_WALKER_SLOT);
                let release = direct_rcs_read_result_qword(state, EDIT_LIFE_POST_RELEASE_SLOT);
                let hz = u64::from(direct_rcs_timestamp_frequency_hz(dev));
                if let (Some(queue), Some(prologue), Some(walker), Some(drain), Some(marker), Some(save)) = (
                    edit_life_phase_us(host_submit_gpu, enter, hz), edit_life_phase_us(enter, pre, hz),
                    edit_life_phase_us(pre, post, hz), edit_life_phase_us(post, release, hz),
                    edit_life_phase_us(release, marker_observed_gpu, hz),
                    edit_life_phase_us(marker_observed_gpu, saved_gpu, hz))
                {
                    timing.samples += 1;
                    timing.queue_us += queue;
                    timing.prologue_us += prologue;
                    timing.walker_us += walker;
                    timing.release_us += drain;
                    timing.marker_us += marker;
                    timing.save_us += save;
                }
            }
            // The poll requires marker + saved HEAD at the published TAIL.
            // It is now safe to rewrite this lane's batch and payload. The
            // caller flips world buffers only when the complete pass returns.
            first_row += rows;
        }
        DirectRcsDispatchOutcome { submitted, observed: EDIT_LIFE_POST_MARKER }
    }
}

fn edit_life_allocation(width: u32, height: u32) -> Option<GpgpuOwnedRgba8Surface> {
    let mut allocation = allocate_font_instance_rgba8_surface(width, height)?;
    allocation.owner = Rgba8SurfaceOwner::EditLife;
    Some(allocation)
}

// Keep the encoder's exact payload layout a compile-time contract. A fresh
// bake that changes implicit arguments must fail the build, not reject every
// render silently at runtime.
const fn edit_life_abi_matches() -> bool {
    let contract = &EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT;
    if contract.payload_args.len() != 13 || contract.bindings.len() != 8 { return false; }
    let mut index = 0;
    while index < 8 {
        let arg = &contract.payload_args[index];
        if arg.arg_index as usize != index || !matches!(arg.kind, GpgpuArtifactArgKind::ByPointer)
            || arg.offset_bytes as usize != 48 + index * 8 || arg.size_bytes != 8
            || contract.bindings[index].arg_index as usize != index
            || contract.bindings[index].bti as usize != index { return false; }
        index += 1;
    }
    while index < 13 {
        let arg = &contract.payload_args[index];
        if arg.arg_index as usize != index || !matches!(arg.kind, GpgpuArtifactArgKind::ByValue)
            || arg.offset_bytes as usize != 112 + (index - 8) * 16 || arg.size_bytes != 16
        { return false; }
        index += 1;
    }
    // This kernel has no implicit buffer offset. The three generated launch
    // fields are written by the encoder below.
    let implicit = contract.implicit_payload_args;
    implicit.len() == 3
        && matches!(implicit[0].kind, GpgpuArtifactImplicitArgKind::GlobalIdOffset)
        && implicit[0].offset_bytes == 0 && implicit[0].size_bytes == 12
        && matches!(implicit[1].kind, GpgpuArtifactImplicitArgKind::LocalSize)
        && implicit[1].offset_bytes == 12 && implicit[1].size_bytes == 12
        && matches!(implicit[2].kind, GpgpuArtifactImplicitArgKind::EnqueuedLocalSize)
        && implicit[2].offset_bytes == 32 && implicit[2].size_bytes == 12
}

fn encode_edit_life_batch(state: DirectRcsState, upload: UploadedKernelArtifact,
    life: &EditLifeState, dst: GpgpuRgba8Surface, source: GpgpuRgba8Surface, mode: u32, reset: bool,
    rect: GpgpuRect, width: u32, first_row: u32, rows: u32, probe: bool) -> bool
{
    if !edit_life_abi_matches() || upload.bin_sha256 != EDIT_LIFE_RGBA8_ARTIFACT.bin_sha256
        || upload.gpu != EDIT_LIFE_RGBA8_GPU || upload.bytes != EDIT_LIFE_RGBA8_ADLS_BIN.len()
    { return false; }
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, EDIT_LIFE_BATCH_USED_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, EDIT_LIFE_RESULT_USED_BYTES);
    }
    let uniforms_gpu = state.gpu_va.batch + EDIT_LIFE_UNIFORMS as u64;
    let font_or_canonical = if mode == 2 { life.canonical.surface() } else { life.font.surface() };
    let output = if mode == 4 { life.canonical.surface() } else { dst };
    let resources = [life.cells[life.read].surface(), life.cells[1 - life.read].surface(),
        life.glyphs.surface(), life.previous.surface(), font_or_canonical, source, output];
    let mut buffers = [(0u64, 0usize); 8];
    for (index, resource) in resources.into_iter().enumerate() { buffers[index] = (resource.gpu, resource.bytes); }
    buffers[7] = (uniforms_gpu, EDIT_LIFE_UNIFORM_BYTES);
    if !direct_rcs_write_interface_descriptor_at(state, EDIT_LIFE_IDD, EDIT_LIFE_BINDING,
        EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.entry_offset, 8, (EDIT_LIFE_CROSS_BYTES / 32) as u32)
    { return false; }
    for (index, (gpu, bytes)) in buffers.iter().copied().enumerate() {
        let surface_offset = EDIT_LIFE_SURFACES + index * 64;
        unsafe { core::ptr::write_volatile((state.batch_virt.add(EDIT_LIFE_BINDING) as *mut u32).add(index), surface_offset as u32); }
        if !direct_rcs_write_buffer_surface_state(state, surface_offset, gpu, bytes) { return false; }
    }
    unsafe {
        let payload = state.batch_virt.add(EDIT_LIFE_PAYLOAD) as *mut u32;
        core::ptr::write_volatile(payload.add(1), first_row);
        for (index, value) in [(3, 16), (4, 1), (5, 1), (8, 16), (9, 1), (10, 1)] {
            core::ptr::write_volatile(payload.add(index), value);
        }
        for (index, (gpu, _)) in buffers.into_iter().enumerate() {
            core::ptr::write_volatile(payload.add(12 + index * 2), gpu as u32);
            core::ptr::write_volatile(payload.add(13 + index * 2), (gpu >> 32) as u32);
        }
        let display = [life.width, life.height, life.viewport.x as u32, life.viewport.y as u32,
            life.viewport.width, life.viewport.height, life.cell_width, life.cell_height,
            life.canonical.surface().pitch_bytes, life.grid_width, life.grid_height, mode,
            rect.x as u32, rect.y as u32, rect.width, rect.height,
            source.pitch_bytes, dst.pitch_bytes, 0, 0];
        core::ptr::copy_nonoverlapping(display.as_ptr(), payload.add(28), display.len());
        let local_ids = state.batch_virt.add(EDIT_LIFE_PAYLOAD + EDIT_LIFE_CROSS_BYTES) as *mut u16;
        for lane in 0..16 { core::ptr::write_volatile(local_ids.add(lane), lane as u16); }
        core::ptr::copy_nonoverlapping(life.geometry_uniforms.as_ptr(),
            state.batch_virt.add(EDIT_LIFE_UNIFORMS).cast::<u32>(), EDIT_LIFE_UNIFORM_WORDS);
        let uniforms = [mode, reset as u32, life.width, life.height,
            life.viewport.x as u32, life.viewport.y as u32, life.viewport.width, life.viewport.height,
            life.grid_width, life.grid_height, life.columns, life.rows,
            source.pitch_bytes, output.pitch_bytes, life.cell_width, life.cell_height];
        core::ptr::copy_nonoverlapping(uniforms.as_ptr(), state.batch_virt.add(EDIT_LIFE_UNIFORMS) as *mut u32, uniforms.len());
        core::ptr::write_volatile((state.batch_virt.add(EDIT_LIFE_UNIFORMS) as *mut u32).add(18),
            life.canonical.surface().pitch_bytes);
    }
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, EDIT_LIFE_BATCH_USED_BYTES / 4) };
    let mut cursor = 0;
    let mut ok = !probe || direct_rcs_push_pipe_control_timestamp_at(batch, &mut cursor,
        state.gpu_va.result, EDIT_LIFE_BATCH_ENTER_SLOT);
    ok &= direct_rcs_push_gpgpu_prologue_with_instruction_base(batch, &mut cursor,
        state.gpu_va.batch, upload.gpu, GPGPU_VFE_DW5_UOS);
    for value in [MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD, 0, 32, EDIT_LIFE_IDD as u32]
    { ok &= direct_rcs_push(batch, &mut cursor, value); }
    if probe { ok &= direct_rcs_push_pipe_control_timestamp_at(batch, &mut cursor,
        state.gpu_va.result, EDIT_LIFE_PRE_WALKER_SLOT); }
    ok &= direct_rcs_push_gpgpu_walker_2d(batch, &mut cursor, EDIT_LIFE_PAYLOAD,
        EDIT_LIFE_INDIRECT_BYTES, width.div_ceil(16), rows, GPGPU_WALKER_SIMD16_MASK);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    if probe {
        ok &= direct_rcs_push_pipe_control_timestamp_at(batch, &mut cursor,
            state.gpu_va.result, EDIT_LIFE_POST_WALKER_SLOT);
        ok &= direct_rcs_push_gpgpu_dispatch_timestamped_epilogue(batch, &mut cursor,
            state.gpu_va.result, EDIT_LIFE_POST_RELEASE_SLOT, EDIT_LIFE_POST_SLOT, EDIT_LIFE_POST_MARKER);
    } else {
        ok &= direct_rcs_push_gpgpu_dispatch_epilogue(batch, &mut cursor, state.gpu_va.result,
            EDIT_LIFE_POST_SLOT, EDIT_LIFE_POST_MARKER);
    }
    if ok {
        if cursor * 4 > EDIT_LIFE_IDD { return false; }
        super::dma_flush(state.batch_virt, EDIT_LIFE_BATCH_USED_BYTES);
        super::dma_flush(state.result_virt, EDIT_LIFE_RESULT_USED_BYTES);
    }
    ok
}
