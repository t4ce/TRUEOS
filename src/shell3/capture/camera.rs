use crate::r::keyboard::TrueosKeyboardOutputEvent;
use crate::ui4::{
    self, CursorFrameKey, DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle,
    FrameSpec, OutputId, PremultipliedRgba8, ScanoutFormat, WindowCreate, WindowId,
    WindowInteraction, WindowOwner, WindowPlacement, WindowPlane, WindowSessionId,
};

const OWNER: WindowOwner = WindowOwner::SHELL3_SERVICE;
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

pub(super) struct Preview {
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
}

impl Preview {
    pub(super) fn open() -> Result<Self, &'static str> {
        let output = OutputId::from_slot(0).ok_or("cam: UI4 output unavailable")?;
        let (screen_width, screen_height) =
            ui4::output_dimensions().ok_or("cam: UI4 scanout unavailable")?;
        let frame = ui4::create_frame(FrameSpec {
            output,
            content: FrameContent::Image,
            cadence: FrameCadence::Immutable,
            buffering: FrameBuffering::Single,
            format: ScanoutFormat::Rgba8888Premultiplied,
            width: WIDTH,
            height: HEIGHT,
            base_color: Some(PremultipliedRgba8::from_straight_rgba(38, 54, 74, 255)),
        })
        .map_err(|_| "cam: could not allocate preview frame")?;
        let Ok(session) = ui4::begin_additional_window_session(OWNER) else {
            ui4::retire_frame_when_released(frame);
            return Err("cam: could not create UI4 session");
        };

        let result = (|| {
            let lease =
                ui4::acquire_frame_buffer(frame).map_err(|_| "cam: preview buffer unavailable")?;
            if ui4::mark_frame_buffer_fully_opaque(lease)
                .and_then(|()| ui4::publish_frame_buffer(lease))
                .is_err()
            {
                let _ = ui4::cancel_frame_buffer(lease);
                return Err("cam: could not publish preview buffer");
            }
            let window = ui4::create_window(WindowCreate {
                owner: OWNER,
                session,
                frame,
                output,
                plane: WindowPlane::Primary,
                placement: WindowPlacement {
                    x: (screen_width.saturating_sub(WIDTH) / 2) as i32,
                    y: (screen_height.saturating_sub(HEIGHT) / 2) as i32,
                    width: WIDTH,
                    height: HEIGHT,
                    z: 100,
                    opacity: u8::MAX,
                    visible: true,
                },
                interaction: WindowInteraction::MOVABLE_FRAME,
            })
            .map_err(|_| "cam: could not create UI4 window")?;
            let _ = ui4::set_window_title(OWNER, window, "Camera preview · 1280×720 · Esc closes");
            ui4::publish_window_frame(OWNER, window, DamageRect::FULL)
                .map_err(|_| "cam: could not publish preview window")?;
            Ok(Self {
                session,
                frame,
                window,
            })
        })();
        if result.is_err() {
            let _ = ui4::finish_window_session(OWNER, session);
            ui4::retire_frame_when_released(frame);
        }
        result
    }

    pub(super) fn select_from(&self, shell_window: WindowId, event: &TrueosKeyboardOutputEvent) {
        ui4::reselect_window_for_keyboard(
            event,
            CursorFrameKey::new(OWNER, shell_window),
            CursorFrameKey::new(OWNER, self.window),
        );
    }

    pub(super) fn is_closed(&self) -> bool {
        ui4::window_is_closed(OWNER, self.window)
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        if !self.is_closed() {
            let _ = ui4::close_window(OWNER, self.window);
        }
        let _ = ui4::finish_window_session(OWNER, self.session);
        ui4::retire_frame_when_released(self.frame);
    }
}
