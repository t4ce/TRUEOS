//! On-demand fullscreen display-color editor. The executor profile is selected
//! by spawn_service; closed editors have no frame and sleep after pending LUT
//! commits finish.
//!
//! The grade belongs to the service, not the editor window: Escape retires the
//! UI while leaving each accepted pipe look in place, like the color picker.

use core::sync::atomic::{AtomicBool, Ordering};
use embassy_sync::signal::Signal;
use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};

use super::{
    DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FrameSpec,
    GlobalKeyboardDisposition, GlobalKeyboardHookId, OutputId, PremultipliedRgba8, ScanoutFormat,
    Ui4CursorSource, Ui4InputEvent, WindowCreate, WindowId, WindowInteraction, WindowOwner,
    WindowPlacement, WindowPlane, WindowSessionCloseRequest, WindowSessionId,
    chroma_animation::Animations,
    chroma_color::{self, LUT_ENTRIES, Settings},
    chroma_controls::Controls,
    chroma_motion::MotionMode,
    chroma_pipes::{PIPE_COUNT, PipeBuckets},
    chroma_render::ChromaPoints,
};

const OWNER: WindowOwner = WindowOwner::CHROMA_SERVICE;
const POLL_MS: u64 = 16;
const INACTIVE_POLL_MS: u64 = 250;
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
    Slider { index: usize, pipe: usize },
    Curve { index: usize, pipe: usize },
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
    preview_dirty: bool,
    poisoned: bool,
    close_requested: bool,
}

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn ui4_chroma_service_task() {
    let mut editor: Option<Editor> = None;
    let mut buckets = PipeBuckets::default();
    let mut animations = Animations::new(core::array::from_fn(|pipe| *buckets.settings(pipe)));
    let mut grade_dirty = 0u8;
    let mut hardware_dirty = 0u8;
    let mut inactive = 0u8;
    let mut last_tick = Instant::now();
    let mut hardware_errors = [None; PIPE_COUNT];
    let mut quarantined = false;
    let mut grade_owned = 0u8;
    loop {
        if editor.is_none() && hardware_dirty == 0 && grade_dirty == 0 && !animations.any_active() {
            WAKE.wait().await;
        }
        let request = OPEN.lock().take();
        if let Some((source, anchor)) = request {
            if editor.is_none() && !quarantined {
                // Restore only service-owned looks. Opening the editor or
                // changing its selection never overwrites another pipe's LUT.
                for pipe in 0..PIPE_COUNT {
                    if grade_owned & (1 << pipe) != 0 {
                        match chroma_color::rearm_pipe_if_lost(pipe, animations.table(pipe)) {
                            Ok(_) | Err("chroma-3dlut-busy" | "chroma-pipe-unavailable") => {}
                            Err(reason) => crate::log_warn!(target: "ui4/chroma";
                                "chroma: pipe={} LUT recovery deferred reason={}\n",
                                (b'A' + pipe as u8) as char, reason),
                        }
                    }
                }
                match open(
                    source,
                    anchor,
                    animations.table(buckets.selected()),
                    buckets.selected(),
                )
                .await
                {
                    Ok(value) => {
                        editor = Some(value);
                        last_tick = Instant::now();
                    }
                    Err(reason) => crate::log_warn!(target: "ui4/chroma";
                        "chroma: open rejected reason={}\n", reason),
                }
            }
        }
        let elapsed = (last_tick.elapsed().as_micros() as f32 / 1_000_000.0).min(0.1);
        last_tick = Instant::now();
        if let Some(active) = editor.as_mut() {
            active.close_requested |= ESCAPE.swap(false, Ordering::AcqRel)
                || super::window_is_closed(OWNER, active.window);
            // Drain the final drag before handling Escape. Even if the LUT
            // port is busy for this vblank, its last edit must not be lost.
            for event in super::take_owner_input_events(OWNER) {
                match event {
                    Ui4InputEvent::Pointer(event) if event.window == active.window => {
                        grade_dirty |= active.pointer(event, &mut buckets, &mut animations);
                    }
                    _ => {}
                }
            }
        }
        if let Some(active) = editor.as_ref() {
            animations.set_gravity(
                buckets.selected(),
                super::chroma_render::view_gravity([
                    libm::sinf(active.yaw),
                    libm::cosf(active.yaw),
                    libm::sinf(active.pitch),
                    libm::cosf(active.pitch),
                ]),
            );
        }
        for pipe in 0..PIPE_COUNT {
            let bit = 1 << pipe;
            if grade_dirty & bit != 0 {
                animations.set_base(pipe, buckets.settings(pipe));
            }
            // An unseen inactive simulation sleeps at the slow retry cadence.
            // Probe it even if its last generated table happened to equal base.
            if inactive & bit != 0
                && animations.mode(pipe).is_some()
                && hardware_dirty & bit == 0
                && chroma_color::pipe_status(pipe).is_ok()
            {
                inactive &= !bit;
            }
        }
        grade_dirty = 0;
        let preview_pipe = editor.as_ref().map(|_| buckets.selected());
        let generated = animations.advance(elapsed, preview_pipe, inactive);
        hardware_dirty |= generated;
        // A pending/disabled B never prevents A's commit, or vice versa.
        for pipe in 0..PIPE_COUNT {
            let bit = 1 << pipe;
            if hardware_dirty & bit == 0 {
                continue;
            }
            match chroma_color::program_pipe(pipe, animations.table(pipe)) {
                Ok(()) => {
                    hardware_dirty &= !bit;
                    inactive &= !bit;
                    hardware_errors[pipe] = None;
                    grade_owned |= bit;
                    if pipe == buckets.selected() {
                        if let Some(active) = editor.as_mut() {
                            active.preview_dirty = true;
                        }
                    }
                }
                Err("chroma-3dlut-busy") => {
                    inactive &= !bit;
                }
                Err("chroma-pipe-unavailable") => {
                    inactive |= bit;
                    if pipe == buckets.selected() && generated & bit != 0 {
                        if let Some(active) = editor.as_mut() {
                            active.preview_dirty = true;
                        }
                    }
                }
                Err(reason) => {
                    if hardware_errors[pipe] != Some(reason) {
                        crate::log_warn!(target: "ui4/chroma";
                            "chroma: pipe={} LUT commit failed reason={}\n",
                            (b'A' + pipe as u8) as char, reason);
                        hardware_errors[pipe] = Some(reason);
                    }
                    hardware_dirty &= !bit;
                    inactive &= !bit;
                }
            }
        }
        if let Some(active) = editor.as_mut() {
            if active.preview_dirty {
                match active
                    .points
                    .set_table(animations.table(buckets.selected()), buckets.selected())
                {
                    Ok(()) => {
                        if animations.mode(buckets.selected()).is_some() {
                            let _ = active.points.snap_to_targets();
                        }
                        active.preview_dirty = false;
                        active.dirty = true;
                    }
                    Err(reason) => crate::log_trace!(target: "ui4/chroma";
                        "chroma: preview deferred reason={}\n", reason),
                }
            }
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
                active.dirty |= active.points.animate(elapsed);
                if active.dirty {
                    match active
                        .paint(
                            buckets.displayed_settings(),
                            buckets.selected(),
                            animations
                                .mode(buckets.selected())
                                .map(|mode| mode as usize),
                        )
                        .await
                    {
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
        if editor.is_some() || hardware_dirty != 0 || animations.any_active() {
            let poll_ms = if editor.is_none()
                && hardware_dirty & !inactive == 0
                && !animations.runnable(None, inactive)
            {
                INACTIVE_POLL_MS
            } else {
                POLL_MS
            };
            Timer::after(Duration::from_millis(poll_ms)).await;
        }
    }
}

async fn open(
    source: Ui4CursorSource,
    anchor: (u32, u32),
    table: &[u32; LUT_ENTRIES],
    selected_pipe: usize,
) -> Result<Editor, &'static str> {
    chroma_color::pipe_status(0)?;
    let (width, height) = super::output_dimensions().ok_or("chroma-output-unavailable")?;
    if !crate::intel::render::active_output_color_extent_supported(width, height) {
        return Err("chroma-resolution-exceeds-native-render-target");
    }
    let controls = Controls::new(width, height).await?;
    let mut points = ChromaPoints::new()?;
    points.set_table(table, selected_pipe)?;
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
    let _ = super::set_window_title(OWNER, window, "CHROMATIC · drag to rotate · Esc closes");
    crate::log_info!(target: "ui4/chroma";
        "chroma: opened fullscreen={}x{} background=transparent dots=4913 primitive=point-list color=per-pipe-3dlut controls=bcs0-cache+microfont grade-persists-on-escape=1\n",
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
        preview_dirty: false,
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
        buckets: &mut PipeBuckets,
        animations: &mut Animations,
    ) -> u8 {
        let (x, y) = (event.local_x, event.local_y);
        if event.buttons_pressed & PRIMARY != 0 && self.drag.is_none() {
            let layout = &self.controls.layout;
            if let Some(pipe) = layout.pipe_at(x, y) {
                if buckets.select(pipe) {
                    self.preview_dirty = true;
                    self.dirty = true;
                }
                return 0;
            }
            let pipe = buckets.selected();
            if let Some(index) = layout.motion_at(x, y) {
                if let Some(mode) = MotionMode::from_index(index) {
                    let next = if animations.mode(pipe) == Some(mode) {
                        None
                    } else {
                        Some(mode)
                    };
                    self.dirty |= animations.select_mode(pipe, next);
                }
                return 0;
            }
            let drag = if let Some(index) = layout.slider_at(x, y) {
                Some(Drag::Slider { index, pipe })
            } else if let Some(index) = layout.curve_point_at(x, y, buckets.displayed_settings()) {
                Some(Drag::Curve { index, pipe })
            } else if x >= 0 && (x as u32) < layout.cube_width {
                Some(Drag::Cube)
            } else {
                None
            };
            self.drag = drag.map(|drag| (event.source, drag));
        }
        let mut changed = 0;
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
                    Drag::Slider { index, pipe } => {
                        let value = self.controls.layout.slider_value(index, x);
                        changed = buckets.edit_slider_for(pipe, index, value);
                    }
                    Drag::Curve { index, pipe } => {
                        let value = self.controls.layout.curve_value(y);
                        changed = buckets.edit_curve_for(pipe, index, value);
                    }
                }
                if event.buttons_down & PRIMARY == 0 {
                    self.drag = None;
                }
            }
        }
        self.dirty |= changed != 0;
        changed
    }

    async fn paint(
        &mut self,
        settings: &Settings,
        selected_pipe: usize,
        active_mode: Option<usize>,
    ) -> Result<(), &'static str> {
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
        if let Err(reason) = self
            .controls
            .paint(
                bcs,
                settings,
                selected_pipe,
                active_mode,
                &mut self.poisoned,
            )
            .await
        {
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
