extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use spin::Mutex;

extern "C" {
    fn trueos_cabi_gfx_draw_rgb_triangles(clear_rgb: u32, vtx_ptr: *const u8, vtx_len: usize) -> i32;
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

static LAST_SUBMIT_RC: AtomicI32 = AtomicI32::new(i32::MIN);
static SUBMIT_ERROR_COUNT: AtomicU32 = AtomicU32::new(0);

#[inline]
fn log_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(1, bytes.as_ptr(), bytes.len()) };
}

#[inline]
fn log_i32_dec(v: i32) {
    if v == 0 {
        log_bytes(b"0");
        return;
    }
    let mut n = v as i64;
    if n < 0 {
        log_bytes(b"-");
        n = -n;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    log_bytes(&buf[i..]);
}

pub(crate) enum CmdStreamCommand {
    BeginFrame,
    SetClearColor { clear_rgb: u32 },
    SetViewport { w: i32, h: i32 },
    SetBlendEnabled { enabled: bool },
    SetBlendFunc {
        src_rgb: u32,
        dst_rgb: u32,
        src_alpha: u32,
        dst_alpha: u32,
    },
    SetBlendEquation { rgb: u32, alpha: u32 },
    DrawTriangles { vertices: Vec<u8> },
    EndFrame,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct BlendState {
    enabled: bool,
    src_rgb: u32,
    dst_rgb: u32,
    src_alpha: u32,
    dst_alpha: u32,
    eq_rgb: u32,
    eq_alpha: u32,
}

impl Default for BlendState {
    fn default() -> Self {
        Self {
            enabled: false,
            src_rgb: 1, // ONE
            dst_rgb: 0, // ZERO
            src_alpha: 1, // ONE
            dst_alpha: 0, // ZERO
            eq_rgb: 0x8006, // FUNC_ADD
            eq_alpha: 0x8006, // FUNC_ADD
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct PipelineKey {
    viewport_w: i32,
    viewport_h: i32,
    blend: BlendState,
}

struct DrawBatch {
    key: PipelineKey,
    vtx: Vec<u8>,
}

struct FrameState {
    active: bool,
    // GL-style clear color state
    clear_rgb: u32,
    // Captured at BeginFrame and used when submitting that frame.
    frame_clear_rgb: u32,
    viewport_w: i32,
    viewport_h: i32,
    blend: BlendState,
    batches: Vec<DrawBatch>,
}

static FRAME_STATE: Mutex<FrameState> = Mutex::new(FrameState {
    active: false,
    clear_rgb: 0x00_08_18_30,
    frame_clear_rgb: 0x00_08_18_30,
    viewport_w: 0,
    viewport_h: 0,
    blend: BlendState {
        enabled: false,
        src_rgb: 1,
        dst_rgb: 0,
        src_alpha: 1,
        dst_alpha: 0,
        eq_rgb: 0x8006,
        eq_alpha: 0x8006,
    },
    batches: Vec::new(),
});

fn submit_rgb_triangles(clear_rgb: u32, vertices: Option<&[u8]>) {
    let rc = match vertices {
        Some(vtx) => unsafe {
            trueos_cabi_gfx_draw_rgb_triangles(clear_rgb, vtx.as_ptr(), vtx.len())
        },
        None => unsafe {
            trueos_cabi_gfx_draw_rgb_triangles(clear_rgb, core::ptr::null(), 0)
        },
    };
    if rc != 0 {
        let prev = LAST_SUBMIT_RC.swap(rc, Ordering::Relaxed);
        let n = SUBMIT_ERROR_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if prev != rc || (n % 120) == 1 {
            log_bytes(b"qjs-cmd-stream: submit rc=");
            log_i32_dec(rc);
            log_bytes(b"\n");
        }
    }
}

fn flush_active_frame(st: &mut FrameState) {
    if !st.active {
        return;
    }
    if st.batches.is_empty() {
        submit_rgb_triangles(st.frame_clear_rgb, None);
    } else if st.batches.len() == 1 {
        submit_rgb_triangles(st.frame_clear_rgb, Some(st.batches[0].vtx.as_slice()));
    } else {
        let merged_len: usize = st.batches.iter().map(|b| b.vtx.len()).sum();
        let mut merged = Vec::with_capacity(merged_len);
        for batch in st.batches.iter() {
            merged.extend_from_slice(batch.vtx.as_slice());
        }
        submit_rgb_triangles(st.frame_clear_rgb, Some(merged.as_slice()));
    }
    st.batches.clear();
    st.active = false;
}

fn current_pipeline_key(st: &FrameState) -> PipelineKey {
    PipelineKey {
        viewport_w: st.viewport_w,
        viewport_h: st.viewport_h,
        blend: st.blend,
    }
}

pub(crate) fn enqueue(cmd: CmdStreamCommand) {
    let mut st = FRAME_STATE.lock();
    match cmd {
        CmdStreamCommand::BeginFrame => {
            flush_active_frame(&mut st);
            st.active = true;
            st.frame_clear_rgb = st.clear_rgb;
        }
        CmdStreamCommand::SetClearColor { clear_rgb } => {
            if st.clear_rgb != clear_rgb {
                st.clear_rgb = clear_rgb;
            }
        }
        CmdStreamCommand::SetViewport { w, h } => {
            if st.viewport_w != w || st.viewport_h != h {
                st.viewport_w = w;
                st.viewport_h = h;
            }
        }
        CmdStreamCommand::SetBlendEnabled { enabled } => {
            if st.blend.enabled != enabled {
                st.blend.enabled = enabled;
            }
        }
        CmdStreamCommand::SetBlendFunc {
            src_rgb,
            dst_rgb,
            src_alpha,
            dst_alpha,
        } => {
            if st.blend.src_rgb != src_rgb
                || st.blend.dst_rgb != dst_rgb
                || st.blend.src_alpha != src_alpha
                || st.blend.dst_alpha != dst_alpha
            {
                st.blend.src_rgb = src_rgb;
                st.blend.dst_rgb = dst_rgb;
                st.blend.src_alpha = src_alpha;
                st.blend.dst_alpha = dst_alpha;
            }
        }
        CmdStreamCommand::SetBlendEquation { rgb, alpha } => {
            if st.blend.eq_rgb != rgb || st.blend.eq_alpha != alpha {
                st.blend.eq_rgb = rgb;
                st.blend.eq_alpha = alpha;
            }
        }
        CmdStreamCommand::DrawTriangles { vertices } => {
            if vertices.is_empty() {
                return;
            }
            if !st.active {
                st.active = true;
                st.frame_clear_rgb = st.clear_rgb;
            }
            let key = current_pipeline_key(&st);
            if let Some(last) = st.batches.last_mut() {
                if last.key == key {
                    last.vtx.extend_from_slice(vertices.as_slice());
                    return;
                }
            }
            st.batches.push(DrawBatch { key, vtx: vertices });
        }
        CmdStreamCommand::EndFrame => {
            flush_active_frame(&mut st);
        }
    }
}
