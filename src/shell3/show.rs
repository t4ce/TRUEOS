//! UI4 frame ownership and plain-text Shell3 presentation.

use alloc::{string::String, vec};
use trueos_time::{Duration, Timer};

use crate::ui4::{
    DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FrameSpec,
    OutputId, PremultipliedRgba8, ScanoutFormat, WindowCreate,
    WindowId, WindowInteraction, WindowOwner, WindowPlacement, WindowPlane,
    WindowSessionCloseRequest, WindowSessionId, acquire_frame_buffer, begin_window_session,
    cancel_frame_buffer, create_frame, create_window, destroy_frame, finish_window_session_with_request,
    publish_frame_buffer, publish_window_frame, retire_frame_when_released, writable_rgba_view,
};

const OWNER: WindowOwner = WindowOwner::SHELL3_SERVICE;
const BACKGROUND: [u8; 4] = [14, 21, 34, 255];
const FOREGROUND: [u8; 4] = [255, 255, 255, 255];

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
    source: FrameHandle,
    frame: FrameHandle,
    session: WindowSessionId,
    window: WindowId,
    width: u32,
    height: u32,
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

    /// Present the Shell3's three text strips in a UI4 window using BCS0.
    /// Non-ASCII symbols currently become `-`; color and formatting metadata
    /// are intentionally ignored at this stage.
    pub(crate) async fn present(
        &mut self,
        lines: [&str; 3],
        columns: usize,
        _rows: usize,
    ) -> Result<(), &'static str> {
        if self.backend != Backend::Copy {
            return Err("shell3-show-backend-not-implemented");
        }
        if self.poisoned {
            return Err("shell3-show-bcs0-allocation-pinned");
        }

        let (width, height) = show_extent(columns)?;
        if self
            .surface
            .as_ref()
            .is_none_or(|surface| surface.width != width || surface.height != height)
        {
            self.release_surface();
            self.surface = Some(create_surface(width, height)?);
        }
        let surface = self.surface.as_ref().ok_or("shell3-show-surface-missing")?;
        let source_lease = acquire_frame_buffer(surface.source)
            .map_err(|_| "shell3-show-source-busy")?;
        let source_view = match writable_rgba_view(source_lease) {
            Ok(view) => view,
            Err(_) => {
                let _ = cancel_frame_buffer(source_lease);
                return Err("shell3-show-source-view");
            }
        };
        if paint_text(source_view, lines).is_err() {
            let _ = cancel_frame_buffer(source_lease);
            return Err("shell3-show-glyph-paint");
        }
        crate::intel::dma_cache_flush_range(source_view.virt, source_view.byte_len);

        let destination_lease = match acquire_frame_buffer(surface.frame) {
            Ok(lease) => lease,
            Err(_) => {
                let _ = cancel_frame_buffer(source_lease);
                return Err("shell3-show-destination-busy");
            }
        };
        let destination_view = match writable_rgba_view(destination_lease) {
            Ok(view) => view,
            Err(_) => {
                let _ = cancel_frame_buffer(source_lease);
                let _ = cancel_frame_buffer(destination_lease);
                return Err("shell3-show-destination-view");
            }
        };
        let source = bcs_surface(source_view);
        let destination = bcs_surface(destination_view);
        let copy = crate::intel::GucBcs0RgbaCopy {
            source,
            source_x: 0,
            source_y: 0,
            destination_x: 0,
            destination_y: 0,
            width,
            height,
        };
        let submission = match crate::intel::queue_guc_bcs0_rgba_copies(destination, &[copy]) {
            Ok(submission) => submission,
            Err(crate::intel::GucBcs0CopySubmitError::SubmitFailed) => {
                self.poisoned = true;
                return Err("shell3-show-bcs0-submit-uncertain");
            }
            Err(_) => {
                let _ = cancel_frame_buffer(source_lease);
                let _ = cancel_frame_buffer(destination_lease);
                return Err("shell3-show-bcs0-unavailable");
            }
        };

        loop {
            match crate::intel::poll_guc_bcs0_rgba_copies(submission) {
                crate::intel::GucBcs0CopyCompletion::Pending => {
                    Timer::after(Duration::from_millis(1)).await;
                }
                crate::intel::GucBcs0CopyCompletion::Complete => break,
                crate::intel::GucBcs0CopyCompletion::Failed
                | crate::intel::GucBcs0CopyCompletion::InvalidSubmission => {
                    self.poisoned = true;
                    return Err("shell3-show-bcs0-retirement-uncertain");
                }
            }
        }

        let _ = cancel_frame_buffer(source_lease);
        crate::intel::dma_cache_flush_range(destination_view.virt, destination_view.byte_len);
        if publish_frame_buffer(destination_lease).is_err() {
            let _ = cancel_frame_buffer(destination_lease);
            return Err("shell3-show-frame-publish");
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
        let _ = finish_window_session_with_request(
            OWNER,
            surface.session,
            WindowSessionCloseRequest::default().animate_and_retire_frames(),
        );
        let _ = destroy_frame(surface.source);
    }
}

