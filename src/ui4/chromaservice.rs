//! On-demand fullscreen display-color editor. The executor profile is selected
//! by spawn_service; closed editors have no frame and sleep after pending LUT
//! commits finish.
//!
//! The grade belongs to the service, not the editor window: Escape retires the
//! UI while leaving the accepted Pipe A look in place, like the color picker.

use alloc::{boxed::Box, vec};
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_sync::signal::Signal;
use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};

use super::{
    DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FrameSpec,
    GlobalKeyboardDisposition, GlobalKeyboardHookId, OutputId, PremultipliedRgba8, ScanoutFormat,
    Ui4CursorSource, Ui4InputEvent, WindowCreate, WindowId, WindowInteraction, WindowOwner,
    WindowPlacement, WindowPlane, WindowSessionCloseRequest, WindowSessionId,
    chroma_color::{self, LUT_ENTRIES, Settings},
    chroma_controls::Controls,
    chroma_render::ChromaPoints,
};

const OWNER: WindowOwner = WindowOwner::CHROMA_SERVICE;
const POLL_MS: u64 = 16;
const PRIMARY: u32 = 1;
static OPEN: Mutex<Option<(Ui4CursorSource, (u32, u32))>> = Mutex::new(None);
static WAKE: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();
static ESCAPE: AtomicBool = AtomicBool::new(false);

pub(super) fn request_open(source: Ui4CursorSource, anchor: (u32, u32)) {
    *OPEN.lock() = Some((source, anchor));
    WAKE.signal(());
}

fn escape(event: &crate::r::keyboard::TrueosKeyboardOutputEvent) -> GlobalKeyboardDisposition {
    use crate::r::keyboard::*;
    if event.kind == KEYBOARD_OUTPUT_KIND_KEY && event.key_code == KEYBOARD_KEY_ESCAPE {
        ESCAPE.store(true, Ordering::Release);
        GlobalKeyboardDisposition::Consume
    } else {
        GlobalKeyboardDisposition::PassThrough
    }
}

#[derive(Clone, Copy)]
enum Drag {
    Cube,
    Slider(usize),
    Curve(usize),
}

