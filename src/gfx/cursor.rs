#[cfg(feature = "gfx_virgl")]
use embassy_time::{Duration as EmbassyDuration, Timer};

#[inline]
pub fn cursor_overlay_tick() -> i32 {
    crate::surface::io::cabi::kernel_cursor_overlay_tick()
}

#[cfg(feature = "gfx_virgl")]
fn build_default_cursor_shape_bgra(width: usize, height: usize) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec![0u8; width.saturating_mul(height).saturating_mul(4)];
    if width == 0 || height == 0 {
        return out;
    }

    // High-contrast cursor marker: black outer ring + white inner disk.
    let cx = (width / 2) as i32;
    let cy = (height / 2) as i32;
    let radius = 6i32;
    let ring = 2i32;

    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let dx = x - cx;
            let dy = y - cy;
            let d2 = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
            let r2 = radius.saturating_mul(radius);
            let r_inner = radius.saturating_sub(ring);
            let r_inner2 = r_inner.saturating_mul(r_inner);
            let off = ((y as usize)
                .saturating_mul(width)
                .saturating_add(x as usize))
            .saturating_mul(4);

            if d2 <= r2 && d2 >= r_inner2 {
                // Opaque black ring.
                out[off] = 0;
                out[off + 1] = 0;
                out[off + 2] = 0;
                out[off + 3] = 255;
            } else if d2 < r_inner2 {
                // Opaque white center.
                out[off] = 255;
                out[off + 1] = 255;
                out[off + 2] = 255;
                out[off + 3] = 255;
            }
        }
    }

    out
}

#[embassy_executor::task]
pub async fn gfx_hw_cursor_task() {
    #[cfg(not(feature = "gfx_virgl"))]
    {
        return;
    }

    #[cfg(feature = "gfx_virgl")]
    {
        const CURSOR_W: u32 = 64;
        const CURSOR_H: u32 = 64;
        let cursor_pixels = build_default_cursor_shape_bgra(CURSOR_W as usize, CURSOR_H as usize);
        let hot_x = CURSOR_W / 2;
        let hot_y = CURSOR_H / 2;
        let mut read_seq: u64 = 0;
        let mut dropped_total: u64 = 0;
        let mut cursor_ready = false;
        let mut events = [crate::usb::hid::TrueosHidCursorEvent::default(); 32];

        loop {
            if !cursor_ready {
                let init = crate::gfx::with_context(|ctx| {
                    if !ctx.hw_cursor_supported() {
                        return Err(trueos_gfx_core::Error::Unsupported);
                    }
                    ctx.hw_cursor_define_bgra(
                        CURSOR_W,
                        CURSOR_H,
                        hot_x,
                        hot_y,
                        cursor_pixels.as_slice(),
                    )
                });

                match init {
                    Some(Ok(())) => {
                        cursor_ready = true;
                        let centered = crate::gfx::with_context(|ctx| {
                            let extent = ctx.swapchain_desc().extent;
                            if extent.width == 0 || extent.height == 0 {
                                return Err(trueos_gfx_core::Error::Invalid);
                            }
                            let cx = (extent.width / 2) as i32;
                            let cy = (extent.height / 2) as i32;
                            ctx.hw_cursor_move(cx, cy)
                        });
                        if !matches!(centered, Some(Ok(()))) {
                            crate::log!("gfx-hw-cursor: initial move failed (will retry)\n");
                            cursor_ready = false;
                            Timer::after(EmbassyDuration::from_millis(60)).await;
                            continue;
                        }
                        crate::log!("gfx-hw-cursor: enabled\n");
                    }
                    Some(Err(trueos_gfx_core::Error::Unsupported)) => {
                        crate::log!("gfx-hw-cursor: unsupported (no hardware cursor queue)\n");
                        return;
                    }
                    Some(Err(_)) | None => {
                        Timer::after(EmbassyDuration::from_millis(100)).await;
                        continue;
                    }
                }
            }

            let (next_seq, dropped, wrote) =
                crate::usb::hid::read_cursor_events_since(read_seq, &mut events);
            read_seq = next_seq;
            if dropped != 0 {
                dropped_total = dropped_total.saturating_add(dropped as u64);
                if (dropped_total % 128) == 0 {
                    crate::log!("gfx-hw-cursor: dropped events total={}\n", dropped_total);
                }
            }

            if wrote > 0 {
                let evt = events[wrote - 1];
                let moved = crate::gfx::with_context(|ctx| {
                    let extent = ctx.swapchain_desc().extent;
                    if extent.width == 0 || extent.height == 0 {
                        return Err(trueos_gfx_core::Error::Invalid);
                    }
                    let nx = if evt.x.is_finite() {
                        evt.x.clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let ny = if evt.y.is_finite() {
                        evt.y.clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let x = libm::round(nx * (extent.width.saturating_sub(1) as f64)) as i32;
                    let y = libm::round(ny * (extent.height.saturating_sub(1) as f64)) as i32;
                    ctx.hw_cursor_move(x, y)
                });

                if !matches!(moved, Some(Ok(()))) {
                    crate::log!("gfx-hw-cursor: move failed (reinit)\n");
                    cursor_ready = false;
                }
            }

            Timer::after(EmbassyDuration::from_millis(4)).await;
        }
    }
}
