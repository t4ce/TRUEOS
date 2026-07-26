pub(crate) const PARTICLE_CRAFT_FRAME_WIDTH: u32 = 640;
pub(crate) const PARTICLE_CRAFT_FRAME_HEIGHT: u32 = 400;
/// Private destination-relative render-quality lever. The artifact accepts
/// 1, 2, or 4 without changing the public Blueprint ABI.
pub(crate) const PARTICLE_CRAFT_RENDER_DIVISOR: u32 = 2;
/// Native-window sample extent retained for diagnostics and shell startup
/// output. Maximized samples are derived from the live destination instead.
pub(crate) const PARTICLE_CRAFT_SAMPLE_WIDTH: u32 =
    PARTICLE_CRAFT_FRAME_WIDTH / PARTICLE_CRAFT_RENDER_DIVISOR;
pub(crate) const PARTICLE_CRAFT_SAMPLE_HEIGHT: u32 =
    PARTICLE_CRAFT_FRAME_HEIGHT / PARTICLE_CRAFT_RENDER_DIVISOR;
pub(crate) const PARTICLE_CRAFT_DEFAULT_PARTICLES: u32 = 128;
pub(crate) const PARTICLE_CRAFT_MAX_PARTICLES: u32 = 256;
pub(crate) const PARTICLE_CRAFT_PARAMS_VERSION: u32 = 1;
pub(crate) const PARTICLE_CRAFT_FLAG_RESET: u32 = 1 << 0;
pub(crate) const PARTICLE_CRAFT_FLAG_ATTRACTOR: u32 = 1 << 1;
pub(crate) const PARTICLE_CRAFT_FLAG_ORBIT: u32 = 1 << 2;
pub(crate) const PARTICLE_CRAFT_KNOWN_FLAGS: u32 =
    PARTICLE_CRAFT_FLAG_RESET | PARTICLE_CRAFT_FLAG_ATTRACTOR | PARTICLE_CRAFT_FLAG_ORBIT;
const PARTICLE_CRAFT_STATE_BYTES: usize = PARTICLE_CRAFT_MAX_PARTICLES as usize * 32;
const PARTICLE_CRAFT_PARAMS_BYTES: usize = 4096;
const PARTICLE_CRAFT_ALLOCATION_BYTES: usize =
    PARTICLE_CRAFT_STATE_BYTES + PARTICLE_CRAFT_PARAMS_BYTES;
const PARTICLE_CRAFT_RENDER_CONTROL_WORDS: usize = 4;
const _: () = assert!(
    matches!(PARTICLE_CRAFT_RENDER_DIVISOR, 1 | 2 | 4)
        && PARTICLE_CRAFT_FRAME_WIDTH.is_multiple_of(PARTICLE_CRAFT_RENDER_DIVISOR)
        && PARTICLE_CRAFT_FRAME_HEIGHT.is_multiple_of(PARTICLE_CRAFT_RENDER_DIVISOR)
);
const _: () = assert!(
    core::mem::size_of::<ParticleCraftParamsV1>()
        + PARTICLE_CRAFT_RENDER_CONTROL_WORDS * core::mem::size_of::<u32>()
        <= PARTICLE_CRAFT_PARAMS_BYTES
);

pub(crate) const fn particle_craft_sample_extent(
    destination_width: u32,
    destination_height: u32,
) -> (u32, u32) {
    (
        destination_width.div_ceil(PARTICLE_CRAFT_RENDER_DIVISOR),
        destination_height.div_ceil(PARTICLE_CRAFT_RENDER_DIVISOR),
    )
}

/// Stable host-side form of the public 64-byte ParticleCraft v1 control block.
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub(crate) struct ParticleCraftParamsV1 {
    pub(crate) version: u32,
    pub(crate) flags: u32,
    pub(crate) seed: u32,
    pub(crate) active_count: u32,
    pub(crate) dt_seconds: f32,
    pub(crate) time_seconds: f32,
    pub(crate) emitter_x: f32,
    pub(crate) emitter_y: f32,
    pub(crate) attractor_x: f32,
    pub(crate) attractor_y: f32,
    pub(crate) attraction: f32,
    pub(crate) swirl: f32,
    pub(crate) gravity_x: f32,
    pub(crate) gravity_y: f32,
    pub(crate) drag: f32,
    pub(crate) intensity: f32,
}

const _: () = assert!(core::mem::size_of::<ParticleCraftParamsV1>() == 64);

