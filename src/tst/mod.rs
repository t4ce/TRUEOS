pub mod html;
pub mod http_trueosfs;
pub mod nalgebra_demo;
pub mod smoke_fs;
pub mod tls_demo;
pub mod ws_smoke;

use embassy_executor::task;

#[task]
pub(crate) async fn boot_parse5_smoke_task() {
    crate::log!("qjs-parse5-smoke: starting\n");
    unsafe { trueos_qjs::trueos_smoke::run_parse5_smoke() };
    crate::log!("qjs-common-modules-smoke: starting\n");
    unsafe { trueos_qjs::trueos_smoke::run_common_modules_smoke() };
    crate::log!("qjs-parse5-smoke: done\n");
    crate::v::readiness::set(crate::v::readiness::QJS_PARSE5_SMOKE_DONE);
}

#[task]
pub(crate) async fn boot_pixi_smoke_task() {
    crate::log!("qjs-pixi-smoke: starting\n");
    unsafe { trueos_qjs::trueos_smoke::run_pixi_import_smoke() };
    crate::log!("qjs-pixi-smoke: done\n");
}

#[task]
pub(crate) async fn boot_pixi_rect_smoke_task() {
    use embassy_time::{Duration as EmbassyDuration, Timer};

    crate::log!("qjs-pixi-rect-smoke: waiting for VirtioSw gfx\n");

    // Wait until the user (or system) switches gfx into a virtio-backed scanout.
    // This avoids drawing into the Limine/VGA console framebuffer.
    loop {
        if crate::gfx::backend_kind() == Some(crate::gfx::BackendKind::VirtioSw) {
            crate::log!("qjs-pixi-rect-smoke: starting\n");
            unsafe { trueos_qjs::trueos_smoke::run_pixi_rect_smoke() };
            crate::log!("qjs-pixi-rect-smoke: done\n");
            return;
        }
        Timer::after(EmbassyDuration::from_millis(100)).await;
    }
}

#[cfg(feature = "gfx_virgl")]
#[task]
pub(crate) async fn boot_virtio_sw_triangle_smoke_task() {
    use embassy_time::{Duration as EmbassyDuration, Timer};

    crate::log!("gfx-virtio-sw-tri-smoke: armed (draw on VirtioSw)\n");

    #[inline]
    fn put_vtx(dst: &mut [u8; 36], idx: usize, x: f32, y: f32, r: u8, g: u8, b: u8) {
        let off = idx * 12;
        dst[off..off + 4].copy_from_slice(&x.to_le_bytes());
        dst[off + 4..off + 8].copy_from_slice(&y.to_le_bytes());
        dst[off + 8] = r;
        dst[off + 9] = g;
        dst[off + 10] = b;
        dst[off + 11] = 0;
    }

    let mut vtx = [0u8; 36];
    // Clip-space (NDC) triangle: should be visible regardless of viewport size.
    put_vtx(&mut vtx, 0, -0.6, -0.6, 255, 0, 0);
    put_vtx(&mut vtx, 1, 0.6, -0.6, 0, 255, 0);
    put_vtx(&mut vtx, 2, 0.0, 0.6, 0, 0, 255);

    let mut last_epoch: u64 = 0;

    loop {
        // Wait until we're on VirtioSw and the backend epoch advanced (new swap).
        loop {
            if crate::gfx::backend_kind() == Some(crate::gfx::BackendKind::VirtioSw) {
                let epoch = crate::gfx::backend_epoch();
                if epoch != last_epoch {
                    last_epoch = epoch;
                    break;
                }
            }
            Timer::after(EmbassyDuration::from_millis(50)).await;
        }

        crate::log!("gfx-virtio-sw-tri-smoke: drawing epoch={}\n", last_epoch);

        // Draw a couple frames to make the transition obvious even if the first present races.
        for _ in 0..3 {
            let rc = unsafe {
                crate::surface::io::cabi::trueos_cabi_gfx_draw_rgb_triangles(
                    0x00_08_18_30,
                    vtx.as_ptr(),
                    vtx.len(),
                )
            };
            crate::log!("gfx-virtio-sw-tri-smoke: draw rc={}\n", rc);
            Timer::after(EmbassyDuration::from_millis(50)).await;
        }

        // Wait until we leave VirtioSw before allowing another draw. (Avoid spamming draws
        // while staying on VirtioSw.)
        loop {
            if crate::gfx::backend_kind() != Some(crate::gfx::BackendKind::VirtioSw) {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(100)).await;
        }
    }
}
