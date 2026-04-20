extern crate alloc;

use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};
use trueos_gfx_core::{Rgba8, TEX_VERTEX_SIZE, ViewTransform, push_tex_quad_px};

use crate::gfx::althlasfont::twemoji;
use crate::r::ui2::Ui2WindowCursorSample;

const UI2_SMILEY_FOUNTAIN_TEX_ID: u32 = crate::tst_ui2_ids::Ui2DemoTexId::SmileyFountain.get();
const UI2_SMILEY_FOUNTAIN_RT_W: u32 = 560;
const UI2_SMILEY_FOUNTAIN_RT_H: u32 = 360;
const UI2_SMILEY_FOUNTAIN_WINDOW_X: f32 = 880.0;
const UI2_SMILEY_FOUNTAIN_WINDOW_Y: f32 = 120.0;
const UI2_SMILEY_FOUNTAIN_WINDOW_Z: i16 = 35;
const UI2_SMILEY_FOUNTAIN_WINDOW_ALPHA: u8 = 255;
const UI2_SMILEY_FOUNTAIN_MAX: usize = 120;
const UI2_SMILEY_FOUNTAIN_FRAME_MS: u64 = 20;
const UI2_SMILEY_FOUNTAIN_CLEAR_RGB: u32 = 0x060A10;
const UI2_SMILEY_FOUNTAIN_BG_RGBA: [u8; 4] = [0x06, 0x0A, 0x10, 0xFF];
const UI2_SMILEY_FOUNTAIN_GRAVITY: f32 = 118.0;
const UI2_SMILEY_FOUNTAIN_DRAG: f32 = 0.12;
const UI2_SMILEY_FOUNTAIN_MIN_SCALE: f32 = 0.9;
const UI2_SMILEY_FOUNTAIN_MAX_SCALE: f32 = 1.0;
const UI2_SMILEY_TORNADO_PULL: f32 = 240.0;
const UI2_SMILEY_TORNADO_SWIRL: f32 = 410.0;
const UI2_SMILEY_TORNADO_UPLIFT: f32 = 180.0;
const UI2_SMILEY_TORNADO_RADIUS: f32 = 180.0;
const UI2_SMILEY_TORNADO_MAX_CURSOR_BOOST: f32 = 1.6;

const SMILEY_CODEPOINTS: &[u32] = &[
    0x1F600, 0x1F601, 0x1F602, 0x1F603, 0x1F604, 0x1F605, 0x1F606, 0x1F607, 0x1F608, 0x1F60A,
    0x1F60D, 0x1F60E, 0x1F60F, 0x1F618, 0x1F61A, 0x1F61C, 0x1F61D, 0x1F61E, 0x1F642, 0x1F643,
    0x1F923, 0x1F970, 0x1F973, 0x1F97A, 0x1F60B, 0x1F929, 0x1F92A, 0x1F917,
];

#[derive(Clone, Copy, Debug)]
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

    fn index(&mut self, len: usize) -> usize {
        (self.next_u32() as usize) % len.max(1)
    }
}

#[derive(Clone, Copy, Debug)]
struct SmileyParticle {
    ch: char,
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    age: f32,
    life: f32,
    wobble: f32,
    wobble_speed: f32,
    orbit_gain: f32,
    orbit_sign: f32,
}

impl Default for SmileyParticle {
    fn default() -> Self {
        Self {
            ch: '?',
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
            age: 1.0,
            life: 1.0,
            wobble: 0.0,
            wobble_speed: 0.0,
            orbit_gain: 1.0,
            orbit_sign: 1.0,
        }
    }
}

#[inline]
fn particle_alive(particle: &SmileyParticle) -> bool {
    particle.age < particle.life
}

#[inline]
fn particle_progress(particle: &SmileyParticle) -> f32 {
    (particle.age / particle.life.max(0.001)).clamp(0.0, 1.0)
}

fn choose_smiley(rng: &mut DemoRng) -> char {
    for _ in 0..SMILEY_CODEPOINTS.len().max(1) {
        let codepoint = SMILEY_CODEPOINTS[rng.index(SMILEY_CODEPOINTS.len())];
        let Some(ch) = char::from_u32(codepoint) else {
            continue;
        };
        if twemoji::twemoji_lookup_glyph_region(ch).is_some() {
            return ch;
        }
    }
    char::from_u32(SMILEY_CODEPOINTS[0]).unwrap_or('?')
}

