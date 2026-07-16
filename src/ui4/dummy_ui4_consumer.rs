//! Temporary UI4 consumers used to exercise the kernel frame/window contract.
//!
//! Two trusted app identities deliberately mimic two future Blueprint
//! consumers without defining any Blueprint transport or ABI:
//! - one app owns three Mandelbrot windows with immutable/dirty/streaming
//!   cadence;
//! - one app owns a separate immutable white CPU-authored window.

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::r::mouse_motion_service::{
    MOUSE_CONTROL_EASING_FAST_LINEAR, MOUSE_CONTROL_EASING_NATURAL, MOUSE_CONTROL_FLAG_CLEAR_QUEUE,
    MOUSE_CONTROL_OPCODE_STROKE, MOUSE_CONTROL_OPCODE_TELEPORT, MOUSE_CONTROL_PATH_CUBIC,
    MOUSE_CONTROL_PATH_LINE, MouseControlCommand, MouseControlCursor, MouseControlError,
    MouseControlPrincipal, cursor_is_idle, release_cursor, request_cursor, submit_program,
};

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FramePoolError, FrameReadLease,
    FrameRgbaView, FrameSpec, OutputId, PublishedFrame, ScanoutFormat, Ui4InputEvent,
    WindowBrokerError, WindowCreate, WindowId, WindowOwner, WindowPlacement, WindowSessionId,
    acquire_frame_buffer, acquire_published_frame, begin_window_session, cancel_frame_buffer,
    create_frame, create_window, destroy_frame, finish_window_session, gpgpu_rgba_surface,
    publish_frame_buffer, publish_window_frame, published_rgba_view, release_published_frame,
    set_window_placement, software_cursor_visuals, take_owner_input_events, writable_rgba_view,
};

const MANDEL_APP_OWNER: WindowOwner = WindowOwner::KernelApp(1);
const WHITE_APP_OWNER: WindowOwner = WindowOwner::KernelApp(2);
const MANDEL_WIDTH: u32 = 768;
const MANDEL_HEIGHT: u32 = 512;
const WHITE_WIDTH: u32 = 512;
const WHITE_HEIGHT: u32 = 512;
const STATIC_PARAMETER: u32 = 128;
const STREAM_PARAMETER_MAX: u32 = 256;
const STREAM_PERIOD_MS: u64 = 33;
const HEARTBEAT_REST_FRAMES: u32 = 50;
const PRIMARY_BUTTON_MASK: u32 = 1 << 0;
const MIDDLE_BUTTON_MASK: u32 = 1 << 2;

const STATIC_PLACEMENT: WindowPlacement = WindowPlacement {
    x: 64,
    y: 64,
    width: MANDEL_WIDTH,
    height: MANDEL_HEIGHT,
    z: 10,
    opacity: u8::MAX,
    visible: true,
};
const DIRTY_PLACEMENT: WindowPlacement = WindowPlacement {
    x: 896,
    y: 64,
    width: MANDEL_WIDTH,
    height: MANDEL_HEIGHT,
    z: 11,
    opacity: u8::MAX,
    visible: true,
};
const STREAM_PLACEMENT: WindowPlacement = WindowPlacement {
    x: 480,
    y: 640,
    width: MANDEL_WIDTH,
    height: MANDEL_HEIGHT,
    z: 12,
    opacity: u8::MAX,
    visible: true,
};
const WHITE_PLACEMENT: WindowPlacement = WindowPlacement {
    x: 1856,
    y: 64,
    width: WHITE_WIDTH,
    height: WHITE_HEIGHT,
    z: 13,
    opacity: u8::MAX,
    visible: true,
};
const MANDEL_PLACEMENTS: [WindowPlacement; 3] =
    [STATIC_PLACEMENT, DIRTY_PLACEMENT, STREAM_PLACEMENT];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum DummyUi4ConsumerError {
    Frame(FramePoolError),
    Window(WindowBrokerError),
    RenderFailed,
    PresentFailed,
    MouseMotion(MouseControlError),
}

