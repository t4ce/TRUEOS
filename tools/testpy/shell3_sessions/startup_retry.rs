#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_crypto;
extern crate self as spin;
extern crate self as trueos_time;
pub struct AccountId;
pub use std::time::Duration;

pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}

pub struct Timer;
pub struct After(bool);
impl Timer {
    pub fn after(_: Duration) -> After { After(false) }
}
impl core::future::Future for After {
    type Output = ();
    fn poll(mut self: core::pin::Pin<&mut Self>, _: &mut core::task::Context<'_>) -> core::task::Poll<()> {
        if self.0 { core::task::Poll::Ready(()) }
        else { self.0 = true; core::task::Poll::Pending }
    }
}

#[macro_export]
macro_rules! log_info { (target: $target:expr; $($args:tt)*) => {{ let _ = ($target, format_args!($($args)*)); }}; }
#[macro_export]
macro_rules! log_warn { (target: $target:expr; $($args:tt)*) => {{ let _ = ($target, format_args!($($args)*)); }}; }

mod percpu {
    pub fn current_slot() -> usize { 4 }
}
mod r {
    pub mod services {
        pub mod shell3_font_service {
            pub fn cell_extent(_: u32) -> Option<(u32, u32)> {
                crate::ui4::WORLD.with_borrow(|world| world.cell_extent)
            }
            pub async fn prepare() -> Result<(), &'static str> {
                crate::ui4::WORLD.with_borrow_mut(|world| {
                    world.font_prepares += 1;
                    if world.font_error { Err("font-prepare-failed") } else { Ok(()) }
                })
            }
        }
    }
}

mod ui4 {
    use std::cell::RefCell;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Ui4CursorSource { pub controller_id: u32, pub slot_id: u32, pub ep_target: u32 }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct WindowId(pub u32);
    impl WindowId { pub fn raw(self) -> u32 { self.0 } }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct WindowOwner(pub u32);
    impl WindowOwner { pub const SHELL3_SERVICE: Self = Self(4); }

    pub(crate) struct World {
        pub dimensions: Option<(u32, u32)>,
        pub cell_extent: Option<(u32, u32)>,
        pub selections: Vec<(Ui4CursorSource, WindowOwner, WindowId, u32, u32)>,
        pub select_error: bool,
        pub font_prepares: usize,
        pub font_error: bool,
    }
    impl Default for World {
        fn default() -> Self {
            Self { dimensions: Some((1920, 1080)), cell_extent: Some((1, 1)),
                selections: Vec::new(), select_error: false, font_prepares: 0, font_error: false }
        }
    }
    thread_local! { pub(crate) static WORLD: RefCell<World> = RefCell::default(); }
    pub fn output_dimensions() -> Option<(u32, u32)> { WORLD.with_borrow(|world| world.dimensions) }
    pub fn select_window_for_cursor_at(
        source: Ui4CursorSource, owner: WindowOwner, window: WindowId, x: u32, y: u32,
    ) -> Result<(), &'static str> {
        WORLD.with_borrow_mut(|world| {
            world.selections.push((source, owner, window, x, y));
            if world.select_error { Err("selection-failed") } else { Ok(()) }
        })
    }

    // This is the production desktop adapter with only its executor attribute
    // removed. No independent frame/window/panel API exists in the fixture.
    #[path = "@LINK_SERVICE_PATH@"]
    pub(crate) mod link_service;
}

mod shell3 {
    use std::{cell::RefCell, collections::VecDeque};

    #[path = "@TRUEOS_ROOT@/src/shell3/startup.rs"]
    pub(crate) mod startup;

    const DEFAULT_COLUMNS: usize = @DEFAULT_COLUMNS@;
    const DEFAULT_ROWS: usize = @DEFAULT_ROWS@;

