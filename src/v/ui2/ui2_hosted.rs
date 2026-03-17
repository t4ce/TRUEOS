pub(super) type HostedContentId = u32;
pub(super) type UiHostedSurfaceState = trueos_qjs::browser_task::HostedBrowserSurfaceState;
pub(super) type UiHostedInteractiveState = trueos_qjs::browser_task::HostedBrowserInteractiveState;
pub(super) type UiHostedKeyboardEvent = trueos_qjs::browser_task::HostedKeyboardEvent;

pub(super) const PRIMARY_HOSTED_CONTENT_ID: HostedContentId =
    trueos_qjs::browser_task::PRIMARY_BROWSER_INSTANCE_ID;
pub(super) const HOSTED_KEYBOARD_MOD_SHIFT: u8 =
    trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_SHIFT;
pub(super) const HOSTED_KEYBOARD_MOD_CTRL: u8 = trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_CTRL;
pub(super) const HOSTED_KEYBOARD_MOD_ALT: u8 = trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_ALT;
pub(super) const HOSTED_KEYBOARD_MOD_META: u8 = trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_META;

pub(super) trait UiHostedSurfaceProvider {
    fn surface_seq(&self, content_id: HostedContentId) -> u32;
    fn interactive_seq(&self, content_id: HostedContentId) -> u32;
    fn surface_state(&self, content_id: HostedContentId) -> UiHostedSurfaceState;
    fn interactive_state(&self, content_id: HostedContentId) -> UiHostedInteractiveState;
}

pub(super) trait UiHostedViewportSink {
    fn set_viewport(
        &self,
        content_id: HostedContentId,
        viewport_width: u32,
        viewport_height: u32,
        content_x: i32,
        content_y: i32,
        content_width: u32,
        content_height: u32,
    ) -> bool;
}

pub(super) enum UiHostedInput<'a> {
    Scroll { scroll_x: u32, scroll_y: u32 },
    Keyboard { events: &'a [UiHostedKeyboardEvent] },
}

pub(super) trait UiHostedInputSink {
    fn send_input(&self, content_id: HostedContentId, input: UiHostedInput<'_>) -> bool;
}

pub(super) trait UiHostedWindowBinder {
    fn bind_window(&self, content_id: HostedContentId, window_id: u32) -> bool;
    fn primary_window_id(&self) -> u32;
    fn window_id_for_content(&self, content_id: HostedContentId) -> u32;
}

struct BrowserUiHostedAdapter;

impl UiHostedSurfaceProvider for BrowserUiHostedAdapter {
    fn surface_seq(&self, content_id: HostedContentId) -> u32 {
        trueos_qjs::browser_task::hosted_surface_seq_for_browser(content_id)
    }

    fn interactive_seq(&self, content_id: HostedContentId) -> u32 {
        trueos_qjs::browser_task::hosted_interactive_seq_for_browser(content_id)
    }

    fn surface_state(&self, content_id: HostedContentId) -> UiHostedSurfaceState {
        trueos_qjs::browser_task::hosted_surface_state_for_browser(content_id)
    }

    fn interactive_state(&self, content_id: HostedContentId) -> UiHostedInteractiveState {
        trueos_qjs::browser_task::hosted_interactive_state_for_browser(content_id)
    }
}

impl UiHostedViewportSink for BrowserUiHostedAdapter {
    fn set_viewport(
        &self,
        content_id: HostedContentId,
        viewport_width: u32,
        viewport_height: u32,
        content_x: i32,
        content_y: i32,
        content_width: u32,
        content_height: u32,
    ) -> bool {
        trueos_qjs::browser_task::set_hosted_viewport_for_browser(
            content_id,
            viewport_width,
            viewport_height,
            content_x,
            content_y,
            content_width,
            content_height,
        )
    }
}

impl UiHostedInputSink for BrowserUiHostedAdapter {
    fn send_input(&self, content_id: HostedContentId, input: UiHostedInput<'_>) -> bool {
        match input {
            UiHostedInput::Scroll { scroll_x, scroll_y } => {
                trueos_qjs::browser_task::set_hosted_scroll_for_browser(
                    content_id, scroll_x, scroll_y,
                )
            }
            UiHostedInput::Keyboard { events } => {
                let window_id =
                    trueos_qjs::browser_task::browser_window_id_for_instance(content_id);
                if window_id == 0 {
                    return false;
                }
                trueos_qjs::browser_task::queue_hosted_keyboard_events(window_id, events)
            }
        }
    }
}

impl UiHostedWindowBinder for BrowserUiHostedAdapter {
    fn bind_window(&self, content_id: HostedContentId, window_id: u32) -> bool {
        trueos_qjs::browser_task::bind_browser_window_to_instance(content_id, window_id)
    }

    fn primary_window_id(&self) -> u32 {
        self.window_id_for_content(PRIMARY_HOSTED_CONTENT_ID)
    }

    fn window_id_for_content(&self, content_id: HostedContentId) -> u32 {
        trueos_qjs::browser_task::browser_window_id_for_instance(content_id)
    }
}

#[inline]
fn hosted_adapter() -> BrowserUiHostedAdapter {
    BrowserUiHostedAdapter
}

#[inline]
pub(super) fn hosted_bind_window(content_id: HostedContentId, window_id: u32) -> bool {
    hosted_adapter().bind_window(content_id, window_id)
}

#[inline]
pub(super) fn hosted_primary_window_id() -> u32 {
    hosted_adapter().primary_window_id()
}

#[inline]
pub(super) fn hosted_window_id_for_content(content_id: HostedContentId) -> u32 {
    hosted_adapter().window_id_for_content(content_id)
}

#[inline]
pub(super) fn hosted_surface_state(content_id: HostedContentId) -> UiHostedSurfaceState {
    hosted_adapter().surface_state(content_id)
}

#[inline]
pub(super) fn hosted_surface_seq(content_id: HostedContentId) -> u32 {
    hosted_adapter().surface_seq(content_id)
}

#[inline]
pub(super) fn hosted_interactive_state(content_id: HostedContentId) -> UiHostedInteractiveState {
    hosted_adapter().interactive_state(content_id)
}

#[inline]
pub(super) fn hosted_interactive_seq(content_id: HostedContentId) -> u32 {
    hosted_adapter().interactive_seq(content_id)
}

#[inline]
pub(super) fn hosted_set_viewport(
    content_id: HostedContentId,
    viewport_width: u32,
    viewport_height: u32,
    content_x: i32,
    content_y: i32,
    content_width: u32,
    content_height: u32,
) -> bool {
    hosted_adapter().set_viewport(
        content_id,
        viewport_width,
        viewport_height,
        content_x,
        content_y,
        content_width,
        content_height,
    )
}

#[inline]
pub(super) fn hosted_set_scroll(content_id: HostedContentId, scroll_x: u32, scroll_y: u32) -> bool {
    hosted_adapter().send_input(content_id, UiHostedInput::Scroll { scroll_x, scroll_y })
}

#[inline]
pub(super) fn hosted_set_scroll_y(content_id: HostedContentId, scroll_y: u32) -> bool {
    let current_x = hosted_surface_state(content_id).scroll_x;
    hosted_set_scroll(content_id, current_x, scroll_y)
}

#[inline]
pub(super) fn hosted_queue_keyboard_events(
    content_id: HostedContentId,
    events: &[UiHostedKeyboardEvent],
) -> bool {
    hosted_adapter().send_input(content_id, UiHostedInput::Keyboard { events })
}
