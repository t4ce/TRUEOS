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
const EDIT_LIFE_MAX_PIXELS_PER_WALKER: u32 = 128 * 1024;
static EDIT_LIFE_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);

const _: () = {
    assert!(matches!(EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
    assert!(EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.cross_thread_data_bytes == 128);
    assert!(EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.per_thread_data_bytes == 96);
    assert!(EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT.bindings.len() == 8);
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

/// No buffers here belong to the input or compositor task. Upload copies the
/// newest immutable page once; generations and presentation read retained GPU
/// inputs. All submissions retire before this owner can be modified or freed.
pub(crate) struct EditLifeState {
    width: u32,
    height: u32,
    viewport: GpgpuRect,
    grid_width: u32,
    grid_height: u32,
    cells: [GpgpuOwnedRgba8Surface; 2],
    glyphs: GpgpuOwnedRgba8Surface,
    previous: GpgpuOwnedRgba8Surface,
    colors: GpgpuOwnedRgba8Surface,
    page: GpgpuOwnedRgba8Surface,
    read: usize,
    uploaded: bool,
    pending_snapshot: bool,
    initialized: bool,
    quarantined: bool,
}

impl EditLifeState {
    pub(crate) fn new(width: u32, height: u32, viewport: GpgpuRect) -> Option<Self> {
        if width == 0 || height == 0 || viewport.x < 0 || viewport.y < 0
            || viewport.is_empty()
            || (viewport.x as u32).checked_add(viewport.width).is_none_or(|right| right > width)
            || (viewport.y as u32).checked_add(viewport.height).is_none_or(|bottom| bottom > height)
            || direct_rcs_context_is_quarantined()
        { return None; }
        upload_edit_life_kernel()?;
        let grid_width = viewport.width.div_ceil(3).min(512);
        let grid_height = viewport.height.div_ceil(3).min(512);
        let cell_words = grid_width.checked_mul(grid_height)?.checked_mul(2)?;
        Some(Self {
            width, height, viewport, grid_width, grid_height,
            // A one-row allocation keeps uint2 cells tightly packed without
            // inserting the RGBA helper's 64-byte padding between cell rows.
            cells: [edit_life_allocation(cell_words, 1)?, edit_life_allocation(cell_words, 1)?],
            glyphs: edit_life_allocation(width.div_ceil(4), height)?,
            previous: edit_life_allocation(viewport.width.div_ceil(4), viewport.height)?,
            colors: edit_life_allocation(width, height)?,
            page: edit_life_allocation(width, height)?,
            read: 0, uploaded: false, pending_snapshot: false,
            initialized: false, quarantined: false,
        })
    }

    pub(crate) fn extent(&self) -> (u32, u32) { (self.width, self.height) }
    pub(crate) fn viewport(&self) -> GpgpuRect { self.viewport }

    pub(crate) fn upload(&mut self, mask: &[u8], colors: &[u8], page: &[u8]) -> bool {
        let Some(pixels) = (self.width as usize).checked_mul(self.height as usize) else { return false; };
        let Some(rgba_bytes) = pixels.checked_mul(4) else { return false; };
        if self.quarantined || direct_rcs_context_is_quarantined()
            || mask.len() != pixels || colors.len() != rgba_bytes || page.len() != rgba_bytes
        { return false; }
        edit_life_upload_rows(&self.glyphs, mask, self.width as usize, self.height as usize);
        edit_life_upload_rows(&self.colors, colors, self.width as usize * 4, self.height as usize);
        edit_life_upload_rows(&self.page, page, self.width as usize * 4, self.height as usize);
        self.uploaded = true;
        self.pending_snapshot = true;
        true
    }

    /// `steps` is a bounded clock debt, normally zero or one at speed 1/10 Hz.
    /// A fresh snapshot always advances its comparison baseline; `emit` only
    /// chooses whether positive new glyph ink joins the current world.
    pub(crate) fn render(&mut self, dst: GpgpuRgba8Surface, reset: bool, emit: bool, steps: u32)
        -> GpgpuRgba8KernelResult
    {
        if self.quarantined || !self.uploaded || !dst.is_valid()
            || dst.storage_order != GpgpuRgba8StorageOrder::Rgba
            || (dst.width, dst.height) != self.extent() || steps > 5
            || direct_rcs_context_is_quarantined()
        { return GpgpuRgba8KernelResult::default(); }
        let started = direct_rcs_now_tick();
        let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
        let Some(dev) = super::claimed_device() else { return GpgpuRgba8KernelResult::default(); };
        let Some(upload) = upload_edit_life_kernel() else { return GpgpuRgba8KernelResult::default(); };
        let Some(state) = direct_rcs_state_once(dev) else { return GpgpuRgba8KernelResult::default(); };
        let mut submitted = false;
        let mut marker = 0;
        let mut run = |this: &mut Self, mode: u32, reseed: bool| {
            let outcome = this.dispatch(dev, state, upload, dst, mode, reseed);
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
        GpgpuRgba8KernelResult { ok, submitted, marker,
            submit_ms: direct_rcs_elapsed_ms_since(started),
            release: ok.then(|| gpgpu_rgba8_release(dst)) }
    }

    fn quarantine(&mut self) {
        self.quarantined = true;
        for resource in [&self.cells[0], &self.cells[1], &self.glyphs, &self.previous, &self.colors, &self.page] {
            resource.quarantine_backing();
        }
    }

    fn dispatch(&self, dev: super::Dev, state: DirectRcsState, upload: UploadedKernelArtifact,
        dst: GpgpuRgba8Surface, mode: u32, reset: bool) -> DirectRcsDispatchOutcome
    {
        let (width, height) = match mode {
            0 | 1 => (self.grid_width, self.grid_height),
            2 => (self.width, self.height),
            3 => (self.viewport.width, self.viewport.height),
            _ => return DirectRcsDispatchOutcome::default(),
        };
        let rows_per_walker = (EDIT_LIFE_MAX_PIXELS_PER_WALKER / width.div_ceil(16).saturating_mul(16)).max(1);
        let mut submitted = false;
        let mut first_row = 0;
        while first_row < height {
            let rows = rows_per_walker.min(height - first_row);
            let forcewake = direct_rcs_forcewake(dev);
            let mut prepared = forcewake && direct_rcs_map_state(dev, state)
                && direct_rcs_init_ppgtt(state)
                && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
            for resource in [self.cells[self.read].surface(), self.cells[1 - self.read].surface(),
                self.glyphs.surface(), self.previous.surface(), self.colors.surface(), self.page.surface()]
            {
                prepared = prepared && direct_rcs_map_ppgtt_kernel(state, resource.gpu, resource.phys, resource.bytes);
            }
            prepared = prepared && direct_rcs_map_ppgtt_destination(state, dst.gpu, dst.phys, dst.bytes, true)
                && encode_edit_life_batch(state, upload, self, dst, mode, reset, width, first_row, rows);
            let admission = if prepared { direct_rcs_submit_batch_state(dev, state) }
                else { DirectRcsSubmissionState::Rejected };
            submitted |= admission.may_have_submitted();
            let observed = if admission.can_poll() {
                direct_rcs_poll_result_slot_timeout_ms(state, EDIT_LIFE_POST_SLOT, EDIT_LIFE_POST_MARKER,
                    UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS)
            } else { 0 };
            if observed != EDIT_LIFE_POST_MARKER {
                if admission.may_have_submitted() { quarantine_direct_rcs_context("edit-life-marker-timeout"); }
                crate::log_error!(target: "gpgpu";
                    "intel/gpgpu: edit-life failed mode={} prepared={} submitted={} marker=0x{:08X} row={} rows={}\n",
                    mode, prepared as u8, admission.may_have_submitted() as u8, observed, first_row, rows);
                return DirectRcsDispatchOutcome { submitted, observed };
            }
            first_row += rows;
        }
        DirectRcsDispatchOutcome { submitted, observed: EDIT_LIFE_POST_MARKER }
    }
}

fn edit_life_allocation(width: u32, height: u32) -> Option<GpgpuOwnedRgba8Surface> {
    let mut allocation = allocate_font_instance_rgba8_surface(width, height)?;
    allocation.system_service = true;
    Some(allocation)
}

fn edit_life_upload_rows(allocation: &GpgpuOwnedRgba8Surface, data: &[u8], row_bytes: usize, rows: usize) {
    for y in 0..rows {
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr().add(y * row_bytes),
            allocation.virt.add(y * allocation.surface.pitch_bytes as usize), row_bytes); }
    }
    super::dma_flush(allocation.virt, allocation.surface.bytes);
}

fn edit_life_abi_matches() -> bool {
    let contract = &EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT;
    if contract.payload_args.len() != 8 || contract.bindings.len() != 8 { return false; }
    for (index, arg) in contract.payload_args.iter().enumerate() {
        if arg.arg_index as usize != index || arg.kind != GpgpuArtifactArgKind::ByPointer
            || arg.offset_bytes as usize != 48 + index * 8 || arg.size_bytes != 8
            || contract.bindings[index].arg_index as usize != index
            || contract.bindings[index].bti as usize != index { return false; }
    }
    // Four generated stateful buffer-offset arguments remain zero in the
    // cleared payload. Reject drift to an unrecognized implicit argument.
    contract.implicit_payload_args.len() == 7
        && contract.implicit_payload_args[3..].iter().enumerate().all(|(index, arg)|
            arg.offset_bytes as usize == 112 + index * 4 && arg.size_bytes == 4
            && matches!(arg.kind, GpgpuArtifactImplicitArgKind::BufferOffset { arg_index }
                if arg_index == [0, 2, 3, 4][index]))
}

fn encode_edit_life_batch(state: DirectRcsState, upload: UploadedKernelArtifact,
    life: &EditLifeState, dst: GpgpuRgba8Surface, mode: u32, reset: bool,
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
        life.glyphs.surface(), life.previous.surface(), life.colors.surface(), life.page.surface(), dst];
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
            life.grid_width, life.grid_height, life.glyphs.surface.pitch_bytes, life.colors.surface.pitch_bytes,
            life.page.surface.pitch_bytes, dst.pitch_bytes, life.previous.surface.pitch_bytes, 0];
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
