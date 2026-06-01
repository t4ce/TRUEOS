use super::*;

const UI2_BTN_ICO_HOVER_RGBA: (u8, u8, u8, u8) = (0x00, 0x00, 0x00, 0xBF);
const UI2_BTN_ICO_TEXT_RGBA: (u8, u8, u8, u8) = (0x00, 0x00, 0x00, 0xFF);
const UI2_SYSTEM_BUTTON_TOGGLE_COMPOSITION_UNLOCKED_TWEMOJI: char = '\u{25FC}';
const UI2_SYSTEM_BUTTON_TOGGLE_COMPOSITION_LOCKED_TWEMOJI: char = '\u{25FB}';
const UI2_SYSTEM_BUTTON_FORK_TWEMOJI: char = '\u{2797}';
const UI2_SYSTEM_BUTTON_MINIMIZE_TWEMOJI: char = '\u{2796}';
const UI2_SYSTEM_BUTTON_MAXIMIZE_TWEMOJI: char = '\u{23F9}';
const UI2_SYSTEM_BUTTON_RESTORE_TWEMOJI: char = '\u{23CF}';
const UI2_SYSTEM_BUTTON_PRESERVE_VM_TWEMOJI: char = '\u{1F4BF}';
const UI2_SYSTEM_BUTTON_VM_PLAY_TWEMOJI: char = '\u{23F5}';
const UI2_SYSTEM_BUTTON_VM_PAUSE_TWEMOJI: char = '\u{23F8}';
const UI2_SYSTEM_BUTTON_TASK_OFFLINE_TWEMOJI: char = '\u{23EF}';
const UI2_SYSTEM_BUTTON_CLOSE_HULL_TWEMOJI: char = '\u{2716}';
const UI2_RESIZE_HANDLE_TWEMOJI: char = '\u{2198}';

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Ui2BtnIco {
    TwemojiFit(char),
    TwemojiFullCell(char),
    Text(&'static str),
}

pub(super) fn ui2_btn_ico_for_system_button(
    window: &Ui2Window,
    action: Ui2SystemButtonAction,
) -> Option<Ui2BtnIco> {
    match action {
        Ui2SystemButtonAction::ToggleComposition => {
            Some(Ui2BtnIco::TwemojiFit(if window.composition_locked {
                UI2_SYSTEM_BUTTON_TOGGLE_COMPOSITION_LOCKED_TWEMOJI
            } else {
                UI2_SYSTEM_BUTTON_TOGGLE_COMPOSITION_UNLOCKED_TWEMOJI
            }))
        }
        Ui2SystemButtonAction::Fork => Some(Ui2BtnIco::TwemojiFit(UI2_SYSTEM_BUTTON_FORK_TWEMOJI)),
        Ui2SystemButtonAction::Minimize => {
            Some(Ui2BtnIco::TwemojiFit(UI2_SYSTEM_BUTTON_MINIMIZE_TWEMOJI))
        }
        Ui2SystemButtonAction::Restore => {
            Some(Ui2BtnIco::TwemojiFit(UI2_SYSTEM_BUTTON_RESTORE_TWEMOJI))
        }
        Ui2SystemButtonAction::ToggleMaximize => {
            Some(Ui2BtnIco::TwemojiFit(UI2_SYSTEM_BUTTON_MAXIMIZE_TWEMOJI))
        }
        Ui2SystemButtonAction::PreserveVm => window
            .vm_origin_hint
            .then_some(Ui2BtnIco::TwemojiFit(UI2_SYSTEM_BUTTON_PRESERVE_VM_TWEMOJI)),
        Ui2SystemButtonAction::RotateLeft => Some(Ui2BtnIco::Text("90L")),
        Ui2SystemButtonAction::RotateRight => Some(Ui2BtnIco::Text("90R")),
        Ui2SystemButtonAction::Close => Some(Ui2BtnIco::TwemojiFit(if window.vm_origin_hint {
            let hv_status = crate::hv::status();
            if hv_status.running_count != 0 || hv_status.starting_count != 0 {
                UI2_SYSTEM_BUTTON_VM_PAUSE_TWEMOJI
            } else {
                UI2_SYSTEM_BUTTON_VM_PLAY_TWEMOJI
            }
        } else if window.spawn_task_index.is_some() {
            UI2_SYSTEM_BUTTON_TASK_OFFLINE_TWEMOJI
        } else {
            UI2_SYSTEM_BUTTON_CLOSE_HULL_TWEMOJI
        })),
    }
}

pub(super) fn ui2_btn_ico_for_resize() -> Ui2BtnIco {
    Ui2BtnIco::TwemojiFullCell(UI2_RESIZE_HANDLE_TWEMOJI)
}

pub(super) fn draw_ui2_btn_ico(
    state: &Ui2State,
    window: &Ui2Window,
    rect: Ui2Rect,
    ico: Ui2BtnIco,
    text_tier: Ui2FontTier,
    hovered: bool,
) -> bool {
    let base_rgba = match ico {
        Ui2BtnIco::Text(_) => modulate_rgba_alpha(UI2_BTN_ICO_TEXT_RGBA, window.alpha),
        Ui2BtnIco::TwemojiFit(_) | Ui2BtnIco::TwemojiFullCell(_) => (255, 255, 255, window.alpha),
    };
    let drew = draw_ui2_btn_ico_pass(state, rect, ico, text_tier, base_rgba);
    if drew && hovered {
        let hover_rgba = modulate_rgba_alpha(UI2_BTN_ICO_HOVER_RGBA, window.alpha);
        let _ = draw_ui2_btn_ico_pass(state, rect, ico, text_tier, hover_rgba);
    }
    drew
}

fn draw_ui2_btn_ico_pass(
    state: &Ui2State,
    rect: Ui2Rect,
    ico: Ui2BtnIco,
    text_tier: Ui2FontTier,
    rgba: (u8, u8, u8, u8),
) -> bool {
    match ico {
        Ui2BtnIco::TwemojiFit(ch) => draw_twemoji_fit(state, rect, ch, rgba),
        Ui2BtnIco::TwemojiFullCell(ch) => draw_twemoji_full_cell(state, rect, ch, rgba),
        Ui2BtnIco::Text(label) => ui2_font_draw_text_line_in_rect_with_tier_rgba_no_present(
            label,
            rect,
            text_tier,
            Ui2FontTextAlign::Center,
            Ui2FontVerticalAlign::Center,
            state.view_w,
            state.view_h,
            rgba,
        ),
    }
}

fn draw_twemoji_fit(state: &Ui2State, rect: Ui2Rect, ch: char, rgba: (u8, u8, u8, u8)) -> bool {
    let Some((texture, src_x, src_y, src_w, src_h, atlas_w, atlas_h)) =
        resolve_twemoji_source(ch, 1.0)
    else {
        return false;
    };

    let inset_rect =
        Ui2Rect::new(rect.x + 1.0, rect.y + 1.0, (rect.w - 2.0).max(1.0), (rect.h - 2.0).max(1.0));
    let scale = libm::fminf(inset_rect.w / src_w, inset_rect.h / src_h);
    let draw_w = libm::fmaxf(1.0, src_w * scale);
    let draw_h = libm::fmaxf(1.0, src_h * scale);
    let draw_x = inset_rect.x + ((inset_rect.w - draw_w) * 0.5).max(0.0);
    let draw_y = inset_rect.y + ((inset_rect.h - draw_h) * 0.5).max(0.0);

    draw_twemoji_source(
        state, texture, draw_x, draw_y, draw_w, draw_h, src_x, src_y, src_w, src_h, atlas_w,
        atlas_h, rgba,
    )
}

fn draw_twemoji_full_cell(
    state: &Ui2State,
    rect: Ui2Rect,
    ch: char,
    rgba: (u8, u8, u8, u8),
) -> bool {
    let Some((texture, src_x, src_y, src_w, src_h, atlas_w, atlas_h)) =
        resolve_twemoji_source(ch, 0.0)
    else {
        return false;
    };

    let draw_rect =
        Ui2Rect::new(rect.x + 1.0, rect.y + 1.0, (rect.w - 2.0).max(1.0), (rect.h - 2.0).max(1.0));

    draw_twemoji_source(
        state,
        texture,
        draw_rect.x,
        draw_rect.y,
        draw_rect.w,
        draw_rect.h,
        src_x,
        src_y,
        src_w,
        src_h,
        atlas_w,
        atlas_h,
        rgba,
    )
}

fn resolve_twemoji_source(
    ch: char,
    crop_px: f32,
) -> Option<(crate::gfx::althlasfont::AthlasBucketTexture, f32, f32, f32, f32, f32, f32)> {
    let glyph = crate::gfx::althlasfont::twemoji::twemoji_resolve_glyph(ch)?;
    if !glyph.ready {
        return None;
    }
    let texture = glyph.texture?;
    let src_x = f32::from(glyph.region.src_x) + crop_px;
    let src_y = f32::from(glyph.region.src_y) + crop_px;
    let src_w = f32::from(glyph.region.src_w.max(1)) - (crop_px * 2.0);
    let src_h = f32::from(glyph.region.src_h.max(1)) - (crop_px * 2.0);
    let atlas_w = f32::from(glyph.region.atlas_w.max(1));
    let atlas_h = f32::from(glyph.region.atlas_h.max(1));
    Some((texture, src_x, src_y, src_w.max(1.0), src_h.max(1.0), atlas_w, atlas_h))
}

fn draw_twemoji_source(
    state: &Ui2State,
    texture: crate::gfx::althlasfont::AthlasBucketTexture,
    draw_x: f32,
    draw_y: f32,
    draw_w: f32,
    draw_h: f32,
    src_x: f32,
    src_y: f32,
    src_w: f32,
    src_h: f32,
    atlas_w: f32,
    atlas_h: f32,
    rgba: (u8, u8, u8, u8),
) -> bool {
    draw_texture_rect_uv_rgba_no_present(
        texture.tex_id,
        draw_x,
        draw_y,
        draw_w,
        draw_h,
        src_x / atlas_w,
        src_y / atlas_h,
        (src_x + src_w) / atlas_w,
        (src_y + src_h) / atlas_h,
        state.view_w,
        state.view_h,
        true,
        rgba,
    )
}
