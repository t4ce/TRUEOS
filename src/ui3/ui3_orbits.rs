use alloc::vec::Vec;
use core::cmp::{max, min};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use libm::{ceilf, cosf, floorf, sinf, sqrtf};
use spin::{Mutex, Once};

const BUTTON_COUNT: usize = 10;
const ORBIT_MOON_PHASES: [u32; BUTTON_COUNT] = [
    0x1F311, 0x1F312, 0x1F313, 0x1F314, 0x1F315, 0x1F316, 0x1F317, 0x1F318, 0x1F319, 0x1F31A,
];
const ORBIT_CENTER_BUTTERFLY: u32 = 0x1F98B;
const ORBIT_RADIUS_PX: f32 = 300.0;
const SPRITE_CELL_PX: f32 = 64.0;
const CENTER_SPRITE_CELL_PX: f32 = 70.0;
const TWO_PI: f32 = 6.2831855;
const INTRO_LERP: f32 = 0.18;
const FOLLOW_LERP: f32 = 0.28;
const TRAIL_ACCEL_MIN: f32 = 0.010;
const TRAIL_ACCEL_MAX: f32 = 0.050;
const TRAIL_DAMPING: f32 = 0.82;
const TRAIL_SPEED_SCALE: f32 = 1800.0;
const EJECT_MARGIN_PX: f32 = 96.0;
const UI3_ORBITS_FRAME_MS: u64 = 16;
const ORBIT_ACTION_SPEED_NUM: u32 = 22;
const ORBIT_ACTION_SPEED_DEN: u32 = 10;
const ORBIT_ACTION_FULL_STEPS: u32 = ORBIT_ACTION_SPEED_NUM / ORBIT_ACTION_SPEED_DEN;
const ORBIT_ACTION_TAIL_STEPS: u32 = ORBIT_ACTION_SPEED_NUM % ORBIT_ACTION_SPEED_DEN;
const ORBIT_RADIUS_WAVE_AMOUNT: f32 = 0.15;
const ORBIT_RADIUS_WAVE_SPEED: f32 = 0.011;
const PARTICLES_PER_BUTTON: usize = 10;
const PARTICLE_LIFE_FRAMES: u32 = 54;
const PARTICLE_MIN_R_PX: f32 = 5.0;
const PARTICLE_AXIS_PX: f32 = 78.0;
const PARTICLE_MU: f32 = 18.0;
const PARTICLE_ALPHA_MAX: u32 = 255;
const PARTICLE_RADIUS_PX: f32 = 3.25;
const PARTICLE_WOBBLE_PX: f32 = 6.0;
const PARTICLE_EMIT_CLEARANCE_PX: f32 = 52.0;
const PARTICLE_DISTANCE_CAP_PX: f32 = 118.0;
const PARTICLE_TRAIL_STEPS: usize = 3;
const PARTICLE_TRAIL_SPACING_PX: f32 = 5.0;
const ACTIVE_PARTICLE_MULTIPLIER: usize = 5;
const ACTIVE_PARTICLE_WOBBLE_SCALE: f32 = 2.35;
const ACTIVE_PARTICLE_TRAIL_SCALE: f32 = 1.75;
const DISTORTION_ROLL_FRAMES: u32 = 125;
const DISTORTION_LIFE_FRAMES: u32 = 125;
const DISTORTION_TARGET_COUNT: u32 = BUTTON_COUNT as u32 + 1;
const DISTORTION_INNER_RADIUS_PX: f32 = ORBIT_RADIUS_PX * 0.5;
const DISCO_LIFE_FRAMES: u32 = 1875;
// Performance degraded: this full-screen gradient pass is intentionally parked
// until the orbit particles prove stable on baremetal.
const DISCO_GRADIENTS_ENABLED: bool = false;
const DISCO_GRADIENT_COUNT: usize = 30;
const HAPPY_ACTION_FRAMES: u32 = 188;
const HAPPY_ACTION_BASELINE_FRAMES: u32 = 1625;
const HAPPY_ACTION_JITTER_FRAMES: u32 = 625;
const HAPPY_COMBO_CHANCE_PER_1000: u32 = 100;
const PARALLEL_SCALE_FRAMES: u32 = 12;
const PARALLEL_RETURN_DELAY_FRAMES: u32 = 938;
const PARALLEL_ROLL_PER_1000: u32 = 10;
const MOOD_ORBIT_ANGLE_SPEED: f32 = 0.018;
const RETURN_SPAWN_MARGIN_PX: f32 = 96.0;
const CURSOR_GAME_LEVEL_MAX: u32 = 15;
const WEATHER_PROC_CHANCE_PER_1000: u32 = 150;
const WEATHER_MIN_RECT_AREA_PX: u32 = 80_000;
const WEATHER_REVERSE_FRAMES: u32 = 188;
const WEATHER_REARRANGE_FRAMES: u32 = 313;
const WEATHER_TOTAL_FRAMES: u32 = WEATHER_REVERSE_FRAMES + WEATHER_REARRANGE_FRAMES;
const WEATHER_GRID_JITTER_PX: f32 = 10.0;

const LEVEL_ORBIT_MOTION: u32 = 2;
const LEVEL_FOLLOW_TRAIL: u32 = 3;
const LEVEL_RADIUS_WAVE: u32 = 4;
const LEVEL_BUTTON_ANIM: u32 = 5;
const LEVEL_PARTICLES: u32 = 6;
const LEVEL_PARTICLE_TRAILS: u32 = 7;
const LEVEL_DISTORTION: u32 = 8;
const LEVEL_ACTIVE_RADIATION: u32 = 9;
const LEVEL_CENTER_BUTTERFLY: u32 = 10;
const LEVEL_SAD_MOOD: u32 = 11;
const LEVEL_HAPPY_MOOD: u32 = 12;
const LEVEL_PARALLEL_WORLD: u32 = 13;
const LEVEL_WEATHER_PROC: u32 = 14;
const LEVEL_FULL_COMBO: u32 = 15;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum OrbitMode {
    Hidden,
    Orbiting,
    Parked,
    Ejecting,
}

