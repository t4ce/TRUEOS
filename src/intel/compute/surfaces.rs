use super::*;

#[derive(Copy, Clone)]
pub(super) struct GpgpuStoreSurfaceState {
	pub(super) ready: bool,
	pub(super) binding_table_offset: usize,
	pub(super) surface_state_offset: usize,
	pub(super) binding_table_index: usize,
	pub(super) surface_gpu: u64,
	pub(super) target_gpu: u64,
	pub(super) surface_dword0: u32,
	pub(super) surface_dword2: u32,
	pub(super) surface_dword3: u32,
	pub(super) binding_entry: u32,
}

pub(super) fn prepare_gpgpu_store_surface_state(warm: RenderWarmState) -> GpgpuStoreSurfaceState {
	let target_gpu = GPU_VA_RESULT_BASE
		+ (RESULT_SLOT_GPGPU_EU_C_STORE_DWORD as u64) * core::mem::size_of::<u32>() as u64;
	prepare_gpgpu_store_surface_state_for_target(
		warm,
		target_gpu,
		"bind-send-bti-to-result-raw-buffer",
	)
}

pub(super) fn prepare_gpgpu_store_surface_state_for_target(
	warm: RenderWarmState,
	target_gpu: u64,
	note: &'static str,
) -> GpgpuStoreSurfaceState {
	prepare_gpgpu_store_surface_state_for_target_span(
		warm,
		target_gpu,
		core::mem::size_of::<u32>(),
		note,
	)
}

pub(super) fn prepare_gpgpu_store_surface_state_for_target_span(
	warm: RenderWarmState,
	target_gpu: u64,
	target_bytes: usize,
	note: &'static str,
) -> GpgpuStoreSurfaceState {
	prepare_gpgpu_store_surface_state_for_target_span_with_bti(
		warm,
		target_gpu,
		target_bytes,
		GPGPU_STORE_BINDING_TABLE_INDEX,
		note,
	)
}

fn prepare_gpgpu_store_surface_state_for_target_span_with_bti(
	warm: RenderWarmState,
	target_gpu: u64,
	target_bytes: usize,
	binding_table_index: usize,
	note: &'static str,
) -> GpgpuStoreSurfaceState {
	prepare_gpgpu_store_surface_state_for_target_span_at_offsets(
		warm,
		target_gpu,
		target_bytes,
		binding_table_index,
		GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES,
		GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES,
		note,
	)
}

pub(super) fn prepare_gpgpu_mandelbrot_store_surface_state_for_target_span(
	warm: RenderWarmState,
	target_gpu: u64,
	target_bytes: usize,
	note: &'static str,
) -> GpgpuStoreSurfaceState {
	prepare_gpgpu_store_surface_state_for_target_span_at_offsets(
		warm,
		target_gpu,
		target_bytes,
		GPGPU_MANDELBROT_STORE_BINDING_TABLE_INDEX,
		GPGPU_MANDELBROT_STORE_BINDING_TABLE_OFFSET_BYTES,
		GPGPU_MANDELBROT_STORE_SURFACE_STATE_OFFSET_BYTES,
		note,
	)
}

