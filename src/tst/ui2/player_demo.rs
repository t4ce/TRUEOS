/*
 * This demo remains opt-in by default because it opens the audio player on boot.
 *
 * The old "creates a lot of UI2 windows" warning came from the pre-reuse
 * hosted-surface path. This demo now uses
 * Ui2SurfaceWindow::get_or_create_for_hosted_content_with_size(...), so relaunches
 * reuse the content-bound window instead of creating a fresh one.
 *
 * To enable it again, see the ui2-player-demo TaskSpec in src/r/spawn_service.rs.
 */

use alloc::{string::String, vec, vec::Vec};

use crate::aud::{pattern::PatternBank, synth::SynthEngine};
use crate::r::ui2::{self, Ui2FontTier, Ui2HostedInteractiveRect, Ui2Rect};

const UI2_PLAYER_TEX_ID: u32 = crate::tst::ui2::ids::Ui2DemoTexId::Player.get();
const UI2_PLAYER_CONTENT_ID: u32 = crate::tst::ui2::ids::Ui2DemoContentId::Player.get();
const UI2_PLAYER_WINDOW_TITLE: &str = "player";
const UI2_PLAYER_VIEW_W: u32 = 328;
const UI2_PLAYER_VIEW_H: u32 = 84;
const UI2_PLAYER_WINDOW_X: f32 = 152.0;
const UI2_PLAYER_WINDOW_Y: f32 = 156.0;
const UI2_PLAYER_WINDOW_Z: i16 = 37;
const UI2_PLAYER_WINDOW_ALPHA: u8 = 0xFF;

const UI2_PLAYER_FONT_TIER: Ui2FontTier = Ui2FontTier::OneX;
const UI2_PLAYER_FONT_SIZE_CASE: usize = UI2_PLAYER_FONT_TIER.size_case();

const UI2_PLAYER_BG_RGBA: [u8; 4] = [0x13, 0x17, 0x1E, 0xFF];
const UI2_PLAYER_BUTTON_RGBA: [u8; 4] = [0x1D, 0x25, 0x31, 0xFF];
const UI2_PLAYER_BUTTON_BORDER_RGBA: [u8; 4] = [0x32, 0x44, 0x57, 0xFF];
const UI2_PLAYER_ICON_RGBA: [u8; 4] = [0xEB, 0xF2, 0xF8, 0xFF];
const UI2_PLAYER_PLAYING_RGBA: [u8; 4] = [0x1D, 0x5D, 0x46, 0xFF];
const UI2_PLAYER_PLAYING_BORDER_RGBA: [u8; 4] = [0x5A, 0xD0, 0xA3, 0xFF];
const UI2_PLAYER_PAUSED_RGBA: [u8; 4] = [0x5F, 0x47, 0x1E, 0xFF];
const UI2_PLAYER_PAUSED_BORDER_RGBA: [u8; 4] = [0xEF, 0xC1, 0x63, 0xFF];
const UI2_PLAYER_STOPPED_RGBA: [u8; 4] = [0x52, 0x23, 0x2A, 0xFF];
const UI2_PLAYER_STOPPED_BORDER_RGBA: [u8; 4] = [0xE7, 0x75, 0x8A, 0xFF];

const UI2_PLAYER_PAD_X: usize = 12;
const UI2_PLAYER_PAD_Y: usize = 12;
const UI2_PLAYER_BUTTON_GAP: usize = 10;
const UI2_PLAYER_BUTTON_MIN_W: usize = 42;
const UI2_PLAYER_BUTTON_MAX_W: usize = 54;
const UI2_PLAYER_BUTTON_H: usize = 42;

const UI2_PLAYER_ITEM_PREVIOUS: u32 = 1;
const UI2_PLAYER_ITEM_PLAY: u32 = 2;
const UI2_PLAYER_ITEM_PAUSE: u32 = 3;
const UI2_PLAYER_ITEM_STOP: u32 = 4;
const UI2_PLAYER_ITEM_NEXT: u32 = 5;

const UI2_PLAYER_TITLE_ICON: char = '\u{24C2}';

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlayerPlaybackState {
    Stopped,
    Playing,
    Paused,
}