#[inline]
fn is_face_emoji(ch: char) -> bool {
    matches!(
        u32::from(ch),
        0x1F600
            | 0x1F601
            | 0x1F602
            | 0x1F603
            | 0x1F604
            | 0x1F605
            | 0x1F606
            | 0x1F607
            | 0x1F608
            | 0x1F60A
            | 0x1F60B
            | 0x1F60D
            | 0x1F60E
            | 0x1F60F
            | 0x1F61A
            | 0x1F61C
            | 0x1F61D
            | 0x1F61E
            | 0x1F618
            | 0x1F642
            | 0x1F643
            | 0x1F917
            | 0x1F923
            | 0x1F929
            | 0x1F92A
            | 0x1F970
            | 0x1F973
            | 0x1F97A
    )
}

fn face_orbit_profile(ch: char) -> (f32, f32) {
    let code = u32::from(ch);
    if !is_face_emoji(ch) {
        return (1.0, 1.0);
    }
    let gain = 0.82 + (((code & 0x7) as f32) * 0.065);
    let sign = if ((code >> 3) & 1) == 0 { 1.0 } else { -1.0 };
    (gain, sign)
}

fn respawn_particle(particle: &mut SmileyParticle, rng: &mut DemoRng, width: u32, height: u32) {
    let base_x = width as f32 * 0.5 + rng.range(-18.0, 18.0);
    let base_y = height as f32 - 44.0 + rng.range(-6.0, 8.0);
    particle.ch = choose_smiley(rng);
    particle.x = base_x;
    particle.y = base_y;
    particle.vx = rng.range(-44.0, 44.0);
    particle.vy = rng.range(-208.0, -132.0);
    particle.age = 0.0;
    particle.life = rng.range(1.6, 2.8);
    particle.wobble = rng.range(0.0, core::f32::consts::TAU);
    particle.wobble_speed = rng.range(2.4, 5.6);
    let (orbit_gain, orbit_sign) = face_orbit_profile(particle.ch);
    particle.orbit_gain = orbit_gain;
    particle.orbit_sign = orbit_sign;
}

fn seed_particles(particles: &mut [SmileyParticle], rng: &mut DemoRng, width: u32, height: u32) {
    for particle in particles.iter_mut() {
        respawn_particle(particle, rng, width, height);
        particle.age = rng.range(0.0, particle.life);
    }
}

fn update_particles(
    particles: &mut [SmileyParticle],
    width: u32,
    height: u32,
    dt: f32,
    rng: &mut DemoRng,
) {
    update_particles_with_cursors(particles, width, height, dt, rng, &[]);
}

fn update_particles_with_cursors(
    particles: &mut [SmileyParticle],
    width: u32,
    height: u32,
    dt: f32,
    rng: &mut DemoRng,
    cursors: &[Ui2WindowCursorSample],
) {
    let cursor_boost = cursor_tornado_strength(cursors.len());
    for particle in particles.iter_mut() {
        particle.age += dt;
        if !particle_alive(particle) {
            respawn_particle(particle, rng, width, height);
            continue;
        }

        particle.wobble += particle.wobble_speed * dt;
        let drift = libm::sinf(particle.wobble) * 18.0;
        particle.vx += drift * dt;
        if !cursors.is_empty() {
            apply_multi_cursor_tornado_force(particle, cursors, dt, cursor_boost);
        }
        particle.vx *= (1.0 - (UI2_SMILEY_FOUNTAIN_DRAG * dt)).clamp(0.82, 1.0);
        particle.vy += UI2_SMILEY_FOUNTAIN_GRAVITY * dt;
        particle.x += particle.vx * dt;
        particle.y += particle.vy * dt;

        if particle.x < -80.0
            || particle.x > width as f32 + 80.0
            || particle.y > height as f32 + 80.0
        {
            respawn_particle(particle, rng, width, height);
        }
    }
}

#[inline]
fn cursor_tornado_strength(cursor_count: usize) -> f32 {
    (1.0 + ((cursor_count.saturating_sub(1)) as f32 * 0.18))
        .clamp(1.0, UI2_SMILEY_TORNADO_MAX_CURSOR_BOOST)
}

