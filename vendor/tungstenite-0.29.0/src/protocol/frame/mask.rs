/// Generate a random frame mask.
#[inline]
pub fn generate_mask() -> [u8; 4] {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        use core::sync::atomic::{AtomicU32, Ordering};

        static MASK_COUNTER: AtomicU32 = AtomicU32::new(0x9e37_79b9);
        return MASK_COUNTER
            .fetch_add(0x9e37_79b9, Ordering::Relaxed)
            .rotate_left(13)
            .to_ne_bytes();
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    rand::random()
}

/// Mask/unmask a frame.
#[inline]
pub fn apply_mask(buf: &mut [u8], mask: [u8; 4]) {
    apply_mask_fast32(buf, mask);
}

/// A safe unoptimized mask application.
#[inline]
fn apply_mask_fallback(buf: &mut [u8], mask: [u8; 4]) {
    for (i, byte) in buf.iter_mut().enumerate() {
        *byte ^= mask[i & 3];
    }
}

/// Faster version of `apply_mask()` which operates on 4-byte blocks.
#[inline]
pub fn apply_mask_fast32(buf: &mut [u8], mask: [u8; 4]) {
    let mask_u32 = u32::from_ne_bytes(mask);

    let (prefix, words, suffix) = unsafe { buf.align_to_mut::<u32>() };
    apply_mask_fallback(prefix, mask);
    let head = prefix.len() & 3;
    let mask_u32 = if head > 0 {
        if cfg!(target_endian = "big") {
            mask_u32.rotate_left(8 * head as u32)
        } else {
            mask_u32.rotate_right(8 * head as u32)
        }
    } else {
        mask_u32
    };
    for word in words.iter_mut() {
        *word ^= mask_u32;
    }
    apply_mask_fallback(suffix, mask_u32.to_ne_bytes());
}