impl From<FramePoolError> for DummyUi4ConsumerError {
    fn from(error: FramePoolError) -> Self {
        Self::Frame(error)
    }
}

impl From<WindowBrokerError> for DummyUi4ConsumerError {
    fn from(error: WindowBrokerError) -> Self {
        Self::Window(error)
    }
}

impl From<MouseControlError> for DummyUi4ConsumerError {
    fn from(error: MouseControlError) -> Self {
        Self::MouseMotion(error)
    }
}

struct MandelPlaceholderApp {
    owner: WindowOwner,
    session: WindowSessionId,
    frames: [FrameHandle; 3],
    windows: [WindowId; 3],
    placements: [WindowPlacement; 3],
    dirty_parameter: u32,
    stream_parameter: u32,
}

struct WhitePlaceholderApp {
    owner: WindowOwner,
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    placement: WindowPlacement,
}

struct Runtime {
    mandel: MandelPlaceholderApp,
    white: WhitePlaceholderApp,
    heartbeat_cursor: MouseControlCursor,
    heartbeat_rest_frames: u32,
}

impl Runtime {
    fn composition_frames(&self) -> [FrameHandle; 4] {
        [
            self.mandel.frames[0],
            self.mandel.frames[1],
            self.mandel.frames[2],
            self.white.frame,
        ]
    }

