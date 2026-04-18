extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};
use trueos_gfx_core::{Rgba8, TEX_VERTEX_SIZE, ViewTransform, push_tex_quad_px};

use crate::gfx::particle::{ParticleSnapshot, ParticleSystem};
use crate::gfx::svg::SvgTextureInfo;

const UI2_PARTICLE_DEMO_TEX_ID: u32 = crate::tst_ui2_ids::Ui2DemoTexId::Particle.get();
const UI2_PARTICLE_DEMO_SPRITE_TEX_ID: u32 =
    crate::tst_ui2_ids::Ui2DemoTexId::ParticleSprite.get();
const UI2_PARTICLE_DEMO_RT_W: u32 = 512;
const UI2_PARTICLE_DEMO_RT_H: u32 = 320;
const UI2_PARTICLE_DEMO_WINDOW_X: f32 = 640.0;
const UI2_PARTICLE_DEMO_WINDOW_Y: f32 = 120.0;
const UI2_PARTICLE_DEMO_WINDOW_Z: i16 = 34;
const UI2_PARTICLE_DEMO_WINDOW_ALPHA: u8 = 255;
const UI2_PARTICLE_DEMO_MAX_PARTICLES: usize = 96;
const UI2_PARTICLE_DEMO_FRAME_MS: u64 = 20;
const UI2_PARTICLE_DEMO_CLEAR_RGB: u32 = 0x070B11;
const UI2_PARTICLE_DEMO_BG_RGBA: [u8; 4] = [0x07, 0x0B, 0x11, 0xFF];
const UI2_PARTICLE_DEMO_SPRITE_SCALE: f32 = 15.0;
const UI2_PARTICLE_DEMO_SVG_SRC: &str = include_str!("parapath.svg");

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
        let size_px = rng.range(3.5, 8.0);
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

fn extract_svg_attr<'a>(svg: &'a str, attr: &str) -> Option<&'a str> {
    let needle = attr;
    let start = svg.find(needle)? + needle.len();
    let tail = &svg[start..];
    let end = tail.find('"')?;
    Some(&tail[..end])
}

fn extract_svg_path_data(svg: &str, ordinal: usize) -> Option<&str> {
    let mut from = 0usize;
    for idx in 0..=ordinal {
        let rel = svg[from..].find(" d=\"")?;
        from += rel + 4;
        let tail = &svg[from..];
        let end = tail.find('"')?;
        if idx == ordinal {
            return Some(&tail[..end]);
        }
        from += end + 1;
    }
    None
}

fn normalized_particle_svg() -> String {
    let view_box = extract_svg_attr(UI2_PARTICLE_DEMO_SVG_SRC, "viewBox=\"")
        .unwrap_or("0 0 241.3044 506.9858");
    let outer = extract_svg_path_data(UI2_PARTICLE_DEMO_SVG_SRC, 0).unwrap_or("");
    let inner = extract_svg_path_data(UI2_PARTICLE_DEMO_SVG_SRC, 1).unwrap_or("");
    let mut d = String::with_capacity(outer.len() + inner.len() + 2);
    d.push_str(outer);
    d.push(' ');
    d.push_str(inner);
    let mut svg = String::with_capacity(d.len() + 128);
    svg.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"");
    svg.push_str(view_box);
    svg.push_str("\"><path fill=\"white\" fill-rule=\"evenodd\" stroke=\"none\" d=\"");
    svg.push_str(d.as_str());
    svg.push_str("\"/></svg>");
    svg
}

fn build_particle_verts(
    snapshot: &[ParticleSnapshot],
    width: u32,
    height: u32,
    sprite_info: SvgTextureInfo,
) -> Vec<u8> {
    let transform = ViewTransform::from_extent(width, height);
    let mut verts = Vec::with_capacity(snapshot.len().saturating_mul(6 * TEX_VERTEX_SIZE));
    let sprite_aspect = if sprite_info.height == 0 {
        1.0
    } else {
        sprite_info.width as f32 / sprite_info.height as f32
    };

    for particle in snapshot.iter().copied() {
        let sprite_h = particle.size_px.max(1.0) * UI2_PARTICLE_DEMO_SPRITE_SCALE;
        let sprite_w = sprite_h * sprite_aspect.max(0.05);
        let half_w = sprite_w * 0.5;
        let half_h = sprite_h * 0.5;
        push_tex_quad_px(
            &mut verts,
            transform,
            particle.x - half_w,
            particle.y - half_h,
            particle.x + half_w,
            particle.y + half_h,
            [0.0, 0.0, 1.0, 1.0],
            Rgba8::from_rgba_u32(particle.color_rgba),
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
        let size_px = rng.range(3.5, 9.0);
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
        UI2_PARTICLE_DEMO_WINDOW_ALPHA,
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
    let sprite_svg = normalized_particle_svg();
    let Ok(sprite_info) = crate::gfx::svg::upload_svg_text_to_texture(
        UI2_PARTICLE_DEMO_SPRITE_TEX_ID,
        sprite_svg.as_str(),
    ) else {
        crate::log!(
            "ui2-particle-demo: svg upload failed tex={}\n",
            UI2_PARTICLE_DEMO_SPRITE_TEX_ID
        );
        return;
    };

    seed_particles(&mut system, surface_w, surface_h);
    let _ = crate::r::ui2::set_window_title(window_id, "Particle System");

    loop {
        let _report = system.update_dual_driven(UI2_PARTICLE_DEMO_FRAME_MS as f32 / 1000.0);
        respawn_dead_particles(&mut system, surface_w, surface_h, &mut rng);
        system.snapshot_into(&mut snapshot);
        let verts = build_particle_verts(snapshot.as_slice(), surface_w, surface_h, sprite_info);
        let _ = crate::r::io::cabi::queue_render_tex_triangles_to_texture_copy(
            surface.tex_id(),
            UI2_PARTICLE_DEMO_SPRITE_TEX_ID,
            UI2_PARTICLE_DEMO_CLEAR_RGB,
            verts.as_slice(),
            window_id,
            "ui2-particle-demo-tinted",
        );
        Timer::after(EmbassyDuration::from_millis(UI2_PARTICLE_DEMO_FRAME_MS)).await;
    }
}
