//! Temporary UI4 consumers used to exercise the kernel frame/window contract.
//!
//! Two trusted app identities deliberately mimic two future Blueprint
//! consumers without defining any Blueprint transport or ABI:
//! - app 1 owns three Mandelbrot windows with immutable/dirty/streaming
//!   cadence;
//! - app 2 is the decoded-video/SFC probe consumer in `video_frame`, with one
//!   ordinary streaming RGBA frame backed by three buffers. The boot demo is
//!   required to reach this UI4 path and cannot escape to linked display
//!   planes or direct-primary CPU presentation.

use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::r::mouse_motion_service::{
    MOUSE_CONTROL_EASING_FAST_LINEAR, MOUSE_CONTROL_EASING_NATURAL, MOUSE_CONTROL_FLAG_CLEAR_QUEUE,
    MOUSE_CONTROL_OPCODE_STROKE, MOUSE_CONTROL_OPCODE_TELEPORT, MOUSE_CONTROL_PATH_CUBIC,
    MOUSE_CONTROL_PATH_LINE, MouseControlCommand, MouseControlCursor, MouseControlError,
    MouseControlPrincipal, cursor_is_idle, release_cursor, request_cursor, submit_program,
};

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FramePoolError, FrameReadLease,
    FrameRgbaView, FrameSpec, OutputId, PublishedFrame, ScanoutFormat, Ui4ButtonPhase,
    Ui4InputEvent, Ui4PanPhase, WindowBrokerError, WindowCreate, WindowId, WindowOwner,
    WindowPlacement, WindowPlane, WindowSessionId, WindowSnapshot, acknowledge_window_frame,
    acquire_frame_buffer, acquire_published_frame, begin_window_session, cancel_frame_buffer,
    create_frame, create_window, destroy_frame, finish_window_session, gpgpu_rgba_surface,
    publish_frame_buffer, publish_window_frame, published_rgba_view, release_published_frame,
    software_cursor_visuals, take_owner_input_events, visible_windows_for_output,
};

const MANDEL_APP_OWNER: WindowOwner = WindowOwner::KernelApp(1);
const VIDEO_APP_OWNER: WindowOwner = WindowOwner::KernelApp(2);
const MANDEL_WIDTH: u32 = super::BOOT_DEMO_FRAME_WIDTH;
const MANDEL_HEIGHT: u32 = super::BOOT_DEMO_FRAME_HEIGHT;
const STATIC_PARAMETER: u32 = 128;
const STREAM_PARAMETER_MAX: u32 = 64;
const COMPOSITION_PERIOD_MS: u64 = 16;
const MANDEL_STREAM_DIVISOR: u8 = 2;
const HEARTBEAT_REST_FRAMES: u32 = 50;
const MAX_COMPOSITION_WINDOWS: usize = 8;
const PRIMARY_BUTTON_MASK: u32 = 1 << 0;

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
    views: [crate::intel::gpgpu::GpgpuRect; 3],
    pending_pans: [(i32, i32); 3],
    dirty_parameter: u32,
    stream_parameter: u32,
}

struct Runtime {
    mandel: MandelPlaceholderApp,
    heartbeat_cursor: MouseControlCursor,
    heartbeat_rest_frames: u32,
    mandel_stream_countdown: u8,
    composition: CompositionState,
    cursor_plane: CursorPlaneState,
}

#[derive(Copy, Clone)]
struct CompositionState {
    primary: PlaneCompositionState,
    solara: PlaneCompositionState,
    draw3d: PlaneCompositionState,
}

#[derive(Copy, Clone)]
struct PlaneCompositionState {
    initialized: bool,
    windows: [Option<CompositionWindowStamp>; MAX_COMPOSITION_WINDOWS],
}

