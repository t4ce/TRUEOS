//! UI4 frame ownership and plain-text Shell3 presentation.

#[path = "copy.rs"]
mod copy;
#[path = "cpu.rs"]
mod cpu;
mod pan;

use super::RgbaColor;
use super::update::RenderedLine;
use crate::ui4::{
    DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FrameSpec, OutputId,
    PremultipliedRgba8, ScanoutFormat, Ui4FrameEscapeKeyAction, Ui4InputEvent, WindowCreate,
    WindowId, WindowInteraction, WindowOwner, WindowPlacement, WindowPlane,
    WindowSessionCloseRequest, WindowSessionId, begin_additional_window_session,
    commit_window_frame_replacement, create_frame, create_window, destroy_frame,
    finish_window_session_with_request, publish_window_frame, retire_frame_when_released,
    set_window_escape_key_action, window_resize_state,
};

const OWNER: WindowOwner = WindowOwner::SHELL3_SERVICE;
const BACKGROUND: RgbaColor = RgbaColor::BlackTransparent;
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
    frame: FrameHandle,
    session: WindowSessionId,
    window: WindowId,
    width: u32,
    height: u32,
    backend: Backend,
    closing: bool,
    broker_resized: bool,
    frame_contents: [Option<alloc::vec::Vec<RenderedLine>>; 2],
    scale: u32,
    clear_buffers: [bool; 2],
}

/// Per-show backend selection and UI4 publication state.
pub struct Show {
    backend: Backend,
    surface: Option<Ui4Surface>,
    poisoned: bool,
    font_scale: u32,
    pan: Option<pan::PanBuffer>,
    pan_budget: Option<crate::ui4::text_area::RasterBudget>,
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
            font_scale: 1,
            pan: None,
            pan_budget: None,
        }
    }

    pub(crate) const fn font_scale(&self) -> u32 {
        self.font_scale
    }

    pub(crate) fn set_font_scale(
        &mut self,
        scale: u32,
        columns: usize,
        rows: usize,
    ) -> Result<(u32, u32), &'static str> {
        let (width, height) = self
            .surface
            .as_ref()
            .map(|s| (s.width, s.height))
            .map(Ok)
            .unwrap_or_else(|| show_extent(columns, rows, self.font_scale))?;
        let extent = (
            width.max(super::MIN_COLUMNS as u32 * microfont::FWIDTH as u32 * scale),
            height.max(super::MIN_ROWS as u32 * microfont::FHEIGHT as u32 * scale),
        );
        if let Some(surface) = self.surface.as_mut() {
            if extent != (width, height) {
                let (mut placement, _) = window_resize_state(OWNER, surface.window)
                    .map_err(|_| "shell3-show-scale-state")?;
                placement.width = extent.0;
                placement.height = extent.1;
                crate::ui4::set_window_placement(OWNER, surface.window, placement)
                    .map_err(|_| "shell3-show-scale-minimum")?;
            }
            surface.scale = scale;
            surface.broker_resized = true;
            surface.frame_contents = [None, None];
            surface.clear_buffers = [true; 2];
        }
        self.font_scale = scale;
        Ok(extent)
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

    pub(crate) fn window(&self) -> Option<WindowId> {
        self.surface.as_ref().map(|surface| surface.window)
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

    pub(crate) async fn resize_to_current(
        &mut self,
        lines: &[&[(char, Option<RgbaColor>)]],
        area: Option<&super::update::MatrixAreaSnapshot>,
    ) -> Result<(), &'static str> {
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
        let mut staged = Ui4Surface {
            frame: replacement,
            session: current.session,
            window: current.window,
            width: placement.width,
            height: placement.height,
            backend: self.backend,
            closing: false,
            broker_resized: true,
            frame_contents: [None, None],
            scale: self.font_scale,
            clear_buffers: [false; 2],
        };
        let render_result = match self.backend {
            Backend::Cpu => cpu::present(&mut staged, lines, &[]),
            Backend::Copy => copy::present(&mut staged, lines, &[], &mut self.poisoned,
                &mut self.pan, self.pan_budget.get_or_insert_with(||
                    crate::ui4::text_area::RasterBudget::new(crate::allcaps::shell3::SH3_PANBUFFER_CAP_BYTES)), area).await,
            _ => Err("shell3-show-backend-not-implemented"),
        };
        if let Err(error) = render_result {
            if !self.poisoned {
                retire_frame_when_released(staged.frame);
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
            return Err("shell3-show-resize-commit");
        }
        let old = self
            .surface
            .replace(staged)
            .ok_or("shell3-show-surface-missing")?;
        retire_frame_when_released(old.frame);
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
    /// MetaFmt foreground colors are preserved; bold has no raster effect yet.
    pub(crate) async fn present(
        &mut self,
        lines: &[&[(char, Option<RgbaColor>)]],
        columns: usize,
        rows: usize,
        batch: &super::UpdateBatch,
        area: Option<&super::update::MatrixAreaSnapshot>,
    ) -> Result<(), &'static str> {
        if !matches!(self.backend, Backend::Copy | Backend::Cpu) {
            return Err("shell3-show-backend-not-implemented");
        }
        if self.poisoned {
            return Err("shell3-show-bcs0-allocation-pinned");
        }

        let requested_extent = show_extent(columns, rows, self.font_scale)?;
        let extent = self
            .surface
            .as_ref()
            .filter(|surface| surface.broker_resized)
            .map(|surface| (surface.width, surface.height))
            .unwrap_or(requested_extent);
        let (width, height) = extent;
        let recreate_surface = self.surface.as_ref().is_none_or(|surface| {
            surface.width != width || surface.height != height || surface.backend != self.backend
        });
        if recreate_surface {
            self.release_surface();
            self.surface = Some(create_surface(width, height, self.backend)?);
            self.surface.as_mut().unwrap().scale = self.font_scale;
        }
        if !recreate_surface
            && batch.segments.is_empty()
            && self
                .surface
                .as_ref()
                .is_some_and(|surface| !surface.clear_buffers.iter().any(|clear| *clear))
        {
            return Ok(());
        }
        let surface = self.surface.as_mut().ok_or("shell3-show-surface-missing")?;
        let damage = match self.backend {
            Backend::Cpu => cpu::present(surface, lines, &batch.segments)?,
            Backend::Copy => {
                copy::present(surface, lines, &batch.segments, &mut self.poisoned,
                    &mut self.pan, self.pan_budget.get_or_insert_with(||
                        crate::ui4::text_area::RasterBudget::new(crate::allcaps::shell3::SH3_PANBUFFER_CAP_BYTES)), area).await?
            }
            _ => return Err("shell3-show-backend-not-implemented"),
        };
        let damage = if recreate_surface {
            Some(DamageRect::FULL)
        } else {
            damage
        };
        if let Some(damage) = damage {
            publish_window_frame(OWNER, surface.window, damage)
                .map_err(|_| "shell3-show-window-publish")?;
        }
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
    }
}