fn apply_tornado_force(
    particle: &mut SmileyParticle,
    cx: f32,
    cy: f32,
    dt: f32,
    strength: f32,
    swirl_sign: f32,
) {
    let dx = cx - particle.x;
    let dy = cy - particle.y;
    let dist_sq = (dx * dx) + (dy * dy);
    let dist = libm::sqrtf(dist_sq).max(1.0);
    if dist > UI2_SMILEY_TORNADO_RADIUS * 1.8 {
        return;
    }

    let falloff = (1.0 - (dist / (UI2_SMILEY_TORNADO_RADIUS * 1.8))).clamp(0.0, 1.0);
    let nx = dx / dist;
    let ny = dy / dist;
    let tx = -ny;
    let ty = nx;
    let swirl = UI2_SMILEY_TORNADO_SWIRL * falloff * strength * particle.orbit_gain * swirl_sign;
    let pull = UI2_SMILEY_TORNADO_PULL * falloff * strength;

    particle.vx += ((tx * swirl) + (nx * pull)) * dt;
    particle.vy +=
        ((ty * swirl) + (ny * pull) - (UI2_SMILEY_TORNADO_UPLIFT * falloff * strength)) * dt;
}

fn apply_multi_cursor_tornado_force(
    particle: &mut SmileyParticle,
    cursors: &[Ui2WindowCursorSample],
    dt: f32,
    cursor_boost: f32,
) {
    let mut nearest_slot = 0u32;
    let mut nearest_dist_sq = f32::INFINITY;
    for cursor in cursors {
        let dx = cursor.x - particle.x;
        let dy = cursor.y - particle.y;
        let dist_sq = (dx * dx) + (dy * dy);
        if dist_sq < nearest_dist_sq {
            nearest_dist_sq = dist_sq;
            nearest_slot = cursor.slot_id;
        }
    }

    for cursor in cursors {
        let slot_sign = if (cursor.slot_id & 1) == 0 { 1.0 } else { -1.0 };
        let swirl_sign = slot_sign * particle.orbit_sign;
        apply_tornado_force(
            particle,
            cursor.x,
            cursor.y,
            dt,
            if cursor.slot_id == nearest_slot {
                cursor_boost
            } else {
                cursor_boost * 0.72
            },
            swirl_sign,
        );
    }
}

#[inline]
fn particle_scale(progress: f32) -> f32 {
    let grow = (progress / 0.22).clamp(0.0, 1.0);
    let fade = (1.0 - ((progress - 0.56).max(0.0) / 0.44)).clamp(0.0, 1.0);
    (UI2_SMILEY_FOUNTAIN_MIN_SCALE
        + ((UI2_SMILEY_FOUNTAIN_MAX_SCALE - UI2_SMILEY_FOUNTAIN_MIN_SCALE) * grow))
        * fade.max(0.05)
}

#[inline]
fn particle_alpha(progress: f32) -> u8 {
    let fade = (1.0 - ((progress - 0.48).max(0.0) / 0.52)).clamp(0.0, 1.0);
    ((fade * 255.0) + 0.5) as u8
}

fn build_smiley_verts(particles: &[SmileyParticle], width: u32, height: u32) -> Vec<u8> {
    let transform = ViewTransform::from_extent(width, height);
    let mut verts = Vec::with_capacity(particles.len().saturating_mul(6 * TEX_VERTEX_SIZE));

    for particle in particles.iter().copied() {
        if !particle_alive(&particle) {
            continue;
        }
        let Some(glyph) = twemoji::twemoji_resolve_glyph(particle.ch) else {
            continue;
        };
        if !(glyph.ready && glyph.texture.is_some()) {
            continue;
        }

        let progress = particle_progress(&particle);
        let scale = particle_scale(progress).clamp(0.0, UI2_SMILEY_FOUNTAIN_MAX_SCALE);
        let alpha = particle_alpha(progress);
        if alpha == 0 || scale <= 0.0 {
            continue;
        }

        let draw_w = f32::from(glyph.region.src_w.max(1)) * scale;
        let draw_h = f32::from(glyph.region.src_h.max(1)) * scale;
        let half_w = draw_w * 0.5;
        let half_h = draw_h * 0.5;
        let atlas_w = f32::from(glyph.region.atlas_w.max(1));
        let atlas_h = f32::from(glyph.region.atlas_h.max(1));
        let src_x = f32::from(glyph.region.src_x);
        let src_y = f32::from(glyph.region.src_y);
        push_tex_quad_px(
            &mut verts,
            transform,
            particle.x - half_w,
            particle.y - half_h,
            particle.x + half_w,
            particle.y + half_h,
            [
                src_x / atlas_w,
                src_y / atlas_h,
                (src_x + f32::from(glyph.region.src_w)) / atlas_w,
                (src_y + f32::from(glyph.region.src_h)) / atlas_h,
            ],
            Rgba8::new(255, 255, 255, alpha),
        );
    }

    verts
}

