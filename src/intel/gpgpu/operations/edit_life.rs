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
const EDIT_LIFE_UNIFORMS: usize = 0xC00;
const EDIT_LIFE_CROSS_BYTES: usize = 128;
const EDIT_LIFE_INDIRECT_BYTES: usize = 256;
const EDIT_LIFE_POST_SLOT: usize = 50;
const EDIT_LIFE_POST_MARKER: u32 = 0xC0DE_E102;
static EDIT_LIFE_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);

const _: () = {
    assert!(matches!(EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
    assert!(EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.cross_thread_data_bytes == 128);
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
};

fn upload_edit_life_kernel() -> Option<UploadedKernelArtifact> {
    let mut cached = EDIT_LIFE_UPLOAD.lock();
    if let Some(upload) = *cached { return Some(upload); }
    let dev = super::claimed_device()?;
    let upload = upload_ppgtt_resident_artifact(dev, EDIT_LIFE_RGBA8_ARTIFACT, EDIT_LIFE_RGBA8_GPU)?;
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
        if width == 0 || height == 0 || cell_width == 0 || cell_height == 0 || viewport.x < 0 || viewport.y < 0
            || viewport.is_empty()
            || (viewport.x as u32).checked_add(viewport.width).is_none_or(|right| right > width)
            || (viewport.y as u32).checked_add(viewport.height).is_none_or(|bottom| bottom > height)
            || direct_rcs_context_is_quarantined()
        { return None; }
        upload_edit_life_kernel()?;
        let grid_width = viewport.width.div_ceil(3).min(512);
        let grid_height = viewport.height.div_ceil(3).min(512);
        let cell_words = grid_width.checked_mul(grid_height)?.checked_mul(2)?;
        let glyph_count = width.div_ceil(cell_width).checked_mul(height.div_ceil(cell_height))?;
        let font_masks = &crate::r::services::shell3_font_service::hex::MASKS;
        let font = edit_life_allocation(u32::try_from(font_masks.len()).ok()?.checked_mul(4)?, 1)?;
        unsafe { core::ptr::copy_nonoverlapping(font_masks.as_ptr().cast::<u8>(), font.virt,
            core::mem::size_of_val(font_masks)); }
        super::dma_flush(font.virt, core::mem::size_of_val(font_masks));
        Some(Self {
            width, height, viewport, grid_width, grid_height, cell_width, cell_height,
            columns: viewport.width.div_ceil(cell_width), rows: viewport.height.div_ceil(cell_height),
            // A one-row allocation keeps uint2 cells tightly packed without
            // inserting the RGBA helper's 64-byte padding between cell rows.
            cells: [edit_life_allocation(cell_words, 1)?, edit_life_allocation(cell_words, 1)?],
            // uint4 terminal cells and uint2 previous glyph/flags pairs. Full
            // terminal capacity lets gutter shifts reuse the same inputs.
            glyphs: edit_life_allocation(glyph_count.checked_mul(4)?, 1)?,
            previous: edit_life_allocation(glyph_count.checked_mul(2)?, 1)?,
            font,
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
        if self.quarantined || direct_rcs_context_is_quarantined()
            || viewport.x < 0 || viewport.y < 0 || viewport.is_empty()
            || (viewport.x as u32).checked_add(viewport.width).is_none_or(|right| right > self.width)
            || (viewport.y as u32).checked_add(viewport.height).is_none_or(|bottom| bottom > self.height)
        { return false; }
        if viewport == self.viewport { return true; }
        let grid_width = viewport.width.div_ceil(3).min(512);
        let grid_height = viewport.height.div_ceil(3).min(512);
        let Some(cell_bytes) = (grid_width as usize).checked_mul(grid_height as usize)
            .and_then(|cells| cells.checked_mul(8)) else { return false; };
        let columns = viewport.width.div_ceil(self.cell_width);
        let rows = viewport.height.div_ceil(self.cell_height);
        let Some(glyph_bytes) = (columns as usize).checked_mul(rows as usize)
            .and_then(|cells| cells.checked_mul(8)) else { return false; };
        if self.cells.iter().any(|cells| cells.surface().bytes < cell_bytes)
            || self.previous.surface().bytes < glyph_bytes
            || self.glyphs.surface().bytes < glyph_bytes.saturating_mul(2)
        { return false; }
        self.viewport = viewport;
        self.grid_width = grid_width;
        self.grid_height = grid_height;
        self.columns = columns;
        self.rows = rows;
        self.initialized = false;
        self.pending_snapshot = true;
        true
    }

    /// Transfer one compact terminal cell, not expanded glyph pixels. The shared
    /// baked 6x11 hex masks are resolved by the kernel for seeding and drawing.
    pub(crate) fn upload(&mut self, cells: &[[u32; 4]]) -> bool {
        let Some(count) = (self.columns as usize).checked_mul(self.rows as usize) else { return false; };
        if self.quarantined || direct_rcs_context_is_quarantined() || cells.len() != count
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
            || direct_rcs_context_is_quarantined()
        { return GpgpuRgba8KernelResult::default(); }
        let started = direct_rcs_now_tick();
        let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
        let lock_ms = direct_rcs_elapsed_ms_since(started);
        let setup_started = direct_rcs_now_tick();
        let Some(dev) = super::claimed_device() else { return GpgpuRgba8KernelResult::default(); };
        let Some(upload) = upload_edit_life_kernel() else { return GpgpuRgba8KernelResult::default(); };
        let Some(state) = direct_rcs_state_once(dev) else { return GpgpuRgba8KernelResult::default(); };
        // The system-service lane rebuilds its entire PPGTT on init. Prepare
        // this retained page once while the submission lock keeps mappings
        // stable for every pass; rebuilding it for every row chunk made a
        // reset flush dozens of whole page tables before it became visible.
        let mut prepared = !direct_rcs_context_is_quarantined()
            && direct_rcs_forcewake(dev) && direct_rcs_map_state(dev, state)
            && direct_rcs_init_ppgtt(state)
            && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
        for resource in [self.cells[0].surface(), self.cells[1].surface(),
            self.glyphs.surface(), self.previous.surface(), self.font.surface(), source]
        {
            prepared = prepared && direct_rcs_map_ppgtt_kernel(state, resource.gpu, resource.phys, resource.bytes);
        }
        prepared = prepared && direct_rcs_map_ppgtt_destination(state, dst.gpu, dst.phys, dst.bytes, true);
        if !prepared { return GpgpuRgba8KernelResult::default(); }
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
        let mut pass_ms = [0u64; 4];
        let mut run = |this: &mut Self, mode: u32, reseed: bool| {
            let pass_started = direct_rcs_now_tick();
            let outcome = this.dispatch(dev, state, upload, dst, source, mode, reseed);
            passes += 1;
            pass_ms[mode as usize] += direct_rcs_elapsed_ms_since(pass_started);
            submitted |= outcome.submitted;
            marker = outcome.observed;
            outcome.observed == EDIT_LIFE_POST_MARKER
        };
        let reseed = reset || !self.initialized;
        let mut ok = true;
        if self.pending_snapshot || reseed {
            if reseed || emit {
                ok = run(self, 0, reseed);
                if ok { self.read = 1 - self.read; }
            }
            if ok { ok = run(self, 3, false); }
            if ok { self.pending_snapshot = false; self.initialized = true; }
        }
        for _ in 0..steps {
            if !ok { break; }
            ok = run(self, 1, false);
            if ok { self.read = 1 - self.read; }
        }
        if ok { ok = run(self, 2, false); }
        if !ok && submitted { self.quarantine(); }
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
                "intel/gpgpu: edit-life timing kind={} ok={} extent={}x{} grid={}x{} steps={} passes={} ppgtt_prepares=1 lock_ms={} setup_ms={} seed_ms={} baseline_ms={} step_ms={} composite_ms={} submit_ms={} total_ms={} renders={} slow_renders={}\n",
                kind, ok as u8, self.width, self.height, self.grid_width, self.grid_height,
                steps, passes, lock_ms, setup_ms, pass_ms[0], pass_ms[3], pass_ms[1], pass_ms[2],
                submit_ms, total_ms, self.renders, self.slow_renders);
        }
        GpgpuRgba8KernelResult { ok, submitted, marker,
            submit_ms: total_ms,
            release: ok.then(|| gpgpu_rgba8_release(dst)) }
    }

    fn quarantine(&mut self) {
        self.quarantined = true;
        for resource in [&self.cells[0], &self.cells[1], &self.glyphs, &self.previous, &self.font] {
            resource.quarantine_backing();
        }
    }

    fn dispatch(&self, dev: super::Dev, state: DirectRcsState, upload: UploadedKernelArtifact,
        dst: GpgpuRgba8Surface, source: GpgpuRgba8Surface, mode: u32, reset: bool) -> DirectRcsDispatchOutcome
    {
        let (width, height) = match mode {
            0 | 1 => (self.grid_width, self.grid_height),
            2 => (self.width, self.height),
            3 => (self.columns, self.rows),
            _ => return DirectRcsDispatchOutcome::default(),
        };
        // GPGPU_WALKER carries independent u32 group counts. These domains are
        // bounded by the editor page/grid limits; the previous 128K pixel cap
        // was a local chunking policy, not a command dimension limit. One
        // walker per ordered pass avoids a context submit/save/poll per chunk.
        let encoded = encode_edit_life_batch(state, upload, self, dst, source, mode, reset, width, 0, height);
        let admission = if encoded { direct_rcs_submit_batch_state(dev, state) }
            else { DirectRcsSubmissionState::Rejected };
        let submitted = admission.may_have_submitted();
        let observed = if admission.can_poll() {
            direct_rcs_poll_result_slot_timeout_ms(state, EDIT_LIFE_POST_SLOT, EDIT_LIFE_POST_MARKER,
                UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS)
        } else { 0 };
        if observed != EDIT_LIFE_POST_MARKER {
            if submitted { quarantine_direct_rcs_context("edit-life-marker-timeout"); }
            crate::log_error!(target: "gpgpu";
                "intel/gpgpu: edit-life failed mode={} encoded={} submitted={} marker=0x{:08X} width={} height={}\n",
                mode, encoded as u8, submitted as u8, observed, width, height);
        }
        DirectRcsDispatchOutcome { submitted, observed }
    }
}

fn edit_life_allocation(width: u32, height: u32) -> Option<GpgpuOwnedRgba8Surface> {
    let mut allocation = allocate_font_instance_rgba8_surface(width, height)?;
    allocation.system_service = true;
    Some(allocation)
}

// Keep the encoder's exact payload layout a compile-time contract. A fresh
// bake that changes implicit arguments must fail the build, not reject every
// render silently at runtime.
const fn edit_life_abi_matches() -> bool {
    let contract = &EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT;
    if contract.payload_args.len() != 8 || contract.bindings.len() != 8 { return false; }
    let mut index = 0;
    while index < 8 {
        let arg = &contract.payload_args[index];
        if arg.arg_index as usize != index || !matches!(arg.kind, GpgpuArtifactArgKind::ByPointer)
            || arg.offset_bytes as usize != 48 + index * 8 || arg.size_bytes != 8
            || contract.bindings[index].arg_index as usize != index
            || contract.bindings[index].bti as usize != index { return false; }
        index += 1;
    }
    // The sole generated stateful buffer offset stays zero in the cleared
    // payload. All other implicit fields are written by the encoder below.
    let implicit = contract.implicit_payload_args;
    implicit.len() == 4
        && matches!(implicit[0].kind, GpgpuArtifactImplicitArgKind::GlobalIdOffset)
        && implicit[0].offset_bytes == 0 && implicit[0].size_bytes == 12
        && matches!(implicit[1].kind, GpgpuArtifactImplicitArgKind::LocalSize)
        && implicit[1].offset_bytes == 12 && implicit[1].size_bytes == 12
        && matches!(implicit[2].kind, GpgpuArtifactImplicitArgKind::EnqueuedLocalSize)
        && implicit[2].offset_bytes == 32 && implicit[2].size_bytes == 12
        && matches!(implicit[3].kind, GpgpuArtifactImplicitArgKind::BufferOffset { arg_index: 0 })
        && implicit[3].offset_bytes == 112 && implicit[3].size_bytes == 4
}

fn encode_edit_life_batch(state: DirectRcsState, upload: UploadedKernelArtifact,
    life: &EditLifeState, dst: GpgpuRgba8Surface, source: GpgpuRgba8Surface, mode: u32, reset: bool,
    width: u32, first_row: u32, rows: u32) -> bool
{
    if !edit_life_abi_matches() || upload.bin_sha256 != EDIT_LIFE_RGBA8_ARTIFACT.bin_sha256
        || upload.gpu != EDIT_LIFE_RGBA8_GPU || upload.bytes != EDIT_LIFE_RGBA8_ADLS_BIN.len()
    { return false; }
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    let uniforms_gpu = DIRECT_RCS_GPU_VA_BATCH_BASE + EDIT_LIFE_UNIFORMS as u64;
    let resources = [life.cells[life.read].surface(), life.cells[1 - life.read].surface(),
        life.glyphs.surface(), life.previous.surface(), life.font.surface(), source, dst];
    let mut buffers = [(0u64, 0usize); 8];
    for (index, resource) in resources.into_iter().enumerate() { buffers[index] = (resource.gpu, resource.bytes); }
    buffers[7] = (uniforms_gpu, 64);
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
        let local_ids = state.batch_virt.add(EDIT_LIFE_PAYLOAD + EDIT_LIFE_CROSS_BYTES) as *mut u16;
        for lane in 0..16 { core::ptr::write_volatile(local_ids.add(lane), lane as u16); }
        let uniforms = [mode, reset as u32, life.width, life.height,
            life.viewport.x as u32, life.viewport.y as u32, life.viewport.width, life.viewport.height,
            life.grid_width, life.grid_height, life.columns, life.rows,
            source.pitch_bytes, dst.pitch_bytes, life.cell_width, life.cell_height];
        core::ptr::copy_nonoverlapping(uniforms.as_ptr(), state.batch_virt.add(EDIT_LIFE_UNIFORMS) as *mut u32, uniforms.len());
    }
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, DIRECT_RCS_BATCH_BYTES / 4) };
    let mut cursor = 0;
    let mut ok = direct_rcs_push_pipe_control_full(batch, &mut cursor, (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL | 1);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_3D);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push_state_base_address(batch, &mut cursor, DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE, upload.gpu);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    for value in [MEDIA_VFE_STATE_CMD, 0, 0, GPGPU_VFE_DW3_UOS, 0, GPGPU_VFE_DW5_UOS, 0, 0, 0,
        MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD, 0, 32, EDIT_LIFE_IDD as u32]
    { ok &= direct_rcs_push(batch, &mut cursor, value); }
    ok &= direct_rcs_push_gpgpu_walker_2d(batch, &mut cursor, EDIT_LIFE_PAYLOAD,
        EDIT_LIFE_INDIRECT_BYTES, width.div_ceil(16), rows, GPGPU_WALKER_SIMD16_MASK);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(batch, &mut cursor, state.gpu_va.result,
        EDIT_LIFE_POST_SLOT, EDIT_LIFE_POST_MARKER);
    if ok {
        super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
        super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    }
    ok
}
