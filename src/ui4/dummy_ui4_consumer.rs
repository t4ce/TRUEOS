//! Temporary UI4 consumers used to exercise the kernel frame/window contract.
//!
//! Two trusted app identities deliberately mimic two future Blueprint
//! consumers without defining any Blueprint transport or ABI:
//! - one app owns three Mandelbrot windows with immutable/dirty/streaming
//!   cadence;
//! - one app owns a separate immutable white CPU-authored window.

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FramePoolError, FrameReadLease,
    FrameRgbaView, FrameSpec, OutputId, PublishedFrame, ScanoutFormat, WindowBrokerError,
    WindowCreate, WindowId, WindowOwner, WindowPlacement, WindowSessionId, acquire_frame_buffer,
    acquire_published_frame, begin_window_session, cancel_frame_buffer, create_frame,
    create_window, destroy_frame, finish_window_session, gpgpu_rgba_surface,
    publish_frame_buffer, publish_window_frame, published_rgba_view, release_published_frame,
    writable_rgba_view,
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

static ACTIVE: AtomicBool = AtomicBool::new(false);
static STATUS: Mutex<DummyUi4ConsumerSnapshot> =
    Mutex::new(DummyUi4ConsumerSnapshot::empty());

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
pub(crate) enum DummyUi4ConsumerControlError {
    AlreadyRunning,
    TaskUnavailable,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum DummyUi4ConsumerError {
    Frame(FramePoolError),
    Window(WindowBrokerError),
    RenderFailed,
    PresentFailed,
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DummyUi4ConsumerSnapshot {
    pub(crate) active: bool,
    pub(crate) static_parameter: u32,
    pub(crate) dirty_parameter: u32,
    pub(crate) stream_parameter: u32,
    pub(crate) static_frame: Option<FrameHandle>,
    pub(crate) dirty_frame: Option<FrameHandle>,
    pub(crate) stream_frame: Option<FrameHandle>,
    pub(crate) white_frame: Option<FrameHandle>,
    pub(crate) static_window: Option<WindowId>,
    pub(crate) dirty_window: Option<WindowId>,
    pub(crate) stream_window: Option<WindowId>,
    pub(crate) white_window: Option<WindowId>,
    pub(crate) static_publish_serial: u64,
    pub(crate) dirty_publish_serial: u64,
    pub(crate) stream_publish_serial: u64,
    pub(crate) white_publish_serial: u64,
}

impl DummyUi4ConsumerSnapshot {
    const fn empty() -> Self {
        Self {
            active: false,
            static_parameter: STATIC_PARAMETER,
            dirty_parameter: 0,
            stream_parameter: 0,
            static_frame: None,
            dirty_frame: None,
            stream_frame: None,
            white_frame: None,
            static_window: None,
            dirty_window: None,
            stream_window: None,
            white_window: None,
            static_publish_serial: 0,
            dirty_publish_serial: 0,
            stream_publish_serial: 0,
            white_publish_serial: 0,
        }
    }
}

struct MandelPlaceholderApp {
    owner: WindowOwner,
    session: WindowSessionId,
    frames: [FrameHandle; 3],
    windows: [WindowId; 3],
    publish_serials: [u64; 3],
    dirty_parameter: u32,
    stream_parameter: u32,
}

struct WhitePlaceholderApp {
    owner: WindowOwner,
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    publish_serial: u64,
}

struct Runtime {
    mandel: MandelPlaceholderApp,
    white: WhitePlaceholderApp,
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
}

pub(crate) fn start_dummy_ui4_consumer(
    spawner: &Spawner,
) -> Result<(), DummyUi4ConsumerControlError> {
    if ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(DummyUi4ConsumerControlError::AlreadyRunning);
    }
    *STATUS.lock() = DummyUi4ConsumerSnapshot {
        active: true,
        ..DummyUi4ConsumerSnapshot::empty()
    };

    match dummy_ui4_consumer_service_task() {
        Ok(token) => {
            spawner.spawn(token);
            Ok(())
        }
        Err(_) => {
            ACTIVE.store(false, Ordering::Release);
            *STATUS.lock() = DummyUi4ConsumerSnapshot::empty();
            Err(DummyUi4ConsumerControlError::TaskUnavailable)
        }
    }
}

pub(crate) fn dummy_ui4_consumer_snapshot() -> DummyUi4ConsumerSnapshot {
    let mut snapshot = *STATUS.lock();
    snapshot.active = ACTIVE.load(Ordering::Acquire);
    snapshot
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn dummy_ui4_consumer_service_task() {
    let mut runtime = match initialize() {
        Ok(runtime) => runtime,
        Err(error) => {
            crate::log_error!(
                target: "ui4";
                "ui4 dummy-consumer init failed error={:?}\n",
                error
            );
            ACTIVE.store(false, Ordering::Release);
            *STATUS.lock() = DummyUi4ConsumerSnapshot::empty();
            return;
        }
    };

    let mut left_down = drain_mouse_button_state();
    crate::log_info!(
        target: "ui4";
        "ui4 dummy-consumer live apps=2 windows=4 mandel_extent={}x{} mandel_buffers=1/2/3 static={} dirty={} stream={}..={} white_extent={}x{} white_buffers=1 cadence_ms={} plane=primary-compositor input=physical-pointer\n",
        MANDEL_WIDTH,
        MANDEL_HEIGHT,
        STATIC_PARAMETER,
        runtime.mandel.dirty_parameter,
        runtime.mandel.stream_parameter,
        STREAM_PARAMETER_MAX,
        WHITE_WIDTH,
        WHITE_HEIGHT,
        STREAM_PERIOD_MS
    );

    loop {
        if let Some((x, y)) = dirty_window_click(&mut left_down) {
            runtime.mandel.dirty_parameter = runtime.mandel.dirty_parameter.saturating_add(1);
            match render_and_publish_mandel(
                &runtime.mandel,
                1,
                runtime.mandel.dirty_parameter,
            ) {
                Ok(published) => {
                    runtime.mandel.publish_serials[1] = published.publish_serial;
                    let mut status = STATUS.lock();
                    status.dirty_parameter = runtime.mandel.dirty_parameter;
                    status.dirty_publish_serial = published.publish_serial;
                }
                Err(error) => {
                    fail_and_cleanup(runtime, error);
                    return;
                }
            }
            crate::log_info!(
                target: "ui4";
                "ui4 dummy-consumer mandel-dirty-click x={} y={} parameter={}\n",
                x,
                y,
                runtime.mandel.dirty_parameter
            );
        }

        runtime.mandel.stream_parameter =
            if runtime.mandel.stream_parameter == STREAM_PARAMETER_MAX {
                0
            } else {
                runtime.mandel.stream_parameter + 1
            };
        match render_and_publish_mandel(
            &runtime.mandel,
            2,
            runtime.mandel.stream_parameter,
        ) {
            Ok(published) => {
                runtime.mandel.publish_serials[2] = published.publish_serial;
                let mut status = STATUS.lock();
                status.stream_parameter = runtime.mandel.stream_parameter;
                status.stream_publish_serial = published.publish_serial;
            }
            Err(error) => {
                fail_and_cleanup(runtime, error);
                return;
            }
        }

        if let Err(error) = present_composition(runtime.composition_frames()) {
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
    let runtime = Runtime { mandel, white };
    if let Err(error) = present_composition(runtime.composition_frames()) {
        cleanup_runtime(runtime);
        return Err(error);
    }

    *STATUS.lock() = DummyUi4ConsumerSnapshot {
        active: true,
        static_parameter: STATIC_PARAMETER,
        dirty_parameter: runtime.mandel.dirty_parameter,
        stream_parameter: runtime.mandel.stream_parameter,
        static_frame: Some(runtime.mandel.frames[0]),
        dirty_frame: Some(runtime.mandel.frames[1]),
        stream_frame: Some(runtime.mandel.frames[2]),
        white_frame: Some(runtime.white.frame),
        static_window: Some(runtime.mandel.windows[0]),
        dirty_window: Some(runtime.mandel.windows[1]),
        stream_window: Some(runtime.mandel.windows[2]),
        white_window: Some(runtime.white.window),
        static_publish_serial: runtime.mandel.publish_serials[0],
        dirty_publish_serial: runtime.mandel.publish_serials[1],
        stream_publish_serial: runtime.mandel.publish_serials[2],
        white_publish_serial: runtime.white.publish_serial,
    };
    Ok(runtime)
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
    let mut publications = [PublishedFrame {
        frame: frames[0],
        buffer_index: 0,
        publish_serial: 0,
    }; 3];
    for slot in 0..frames.len() {
        match render_mandel_frame(frames[slot], initial_parameters[slot]) {
            Ok(published) => publications[slot] = published,
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
        publish_serials: publications.map(|published| published.publish_serial),
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
    let published = match render_white_frame(frame) {
        Ok(published) => published,
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
        publish_serial: published.publish_serial,
    })
}

fn present_composition(frames: [FrameHandle; 4]) -> Result<(), DummyUi4ConsumerError> {
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
            tile(0, STATIC_PLACEMENT),
            tile(1, DIRTY_PLACEMENT),
            tile(2, STREAM_PLACEMENT),
            tile(3, WHITE_PLACEMENT),
        ];
        if crate::intel::present_premultiplied_rgba_primary_tiles(
            &tiles,
            "ui4-dummy-consumer",
        ) {
            Ok(())
        } else {
            Err(DummyUi4ConsumerError::PresentFailed)
        }
    })();
    release_leases(leases);
    result
}

fn release_leases(leases: [Option<FrameReadLease>; 4]) {
    for lease in leases.into_iter().flatten() {
        let _ = release_published_frame(lease);
    }
}

fn current_left_button() -> bool {
    crate::r::cursor::preferred_kernel_hw_cursor_snapshot_with_slot_buttons()
        .is_some_and(|(_, _, _, buttons)| buttons & 1 != 0)
}

fn drain_mouse_button_state() -> bool {
    let mut left_down = current_left_button();
    while let Some(event) = crate::usb3::hid::input::pop_mouse_event() {
        left_down = event.buttons & 1 != 0;
    }
    left_down
}

fn dirty_window_click(left_down: &mut bool) -> Option<(u32, u32)> {
    let mut pressed = false;
    let mut saw_event = false;
    while let Some(event) = crate::usb3::hid::input::pop_mouse_event() {
        saw_event = true;
        let now_down = event.buttons & 1 != 0;
        pressed |= now_down && !*left_down;
        *left_down = now_down;
    }
    let (_, nx, ny, buttons) =
        crate::r::cursor::preferred_kernel_hw_cursor_snapshot_with_slot_buttons()?;
    if !saw_event {
        let now_down = buttons & 1 != 0;
        pressed = now_down && !*left_down;
        *left_down = now_down;
    }
    if !pressed {
        return None;
    }
    let (scanout_width, scanout_height) = crate::intel::active_scanout_dimensions()?;
    let x = (nx.clamp(0.0, 1.0) * f64::from(scanout_width)) as u32;
    let y = (ny.clamp(0.0, 1.0) * f64::from(scanout_height)) as u32;
    let x1 = (DIRTY_PLACEMENT.x as u32).saturating_add(DIRTY_PLACEMENT.width);
    let y1 = (DIRTY_PLACEMENT.y as u32).saturating_add(DIRTY_PLACEMENT.height);
    (x >= DIRTY_PLACEMENT.x as u32 && x < x1 && y >= DIRTY_PLACEMENT.y as u32 && y < y1)
        .then_some((x, y))
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
    ACTIVE.store(false, Ordering::Release);
    *STATUS.lock() = DummyUi4ConsumerSnapshot::empty();
}

fn cleanup_runtime(runtime: Runtime) {
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
