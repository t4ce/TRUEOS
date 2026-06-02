use super::*;

pub(super) fn gpgpu_curbe_total_bytes(load_dummy_curbe: bool) -> usize {
    if load_dummy_curbe {
        GPGPU_DUMMY_CURBE_BYTES
    } else {
        0
    }
}

pub(super) fn gpgpu_curbe_read_length_8dw(load_dummy_curbe: bool) -> u32 {
    if load_dummy_curbe { 1 } else { 0 }
}

pub(super) fn gpgpu_vfe_curbe_allocation_32b(load_dummy_curbe: bool) -> u32 {
    if load_dummy_curbe {
        (GPGPU_DUMMY_CURBE_BYTES / 32) as u32
    } else {
        0
    }
}

pub(super) fn gpgpu_thread_payload_urb_read_length_8dw(
    simd_width: u32,
    use_uos_thread_payload: bool,
    load_dummy_curbe: bool,
) -> u32 {
    if !use_uos_thread_payload {
        return gpgpu_curbe_read_length_8dw(load_dummy_curbe);
    }
    match simd_width {
        32 => 6,
        // UOS uses 3 for SIMD16, and notes SIMD8 still consumes a minimum
        // three 8-DW blocks for x/y/z payload IDs.
        _ => 3,
    }
}

pub(super) fn gpgpu_cross_thread_read_length_8dw() -> u32 {
    0
}

pub(super) fn write_gpgpu_uos_thread_payload(
    warm: RenderWarmState,
    indirect_state_offset_bytes: usize,
    simd_width: u32,
    dispatch_count: u32,
    use_uos_thread_payload: bool,
) -> Result<usize, &'static str> {
    if !use_uos_thread_payload {
        return Ok(0);
    }

    let simd_width = match simd_width {
        32 => 32usize,
        16 => 16usize,
        _ => 8usize,
    };
    let dispatch_count = dispatch_count.max(1) as usize;
    let payload_u16s = dispatch_count
        .checked_mul(3)
        .and_then(|value| value.checked_mul(simd_width))
        .ok_or("gpgpu-uos-payload-size-overflow")?;
    let payload_bytes = payload_u16s
        .checked_mul(core::mem::size_of::<u16>())
        .ok_or("gpgpu-uos-payload-byte-overflow")?;
    let aligned_bytes =
        crate::intel::align_up(payload_bytes, 64).ok_or("gpgpu-uos-payload-align")?;
    if indirect_state_offset_bytes
        .checked_add(aligned_bytes)
        .is_none_or(|end| end > warm.draw_state_len)
    {
        return Err("gpgpu-uos-payload-scratch-exhausted");
    }

    unsafe {
        let payload = warm.draw_state_virt.add(indirect_state_offset_bytes);
        core::ptr::write_bytes(payload, 0, aligned_bytes);
        let ids = payload as *mut u16;
        let mut local = [0u16, 0u16, 0u16];
        let mut dispatch = 0usize;
        while dispatch < dispatch_count {
            let base = dispatch * 3 * simd_width;
            let mut lane = 0usize;
            while lane < simd_width {
                core::ptr::write_volatile(ids.add(base + lane), local[0]);
                core::ptr::write_volatile(ids.add(base + simd_width + lane), local[1]);
                core::ptr::write_volatile(ids.add(base + (2 * simd_width) + lane), local[2]);
                local[0] = local[0].wrapping_add(1);
                lane += 1;
            }
            dispatch += 1;
        }
    }
    crate::intel::dma_flush(
        unsafe { warm.draw_state_virt.add(indirect_state_offset_bytes) },
        aligned_bytes,
    );
    Ok(aligned_bytes)
}

pub(super) fn write_gpgpu_dummy_curbe(
    warm: RenderWarmState,
    curbe_state_offset_bytes: usize,
    load_dummy_curbe: bool,
) -> Result<(), &'static str> {
    if !load_dummy_curbe {
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
            // Mesa compute payloads can add CURBE dword 4 as the base workgroup X.
            // Keep the rest poisoned, but make the base-group offset semantically zero.
            let value = if index == 4 { 0 } else { 0x5A5A_5A5A };
            core::ptr::write_volatile(curbe.add(index), value);
        }
    }
    crate::intel::dma_flush(
        unsafe { warm.draw_state_virt.add(curbe_state_offset_bytes) },
        GPGPU_DUMMY_CURBE_BYTES,
    );
    Ok(())
}