struct PlayerTrack {
    name: String,
    samples: Vec<i16>,
}

struct PlayerRuntime {
    tracks: Vec<PlayerTrack>,
    current_track: usize,
    playback: PlayerPlaybackState,
    audio_ready: bool,
}

impl PlayerRuntime {
    fn new() -> Self {
        let audio_ready = crate::aud::init().is_ok();
        let tracks = build_preset_tracks();
        if !audio_ready {
            crate::log!("ui2-player-demo: audio init failed; controls will retry on play\n");
        }
        Self {
            tracks,
            current_track: 0,
            playback: PlayerPlaybackState::Stopped,
            audio_ready,
        }
    }

    fn ensure_audio(&mut self) -> bool {
        if self.audio_ready {
            return true;
        }
        self.audio_ready = crate::aud::init().is_ok();
        self.audio_ready
    }

    fn start_current(&mut self) {
        if self.tracks.is_empty() || !self.ensure_audio() {
            return;
        }
        let track = &self.tracks[self.current_track % self.tracks.len()];
        if crate::hda::start_looped_playback(track.samples.as_slice()).is_ok() {
            self.playback = PlayerPlaybackState::Playing;
            crate::log!(
                "ui2-player-demo: playing track={} bytes={}\n",
                track.name.as_str(),
                track.samples.len() * core::mem::size_of::<i16>()
            );
        }
    }

    fn pause(&mut self) {
        let _ = crate::aud::stop();
        self.playback = PlayerPlaybackState::Paused;
    }

    fn stop(&mut self) {
        let _ = crate::aud::stop();
        self.playback = PlayerPlaybackState::Stopped;
    }

    fn step_track(&mut self, delta: isize) {
        if self.tracks.is_empty() {
            return;
        }
        let len = self.tracks.len() as isize;
        let current = self.current_track as isize;
        self.current_track = (current + delta).rem_euclid(len) as usize;
        self.start_current();
    }

    fn click(&mut self, item_id: u32) {
        match item_id {
            UI2_PLAYER_ITEM_PREVIOUS => self.step_track(-1),
            UI2_PLAYER_ITEM_PLAY => self.start_current(),
            UI2_PLAYER_ITEM_PAUSE => self.pause(),
            UI2_PLAYER_ITEM_STOP => self.stop(),
            UI2_PLAYER_ITEM_NEXT => self.step_track(1),
            _ => {}
        }
    }
}

fn build_preset_tracks() -> Vec<PlayerTrack> {
    let mut bank = PatternBank::new();
    bank.load_presets();
    let mut engine = SynthEngine::new();
    let mut out = Vec::with_capacity(bank.patterns.len());
    for pattern in bank.patterns.iter() {
        out.push(PlayerTrack {
            name: String::from(pattern.name_str()),
            samples: pattern.render(&mut engine),
        });
    }
    out
}

fn fill_rect_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgba: [u8; 4],
) {
    let end_y = y.saturating_add(h).min(dst_height);
    let end_x = x.saturating_add(w).min(dst_width);
    for row in y.min(dst_height)..end_y {
        for col in x.min(dst_width)..end_x {
            let idx = (row * dst_width + col) * 4;
            dst[idx] = rgba[0];
            dst[idx + 1] = rgba[1];
            dst[idx + 2] = rgba[2];
            dst[idx + 3] = rgba[3];
        }
    }
}

fn stroke_rect_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    rect: Ui2Rect,
    rgba: [u8; 4],
) {
    let x = rect.x.max(0.0) as usize;
    let y = rect.y.max(0.0) as usize;
    let w = rect.w.max(0.0) as usize;
    let h = rect.h.max(0.0) as usize;
    if w == 0 || h == 0 {
        return;
    }
    fill_rect_rgba(dst, dst_width, dst_height, x, y, w, 1, rgba);
    fill_rect_rgba(
        dst,
        dst_width,
        dst_height,
        x,
        y.saturating_add(h.saturating_sub(1)),
        w,
        1,
        rgba,
    );
    fill_rect_rgba(dst, dst_width, dst_height, x, y, 1, h, rgba);
    fill_rect_rgba(
        dst,
        dst_width,
        dst_height,
        x.saturating_add(w.saturating_sub(1)),
        y,
        1,
        h,
        rgba,
    );
}