impl OrbitMode {
    const fn label(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::Orbiting => "orbiting",
            Self::Parked => "parked",
            Self::Ejecting => "ejecting",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ButtonMood {
    Sad,
    Normal,
    Happy,
}

#[derive(Copy, Clone, Debug)]
struct ButtonPersona {
    mood: ButtonMood,
    orbit_bucket: u32,
    parallel_scale_until: u32,
    parallel_return_frame: u32,
    parallel_side: u8,
    happy_until: u32,
    next_happy_frame: u32,
    happy_seed: u32,
}

impl ButtonPersona {
    const fn new() -> Self {
        Self {
            mood: ButtonMood::Normal,
            orbit_bucket: 0,
            parallel_scale_until: 0,
            parallel_return_frame: 0,
            parallel_side: 0,
            happy_until: 0,
            next_happy_frame: HAPPY_ACTION_BASELINE_FRAMES,
            happy_seed: 0,
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct OrbitButton {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    slot: u16,
    alive: bool,
}

impl OrbitButton {
    const fn new() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
            slot: 0,
            alive: false,
        }
    }
}

struct OrbitState {
    mode: OrbitMode,
    buttons: [OrbitButton; BUTTON_COUNT],
    personas: [ButtonPersona; BUTTON_COUNT],
    frame: u32,
    epoch: u32,
    needs_clear: bool,
    parked_cursor_x: f32,
    parked_cursor_y: f32,
    follow_x: f32,
    follow_y: f32,
    follow_vx: f32,
    follow_vy: f32,
    follow_ready: bool,
    distortion_idx: Option<usize>,
    distortion_until_frame: u32,
    next_distortion_roll_frame: u32,
    distortion_seed: u32,
    disco_until_frame: u32,
    weather: WeatherState,
}

impl OrbitState {
    const fn new() -> Self {
        Self {
            mode: OrbitMode::Hidden,
            buttons: [OrbitButton::new(); BUTTON_COUNT],
            personas: [ButtonPersona::new(); BUTTON_COUNT],
            frame: 0,
            epoch: 0,
            needs_clear: false,
            parked_cursor_x: 0.0,
            parked_cursor_y: 0.0,
            follow_x: 0.0,
            follow_y: 0.0,
            follow_vx: 0.0,
            follow_vy: 0.0,
            follow_ready: false,
            distortion_idx: None,
            distortion_until_frame: 0,
            next_distortion_roll_frame: DISTORTION_ROLL_FRAMES,
            distortion_seed: 0,
            disco_until_frame: 0,
            weather: WeatherState::new(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct WeatherState {
    start_frame: u32,
    rect: crate::intel::LiveOverlayRect,
    seed: u32,
}

impl WeatherState {
    const fn new() -> Self {
        Self {
            start_frame: 0,
            rect: crate::intel::LiveOverlayRect::new(
                0,
                0,
                0,
                0,
                crate::intel::types::Rgba8::new(0, 0, 0, 0),
            ),
            seed: 0,
        }
    }
}

struct OrbitAtlas {
    width: u32,
    height: u32,
    rgba: alloc::vec::Vec<u8>,
}

#[derive(Copy, Clone)]
struct OrbitFrameBuffer {
    width: u32,
    height: u32,
    pitch_bytes: u32,
    bytes: usize,
    virt: *mut u8,
}

unsafe impl Send for OrbitFrameBuffer {}

#[derive(Copy, Clone)]
struct ButtonVisualTraits {
    hidden: bool,
    cell_px: f32,
    phase_offset: f32,
    radial_offset: f32,
    lerp_scale: f32,
    happy_active: bool,
    return_side: Option<u8>,
}

#[derive(Copy, Clone)]
struct WeatherFrame {
    state: WeatherState,
    age: u32,
}

struct Slot1OverlayLayer {
    rects: Vec<crate::intel::LiveOverlayRect>,
    preserve: Option<crate::intel::LiveOverlayRect>,
    dirty: bool,
}

impl Slot1OverlayLayer {
    const fn new() -> Self {
        Self {
            rects: Vec::new(),
            preserve: None,
            dirty: false,
        }
    }
}

#[derive(Copy, Clone)]
struct Slot1CanvasLayer {
    visible: bool,
    rect: crate::intel::LiveOverlayRect,
    src: *mut u8,
    src_pitch_bytes: usize,
}

unsafe impl Send for Slot1CanvasLayer {}

impl Slot1CanvasLayer {
    const fn new() -> Self {
        Self {
            visible: false,
            rect: crate::intel::LiveOverlayRect::new(
                0,
                0,
                0,
                0,
                crate::intel::types::Rgba8::new(0, 0, 0, 0),
            ),
            src: core::ptr::null_mut(),
            src_pitch_bytes: 0,
        }
    }
}

static ORBIT_STATE: Mutex<OrbitState> = Mutex::new(OrbitState::new());
static CURSOR_GAME_LEVEL: AtomicU32 = AtomicU32::new(CURSOR_GAME_LEVEL_MAX);
static ORBIT_ATLAS: Once<Option<OrbitAtlas>> = Once::new();
static ORBIT_FRAME_BUFFER: Mutex<Option<OrbitFrameBuffer>> = Mutex::new(None);
static SLOT1_OVERLAY_LAYER: Mutex<Slot1OverlayLayer> = Mutex::new(Slot1OverlayLayer::new());
static SLOT1_CANVAS_LAYER: Mutex<Slot1CanvasLayer> = Mutex::new(Slot1CanvasLayer::new());

#[embassy_executor::task]
pub async fn ui3_orbits_task() {
    crate::log!("ui3-orbits: task start frame_ms={}\n", UI3_ORBITS_FRAME_MS);
    loop {
        let _ = update_slot1_cursor_orbit_buttons();
        Timer::after(EmbassyDuration::from_millis(UI3_ORBITS_FRAME_MS)).await;
    }
}

pub(crate) fn toggle_slot1_cursor_orbit_buttons() {
    let mut state = ORBIT_STATE.lock();
    state.epoch = state.epoch.wrapping_add(1);
    let next = match state.mode {
        OrbitMode::Hidden => {
            init_orbit_buttons(&mut state);
            OrbitMode::Orbiting
        }
        OrbitMode::Orbiting => {
            state.parked_cursor_x = state.follow_x;
            state.parked_cursor_y = state.follow_y;
            for button in state.buttons.iter_mut() {
                button.vx = 0.0;
                button.vy = 0.0;
            }
            OrbitMode::Parked
        }
        OrbitMode::Parked => {
            init_eject_buttons(&mut state);
            OrbitMode::Ejecting
        }
        OrbitMode::Ejecting => {
            init_orbit_buttons(&mut state);
            OrbitMode::Orbiting
        }
    };
    state.mode = next;
    state.needs_clear = true;
    crate::log!("ui3-orbits: ctrl-space mode={} epoch={}\n", state.mode.label(), state.epoch);
}

pub(crate) fn set_cursor_game_level(level: u32) {
    let clamped = level.clamp(1, CURSOR_GAME_LEVEL_MAX);
    CURSOR_GAME_LEVEL.store(clamped, Ordering::Relaxed);
    crate::log!("ui3-orbits: cursor-game level={}\n", clamped);
}

pub(crate) fn cursor_game_level() -> u32 {
    CURSOR_GAME_LEVEL
        .load(Ordering::Relaxed)
        .clamp(1, CURSOR_GAME_LEVEL_MAX)
}

pub(crate) fn maybe_proc_weather_from_drag(start_x: u32, start_y: u32, end_x: u32, end_y: u32) {
    if !feature_enabled(LEVEL_WEATHER_PROC) || !orbit_visuals_active() {
        return;
    }
    let x0 = start_x.min(end_x);
    let y0 = start_y.min(end_y);
    let x1 = start_x.max(end_x);
    let y1 = start_y.max(end_y);
    let width = x1.saturating_sub(x0);
    let height = y1.saturating_sub(y0);
    if width.saturating_mul(height) < WEATHER_MIN_RECT_AREA_PX {
        return;
    }

    let mut state = ORBIT_STATE.lock();
    let seed = xorshift32(
        state.frame
            ^ x0.rotate_left(3)
            ^ y0.rotate_left(9)
            ^ width.rotate_left(15)
            ^ height.rotate_left(21)
            ^ state.epoch.rotate_left(27)
            ^ 0xC10D_C10D,
    );
    if seed % 1000 >= WEATHER_PROC_CHANCE_PER_1000 {
        return;
    }
    state.weather = WeatherState {
        start_frame: state.frame.max(1),
        rect: crate::intel::LiveOverlayRect::new(
            x0,
            y0,
            width.max(1),
            height.max(1),
            crate::intel::types::Rgba8::new(0, 0, 0, 0),
        ),
        seed,
    };
    crate::log!(
        "ui3-orbits: weather proc rect={}x{}@{},{} seed=0x{:08X}\n",
        width,
        height,
        x0,
        y0,
        seed
    );
}

pub(crate) fn submit_live_overlay_rects(
    rects: &[crate::intel::LiveOverlayRect],
    preserve: Option<crate::intel::LiveOverlayRect>,
    reason: &str,
) -> bool {
    {
        let mut layer = SLOT1_OVERLAY_LAYER.lock();
        layer.rects.clear();
        for rect in rects {
            layer.rects.push(*rect);
        }
        layer.preserve = preserve;
        layer.dirty = true;
    }

    if orbit_visuals_active() {
        return true;
    }

    if SLOT1_CANVAS_LAYER.lock().visible {
        return compose_slot1_frame(reason);
    }

    let ok = crate::intel::present_live_overlay_rects_preserving(rects, preserve, reason);
    if ok {
        SLOT1_OVERLAY_LAYER.lock().dirty = false;
    }
    ok
}

pub(crate) fn submit_canvas_rgba(
    rect: crate::intel::LiveOverlayRect,
    src: *mut u8,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    if src.is_null() || rect.width == 0 || rect.height == 0 {
        return false;
    }
    {
        let mut canvas = SLOT1_CANVAS_LAYER.lock();
        canvas.visible = true;
        canvas.rect = rect;
        canvas.src = src;
        canvas.src_pitch_bytes = src_pitch_bytes;
    }
    compose_slot1_frame(reason)
}

fn update_slot1_cursor_orbit_buttons() -> bool {
    let mode = { ORBIT_STATE.lock().mode };
    if mode == OrbitMode::Hidden {
        return present_overlay_layer_once_if_needed();
    }

    let Some((cursor_x, cursor_y, scanout_w, scanout_h)) = cursor_scanout_px() else {
        return false;
    };
    let Some(atlas) = orbit_atlas_once() else {
        return false;
    };

    let Some(buffer) = prepare_slot1_frame(scanout_w, scanout_h) else {
        return false;
    };

    let mut state = ORBIT_STATE.lock();
    state.frame = state.frame.wrapping_add(1);
    let frame = state.frame;
    let draw_frame = orbit_action_frame(frame);
    let mode = state.mode;
    if feature_enabled(LEVEL_DISTORTION) {
        update_distortion_state(&mut state, frame);
    } else {
        state.distortion_idx = None;
        state.disco_until_frame = 0;
    }
    let weather = update_weather_state(&mut state, frame);
    let disco_active = distortion_frame_active(frame, state.disco_until_frame);
    let mut alive_count = 0usize;

    if mode == OrbitMode::Parked && state.parked_cursor_x == 0.0 && state.parked_cursor_y == 0.0 {
        state.parked_cursor_x = state.follow_x;
        state.parked_cursor_y = state.follow_y;
    }

    if mode == OrbitMode::Orbiting && feature_enabled(LEVEL_FOLLOW_TRAIL) {
        for _ in 0..ORBIT_ACTION_FULL_STEPS {
            update_follow_center(&mut state, cursor_x, cursor_y, 1.0);
        }
        let tail_weight = orbit_action_tail_weight();
        if tail_weight > 0.0 {
            update_follow_center(&mut state, cursor_x, cursor_y, tail_weight);
        }
    } else if !state.follow_ready {
        state.follow_x = cursor_x;
        state.follow_y = cursor_y;
        state.follow_ready = true;
    }
    let center_x = if mode == OrbitMode::Parked {
        state.parked_cursor_x
    } else {
        state.follow_x
    };
    let center_y = if mode == OrbitMode::Parked {
        state.parked_cursor_y
    } else {
        state.follow_y
    };

    for idx in 0..BUTTON_COUNT {
        let phase = button_phase(idx);
        match mode {
            OrbitMode::Orbiting | OrbitMode::Parked => {
                let orbiting = mode == OrbitMode::Orbiting;
                let distorted = feature_enabled(LEVEL_DISTORTION)
                    && state.distortion_idx == Some(idx)
                    && distortion_frame_active(frame, state.distortion_until_frame);
                let distortion_seed = state.distortion_seed;
                let traits = update_button_persona(
                    &mut state.personas[idx],
                    idx,
                    phase,
                    frame,
                    draw_frame,
                    scanout_w,
                    scanout_h,
                );
                if let Some(side) = traits.return_side {
                    spawn_parallel_return(&mut state.buttons[idx], side, scanout_w, scanout_h);
                }
                let button = &mut state.buttons[idx];
                if traits.hidden {
                    button.alive = false;
                    continue;
                }
                for step in 0..ORBIT_ACTION_FULL_STEPS {
                    let sim_frame = orbit_subframe(frame, step);
                    let motion_frame = weather_motion_frame(sim_frame, weather);
                    let (mut target_x, mut target_y) = wavy_ring_target(
                        center_x,
                        center_y,
                        phase + traits.phase_offset,
                        motion_frame,
                        orbiting,
                    );
                    apply_button_radial_offset(
                        &mut target_x,
                        &mut target_y,
                        center_x,
                        center_y,
                        traits.radial_offset,
                    );
                    if distorted {
                        (target_x, target_y) = distorted_ring_target(
                            target_x,
                            target_y,
                            phase,
                            sim_frame,
                            distortion_seed,
                        );
                    }
                    apply_weather_target(&mut target_x, &mut target_y, idx, frame, weather);
                    let lerp = if sim_frame < 32.0 {
                        INTRO_LERP
                    } else {
                        FOLLOW_LERP
                    } * traits.lerp_scale;
                    button.x += (target_x - button.x) * lerp;
                    button.y += (target_y - button.y) * lerp;
                }
                let tail_weight = orbit_action_tail_weight();
                if tail_weight > 0.0 {
                    let sim_frame = orbit_action_frame(frame);
                    let motion_frame = weather_motion_frame(sim_frame, weather);
                    let (mut target_x, mut target_y) = wavy_ring_target(
                        center_x,
                        center_y,
                        phase + traits.phase_offset,
                        motion_frame,
                        orbiting,
                    );
                    apply_button_radial_offset(
                        &mut target_x,
                        &mut target_y,
                        center_x,
                        center_y,
                        traits.radial_offset,
                    );
                    if distorted {
                        (target_x, target_y) = distorted_ring_target(
                            target_x,
                            target_y,
                            phase,
                            sim_frame,
                            distortion_seed,
                        );
                    }
                    apply_weather_target(&mut target_x, &mut target_y, idx, frame, weather);
                    let lerp = if sim_frame < 32.0 {
                        INTRO_LERP
                    } else {
                        FOLLOW_LERP
                    } * traits.lerp_scale
                        * tail_weight;
                    button.x += (target_x - button.x) * lerp;
                    button.y += (target_y - button.y) * lerp;
                }
                button.alive = true;
                alive_count = alive_count.saturating_add(1);
                let visual_frame = weather_motion_frame(draw_frame, weather);
                if traits.happy_active {
                    draw_happy_button_echoes(
                        buffer,
                        atlas,
                        *button,
                        idx,
                        visual_frame,
                        traits.cell_px,
                    );
                }
                draw_orbit_button(buffer, atlas, *button, idx, visual_frame, traits.cell_px);
                if feature_enabled(LEVEL_PARTICLES) {
                    let active_radiation = distorted && feature_enabled(LEVEL_ACTIVE_RADIATION);
                    draw_button_particles(
                        buffer,
                        button.x,
                        button.y,
                        idx,
                        visual_frame,
                        if active_radiation {
                            ACTIVE_PARTICLE_MULTIPLIER
                        } else {
                            1
                        },
                        active_radiation,
                    );
                }
            }
            OrbitMode::Ejecting => {
                let button = &mut state.buttons[idx];
                for _ in 0..ORBIT_ACTION_FULL_STEPS {
                    update_eject_button(button, 1.0);
                }
                update_eject_button(button, orbit_action_tail_weight());
                button.alive = button.x > -EJECT_MARGIN_PX
                    && button.x < scanout_w as f32 + EJECT_MARGIN_PX
                    && button.y > -EJECT_MARGIN_PX
                    && button.y < scanout_h as f32 + EJECT_MARGIN_PX;
                if button.alive {
                    alive_count = alive_count.saturating_add(1);
                    draw_orbit_button(buffer, atlas, *button, idx, draw_frame, SPRITE_CELL_PX);
                    if feature_enabled(LEVEL_PARTICLES) {
                        draw_button_particles(
                            buffer, button.x, button.y, idx, draw_frame, 1, false,
                        );
                    }
                }
            }
            OrbitMode::Hidden => {}
        }
    }
    if matches!(mode, OrbitMode::Orbiting | OrbitMode::Parked)
        && feature_enabled(LEVEL_CENTER_BUTTERFLY)
    {
        draw_center_butterfly(buffer, atlas, center_x, center_y, draw_frame);
    }
    if disco_active && DISCO_GRADIENTS_ENABLED {
        draw_disco_gradients(buffer, draw_frame, state.distortion_seed);
    }
    draw_slot1_overlay_layer(buffer);

    if mode == OrbitMode::Ejecting && alive_count == 0 {
        state.mode = OrbitMode::Hidden;
        state.needs_clear = true;
    } else {
        state.needs_clear = true;
    }
    drop(state);

    present_orbit_frame_buffer(buffer, "ui3-orbits")
}

fn init_orbit_buttons(state: &mut OrbitState) {
    let Some((cursor_x, cursor_y, scanout_w, scanout_h)) = cursor_scanout_px() else {
        return;
    };
    state.frame = 0;
    state.parked_cursor_x = 0.0;
    state.parked_cursor_y = 0.0;
    state.follow_x = cursor_x;
    state.follow_y = cursor_y;
    state.follow_vx = 0.0;
    state.follow_vy = 0.0;
    state.follow_ready = true;
    state.distortion_idx = None;
    state.distortion_until_frame = 0;
    state.next_distortion_roll_frame = DISTORTION_ROLL_FRAMES;
    state.disco_until_frame = 0;
    for idx in 0..BUTTON_COUNT {
        state.personas[idx] = ButtonPersona::new();
        let phase = button_phase(idx);
        let spawn_radius = (scanout_w.max(scanout_h) as f32) * 0.72 + 220.0;
        let spawn_angle = phase + 0.37 * (state.epoch as f32);
        let Some(slot) = orbit_moon_phase_slot(idx) else {
            state.buttons[idx] = OrbitButton::new();
            continue;
        };
        state.buttons[idx] = OrbitButton {
            x: cursor_x + cosf(spawn_angle) * spawn_radius,
            y: cursor_y + sinf(spawn_angle) * spawn_radius,
            vx: 0.0,
            vy: 0.0,
            slot,
            alive: true,
        };
    }
}

fn init_eject_buttons(state: &mut OrbitState) {
    for idx in 0..BUTTON_COUNT {
        let phase = button_phase(idx) + 0.29 * (state.epoch as f32);
        let seed = xorshift32((state.epoch << 8) ^ idx as u32 ^ 0x9E37_79B9);
        let speed = 12.0 + ((seed & 0xFF) as f32 / 255.0) * 13.0;
        state.buttons[idx].vx = cosf(phase) * speed;
        state.buttons[idx].vy = sinf(phase) * speed;
        state.buttons[idx].alive = true;
    }
}

pub(crate) fn orbit_visuals_active() -> bool {
    ORBIT_STATE.lock().mode != OrbitMode::Hidden
}

fn present_overlay_layer_once_if_needed() -> bool {
    let mut state = ORBIT_STATE.lock();
    if !state.needs_clear {
        return false;
    }
    state.needs_clear = false;
    drop(state);

    let layer = SLOT1_OVERLAY_LAYER.lock();
    if SLOT1_CANVAS_LAYER.lock().visible {
        drop(layer);
        return compose_slot1_frame("ui3-orbits-retire");
    }
    if layer.rects.is_empty() {
        crate::intel::present_live_overlay_rects(&[], "ui3-orbits-clear")
    } else {
        crate::intel::present_live_overlay_rects_preserving(
            layer.rects.as_slice(),
            layer.preserve,
            "ui3-orbits-retire",
        )
    }
}

fn compose_slot1_frame(reason: &str) -> bool {
    let Some((scanout_w, scanout_h)) = crate::intel::active_scanout_dimensions() else {
        return false;
    };
    let Some(buffer) = prepare_slot1_frame(scanout_w, scanout_h) else {
        return false;
    };
    draw_slot1_overlay_layer(buffer);
    present_orbit_frame_buffer(buffer, reason)
}

fn prepare_slot1_frame(width: u32, height: u32) -> Option<OrbitFrameBuffer> {
    let buffer = orbit_frame_buffer(width, height)?;
    clear_orbit_frame_buffer(buffer);
    draw_canvas_layer(buffer);
    Some(buffer)
}

fn orbit_frame_buffer(width: u32, height: u32) -> Option<OrbitFrameBuffer> {
    if width == 0 || height == 0 {
        return None;
    }

    {
        let state = ORBIT_FRAME_BUFFER.lock();
        if let Some(buffer) = *state
            && buffer.width == width
            && buffer.height == height
        {
            return Some(buffer);
        }
    }

    let pitch_bytes = width.checked_mul(core::mem::size_of::<u32>() as u32)?;
    let bytes = (pitch_bytes as usize).checked_mul(height as usize)?;
    let (_phys, virt) = crate::dma::alloc(bytes, crate::intel::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    crate::intel::dma_flush(virt, bytes);

    let buffer = OrbitFrameBuffer {
        width,
        height,
        pitch_bytes,
        bytes,
        virt,
    };
    *ORBIT_FRAME_BUFFER.lock() = Some(buffer);
    crate::log!(
        "ui3-orbits: framebuffer alloc size={}x{} pitch={} bytes=0x{:X}\n",
        width,
        height,
        pitch_bytes,
        bytes
    );
    Some(buffer)
}

fn clear_orbit_frame_buffer(buffer: OrbitFrameBuffer) {
    unsafe {
        core::ptr::write_bytes(buffer.virt, 0, buffer.bytes);
    }
}

fn present_orbit_frame_buffer(buffer: OrbitFrameBuffer, reason: &str) -> bool {
    crate::intel::dma_flush(buffer.virt, buffer.bytes);
    let rect = crate::intel::LiveOverlayRect::new(
        0,
        0,
        buffer.width,
        buffer.height,
        crate::intel::types::Rgba8::new(0, 0, 0, 0),
    );
    crate::intel::present_ui3_canvas_rgba(rect, buffer.virt, buffer.pitch_bytes as usize, reason)
}

fn draw_slot1_overlay_layer(buffer: OrbitFrameBuffer) {
    let layer = SLOT1_OVERLAY_LAYER.lock();
    for rect in layer.rects.iter().copied() {
        draw_live_rect(buffer, rect);
    }
}

fn draw_canvas_layer(buffer: OrbitFrameBuffer) {
    let canvas = *SLOT1_CANVAS_LAYER.lock();
    if !canvas.visible || canvas.src.is_null() {
        return;
    }
    draw_src_rgba_layer(buffer, canvas.rect, canvas.src, canvas.src_pitch_bytes);
}

fn draw_src_rgba_layer(
    buffer: OrbitFrameBuffer,
    rect: crate::intel::LiveOverlayRect,
    src: *mut u8,
    src_pitch_bytes: usize,
) {
    if rect.width == 0
        || rect.height == 0
        || src.is_null()
        || src_pitch_bytes < rect.width as usize * core::mem::size_of::<u32>()
    {
        return;
    }
    let x0 = rect.x.min(buffer.width);
    let y0 = rect.y.min(buffer.height);
    let copy_w = rect.width.min(buffer.width.saturating_sub(x0));
    let copy_h = rect.height.min(buffer.height.saturating_sub(y0));
    if copy_w == 0 || copy_h == 0 {
        return;
    }

    let dst_pitch_pixels = (buffer.pitch_bytes as usize) / core::mem::size_of::<u32>();
    for row in 0..copy_h as usize {
        let src_row = unsafe { src.add(row.saturating_mul(src_pitch_bytes)) as *const u32 };
        for col in 0..copy_w as usize {
            let src_pixel = unsafe { core::ptr::read_volatile(src_row.add(col)) };
            if ((src_pixel >> 24) & 0xFF) == 0 {
                continue;
            }
            let dst_idx = (y0 as usize + row).saturating_mul(dst_pitch_pixels) + x0 as usize + col;
            unsafe {
                let dst_ptr = (buffer.virt as *mut u32).add(dst_idx);
                let dst = core::ptr::read_volatile(dst_ptr);
                core::ptr::write_volatile(dst_ptr, src_over(src_pixel, dst));
            }
        }
    }
}

fn draw_live_rect(buffer: OrbitFrameBuffer, rect: crate::intel::LiveOverlayRect) {
    if rect.width == 0 || rect.height == 0 || rect.color.a == 0 {
        return;
    }
    let x0 = rect.x.min(buffer.width);
    let y0 = rect.y.min(buffer.height);
    let x1 = rect.x.saturating_add(rect.width).min(buffer.width);
    let y1 = rect.y.saturating_add(rect.height).min(buffer.height);
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let src = rgba8_word(rect.color);
    let pitch_pixels = (buffer.pitch_bytes as usize) / core::mem::size_of::<u32>();
    for y in y0..y1 {
        for x in x0..x1 {
            let dst_idx = (y as usize).saturating_mul(pitch_pixels) + x as usize;
            unsafe {
                let dst_ptr = (buffer.virt as *mut u32).add(dst_idx);
                let dst = core::ptr::read_volatile(dst_ptr);
                core::ptr::write_volatile(dst_ptr, src_over(src, dst));
            }
        }
    }
}

fn rgba8_word(color: crate::intel::types::Rgba8) -> u32 {
    ((color.a as u32) << 24) | ((color.b as u32) << 16) | ((color.g as u32) << 8) | color.r as u32
}

fn cursor_scanout_px() -> Option<(f32, f32, u32, u32)> {
    let (scanout_w, scanout_h) = crate::intel::active_scanout_dimensions()?;
    let (_, nx, ny, _) = crate::r::cursor::preferred_kernel_hw_cursor_snapshot_with_slot_buttons()
        .unwrap_or((0, 0.5, 0.5, 0));
    let x = (nx.clamp(0.0, 1.0) as f32) * scanout_w.saturating_sub(1) as f32;
    let y = (ny.clamp(0.0, 1.0) as f32) * scanout_h.saturating_sub(1) as f32;
    Some((x, y, scanout_w, scanout_h))
}

fn update_follow_center(state: &mut OrbitState, cursor_x: f32, cursor_y: f32, weight: f32) {
    if weight <= 0.0 {
        return;
    }
    if !state.follow_ready {
        state.follow_x = cursor_x;
        state.follow_y = cursor_y;
        state.follow_vx = 0.0;
        state.follow_vy = 0.0;
        state.follow_ready = true;
        return;
    }

    let dx = cursor_x - state.follow_x;
    let dy = cursor_y - state.follow_y;
    let speed_hint = abs_f32(dx) + abs_f32(dy);
    let accel = TRAIL_ACCEL_MIN
        + (TRAIL_ACCEL_MAX - TRAIL_ACCEL_MIN) * (speed_hint / TRAIL_SPEED_SCALE).clamp(0.0, 1.0);
    let damping = 1.0 - (1.0 - TRAIL_DAMPING) * weight;
    state.follow_vx = (state.follow_vx + dx * accel * weight) * damping;
    state.follow_vy = (state.follow_vy + dy * accel * weight) * damping;
    state.follow_x += state.follow_vx * weight;
    state.follow_y += state.follow_vy * weight;
}

fn update_eject_button(button: &mut OrbitButton, weight: f32) {
    if weight <= 0.0 {
        return;
    }
    button.x += button.vx * weight;
    button.y += button.vy * weight;
    let accel = 1.0 + (1.018 - 1.0) * weight;
    button.vx *= accel;
    button.vy *= accel;
}

fn feature_enabled(level: u32) -> bool {
    cursor_game_level() >= level
}

fn update_weather_state(state: &mut OrbitState, frame: u32) -> Option<WeatherFrame> {
    if !feature_enabled(LEVEL_WEATHER_PROC) || state.weather.start_frame == 0 {
        return None;
    }
    let age = frame.saturating_sub(state.weather.start_frame);
    if age > WEATHER_TOTAL_FRAMES {
        state.weather = WeatherState::new();
        return None;
    }
    Some(WeatherFrame {
        state: state.weather,
        age,
    })
}

fn weather_motion_frame(frame: f32, weather: Option<WeatherFrame>) -> f32 {
    if let Some(weather) = weather
        && weather.age < WEATHER_REVERSE_FRAMES
    {
        return -frame * 2.0;
    }
    if feature_enabled(LEVEL_ORBIT_MOTION) {
        frame
    } else {
        0.0
    }
}

fn apply_weather_target(
    target_x: &mut f32,
    target_y: &mut f32,
    idx: usize,
    frame: u32,
    weather: Option<WeatherFrame>,
) {
    let Some(weather) = weather else {
        return;
    };
    if weather.age < WEATHER_REVERSE_FRAMES {
        return;
    }

    let grid = weather_grid_target(weather.state, idx);
    let settle_age = weather.age.saturating_sub(WEATHER_REVERSE_FRAMES);
    let t = (settle_age as f32 / WEATHER_REARRANGE_FRAMES as f32).clamp(0.0, 1.0);
    let eased = t * t * (3.0 - 2.0 * t);
    let storm = 1.0 - eased;
    let shimmer = sinf(frame as f32 * 0.18 + idx as f32 * 1.7) * WEATHER_GRID_JITTER_PX * storm;
    *target_x = grid.0 + (*target_x - grid.0) * eased + shimmer;
    *target_y = grid.1 + (*target_y - grid.1) * eased - shimmer * 0.5;
}

fn weather_grid_target(weather: WeatherState, idx: usize) -> (f32, f32) {
    let cols = 4u32;
    let rows = 3u32;
    let col = (idx as u32) % cols;
    let row = (idx as u32) / cols;
    let cell_w = weather.rect.width.max(1) as f32 / cols as f32;
    let cell_h = weather.rect.height.max(1) as f32 / rows as f32;
    let jitter_seed = xorshift32(weather.seed ^ (idx as u32).wrapping_mul(0x9E37_79B9));
    let jx = ((jitter_seed & 0xFF) as f32 / 255.0 - 0.5) * WEATHER_GRID_JITTER_PX;
    let jy = (((jitter_seed >> 8) & 0xFF) as f32 / 255.0 - 0.5) * WEATHER_GRID_JITTER_PX;
    (
        weather.rect.x as f32 + (col as f32 + 0.5) * cell_w + jx,
        weather.rect.y as f32 + (row as f32 + 0.5) * cell_h + jy,
    )
}

fn update_distortion_state(state: &mut OrbitState, frame: u32) {
    if state.distortion_idx.is_some()
        && !distortion_frame_active(frame, state.distortion_until_frame)
    {
        state.distortion_idx = None;
    }
    if frame < state.next_distortion_roll_frame {
        return;
    }

    let seed = xorshift32(
        frame ^ state.epoch.rotate_left(11) ^ state.distortion_seed.rotate_right(5) ^ 0xA53C_6D1B,
    );
    state.distortion_seed = seed;
    state.next_distortion_roll_frame = frame.saturating_add(DISTORTION_ROLL_FRAMES);
    let target = seed % DISTORTION_TARGET_COUNT;
    if target == BUTTON_COUNT as u32 {
        state.distortion_idx = None;
        state.disco_until_frame = frame.saturating_add(DISCO_LIFE_FRAMES);
        crate::log!(
            "ui3-orbits: distortion-roll target=butterfly disco_until={} seed=0x{:08X}\n",
            state.disco_until_frame,
            seed
        );
    } else {
        state.distortion_idx = Some(target as usize);
        state.distortion_until_frame = frame.saturating_add(DISTORTION_LIFE_FRAMES);
        crate::log!(
            "ui3-orbits: distortion-roll target={} until={} seed=0x{:08X}\n",
            target,
            state.distortion_until_frame,
            seed
        );
    }
}

fn update_button_persona(
    persona: &mut ButtonPersona,
    idx: usize,
    phase: f32,
    frame: u32,
    draw_frame: f32,
    scanout_w: u32,
    scanout_h: u32,
) -> ButtonVisualTraits {
    if !feature_enabled(LEVEL_SAD_MOOD) {
        return normal_button_traits();
    }

    let orbit_bucket = ((draw_frame * MOOD_ORBIT_ANGLE_SPEED + phase) / TWO_PI) as u32;
    if orbit_bucket != persona.orbit_bucket {
        persona.orbit_bucket = orbit_bucket;
        roll_button_mood(persona, idx, frame, orbit_bucket);
    }

    if persona.parallel_return_frame != 0 {
        if frame > persona.parallel_scale_until && frame < persona.parallel_return_frame {
            return ButtonVisualTraits {
                hidden: true,
                cell_px: SPRITE_CELL_PX,
                phase_offset: 0.0,
                radial_offset: 0.0,
                lerp_scale: 1.0,
                happy_active: false,
                return_side: None,
            };
        }
        let mut return_side = None;
        if frame >= persona.parallel_return_frame {
            return_side = Some(persona.parallel_side);
            persona.parallel_scale_until = 0;
            persona.parallel_return_frame = 0;
            let mut traits = normal_button_traits();
            traits.return_side = return_side;
            return traits;
        }
    }

    let happy_active =
        feature_enabled(LEVEL_HAPPY_MOOD) && update_happy_action(persona, idx, frame);
    let mut traits = match persona.mood {
        ButtonMood::Sad => ButtonVisualTraits {
            hidden: false,
            cell_px: SPRITE_CELL_PX * 0.88,
            phase_offset: -0.08 + 0.04 * sinf(draw_frame * 0.021 + phase),
            radial_offset: -18.0 + 8.0 * sinf(draw_frame * 0.017 + phase * 2.0),
            lerp_scale: 0.42,
            happy_active: false,
            return_side: None,
        },
        ButtonMood::Normal => normal_button_traits(),
        ButtonMood::Happy => ButtonVisualTraits {
            hidden: false,
            cell_px: SPRITE_CELL_PX,
            phase_offset: 0.0,
            radial_offset: 0.0,
            lerp_scale: 1.08,
            happy_active,
            return_side: None,
        },
    };

    if happy_active {
        let seed_phase = ((persona.happy_seed & 0xFFFF) as f32 / 65535.0) * TWO_PI;
        let pulse = sin01(draw_frame * 0.11 + seed_phase);
        traits.cell_px *= 1.04 + 0.22 * pulse;
        traits.phase_offset += 0.20 * sinf(draw_frame * 0.10 + seed_phase + phase);
        traits.radial_offset += 34.0 * sinf(draw_frame * 0.07 + seed_phase);
        traits.lerp_scale = 1.35;
    }

    if persona.parallel_scale_until != 0 && frame <= persona.parallel_scale_until {
        let elapsed = PARALLEL_SCALE_FRAMES.saturating_sub(persona.parallel_scale_until - frame);
        let t = (elapsed as f32 / PARALLEL_SCALE_FRAMES as f32).clamp(0.0, 1.0);
        let giant = scanout_w.min(scanout_h) as f32 * 0.5;
        traits.cell_px = SPRITE_CELL_PX + (giant - SPRITE_CELL_PX) * t * t;
        traits.lerp_scale = 1.8;
    }

    traits
}

fn normal_button_traits() -> ButtonVisualTraits {
    ButtonVisualTraits {
        hidden: false,
        cell_px: SPRITE_CELL_PX,
        phase_offset: 0.0,
        radial_offset: 0.0,
        lerp_scale: 1.0,
        happy_active: false,
        return_side: None,
    }
}

fn roll_button_mood(persona: &mut ButtonPersona, idx: usize, frame: u32, orbit_bucket: u32) {
    let seed = xorshift32(
        frame ^ orbit_bucket.rotate_left(5) ^ ((idx as u32).wrapping_mul(0x45D9_F3B)) ^ 0xB16B_00B5,
    );
    let roll = seed % 100;
    persona.mood = if roll < 18 {
        ButtonMood::Sad
    } else if feature_enabled(LEVEL_HAPPY_MOOD) && roll < 42 {
        if persona.next_happy_frame <= frame {
            schedule_next_happy_action(persona, frame, seed);
        }
        ButtonMood::Happy
    } else {
        ButtonMood::Normal
    };

    if persona.mood == ButtonMood::Normal
        && feature_enabled(LEVEL_PARALLEL_WORLD)
        && persona.parallel_return_frame == 0
        && seed % 1000 < PARALLEL_ROLL_PER_1000
    {
        persona.parallel_scale_until = frame.saturating_add(PARALLEL_SCALE_FRAMES);
        persona.parallel_return_frame = frame.saturating_add(PARALLEL_RETURN_DELAY_FRAMES);
        persona.parallel_side = ((seed >> 9) & 3) as u8;
    }
}

fn update_happy_action(persona: &mut ButtonPersona, idx: usize, frame: u32) -> bool {
    if persona.mood != ButtonMood::Happy {
        return false;
    }
    if persona.happy_until != 0 {
        if frame <= persona.happy_until {
            return true;
        }
        let seed = xorshift32(persona.happy_seed ^ frame ^ ((idx as u32) << 17));
        persona.happy_seed = seed;
        if feature_enabled(LEVEL_FULL_COMBO) && seed % 1000 < HAPPY_COMBO_CHANCE_PER_1000 {
            persona.happy_until = frame.saturating_add(HAPPY_ACTION_FRAMES);
            return true;
        }
        persona.happy_until = 0;
        schedule_next_happy_action(persona, frame, seed);
        return false;
    }
    if frame >= persona.next_happy_frame {
        let seed = xorshift32(persona.happy_seed ^ frame ^ ((idx as u32) << 11) ^ 0x51ED_F00D);
        persona.happy_seed = seed;
        persona.happy_until = frame.saturating_add(HAPPY_ACTION_FRAMES);
        return true;
    }
    false
}

fn schedule_next_happy_action(persona: &mut ButtonPersona, frame: u32, seed: u32) {
    let jitter = seed
        % (HAPPY_ACTION_JITTER_FRAMES
            .saturating_mul(2)
            .saturating_add(1));
    let offset = jitter as i32 - HAPPY_ACTION_JITTER_FRAMES as i32;
    let delay = (HAPPY_ACTION_BASELINE_FRAMES as i32 + offset).max(1) as u32;
    persona.next_happy_frame = frame.saturating_add(delay);
}

fn spawn_parallel_return(button: &mut OrbitButton, side: u8, scanout_w: u32, scanout_h: u32) {
    let w = scanout_w as f32;
    let h = scanout_h as f32;
    match side & 3 {
        0 => {
            button.x = -RETURN_SPAWN_MARGIN_PX;
            button.y = h * 0.28;
        }
        1 => {
            button.x = w + RETURN_SPAWN_MARGIN_PX;
            button.y = h * 0.72;
        }
        2 => {
            button.x = w * 0.32;
            button.y = -RETURN_SPAWN_MARGIN_PX;
        }
        _ => {
            button.x = w * 0.68;
            button.y = h + RETURN_SPAWN_MARGIN_PX;
        }
    }
    button.vx = 0.0;
    button.vy = 0.0;
    button.alive = true;
}

fn apply_button_radial_offset(
    target_x: &mut f32,
    target_y: &mut f32,
    center_x: f32,
    center_y: f32,
    offset: f32,
) {
    if offset == 0.0 {
        return;
    }
    let dx = *target_x - center_x;
    let dy = *target_y - center_y;
    let len = sqrtf(dx * dx + dy * dy).max(1.0);
    *target_x += dx / len * offset;
    *target_y += dy / len * offset;
}

fn wavy_ring_target(cx: f32, cy: f32, phase: f32, frame: f32, orbiting: bool) -> (f32, f32) {
    let t = frame;
    let spin = if feature_enabled(LEVEL_ORBIT_MOTION) {
        if orbiting { t * 0.018 } else { t * 0.004 }
    } else {
        0.0
    };
    let angle = phase + spin;
    let ripple_phase = t * 0.075;
    let (fine, medium, slow, radius_wave) = if feature_enabled(LEVEL_RADIUS_WAVE) {
        let lobe = 0.5 + 0.5 * sinf(angle * 3.0 + ripple_phase * 0.65);
        (
            sinf(angle * 10.0 - ripple_phase + phase * 0.25) * 9.0,
            sinf(angle * 5.0 - ripple_phase * 1.7 + phase) * (12.0 + 20.0 * lobe),
            sinf(angle * 2.0 + ripple_phase * 0.45) * 7.0,
            1.0 + ORBIT_RADIUS_WAVE_AMOUNT * sinf(t * ORBIT_RADIUS_WAVE_SPEED),
        )
    } else {
        (0.0, 0.0, 0.0, 1.0)
    };
    let radius = ORBIT_RADIUS_PX * radius_wave + fine + medium + slow;
    (cx + cosf(angle) * radius, cy + sinf(angle) * radius)
}

fn distorted_ring_target(
    base_x: f32,
    base_y: f32,
    phase: f32,
    frame: f32,
    seed: u32,
) -> (f32, f32) {
    let seed_phase = ((seed & 0xFFFF) as f32 / 65535.0) * TWO_PI;
    let angle = frame * 0.082 + phase * 1.37 + seed_phase;
    let pulse = 0.82 + 0.18 * sinf(frame * 0.14 + seed_phase);
    let radius = DISTORTION_INNER_RADIUS_PX * pulse;
    (base_x + cosf(angle) * radius, base_y + sinf(angle) * radius)
}

fn orbit_action_frame(frame: u32) -> f32 {
    frame as f32 * orbit_action_speed()
}

fn orbit_subframe(frame: u32, step: u32) -> f32 {
    frame.saturating_sub(1) as f32 * orbit_action_speed() + step as f32 + 1.0
}

fn orbit_action_speed() -> f32 {
    ORBIT_ACTION_SPEED_NUM as f32 / ORBIT_ACTION_SPEED_DEN as f32
}

fn orbit_action_tail_weight() -> f32 {
    ORBIT_ACTION_TAIL_STEPS as f32 / ORBIT_ACTION_SPEED_DEN as f32
}

fn distortion_frame_active(frame: u32, until_frame: u32) -> bool {
    until_frame != 0 && frame <= until_frame
}

fn abs_f32(value: f32) -> f32 {
    if value < 0.0 { -value } else { value }
}

fn button_phase(idx: usize) -> f32 {
    TWO_PI * (idx as f32) / (BUTTON_COUNT as f32)
}

fn orbit_moon_phase_slot(idx: usize) -> Option<u16> {
    let ch = char::from_u32(*ORBIT_MOON_PHASES.get(idx)?)?;
    crate::ui3::althlasfont::twemoji::twemoji_lookup_glyph_region(ch).map(|region| region.slot)
}

fn orbit_center_butterfly_slot() -> Option<u16> {
    let ch = char::from_u32(ORBIT_CENTER_BUTTERFLY)?;
    crate::ui3::althlasfont::twemoji::twemoji_lookup_glyph_region(ch).map(|region| region.slot)
}

fn draw_center_butterfly(
    surface: OrbitFrameBuffer,
    atlas: &OrbitAtlas,
    center_x: f32,
    center_y: f32,
    frame: f32,
) {
    let Some(slot) = orbit_center_butterfly_slot() else {
        return;
    };
    let button = OrbitButton {
        x: center_x,
        y: center_y,
        vx: 0.0,
        vy: 0.0,
        slot,
        alive: true,
    };
    draw_orbit_button(surface, atlas, button, BUTTON_COUNT, frame, CENTER_SPRITE_CELL_PX);
    draw_button_particles(surface, center_x, center_y, BUTTON_COUNT, frame, 1, false);
}

fn orbit_atlas_once() -> Option<&'static OrbitAtlas> {
    ORBIT_ATLAS
        .call_once(|| {
            let decoded = crate::ui3::img::png_codec::decode_png_rgba(
                crate::ui3::althlasfont::twemoji::TWEMOJI_ATLAS_PNG,
            )
            .ok()?;
            Some(OrbitAtlas {
                width: decoded.width,
                height: decoded.height,
                rgba: decoded.rgba,
            })
        })
        .as_ref()
}

fn draw_orbit_button(
    surface: OrbitFrameBuffer,
    atlas: &OrbitAtlas,
    button: OrbitButton,
    idx: usize,
    frame: f32,
    cell_px: f32,
) {
    let Some(region) = crate::ui3::althlasfont::twemoji::twemoji_lookup_slot_region(button.slot)
    else {
        return;
    };
    let time = frame * 0.08;
    let phase = button_phase(idx);
    let scale = if feature_enabled(LEVEL_BUTTON_ANIM) {
        1.0 + 0.25 * cosf(time + phase)
    } else {
        1.0
    };
    let rotation = if feature_enabled(LEVEL_BUTTON_ANIM) {
        0.10 * sinf(time * 0.7 + phase)
    } else {
        0.0
    };
    let half = cell_px * scale * 0.72;
    let min_x = floorf(button.x - half) as i32;
    let min_y = floorf(button.y - half) as i32;
    let max_x = ceilf(button.x + half) as i32;
    let max_y = ceilf(button.y + half) as i32;
    let clip_x0 = max(0, min_x);
    let clip_y0 = max(0, min_y);
    let clip_x1 = min(surface.width as i32, max_x);
    let clip_y1 = min(surface.height as i32, max_y);
    if clip_x0 >= clip_x1 || clip_y0 >= clip_y1 {
        return;
    }

    let cos_r = cosf(rotation);
    let sin_r = sinf(rotation);
    let inv_scale = scale.recip();
    let dst_pitch_pixels = (surface.pitch_bytes as usize) / core::mem::size_of::<u32>();
    let src_w = region.src_w as f32;
    let src_h = region.src_h as f32;
    let src_cx = src_w * 0.5;
    let src_cy = src_h * 0.5;

    for y in clip_y0..clip_y1 {
        for x in clip_x0..clip_x1 {
            let dx = (x as f32 + 0.5 - button.x) * inv_scale;
            let dy = (y as f32 + 0.5 - button.y) * inv_scale;
            let src_xf = dx * cos_r + dy * sin_r + src_cx;
            let src_yf = -dx * sin_r + dy * cos_r + src_cy;
            if src_xf < 0.0 || src_yf < 0.0 || src_xf >= src_w || src_yf >= src_h {
                continue;
            }

            let sx = region.src_x as u32 + floorf(src_xf) as u32;
            let sy = region.src_y as u32 + floorf(src_yf) as u32;
            if sx >= atlas.width || sy >= atlas.height {
                continue;
            }
            let src_idx = ((sy as usize) * (atlas.width as usize) + sx as usize) * 4;
            let Some(src) = read_rgba8(atlas.rgba.as_slice(), src_idx) else {
                continue;
            };
            if ((src >> 24) & 0xFF) == 0 {
                continue;
            }

            let dst_idx = (y as usize).saturating_mul(dst_pitch_pixels) + x as usize;
            unsafe {
                let dst_ptr = (surface.virt as *mut u32).add(dst_idx);
                let dst = core::ptr::read_volatile(dst_ptr);
                core::ptr::write_volatile(dst_ptr, src_over(src, dst));
            }
        }
    }
}

fn draw_happy_button_echoes(
    surface: OrbitFrameBuffer,
    atlas: &OrbitAtlas,
    button: OrbitButton,
    idx: usize,
    frame: f32,
    cell_px: f32,
) {
    let phase = button_phase(idx);
    let split = 18.0 + 18.0 * sin01(frame * 0.18 + phase);
    for echo_idx in 0..2 {
        let sign = if echo_idx == 0 { -1.0 } else { 1.0 };
        let angle = frame * 0.13 * sign + phase + sign * 1.5707964;
        let mut echo = button;
        echo.x += cosf(angle) * split;
        echo.y += sinf(angle) * split;
        draw_orbit_button(surface, atlas, echo, idx, frame + sign * 3.0, cell_px * 0.55);
    }
}

fn draw_button_particles(
    surface: OrbitFrameBuffer,
    center_x: f32,
    center_y: f32,
    idx: usize,
    frame: f32,
    particle_multiplier: usize,
    violent: bool,
) {
    if !center_x.is_finite() || !center_y.is_finite() {
        return;
    }

    let frame_u = if frame > 0.0 { frame as u32 } else { 0 };
    let particle_count = PARTICLES_PER_BUTTON.saturating_mul(particle_multiplier.max(1));
    for particle_idx in 0..particle_count {
        let offset_seed = particle_seed(idx, particle_idx, 0);
        let age_offset = xorshift32(offset_seed as u32) % PARTICLE_LIFE_FRAMES;
        let stream_frame = frame_u.wrapping_add(age_offset);
        let cycle = stream_frame / PARTICLE_LIFE_FRAMES;
        let age_frame = stream_frame % PARTICLE_LIFE_FRAMES;
        let age = age_frame as f32 / PARTICLE_LIFE_FRAMES as f32;
        let decay = 1.0 - age;

        let mut rng = crate::tyche::SoftRng::from_seed(particle_seed(idx, particle_idx, cycle));
        let angle = rng_unit(&mut rng) * TWO_PI;
        let axis = PARTICLE_AXIS_PX * (0.72 + rng_unit(&mut rng) * 0.56);
        let orbital_radius = PARTICLE_MIN_R_PX + axis * age;
        let speed = vis_viva_particle_speed(orbital_radius, axis);
        let wobble_phase = rng_unit(&mut rng) * TWO_PI;
        let wobble_scale = if violent {
            ACTIVE_PARTICLE_WOBBLE_SCALE
        } else {
            1.0
        };
        let wobble = sinf(age * TWO_PI + wobble_phase) * PARTICLE_WOBBLE_PX * wobble_scale * decay;
        let violence_push = if violent {
            14.0 * sin01(frame * 0.21 + particle_idx as f32)
        } else {
            0.0
        };
        let distance =
            (PARTICLE_EMIT_CLEARANCE_PX + orbital_radius + speed * age * 10.0 + violence_push)
                .min(PARTICLE_DISTANCE_CAP_PX + violence_push);
        let normal_angle = angle + 1.5707964;
        let x = center_x + cosf(angle) * distance + cosf(normal_angle) * wobble;
        let y = center_y + sinf(angle) * distance + sinf(normal_angle) * wobble;
        let alpha = (PARTICLE_ALPHA_MAX as f32 * decay) as u32;
        let tint = rng_unit(&mut rng);
        let (red, green, blue) = if tint < 0.5 {
            (255u32, 86 + (90.0 * decay) as u32, 32u32)
        } else {
            (68u32, 210 + (35.0 * decay) as u32, 255u32)
        };
        let radius = PARTICLE_RADIUS_PX
            * if violent {
                1.0 + 0.45 * sin01(frame * 0.33 + particle_idx as f32)
            } else {
                1.0
            };
        if feature_enabled(LEVEL_PARTICLE_TRAILS) {
            draw_particle_trail(
                surface,
                x,
                y,
                angle,
                radius,
                red,
                green,
                blue,
                alpha,
                if violent {
                    ACTIVE_PARTICLE_TRAIL_SCALE
                } else {
                    1.0
                },
            );
        }

        draw_particle_disc(surface, x, y, radius * (0.85 + decay * 0.55), red, green, blue, alpha);
    }
}

fn draw_particle_trail(
    surface: OrbitFrameBuffer,
    x: f32,
    y: f32,
    angle: f32,
    radius: f32,
    red: u32,
    green: u32,
    blue: u32,
    alpha: u32,
    spacing_scale: f32,
) {
    if alpha == 0 {
        return;
    }
    let back_x = -cosf(angle);
    let back_y = -sinf(angle);
    for step in 1..=PARTICLE_TRAIL_STEPS {
        let fade_num = (PARTICLE_TRAIL_STEPS + 1 - step) as u32;
        let fade_den = (PARTICLE_TRAIL_STEPS + 2) as u32;
        let trail_alpha = alpha.saturating_mul(fade_num) / fade_den;
        if trail_alpha == 0 {
            continue;
        }
        let offset = PARTICLE_TRAIL_SPACING_PX * spacing_scale * step as f32;
        draw_particle_disc(
            surface,
            x + back_x * offset,
            y + back_y * offset,
            radius * (0.66 - 0.10 * step as f32).max(0.28),
            red,
            green,
            blue,
            trail_alpha,
        );
    }
}

fn draw_disco_gradients(surface: OrbitFrameBuffer, frame: f32, seed: u32) {
    if surface.width == 0 || surface.height == 0 {
        return;
    }

    let w = surface.width as f32;
    let h = surface.height as f32;
    let min_extent = w.min(h).max(1.0);
    for idx in 0..DISCO_GRADIENT_COUNT {
        let mut rng = crate::tyche::SoftRng::from_seed(particle_seed(
            BUTTON_COUNT + 1,
            idx,
            seed ^ frame as u32,
        ));
        let base_x = rng_unit(&mut rng) * w;
        let base_y = rng_unit(&mut rng) * h;
        let drift_phase = rng_unit(&mut rng) * TWO_PI;
        let drift = min_extent * 0.07;
        let x = (base_x + cosf(frame * 0.014 + drift_phase) * drift).clamp(0.0, w - 1.0);
        let y = (base_y + sinf(frame * 0.017 + drift_phase * 1.3) * drift).clamp(0.0, h - 1.0);
        let radius = min_extent * (0.10 + rng_unit(&mut rng) * 0.18);
        let hue = rng_unit(&mut rng);
        let red = (96.0 + 159.0 * sin01(hue * TWO_PI + frame * 0.011)) as u32;
        let green = (96.0 + 159.0 * sin01(hue * TWO_PI + 2.094 + frame * 0.015)) as u32;
        let blue = (96.0 + 159.0 * sin01(hue * TWO_PI + 4.188 + frame * 0.019)) as u32;
        let alpha = 18 + (18.0 * sin01(frame * 0.03 + idx as f32)) as u32;
        draw_particle_disc(surface, x, y, radius, red, green, blue, alpha);
    }
}

fn draw_particle_disc(
    surface: OrbitFrameBuffer,
    center_x: f32,
    center_y: f32,
    radius: f32,
    red: u32,
    green: u32,
    blue: u32,
    alpha: u32,
) {
    if alpha == 0 || radius <= 0.0 {
        return;
    }
    if radius < 1.0 {
        draw_particle_point(surface, center_x, center_y, red, green, blue, alpha);
        return;
    }

    let min_x = floorf(center_x - radius) as i32;
    let min_y = floorf(center_y - radius) as i32;
    let max_x = ceilf(center_x + radius) as i32;
    let max_y = ceilf(center_y + radius) as i32;
    let clip_x0 = max(0, min_x);
    let clip_y0 = max(0, min_y);
    let clip_x1 = min(surface.width as i32, max_x);
    let clip_y1 = min(surface.height as i32, max_y);
    if clip_x0 >= clip_x1 || clip_y0 >= clip_y1 {
        return;
    }

    let radius_sq = radius * radius;
    let pitch_pixels = (surface.pitch_bytes as usize) / core::mem::size_of::<u32>();
    for y in clip_y0..clip_y1 {
        for x in clip_x0..clip_x1 {
            let dx = x as f32 + 0.5 - center_x;
            let dy = y as f32 + 0.5 - center_y;
            let dist_sq = dx * dx + dy * dy;
            if dist_sq > radius_sq {
                continue;
            }
            let falloff = 1.0 - dist_sq / radius_sq;
            let local_alpha = ((alpha as f32) * falloff).clamp(0.0, 255.0) as u32;
            if local_alpha == 0 {
                continue;
            }
            let src =
                (local_alpha << 24) | ((blue & 0xFF) << 16) | ((green & 0xFF) << 8) | (red & 0xFF);
            let dst_idx = (y as usize).saturating_mul(pitch_pixels) + x as usize;
            unsafe {
                let dst_ptr = (surface.virt as *mut u32).add(dst_idx);
                let dst = core::ptr::read_volatile(dst_ptr);
                core::ptr::write_volatile(dst_ptr, src_over(src, dst));
            }
        }
    }
}

fn draw_particle_point(
    surface: OrbitFrameBuffer,
    center_x: f32,
    center_y: f32,
    red: u32,
    green: u32,
    blue: u32,
    alpha: u32,
) {
    if surface.width == 0 || surface.height == 0 {
        return;
    }
    let x = floorf(center_x + 0.5) as i32;
    let y = floorf(center_y + 0.5) as i32;
    if x < 0 || y < 0 || x >= surface.width as i32 || y >= surface.height as i32 {
        return;
    }

    let pitch_pixels = (surface.pitch_bytes as usize) / core::mem::size_of::<u32>();
    let src = ((alpha & 0xFF) << 24) | ((blue & 0xFF) << 16) | ((green & 0xFF) << 8) | (red & 0xFF);
    let dst_idx = (y as usize).saturating_mul(pitch_pixels) + x as usize;
    unsafe {
        let dst_ptr = (surface.virt as *mut u32).add(dst_idx);
        let dst = core::ptr::read_volatile(dst_ptr);
        core::ptr::write_volatile(dst_ptr, src_over(src, dst));
    }
}

fn vis_viva_particle_speed(radius: f32, semi_major_axis: f32) -> f32 {
    let r = radius.max(1.0);
    let a = semi_major_axis.max(r + 1.0);
    sqrtf((PARTICLE_MU * (2.0 / r - 1.0 / a)).max(0.0))
}

fn particle_seed(idx: usize, particle_idx: usize, cycle: u32) -> u64 {
    let a = ((idx as u64) + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let b = ((particle_idx as u64) + 1).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let c = (cycle as u64).wrapping_mul(0x94D0_49BB_1331_11EB);
    a ^ b.rotate_left(17) ^ c.rotate_right(11)
}

fn rng_unit(rng: &mut crate::tyche::SoftRng) -> f32 {
    rng.next_u32() as f32 / u32::MAX as f32
}

fn sin01(value: f32) -> f32 {
    0.5 + 0.5 * sinf(value)
}

fn read_rgba8(bytes: &[u8], idx: usize) -> Option<u32> {
    let r = *bytes.get(idx)? as u32;
    let g = *bytes.get(idx + 1)? as u32;
    let b = *bytes.get(idx + 2)? as u32;
    let a = *bytes.get(idx + 3)? as u32;
    Some((a << 24) | (b << 16) | (g << 8) | r)
}

fn src_over(src: u32, dst: u32) -> u32 {
    let sa = (src >> 24) & 0xFF;
    if sa == 0 {
        return dst;
    }
    if sa == 255 {
        return src;
    }

    let sr = src & 0xFF;
    let sg = (src >> 8) & 0xFF;
    let sb = (src >> 16) & 0xFF;
    let da = (dst >> 24) & 0xFF;
    let dr = dst & 0xFF;
    let dg = (dst >> 8) & 0xFF;
    let db = (dst >> 16) & 0xFF;
    let inv = 255 - sa;
    let out_r = div255(sr * sa + dr * inv);
    let out_g = div255(sg * sa + dg * inv);
    let out_b = div255(sb * sa + db * inv);
    let out_a = sa + div255(da * inv);
    (out_a << 24) | (out_b << 16) | (out_g << 8) | out_r
}

fn div255(value: u32) -> u32 {
    (value + 127) / 255
}

fn xorshift32(mut x: u32) -> u32 {
    x ^= x << 13;
    x ^= x >> 17;
    x ^ (x << 5)
}