impl ParticleCraftParamsV1 {
    pub(crate) const fn arc_forge(time_seconds: f32, dt_seconds: f32, seed: u32) -> Self {
        Self {
            version: PARTICLE_CRAFT_PARAMS_VERSION,
            flags: PARTICLE_CRAFT_FLAG_ORBIT,
            seed,
            active_count: PARTICLE_CRAFT_DEFAULT_PARTICLES,
            dt_seconds,
            time_seconds,
            emitter_x: 320.0,
            emitter_y: 300.0,
            attractor_x: 320.0,
            attractor_y: 180.0,
            attraction: 94.0,
            swirl: 72.0,
            gravity_x: 0.0,
            gravity_y: 58.0,
            drag: 0.42,
            intensity: 1.0,
        }
    }

    pub(crate) fn is_valid(self) -> bool {
        let floats = [
            self.dt_seconds,
            self.time_seconds,
            self.emitter_x,
            self.emitter_y,
            self.attractor_x,
            self.attractor_y,
            self.attraction,
            self.swirl,
            self.gravity_x,
            self.gravity_y,
            self.drag,
            self.intensity,
        ];
        self.version == PARTICLE_CRAFT_PARAMS_VERSION
            && self.flags & !PARTICLE_CRAFT_KNOWN_FLAGS == 0
            && (1..=PARTICLE_CRAFT_MAX_PARTICLES).contains(&self.active_count)
            && floats.into_iter().all(f32::is_finite)
            && (0.0..=0.05).contains(&self.dt_seconds)
            && self.time_seconds >= 0.0
            && (-480.0..=480.0).contains(&self.attraction)
            && (-480.0..=480.0).contains(&self.swirl)
            && (0.0..=5.0).contains(&self.drag)
            && (0.0..=4.0).contains(&self.intensity)
    }
}

fn reserve_particle_craft_gpu_va(bytes: usize) -> Option<u64> {
    let bytes = align_up(bytes, super::WARM_ALIGN)? as u64;
    {
        let mut free = PARTICLE_CRAFT_GPU_VA_FREE.lock();
        if let Some(index) = free
            .iter()
            .position(|(start, end)| end.saturating_sub(*start) >= bytes)
        {
            let (start, end) = free[index];
            let next = start.checked_add(bytes)?;
            if next == end {
                free.swap_remove(index);
            } else {
                free[index].0 = next;
            }
            return Some(start);
        }
    }
    loop {
        let current = PARTICLE_CRAFT_GPU_VA_CURSOR.load(Ordering::Acquire);
        let aligned = current.checked_add((super::WARM_ALIGN - 1) as u64)?
            & !((super::WARM_ALIGN - 1) as u64);
        let next = aligned.checked_add(bytes)?;
        if next > DIRECT_RCS_GPU_VA_PARTICLE_CRAFT_LIMIT {
            return None;
        }
        if PARTICLE_CRAFT_GPU_VA_CURSOR
            .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Some(aligned);
        }
    }
}

fn recycle_particle_craft_gpu_va(gpu: u64, bytes: usize) {
    let Some(bytes) = align_up(bytes, super::WARM_ALIGN).map(|value| value as u64) else {
        return;
    };
    let Some(end) = gpu.checked_add(bytes) else {
        return;
    };
    if gpu < DIRECT_RCS_GPU_VA_PARTICLE_CRAFT_BASE || end > DIRECT_RCS_GPU_VA_PARTICLE_CRAFT_LIMIT {
        return;
    }
    let mut free = PARTICLE_CRAFT_GPU_VA_FREE.lock();
    free.push((gpu, end));
    free.sort_unstable_by_key(|range| range.0);
    let mut write = 0usize;
    for read in 0..free.len() {
        let range = free[read];
        if write != 0 && range.0 <= free[write - 1].1 {
            free[write - 1].1 = free[write - 1].1.max(range.1);
        } else {
            free[write] = range;
            write += 1;
        }
    }
    free.truncate(write);
}

/// Persistent state/control storage owned by exactly one preview or UI4 frame.
///
/// A completion timeout quarantines this allocation alongside the RCS context;
/// a possibly-late walker must never observe recycled physical pages or a
/// remapped VA.
pub(crate) struct GpgpuOwnedParticleCraftState {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
    quarantined: bool,
}

unsafe impl Send for GpgpuOwnedParticleCraftState {}
unsafe impl Sync for GpgpuOwnedParticleCraftState {}