fn button_rect(index: usize, viewport_w: usize, viewport_h: usize) -> Ui2Rect {
    let usable_w = viewport_w.saturating_sub(UI2_PLAYER_PAD_X * 2 + UI2_PLAYER_BUTTON_GAP * 4);
    let button_w = (usable_w / 5).clamp(UI2_PLAYER_BUTTON_MIN_W, UI2_PLAYER_BUTTON_MAX_W);
    let total_w = button_w * 5 + UI2_PLAYER_BUTTON_GAP * 4;
    let origin_x = viewport_w.saturating_sub(total_w) / 2;
    let button_h = UI2_PLAYER_BUTTON_H.min(viewport_h.saturating_sub(UI2_PLAYER_PAD_Y * 2).max(1));
    let origin_y = viewport_h.saturating_sub(button_h) / 2;
    Ui2Rect {
        x: (origin_x + index * (button_w + UI2_PLAYER_BUTTON_GAP)) as f32,
        y: origin_y as f32,
        w: button_w as f32,
        h: button_h as f32,
    }
}

fn button_style(playback: PlayerPlaybackState, item_id: u32) -> ([u8; 4], [u8; 4], [u8; 4]) {
    match (playback, item_id) {
        (PlayerPlaybackState::Playing, UI2_PLAYER_ITEM_PLAY) => {
            (UI2_PLAYER_PLAYING_RGBA, UI2_PLAYER_PLAYING_BORDER_RGBA, UI2_PLAYER_ICON_RGBA)
        }
        (PlayerPlaybackState::Paused, UI2_PLAYER_ITEM_PAUSE) => {
            (UI2_PLAYER_PAUSED_RGBA, UI2_PLAYER_PAUSED_BORDER_RGBA, UI2_PLAYER_ICON_RGBA)
        }
        (PlayerPlaybackState::Stopped, UI2_PLAYER_ITEM_STOP) => {
            (UI2_PLAYER_STOPPED_RGBA, UI2_PLAYER_STOPPED_BORDER_RGBA, UI2_PLAYER_ICON_RGBA)
        }
        _ => (UI2_PLAYER_BUTTON_RGBA, UI2_PLAYER_BUTTON_BORDER_RGBA, UI2_PLAYER_ICON_RGBA),
    }
}

fn render_scene(
    viewport_w: u32,
    viewport_h: u32,
    atlases: &ui2::Ui2FontCpuAtlases,
    runtime: &PlayerRuntime,
) -> (Vec<u8>, Vec<Ui2HostedInteractiveRect>) {
    let width = viewport_w.max(1) as usize;
    let height = viewport_h.max(1) as usize;
    let mut pixels = vec![0u8; width * height * 4];
    for px in pixels.chunks_exact_mut(4) {
        px.copy_from_slice(UI2_PLAYER_BG_RGBA.as_slice());
    }

    let buttons = [
        (UI2_PLAYER_ITEM_PREVIOUS, '\u{23EE}'),
        (UI2_PLAYER_ITEM_PLAY, '\u{25B6}'),
        (UI2_PLAYER_ITEM_PAUSE, '\u{23F8}'),
        (UI2_PLAYER_ITEM_STOP, '\u{23F9}'),
        (UI2_PLAYER_ITEM_NEXT, '\u{23ED}'),
    ];

    let mut interactives = Vec::with_capacity(buttons.len());
    for (index, (item_id, icon)) in buttons.iter().enumerate() {
        let rect = button_rect(index, width, height);
        let (fill, border, icon_rgba) = button_style(runtime.playback, *item_id);
        fill_rect_rgba(
            pixels.as_mut_slice(),
            width,
            height,
            rect.x.max(0.0) as usize,
            rect.y.max(0.0) as usize,
            rect.w.max(0.0) as usize,
            rect.h.max(0.0) as usize,
            fill,
        );
        stroke_rect_rgba(pixels.as_mut_slice(), width, height, rect, border);
        let _ = ui2::ui2_font_blit_char_rgba(
            pixels.as_mut_slice(),
            width,
            height,
            atlases,
            UI2_PLAYER_FONT_TIER,
            *icon,
            rect,
            icon_rgba,
        );
        interactives.push(Ui2HostedInteractiveRect {
            item_id: *item_id,
            x: rect.x.max(0.0) as u32,
            y: rect.y.max(0.0) as u32,
            width: rect.w.max(0.0) as u32,
            height: rect.h.max(0.0) as u32,
        });
    }

    (pixels, interactives)
}

