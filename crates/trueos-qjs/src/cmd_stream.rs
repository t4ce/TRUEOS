extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use spin::Mutex;

extern "C" {
    fn trueos_cabi_gfx_draw_rgb_triangles(clear_rgb: u32, vtx_ptr: *const u8, vtx_len: usize) -> i32;
    fn trueos_cabi_gfx_begin_frame(clear_rgb: u32) -> i32;
    fn trueos_cabi_gfx_draw_rgb_triangles_no_present(vtx_ptr: *const u8, vtx_len: usize) -> i32;
    fn trueos_cabi_gfx_draw_tex_triangles_no_present(
        tex_id: u32,
        vtx_ptr: *const u8,
        vtx_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_end_frame() -> i32;
    fn trueos_cabi_gfx_upload_texture_rgba(
        tex_id: u32,
        width: u32,
        height: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

static LAST_SUBMIT_RC: AtomicI32 = AtomicI32::new(i32::MIN);
static SUBMIT_ERROR_COUNT: AtomicU32 = AtomicU32::new(0);
static FRAME_SEQ: AtomicU32 = AtomicU32::new(0);

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
    DrawTrianglesTex { tex_id: u32, vertices: Vec<u8> },
    UploadTexture { tex_id: u32, width: u32, height: u32, rgba: Vec<u8> },
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
    textured: bool,
    tex_id: u32,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum DrawKind {
    Rgb,
    Tex { id: u32 },
}

struct DrawBatch {
    key: PipelineKey,
    kind: DrawKind,
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
    merge_scratch: Vec<u8>,
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
    merge_scratch: Vec::new(),
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
    let rc = unsafe { trueos_cabi_gfx_begin_frame(st.frame_clear_rgb) };
    if rc != 0 {
        let prev = LAST_SUBMIT_RC.swap(rc, Ordering::Relaxed);
        let n = SUBMIT_ERROR_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if prev != rc || (n % 120) == 1 {
            log_bytes(b"qjs-cmd-stream: begin rc=");
            log_i32_dec(rc);
            log_bytes(b"\n");
        }
    }

    for batch in st.batches.iter() {
        let rc = match batch.kind {
            DrawKind::Rgb => unsafe {
                trueos_cabi_gfx_draw_rgb_triangles_no_present(
                    batch.vtx.as_ptr(),
                    batch.vtx.len(),
                )
            },
            DrawKind::Tex { id } => unsafe {
                trueos_cabi_gfx_draw_tex_triangles_no_present(
                    id,
                    batch.vtx.as_ptr(),
                    batch.vtx.len(),
                )
            },
        };
        if rc != 0 {
            let prev = LAST_SUBMIT_RC.swap(rc, Ordering::Relaxed);
            let n = SUBMIT_ERROR_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            if prev != rc || (n % 120) == 1 {
                match batch.kind {
                    DrawKind::Rgb => log_bytes(b"qjs-cmd-stream: draw-rgb rc="),
                    DrawKind::Tex { .. } => log_bytes(b"qjs-cmd-stream: draw-tex rc="),
                }
                log_i32_dec(rc);
                log_bytes(b"\n");
            }
        }
    }

    let rc = unsafe { trueos_cabi_gfx_end_frame() };
    if rc != 0 {
        let prev = LAST_SUBMIT_RC.swap(rc, Ordering::Relaxed);
        let n = SUBMIT_ERROR_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if prev != rc || (n % 120) == 1 {
            log_bytes(b"qjs-cmd-stream: end rc=");
            log_i32_dec(rc);
            log_bytes(b"\n");
        }
    }
    let seq = FRAME_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    if seq == 1 {
        let draw_batches = st.batches.len() as u32;
        let draw_bytes: usize = st.batches.iter().map(|b| b.vtx.len()).sum();
        log_bytes(b"qjs-cmd-stream: frame seq=");
        log_i32_dec(seq as i32);
        log_bytes(b" batches=");
        log_i32_dec(draw_batches as i32);
        log_bytes(b" bytes=");
        log_i32_dec(draw_bytes as i32);
        log_bytes(b"\n");
    }
    st.batches.clear();
    st.active = false;
}

fn current_pipeline_key(st: &FrameState, textured: bool, tex_id: u32) -> PipelineKey {
    PipelineKey {
        viewport_w: st.viewport_w,
        viewport_h: st.viewport_h,
        blend: st.blend,
        textured,
        tex_id,
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
            let key = current_pipeline_key(&st, false, 0);
            if let Some(last) = st.batches.last_mut() {
                if last.key == key {
                    last.vtx.extend_from_slice(vertices.as_slice());
                    return;
                }
            }
            st.batches.push(DrawBatch {
                key,
                kind: DrawKind::Rgb,
                vtx: vertices,
            });
        }
        CmdStreamCommand::DrawTrianglesTex { tex_id, vertices } => {
            if vertices.is_empty() {
                return;
            }
            if !st.active {
                st.active = true;
                st.frame_clear_rgb = st.clear_rgb;
            }
            let key = current_pipeline_key(&st, true, tex_id);
            if let Some(last) = st.batches.last_mut() {
                if last.key == key {
                    last.vtx.extend_from_slice(vertices.as_slice());
                    return;
                }
            }
            st.batches.push(DrawBatch {
                key,
                kind: DrawKind::Tex { id: tex_id },
                vtx: vertices,
            });
        }
        CmdStreamCommand::UploadTexture {
            tex_id,
            width,
            height,
            rgba,
        } => {
            if rgba.is_empty() || width == 0 || height == 0 || tex_id == 0 {
                return;
            }
            let rc = unsafe {
                trueos_cabi_gfx_upload_texture_rgba(
                    tex_id,
                    width,
                    height,
                    rgba.as_ptr(),
                    rgba.len(),
                )
            };
            if rc != 0 {
                let prev = LAST_SUBMIT_RC.swap(rc, Ordering::Relaxed);
                let n = SUBMIT_ERROR_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                if prev != rc || (n % 120) == 1 {
                    log_bytes(b"qjs-cmd-stream: tex upload rc=");
                    log_i32_dec(rc);
                    log_bytes(b"\n");
                }
            }
        }
        CmdStreamCommand::EndFrame => {
            flush_active_frame(&mut st);
        }
    }
}