#[derive(Copy, Clone)]
enum CompositionTarget {
    Primary,
    Overlay(usize),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct CompositionWindowStamp {
    id: WindowId,
    frame: FrameHandle,
    publish_serial: u64,
    placement: WindowPlacement,
}

#[derive(Copy, Clone)]
struct CursorPlaneState {
    previous_bounds: Option<crate::intel::CompositionDamageRect>,
    signature: u64,
    initialized: bool,
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
        "ui4 dummy-consumer live app1=mandel windows=3 mandel_extent={}x{} mandel_buffers=1/2/3 static={} dirty={} stream={}..={} app2=decoded-video-sfc-probe buffers=3 format=rgba8-premultiplied boot_playback=ui4-probe-required legacy_fallback=0 composition_ms={} plane=primary-compositor input=ui4-owner-queues callbacks=focus,left-click,middle-pan,keyboard frame_drag=ui4-broker-secondary heartbeat_vcursor_slot={}\n",
        MANDEL_WIDTH,
        MANDEL_HEIGHT,
        STATIC_PARAMETER,
        runtime.mandel.dirty_parameter,
        runtime.mandel.stream_parameter,
        STREAM_PARAMETER_MAX,
        COMPOSITION_PERIOD_MS,
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

        if runtime.mandel_stream_countdown == 0 {
            runtime.mandel.stream_parameter =
                if runtime.mandel.stream_parameter == STREAM_PARAMETER_MAX {
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
            runtime.mandel_stream_countdown = MANDEL_STREAM_DIVISOR.saturating_sub(1);
        } else {
            runtime.mandel_stream_countdown -= 1;
        }

        if let Err(error) = present_composition(&mut runtime) {
            fail_and_cleanup(runtime, error);
            return;
        }

        Timer::after(EmbassyDuration::from_millis(COMPOSITION_PERIOD_MS)).await;
    }
}

fn initialize() -> Result<Runtime, DummyUi4ConsumerError> {
    let output = OutputId::from_slot(0).expect("UI4 D01 must exist");
    let mandel = initialize_mandel_app(output)?;
    let heartbeat_cursor =
        match request_cursor(MouseControlPrincipal::KernelApp(1), "ui4-heartbeat") {
            Ok(cursor) => cursor,
            Err(error) => {
                cleanup_mandel_app(mandel);
                return Err(error.into());
            }
        };
    let mut runtime = Runtime {
        mandel,
        heartbeat_cursor,
        heartbeat_rest_frames: 0,
        mandel_stream_countdown: 0,
        composition: CompositionState {
            primary: PlaneCompositionState {
                initialized: false,
                windows: [None; MAX_COMPOSITION_WINDOWS],
            },
            solara: PlaneCompositionState {
                initialized: false,
                windows: [None; MAX_COMPOSITION_WINDOWS],
            },
            draw3d: PlaneCompositionState {
                initialized: false,
                windows: [None; MAX_COMPOSITION_WINDOWS],
            },
        },
        cursor_plane: CursorPlaneState {
            previous_bounds: None,
            signature: 0,
            initialized: false,
        },
    };
    if let Err(error) = present_composition(&mut runtime) {
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
            base_color: None,
        }) {
            Ok(frame) => frames[slot] = Some(frame),
            Err(error) => {
                destroy_optional_frames(frames);
                return Err(error.into());
            }
        }
    }
    let frames = [frames[0].unwrap(), frames[1].unwrap(), frames[2].unwrap()];
    let views = [crate::intel::gpgpu::GpgpuRect::new(0, 0, MANDEL_WIDTH, MANDEL_HEIGHT); 3];
    let initial_parameters = [STATIC_PARAMETER, 0, 0];
    for slot in 0..frames.len() {
        match render_mandel_frame(frames[slot], views[slot], initial_parameters[slot], slot != 1) {
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
            plane: WindowPlane::Primary,
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
        views,
        pending_pans: [(0, 0); 3],
        dirty_parameter: 0,
        stream_parameter: 0,
    })
}