struct Editor {
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    source: Ui4CursorSource,
    anchor: (u32, u32),
    focus_pending: bool,
    hook: GlobalKeyboardHookId,
    width: u32,
    height: u32,
    controls: Controls,
    points: ChromaPoints,
    yaw: f32,
    pitch: f32,
    drag: Option<(Ui4CursorSource, Drag)>,
    dirty: bool,
    poisoned: bool,
    close_requested: bool,
}

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn ui4_chroma_service_task() {
    let mut editor: Option<Editor> = None;
    let mut settings = Settings::default();
    // Allocate the LUT on the heap, never a 20-KiB task-stack temporary.
    let mut table: Box<[u32; LUT_ENTRIES]> = vec![0; LUT_ENTRIES]
        .into_boxed_slice()
        .try_into()
        .expect("chroma-lut-length");
    settings.build_table(&mut table);
    let mut grade_dirty = false;
    let mut hardware_dirty = false;
    let mut last_tick = Instant::now();
    let mut hardware_error = None;
    let mut quarantined = false;
    let mut grade_owned = false;
    loop {
        if editor.is_none() && !hardware_dirty && !grade_dirty {
            WAKE.wait().await;
        }
        let request = OPEN.lock().take();
        if let Some((source, anchor)) = request {
            if editor.is_none() && !quarantined {
                // Reopening after a pipe reset restores only a grade already
                // accepted by this service. First open preserves existing LUTs.
                if grade_owned {
                    match chroma_color::rearm_pipe_a_if_lost(&table) {
                        Ok(_) | Err("chroma-3dlut-busy") => {}
                        Err(reason) => crate::log_warn!(target: "ui4/chroma";
                            "chroma: LUT recovery deferred reason={}\n", reason),
                    }
                }
                match open(source, anchor, &table).await {
                    Ok(value) => {
                        editor = Some(value);
                        last_tick = Instant::now();
                    }
                    Err(reason) => crate::log_warn!(target: "ui4/chroma";
                        "chroma: open rejected reason={}\n", reason),
                }
            }
        }
        if let Some(active) = editor.as_mut() {
            active.close_requested |= ESCAPE.swap(false, Ordering::AcqRel)
                || super::window_is_closed(OWNER, active.window);
            // Drain the final drag before handling Escape. Even if the LUT
            // port is busy for this vblank, its last edit must not be lost.
            for event in super::take_owner_input_events(OWNER) {
                match event {
                    Ui4InputEvent::Pointer(event) if event.window == active.window => {
                        grade_dirty |= active.pointer(event, &mut settings);
                    }
                    Ui4InputEvent::Keyboard(event) if event.window == active.window => {
                        active.keyboard(event.event);
                    }
                    _ => {}
                }
            }
        }
        if grade_dirty {
            settings.build_table(&mut table);
            hardware_dirty = true;
            grade_dirty = false;
        }
        if hardware_dirty {
            match chroma_color::program_pipe_a(&table) {
                Ok(()) => {
                    hardware_dirty = false;
                    hardware_error = None;
                    grade_owned = true;
                    if let Some(active) = editor.as_mut() {
                        active.points.set_table(&table);
                        active.dirty = true;
                    }
                }
                Err("chroma-3dlut-busy" | "chroma-pipe-a-unavailable") => {}
                Err(reason) => {
                    if hardware_error != Some(reason) {
                        crate::log_warn!(target: "ui4/chroma";
                            "chroma: LUT commit failed reason={}\n", reason);
                        hardware_error = Some(reason);
                    }
                    // Retry on the next open/edit rather than spinning a
                    // background service forever on an unsupported device.
                    hardware_dirty = false;
                }
            }
        }
        if let Some(active) = editor.as_mut() {
            if active.close_requested {
                if close(active) {
                    editor = None;
                }
            } else if super::output_dimensions()
                .is_some_and(|size| size != (active.width, active.height))
            {
                let source = active.source;
                let anchor = active.anchor;
                if close(active) {
                    editor = None;
                    *OPEN.lock() = Some((source, anchor));
                    WAKE.signal(());
                }
            } else {
                let elapsed = last_tick.elapsed().as_micros() as f32 / 1_000_000.0;
                last_tick = Instant::now();
                active.dirty |= active.points.animate(elapsed.min(0.1));
                if active.dirty {
                    match active.paint(&settings).await {
                        Ok(()) => {
                            active.dirty = false;
                            if active.focus_pending {
                                active.focus_pending =
                                    super::input_broker::select_window_for_cursor_at(
                                        active.source,
                                        OWNER,
                                        active.window,
                                        active.anchor.0,
                                        active.anchor.1,
                                    )
                                    .is_err();
                                active.dirty |= active.focus_pending;
                            }
                        }
                        Err(reason) if !active.poisoned => {
                            crate::log_trace!(target: "ui4/chroma";
                                "chroma: render deferred reason={}\n", reason);
                        }
                        Err(reason) => {
                            crate::log_warn!(target: "ui4/chroma";
                                "chroma: GPU retirement uncertain reason={} action=quarantine\n", reason);
                            // Close without retiring the frame, and retain all
                            // sources while GPU completion remains unproven.
                            let _ = super::unregister_global_keyboard_hook(active.hook);
                            let _ = super::finish_window_session(OWNER, active.session);
                            core::mem::forget(editor.take().expect("active-chroma"));
                            quarantined = true;
                        }
                    }
                }
            }
        }
        if editor.is_some() || hardware_dirty {
            Timer::after(Duration::from_millis(POLL_MS)).await;
        }
    }
}

