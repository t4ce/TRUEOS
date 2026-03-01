use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_executor::{SpawnError, Spawner};
use embassy_time::{Duration as EmbassyDuration, Timer};
use embassy_time_driver::{TICK_HZ, now};

// NOTE: This file is intended to become the single source of truth for Embassy task startup.

/// Central task orchestrator ("FSM spawn service").
///
/// Ideal-world model:
/// - One file owns the boot task registry (what runs + under which readiness conditions).
/// - Individual tasks can still contain internal gating today; later we can delete those
///   once this registry is trusted.
/// - Readiness is monotonic, so this service only ever adds tasks; it never stops them.
///
/// This is intentionally simple: a small polling loop over a static registry.

struct TaskSpec {
    name: &'static str,
    disabled: bool,
    required: u32,
    started: &'static AtomicBool,
    spawn: fn(Spawner) -> SpawnAttempt,
}

enum SpawnAttempt {
    Spawned,
    Skipped,
    Failed(SpawnError),
}

// --- one-shot guards (kept here so boot/task wiring is centralized) ---

static VGA_FONT_CACHE_STARTED: AtomicBool = AtomicBool::new(false);
static TRUEOSFS_MOUNT_SERVICE_STARTED: AtomicBool = AtomicBool::new(false);

static NET_POLL_STARTED: AtomicBool = AtomicBool::new(false);
static NET_SERVICE_STARTED: AtomicBool = AtomicBool::new(false);
static TLS_SOCKET_SERVICE_STARTED: AtomicBool = AtomicBool::new(false);
static NET_SHELL_STARTED: AtomicBool = AtomicBool::new(false);
static AI_TCP_BRIDGE_STARTED: AtomicBool = AtomicBool::new(false);
static AI_QJS_REPL_STARTED: AtomicBool = AtomicBool::new(false);
static HTTP_TRUEOSFS_STARTED: AtomicBool = AtomicBool::new(false);
static FTP_SERVER_STARTED: AtomicBool = AtomicBool::new(false);

static TGA_TASK_STARTED: AtomicBool = AtomicBool::new(false);

static GFX_VIRGL_READY_TASK_STARTED: AtomicBool = AtomicBool::new(false);
static GFX_VGA_SWAP_FORWARD_STARTED: AtomicBool = AtomicBool::new(false);
static WGPU_TEXT_STARTED: AtomicBool = AtomicBool::new(false);
static WEBGPU_PIXI_SMOKE_STARTED: AtomicBool = AtomicBool::new(false);
static WEBGPU_BROWSER_STARTED: AtomicBool = AtomicBool::new(false);
static GFX_MATMUL_DEMO_STARTED: AtomicBool = AtomicBool::new(false);

static USB_CONTROLLER_TASKS_STARTED: AtomicBool = AtomicBool::new(false);
static HID_INPUT_LOGGER_STARTED: AtomicBool = AtomicBool::new(false);
static UAC_EVENT_DRAIN_STARTED: AtomicBool = AtomicBool::new(false);
static UAC_SONG_STARTED: AtomicBool = AtomicBool::new(false);
static VLEDS_MUX_STARTED: AtomicBool = AtomicBool::new(false);
static VLEDS_CYCLE_STARTED: AtomicBool = AtomicBool::new(false);
static TRUEKEY_DRAIN_STARTED: AtomicBool = AtomicBool::new(false);
static PIANO_DRAIN_STARTED: AtomicBool = AtomicBool::new(false);

static BOOT_WS_SMOKE_STARTED: AtomicBool = AtomicBool::new(false);
static BOOT_NETBENCH_STARTED: AtomicBool = AtomicBool::new(false);

static UART_SHELL_STARTED: AtomicBool = AtomicBool::new(false);
static NET_TCP_SHELL_STARTED: AtomicBool = AtomicBool::new(false);
static GFX_VIRGL_READY_DELAY_DEADLINE_TICKS: AtomicU64 = AtomicU64::new(0);

// --- spawn wrappers (keep per-task logic out of main.rs) ---

