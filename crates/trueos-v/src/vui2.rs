use crate::vcabi;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WindowId(u32);

impl WindowId {
    #[inline]
    pub const fn new(raw: u32) -> Option<Self> {
        if raw == 0 { None } else { Some(Self(raw)) }
    }

    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }

    pub fn info(self) -> Option<WindowInfo> {
        let mut raw = vcabi::TrueosUi2WindowInfo::default();
        let rc = unsafe { vcabi::trueos_cabi_ui2_window_info(self.0, &mut raw as *mut _) };
        if rc == 0 {
            Some(WindowInfo::from_raw(raw))
        } else {
            None
        }
    }

    pub fn set_title(self, title: &str) -> bool {
        unsafe { vcabi::trueos_cabi_ui2_window_set_title(self.0, title.as_ptr(), title.len()) == 0 }
    }

    pub fn set_icon(self, icon_id: u32) -> bool {
        unsafe { vcabi::trueos_cabi_ui2_window_set_icon(self.0, icon_id) == 0 }
    }

    pub fn set_position(self, x: i32, y: i32) -> bool {
        unsafe { vcabi::trueos_cabi_ui2_window_set_position(self.0, x, y) == 0 }
    }

    pub fn set_size(self, width: u32, height: u32) -> bool {
        unsafe { vcabi::trueos_cabi_ui2_window_set_size(self.0, width.max(1), height.max(1)) == 0 }
    }

    pub fn set_decorations(self, mode: WindowDecorationMode) -> bool {
        unsafe { vcabi::trueos_cabi_ui2_window_set_decorations(self.0, mode as u32) == 0 }
    }

    pub fn set_decoration_options(self, options: WindowDecorationOptions) -> bool {
        let mut ok = true;
        ok &= self.set_decorations(options.mode);
        ok &= self.set_titlebar_visible(options.titlebar_visible);
        ok &= self.set_bottom_bar_visible(options.bottom_bar_visible);
        ok &= self.set_title_icon_visible(options.title_icon_visible);
        ok &= self.set_vertical_scrollbar_visible(options.vertical_scrollbar_visible);
        ok &= self.set_horizontal_scrollbar_visible(options.horizontal_scrollbar_visible);
        ok &= self.set_vertical_scrollbar_side(options.vertical_scrollbar_side);
        ok &= self.set_horizontal_scrollbar_side(options.horizontal_scrollbar_side);
        ok &= self.set_resize_mode(options.resize_mode);
        ok &= self.set_resize_maintain_aspect(options.resize_maintain_aspect);
        ok &= self.set_content_preserve_scale(options.content_preserve_scale);
        ok &= self.set_rotate_buttons_visible(options.rotate_buttons_visible);
        ok &= self.set_resize_button_visible(options.resize_button_visible);
        ok &= self.set_decoration_button_visible(
            WindowDecorationButton::ToggleComposition,
            options.buttons.toggle_composition,
        );
        ok &= self.set_decoration_button_visible(WindowDecorationButton::Fork, options.buttons.fork);
        ok &= self
            .set_decoration_button_visible(WindowDecorationButton::Minimize, options.buttons.minimize);
        ok &= self
            .set_decoration_button_visible(WindowDecorationButton::Restore, options.buttons.restore);
        ok &= self.set_decoration_button_visible(
            WindowDecorationButton::ToggleMaximize,
            options.buttons.toggle_maximize,
        );
        ok &= self
            .set_decoration_button_visible(WindowDecorationButton::PreserveVm, options.buttons.preserve_vm);
        ok &= self.set_decoration_button_visible(WindowDecorationButton::Close, options.buttons.close);
        ok
    }

    pub fn set_titlebar_visible(self, visible: bool) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_titlebar_visible(self.0, u32::from(visible)) == 0
        }
    }

    pub fn set_bottom_bar_visible(self, visible: bool) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_bottom_bar_visible(self.0, u32::from(visible)) == 0
        }
    }

    pub fn set_title_icon_visible(self, visible: bool) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_title_icon_visible(self.0, u32::from(visible)) == 0
        }
    }

    pub fn set_decoration_button_visible(
        self,
        button: WindowDecorationButton,
        visible: bool,
    ) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_decoration_button_visible(
                self.0,
                button as u32,
                u32::from(visible),
            ) == 0
        }
    }

    pub fn set_resize_button_visible(self, visible: bool) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_resize_button_visible(self.0, u32::from(visible))
                == 0
        }
    }

    pub fn set_hit_test_visible(self, visible: bool) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_hit_test_visible(self.0, u32::from(visible)) == 0
        }
    }

    pub fn set_vertical_scrollbar_visible(self, visible: bool) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_vertical_scrollbar_visible(self.0, u32::from(visible))
                == 0
        }
    }

    pub fn set_horizontal_scrollbar_visible(self, visible: bool) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_horizontal_scrollbar_visible(
                self.0,
                u32::from(visible),
            ) == 0
        }
    }

    pub fn set_resize_maintain_aspect(self, maintain_aspect: bool) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_resize_maintain_aspect(
                self.0,
                u32::from(maintain_aspect),
            ) == 0
        }
    }

    pub fn set_content_preserve_scale(self, preserve_scale: bool) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_content_preserve_scale(
                self.0,
                u32::from(preserve_scale),
            ) == 0
        }
    }

    pub fn set_resize_mode(self, mode: WindowResizeMode) -> bool {
        unsafe { vcabi::trueos_cabi_ui2_window_set_resize_mode(self.0, mode as u32) == 0 }
    }

    pub fn set_vertical_scrollbar_side(self, side: VerticalScrollbarSide) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_vertical_scrollbar_side(self.0, side as u32) == 0
        }
    }

    pub fn set_horizontal_scrollbar_side(self, side: HorizontalScrollbarSide) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_horizontal_scrollbar_side(self.0, side as u32) == 0
        }
    }

    pub fn set_rotate_buttons_visible(self, visible: bool) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_rotate_buttons_visible(self.0, u32::from(visible))
                == 0
        }
    }

    pub fn set_content_rotation_quadrants(self, quadrants: u32) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_content_rotation_quadrants(self.0, quadrants) == 0
        }
    }

    pub fn minimize(self) -> bool {
        unsafe { vcabi::trueos_cabi_ui2_window_minimize(self.0) == 0 }
    }

    pub fn maximize(self) -> bool {
        unsafe { vcabi::trueos_cabi_ui2_window_maximize(self.0) == 0 }
    }

    pub fn restore(self) -> bool {
        unsafe { vcabi::trueos_cabi_ui2_window_restore(self.0) == 0 }
    }

    pub fn focus(self) -> bool {
        unsafe { vcabi::trueos_cabi_ui2_window_focus(self.0) == 0 }
    }

    pub fn close(self) -> bool {
        unsafe { vcabi::trueos_cabi_ui2_window_close(self.0) == 0 }
    }

    pub fn begin_move(self) -> bool {
        unsafe { vcabi::trueos_cabi_ui2_window_begin_move(self.0) == 0 }
    }

    pub fn begin_resize(self, edge_mask: u32) -> bool {
        unsafe { vcabi::trueos_cabi_ui2_window_begin_resize(self.0, edge_mask) == 0 }
    }
}

