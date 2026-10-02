//! UI4 frame ownership and plain-text Shell3 presentation.

#[path = "copy.rs"]
mod copy;
#[path = "cpu.rs"]
mod cpu;

use super::RgbaColor;
use crate::ui4::{
    DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FrameSpec, OutputId,
    PremultipliedRgba8, ScanoutFormat, Ui4FrameEscapeKeyAction, Ui4InputEvent, WindowCreate,
    WindowId, WindowInteraction, WindowOwner, WindowPlacement, WindowPlane,
    WindowSessionCloseRequest, WindowSessionId, begin_additional_window_session,
    commit_window_frame_replacement, create_frame, create_window, destroy_frame,
    finish_window_session_with_request, publish_window_frame, retire_frame_when_released,
    set_window_escape_key_action, window_resize_state, writable_rgba_view,
};

const OWNER: WindowOwner = WindowOwner::SHELL3_SERVICE;
const BACKGROUND: RgbaColor = RgbaColor::Gray;
const FOREGROUND: RgbaColor = RgbaColor::White;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backend {
    #[default]
    Cpu,
    Render,
    Copy,
    VirGL,
    Headless, // nowhere
    Network,
    File,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Frontend {
    #[default]
    Ui4,
    Headless,
}

struct Ui4Surface {
    source: Option<FrameHandle>,
    frame: FrameHandle,
    session: WindowSessionId,
    window: WindowId,
    width: u32,
    height: u32,
    backend: Backend,
    closing: bool,
    broker_resized: bool,
}

/// Per-show backend selection and UI4 publication state.
pub struct Show {
    backend: Backend,
    surface: Option<Ui4Surface>,
    poisoned: bool,
}

impl Default for Show {
    fn default() -> Self {
        Self::new(Backend::Copy)
    }
}

impl Show {
    pub const fn new(backend: Backend) -> Self {
        Self {
            backend,
            surface: None,
            poisoned: false,
        }
    }

    pub const fn backend(&self) -> Backend {
        self.backend
    }

    pub fn set_backend(&mut self, backend: Backend) {
        self.backend = backend;
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.surface
            .as_ref()
            .is_none_or(|surface| crate::ui4::window_is_closed(OWNER, surface.window))
    }

    pub(crate) fn handles_window(&self, window: WindowId) -> bool {
        self.surface
            .as_ref()
            .is_some_and(|surface| surface.window == window)
    }

    pub(crate) fn resize_needed(&self) -> bool {
        let Some(surface) = self.surface.as_ref() else {
            return false;
        };
        window_resize_state(OWNER, surface.window).is_ok_and(|(placement, _)| {
            (placement.width, placement.height) != (surface.width, surface.height)
        })
    }

    pub(crate) fn resize_target_extent(&self) -> Option<(u32, u32)> {
        let surface = self.surface.as_ref()?;
        let (placement, _) = window_resize_state(OWNER, surface.window).ok()?;
        Some((placement.width, placement.height))
    }

    pub(crate) async fn resize_to_current(&mut self, lines: [&str; 3]) -> Result<(), &'static str> {
        if self.poisoned {
            return Err("shell3-show-bcs0-allocation-pinned");
        }
        let current = self.surface.as_ref().ok_or("shell3-show-surface-missing")?;
        let (placement, epoch) =
            window_resize_state(OWNER, current.window).map_err(|_| "shell3-show-resize-state")?;
        if (placement.width, placement.height) == (current.width, current.height) {
            return Ok(());
        }
        let replacement = create_surface_frames(placement.width, placement.height, self.backend)?;
        let staged = Ui4Surface {
            source: replacement.0,
            frame: replacement.1,
            session: current.session,
            window: current.window,
            width: placement.width,
            height: placement.height,
            backend: self.backend,
            closing: false,
            broker_resized: true,
        };
        let render_result = match self.backend {
            Backend::Cpu => cpu::present(&staged, lines),
            Backend::Copy => copy::present(&staged, lines, &mut self.poisoned).await,
            _ => Err("shell3-show-backend-not-implemented"),
        };
        if let Err(error) = render_result {
            if !self.poisoned {
                retire_frame_when_released(staged.frame);
                if let Some(source) = staged.source {
                    let _ = destroy_frame(source);
                }
            }
            return Err(error);
        }
        let commit = commit_window_frame_replacement(
            OWNER,
            staged.window,
            staged.frame,
            placement,
            epoch,
            DamageRect::FULL,
        );
        if commit.is_err() {
            retire_frame_when_released(staged.frame);
            if let Some(source) = staged.source {
                let _ = destroy_frame(source);
            }
            return Err("shell3-show-resize-commit");
        }
        let old = self
            .surface
            .replace(staged)
            .ok_or("shell3-show-surface-missing")?;
        retire_frame_when_released(old.frame);
        if let Some(source) = old.source {
            let _ = destroy_frame(source);
        }
        Ok(())
    }

    pub(crate) fn handle_escape(&mut self, event: &Ui4InputEvent) {
        let Ui4InputEvent::Keyboard(event) = event else {
            return;
        };
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        if event.window != surface.window
            || event.event.kind != crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
            || event.event.key_code != crate::r::keyboard::KEYBOARD_KEY_ESCAPE
        {
            return;
        }
        let session = surface.session;
        let window = surface.window;
        if finish_window_session_with_request(
            OWNER,
            session,
            WindowSessionCloseRequest::default().animate_and_retire_frames(),
        )
        .is_ok()
        {
            if let Some(surface) = self.surface.as_mut() {
                surface.closing = true;
            }
            crate::log_info!(target: "service";
                "sh3srv: Escape requested graceful UI4 close window={}\n",
                window.raw(),
            );
        }
    }

    /// Present the Shell3's three text strips in a UI4 window.
    /// Non-ASCII symbols currently become `-`; color and formatting metadata
    /// are intentionally ignored at this stage.
    pub(crate) async fn present(
        &mut self,
        lines: [&str; 3],
        columns: usize,
        rows: usize,
    ) -> Result<(), &'static str> {
        if !matches!(self.backend, Backend::Copy | Backend::Cpu) {
            return Err("shell3-show-backend-not-implemented");
        }
        if self.poisoned {
            return Err("shell3-show-bcs0-allocation-pinned");
        }

        let requested_extent = show_extent(columns, rows)?;
        let extent = self
            .surface
            .as_ref()
            .filter(|surface| surface.broker_resized)
            .map(|surface| (surface.width, surface.height))
            .unwrap_or(requested_extent);
        let (width, height) = extent;
        if self.surface.as_ref().is_none_or(|surface| {
            surface.width != width || surface.height != height || surface.backend != self.backend
        }) {
            self.release_surface();
            self.surface = Some(create_surface(width, height, self.backend)?);
        }
        let surface = self.surface.as_ref().ok_or("shell3-show-surface-missing")?;
        match self.backend {
            Backend::Cpu => cpu::present(surface, lines)?,
            Backend::Copy => copy::present(surface, lines, &mut self.poisoned).await?,
            _ => return Err("shell3-show-backend-not-implemented"),
        }
        publish_window_frame(OWNER, surface.window, DamageRect::FULL)
            .map_err(|_| "shell3-show-window-publish")?;
        Ok(())
    }

    fn release_surface(&mut self) {
        if self.poisoned {
            return;
        }
        let Some(surface) = self.surface.take() else {
            return;
        };
        retire_frame_when_released(surface.frame);
        if !surface.closing {
            let _ = finish_window_session_with_request(
                OWNER,
                surface.session,
                WindowSessionCloseRequest::default().animate_and_retire_frames(),
            );
        }
        if let Some(source) = surface.source {
            let _ = destroy_frame(source);
        }
    }
}

