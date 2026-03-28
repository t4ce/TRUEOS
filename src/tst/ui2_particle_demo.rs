extern crate alloc;

use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};
use trueos_gfx_core::{Rgba8, TEX_VERTEX_SIZE, ViewTransform, push_tex_quad_px};

use crate::gfx::particle::{ParticleSnapshot, ParticleSystem};

const UI2_PARTICLE_DEMO_TEX_ID: u32 = 4_709;
const UI2_PARTICLE_DEMO_RT_W: u32 = 512;
const UI2_PARTICLE_DEMO_RT_H: u32 = 320;
const UI2_PARTICLE_DEMO_WINDOW_X: f32 = 640.0;
const UI2_PARTICLE_DEMO_WINDOW_Y: f32 = 120.0;
const UI2_PARTICLE_DEMO_WINDOW_Z: i16 = 34;
const UI2_PARTICLE_DEMO_MAX_PARTICLES: usize = 300;
const UI2_PARTICLE_DEMO_FRAME_MS: u64 = 20;
const UI2_PARTICLE_DEMO_CLEAR_RGB: u32 = 0x070B11;
const UI2_PARTICLE_DEMO_BG_RGBA: [u8; 4] = [0x07, 0x0B, 0x11, 0xFF];
const UI2_PARTICLE_DEMO_GLYPH: char = '§';

struct DemoRng(u64);

impl DemoRng {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 32) as u32
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u32() as f32) / (u32::MAX as f32)
    }

    fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next_f32()
    }
}

fn seed_particles(system: &mut ParticleSystem, width: u32, height: u32) {
    let mut rng = DemoRng::new(0xC0FFEE_1000);
    let width_f = width as f32;
    let height_f = height as f32;
    let cx = width_f * 0.5;
    let cy = height_f * 0.5;

    for _ in 0..UI2_PARTICLE_DEMO_MAX_PARTICLES {
        let angle = rng.range(0.0, core::f32::consts::TAU);
        let speed = rng.range(10.0, 64.0);
        let drift = rng.range(-8.0, 8.0);
        let vx = libm::cosf(angle) * speed + drift;
        let vy = libm::sinf(angle) * speed + drift * 0.35;
        let life = rng.range(1.5, 6.0);
        let size_px = rng.range(1.0, 3.5);
        let color = pack_rgba(
            0x90u8.saturating_add((rng.next_u32() & 0x3F) as u8),
            0x70u8.saturating_add((rng.next_u32() & 0x5F) as u8),
            0xA0u8.saturating_add((rng.next_u32() & 0x5F) as u8),
            0xC0u8.saturating_add((rng.next_u32() & 0x3F) as u8),
        );
        system.spawn_styled(cx, cy, vx, vy, life, size_px, color);
    }
}

fn pack_rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | (a as u32)
}

fn push_particle_quad(
    verts: &mut Vec<u8>,
    transform: ViewTransform,
    uv: [f32; 4],
    sprite_w: u32,
    sprite_h: u32,
    x: f32,
    y: f32,
    size_px: f32,
    color: Rgba8,
) {
    let scale = (size_px.max(1.0) / sprite_h.max(1) as f32).max(0.35) * 2.15;
    let draw_w = (sprite_w as f32 * scale).max(1.0);
    let draw_h = (sprite_h as f32 * scale).max(1.0);
    let left = x - draw_w * 0.5;
    let top = y - draw_h * 0.5;
    push_tex_quad_px(
        verts,
        transform,
        left,
        top,
        left + draw_w,
        top + draw_h,
        uv,
        color,
    );
}

fn build_particle_verts(
    snapshot: &[ParticleSnapshot],
    width: u32,
    height: u32,
    uv: [f32; 4],
    sprite_w: u32,
    sprite_h: u32,
    tick: u32,
) -> Vec<u8> {
    let transform = ViewTransform::from_extent(width, height);
    let mut verts = Vec::with_capacity(snapshot.len().saturating_mul(6 * TEX_VERTEX_SIZE));
    let t = tick as f32 * 0.045;

    for (idx, particle) in snapshot.iter().enumerate() {
        let seed = idx as f32 * 0.618_034 + t;
        let dx_a = libm::sinf(seed * 1.13) * 0.5 + 0.5;
        let dy_a = libm::cosf(seed * 0.91) * 0.5 + 0.5;
        let dx_b = libm::sinf(seed * 1.87 + 1.2) * 0.5 + 0.5;
        let dy_b = libm::cosf(seed * 1.41 + 0.7) * 0.5 + 0.5;
        let packed = Rgba8::new(
            (dx_a.clamp(0.0, 1.0) * 255.0) as u8,
            (dy_a.clamp(0.0, 1.0) * 255.0) as u8,
            (dx_b.clamp(0.0, 1.0) * 255.0) as u8,
            (dy_b.clamp(0.0, 1.0) * 255.0) as u8,
        );
        push_particle_quad(
            &mut verts,
            transform,
            uv,
            sprite_w,
            sprite_h,
            particle.x,
            particle.y,
            particle.size_px,
            packed,
        );
    }

    verts
}