#[derive(Debug)]
pub struct OwnedWindow {
    id: WindowId,
    close_on_drop: bool,
}

impl OwnedWindow {
    pub fn create(title: &str, rect: Rect) -> Option<Self> {
        Self::create_with_options(title, rect, CreateOptions::default())
    }

    pub fn create_with_options(title: &str, rect: Rect, options: CreateOptions) -> Option<Self> {
        let raw = unsafe {
            vcabi::trueos_cabi_ui2_window_create(
                title.as_ptr(),
                title.len(),
                rect.x,
                rect.y,
                rect.width.max(1),
                rect.height.max(1),
                options.z,
                options.alpha as u32,
            )
        };
        WindowId::new(raw).map(|id| {
            let _ = id.set_decoration_options(options.decorations);
            Self {
                id,
                close_on_drop: true,
            }
        })
    }

    pub fn from_existing(id: WindowId) -> Self {
        Self {
            id,
            close_on_drop: false,
        }
    }

    #[inline]
    pub const fn id(&self) -> WindowId {
        self.id
    }

    pub fn info(&self) -> Option<WindowInfo> {
        self.id.info()
    }

    pub fn leak(mut self) -> WindowId {
        self.close_on_drop = false;
        self.id
    }
}

impl Drop for OwnedWindow {
    fn drop(&mut self) {
        if self.close_on_drop {
            let _ = self.id.close();
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CreateOptions {
    pub z: i32,
    pub alpha: u8,
    pub decorations: WindowDecorationOptions,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self {
            z: 0,
            alpha: 255,
            decorations: WindowDecorationOptions::default(),
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub const RESIZE_LEFT: u32 = 1 << 0;
pub const RESIZE_TOP: u32 = 1 << 1;
pub const RESIZE_RIGHT: u32 = 1 << 2;
pub const RESIZE_BOTTOM: u32 = 1 << 3;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WindowState {
    Normal,
    Minimized,
    Maximized,
    Unknown(u32),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WindowDecorationMode {
    System = 0,
    Client = 1,
    None = 2,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WindowDecorationOptions {
    pub mode: WindowDecorationMode,
    pub titlebar_visible: bool,
    pub bottom_bar_visible: bool,
    pub title_icon_visible: bool,
    pub buttons: WindowDecorationButtons,
    pub resize_button_visible: bool,
    pub rotate_buttons_visible: bool,
    pub vertical_scrollbar_visible: bool,
    pub horizontal_scrollbar_visible: bool,
    pub vertical_scrollbar_side: VerticalScrollbarSide,
    pub horizontal_scrollbar_side: HorizontalScrollbarSide,
    pub resize_mode: WindowResizeMode,
    pub resize_maintain_aspect: bool,
    pub content_preserve_scale: bool,
}

impl WindowDecorationOptions {
    pub const fn system() -> Self {
        Self {
            mode: WindowDecorationMode::System,
            titlebar_visible: true,
            bottom_bar_visible: true,
            title_icon_visible: true,
            buttons: WindowDecorationButtons::all(),
            resize_button_visible: true,
            rotate_buttons_visible: false,
            vertical_scrollbar_visible: true,
            horizontal_scrollbar_visible: true,
            vertical_scrollbar_side: VerticalScrollbarSide::Left,
            horizontal_scrollbar_side: HorizontalScrollbarSide::Bottom,
            resize_mode: WindowResizeMode::Auto,
            resize_maintain_aspect: false,
            content_preserve_scale: false,
        }
    }

    pub const fn undecorated() -> Self {
        Self {
            mode: WindowDecorationMode::None,
            titlebar_visible: false,
            bottom_bar_visible: false,
            title_icon_visible: false,
            buttons: WindowDecorationButtons::none(),
            resize_button_visible: false,
            rotate_buttons_visible: false,
            vertical_scrollbar_visible: false,
            horizontal_scrollbar_visible: false,
            vertical_scrollbar_side: VerticalScrollbarSide::Left,
            horizontal_scrollbar_side: HorizontalScrollbarSide::Bottom,
            resize_mode: WindowResizeMode::Auto,
            resize_maintain_aspect: false,
            content_preserve_scale: false,
        }
    }
}

impl Default for WindowDecorationOptions {
    fn default() -> Self {
        Self::system()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WindowDecorationButtons {
    pub toggle_composition: bool,
    pub fork: bool,
    pub minimize: bool,
    pub restore: bool,
    pub toggle_maximize: bool,
    pub preserve_vm: bool,
    pub close: bool,
}

impl WindowDecorationButtons {
    pub const fn all() -> Self {
        Self {
            toggle_composition: true,
            fork: true,
            minimize: true,
            restore: true,
            toggle_maximize: true,
            preserve_vm: true,
            close: true,
        }
    }

    pub const fn none() -> Self {
        Self {
            toggle_composition: false,
            fork: false,
            minimize: false,
            restore: false,
            toggle_maximize: false,
            preserve_vm: false,
            close: false,
        }
    }

    pub const fn close_only() -> Self {
        Self {
            close: true,
            ..Self::none()
        }
    }
}

impl Default for WindowDecorationButtons {
    fn default() -> Self {
        Self::all()
    }
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WindowDecorationButton {
    ToggleComposition = 0,
    Fork = 1,
    Minimize = 2,
    Restore = 3,
    ToggleMaximize = 4,
    PreserveVm = 5,
    Close = 6,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WindowResizeMode {
    Auto = 0,
    Live = 1,
    PreviewCommit = 2,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VerticalScrollbarSide {
    Left = 0,
    Right = 1,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum HorizontalScrollbarSide {
    Top = 0,
    Bottom = 1,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WindowInfo {
    pub id: WindowId,
    pub kind: u32,
    pub state: WindowState,
    pub decoration_mode: u32,
    pub icon_id: u32,
    pub visible: bool,
    pub hit_test_visible: bool,
    pub selected: bool,
    pub frame: Rect,
    pub content: Rect,
    pub decoration: Rect,
}

impl WindowInfo {
    fn from_raw(raw: vcabi::TrueosUi2WindowInfo) -> Self {
        Self {
            id: WindowId(raw.id),
            kind: raw.kind,
            state: match raw.state {
                0 => WindowState::Normal,
                1 => WindowState::Minimized,
                2 => WindowState::Maximized,
                other => WindowState::Unknown(other),
            },
            decoration_mode: raw.decoration_mode,
            icon_id: raw.icon_id,
            visible: raw.visible != 0,
            hit_test_visible: raw.hit_test_visible != 0,
            selected: raw.selected != 0,
            frame: Rect {
                x: raw.x,
                y: raw.y,
                width: raw.width,
                height: raw.height,
            },
            content: Rect {
                x: raw.content_x,
                y: raw.content_y,
                width: raw.content_width,
                height: raw.content_height,
            },
            decoration: Rect {
                x: raw.decoration_x,
                y: raw.decoration_y,
                width: raw.decoration_width,
                height: raw.decoration_height,
            },
        }
    }
}