impl Drop for Show {
    fn drop(&mut self) {
        self.release_surface();
    }
}

fn show_extent(columns: usize, rows: usize) -> Result<(u32, u32), &'static str> {
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().ok_or("shell3-show-scanout-unavailable")?;
    let min_width = (super::MIN_COLUMNS as u32).saturating_mul(microfont::FWIDTH as u32);
    let min_height = (super::MIN_ROWS as u32).saturating_mul(microfont::FHEIGHT as u32);
    if screen_width < min_width || screen_height < min_height {
        return Err("shell3-show-invalid-extent");
    }
    let width = u32::try_from(columns)
        .unwrap_or(u32::MAX)
        .max(super::MIN_COLUMNS as u32)
        .saturating_mul(microfont::FWIDTH as u32)
        .min(screen_width);
    let height = u32::try_from(rows)
        .unwrap_or(u32::MAX)
        .max(super::MIN_ROWS as u32)
        .saturating_mul(microfont::FHEIGHT as u32)
        .min(screen_height);
    Ok((width, height))
}

fn create_surface(width: u32, height: u32, backend: Backend) -> Result<Ui4Surface, &'static str> {
    let output = OutputId::from_slot(0).ok_or("shell3-show-output-unavailable")?;
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().ok_or("shell3-show-scanout-unavailable")?;
    let (source, frame) = create_surface_frames(width, height, backend)?;
    let session = match begin_additional_window_session(OWNER) {
        Ok(session) => session,
        Err(_) => {
            let _ = destroy_frame(frame);
            if let Some(source) = source {
                let _ = destroy_frame(source);
            }
            return Err("shell3-show-session-create");
        }
    };
    let window = match create_window(WindowCreate {
        owner: OWNER,
        session,
        frame,
        output,
        plane: WindowPlane::Universal(crate::ui4::ALPHA_OVERLAY_PLANE_SLOT as u8),
        placement: WindowPlacement {
            x: screen_width.saturating_sub(width) as i32 / 2,
            y: screen_height.saturating_sub(height) as i32 / 2,
            width,
            height,
            z: 90,
            opacity: u8::MAX,
            visible: true,
        },
        interaction: WindowInteraction::APPLICATION,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = finish_window_session_with_request(
                OWNER,
                session,
                WindowSessionCloseRequest::default().animate_and_retire_frames(),
            );
            let _ = destroy_frame(frame);
            if let Some(source) = source {
                let _ = destroy_frame(source);
            }
            return Err("shell3-show-window-create");
        }
    };
    if set_window_escape_key_action(OWNER, window, Ui4FrameEscapeKeyAction::DeliverToApplication)
        .is_err()
    {
        let _ = finish_window_session_with_request(
            OWNER,
            session,
            WindowSessionCloseRequest::default().animate_and_retire_frames(),
        );
        if let Some(source) = source {
            let _ = destroy_frame(source);
        }
        return Err("shell3-show-escape-policy");
    }
    Ok(Ui4Surface {
        source,
        frame,
        session,
        window,
        width,
        height,
        backend,
        closing: false,
        broker_resized: false,
    })
}