async fn open(
    source: Ui4CursorSource,
    anchor: (u32, u32),
    table: &[u32; LUT_ENTRIES],
) -> Result<Editor, &'static str> {
    chroma_color::pipe_a_status()?;
    let (width, height) = super::output_dimensions().ok_or("chroma-output-unavailable")?;
    if !crate::intel::render::active_output_color_extent_supported(width, height) {
        return Err("chroma-resolution-exceeds-native-render-target");
    }
    let controls = Controls::new(width, height).await?;
    let mut points = ChromaPoints::new()?;
    points.set_table(table);
    let output = OutputId::from_slot(0).ok_or("chroma-output-unavailable")?;
    let session = super::begin_window_session(OWNER).map_err(|_| "chroma-session-create")?;
    let frame = match super::create_frame(FrameSpec {
        output,
        content: FrameContent::BlueprintScene,
        cadence: FrameCadence::Dirty,
        buffering: FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::TRANSPARENT),
    }) {
        Ok(frame) => frame,
        Err(_) => {
            let _ = super::finish_window_session(OWNER, session);
            return Err("chroma-frame-create");
        }
    };
    let window = match super::create_window(WindowCreate {
        owner: OWNER,
        session,
        frame,
        output,
        plane: WindowPlane::Primary,
        placement: WindowPlacement {
            x: 0,
            y: 0,
            width,
            height,
            z: 100,
            opacity: u8::MAX,
            visible: true,
        },
        interaction: WindowInteraction {
            movable: false,
            maximizable: false,
            receives_input: true,
            primary_activation: true,
            hit_testable: true,
            resize_on_maximize: false,
        },
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = super::finish_window_session(OWNER, session);
            let _ = super::destroy_frame(frame);
            return Err("chroma-window-create");
        }
    };
    let hook = match super::register_global_keyboard_hook(u8::MAX, escape) {
        Ok(hook) => hook,
        Err(_) => {
            let _ = super::finish_window_session_with_request(
                OWNER,
                session,
                WindowSessionCloseRequest::default().animate_and_retire_frames(),
            );
            return Err("chroma-escape-hook");
        }
    };
    ESCAPE.store(false, Ordering::Release);
    let _ = super::set_window_title(OWNER, window, "CHROMATIC · drag / WASD rotate · Esc closes");
    crate::log_info!(target: "ui4/chroma";
        "chroma: opened fullscreen={}x{} background=transparent dots=4913 primitive=point-list color=pipe-a-3dlut controls=bcs0-cache+microfont grade-persists-on-escape=1\n",
        width, height);
    Ok(Editor {
        session,
        frame,
        window,
        source,
        anchor,
        focus_pending: true,
        hook,
        width,
        height,
        controls,
        points,
        yaw: -0.63,
        pitch: 0.40,
        drag: None,
        dirty: true,
        poisoned: false,
        close_requested: false,
    })
}

fn close(editor: &Editor) -> bool {
    if super::finish_window_session_with_request(
        OWNER,
        editor.session,
        WindowSessionCloseRequest::default().animate_and_retire_frames(),
    )
    .is_err()
    {
        return false;
    }
    let _ = super::unregister_global_keyboard_hook(editor.hook);
    let _ = super::take_owner_input_events(OWNER);
    true
}