    fn composition_placements(&self) -> [WindowPlacement; 4] {
        [
            self.mandel.placements[0],
            self.mandel.placements[1],
            self.mandel.placements[2],
            self.white.placement,
        ]
    }
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn dummy_ui4_consumer_service_task(worker_slot: u32) {
    crate::log_info!(
        target: "ui4";
        "ui4 dummy-consumer carrier online placement=worker-ap2+ assigned_slot={} current_slot={}\n",
        worker_slot,
        crate::percpu::current_slot()
    );
    crate::intel::wait_hw_logo_sequence_done().await;
    let mut runtime = match initialize() {
        Ok(runtime) => runtime,
        Err(error) => {
            crate::log_error!(
                target: "ui4";
                "ui4 dummy-consumer init failed error={:?}\n",
                error
            );
            return;
        }
    };

    crate::log_info!(
        target: "ui4";
        "ui4 dummy-consumer live apps=2 windows=4 mandel_extent={}x{} mandel_buffers=1/2/3 static={} dirty={} stream={}..={} white_extent={}x{} white_buffers=1 cadence_ms={} plane=primary-compositor input=ui4-owner-queues callbacks=focus,left-click,middle-pan,keyboard heartbeat_vcursor_slot={}\n",
        MANDEL_WIDTH,
        MANDEL_HEIGHT,
        STATIC_PARAMETER,
        runtime.mandel.dirty_parameter,
        runtime.mandel.stream_parameter,
        STREAM_PARAMETER_MAX,
        WHITE_WIDTH,
        WHITE_HEIGHT,
        STREAM_PERIOD_MS,
        runtime.heartbeat_cursor.slot_id
    );

    loop {
        let (dirty_clicks, last_dirty_click) = match dispatch_ui4_callbacks(&mut runtime) {
            Ok(result) => result,
            Err(error) => {
                fail_and_cleanup(runtime, error);
                return;
            }
        };
        if dirty_clicks != 0 {
            runtime.mandel.dirty_parameter =
                runtime.mandel.dirty_parameter.saturating_add(dirty_clicks);
            match render_and_publish_mandel(&runtime.mandel, 1, runtime.mandel.dirty_parameter) {
                Ok(_) => {}
                Err(error) => {
                    fail_and_cleanup(runtime, error);
                    return;
                }
            }
            let (x, y) = last_dirty_click.unwrap_or((0, 0));
            crate::log_info!(
                target: "ui4";
                "ui4 dummy-consumer mandel-dirty-click x={} y={} clicks={} parameter={}\n",
                x,
                y,
                dirty_clicks,
                runtime.mandel.dirty_parameter
            );
        }
        if runtime.heartbeat_rest_frames != 0 {
            runtime.heartbeat_rest_frames -= 1;
        } else {
            let idle = match cursor_is_idle(
                MouseControlPrincipal::KernelApp(1),
                runtime.heartbeat_cursor.handle,
            ) {
                Ok(idle) => idle,
                Err(error) => {
                    fail_and_cleanup(runtime, error.into());
                    return;
                }
            };
            if idle {
                if let Err(error) = submit_heartbeat_program(runtime.heartbeat_cursor) {
                    fail_and_cleanup(runtime, error);
                    return;
                }
                runtime.heartbeat_rest_frames = HEARTBEAT_REST_FRAMES;
            }
        }

        runtime.mandel.stream_parameter = if runtime.mandel.stream_parameter == STREAM_PARAMETER_MAX
        {
            0
        } else {
            runtime.mandel.stream_parameter + 1
        };
        match render_and_publish_mandel(&runtime.mandel, 2, runtime.mandel.stream_parameter) {
            Ok(_) => {}
            Err(error) => {
                fail_and_cleanup(runtime, error);
                return;
            }
        }

        if let Err(error) =
            present_composition(runtime.composition_frames(), runtime.composition_placements())
        {
            fail_and_cleanup(runtime, error);
            return;
        }

        Timer::after(EmbassyDuration::from_millis(STREAM_PERIOD_MS)).await;
    }
}

fn initialize() -> Result<Runtime, DummyUi4ConsumerError> {
    let output = OutputId::from_slot(0).expect("UI4 D01 must exist");
    let mandel = initialize_mandel_app(output)?;
    let white = match initialize_white_app(output) {
        Ok(white) => white,
        Err(error) => {
            cleanup_mandel_app(mandel);
            return Err(error);
        }
    };
    let heartbeat_cursor =
        match request_cursor(MouseControlPrincipal::KernelApp(1), "ui4-heartbeat") {
            Ok(cursor) => cursor,
            Err(error) => {
                cleanup_mandel_app(mandel);
                cleanup_white_app(white);
                return Err(error.into());
            }
        };
    let runtime = Runtime {
        mandel,
        white,
        heartbeat_cursor,
        heartbeat_rest_frames: 0,
    };
    if let Err(error) =
        present_composition(runtime.composition_frames(), runtime.composition_placements())
    {
        cleanup_runtime(runtime);
        return Err(error);
    }

    Ok(runtime)
}

fn submit_heartbeat_program(cursor: MouseControlCursor) -> Result<(), DummyUi4ConsumerError> {
    let teleport = MouseControlCommand {
        opcode: MOUSE_CONTROL_OPCODE_TELEPORT,
        flags: MOUSE_CONTROL_FLAG_CLEAR_QUEUE,
        x: 1380,
        y: 1220,
        ..MouseControlCommand::default()
    };
    let line = |x, y, duration_ms| MouseControlCommand {
        opcode: MOUSE_CONTROL_OPCODE_STROKE,
        path: MOUSE_CONTROL_PATH_LINE,
        easing: MOUSE_CONTROL_EASING_FAST_LINEAR,
        duration_ms,
        x,
        y,
        ..MouseControlCommand::default()
    };
    let accent = |x, y, c1x, c1y, c2x, c2y, duration_ms| MouseControlCommand {
        opcode: MOUSE_CONTROL_OPCODE_STROKE,
        path: MOUSE_CONTROL_PATH_CUBIC,
        easing: MOUSE_CONTROL_EASING_NATURAL,
        duration_ms,
        x,
        y,
        control1_x: c1x,
        control1_y: c1y,
        control2_x: c2x,
        control2_y: c2y,
        ..MouseControlCommand::default()
    };
    let program = [
        teleport,
        line(1430, 1220, 120),
        accent(1470, 1192, 1440, 1220, 1456, 1192, 100),
        accent(1500, 1268, 1480, 1192, 1490, 1268, 110),
        accent(1535, 1130, 1510, 1268, 1520, 1130, 125),
        accent(1570, 1220, 1545, 1130, 1555, 1220, 120),
        line(1640, 1220, 150),
    ];
    submit_program(MouseControlPrincipal::KernelApp(1), cursor.handle, &program)?;
    Ok(())
}

fn initialize_mandel_app(output: OutputId) -> Result<MandelPlaceholderApp, DummyUi4ConsumerError> {
    let mut frames = [None; 3];
    for (slot, cadence) in [
        FrameCadence::Immutable,
        FrameCadence::Dirty,
        FrameCadence::Streaming,
    ]
    .into_iter()
    .enumerate()
    {
        match create_frame(FrameSpec {
            output,
            content: FrameContent::Image,
            cadence,
            format: ScanoutFormat::Rgba8888Premultiplied,
            width: MANDEL_WIDTH,
            height: MANDEL_HEIGHT,
        }) {
            Ok(frame) => frames[slot] = Some(frame),
            Err(error) => {
                destroy_optional_frames(frames);
                return Err(error.into());
            }
        }
    }
    let frames = [frames[0].unwrap(), frames[1].unwrap(), frames[2].unwrap()];
    let initial_parameters = [STATIC_PARAMETER, 0, 0];
    for slot in 0..frames.len() {
        match render_mandel_frame(frames[slot], initial_parameters[slot]) {
            Ok(_) => {}
            Err(error) => {
                destroy_frames(frames);
                return Err(error);
            }
        }
    }

    let session = match begin_window_session(MANDEL_APP_OWNER) {
        Ok(session) => session,
        Err(error) => {
            destroy_frames(frames);
            return Err(error.into());
        }
    };
    let mut windows = [None; 3];
    for slot in 0..frames.len() {
        let result = create_window(WindowCreate {
            owner: MANDEL_APP_OWNER,
            session,
            frame: frames[slot],
            output,
            placement: MANDEL_PLACEMENTS[slot],
        })
        .and_then(|window| {
            publish_window_frame(MANDEL_APP_OWNER, window, DamageRect::FULL)?;
            Ok(window)
        });
        match result {
            Ok(window) => windows[slot] = Some(window),
            Err(error) => {
                let _ = finish_window_session(MANDEL_APP_OWNER, session);
                destroy_frames(frames);
                return Err(error.into());
            }
        }
    }

    Ok(MandelPlaceholderApp {
        owner: MANDEL_APP_OWNER,
        session,
        frames,
        windows: [
            windows[0].unwrap(),
            windows[1].unwrap(),
            windows[2].unwrap(),
        ],
        placements: MANDEL_PLACEMENTS,
        dirty_parameter: 0,
        stream_parameter: 0,
    })
}

fn initialize_white_app(output: OutputId) -> Result<WhitePlaceholderApp, DummyUi4ConsumerError> {
    let frame = create_frame(FrameSpec {
        output,
        content: FrameContent::CpuBlit,
        cadence: FrameCadence::Immutable,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width: WHITE_WIDTH,
        height: WHITE_HEIGHT,
    })?;
    match render_white_frame(frame) {
        Ok(_) => {}
        Err(error) => {
            let _ = destroy_frame(frame);
            return Err(error);
        }
    };
    let session = match begin_window_session(WHITE_APP_OWNER) {
        Ok(session) => session,
        Err(error) => {
            let _ = destroy_frame(frame);
            return Err(error.into());
        }
    };
    let window = match create_window(WindowCreate {
        owner: WHITE_APP_OWNER,
        session,
        frame,
        output,
        placement: WHITE_PLACEMENT,
    })
    .and_then(|window| {
        publish_window_frame(WHITE_APP_OWNER, window, DamageRect::FULL)?;
        Ok(window)
    }) {
        Ok(window) => window,
        Err(error) => {
            let _ = finish_window_session(WHITE_APP_OWNER, session);
            let _ = destroy_frame(frame);
            return Err(error.into());
        }
    };

    Ok(WhitePlaceholderApp {
        owner: WHITE_APP_OWNER,
        session,
        frame,
        window,
        placement: WHITE_PLACEMENT,
    })
}

fn present_composition(
    frames: [FrameHandle; 4],
    placements: [WindowPlacement; 4],
) -> Result<(), DummyUi4ConsumerError> {
    let mut leases: [Option<FrameReadLease>; 4] = [None; 4];
    for slot in 0..frames.len() {
        match acquire_published_frame(frames[slot]) {
            Ok(lease) => leases[slot] = Some(lease),
            Err(error) => {
                release_leases(leases);
                return Err(error.into());
            }
        }
    }

    let result = (|| {
        let mut views: [Option<FrameRgbaView>; 4] = [None; 4];
        for slot in 0..leases.len() {
            views[slot] = Some(published_rgba_view(leases[slot].unwrap())?);
        }
        let views = [
            views[0].unwrap(),
            views[1].unwrap(),
            views[2].unwrap(),
            views[3].unwrap(),
        ];
        for view in views {
            crate::intel::dma_flush(view.virt, view.byte_len);
        }
        let pixels = views.map(|view| unsafe {
            core::slice::from_raw_parts(
                view.virt.cast_const(),
                (view.pitch as usize).saturating_mul(view.height as usize),
            )
        });
        let tile = |slot: usize, placement: WindowPlacement| crate::intel::RgbaOverlayTile {
            x: placement.x as u32,
            y: placement.y as u32,
            width: views[slot].width,
            height: views[slot].height,
            pitch_bytes: views[slot].pitch as usize,
            pixels: pixels[slot],
            expected_rgba: None,
        };
        let tiles = [
            tile(0, placements[0]),
            tile(1, placements[1]),
            tile(2, placements[2]),
            tile(3, placements[3]),
        ];
        if !crate::intel::present_premultiplied_rgba_primary_tiles(&tiles, "ui4-dummy-consumer") {
            return Err(DummyUi4ConsumerError::PresentFailed);
        }
        present_software_cursor_plane()
    })();
    release_leases(leases);
    result
}

fn present_software_cursor_plane() -> Result<(), DummyUi4ConsumerError> {
    use crate::graphics::primitives::Rgba8;

    let visuals = software_cursor_visuals();
    let mut rects: heapless::Vec<crate::intel::LiveOverlayRect, 512> = heapless::Vec::new();

    // Persistent gestures sit below menus and cursor glyphs.
    for visual in &visuals {
        let Some(selection) = visual.selection else {
            continue;
        };
        let fill = Rgba8::new(visual.color.r, visual.color.g, visual.color.b, 42);
        let border = Rgba8::new(visual.color.r, visual.color.g, visual.color.b, 220);
        push_overlay_rect(
            &mut rects,
            selection.x,
            selection.y,
            selection.width,
            selection.height,
            fill,
        );
        push_rect_border(&mut rects, selection, 2, border);
    }

    for visual in &visuals {
        let Some((x, y)) = visual.context_menu else {
            continue;
        };
        let (screen_w, screen_h) =
            crate::intel::active_scanout_dimensions().unwrap_or((2560, 1440));
        let menu_w = 196u32;
        let menu_h = 116u32;
        let menu_x = x.saturating_add(14).min(screen_w.saturating_sub(menu_w));
        let menu_y = y.saturating_add(14).min(screen_h.saturating_sub(menu_h));
        let menu_rect = super::Ui4VisualRect {
            x: menu_x,
            y: menu_y,
            width: menu_w,
            height: menu_h,
        };
        push_overlay_rect(&mut rects, menu_x, menu_y, menu_w, menu_h, Rgba8::new(22, 25, 33, 235));
        push_rect_border(&mut rects, menu_rect, 2, visual.color);
        for row in 1..4u32 {
            push_overlay_rect(
                &mut rects,
                menu_x.saturating_add(12),
                menu_y.saturating_add(row * 27),
                menu_w.saturating_sub(24),
                1,
                Rgba8::new(180, 188, 204, 150),
            );
        }
    }

    // Every source becomes visible only after its first real movement. The
    // color remains bound to the full HID source identity, not to focus.
    for visual in &visuals {
        let x = visual.x;
        let y = visual.y;
        let color = visual.color;
        push_overlay_rect(&mut rects, x.saturating_sub(2), y.saturating_sub(13), 5, 27, color);
        push_overlay_rect(&mut rects, x.saturating_sub(13), y.saturating_sub(2), 27, 5, color);
        push_overlay_rect(
            &mut rects,
            x.saturating_sub(4),
            y.saturating_sub(4),
            9,
            9,
            Rgba8::new(255, 255, 255, 240),
        );
        push_overlay_rect(&mut rects, x.saturating_sub(2), y.saturating_sub(2), 5, 5, color);
    }

    if crate::intel::present_live_overlay_rects(&rects, "ui4-software-cursors") {
        Ok(())
    } else {
        Err(DummyUi4ConsumerError::PresentFailed)
    }
}

fn push_rect_border<const N: usize>(
    rects: &mut heapless::Vec<crate::intel::LiveOverlayRect, N>,
    rect: super::Ui4VisualRect,
    thickness: u32,
    color: crate::graphics::primitives::Rgba8,
) {
    let thickness = thickness.min(rect.width).min(rect.height);
    push_overlay_rect(rects, rect.x, rect.y, rect.width, thickness, color);
    push_overlay_rect(
        rects,
        rect.x,
        rect.y.saturating_add(rect.height.saturating_sub(thickness)),
        rect.width,
        thickness,
        color,
    );
    push_overlay_rect(rects, rect.x, rect.y, thickness, rect.height, color);
    push_overlay_rect(
        rects,
        rect.x.saturating_add(rect.width.saturating_sub(thickness)),
        rect.y,
        thickness,
        rect.height,
        color,
    );
}

fn push_overlay_rect<const N: usize>(
    rects: &mut heapless::Vec<crate::intel::LiveOverlayRect, N>,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: crate::graphics::primitives::Rgba8,
) {
    if width == 0 || height == 0 {
        return;
    }
    let _ = rects.push(crate::intel::LiveOverlayRect::new(x, y, width, height, color));
}

fn release_leases(leases: [Option<FrameReadLease>; 4]) {
    for lease in leases.into_iter().flatten() {
        let _ = release_published_frame(lease);
    }
}

/// Consume only callbacks already routed to these two trusted application
/// owners. UI4 owns hit-testing, per-cursor focus/capture and keyboard pairing.
fn dispatch_ui4_callbacks(
    runtime: &mut Runtime,
) -> Result<(u32, Option<(u32, u32)>), DummyUi4ConsumerError> {
    let mut dirty_clicks = 0u32;
    let mut last_dirty_click = None;
    for owner in [runtime.mandel.owner, runtime.white.owner] {
        for event in take_owner_input_events(owner) {
            match event {
                Ui4InputEvent::Pointer(event) => {
                    if event.buttons_pressed & MIDDLE_BUTTON_MASK != 0 {
                        crate::log_info!(
                            target: "ui4";
                            "ui4 dummy-consumer pan-callback phase=begin owner={:?} cursor={}:{}:{} kind={} vcursor={}\n",
                            owner,
                            event.source.controller_id,
                            event.source.slot_id,
                            event.source.ep_target,
                            event.source.hid_kind,
                            event.vcursor as u8
                        );
                    }
                    if event.buttons_down & MIDDLE_BUTTON_MASK != 0
                        && (event.dx != 0 || event.dy != 0)
                    {
                        handle_pan_callback(runtime, owner, event.dx, event.dy)?;
                    }
                    if event.buttons_released & MIDDLE_BUTTON_MASK != 0 {
                        crate::log_info!(
                            target: "ui4";
                            "ui4 dummy-consumer pan-callback phase=end owner={:?} cursor={}:{}:{} kind={}\n",
                            owner,
                            event.source.controller_id,
                            event.source.slot_id,
                            event.source.ep_target,
                            event.source.hid_kind
                        );
                    }
                    if owner == runtime.mandel.owner
                        && event.window == runtime.mandel.windows[1]
                        && event.buttons_pressed & PRIMARY_BUTTON_MASK != 0
                    {
                        dirty_clicks = dirty_clicks.saturating_add(1);
                        last_dirty_click = Some((event.x, event.y));
                    }
                }
                Ui4InputEvent::Keyboard(event) => {
                    crate::log_info!(
                        target: "ui4";
                        "ui4 dummy-consumer keyboard-callback owner={:?} window={} keyboard={}:{}:{} kind={} codepoint={} combo={} virtual={}\n",
                        owner,
                        event.window.raw(),
                        event.event.controller_id,
                        event.event.slot_id,
                        event.event.ep_target,
                        event.event.kind,
                        event.event.codepoint,
                        event.combo_id,
                        event.virtual_keyboard as u8
                    );
                }
                Ui4InputEvent::Focus(event) => {
                    crate::log_info!(
                        target: "ui4";
                        "ui4 dummy-consumer focus-callback owner={:?} window={} focused={} cursor={}:{}:{} kind={} combo={} vcursor={}\n",
                        owner,
                        event.window.raw(),
                        event.focused as u8,
                        event.source.controller_id,
                        event.source.slot_id,
                        event.source.ep_target,
                        event.source.hid_kind,
                        event.combo_id,
                        event.vcursor as u8
                    );
                }
            }
        }
    }
    Ok((dirty_clicks, last_dirty_click))
}

fn handle_pan_callback(
    runtime: &mut Runtime,
    owner: WindowOwner,
    dx: i32,
    dy: i32,
) -> Result<(), DummyUi4ConsumerError> {
    let Some((width, height)) = crate::intel::active_scanout_dimensions() else {
        return Ok(());
    };
    if owner == runtime.mandel.owner {
        let (dx, dy) = clamp_pan_delta(&runtime.mandel.placements, dx, dy, width, height);
        if dx == 0 && dy == 0 {
            return Ok(());
        }
        for slot in 0..runtime.mandel.windows.len() {
            let next = translated_placement(runtime.mandel.placements[slot], dx, dy);
            set_window_placement(runtime.mandel.owner, runtime.mandel.windows[slot], next)?;
            runtime.mandel.placements[slot] = next;
        }
        crate::log_info!(
            target: "ui4";
            "ui4 dummy-consumer pan-callback phase=update owner={:?} dx={} dy={} anchor={},{}\n",
            owner,
            dx,
            dy,
            runtime.mandel.placements[0].x,
            runtime.mandel.placements[0].y
        );
    } else if owner == runtime.white.owner {
        let (dx, dy) = clamp_pan_delta(&[runtime.white.placement], dx, dy, width, height);
        if dx == 0 && dy == 0 {
            return Ok(());
        }
        let next = translated_placement(runtime.white.placement, dx, dy);
        set_window_placement(runtime.white.owner, runtime.white.window, next)?;
        runtime.white.placement = next;
        crate::log_info!(
            target: "ui4";
            "ui4 dummy-consumer pan-callback phase=update owner={:?} dx={} dy={} anchor={},{}\n",
            owner,
            dx,
            dy,
            runtime.white.placement.x,
            runtime.white.placement.y
        );
    }
    Ok(())
}

fn clamp_pan_delta(
    placements: &[WindowPlacement],
    dx: i32,
    dy: i32,
    width: u32,
    height: u32,
) -> (i32, i32) {
    let min_x = placements
        .iter()
        .map(|placement| placement.x)
        .min()
        .unwrap_or(0);
    let min_y = placements
        .iter()
        .map(|placement| placement.y)
        .min()
        .unwrap_or(0);
    let max_x = placements
        .iter()
        .map(|placement| i64::from(placement.x) + i64::from(placement.width))
        .max()
        .unwrap_or(0);
    let max_y = placements
        .iter()
        .map(|placement| i64::from(placement.y) + i64::from(placement.height))
        .max()
        .unwrap_or(0);
    let min_dx = -i64::from(min_x);
    let min_dy = -i64::from(min_y);
    let max_dx = i64::from(width).saturating_sub(max_x);
    let max_dy = i64::from(height).saturating_sub(max_y);
    (clamp_pan_axis(dx, min_dx, max_dx), clamp_pan_axis(dy, min_dy, max_dy))
}

fn clamp_pan_axis(value: i32, minimum: i64, maximum: i64) -> i32 {
    if minimum > maximum {
        0
    } else {
        i64::from(value).clamp(minimum, maximum) as i32
    }
}

fn translated_placement(mut placement: WindowPlacement, dx: i32, dy: i32) -> WindowPlacement {
    placement.x = placement.x.saturating_add(dx);
    placement.y = placement.y.saturating_add(dy);
    placement
}

fn render_and_publish_mandel(
    app: &MandelPlaceholderApp,
    slot: usize,
    parameter: u32,
) -> Result<PublishedFrame, DummyUi4ConsumerError> {
    let published = render_mandel_frame(app.frames[slot], parameter)?;
    publish_window_frame(app.owner, app.windows[slot], DamageRect::FULL)?;
    Ok(published)
}

fn render_mandel_frame(
    frame: FrameHandle,
    parameter: u32,
) -> Result<PublishedFrame, DummyUi4ConsumerError> {
    let lease = acquire_frame_buffer(frame)?;
    let surface = match gpgpu_rgba_surface(lease) {
        Ok(surface) => surface,
        Err(error) => {
            let _ = cancel_frame_buffer(lease);
            return Err(error.into());
        }
    };
    let rendered = crate::intel::gpgpu::mandel64_worklist_surface_full(surface, parameter)
        .is_some_and(|result| result.ok);
    if !rendered {
        let _ = cancel_frame_buffer(lease);
        return Err(DummyUi4ConsumerError::RenderFailed);
    }
    publish_frame_buffer(lease).map_err(Into::into)
}

fn render_white_frame(frame: FrameHandle) -> Result<PublishedFrame, DummyUi4ConsumerError> {
    let lease = acquire_frame_buffer(frame)?;
    let view = match writable_rgba_view(lease) {
        Ok(view) => view,
        Err(error) => {
            let _ = cancel_frame_buffer(lease);
            return Err(error.into());
        }
    };
    unsafe {
        core::ptr::write_bytes(view.virt, u8::MAX, view.byte_len);
    }
    crate::intel::dma_flush(view.virt, view.byte_len);
    publish_frame_buffer(lease).map_err(Into::into)
}

fn fail_and_cleanup(runtime: Runtime, error: DummyUi4ConsumerError) {
    crate::log_error!(
        target: "ui4";
        "ui4 dummy-consumer stopped dirty={} stream={} error={:?}\n",
        runtime.mandel.dirty_parameter,
        runtime.mandel.stream_parameter,
        error
    );
    cleanup_runtime(runtime);
}

fn cleanup_runtime(runtime: Runtime) {
    let _ = release_cursor(MouseControlPrincipal::KernelApp(1), runtime.heartbeat_cursor.handle);
    cleanup_white_app(runtime.white);
    cleanup_mandel_app(runtime.mandel);
}

fn cleanup_mandel_app(app: MandelPlaceholderApp) {
    let _ = finish_window_session(app.owner, app.session);
    destroy_frames(app.frames);
}

fn cleanup_white_app(app: WhitePlaceholderApp) {
    let _ = finish_window_session(app.owner, app.session);
    let _ = destroy_frame(app.frame);
}

fn destroy_optional_frames(frames: [Option<FrameHandle>; 3]) {
    for frame in frames.into_iter().flatten() {
        let _ = destroy_frame(frame);
    }
}

fn destroy_frames(frames: [FrameHandle; 3]) {
    for frame in frames {
        let _ = destroy_frame(frame);
    }
}