fn present_composition(runtime: &mut Runtime) -> Result<(), DummyUi4ConsumerError> {
    let output = OutputId::from_slot(0).expect("UI4 D01 must exist");
    let windows = visible_windows_for_output(output);
    if windows.is_empty()
        || windows.len() > MAX_COMPOSITION_WINDOWS
        || windows.iter().any(|window| {
            let slot = window.plane.slot();
            slot != super::PRIMARY_PLANE_SLOT
                && slot != super::RGB_OVERLAY_PLANE_SLOT_2
                && slot != super::RGB_OVERLAY_PLANE_SLOT_3
        })
    {
        return Err(DummyUi4ConsumerError::PresentFailed);
    }

    present_plane_composition(
        &mut runtime.composition.primary,
        &windows,
        CompositionTarget::Primary,
    )?;
    present_plane_composition(
        &mut runtime.composition.solara,
        &windows,
        CompositionTarget::Overlay(super::RGB_OVERLAY_PLANE_SLOT_2),
    )?;
    present_plane_composition(
        &mut runtime.composition.draw3d,
        &windows,
        CompositionTarget::Overlay(super::RGB_OVERLAY_PLANE_SLOT_3),
    )?;
    let cursor_rects = software_cursor_rects();
    present_software_cursor_plane(&mut runtime.cursor_plane, &cursor_rects)?;
    super::screenshot::capture_compositor_frame(&windows, &cursor_rects);
    Ok(())
}

fn present_plane_composition(
    state: &mut PlaneCompositionState,
    all_windows: &[WindowSnapshot],
    target: CompositionTarget,
) -> Result<(), DummyUi4ConsumerError> {
    let plane_slot = match target {
        CompositionTarget::Primary => super::PRIMARY_PLANE_SLOT,
        CompositionTarget::Overlay(slot) => slot,
    };
    let windows: Vec<WindowSnapshot> = all_windows
        .iter()
        .copied()
        .filter(|window| window.plane.slot() == plane_slot)
        .collect();
    if windows.len() > MAX_COMPOSITION_WINDOWS {
        return Err(DummyUi4ConsumerError::PresentFailed);
    }
    if windows.is_empty() && !state.initialized {
        return Ok(());
    }

    let mut next_windows = [None; MAX_COMPOSITION_WINDOWS];
    for (slot, window) in windows.iter().enumerate() {
        next_windows[slot] = Some(CompositionWindowStamp {
            id: window.id,
            frame: window.frame,
            publish_serial: window.publish_serial,
            placement: window.placement,
        });
    }

    let mut changed = [false; MAX_COMPOSITION_WINDOWS];
    let mut damage = None;
    for (slot, current) in next_windows.iter().flatten().enumerate() {
        let previous = state
            .windows
            .iter()
            .flatten()
            .find(|previous| previous.id == current.id);
        changed[slot] = !state.initialized || previous != Some(current);
        if changed[slot] {
            damage = union_damage(damage, placement_damage(current.placement));
        }
        if let Some(previous) = previous
            && previous.placement != current.placement
        {
            damage = union_damage(damage, placement_damage(previous.placement));
        }
    }
    for previous in state.windows.iter().flatten() {
        if !next_windows
            .iter()
            .flatten()
            .any(|current| current.id == previous.id)
        {
            damage = union_damage(damage, placement_damage(previous.placement));
        }
    }
    if !state.initialized {
        let (width, height) = crate::intel::active_scanout_dimensions().unwrap_or((0, 0));
        damage = Some(crate::intel::CompositionDamageRect::new(0, 0, width, height));
    }

    if let Some(damage) = damage {
        let mut leases = Vec::with_capacity(windows.len());
        for window in &windows {
            match acquire_published_frame(window.frame) {
                Ok(lease) => leases.push(lease),
                Err(error) => {
                    release_leases(&leases);
                    return Err(error.into());
                }
            }
        }

        let result = (|| {
            let views: Vec<FrameRgbaView> = leases
                .iter()
                .copied()
                .map(published_rgba_view)
                .collect::<Result<_, _>>()?;
            for (slot, view) in views.iter().enumerate() {
                if changed[slot] {
                    crate::intel::dma_flush(view.virt, view.byte_len);
                }
            }
            let pixels: Vec<&[u8]> = views
                .iter()
                .map(|view| unsafe {
                    core::slice::from_raw_parts(
                        view.virt.cast_const(),
                        (view.pitch as usize).saturating_mul(view.height as usize),
                    )
                })
                .collect();
            let tiles: Vec<_> = windows
                .iter()
                .zip(views.iter())
                .zip(pixels.iter())
                .map(|((window, view), pixels)| crate::intel::RgbaOverlayTile {
                    x: window.placement.x.max(0) as u32,
                    y: window.placement.y.max(0) as u32,
                    width: view.width.min(window.placement.width),
                    height: view.height.min(window.placement.height),
                    pitch_bytes: view.pitch as usize,
                    pixels,
                    expected_rgba: None,
                })
                .collect();
            let presented = match target {
                CompositionTarget::Primary => {
                    crate::intel::present_premultiplied_rgba_primary_tiles_damage(
                        &tiles,
                        damage,
                        "ui4-dummy-consumer-primary",
                    )
                }
                CompositionTarget::Overlay(slot) => {
                    let reason = match slot {
                        super::RGB_OVERLAY_PLANE_SLOT_2 => "ui4-solara-slot2",
                        super::RGB_OVERLAY_PLANE_SLOT_3 => "ui4-draw3d-slot3",
                        _ => "ui4-overlay",
                    };
                    crate::intel::present_premultiplied_rgba_overlay_tiles_on_slot_damage(
                        slot, &tiles, damage, reason,
                    )
                }
            };
            if !presented {
                return Err(DummyUi4ConsumerError::PresentFailed);
            }
            Ok(())
        })();
        release_leases(&leases);
        result?;
        for window in &windows {
            let _ = acknowledge_window_frame(window.id, window.publish_serial);
        }
        state.initialized = true;
        state.windows = next_windows;
    }
    Ok(())
}

