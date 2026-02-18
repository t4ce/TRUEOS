#![cfg(feature = "trueos")]

use core::sync::atomic::{AtomicBool, Ordering};

use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Timer};
use crate::cmd_stream::{self, CmdStreamCommand};

extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

static WEBGL_SMOKE_TASK_STARTED: AtomicBool = AtomicBool::new(false);

#[inline]
fn log_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(1, bytes.as_ptr(), bytes.len()) };
}

#[inline]
fn log_str(s: &str) {
    log_bytes(s.as_bytes());
}

#[inline]
fn push_vertex(dst: &mut Vec<u8>, x_px: f32, y_px: f32, w: f32, h: f32, rgb: u32) {
    let nx = (2.0 * (x_px / w)) - 1.0;
    let ny = 1.0 - (2.0 * (y_px / h));
    dst.extend_from_slice(&nx.to_le_bytes());
    dst.extend_from_slice(&ny.to_le_bytes());
    dst.push(((rgb >> 16) & 0xff) as u8);
    dst.push(((rgb >> 8) & 0xff) as u8);
    dst.push((rgb & 0xff) as u8);
    dst.push(0);
}

fn build_rot_rect_vertices(w: i32, h: i32, angle: f32) -> Vec<u8> {
    let wf = w.max(1) as f32;
    let hf = h.max(1) as f32;
    let cx = wf * 0.5;
    let cy = hf * 0.5;
    let hw = 120.0f32;
    let hh = 80.0f32;
    let c = libm::cosf(angle);
    let s = libm::sinf(angle);
    let fill = 0xffe45e;

    let rot = |x: f32, y: f32| -> (f32, f32) {
        let rx = (x * c) - (y * s);
        let ry = (x * s) + (y * c);
        (cx + rx, cy + ry)
    };

    let p0 = rot(-hw, -hh);
    let p1 = rot(hw, -hh);
    let p2 = rot(hw, hh);
    let p3 = rot(-hw, hh);

    let mut out = Vec::with_capacity(6 * 12);
    // Triangle 1: p0, p1, p2
    push_vertex(&mut out, p0.0, p0.1, wf, hf, fill);
    push_vertex(&mut out, p1.0, p1.1, wf, hf, fill);
    push_vertex(&mut out, p2.0, p2.1, wf, hf, fill);
    // Triangle 2: p0, p2, p3
    push_vertex(&mut out, p0.0, p0.1, wf, hf, fill);
    push_vertex(&mut out, p2.0, p2.1, wf, hf, fill);
    push_vertex(&mut out, p3.0, p3.1, wf, hf, fill);
    out
}

#[embassy_executor::task]
pub async fn boot_webgl_smoke_task() {
    if WEBGL_SMOKE_TASK_STARTED.swap(true, Ordering::SeqCst) {
        log_str("qjs-webgl-smoke: already running\n");
        return;
    }

    let w = 1280i32;
    let h = 800i32;
    let clear_rgb = 0x1f3f7a;
    let mut angle = 0.0f32;

    log_str("qjs-webgl-smoke: starting (20Hz, no-qjs, split-transactional)\n");
    loop {
        angle += 0.05;
        if angle > 6.2831855 {
            angle -= 6.2831855;
        }

        cmd_stream::enqueue(CmdStreamCommand::SetViewport { w, h });
        cmd_stream::enqueue(CmdStreamCommand::SetBlendEnabled { enabled: false });
        cmd_stream::enqueue(CmdStreamCommand::SetClearColor { clear_rgb });
        cmd_stream::enqueue(CmdStreamCommand::BeginFrame);
        cmd_stream::enqueue(CmdStreamCommand::DrawTriangles {
            vertices: build_rot_rect_vertices(w, h, angle),
        });
        cmd_stream::enqueue(CmdStreamCommand::EndFrame);

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}
