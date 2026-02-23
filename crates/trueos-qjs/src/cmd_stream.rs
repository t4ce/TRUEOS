extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use spin::Mutex;

unsafe extern "C" {
    fn trueos_cabi_gfx_draw_rgb_triangles(
        clear_rgb: u32,
        vtx_ptr: *const u8,
        vtx_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_begin_frame(clear_rgb: u32) -> i32;
    fn trueos_cabi_gfx_draw_rgb_triangles_no_present(vtx_ptr: *const u8, vtx_len: usize) -> i32;
    fn trueos_cabi_gfx_draw_tex_triangles_no_present(
        tex_id: u32,
        vtx_ptr: *const u8,
        vtx_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_set_sampler(wrap_s: u32, wrap_t: u32, min_filter: u32, mag_filter: u32)
        -> i32;
    fn trueos_cabi_gfx_set_blend(
        enabled: u32,
        src_rgb: u32,
        dst_rgb: u32,
        src_alpha: u32,
        dst_alpha: u32,
        eq_rgb: u32,
        eq_alpha: u32,
    ) -> i32;
    fn trueos_cabi_gfx_end_frame() -> i32;
    fn trueos_cabi_gfx_upload_texture_rgba(
        tex_id: u32,
        width: u32,
        height: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;

    fn trueos_cabi_gfx_frame_done_signal(bits: u32);
    fn trueos_cabi_gfx_frame_done_is_ready() -> u32;
    fn trueos_cabi_gfx_frame_done_consume_if_ready() -> u32;
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

static LAST_SUBMIT_RC: AtomicI32 = AtomicI32::new(i32::MIN);
static SUBMIT_ERROR_COUNT: AtomicU32 = AtomicU32::new(0);
static FRAME_SEQ: AtomicU32 = AtomicU32::new(0);
static UPLOAD_OK_COUNT: AtomicU32 = AtomicU32::new(0);

// When the gfx backend rejects submits (surface/io.rs returns -11), back off for a
// few ticks to avoid spamming submit attempts that keep the screen stuck on the
// last successful clear.
static SUBMIT_COOLDOWN: AtomicU32 = AtomicU32::new(0);

const FRAME_DONE_SRC_WEBGL: u32 = 1 << 0;

#[inline]
fn log_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(1, bytes.as_ptr(), bytes.len()) };
}

#[inline]
fn log_i32_dec(v: i32) {
    // Minimal decimal formatter (no alloc).
    let mut n = v as i64;
    if n == 0 {
        log_bytes(b"0");
        return;
    }
    if n < 0 {
        log_bytes(b"-");
        n = -n;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while n != 0 {
        let d = (n % 10) as u8;
        n /= 10;
        i = i.saturating_sub(1);
        buf[i] = b'0' + d;
    }
    log_bytes(&buf[i..]);
}

pub(crate) enum CmdStreamCommand {
    BeginFrame,
    SetClearColor { clear_rgb: u32 },
    SetViewport { w: i32, h: i32 },

    // Blend state (tracked for future; not all backends consume it yet).
    SetBlendEnabled { enabled: bool },
    SetBlendFunc {
        src_rgb: u32,
        dst_rgb: u32,
        src_alpha: u32,
        dst_alpha: u32,
    },
    SetBlendEquation { rgb: u32, alpha: u32 },

    // Sampler state (WebGL subset).
    SetSampler {
        wrap_s: u32,
        wrap_t: u32,
        min_filter: u32,
        mag_filter: u32,
    },

    DrawTriangles { vertices: Vec<u8> },
    DrawTrianglesTex { tex_id: u32, vertices: Vec<u8> },
    UploadTexture {
        tex_id: u32,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
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
            src_rgb: 1,       // ONE
            dst_rgb: 0,       // ZERO
            src_alpha: 1,     // ONE
            dst_alpha: 0,     // ZERO
            eq_rgb: 0x8006,   // FUNC_ADD
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
    // Only used when textured=true.
    sampler_wrap_s: u32,
    sampler_wrap_t: u32,
    sampler_min: u32,
    sampler_mag: u32,
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
    end_requested: bool,
    suppress: bool,
    // GL-style clear color state
    clear_rgb: u32,
    // Captured at BeginFrame and used when submitting that frame.
    frame_clear_rgb: u32,
    viewport_w: i32,
    viewport_h: i32,
    blend: BlendState,
    sampler_wrap_s: u32,
    sampler_wrap_t: u32,
    sampler_min: u32,
    sampler_mag: u32,
    batches: Vec<DrawBatch>,
    merge_scratch: Vec<u8>,
}

static FRAME_STATE: Mutex<FrameState> = Mutex::new(FrameState {
    active: false,
    end_requested: false,
    suppress: false,
    clear_rgb: 0x00ff_ffff,
    frame_clear_rgb: 0x00ff_ffff,
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
    // WebGL defaults: repeat + nearest-mipmap-linear/linear; Pixi typically overrides.
    // We store only a simplified subset.
    sampler_wrap_s: 0,
    sampler_wrap_t: 0,
    sampler_min: 0,
    sampler_mag: 0,
    batches: Vec::new(),
    merge_scratch: Vec::new(),
});

fn submit_rgb_triangles(clear_rgb: u32, vertices: Option<&[u8]>) {
    let rc = match vertices {
        Some(vtx) => unsafe {
            trueos_cabi_gfx_draw_rgb_triangles(clear_rgb, vtx.as_ptr(), vtx.len())
        },
        None => unsafe { trueos_cabi_gfx_draw_rgb_triangles(clear_rgb, core::ptr::null(), 0) },
    };
    if rc != 0 {
        let prev = LAST_SUBMIT_RC.swap(rc, Ordering::Relaxed);
        let n = SUBMIT_ERROR_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if prev != rc || (n % 120) == 1 {
            log_bytes(b"qjs-webgl-cmd-stream: submit rc=");
            log_i32_dec(rc);
            log_bytes(b"\n");
        }
    }
}

fn flush_active_frame(st: &mut FrameState) {
    if !st.active {
        return;
    }

    // Best-effort frame number for debug logs in this function.
    // `FRAME_SEQ` is incremented at the end of flush; add 1 for the in-progress frame.
    let dbg_seq = FRAME_SEQ.load(Ordering::Relaxed).wrapping_add(1);

    // If we're in cooldown, drop the frame without calling into gfx.
    if SUBMIT_COOLDOWN.load(Ordering::Relaxed) != 0 {
        st.batches.clear();
        st.active = false;
        st.end_requested = false;
        st.suppress = false;
        return;
    }

    let rc = unsafe { trueos_cabi_gfx_begin_frame(st.frame_clear_rgb) };
    if rc != 0 && rc != -3 {
        let prev = LAST_SUBMIT_RC.swap(rc, Ordering::Relaxed);
        let n = SUBMIT_ERROR_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if prev != rc || (n % 120) == 1 {
            log_bytes(b"qjs-webgl-cmd-stream: begin rc=");
            log_i32_dec(rc);
            log_bytes(b"\n");
        }
    }

    let mut last_blend: Option<BlendState> = None;

    for batch in st.batches.iter() {
        if last_blend != Some(batch.key.blend) {
            let b = batch.key.blend;
            let _ = unsafe {
                trueos_cabi_gfx_set_blend(
                    if b.enabled { 1 } else { 0 },
                    b.src_rgb,
                    b.dst_rgb,
                    b.src_alpha,
                    b.dst_alpha,
                    b.eq_rgb,
                    b.eq_alpha,
                )
            };
            last_blend = Some(b);
        }
        let rc = match batch.kind {
            DrawKind::Rgb => unsafe {
                trueos_cabi_gfx_draw_rgb_triangles_no_present(batch.vtx.as_ptr(), batch.vtx.len())
            },
            DrawKind::Tex { id } => unsafe {
                if dbg_seq <= 5 {
                    log_bytes(b"qjs-webgl-cmd-stream: draw-tex id=");
                    log_i32_dec(id as i32);
                    log_bytes(b" bytes=");
                    log_i32_dec(batch.vtx.len() as i32);
                    log_bytes(b" min=");
                    log_i32_dec(batch.key.sampler_min as i32);
                    log_bytes(b" mag=");
                    log_i32_dec(batch.key.sampler_mag as i32);
                    log_bytes(b" vp=");
                    log_i32_dec(st.viewport_w as i32);
                    log_bytes(b"x");
                    log_i32_dec(st.viewport_h as i32);
                    log_bytes(b"\n");

                    // Vertex ABI for textured draws (stride 20):
                    // f32 x, f32 y, f32 u, f32 v, u8 r, u8 g, u8 b, u8 a
                    // Dump a few vertices to diagnose black screens.
                    let stride = 20usize;
                    let count = batch.vtx.len() / stride;
                    let dump = if count < 4 { count } else { 4 };
                    for i in 0..dump {
                        let off = i * stride;
                        if off + stride > batch.vtx.len() {
                            break;
                        }
                        let xb = [batch.vtx[off + 0], batch.vtx[off + 1], batch.vtx[off + 2], batch.vtx[off + 3]];
                        let yb = [batch.vtx[off + 4], batch.vtx[off + 5], batch.vtx[off + 6], batch.vtx[off + 7]];
                        let ub = [batch.vtx[off + 8], batch.vtx[off + 9], batch.vtx[off + 10], batch.vtx[off + 11]];
                        let vb = [batch.vtx[off + 12], batch.vtx[off + 13], batch.vtx[off + 14], batch.vtx[off + 15]];
                        let x = f32::from_bits(u32::from_le_bytes(xb));
                        let y = f32::from_bits(u32::from_le_bytes(yb));
                        let u = f32::from_bits(u32::from_le_bytes(ub));
                        let v = f32::from_bits(u32::from_le_bytes(vb));
                        let r = batch.vtx[off + 16];
                        let g = batch.vtx[off + 17];
                        let b = batch.vtx[off + 18];
                        let a = batch.vtx[off + 19];

                        log_bytes(b"qjs-webgl-cmd-stream:  v");
                        log_i32_dec(i as i32);
                        log_bytes(b" xy=");
                        log_i32_dec((x * 1000.0) as i32);
                        log_bytes(b",");
                        log_i32_dec((y * 1000.0) as i32);
                        log_bytes(b" uv=");
                        log_i32_dec((u * 1000.0) as i32);
                        log_bytes(b",");
                        log_i32_dec((v * 1000.0) as i32);
                        log_bytes(b" rgba=");
                        log_i32_dec(r as i32);
                        log_bytes(b",");
                        log_i32_dec(g as i32);
                        log_bytes(b",");
                        log_i32_dec(b as i32);
                        log_bytes(b",");
                        log_i32_dec(a as i32);
                        log_bytes(b"\n");
                    }
                }
                // Apply sampler state captured in the batch key.
                let _ = trueos_cabi_gfx_set_sampler(
                    batch.key.sampler_wrap_s,
                    batch.key.sampler_wrap_t,
                    batch.key.sampler_min,
                    batch.key.sampler_mag,
                );
                trueos_cabi_gfx_draw_tex_triangles_no_present(
                    id,
                    batch.vtx.as_ptr(),
                    batch.vtx.len(),
                )
            },
        };
        if rc != 0 && rc != -6 {
            let prev = LAST_SUBMIT_RC.swap(rc, Ordering::Relaxed);
            let n = SUBMIT_ERROR_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            if prev != rc || (n % 120) == 1 {
                match batch.kind {
                    DrawKind::Rgb => log_bytes(b"qjs-webgl-cmd-stream: draw-rgb rc="),
                    DrawKind::Tex { .. } => log_bytes(b"qjs-webgl-cmd-stream: draw-tex rc="),
                }
                log_i32_dec(rc);
                log_bytes(b"\n");
            }
        }
    }

    let rc = unsafe { trueos_cabi_gfx_end_frame() };
    if rc != 0 && rc != -2 {
        let prev = LAST_SUBMIT_RC.swap(rc, Ordering::Relaxed);
        let n = SUBMIT_ERROR_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if prev != rc || (n % 120) == 1 {
            log_bytes(b"qjs-webgl-cmd-stream: end rc=");
            log_i32_dec(rc);
            log_bytes(b"\n");
        }
    }

    // Back off briefly on submit failure.
    if rc == -11 {
        SUBMIT_COOLDOWN.store(3, Ordering::Relaxed);
    }
    let seq = FRAME_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    if seq <= 10 || (seq % 20) == 0 {
        let draw_batches = st.batches.len() as u32;
        let draw_bytes: usize = st.batches.iter().map(|b| b.vtx.len()).sum();
        log_bytes(b"qjs-webgl-cmd-stream: frame seq=");
        log_i32_dec(seq as i32);
        log_bytes(b" batches=");
        log_i32_dec(draw_batches as i32);
        log_bytes(b" bytes=");
        log_i32_dec(draw_bytes as i32);
        log_bytes(b"\n");
    }
    st.batches.clear();
    st.active = false;
    st.end_requested = false;
    st.suppress = false;
}

#[inline]
fn maybe_flush_on_frame_done(st: &mut FrameState) {
    if !st.end_requested {
        return;
    }

    unsafe { trueos_cabi_gfx_frame_done_signal(FRAME_DONE_SRC_WEBGL) };

    if unsafe { trueos_cabi_gfx_frame_done_is_ready() } == 0 {
        return;
    }

    // Consume the boundary before flushing so the next frame can begin immediately.
    let _seq = unsafe { trueos_cabi_gfx_frame_done_consume_if_ready() };
    flush_active_frame(st);
}

fn current_pipeline_key(st: &FrameState, textured: bool, tex_id: u32) -> PipelineKey {
    PipelineKey {
        viewport_w: st.viewport_w,
        viewport_h: st.viewport_h,
        blend: st.blend,
        textured,
        tex_id,
        sampler_wrap_s: st.sampler_wrap_s,
        sampler_wrap_t: st.sampler_wrap_t,
        sampler_min: st.sampler_min,
        sampler_mag: st.sampler_mag,
    }
}

pub(crate) fn enqueue(cmd: CmdStreamCommand) {
    let mut st = FRAME_STATE.lock();

    // Decrement cooldown once per tick-ish (we don't have a clock here, so treat
    // any cmd stream traffic as a tick). This is intentionally tiny.
    let cd = SUBMIT_COOLDOWN.load(Ordering::Relaxed);
    if cd != 0 {
        let _ = SUBMIT_COOLDOWN.compare_exchange(cd, cd - 1, Ordering::Relaxed, Ordering::Relaxed);
    }

    match cmd {
        CmdStreamCommand::BeginFrame => {
            // If backend is currently rejecting submits, suppress this whole frame.
            if SUBMIT_COOLDOWN.load(Ordering::Relaxed) != 0 {
                st.suppress = true;
                st.active = false;
                st.end_requested = false;
                st.batches.clear();
                return;
            }

            // If the previous frame is waiting on the backend boundary, don't
            // force-flush here (that can time out and lead to missing presents).
            // Instead, try to flush only when the frame-done signal is ready.
            if st.active && st.end_requested {
                maybe_flush_on_frame_done(&mut st);
                // Still not ready: drop this BeginFrame (and any following draw
                // calls should be ignored until the pending frame flushes).
                if st.active {
                    return;
                }
            }

            // Start capturing a new frame.
            st.active = true;
            st.end_requested = false;
            st.suppress = false;
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
        CmdStreamCommand::SetSampler {
            wrap_s,
            wrap_t,
            min_filter,
            mag_filter,
        } => {
            st.sampler_wrap_s = wrap_s;
            st.sampler_wrap_t = wrap_t;
            st.sampler_min = min_filter;
            st.sampler_mag = mag_filter;
        }
        CmdStreamCommand::DrawTriangles { vertices } => {
            if vertices.is_empty() {
                return;
            }
            if st.suppress {
                return;
            }
            // If we've already seen EndFrame but haven't flushed yet, ignore any
            // new draws so we don't accumulate unbounded batches.
            if st.end_requested {
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
            if st.suppress {
                return;
            }
            if st.end_requested {
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
                    log_bytes(b"qjs-webgl-cmd-stream: tex upload rc=");
                    log_i32_dec(rc);
                    log_bytes(b"\n");
                }
            } else {
                // Log only the first few successful uploads to validate the pipeline.
                let n = UPLOAD_OK_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                if n <= 16 {
                    log_bytes(b"qjs-webgl-cmd-stream: tex upload ok id=");
                    log_i32_dec(tex_id as i32);
                    log_bytes(b" w=");
                    log_i32_dec(width as i32);
                    log_bytes(b" h=");
                    log_i32_dec(height as i32);
                    log_bytes(b"\n");
                }
            }
        }
        CmdStreamCommand::EndFrame => {
            if st.suppress {
                st.suppress = false;
                st.active = false;
                st.end_requested = false;
                st.batches.clear();
                return;
            }
            st.end_requested = true;
            maybe_flush_on_frame_done(&mut st);
        }
    }
}