fn software_cursor_rects() -> heapless::Vec<crate::intel::LiveOverlayRect, 512> {
    use crate::graphics::primitives::Rgba8;

    let visuals = software_cursor_visuals();
    let mut rects: heapless::Vec<crate::intel::LiveOverlayRect, 512> = heapless::Vec::new();

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
        if !visual.draw_cursor {
            continue;
        }
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

    rects
}

fn present_software_cursor_plane(
    state: &mut CursorPlaneState,
    rects: &[crate::intel::LiveOverlayRect],
) -> Result<(), DummyUi4ConsumerError> {
    let current_bounds = overlay_rect_bounds(rects);
    let signature = overlay_rect_signature(rects);
    if state.initialized && signature == state.signature && current_bounds == state.previous_bounds
    {
        return Ok(());
    }
    let damage = match (state.previous_bounds, current_bounds) {
        (Some(previous), Some(current)) => damage_union(previous, current),
        (Some(previous), None) => previous,
        (None, Some(current)) => current,
        (None, None) => return Ok(()),
    };
    if crate::intel::present_live_overlay_rects_damage(rects, damage, "ui4-software-cursors") {
        state.previous_bounds = current_bounds;
        state.signature = signature;
        state.initialized = true;
        Ok(())
    } else {
        Err(DummyUi4ConsumerError::PresentFailed)
    }
}

fn damage_union(
    a: crate::intel::CompositionDamageRect,
    b: crate::intel::CompositionDamageRect,
) -> crate::intel::CompositionDamageRect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = a.x.saturating_add(a.width).max(b.x.saturating_add(b.width));
    let bottom =
        a.y.saturating_add(a.height)
            .max(b.y.saturating_add(b.height));
    crate::intel::CompositionDamageRect::new(
        x,
        y,
        right.saturating_sub(x),
        bottom.saturating_sub(y),
    )
}