impl Editor {
    fn pointer(
        &mut self,
        event: super::input_broker::Ui4PointerEvent,
        settings: &mut Settings,
    ) -> bool {
        let (x, y) = (event.local_x, event.local_y);
        if event.buttons_pressed & PRIMARY != 0 && self.drag.is_none() {
            let layout = &self.controls.layout;
            let drag = if let Some(index) = layout.slider_at(x, y) {
                Some(Drag::Slider(index))
            } else if let Some(index) = layout.curve_point_at(x, y, settings) {
                Some(Drag::Curve(index))
            } else if x >= 0 && (x as u32) < layout.cube_width {
                Some(Drag::Cube)
            } else {
                None
            };
            self.drag = drag.map(|drag| (event.source, drag));
        }
        let mut changed = false;
        if let Some((source, drag)) = self.drag {
            if source == event.source {
                match drag {
                    Drag::Cube => {
                        if event.dx != 0 || event.dy != 0 {
                            self.yaw += event.dx as f32 * 0.007;
                            self.pitch = (self.pitch + event.dy as f32 * 0.007).clamp(-1.4, 1.4);
                            self.dirty = true;
                        }
                    }
                    Drag::Slider(index) => {
                        let value = self.controls.layout.slider_value(index, x);
                        let slot = match index {
                            0 => &mut settings.yellow,
                            1 => &mut settings.shadows,
                            2 => &mut settings.highlights,
                            3 => &mut settings.saturation,
                            _ => &mut settings.strength,
                        };
                        changed = *slot != value;
                        *slot = value;
                    }
                    Drag::Curve(index) => {
                        let value = self.controls.layout.curve_value(y);
                        changed = settings.sat_curve[index] != value;
                        settings.sat_curve[index] = value;
                        if index == 0 || index == 6 {
                            settings.sat_curve[0] = value;
                            settings.sat_curve[6] = value;
                        }
                    }
                }
                if event.buttons_down & PRIMARY == 0 {
                    self.drag = None;
                }
            }
        }
        self.dirty |= changed;
        changed
    }

    fn keyboard(&mut self, event: crate::r::keyboard::TrueosKeyboardOutputEvent) {
        use crate::r::keyboard::*;
        if event.kind != KEYBOARD_OUTPUT_KIND_TEXT {
            return;
        }
        let Some(character) = char::from_u32(event.codepoint) else {
            return;
        };
        match character.to_ascii_lowercase() {
            'a' => self.yaw -= 0.08,
            'd' => self.yaw += 0.08,
            'w' => self.pitch = (self.pitch - 0.08).clamp(-1.4, 1.4),
            's' => self.pitch = (self.pitch + 0.08).clamp(-1.4, 1.4),
            _ => return,
        }
        self.dirty = true;
    }

    async fn paint(&mut self, settings: &Settings) -> Result<(), &'static str> {
        let lease = super::acquire_frame_buffer(self.frame).map_err(|_| "chroma-frame-busy")?;
        let destination = match super::gpgpu_rgba_surface(lease) {
            Ok(surface) => surface,
            Err(_) => {
                let _ = super::cancel_frame_buffer(lease);
                return Err("chroma-frame-surface");
            }
        };
        let bcs = crate::intel::GucBcs0RgbaSurface {
            phys: destination.phys,
            gpu: destination.gpu,
            bytes: destination.bytes,
            width: destination.width,
            height: destination.height,
            pitch_bytes: destination.pitch_bytes,
        };
        if let Err(reason) = self.controls.paint(bcs, settings, &mut self.poisoned).await {
            if !self.poisoned {
                let _ = super::cancel_frame_buffer(lease);
            }
            return Err(reason);
        }
        // RCS is the final writer, after all BCS0 clear/copy/handle work has
        // retired. Its exact-allocation release is UI4's publication proof.
        let result = match self.points.render(
            destination,
            self.controls.layout.cube_width,
            self.yaw,
            self.pitch,
            None,
        ) {
            Ok(result) => result,
            Err(reason) => {
                self.poisoned = self.points.is_poisoned();
                if !self.poisoned {
                    let _ = super::cancel_frame_buffer(lease);
                }
                return Err(reason);
            }
        };
        if let Some(reason) = result.completion_error() {
            self.poisoned = true;
            return Err(reason);
        }
        let Some(release) = result.release_fence else {
            self.poisoned = true;
            return Err("chroma-render-release-missing");
        };
        if super::publish_resident_scene_frame_buffer(lease, release).is_err() {
            let _ = super::cancel_frame_buffer(lease);
            return Err("chroma-frame-publish");
        }
        super::publish_window_frame(OWNER, self.window, DamageRect::FULL)
            .map_err(|_| "chroma-window-publish")?;
        Ok(())
    }
}