impl Drop for Show {
    fn drop(&mut self) {
        self.release_surface();
    }
}

fn show_extent(columns: usize) -> Result<(u32, u32), &'static str> {
    let (screen_width, screen_height) = crate::intel::active_scanout_dimensions()
        .ok_or("shell3-show-scanout-unavailable")?;
    let width = u32::try_from(columns)
        .unwrap_or(u32::MAX)
        .saturating_mul(microfont::FWIDTH as u32)
        .min(screen_width);
    let height = (3 * microfont::FHEIGHT as u32).min(screen_height);
    if width == 0 || height < 3 * microfont::FHEIGHT as u32 {
        return Err("shell3-show-invalid-extent");
    }
    Ok((width, height))
}

fn create_surface(width: u32, height: u32) -> Result<Ui4Surface, &'static str> {
    let output = OutputId::from_slot(0).ok_or("shell3-show-output-unavailable")?;
    let (screen_width, screen_height) = crate::intel::active_scanout_dimensions()
        .ok_or("shell3-show-scanout-unavailable")?;
    let source = create_frame(FrameSpec {
        output,
        content: FrameContent::Image,
        cadence: FrameCadence::Immutable,
        buffering: FrameBuffering::Single,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: None,
    })
    .map_err(|_| "shell3-show-source-frame-create")?;
    let frame = match create_frame(FrameSpec {
        output,
        content: FrameContent::CopyEngine,
        cadence: FrameCadence::Dirty,
        buffering: FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::from_straight_rgba(
            BACKGROUND[0], BACKGROUND[1], BACKGROUND[2], BACKGROUND[3],
        )),
    }) {
        Ok(frame) => frame,
        Err(_) => {
            let _ = destroy_frame(source);
            return Err("shell3-show-frame-create");
        }
    };
    let session = match begin_window_session(OWNER) {
        Ok(session) => session,
        Err(_) => {
            let _ = destroy_frame(frame);
            let _ = destroy_frame(source);
            return Err("shell3-show-session-create");
        }
    };
    let window = match create_window(WindowCreate {
        owner: OWNER,
        session,
        frame,
        output,
        plane: WindowPlane::Universal(super::ALPHA_OVERLAY_PLANE_SLOT as u8),
        placement: WindowPlacement {
            x: screen_width.saturating_sub(width) as i32 / 2,
            y: screen_height.saturating_sub(height) as i32 / 2,
            width,
            height,
            z: 90,
            opacity: u8::MAX,
            visible: true,
        },
        interaction: WindowInteraction::MOVABLE_FRAME,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = finish_window_session_with_request(
                OWNER,
                session,
                WindowSessionCloseRequest::default().animate_and_retire_frames(),
            );
            let _ = destroy_frame(frame);
            let _ = destroy_frame(source);
            return Err("shell3-show-window-create");
        }
    };
    Ok(Ui4Surface {
        source,
        frame,
        session,
        window,
        width,
        height,
    })
}

fn paint_text(view: crate::ui4::FrameRgbaView, lines: [&str; 3]) -> Result<(), ()> {
    let pixels = unsafe { core::slice::from_raw_parts_mut(view.virt as *mut u8, view.byte_len) };
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&BACKGROUND);
    }
    let width = view.width as usize;
    let height = view.height as usize;
    let mut glyphs = vec![0u8; width.checked_mul(height).ok_or(())?];
    for (row, line) in lines.iter().enumerate() {
        let text = ascii_text(line);
        let y = i32::try_from(row.saturating_mul(microfont::FHEIGHT)).map_err(|_| ())?;
        microfont::stamp_text(&mut glyphs, width, height, 0, y, &text, 1u8).map_err(|_| ())?;
    }
    for (index, alpha) in glyphs.iter().copied().enumerate() {
        if alpha == 0 {
            continue;
        }
        let x = index % width;
        let y = index / width;
        let offset = y * view.pitch as usize + x * 4;
        pixels.get_mut(offset..offset + 4).ok_or(())?.copy_from_slice(&FOREGROUND);
    }
    Ok(())
}

fn ascii_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_ascii_graphic() || character == ' ' {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn bcs_surface(view: crate::ui4::FrameRgbaView) -> crate::intel::GucBcs0RgbaSurface {
    crate::intel::GucBcs0RgbaSurface {
        phys: view.phys,
        gpu: view.gpu,
        bytes: view.byte_len,
        width: view.width,
        height: view.height,
        pitch_bytes: view.pitch,
    }
}
