#![allow(dead_code)]

use crate::intel::types::Rgba8;

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Ui2Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi2WindowInfo {
    pub id: u32,
    pub kind: u32,
    pub state: u32,
    pub decoration_mode: u32,
    pub icon_id: u32,
    pub visible: u32,
    pub hit_test_visible: u32,
    pub selected: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub content_x: i32,
    pub content_y: i32,
    pub content_width: u32,
    pub content_height: u32,
    pub decoration_x: i32,
    pub decoration_y: i32,
    pub decoration_width: u32,
    pub decoration_height: u32,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Ui2FontTier {
    Half = 0,
    OneX = 1,
    TwoX = 2,
    Third = 3,
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui2WindowDecorationButton {
    ToggleComposition = 0,
    Fork = 1,
    Minimize = 2,
    Restore = 3,
    ToggleMaximize = 4,
    PreserveVm = 5,
    Close = 6,
}

impl Ui2WindowDecorationButton {
    pub const COUNT: usize = 7;
    pub const ALL_MASK: u32 = (1 << Self::COUNT) - 1;

    #[inline]
    pub const fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::ToggleComposition),
            1 => Some(Self::Fork),
            2 => Some(Self::Minimize),
            3 => Some(Self::Restore),
            4 => Some(Self::ToggleMaximize),
            5 => Some(Self::PreserveVm),
            6 => Some(Self::Close),
            _ => None,
        }
    }
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui2WindowDecorationMode {
    System = 0,
    Client = 1,
    None = 2,
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum Ui2WindowResizeMode {
    #[default]
    Auto = 0,
    Live = 1,
    PreviewCommit = 2,
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui2WindowVerticalScrollbarSide {
    Left = 0,
    Right = 1,
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui2WindowHorizontalScrollbarSide {
    Top = 0,
    Bottom = 1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Ui2CursorOverlayGlyphSpec {
    pub tex_id: u32,
    pub draw_w_px: u16,
    pub draw_h_px: u16,
    pub src_x: u16,
    pub src_y: u16,
    pub src_w: u16,
    pub src_h: u16,
    pub atlas_w: u16,
    pub atlas_h: u16,
}

pub fn create_window(_title: &str, _rect: Ui2Rect, _z: i16, _alpha: u8) -> u32 {
    0
}

pub fn create_hosted_surface_content_window(
    _title: &str,
    _content_rect: Ui2Rect,
    _z: i16,
    _alpha: u8,
    _tex_id: u32,
    _blend_enabled: bool,
) -> u32 {
    0
}

pub(crate) fn set_window_vm_origin(_id: u32, _vm_id: Option<u8>) -> bool {
    false
}

pub fn window_info_by_id(_id: u32) -> Option<TrueosUi2WindowInfo> {
    None
}

pub fn close_window(_id: u32) -> bool {
    false
}

pub fn destroy_window(_id: u32) -> bool {
    false
}

pub fn focus_window(_id: u32) -> bool {
    false
}

pub fn begin_window_move(_id: u32) -> bool {
    false
}

pub fn request_window_content_present(_id: u32, _reason: &'static str) -> bool {
    false
}

pub fn request_window_content_region_present(
    _id: u32,
    _x: u32,
    _y: u32,
    _width: u32,
    _height: u32,
    _reason: &'static str,
) -> bool {
    false
}

pub fn request_full_recompose(_reason: &'static str) {}

pub fn set_window_title(_id: u32, _title: &str) -> bool {
    false
}

pub fn set_window_icon(_id: u32, _icon_id: u32) -> bool {
    false
}

pub fn set_window_decorations(_id: u32, _mode: Ui2WindowDecorationMode) -> bool {
    false
}

pub fn set_window_titlebar_visible(_id: u32, _visible: bool) -> bool {
    false
}

pub fn set_window_bottom_bar_visible(_id: u32, _visible: bool) -> bool {
    false
}

pub fn set_window_title_icon_visible(_id: u32, _visible: bool) -> bool {
    false
}

pub fn set_window_titlebar_button_visible(
    _id: u32,
    _button: Ui2WindowDecorationButton,
    _visible: bool,
) -> bool {
    false
}

pub fn set_window_resize_button_visible(_id: u32, _visible: bool) -> bool {
    false
}

pub fn set_window_hit_test_visible(_id: u32, _visible: bool) -> bool {
    false
}

pub fn set_window_left_scrollbar_visible(_id: u32, _visible: bool) -> bool {
    false
}

pub fn set_window_bottom_scrollbar_visible(_id: u32, _visible: bool) -> bool {
    false
}

pub fn set_window_vertical_scrollbar_side(_id: u32, _side: Ui2WindowVerticalScrollbarSide) -> bool {
    false
}

pub fn set_window_horizontal_scrollbar_side(
    _id: u32,
    _side: Ui2WindowHorizontalScrollbarSide,
) -> bool {
    false
}

pub fn set_window_rotate_buttons_visible(_id: u32, _visible: bool) -> bool {
    false
}

pub fn set_window_content_rotation_quadrants(_id: u32, _quadrants: u8) -> bool {
    false
}

pub fn set_window_resize_maintain_aspect(_id: u32, _maintain_aspect: bool) -> bool {
    false
}

pub fn set_window_content_preserve_scale(_id: u32, _preserve_scale: bool) -> bool {
    false
}

pub fn set_window_resize_mode(_id: u32, _resize_mode: Ui2WindowResizeMode) -> bool {
    false
}

pub(crate) fn cursor_overlay_glyph_spec(
    _cursor_id: u32,
    _slot_id: u32,
    _view_h: u32,
) -> Option<Ui2CursorOverlayGlyphSpec> {
    None
}

pub(crate) fn cursor_color_rgba8_for_cursor_id(cursor_id: u32) -> Rgba8 {
    const COLORS: [Rgba8; 6] = [
        Rgba8::new(255, 0, 0, 255),
        Rgba8::new(0, 160, 255, 255),
        Rgba8::new(0, 220, 120, 255),
        Rgba8::new(255, 190, 0, 255),
        Rgba8::new(220, 80, 255, 255),
        Rgba8::new(255, 255, 255, 255),
    ];
    COLORS[(cursor_id.saturating_sub(1) as usize) % COLORS.len()]
}

pub fn host_ui2_window_cursor_events(
    _window_id: u32,
    _out_cap: u32,
    _payload: &mut [u8],
) -> (usize, usize) {
    (0, 0)
}
