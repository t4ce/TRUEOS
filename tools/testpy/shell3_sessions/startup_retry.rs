#![allow(dead_code)]
extern crate alloc;

#[macro_export]
macro_rules! log_info { ($($args:tt)*) => {}; }

mod percpu {
    pub fn current_slot() -> usize { 4 }
}
mod r {
    pub mod services {
        pub mod shell3_font_service {
            pub fn cell_extent(_: u32) -> Option<(u32, u32)> { Some((1, 1)) }
        }
    }
}

mod shell3 {
    use std::{cell::RefCell, collections::VecDeque};

    #[path = "@TRUEOS_ROOT@/src/shell3/startup.rs"]
    mod startup;

    #[derive(Default)]
    struct World {
        bound: bool,
        launches: Vec<(String, Vec<u8>, u64, Option<String>)>,
        launch_error: bool,
        errors: Vec<&'static str>,
    }
    thread_local! { static WORLD: RefCell<World> = RefCell::default(); }
    mod service {
        pub fn microfont_scale() -> u32 { 1 }
        pub fn launch_bytes_with_script(
            archive: String, bytes: Vec<u8>, frontend: u64, script: Option<String>,
        ) -> Result<super::QueuedApp, &'static str> {
            super::WORLD.with_borrow_mut(|world| {
                assert!(world.bound, "launch must follow frontend/window binding");
                world.launches.push((archive, bytes, frontend, script));
                if world.launch_error { return Err("vm-admission-failed"); }
                Ok(super::QueuedApp { slot: "runtime-test".into() })
            })
        }
    }
    mod tui {
        pub fn bind_ui4_window(_: u64, _: u32) {
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
    }
    mod update {
        pub fn take_updates(_: &mut super::Snapshot, _: super::Snapshot, _: &[()]) {}
    }

    #[derive(Default)]
    struct Show {
        window: Option<u32>,
        closed: bool,
        closing: bool,
        close_during_present: bool,
        initial_grid_error: bool,
        outcomes: VecDeque<Result<(), &'static str>>,
        calls: usize,
    }
    impl Show {
        fn window(&self) -> Option<u32> { self.window }
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
            &mut self, _: &[&[u8]], _: usize, _: usize, _: &(), _: (), _: (),
        ) -> Result<(), &'static str> {
            self.calls += 1;
            self.window = Some(7);
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
            Self {
                executor_slot: 4, columns: 80, rows_count: 25,
                pending_presentation: None,
                pending_startup: Some(startup::Startup::Blueprint {
                    archive: "/apps/common/ide/builds/1-7/4/example.bp".into(),
                    bytes: vec![1, 2, 3], script: Some("ide example".into()),
                }),
                show: Show::default(), tui_frontend: 33,
                update_baseline: Snapshot, update_callbacks: vec![],
                matrix_selection_dirty: false, active_matrix_slot: None,
                active_matrix_lifetime: None,
            }
        }
        fn reconcile_matrix_selection(&mut self) {}
        fn font_scale_needed(&self) -> bool { false }
        fn set(&mut self, columns: usize, rows: usize) { self.columns = columns; self.rows_count = rows; }
        async fn resize_ui4_to_current(&mut self) -> Result<(), &'static str> { Ok(()) }
        fn capture_update_snapshot(&self) -> Snapshot { Snapshot }
        fn tui_frontend(&self) -> u64 { self.tui_frontend }
        fn select_queued_app(&mut self, _: QueuedApp) {}
        fn select_matrix_slot_name(&mut self, slot: &str) {
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
}