impl GpgpuOwnedParticleCraftState {
    pub(crate) fn allocate() -> Option<Self> {
        let (phys, virt) = crate::dma::alloc(PARTICLE_CRAFT_ALLOCATION_BYTES, super::WARM_ALIGN)?;
        let Some(gpu) = reserve_particle_craft_gpu_va(PARTICLE_CRAFT_ALLOCATION_BYTES) else {
            crate::dma::dealloc(virt, PARTICLE_CRAFT_ALLOCATION_BYTES);
            return None;
        };
        unsafe {
            core::ptr::write_bytes(virt, 0, PARTICLE_CRAFT_ALLOCATION_BYTES);
        }
        super::dma_flush(virt, PARTICLE_CRAFT_ALLOCATION_BYTES);
        Some(Self {
            phys,
            gpu,
            virt,
            bytes: PARTICLE_CRAFT_ALLOCATION_BYTES,
            quarantined: false,
        })
    }

    pub(crate) const fn state_phys(&self) -> u64 {
        self.phys
    }

    pub(crate) const fn state_gpu(&self) -> u64 {
        self.gpu
    }

    pub(crate) const fn params_phys(&self) -> u64 {
        self.phys + PARTICLE_CRAFT_STATE_BYTES as u64
    }

    pub(crate) const fn params_gpu(&self) -> u64 {
        self.gpu + PARTICLE_CRAFT_STATE_BYTES as u64
    }

    pub(crate) const fn bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn quarantine(&mut self) {
        self.quarantined = true;
    }

    fn write_params(&mut self, params: ParticleCraftParamsV1, dst: GpgpuRgba8Surface) {
        unsafe {
            core::ptr::copy_nonoverlapping(
                &params as *const ParticleCraftParamsV1 as *const u8,
                self.virt.add(PARTICLE_CRAFT_STATE_BYTES),
                core::mem::size_of::<ParticleCraftParamsV1>(),
            );
            let render_controls = [
                dst.width,
                dst.height,
                dst.pitch_bytes / core::mem::size_of::<u32>() as u32,
                PARTICLE_CRAFT_RENDER_DIVISOR,
            ];
            core::ptr::copy_nonoverlapping(
                render_controls.as_ptr().cast::<u8>(),
                self.virt.add(
                    PARTICLE_CRAFT_STATE_BYTES + core::mem::size_of::<ParticleCraftParamsV1>(),
                ),
                PARTICLE_CRAFT_RENDER_CONTROL_WORDS * core::mem::size_of::<u32>(),
            );
        }
        super::dma_flush(
            unsafe { self.virt.add(PARTICLE_CRAFT_STATE_BYTES) },
            PARTICLE_CRAFT_PARAMS_BYTES,
        );
    }
}

impl Drop for GpgpuOwnedParticleCraftState {
    fn drop(&mut self) {
        if self.quarantined {
            crate::log_warn!(
                target: "gpgpu";
                "intel/gpgpu: ParticleCraft state quarantined phys=0x{:X} gpu=0x{:X} bytes=0x{:X} action=no-unmap-no-free reboot_required=1\n",
                self.phys,
                self.gpu,
                self.bytes,
            );
            return;
        }
        crate::dma::dealloc(self.virt, self.bytes);
        recycle_particle_craft_gpu_va(self.gpu, self.bytes);
    }
}

pub(crate) fn particle_craft_rgba8_frame(
    state: &mut GpgpuOwnedParticleCraftState,
    dst: GpgpuRgba8Surface,
    params: ParticleCraftParamsV1,
) -> GpgpuRgba8KernelResult {
    if !params.is_valid()
        || !dst.is_valid()
        || !dst
            .pitch_bytes
            .is_multiple_of(core::mem::size_of::<u32>() as u32)
        || dst.storage_order != GpgpuRgba8StorageOrder::Rgba
    {
        return GpgpuRgba8KernelResult::default();
    }

    state.write_params(params, dst);
    let start_tick = direct_rcs_now_tick();
    let outcome = submit_particle_craft_rgba8(state, dst, params.active_count);
    let ok = outcome.observed == PARTICLE_CRAFT_POST_MARKER;
    if outcome.submitted && !ok {
        state.quarantine();
    }
    GpgpuRgba8KernelResult {
        ok,
        submitted: outcome.submitted,
        marker: outcome.observed,
        submit_ms: direct_rcs_elapsed_ms_since(start_tick),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}