fn union_damage(
    current: Option<crate::intel::CompositionDamageRect>,
    next: crate::intel::CompositionDamageRect,
) -> Option<crate::intel::CompositionDamageRect> {
    Some(
        current
            .map(|current| damage_union(current, next))
            .unwrap_or(next),
    )
}

fn placement_damage(placement: WindowPlacement) -> crate::intel::CompositionDamageRect {
    crate::intel::CompositionDamageRect::new(
        placement.x.max(0) as u32,
        placement.y.max(0) as u32,
        placement.width,
        placement.height,
    )
}

fn overlay_rect_bounds(
    rects: &[crate::intel::LiveOverlayRect],
) -> Option<crate::intel::CompositionDamageRect> {
    rects.iter().fold(None, |bounds, rect| {
        union_damage(
            bounds,
            crate::intel::CompositionDamageRect::new(rect.x, rect.y, rect.width, rect.height),
        )
    })
}

fn overlay_rect_signature(rects: &[crate::intel::LiveOverlayRect]) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for rect in rects {
        for value in [
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            u32::from_le_bytes([rect.color.r, rect.color.g, rect.color.b, rect.color.a]),
        ] {
            hash ^= u64::from(value);
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    hash ^ rects.len() as u64
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

fn release_leases(leases: &[FrameReadLease]) {
    for lease in leases {
        let _ = release_published_frame(*lease);
    }
}

/// Consume callbacks routed to both temporary app identities. App 2's pixels
/// are driven by the boot decoder, while its window still follows normal UI4
/// focus and movement policy.
fn dispatch_ui4_callbacks(
    runtime: &mut Runtime,
) -> Result<(u32, Option<(u32, u32)>), DummyUi4ConsumerError> {
    let mut dirty_clicks = 0u32;
    let mut last_dirty_click = None;
    for owner in [runtime.mandel.owner, VIDEO_APP_OWNER] {
        for event in take_owner_input_events(owner) {
            match event {
                Ui4InputEvent::Pointer(_) => {}
                Ui4InputEvent::Button(event) => {
                    crate::log_info!(
                        target: "ui4";
                        "ui4 dummy-consumer button-callback owner={:?} window={} phase={:?} changed=0x{:X} down=0x{:X} local={},{} cursor={}:{}:{} kind={}\n",
                        owner,
                        event.window.raw(),
                        event.phase,
                        event.changed_buttons,
                        event.buttons_down,
                        event.local_x,
                        event.local_y,
                        event.source.controller_id,
                        event.source.slot_id,
                        event.source.ep_target,
                        event.source.hid_kind
                    );
                    if owner == runtime.mandel.owner
                        && event.window == runtime.mandel.windows[1]
                        && event.phase == Ui4ButtonPhase::Down
                        && event.changed_buttons & PRIMARY_BUTTON_MASK != 0
                    {
                        dirty_clicks = dirty_clicks.saturating_add(1);
                        last_dirty_click = Some((event.x, event.y));
                    }
                }
                Ui4InputEvent::Pan(event) => {
                    crate::log_info!(
                        target: "ui4";
                        "ui4 dummy-consumer pan-callback owner={:?} window={} phase={:?} dx={} dy={} local={},{} cursor={}:{}:{} kind={} combo={} vcursor={}\n",
                        owner,
                        event.window.raw(),
                        event.phase,
                        event.dx,
                        event.dy,
                        event.local_x,
                        event.local_y,
                        event.source.controller_id,
                        event.source.slot_id,
                        event.source.ep_target,
                        event.source.hid_kind,
                        event.combo_id,
                        event.vcursor as u8
                    );
                    handle_mandel_pan_callback(
                        runtime,
                        owner,
                        event.window,
                        event.phase,
                        event.dx,
                        event.dy,
                    )?;
                }
                Ui4InputEvent::Resize(event) => {
                    crate::log_info!(
                        target: "ui4";
                        "ui4 dummy-consumer resize-callback owner={:?} window={} old={}x{} new={}x{}\n",
                        owner,
                        event.window.raw(),
                        event.old_width,
                        event.old_height,
                        event.width,
                        event.height
                    );
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

fn handle_mandel_pan_callback(
    runtime: &mut Runtime,
    owner: WindowOwner,
    window: WindowId,
    phase: Ui4PanPhase,
    dx: i32,
    dy: i32,
) -> Result<(), DummyUi4ConsumerError> {
    if owner != runtime.mandel.owner {
        return Ok(());
    }
    let Some(slot) = runtime
        .mandel
        .windows
        .iter()
        .position(|candidate| *candidate == window)
    else {
        return Ok(());
    };
    // Only the dirty frame accepts complex-plane pan. The immutable frame is
    // static, while the streaming frame retains the mirrored half-render.
    if slot != 1 {
        return Ok(());
    }

    match phase {
        Ui4PanPhase::Begin => runtime.mandel.pending_pans[slot] = (0, 0),
        Ui4PanPhase::Update => {
            let pending = &mut runtime.mandel.pending_pans[slot];
            pending.0 = pending.0.saturating_add(dx);
            pending.1 = pending.1.saturating_add(dy);
        }
        Ui4PanPhase::End => {
            let (pan_x, pan_y) = runtime.mandel.pending_pans[slot];
            runtime.mandel.pending_pans[slot] = (0, 0);
            if pan_x == 0 && pan_y == 0 {
                return Ok(());
            }

            // Dragging the canvas right/down moves the sampled complex-plane
            // interval left/up so the rendered grid follows the pointer.
            let view = {
                let view = &mut runtime.mandel.views[slot];
                view.x = view.x.saturating_sub(pan_x);
                view.y = view.y.saturating_sub(pan_y);
                *view
            };
            let parameter = runtime.mandel.dirty_parameter;
            render_and_publish_mandel(&runtime.mandel, slot, parameter)?;
            crate::log_info!(
                target: "ui4";
                "ui4 dummy-consumer mandel-pan-applied window={} slot={} drag={},{} view={},{},{}x{} iterations={}\n",
                window.raw(),
                slot,
                pan_x,
                pan_y,
                view.x,
                view.y,
                view.width,
                view.height,
                parameter
            );
        }
    }
    Ok(())
}

fn render_and_publish_mandel(
    app: &MandelPlaceholderApp,
    slot: usize,
    parameter: u32,
) -> Result<PublishedFrame, DummyUi4ConsumerError> {
    let published = render_mandel_frame(app.frames[slot], app.views[slot], parameter, slot != 1)?;
    publish_window_frame(app.owner, app.windows[slot], DamageRect::FULL)?;
    Ok(published)
}

fn render_mandel_frame(
    frame: FrameHandle,
    view: crate::intel::gpgpu::GpgpuRect,
    parameter: u32,
    mirror_at_center: bool,
) -> Result<PublishedFrame, DummyUi4ConsumerError> {
    let lease = acquire_frame_buffer(frame)?;
    let surface = match gpgpu_rgba_surface(lease) {
        Ok(surface) => surface,
        Err(error) => {
            let _ = cancel_frame_buffer(lease);
            return Err(error.into());
        }
    };
    let rendered = if mirror_at_center {
        crate::intel::gpgpu::mandel64_worklist_surface_full(surface, parameter)
    } else {
        crate::intel::gpgpu::mandel64_worklist_surface_view(surface, view, parameter)
    }
    .is_some_and(|result| result.ok);
    if !rendered {
        let _ = cancel_frame_buffer(lease);
        return Err(DummyUi4ConsumerError::RenderFailed);
    }
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
    cleanup_mandel_app(runtime.mandel);
}

fn cleanup_mandel_app(app: MandelPlaceholderApp) {
    let _ = finish_window_session(app.owner, app.session);
    destroy_frames(app.frames);
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
