extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::gfx::particle::{ParticleSnapshot, ParticleSystem};

const UI2_PARTICLE_DEMO_TEX_ID: u32 = 4_709;
const UI2_PARTICLE_DEMO_RT_W: u32 = 512;
const UI2_PARTICLE_DEMO_RT_H: u32 = 320;
const UI2_PARTICLE_DEMO_WINDOW_X: f32 = 640.0;
const UI2_PARTICLE_DEMO_WINDOW_Y: f32 = 120.0;
const UI2_PARTICLE_DEMO_WINDOW_Z: i16 = 34;
const UI2_PARTICLE_DEMO_MAX_PARTICLES: usize = 1_000;
const UI2_PARTICLE_DEMO_FRAME_MS: u64 = 33;
const UI2_PARTICLE_DEMO_BG_RGBA: [u8; 4] = [0x07, 0x0B, 0x11, 0xFF];

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

fn unpack_rgba(color: u32) -> (u8, u8, u8, u8) {
    (
        ((color >> 24) & 0xFF) as u8,
        ((color >> 16) & 0xFF) as u8,
        ((color >> 8) & 0xFF) as u8,
        (color & 0xFF) as u8,
    )
}

fn clear_rgba(frame: &mut [u8], color: [u8; 4]) {
    for px in frame.chunks_exact_mut(4) {
        px.copy_from_slice(&color);
    }
}

fn blend_px(dst: &mut [u8], src: (u8, u8, u8, u8)) {
    let sa = src.3 as u32;
    let inv = 255u32.saturating_sub(sa);
    dst[0] = (((src.0 as u32 * sa) + (dst[0] as u32 * inv) + 127) / 255) as u8;
    dst[1] = (((src.1 as u32 * sa) + (dst[1] as u32 * inv) + 127) / 255) as u8;
    dst[2] = (((src.2 as u32 * sa) + (dst[2] as u32 * inv) + 127) / 255) as u8;
    dst[3] = (sa + (((dst[3] as u32) * inv) + 127) / 255).min(255) as u8;
}

fn draw_snapshot_rgba(frame: &mut [u8], width: u32, height: u32, snapshot: &[ParticleSnapshot]) {
    clear_rgba(frame, UI2_PARTICLE_DEMO_BG_RGBA);

    let width_i = width as i32;
    let height_i = height as i32;

    for particle in snapshot {
        let size = libm::ceilf(particle.size_px).max(1.0) as i32;
        let half = size / 2;
        let x = libm::roundf(particle.x) as i32;
        let y = libm::roundf(particle.y) as i32;
        let rgba = unpack_rgba(particle.color_rgba);

        for py in (y - half)..=(y + half) {
            if py < 0 || py >= height_i {
                continue;
            }
            for px in (x - half)..=(x + half) {
                if px < 0 || px >= width_i {
                    continue;
                }
                let idx = ((py as usize * width as usize) + px as usize) * 4;
                blend_px(&mut frame[idx..idx + 4], rgba);
            }
        }
    }
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
    let mut rgba = vec![0u8; surface_w as usize * surface_h as usize * 4];
    let mut rng = DemoRng::new(0x51D3_1000);

    seed_particles(&mut system, surface_w, surface_h);
    let _ = crate::r::ui2::set_window_title(window_id, "Particle System (1000)");

    loop {
        let _report = system.update_dual_driven(UI2_PARTICLE_DEMO_FRAME_MS as f32 / 1000.0);
        respawn_dead_particles(&mut system, surface_w, surface_h, &mut rng);
        system.snapshot_into(&mut snapshot);
        draw_snapshot_rgba(&mut rgba, surface_w, surface_h, snapshot.as_slice());
        let _ = surface.upload_rgba(rgba.as_slice(), "ui2-particle-demo-upload");
        Timer::after(EmbassyDuration::from_millis(UI2_PARTICLE_DEMO_FRAME_MS)).await;
    }
}