fn create_surface_frames(
    width: u32,
    height: u32,
    backend: Backend,
) -> Result<(Option<FrameHandle>, FrameHandle), &'static str> {
    let output = OutputId::from_slot(0).ok_or("shell3-show-output-unavailable")?;
    let source = if backend == Backend::Copy {
        Some(
            create_frame(FrameSpec {
                output,
                content: FrameContent::Image,
                cadence: FrameCadence::Immutable,
                buffering: FrameBuffering::Single,
                format: ScanoutFormat::Rgba8888Premultiplied,
                width,
                height,
                base_color: None,
            })
            .map_err(|_| "shell3-show-source-frame-create")?,
        )
    } else {
        None
    };
    let frame = match create_frame(FrameSpec {
        output,
        content: if backend == Backend::Copy {
            FrameContent::CopyEngine
        } else {
            FrameContent::Image
        },
        cadence: FrameCadence::Dirty,
        buffering: FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::from_straight_rgba(
            BACKGROUND.rgba()[0],
            BACKGROUND.rgba()[1],
            BACKGROUND.rgba()[2],
            BACKGROUND.rgba()[3],
        )),
    }) {
        Ok(frame) => frame,
        Err(_) => {
            if let Some(source) = source {
                let _ = destroy_frame(source);
            }
            return Err("shell3-show-frame-create");
        }
    };
    Ok((source, frame))
}