#[embassy_executor::task]
pub async fn ui2_player_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-player-demo");
    let Some(atlases) = ui2::ui2_font_decode_cpu_atlases(UI2_PLAYER_FONT_SIZE_CASE) else {
        return;
    };

    let Some(surface) = ui2::Ui2SurfaceWindow::get_or_create_for_hosted_content_with_size(
        UI2_PLAYER_WINDOW_TITLE,
        Ui2Rect {
            x: UI2_PLAYER_WINDOW_X,
            y: UI2_PLAYER_WINDOW_Y,
            w: UI2_PLAYER_VIEW_W as f32,
            h: UI2_PLAYER_VIEW_H as f32,
        },
        UI2_PLAYER_WINDOW_Z,
        UI2_PLAYER_WINDOW_ALPHA,
        UI2_PLAYER_CONTENT_ID,
        UI2_PLAYER_TEX_ID,
        true,
        UI2_PLAYER_VIEW_W,
        UI2_PLAYER_VIEW_H,
    ) else {
        return;
    };

    let _ = surface.bind_spawn_task("ui2-player-demo");
    let _ = ui2::set_window_title(surface.window_id(), UI2_PLAYER_WINDOW_TITLE);
    let _ = ui2::set_window_title_twemoji(surface.window_id(), UI2_PLAYER_TITLE_ICON);
    let _ = ui2::set_window_decorations(surface.window_id(), ui2::Ui2WindowDecorationMode::System);
    let _ = ui2::set_window_titlebar_visible(surface.window_id(), true);
    let _ = ui2::set_window_bottom_bar_visible(surface.window_id(), false);
    let _ = ui2::set_window_left_scrollbar_visible(surface.window_id(), false);
    let _ = ui2::set_window_bottom_scrollbar_visible(surface.window_id(), false);

    let mut runtime = PlayerRuntime::new();
    let mut last_viewport = (0u32, 0u32);
    let mut last_click_seq = 0u32;
    let mut needs_render = true;

    loop {
        if crate::r::spawn_service::task_stop_requested("ui2-player-demo") {
            break;
        }

        let viewport = crate::r::ui2::window_content_rect_by_id(surface.window_id())
            .map(|rect| (rect.w.max(1.0) as u32, rect.h.max(1.0) as u32))
            .unwrap_or((UI2_PLAYER_VIEW_W, UI2_PLAYER_VIEW_H));
        if viewport != last_viewport {
            last_viewport = viewport;
            needs_render = true;
        }

        if let Some((seq, item_id)) =
            crate::r::ui2::take_window_last_clicked_item(surface.window_id())
            && seq != last_click_seq
        {
            last_click_seq = seq;
            runtime.click(item_id);
            needs_render = true;
        }

        if needs_render {
            let (pixels, interactives) =
                render_scene(last_viewport.0, last_viewport.1, &atlases, &runtime);
            let _ = surface.bind_hosted_scroll_state(
                UI2_PLAYER_CONTENT_ID,
                last_viewport.0.max(1),
                last_viewport.1.max(1),
            );
            let _ = surface.set_interactives(interactives.as_slice());
            if !crate::r::io::cabi::queue_texture_rgba_image_upload_owned(
                surface.tex_id(),
                last_viewport.0.max(1),
                last_viewport.1.max(1),
                pixels,
                surface.window_id(),
                "ui2-player-demo-present",
            ) {
                break;
            }
            needs_render = false;
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-player-demo", 80).await {
            break;
        }
    }

    runtime.stop();
}
