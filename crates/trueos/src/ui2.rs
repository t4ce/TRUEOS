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

    pub fn set_hit_test_visible(self, visible: bool) -> bool {
        unsafe {
            vcabi::trueos_cabi_ui2_window_set_hit_test_visible(self.0, u32::from(visible)) == 0
        }
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
        WindowId::new(raw).map(|id| Self {
            id,
            close_on_drop: true,
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
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self { z: 0, alpha: 255 }
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

pub fn primary_browser_window() -> Option<WindowId> {
    WindowId::new(unsafe { vcabi::trueos_cabi_ui2_primary_browser_window_id() })
}