fn spawn_vga_font_cache(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::vga::init_font_cache_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_trueosfs_mount_service(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::v::fs::trueosfs::mount_service_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_net_poll_tasks(spawner: Spawner) -> SpawnAttempt {
    // Some drivers may fail to report a MAC early; treat any detected NIC as usable.
    let count = crate::net::device_count();
    if count == 0 {
        return SpawnAttempt::Skipped;
    }
    for idx in 0..count {
        if let Err(e) = spawner.spawn(crate::net::adapter::net_poll_task(idx)) {
            crate::log!("net: spawn net_poll_task({}) failed: {:?}\n", idx, e);
        }
    }
    SpawnAttempt::Spawned
}

fn spawn_net_service(spawner: Spawner) -> SpawnAttempt {
    if crate::net::device_count() == 0 {
        return SpawnAttempt::Skipped;
    }
    match spawner.spawn(crate::net::adapter::net_service_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_tls_socket_service(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::net::tls_socket::tls_socket_service_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_net_shell(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::shell::backends::net_tcp::net_shell_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_ai_tcp_bridge(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::shell::backends::ai_tcp::ai_tcp_bridge_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_ai_qjs_repl(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::shell::backends::ai_tcp::ai_qjs_repl_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_http_trueosfs(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    SpawnAttempt::Skipped
}

fn spawn_ftp_server(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::v::net::ftp::ftp_server_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_tga_task(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::tga::tga_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

#[embassy_executor::task]
async fn gfx_virgl_ready_task() {
    crate::gfx::init(crate::limine::framebuffer_response());

    #[cfg(not(feature = "gfx_virgl"))]
    {
        return;
    }

    #[cfg(feature = "gfx_virgl")]
    {
        for _ in 0..400 {
            if crate::v::readiness::is_set(crate::v::readiness::GFX_VIRGL_READY) {
                return;
            }
            if crate::gfx::is_virgl_present_cached()
                && (crate::gfx::is_virgl_active() || crate::gfx::switch_to_virgl())
            {
                return;
            }
            Timer::after(EmbassyDuration::from_millis(25)).await;
        }
        crate::log!("gfx-virgl-ready: timeout\n");
    }
}

fn spawn_gfx_virgl_ready_task(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(gfx_virgl_ready_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

#[embassy_executor::task]
async fn gfx_vga_swap_forward_task() {
    #[cfg(not(feature = "gfx_virgl"))]
    {
        return;
    }

    #[cfg(feature = "gfx_virgl")]
    {
        const TEX_ID: u32 = 2;
        let mut last_rc: i32 = 0;
        let mut stage: alloc::vec::Vec<u32> = alloc::vec::Vec::new();
        let mut stage_w: usize = 0;
        let mut stage_h: usize = 0;

        loop {
            if !matches!(
                crate::gfx::present_owner(),
                crate::gfx::PresentOwner::Forward
            ) {
                Timer::after(EmbassyDuration::from_millis(16)).await;
                continue;
            }

            let copied = crate::gfx::with_cpu_backbuffer_mut(|pixels, w, h| {
                let need = w.saturating_mul(h);
                if need == 0 || pixels.len() < need {
                    return false;
                }
                if stage.len() != need {
                    stage.resize(need, 0);
                }
                stage[..need].copy_from_slice(&pixels[..need]);
                stage_w = w;
                stage_h = h;
                true
            })
            .unwrap_or(false);
            if !copied {
                Timer::after(EmbassyDuration::from_millis(16)).await;
                continue;
            }

            let data_len = stage.len().saturating_mul(core::mem::size_of::<u32>());
            let rc = unsafe {
                crate::surface::io::cabi::trueos_cabi_gfx_present_rgba(
                    TEX_ID,
                    stage_w as u32,
                    stage_h as u32,
                    stage.as_ptr() as *const u8,
                    data_len,
                    0xFFFFFF,
                )
            };

            if rc != 0 && rc != last_rc {
                crate::log!("gfx-vga-swap-forward: present rc={}\n", rc);
                last_rc = rc;
            } else if rc == 0 {
                last_rc = 0;
            }

            Timer::after(EmbassyDuration::from_millis(16)).await;
        }
    }
}

fn spawn_gfx_vga_swap_forward_task(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(gfx_vga_swap_forward_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

#[inline]
fn task_start_delay(spec: &TaskSpec) -> Option<(u64, &'static AtomicU64)> {
    match spec.name {
        "gfx-virgl-ready" => Some((2500, &GFX_VIRGL_READY_DELAY_DEADLINE_TICKS)),
        _ => None,
    }
}

struct MeshTex2d {
    positions: &'static [(f32, f32)],
    uvs: &'static [(f32, f32)],
    indices: &'static [u16],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TexVertex {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

fn emit_mesh_tex(
    out: &mut [TexVertex],
    mesh: &MeshTex2d,
    angle_rad: f32,
    tx: f32,
    ty: f32,
    scale: f32,
    color: (u8, u8, u8, u8),
) -> usize {
    let (s, c) = libm::sincosf(angle_rad);
    let mut w = 0usize;
    for tri in mesh.indices.chunks_exact(3) {
        for &idx in tri {
            let i = idx as usize;
            let (px, py) = mesh.positions[i];
            let (u, v) = mesh.uvs[i];
            let sx = px * scale;
            let sy = py * scale;
            out[w] = TexVertex {
                x: sx * c - sy * s + tx,
                y: sx * s + sy * c + ty,
                u,
                v,
                r: color.0,
                g: color.1,
                b: color.2,
                a: color.3,
            };
            w += 1;
        }
    }
    w
}

#[embassy_executor::task]
async fn webgpu_mesh_task() {
    crate::gfx::init(crate::limine::framebuffer_response());
    #[cfg(not(feature = "gfx_virgl"))]
    {
        crate::log!("webgpu-text: gfx_virgl feature disabled\n");
        return;
    }
    #[cfg(feature = "gfx_virgl")]
    {
        // Boot ordering can race: spawn-service may start this task before
        // virtio-gpu enumeration/bring-up is fully observable.
        let mut ready = false;
        for _ in 0..200 {
            if crate::gfx::is_virgl_present_cached()
                && (crate::gfx::is_virgl_active() || crate::gfx::switch_to_virgl())
            {
                ready = true;
                break;
            }
            Timer::after(EmbassyDuration::from_millis(25)).await;
        }
        if !ready {
            crate::log!("webgpu-text: virgl not ready (timeout)\n");
            return;
        }
    }

    // Reserve presenter ownership for the text demo so forward-task doesn't
    // race and overwrite frames during the 2.5s text window.
    crate::gfx::set_present_owner(crate::gfx::PresentOwner::Pixi);

    const TEX_ID: u32 = 1;
    let atlas = crate::gfx::webgpu_font::font_atlas_large_view();
    let tex_px = (atlas.width as usize).saturating_mul(atlas.height as usize);
    let mut tex_rgba = alloc::vec![0u8; tex_px.saturating_mul(4)];
    for (i, &a) in atlas.alpha.iter().enumerate() {
        let o = i.saturating_mul(4);
        tex_rgba[o] = 255;
        tex_rgba[o + 1] = 255;
        tex_rgba[o + 2] = 255;
        tex_rgba[o + 3] = a;
    }
    let _ = unsafe {
        crate::surface::io::cabi::trueos_cabi_gfx_upload_texture_rgba(
            TEX_ID,
            atlas.width,
            atlas.height,
            tex_rgba.as_ptr(),
            tex_rgba.len(),
        )
    };
    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe {
        crate::surface::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0)
    };

    let mut verts = [TexVertex {
        x: 0.0,
        y: 0.0,
        u: 0.0,
        v: 0.0,
        r: 16,
        g: 16,
        b: 16,
        a: 255,
    }; 6 * 96];

    const MSG: &[u8] = b"TRUEOS WEBGPU TEXT";
    let mut n = 0usize;
    let mut pen_x = -0.92f32;
    let pen_y = 0.10f32;
    let (fb_w, fb_h) = crate::limine::framebuffer_response()
        .and_then(|resp| resp.framebuffers().next())
        .map(|fb| (fb.width() as f32, fb.height() as f32))
        .unwrap_or((1024.0, 768.0));
    let px_to_ndc_x = 2.0f32 / fb_w.max(1.0);
    let px_to_ndc_y = 2.0f32 / fb_h.max(1.0);
    let glyph_h_ndc = atlas.cell_h as f32 * px_to_ndc_y;
    let atlas_w = atlas.width as f32;
    let atlas_h = atlas.height as f32;
    let grid_w = atlas.grid_w.max(1);
    let fallback = atlas.index.get(b'?' as usize).copied().unwrap_or(0);
    for &ch in MSG {
        if ch == b' ' {
            pen_x += atlas.cell_w as f32 * 0.45 * px_to_ndc_x;
            continue;
        }
        let mut slot = atlas.index.get(ch as usize).copied().unwrap_or(fallback);
        if slot == u16::MAX {
            slot = fallback;
        }
        let glyph_w_px = atlas
            .widths
            .get(slot as usize)
            .copied()
            .unwrap_or(atlas.cell_w as u8) as f32;
        let glyph_w_ndc = glyph_w_px * px_to_ndc_x;

        let sx = (slot as u32) % grid_w;
        let sy = (slot as u32) / grid_w;
        let px0 = (sx * atlas.cell_w) as f32;
        let py0 = (sy * atlas.cell_h) as f32;
        let u0 = px0 / atlas_w;
        let v0 = py0 / atlas_h;
        let u1 = (px0 + glyph_w_px) / atlas_w;
        let v1 = (py0 + atlas.cell_h as f32) / atlas_h;

        let x0 = pen_x;
        let y0 = pen_y;
        let x1 = x0 + glyph_w_ndc;
        let y1 = y0 + glyph_h_ndc;
        if n + 6 > verts.len() {
            break;
        }
        let c = (16u8, 16u8, 16u8, 255u8);
        verts[n] = TexVertex {
            x: x0,
            y: y0,
            u: u0,
            v: v1,
            r: c.0,
            g: c.1,
            b: c.2,
            a: c.3,
        };
        verts[n + 1] = TexVertex {
            x: x1,
            y: y0,
            u: u1,
            v: v1,
            r: c.0,
            g: c.1,
            b: c.2,
            a: c.3,
        };
        verts[n + 2] = TexVertex {
            x: x1,
            y: y1,
            u: u1,
            v: v0,
            r: c.0,
            g: c.1,
            b: c.2,
            a: c.3,
        };
        verts[n + 3] = TexVertex {
            x: x0,
            y: y0,
            u: u0,
            v: v1,
            r: c.0,
            g: c.1,
            b: c.2,
            a: c.3,
        };
        verts[n + 4] = TexVertex {
            x: x1,
            y: y1,
            u: u1,
            v: v0,
            r: c.0,
            g: c.1,
            b: c.2,
            a: c.3,
        };
        verts[n + 5] = TexVertex {
            x: x0,
            y: y1,
            u: u0,
            v: v0,
            r: c.0,
            g: c.1,
            b: c.2,
            a: c.3,
        };
        n += 6;
        pen_x += glyph_w_px * 1.10 * px_to_ndc_x;
    }
    let ptr = verts.as_ptr() as *const u8;
    let len = n.saturating_mul(core::mem::size_of::<TexVertex>());
    let swap_delay_ms: u64 = 2500;
    let swap_ticks = {
        let hz = TICK_HZ;
        let ticks = if hz == 0 {
            0
        } else {
            swap_delay_ms.saturating_mul(hz).div_ceil(1000).max(1)
        };
        ticks
    };
    let phase_a_start = now();
    let mut swapped = false;
    let mut hold_start_ticks: u64 = 0;
    let hold_ms: u64 = 2500;
    let hold_ticks = if TICK_HZ == 0 {
        0
    } else {
        hold_ms.saturating_mul(TICK_HZ).div_ceil(1000).max(1)
    };
    let mut single_submitted = false;
    loop {
        Timer::after(EmbassyDuration::from_millis(16)).await;
        let now_ticks = now();
        let elapsed_ticks = now_ticks.saturating_sub(phase_a_start);
        if !swapped && swap_ticks != 0 && elapsed_ticks >= swap_ticks {
            let elapsed_ms = if TICK_HZ == 0 {
                0
            } else {
                elapsed_ticks.saturating_mul(1000).div_ceil(TICK_HZ)
            };
            crate::log!(
                "wgpu_text: swap VGA->GFX after {}ms (target={}ms)\n",
                elapsed_ms,
                swap_delay_ms
            );
            if !single_submitted {
                let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_begin_frame(0xFFFFFF) };
                if len != 0 {
                    let _ = unsafe {
                        crate::surface::io::cabi::trueos_cabi_gfx_draw_tex_triangles_no_present(
                            TEX_ID, ptr, len,
                        )
                    };
                }
                let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_end_frame() };
                single_submitted = true;
                let _ = trueos_qjs::pixi::smoke::preload_pixi_cdn_once().await;
            }
            crate::log!("wgpu_text: hold GFX mode for 2500ms\n");
            swapped = true;
            hold_start_ticks = now_ticks;
        }
        if swapped && hold_ticks != 0 && now_ticks.saturating_sub(hold_start_ticks) >= hold_ticks {
            crate::log!("wgpu_text: text phase done; handoff to VGA-buffer-on-GFX\n");
            crate::gfx::set_present_owner(crate::gfx::PresentOwner::Forward);
            crate::v::readiness::set(crate::v::readiness::WGPU_TEXT_DONE);
            return;
        }
    }
}

fn spawn_wgpu_text(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(webgpu_mesh_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_webgpu_pixi_smoke(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(trueos_qjs::pixi::smoke::boot_pixi_scene_smoke_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_webgpu_browser(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(trueos_qjs::pixi::boot_browser()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn fill_demo_matrix(out: &mut [f32], n: usize, seed: f32) {
    let mut i = 0usize;
    while i < n {
        let mut j = 0usize;
        while j < n {
            let idx = i * n + j;
            out[idx] = libm::sinf((i as f32) * 0.013 + (j as f32) * 0.021 + seed);
            j += 1;
        }
        i += 1;
    }
}

fn matmul_square(a: &[f32], b: &[f32], c: &mut [f32], n: usize) {
    let mut i = 0usize;
    while i < n {
        let mut j = 0usize;
        while j < n {
            let mut acc = 0.0f32;
            let mut k = 0usize;
            while k < n {
                acc += a[i * n + k] * b[k * n + j];
                k += 1;
            }
            c[i * n + j] = acc;
            j += 1;
        }
        i += 1;
    }
}

#[embassy_executor::task]
async fn gfx_matmul_demo_task() {
    const N: usize = 64;
    const M: usize = 32;

    crate::log!("gfx-matmul: begin\n");

    let mut a = alloc::vec![0.0f32; N * N];
    let mut b = alloc::vec![0.0f32; N * N];
    let mut c = alloc::vec![0.0f32; N * N];
    fill_demo_matrix(&mut a, N, 0.11);
    fill_demo_matrix(&mut b, N, 0.37);

    // Matrix-Matrix multiplication is embarrassingly parallel:
    // each C[i,j] can be computed independently.
    matmul_square(&a, &b, &mut c, N);

    // Also run a smaller size to show variable workloads.
    let mut a2 = alloc::vec![0.0f32; M * M];
    let mut b2 = alloc::vec![0.0f32; M * M];
    let mut c2 = alloc::vec![0.0f32; M * M];
    fill_demo_matrix(&mut a2, M, 0.07);
    fill_demo_matrix(&mut b2, M, 0.19);
    matmul_square(&a2, &b2, &mut c2, M);

    // Log compact checksums so work is observable and not optimized away.
    let mut s1 = 0.0f32;
    for v in &c {
        s1 += *v;
    }
    let mut s2 = 0.0f32;
    for v in &c2 {
        s2 += *v;
    }
    crate::log!(
        "gfx-matmul: done N={} sum={:.5} M={} sum={:.5}\n",
        N,
        s1,
        M,
        s2
    );
}

fn spawn_gfx_matmul_demo(spawner: Spawner) -> SpawnAttempt {
    #[cfg(not(feature = "gfx_virgl"))]
    {
        let _ = spawner;
        return SpawnAttempt::Skipped;
    }

    #[cfg(feature = "gfx_virgl")]
    {
        crate::gfx::init(crate::limine::framebuffer_response());
        if !crate::gfx::is_virgl_present_cached() {
            return SpawnAttempt::Skipped;
        }
        if !crate::gfx::is_virgl_active() {
            return SpawnAttempt::Skipped;
        }
        match spawner.spawn(gfx_matmul_demo_task()) {
            Ok(()) => SpawnAttempt::Spawned,
            Err(e) => SpawnAttempt::Failed(e),
        }
    }
}

fn spawn_usb_controller_tasks(spawner: Spawner) -> SpawnAttempt {
    for info in crate::usb::xhci::xhc_list().iter().copied() {
        // reads from hardware into dma buffs
        let _ = spawner.spawn(crate::usb::xhci::poll_task(info));
        // reads from our dma buffs into usb rings
        let _ = spawner.spawn(crate::usb::poll_task(info));
        // Single long-lived scout per controller. Rescans are triggered via a flag.
        let _ = spawner.spawn(crate::usb::usb_scout_service(info));
    }
    SpawnAttempt::Spawned
}

fn spawn_hid_input_logger(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::usb::hid::input_logger()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_uac_song(spawner: Spawner) -> SpawnAttempt {
    let Some(ap1_spawner) = crate::runtime::first_ap_spawner() else {
        // Wait until AP1 executor is online so this task runs there.
        return SpawnAttempt::Skipped;
    };
    let _ = spawner; // keep signature stable; song intentionally targets AP1.
    match ap1_spawner.spawn(crate::usb::uac::song_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_uac_event_drain(spawner: Spawner) -> SpawnAttempt {
    let Some(ap1_spawner) = crate::runtime::first_ap_spawner() else {
        // Wait until AP1 executor is online so this task runs there.
        return SpawnAttempt::Skipped;
    };
    let _ = spawner; // keep signature stable; drain intentionally targets AP1.
    match ap1_spawner.spawn(crate::usb::uac::event_drain_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_vleds_mux(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::v::leds::task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_vleds_cycle(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::v::leds::color_cycle_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_truekey_drain(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::usb::truekey::drain_loop()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_piano_drain(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::usb::midi::piano_drain_loop()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_boot_ws_smoke(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    SpawnAttempt::Skipped
}

fn spawn_boot_netbench(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    SpawnAttempt::Skipped
}

fn spawn_uart_shell(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::shell::task(
        spawner,
        &crate::shell::UART1_COM1_BACKEND,
    )) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_net_tcp_shell(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::shell::task(
        spawner,
        &crate::shell::NET_TCP_SHELL_BACKEND,
    )) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

// --- registry ---

const HID_ANY_CLAIMED: u32 = crate::v::readiness::HID_KEYBOARD_CLAIMED;

const NET_AND_ROOT_READY: u32 =
    crate::v::readiness::NET_GATEWAY_REACHABLE | crate::v::readiness::TRUEOSFS_ROOT_MOUNTED;
const WS_BOOT_READY: u32 = crate::v::readiness::NET_GATEWAY_REACHABLE
    | crate::v::readiness::TLS_SOCKET_SERVICE_READY
    | crate::v::readiness::TRUEOSFS_ROOT_MOUNTED;

const BOOT_NETBENCH_ENABLED: bool = false;
const WGPU_TEXT_ENABLED: bool = true;
const GFX_MATMUL_DEMO_ENABLED: bool = true;

static TASKS: &[TaskSpec] = &[
    // Core background services (always-on / request-driven)
    TaskSpec {
        name: "vga-font-cache",
        disabled: false,
        required: 0,
        started: &VGA_FONT_CACHE_STARTED,
        spawn: spawn_vga_font_cache,
    },
    TaskSpec {
        name: "trueosfs-mount-service",
        disabled: false,
        required: 0,
        started: &TRUEOSFS_MOUNT_SERVICE_STARTED,
        spawn: spawn_trueosfs_mount_service,
    },
    // Network producers (may no-op if no NIC exists)
    TaskSpec {
        name: "net-poll-tasks",
        disabled: false,
        required: 0,
        started: &NET_POLL_STARTED,
        spawn: spawn_net_poll_tasks,
    },
    TaskSpec {
        name: "net-service",
        disabled: false,
        required: 0,
        started: &NET_SERVICE_STARTED,
        spawn: spawn_net_service,
    },
    // Network consumers
    TaskSpec {
        name: "tls-socket-service",
        disabled: false,
        required: crate::v::readiness::NET_GATEWAY_REACHABLE,
        started: &TLS_SOCKET_SERVICE_STARTED,
        spawn: spawn_tls_socket_service,
    },
    TaskSpec {
        name: "net-shell",
        disabled: false,
        required: 0,
        started: &NET_SHELL_STARTED,
        spawn: spawn_net_shell,
    },
    TaskSpec {
        name: "ai-tcp-bridge",
        disabled: false,
        required: 0,
        started: &AI_TCP_BRIDGE_STARTED,
        spawn: spawn_ai_tcp_bridge,
    },
    TaskSpec {
        name: "ai-qjs-repl",
        disabled: false,
        required: 0,
        started: &AI_QJS_REPL_STARTED,
        spawn: spawn_ai_qjs_repl,
    },
    TaskSpec {
        name: "http-trueosfs",
        disabled: false,
        required: NET_AND_ROOT_READY,
        started: &HTTP_TRUEOSFS_STARTED,
        spawn: spawn_http_trueosfs,
    },
    TaskSpec {
        name: "ftp-server",
        disabled: false,
        required: NET_AND_ROOT_READY,
        started: &FTP_SERVER_STARTED,
        spawn: spawn_ftp_server,
    },
    // USB core + peripherals
    TaskSpec {
        name: "tga",
        disabled: false,
        required: 0,
        started: &TGA_TASK_STARTED,
        spawn: spawn_tga_task,
    },
    TaskSpec {
        name: "gfx-virgl-ready",
        disabled: false,
        required: 0,
        started: &GFX_VIRGL_READY_TASK_STARTED,
        spawn: spawn_gfx_virgl_ready_task,
    },
    TaskSpec {
        name: "gfx-vga-swap-forward",
        disabled: false,
        required: crate::v::readiness::GFX_VIRGL_READY,
        started: &GFX_VGA_SWAP_FORWARD_STARTED,
        spawn: spawn_gfx_vga_swap_forward_task,
    },
    TaskSpec {
        name: "wgpu_text",
        disabled: !WGPU_TEXT_ENABLED,
        required: crate::v::readiness::GFX_VIRGL_READY,
        started: &WGPU_TEXT_STARTED,
        spawn: spawn_wgpu_text,
    },
    TaskSpec {
        name: "webgpu_pixi_smoke",
        disabled: true,
        required: crate::v::readiness::WGPU_TEXT_DONE,
        started: &WEBGPU_PIXI_SMOKE_STARTED,
        spawn: spawn_webgpu_pixi_smoke,
    },
    TaskSpec {
        name: "webgpu_browser",
        disabled: false,
        required: crate::v::readiness::WGPU_TEXT_DONE,
        started: &WEBGPU_BROWSER_STARTED,
        spawn: spawn_webgpu_browser,
    },
    TaskSpec {
        name: "gfx-matmul-demo",
        disabled: !GFX_MATMUL_DEMO_ENABLED,
        required: crate::v::readiness::GFX_VIRGL_READY,
        started: &GFX_MATMUL_DEMO_STARTED,
        spawn: spawn_gfx_matmul_demo,
    },
    TaskSpec {
        name: "usb-controller-tasks",
        disabled: false,
        required: 0,
        started: &USB_CONTROLLER_TASKS_STARTED,
        spawn: spawn_usb_controller_tasks,
    },
    TaskSpec {
        name: "hid-input-logger",
        disabled: false,
        required: HID_ANY_CLAIMED,
        started: &HID_INPUT_LOGGER_STARTED,
        spawn: spawn_hid_input_logger,
    },
    TaskSpec {
        name: "uac-event-drain",
        disabled: false,
        required: crate::v::readiness::UAC_ATTACHED,
        started: &UAC_EVENT_DRAIN_STARTED,
        spawn: spawn_uac_event_drain,
    },
    TaskSpec {
        name: "uac-song",
        disabled: false,
        required: crate::v::readiness::UAC_ATTACHED,
        started: &UAC_SONG_STARTED,
        spawn: spawn_uac_song,
    },
    TaskSpec {
        name: "vleds-mux",
        disabled: false,
        required: 0,
        started: &VLEDS_MUX_STARTED,
        spawn: spawn_vleds_mux,
    },
    TaskSpec {
        name: "vleds-cycle",
        disabled: false,
        required: 0,
        started: &VLEDS_CYCLE_STARTED,
        spawn: spawn_vleds_cycle,
    },
    TaskSpec {
        name: "truekey-drain",
        disabled: false,
        required: 0,
        started: &TRUEKEY_DRAIN_STARTED,
        spawn: spawn_truekey_drain,
    },
    TaskSpec {
        name: "piano-drain",
        disabled: false,
        required: crate::v::readiness::PIANO_CLAIMED,
        started: &PIANO_DRAIN_STARTED,
        spawn: spawn_piano_drain,
    },
    // Boot-time gated tasks
    TaskSpec {
        name: "boot-ws-smoke",
        disabled: true,
        required: WS_BOOT_READY,
        started: &BOOT_WS_SMOKE_STARTED,
        spawn: spawn_boot_ws_smoke,
    },
    TaskSpec {
        name: "boot-netbench",
        disabled: !BOOT_NETBENCH_ENABLED,
        required: 0,
        started: &BOOT_NETBENCH_STARTED,
        spawn: spawn_boot_netbench,
    },
    TaskSpec {
        name: "uart-shell",
        disabled: false,
        required: 0,
        started: &UART_SHELL_STARTED,
        spawn: spawn_uart_shell,
    },
    TaskSpec {
        name: "net-tcp-shell",
        disabled: false,
        required: 0,
        started: &NET_TCP_SHELL_STARTED,
        spawn: spawn_net_tcp_shell,
    },
];

#[embassy_executor::task]
pub async fn spawn_service_task(spawner: Spawner) {
    async move {
        // Poll quickly until we have started everything; then back off.
        loop {
            let ready = crate::v::readiness::mask();
            let mut pending = 0usize;
            let mut started_any = false;

            for spec in TASKS {
                if spec.disabled {
                    continue;
                }
                if (ready & spec.required) != spec.required {
                    pending += 1;
                    continue;
                }

                if let Some((delay_ms, deadline_ticks)) = task_start_delay(spec) {
                    let mut deadline = deadline_ticks.load(Ordering::Acquire);
                    if deadline == 0 {
                        let delay_ticks = if TICK_HZ == 0 {
                            1
                        } else {
                            delay_ms.saturating_mul(TICK_HZ).div_ceil(1000).max(1)
                        };
                        let target = now().saturating_add(delay_ticks);
                        let _ = deadline_ticks.compare_exchange(
                            0,
                            target,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        );
                        deadline = deadline_ticks.load(Ordering::Acquire);
                    }
                    if now() < deadline {
                        pending += 1;
                        continue;
                    }
                }

                if spec
                    .started
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    continue;
                }

                match (spec.spawn)(spawner) {
                    SpawnAttempt::Spawned => {
                        started_any = true;
                        crate::log!(
                            "spawn-svc: started {} (mask=0x{:08X})\n",
                            spec.name,
                            spec.required
                        );
                    }
                    SpawnAttempt::Skipped => {
                        // Not applicable right now (e.g. no NIC). Allow re-attempt later.
                        spec.started.store(false, Ordering::Release);
                        pending += 1;
                    }
                    SpawnAttempt::Failed(e) => {
                        // Allow retry.
                        spec.started.store(false, Ordering::Release);
                        pending += 1;
                        crate::log!(
                            "spawn-svc: failed to start {} (mask=0x{:08X}) err={:?}\n",
                            spec.name,
                            spec.required,
                            e
                        );
                    }
                }
            }

            // If we made progress, poll again quickly so chains of dependent tasks start promptly.
            // If nothing changed, back off to reduce idle overhead.
            let sleep_ms = if started_any {
                10
            } else if pending == 0 {
                250
            } else {
                50
            };
            Timer::after(EmbassyDuration::from_millis(sleep_ms)).await;
        }
    }
    .await;
}
