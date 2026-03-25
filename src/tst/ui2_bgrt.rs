use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Timer};

const UI2_BGRT_TEX_ID: u32 = 4_704;
const UI2_BGRT_WINDOW_X: f32 = 240.0;
const UI2_BGRT_WINDOW_Y: f32 = 120.0;
const UI2_BGRT_WINDOW_Z: i16 = 31;

fn bgrt_pixels_to_rgba(pixels: &[u32]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(pixels.len().saturating_mul(4));
    for &pixel in pixels {
        let r = ((pixel >> 16) & 0xFF) as u8;
        let g = ((pixel >> 8) & 0xFF) as u8;
        let b = (pixel & 0xFF) as u8;
        rgba.extend_from_slice(&[r, g, b, 0xFF]);
    }
    rgba
}

#[embassy_executor::task]
pub async fn ui2_bgrt_demo_task() {
    let Some((width, height, pixels)) = crate::efi::acpi::bgrt::decoded_logo_rgba() else {
        crate::log!("ui2-bgrt-demo: no BGRT logo available\n");
        return;
    };

    let rgba = bgrt_pixels_to_rgba(pixels);
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "Demo BGRT",
        crate::r::ui2::Ui2Rect {
            x: UI2_BGRT_WINDOW_X,
            y: UI2_BGRT_WINDOW_Y,
            w: width as f32,
            h: height as f32,
        },
        UI2_BGRT_WINDOW_Z,
        128,
        UI2_BGRT_TEX_ID,
        true,
        [0x00, 0x00, 0x00, 0x00],
    ) else {
        crate::log!("ui2-bgrt-demo: window creation failed tex={}\n", UI2_BGRT_TEX_ID);
        return;
    };

    if !surface.upload_rgba(rgba.as_slice(), "ui2-bgrt-demo-upload") {
        crate::log!(
            "ui2-bgrt-demo: upload failed window={} tex={} size={}x{}\n",
            surface.window_id(),
            surface.tex_id(),
            width,
            height
        );
        return;
    }

    crate::log!(
        "ui2-bgrt-demo: window={} tex={} size={}x{}\n",
        surface.window_id(),
        surface.tex_id(),
        width,
        height
    );

    loop {
        Timer::after(EmbassyDuration::from_secs(3600)).await;
    }
}