    #[derive(Default)]
    struct World {
        bound: bool,
        launches: Vec<(String, Vec<u8>, u64, Option<String>)>,
        launch_error: bool,
        errors: Vec<&'static str>,
        terminal_selected: bool,
        link_launches: Vec<u64>,
        link_error: bool,
        link_enters: Vec<&'static str>,
        configured: Vec<service::WindowGeometry>,
        startup_requests: Vec<startup::Startup>,
        request_error: bool,
        request_calls: usize,
    }
    thread_local! { static WORLD: RefCell<World> = RefCell::default(); }
    pub(crate) mod service {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) struct WindowGeometry { pub x: i32, pub y: i32, pub width: u32, pub height: u32 }
        pub fn microfont_scale() -> u32 { 1 }
        pub(crate) fn request_shell3_with_startup(startup: Option<super::startup::Startup>) -> Result<u32, &'static str> {
            super::WORLD.with_borrow_mut(|world| {
                world.request_calls += 1;
                if world.request_error { return Err("instance-limit"); }
                world.startup_requests.push(startup.expect("desktop must request retained startup"));
                Ok(4)
            })
        }
        pub(super) fn launch_bytes_with_script_for_account(
            archive: String, bytes: Vec<u8>, frontend: u64, script: Option<String>,
            _: Option<crate::AccountId>,
        ) -> Result<super::QueuedApp, &'static str> {
            super::WORLD.with_borrow_mut(|world| {
                assert!(world.bound, "launch must follow frontend/window binding");
                world.launches.push((archive, bytes, frontend, script));
                if world.launch_error { return Err("vm-admission-failed"); }
                Ok(super::QueuedApp { slot: "runtime-test".into() })
            })
        }
    }
    mod link {
        pub fn start(frontend: u64) -> Result<(), &'static str> {
            super::WORLD.with_borrow_mut(|world| {
                assert!(world.bound, "native Link must follow frontend/window binding");
                world.link_launches.push(frontend);
                if world.link_error { Err("link-admission-failed") } else { Ok(()) }
            })
        }
    }
    mod tui {
        pub fn bind_ui4_window(_: u64, _: crate::ui4::WindowId, _: u32) {
            super::WORLD.with_borrow_mut(|world| world.bound = true);
        }
    }
    struct MatrixSlots;
    impl MatrixSlots {
        fn echo(_: Option<&str>, _: Option<u64>, error: &'static str) {
            WORLD.with_borrow_mut(|world| world.errors.push(error));
        }
    }
    struct QueuedApp { slot: String }

    #[derive(Clone, Default, PartialEq)]
    struct Snapshot;
    impl Snapshot {
        fn size(&self) -> (usize, usize) { (80, 25) }
        fn rendered_lines(&self) -> Vec<Vec<u8>> { vec![vec![]] }
        fn reveals(&self) {}
        fn matrix_area(&self) {}
        fn edit_life(&self) {}
    }
    mod update {
        pub fn take_updates(_: &mut super::Snapshot, _: super::Snapshot, _: &[()]) {}
    }

    #[derive(Default)]
    struct Show {
        window: Option<crate::ui4::WindowId>,
        closed: bool,
        closing: bool,
        close_during_present: bool,
        initial_grid_error: bool,
        outcomes: VecDeque<Result<(), &'static str>>,
        calls: usize,
    }
    impl Show {
        fn font_scale(&self) -> u32 { 1 }
        fn window(&self) -> Option<crate::ui4::WindowId> { self.window }
        fn is_closed(&self) -> bool { self.window.is_none() || self.closed }
        fn is_closing(&self) -> bool { self.closing }
        fn initial_grid(&self, columns: usize, rows: usize) -> Result<(usize, usize), &'static str> {
            if self.initial_grid_error { Err("shell3-font-physical-size-unavailable") }
            else { Ok((columns, rows)) }
        }
        fn set_font_scale(&self, _: u32, _: usize, _: usize) -> Result<(u32, u32), &'static str> {
            Ok((80, 25))
        }
        fn resize_needed(&self) -> bool { false }
        async fn present(
            &mut self, _: &[&[u8]], _: usize, _: usize, _: &(), _: (), _: (), _: (),
        ) -> Result<(), &'static str> {
            self.calls += 1;
            self.window = Some(crate::ui4::WindowId(7));
            if self.close_during_present { self.closing = true; }
            self.outcomes.pop_front().unwrap_or(Ok(()))
        }
    }

    struct Shell3 {
        executor_slot: u32,
        columns: usize,
        rows_count: usize,
        pending_presentation: Option<(Snapshot, (), Vec<Vec<u8>>)>,
        pending_startup: Option<startup::Startup>,
        show: Show,
        tui_frontend: u64,
        update_baseline: Snapshot,
        update_callbacks: Vec<()>,
        matrix_selection_dirty: bool,
        active_matrix_slot: Option<String>,
        active_matrix_lifetime: Option<u64>,
    }
    impl Shell3 {
        fn fixture() -> Self {
            WORLD.with_borrow_mut(|world| *world = World::default());
            crate::ui4::WORLD.with_borrow_mut(|world| *world = crate::ui4::World::default());
            Self {
                executor_slot: 4, columns: 80, rows_count: 25,
                pending_presentation: None,
                pending_startup: Some(startup::Startup::Blueprint {
                    archive: "/apps/common/ide/builds/1-7/4/example.bp".into(),
                    bytes: vec![1, 2, 3], script: Some("ide example".into()),
                    access_account: None,
                }),
                show: Show::default(), tui_frontend: 33,
                update_baseline: Snapshot, update_callbacks: vec![],
                matrix_selection_dirty: false, active_matrix_slot: None,
                active_matrix_lifetime: None,
            }
        }
        fn reconcile_matrix_selection(&mut self) {}
        fn font_scale_needed(&self) -> bool { false }
        fn requested_font_scale(&self) -> u32 { 1 }
        fn set(&mut self, columns: usize, rows: usize) { self.columns = columns; self.rows_count = rows; }
        fn configure_window(&mut self, geometry: service::WindowGeometry) {
            assert_eq!(self.show.calls, 0, "configure placement before first paint");
            WORLD.with_borrow_mut(|world| world.configured.push(geometry));
        }
        async fn resize_ui4_to_current(&mut self) -> Result<(), &'static str> { Ok(()) }
        fn capture_update_snapshot(&self) -> Snapshot { Snapshot }
        fn tui_frontend(&self) -> u64 { self.tui_frontend }
        fn select_queued_app(&mut self, app: QueuedApp) {
            WORLD.with_borrow_mut(|world| world.terminal_selected = true);
            self.active_matrix_slot = Some(app.slot);
            self.matrix_selection_dirty = true;
        }
        fn enter_native_terminal(&mut self, slot: &'static str) {
            WORLD.with_borrow_mut(|world| {
                assert!(world.bound);
                assert_eq!(world.link_launches, vec![self.tui_frontend]);
                world.link_enters.push(slot);
                world.terminal_selected = true;
            });
            self.active_matrix_slot = Some(slot.into());
            self.matrix_selection_dirty = true;
        }
        fn select_matrix_slot_name(&mut self, slot: &str) {
            // Ordinary Matrix navigation returns to the command pane. It
            // deliberately does not enter a TUI or grant its initial claim.
            WORLD.with_borrow_mut(|world| world.terminal_selected = false);
            self.active_matrix_slot = Some(slot.into());
            self.matrix_selection_dirty = true;
        }

        @PRESENTATION_METHODS@
    }

    fn run<T>(future: impl core::future::Future<Output = T>) -> T {
        let mut future = std::pin::pin!(future);
        let mut context = core::task::Context::from_waker(core::task::Waker::noop());
        match future.as_mut().poll(&mut context) {
            core::task::Poll::Ready(value) => value,
            core::task::Poll::Pending => panic!("unexpected boundary wait"),
        }
    }
    fn launch_count() -> usize { WORLD.with_borrow(|world| world.launches.len()) }

    #[test]
    fn failed_paints_retain_startup_and_success_launches_once() {
        let mut shell = Shell3::fixture();
        shell.show.outcomes.extend([Err("shell3-show-copy-unavailable"), Err("retry"), Ok(())]);
        for _ in 0..2 {
            assert!(run(shell.present()).is_err());
            assert!(shell.presentation_pending());
            assert!(!shell.show_is_closed());
            assert_eq!(launch_count(), 0);
        }
        assert_eq!(shell.show.calls, 2, "one paint attempt per caller retry");
        run(shell.present()).unwrap();
        assert!(!shell.presentation_pending());
        assert!(shell.matrix_selection_dirty, "startup selection still needs its redraw");
        assert_eq!(shell.active_matrix_slot.as_deref(), Some("runtime-test"));
        WORLD.with_borrow(|world| assert!(world.terminal_selected,
            "startup must preserve the foreground route for the guest's first terminal claim"));
        WORLD.with_borrow(|world| assert_eq!(world.launches, vec![(
            "/apps/common/ide/builds/1-7/4/example.bp".into(), vec![1, 2, 3],
            33, Some("ide example".into()),
        )]));
        run(shell.present()).unwrap();
        assert_eq!(launch_count(), 1);
    }

    #[test]
    fn failure_before_surface_allocation_keeps_owner_for_later_retry() {
        let mut shell = Shell3::fixture();
        shell.show.initial_grid_error = true;
        assert!(run(shell.present()).is_err());
        assert!(shell.pending_presentation.is_none());
        assert!(shell.presentation_pending());
        assert!(!shell.show_is_closed());
        assert_eq!(shell.show.calls, 0);
        shell.show.initial_grid_error = false;
        run(shell.present()).unwrap();
        assert_eq!(launch_count(), 1);
        assert!(!shell.presentation_pending());
    }

    #[test]
    fn closing_or_closed_shell_cancels_startup_before_retry() {
        for closed in [false, true] {
            let mut shell = Shell3::fixture();
            shell.show.outcomes.push_back(Err("shell3-show-copy-unavailable"));
            assert!(run(shell.present()).is_err());
            shell.show.closing = !closed;
            shell.show.closed = closed;
            assert_eq!(run(shell.present()), Err("shell3-show-closed"));
            assert!(shell.pending_startup.is_none());
            assert!(!shell.presentation_pending());
            assert_eq!(shell.show.calls, 1);
            drop(shell);
            assert_eq!(launch_count(), 0);
        }
    }

    #[test]
    fn closing_during_presentation_cancels_startup() {
        let mut shell = Shell3::fixture();
        shell.show.close_during_present = true;
        run(shell.present()).unwrap();
        assert!(shell.pending_startup.is_none());
        assert!(!shell.presentation_pending());
        assert_eq!(launch_count(), 0);
    }

    #[test]
    fn dropping_unpresented_shell_does_not_launch() {
        let mut shell = Shell3::fixture();
        shell.show.outcomes.push_back(Err("shell3-show-copy-unavailable"));
        assert!(run(shell.present()).is_err());
        drop(shell);
        assert_eq!(launch_count(), 0);
    }

    #[test]
    fn launch_failure_is_reported_once_without_retrying_admission() {
        let mut shell = Shell3::fixture();
        WORLD.with_borrow_mut(|world| world.launch_error = true);
        run(shell.present()).unwrap();
        run(shell.present()).unwrap();
        assert!(!shell.presentation_pending());
        assert_eq!(launch_count(), 1);
        WORLD.with_borrow(|world| assert_eq!(world.errors, vec!["vm-admission-failed"]));
    }

    fn link_startup() -> startup::Startup {
        startup::Startup::Link {
            source: crate::ui4::Ui4CursorSource { controller_id: 2, slot_id: 3, ep_target: 4 },
            anchor: (1800, 1000),
        }
    }

    #[test]
    fn link_first_paint_retries_then_enters_native_route_and_selects_exact_cursor_window_once() {
        let mut shell = Shell3::fixture();
        let startup = link_startup();
        startup.configure_window(&mut shell);
        shell.pending_startup = Some(startup);
        shell.show.outcomes.extend([Err("copy-unavailable"), Ok(())]);
        assert!(run(shell.present()).is_err());
        assert!(shell.presentation_pending());
        WORLD.with_borrow(|world| {
            assert!(world.link_launches.is_empty());
            assert_eq!(world.configured, vec![service::WindowGeometry {
                x: 1800, y: 1000, width: DEFAULT_COLUMNS as u32, height: DEFAULT_ROWS as u32,
            }]);
        });
        crate::ui4::WORLD.with_borrow(|world| assert!(world.selections.is_empty()));
        run(shell.present()).unwrap();
        run(shell.present()).unwrap();
        assert_eq!(shell.active_matrix_slot.as_deref(), Some("link"));
        WORLD.with_borrow(|world| {
            assert_eq!(world.link_launches, vec![33]);
            assert_eq!(world.link_enters, vec!["link"]);
            assert!(world.launches.is_empty(), "Link needs no Blueprint launch");
        });
        crate::ui4::WORLD.with_borrow(|world| assert_eq!(world.selections, vec![(
            crate::ui4::Ui4CursorSource { controller_id: 2, slot_id: 3, ep_target: 4 },
            crate::ui4::WindowOwner::SHELL3_SERVICE, crate::ui4::WindowId(7), 1800, 1000,
        )]));
    }

    #[test]
    fn link_default_terminal_geometry_clamps_to_output_at_the_desktop_anchor() {
        let mut shell = Shell3::fixture();
        crate::ui4::WORLD.with_borrow_mut(|world| world.cell_extent = Some((8, 16)));
        link_startup().configure_window(&mut shell);
        WORLD.with_borrow(|world| assert_eq!(world.configured.last(), Some(&service::WindowGeometry {
            x: 1920 - DEFAULT_COLUMNS as i32 * 8,
            y: 1080 - DEFAULT_ROWS as i32 * 16,
            width: DEFAULT_COLUMNS as u32 * 8,
            height: DEFAULT_ROWS as u32 * 16,
        })));
        crate::ui4::WORLD.with_borrow_mut(|world| world.dimensions = Some((320, 200)));
        link_startup().configure_window(&mut shell);
        WORLD.with_borrow(|world| assert_eq!(world.configured.last(), Some(&service::WindowGeometry {
            x: 0, y: 0, width: 320, height: 200,
        })));
        crate::ui4::WORLD.with_borrow_mut(|world| world.dimensions = None);
        link_startup().configure_window(&mut shell);
        crate::ui4::WORLD.with_borrow_mut(|world| {
            world.dimensions = Some((1920, 1080)); world.cell_extent = None;
        });
        link_startup().configure_window(&mut shell);
        WORLD.with_borrow(|world| assert_eq!(world.configured.len(), 2));
    }

    #[test]
    fn link_admission_failure_reports_once_and_still_selects_shell_to_show_the_error() {
        let mut shell = Shell3::fixture();
        shell.pending_startup = Some(link_startup());
        WORLD.with_borrow_mut(|world| world.link_error = true);
        crate::ui4::WORLD.with_borrow_mut(|world| world.select_error = true);
        run(shell.present()).unwrap();
        run(shell.present()).unwrap();
        assert!(!shell.presentation_pending());
        assert_eq!(shell.active_matrix_slot, None);
        WORLD.with_borrow(|world| {
            assert_eq!(world.link_launches, vec![33]);
            assert!(world.link_enters.is_empty());
            assert_eq!(world.errors, vec!["link-admission-failed"]);
        });
        crate::ui4::WORLD.with_borrow(|world| assert_eq!(world.selections.len(), 1));
    }

    #[test]
    fn closing_before_link_first_paint_cancels_helper_and_cursor_selection() {
        let mut shell = Shell3::fixture();
        shell.pending_startup = Some(link_startup());
        shell.show.outcomes.push_back(Err("copy-unavailable"));
        assert!(run(shell.present()).is_err());
        shell.show.closing = true;
        assert_eq!(run(shell.present()), Err("shell3-show-closed"));
        WORLD.with_borrow(|world| assert!(world.link_launches.is_empty()));
        crate::ui4::WORLD.with_borrow(|world| assert!(world.selections.is_empty()));
    }

    fn poll_service(future: core::pin::Pin<&mut impl core::future::Future>) {
        let mut context = core::task::Context::from_waker(core::task::Waker::noop());
        assert!(future.poll(&mut context).is_pending());
    }

    #[test]
    fn desktop_link_adapter_coalesces_requests_and_admits_new_shell3_startups() {
        let _shell = Shell3::fixture();
        let first = crate::ui4::Ui4CursorSource { controller_id: 1, slot_id: 1, ep_target: 1 };
        let latest = crate::ui4::Ui4CursorSource { controller_id: 2, slot_id: 2, ep_target: 2 };
        crate::ui4::link_service::request_open(first, (10, 20));
        crate::ui4::link_service::request_open(latest, (30, 40));
        let mut task = std::pin::pin!(crate::ui4::link_service::ui4_link_service_task());
        poll_service(task.as_mut());
        WORLD.with_borrow(|world| {
            assert_eq!(world.request_calls, 1);
            assert!(matches!(&world.startup_requests[..], [startup::Startup::Link { source, anchor }]
                if *source == latest && *anchor == (30, 40)));
            assert!(world.link_launches.is_empty(), "desktop adapter only queues a Shell3 startup");
        });
        crate::ui4::WORLD.with_borrow(|world| assert!(world.selections.is_empty()));
        crate::ui4::link_service::request_open(first, (50, 60));
        poll_service(task.as_mut());
        WORLD.with_borrow(|world| {
            assert_eq!(world.request_calls, 2, "a later desktop action admits a fresh Shell3");
            assert!(matches!(world.startup_requests.last(), Some(startup::Startup::Link { source, anchor })
                if *source == first && *anchor == (50, 60)));
        });
        crate::ui4::WORLD.with_borrow(|world| assert_eq!(world.font_prepares, 2));
    }
}