fn create_smiley_fountain_window() -> Option<crate::r::ui2::Ui2SurfaceWindow> {
    crate::r::ui2::Ui2SurfaceWindow::new(
        "Smiley Fountain",
        crate::r::ui2::Ui2Rect {
            x: UI2_SMILEY_FOUNTAIN_WINDOW_X,
            y: UI2_SMILEY_FOUNTAIN_WINDOW_Y,
            w: UI2_SMILEY_FOUNTAIN_RT_W as f32,
            h: UI2_SMILEY_FOUNTAIN_RT_H as f32,
        },
        UI2_SMILEY_FOUNTAIN_WINDOW_Z,
        UI2_SMILEY_FOUNTAIN_WINDOW_ALPHA,
        UI2_SMILEY_FOUNTAIN_TEX_ID,
        false,
        UI2_SMILEY_FOUNTAIN_BG_RGBA,
    )
}

#[embassy_executor::task]
pub async fn ui2_smiley_fountain_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-smiley-fountain-demo");
    while !twemoji::twemoji_ready() {
        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-smiley-fountain-demo", 20).await {
            return;
        }
    }

    let Some(surface) = create_smiley_fountain_window() else {
        return;
    };
    let _ = surface.bind_spawn_task("ui2-smiley-fountain-demo");

    let window_id = surface.window_id();
    let (surface_w, surface_h) = surface.size();
    let _ = crate::r::ui2::set_window_title(window_id, "Smiley Fountain");
    crate::log!(
        "ui2-smiley-fountain: window={} tex={} size={}x{} start\n",
        window_id,
        surface.tex_id(),
        surface_w,
        surface_h
    );

    let mut rng = DemoRng::new(0x5A11_EE00);
    let mut particles = [SmileyParticle::default(); UI2_SMILEY_FOUNTAIN_MAX];
    let mut tornado_active = false;
    seed_particles(&mut particles, &mut rng, surface_w, surface_h);

    loop {
        if crate::r::spawn_service::task_stop_requested("ui2-smiley-fountain-demo") {
            break;
        }
        let dt = UI2_SMILEY_FOUNTAIN_FRAME_MS as f32 / 1000.0;
        let cursors = crate::r::ui2::window_content_cursor_positions(window_id);
        let active_now = !cursors.is_empty();
        if active_now != tornado_active {
            tornado_active = active_now;
            let _ = crate::r::ui2::set_window_title(
                window_id,
                if tornado_active {
                    "Smiley Fountaintornado"
                } else {
                    "Smiley Fountain"
                },
            );
        }
        update_particles_with_cursors(
            &mut particles,
            surface_w,
            surface_h,
            dt,
            &mut rng,
            cursors.as_slice(),
        );
        let verts = build_smiley_verts(&particles, surface_w, surface_h);
        let _ = crate::r::io::cabi::queue_render_tex_triangles_to_texture_copy(
            surface.tex_id(),
            twemoji::TWEMOJI_TEX_ID,
            UI2_SMILEY_FOUNTAIN_CLEAR_RGB,
            verts.as_slice(),
            window_id,
            if tornado_active {
                "ui2-smiley-fountaintornado"
            } else {
                "ui2-smiley-fountain-demo"
            },
        );
        if crate::r::spawn_service::wait_task_or_timeout_ms(
            "ui2-smiley-fountain-demo",
            UI2_SMILEY_FOUNTAIN_FRAME_MS,
        )
        .await
        {
            break;
        }
    }
}