pub(super) fn prepare_gpgpu_store_surface_state_for_target_span_at_offsets(
	warm: RenderWarmState,
	target_gpu: u64,
	target_bytes: usize,
	binding_table_index: usize,
	binding_table_offset: usize,
	surface_state_offset: usize,
	note: &'static str,
) -> GpgpuStoreSurfaceState {
	let binding_table_entries =
		GPGPU_STORE_BINDING_TABLE_ENTRIES.max(binding_table_index.saturating_add(1));
	let binding_table_bytes = binding_table_entries * core::mem::size_of::<u32>();
	let surface_bytes = GPGPU_STORE_SURFACE_DWORDS * core::mem::size_of::<u32>();
	let binding_end = binding_table_offset.saturating_add(binding_table_bytes);
	let surface_end = surface_state_offset.saturating_add(surface_bytes);
	let binding_table_aligned = binding_table_offset & 0x3F == 0;
	let surface_aligned = surface_state_offset & 0x3F == 0;
	let ready = binding_table_aligned
		&& surface_aligned
		&& binding_end <= warm.draw_state_len
		&& surface_end <= warm.draw_state_len;
	if !ready {
		crate::log!(
			"intel/gpgpu: gpu-program-surface-state ready=0 reason=draw-state-bounds bt_off=0x{:X} bt_bytes=0x{:X} surf_off=0x{:X} surf_bytes=0x{:X} draw_state_len=0x{:X}\n",
			binding_table_offset,
			binding_table_bytes,
			surface_state_offset,
			surface_bytes,
			warm.draw_state_len,
		);
		return GpgpuStoreSurfaceState {
			ready: false,
			binding_table_offset,
			surface_state_offset,
			binding_table_index,
			surface_gpu: GPU_VA_DRAW_STATE_BASE + surface_state_offset as u64,
			target_gpu,
			surface_dword0: 0,
			surface_dword2: 0,
			surface_dword3: 0,
			binding_entry: 0,
		};
	}

	let binding_entry = surface_state_offset as u32;
	let surface_dword0 = (SURFTYPE_BUFFER << 29) | (SURFACE_FORMAT_RAW << 18);
	let surface_span_bytes = target_bytes.max(1);
	let surface_extent = surface_span_bytes.saturating_sub(1);
	let surface_width_minus1 = (surface_extent & 0x7F) as u32;
	let surface_height_minus1 = ((surface_extent >> 7) & 0x3FFF) as u32;
	let surface_depth_minus1 = ((surface_extent >> 21) & 0x7FF) as u32;
	let surface_dword2 = (surface_height_minus1 << 16) | surface_width_minus1;
	let surface_dword3 = surface_depth_minus1 << 21;
	unsafe {
		let binding_table = warm.draw_state_virt.add(binding_table_offset) as *mut u32;
		for index in 0..binding_table_entries {
			core::ptr::write_volatile(binding_table.add(index), binding_entry);
		}

		let surface = warm.draw_state_virt.add(surface_state_offset) as *mut u32;
		for index in 0..GPGPU_STORE_SURFACE_DWORDS {
			core::ptr::write_volatile(surface.add(index), 0);
		}
		core::ptr::write_volatile(surface.add(0), surface_dword0);
		core::ptr::write_volatile(surface.add(1), RENDER_MOCS << 24);
		core::ptr::write_volatile(surface.add(2), surface_dword2);
		core::ptr::write_volatile(surface.add(3), surface_dword3);
		core::ptr::write_volatile(surface.add(8), target_gpu as u32);
		core::ptr::write_volatile(surface.add(9), (target_gpu >> 32) as u32);
	}
	crate::intel::dma_flush(
		unsafe { warm.draw_state_virt.add(binding_table_offset) },
		binding_table_bytes,
	);
	crate::intel::dma_flush(
		unsafe { warm.draw_state_virt.add(surface_state_offset) },
		surface_bytes,
	);
	if should_log_gpgpu_surface_state(note) {
		crate::log!(
			"intel/gpgpu: gpu-program-surface-state ready=1 bti=0x{:02X} bt_off=0x{:X} bt_entries={} bt_entry=0x{:08X} surf_off=0x{:X} surf_gpu=0x{:X} target_gpu=0x{:X} target_bytes=0x{:X} surf_width_m1=0x{:X} surf_height_m1=0x{:X} surf_depth_m1=0x{:X} surf0=0x{:08X} surf1=0x{:08X} surf2=0x{:08X} surf3=0x{:08X} note={}\n",
			binding_table_index,
			binding_table_offset,
			binding_table_entries,
			binding_entry,
			surface_state_offset,
			GPU_VA_DRAW_STATE_BASE + surface_state_offset as u64,
			target_gpu,
			surface_span_bytes,
			surface_width_minus1,
			surface_height_minus1,
			surface_depth_minus1,
			surface_dword0,
			RENDER_MOCS << 24,
			surface_dword2,
			surface_dword3,
			note,
		);
	}

	GpgpuStoreSurfaceState {
		ready: true,
		binding_table_offset,
		surface_state_offset,
		binding_table_index,
		surface_gpu: GPU_VA_DRAW_STATE_BASE + surface_state_offset as u64,
		target_gpu,
		surface_dword0,
		surface_dword2,
		surface_dword3,
		binding_entry,
	}
}

pub(super) fn disabled_gpgpu_store_surface_state() -> GpgpuStoreSurfaceState {
	GpgpuStoreSurfaceState {
		ready: false,
		binding_table_offset: GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES,
		surface_state_offset: GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES,
		binding_table_index: GPGPU_STORE_BINDING_TABLE_INDEX,
		surface_gpu: GPU_VA_DRAW_STATE_BASE + GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES as u64,
		target_gpu: GPU_VA_RESULT_BASE
			+ (RESULT_SLOT_GPGPU_EU_C_STORE_DWORD as u64) * core::mem::size_of::<u32>() as u64,
		surface_dword0: 0,
		surface_dword2: 0,
		surface_dword3: 0,
		binding_entry: 0,
	}
}
