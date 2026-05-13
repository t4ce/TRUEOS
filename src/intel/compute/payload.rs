use super::*;

pub(super) fn gpgpu_curbe_total_bytes() -> usize {
	if GPGPU_LOAD_DUMMY_CURBE {
		GPGPU_DUMMY_CURBE_BYTES
	} else {
		0
	}
}

pub(super) fn gpgpu_curbe_read_length_8dw() -> u32 {
	if GPGPU_LOAD_DUMMY_CURBE {
		1
	} else {
		0
	}
}

pub(super) fn gpgpu_vfe_curbe_allocation_32b() -> u32 {
	if GPGPU_LOAD_DUMMY_CURBE {
		(GPGPU_DUMMY_CURBE_BYTES / 32) as u32
	} else {
		0
	}
}

pub(super) fn write_gpgpu_dummy_curbe(
	warm: RenderWarmState,
	curbe_state_offset_bytes: usize,
) -> Result<(), &'static str> {
	if !GPGPU_LOAD_DUMMY_CURBE {
		return Ok(());
	}

	let curbe_index = curbe_state_offset_bytes / core::mem::size_of::<u32>();
	let curbe_dwords = GPGPU_DUMMY_CURBE_BYTES / core::mem::size_of::<u32>();
	if curbe_index
		.checked_add(curbe_dwords)
		.is_none_or(|end| end * core::mem::size_of::<u32>() > warm.draw_state_len)
	{
		return Err("gpgpu-curbe-scratch-exhausted");
	}

	unsafe {
		let curbe = warm.draw_state_virt.add(curbe_state_offset_bytes) as *mut u32;
		for index in 0..curbe_dwords {
			core::ptr::write_volatile(curbe.add(index), 0x5A5A_5A5A);
		}
	}
	crate::intel::dma_flush(
		unsafe { warm.draw_state_virt.add(curbe_state_offset_bytes) },
		GPGPU_DUMMY_CURBE_BYTES,
	);
	Ok(())
}