fn respawn_dead_particles(system: &mut ParticleSystem, width: u32, height: u32, rng: &mut DemoRng) {
    let width_f = width as f32;
    let height_f = height as f32;
    let cx = width_f * 0.5;
    let cy = height_f * 0.55;

    while system.alive_count() < UI2_PARTICLE_DEMO_MAX_PARTICLES {
        let angle = rng.range(-1.4, -1.7);
        let spread = rng.range(-0.55, 0.55);
        let speed = rng.range(18.0, 92.0);
        let vx = libm::cosf(angle + spread) * speed;
        let vy = libm::sinf(angle + spread) * speed - rng.range(4.0, 24.0);
        let life = rng.range(1.0, 4.0);
        let size_px = rng.range(1.0, 4.0);
        let color = pack_rgba(
            0xC8u8.saturating_add((rng.next_u32() & 0x27) as u8),
            0x90u8.saturating_add((rng.next_u32() & 0x4F) as u8),
            0x48u8.saturating_add((rng.next_u32() & 0x67) as u8),
            0xA0u8.saturating_add((rng.next_u32() & 0x4F) as u8),
        );
        system.spawn_styled(cx, cy, vx, vy, life, size_px, color);
    }
}

fn create_particle_demo_window() -> Option<crate::r::ui2::Ui2SurfaceWindow> {
    crate::r::ui2::Ui2SurfaceWindow::new(
        "Particle System",
        crate::r::ui2::Ui2Rect {
            x: UI2_PARTICLE_DEMO_WINDOW_X,
            y: UI2_PARTICLE_DEMO_WINDOW_Y,
            w: UI2_PARTICLE_DEMO_RT_W as f32,
            h: UI2_PARTICLE_DEMO_RT_H as f32,
        },
        UI2_PARTICLE_DEMO_WINDOW_Z,
        128,
        UI2_PARTICLE_DEMO_TEX_ID,
        false,
        UI2_PARTICLE_DEMO_BG_RGBA,
    )
}

#[embassy_executor::task]
pub async fn ui2_particle_demo_task() {
    let Some(surface) = create_particle_demo_window() else {
        return;
    };

    let window_id = surface.window_id();
    let (surface_w, surface_h) = surface.size();
    crate::log!(
        "ui2-particle-demo: window={} tex={} size={}x{} start\n",
        window_id,
        surface.tex_id(),
        surface_w,
        surface_h
    );

    let mut system = ParticleSystem::new(UI2_PARTICLE_DEMO_MAX_PARTICLES);
    let mut snapshot = Vec::with_capacity(UI2_PARTICLE_DEMO_MAX_PARTICLES);
    let mut rng = DemoRng::new(0x51D3_1000);
    let mut tick = 0u32;
    if !crate::gfx::imba_athlas::ensure_imba_athlas_png_buckets_uploaded() {
        crate::log!("ui2-particle-demo: athlas upload failed\n");
        return;
    }
    let Some((sprite_tex_id, sprite_uv, sprite_w, sprite_h)) =
        crate::gfx::imba_athlas::imba_athlas_sprite_for_char(2, UI2_PARTICLE_DEMO_GLYPH)
    else {
        crate::log!("ui2-particle-demo: glyph lookup failed '{}'\n", UI2_PARTICLE_DEMO_GLYPH);
        return;
    };

    seed_particles(&mut system, surface_w, surface_h);
    let _ = crate::r::ui2::set_window_title(window_id, "Particle System (1000)");

    loop {
        let _report = system.update_dual_driven(UI2_PARTICLE_DEMO_FRAME_MS as f32 / 1000.0);
        respawn_dead_particles(&mut system, surface_w, surface_h, &mut rng);
        system.snapshot_into(&mut snapshot);
        let verts = build_particle_verts(
            snapshot.as_slice(),
            surface_w,
            surface_h,
            sprite_uv,
            sprite_w,
            sprite_h,
            tick,
        );
        let _ = crate::r::io::cabi::queue_render_particle_tex_triangles_to_texture_copy(
            surface.tex_id(),
            sprite_tex_id,
            UI2_PARTICLE_DEMO_CLEAR_RGB,
            verts.as_slice(),
            window_id,
            "ui2-particle-demo-tex",
        );
        tick = tick.wrapping_add(1);
        Timer::after(EmbassyDuration::from_millis(UI2_PARTICLE_DEMO_FRAME_MS)).await;
    }
}