impl Drop for Show {
    fn drop(&mut self) {
        self.release_surface();
    }
}

fn show_extent(columns: usize, rows: usize, scale: u32) -> Result<(u32, u32), &'static str> {
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().ok_or("shell3-show-scanout-unavailable")?;
    let min_width = (super::MIN_COLUMNS as u32)
        .saturating_mul((microfont::FWIDTH as u32).saturating_mul(scale));
    let min_height =
        (super::MIN_ROWS as u32).saturating_mul((microfont::FHEIGHT as u32).saturating_mul(scale));
    if screen_width < min_width || screen_height < min_height {
        return Err("shell3-show-invalid-extent");
    }
    let width = u32::try_from(columns)
        .unwrap_or(u32::MAX)
        .max(super::MIN_COLUMNS as u32)
        .saturating_mul((microfont::FWIDTH as u32).saturating_mul(scale))
        .min(screen_width);
    let height = u32::try_from(rows)
        .unwrap_or(u32::MAX)
        .max(super::MIN_ROWS as u32)
        .saturating_mul((microfont::FHEIGHT as u32).saturating_mul(scale))
        .min(screen_height);
    Ok((width, height))
}

fn create_surface(width: u32, height: u32, backend: Backend) -> Result<Ui4Surface, &'static str> {
    let output = OutputId::from_slot(0).ok_or("shell3-show-output-unavailable")?;
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().ok_or("shell3-show-scanout-unavailable")?;
    let frame = create_surface_frames(width, height, backend)?;
    let session = match begin_additional_window_session(OWNER) {
        Ok(session) => session,
        Err(_) => {
            let _ = destroy_frame(frame);
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
        return Err("shell3-show-escape-policy");
    }
    Ok(Ui4Surface {
        frame,
        session,
        window,
        width,
        height,
        backend,
        closing: false,
        broker_resized: false,
        frame_contents: [None, None],
        scale: 1,
        clear_buffers: [false; 2],
    })
}

pub(super) fn rendered_lines(
    lines: &[&[(char, Option<RgbaColor>)]],
) -> alloc::vec::Vec<RenderedLine> {
    lines.iter().map(|line| line.to_vec()).collect()
}

fn damage_for_segments(
    segments: &[super::SegmentUpdate],
    width: u32,
    height: u32,
    scale: u32,
) -> Option<DamageRect> {
    segments
        .iter()
        .filter_map(|segment| {
            let row = match segment.row {
                super::SpecialRows::TitleRow => 0,
                super::SpecialRows::StatusRow => 1,
                super::SpecialRows::PromtRow => 2,
                super::SpecialRows::MatrixRow(index) => index + 3,
            };
            let x = u32::try_from(segment.offset)
                .ok()?
                .saturating_mul((microfont::FWIDTH as u32).saturating_mul(scale));
            let y = (row as u32).saturating_mul((microfont::FHEIGHT as u32).saturating_mul(scale));
            let columns = segment.remove.max(segment.text.chars().count());
            let patch_width = u32::try_from(columns)
                .ok()?
                .saturating_mul((microfont::FWIDTH as u32).saturating_mul(scale));
            if patch_width == 0 || x >= width || y >= height {
                return None;
            }
            Some(DamageRect::new(
                x,
                y,
                patch_width.min(width - x),
                ((microfont::FHEIGHT as u32).saturating_mul(scale)).min(height - y),
            ))
        })
        .reduce(DamageRect::union)
}

fn create_surface_frames(
    width: u32,
    height: u32,
    backend: Backend,
) -> Result<FrameHandle, &'static str> {
    let output = OutputId::from_slot(0).ok_or("shell3-show-output-unavailable")?;
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
            return Err("shell3-show-frame-create");
        }
    };
    Ok(frame)
}
