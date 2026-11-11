#!/usr/bin/env python3
"""Exercise UI4 dock geometry, compact presentation, and placing-click routing.

Compile the production broker records, collapse methods, geometry helpers and
input blocks on the host. Only storage, output discovery and animation/GPU
services are stubbed; frame projection and state changes run unchanged.
"""
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item
from test_vgpu_batch_overlay import block


ROOT = Path(__file__).resolve().parents[2]
BROKER = "src/ui4/window_broker.rs"
INPUT = "src/ui4/input_broker.rs"
COLLAPSE = "src/ui4/collapse.rs"


def harness_source():
    broker = (ROOT / BROKER).read_text()
    input_source = (ROOT / INPUT).read_text()
    collapse = (ROOT / COLLAPSE).read_text()
    broker_types = [
        "WindowOwner", "WindowId", "WindowSessionId", "WindowState", "WindowPlane",
        "WindowPlacement", "WindowInteraction", "Ui4FrameEscapeKeyAction", "WindowCreate",
        "WindowBackground", "WindowSnapshot", "WindowPlacementTransition", "WindowDockTarget",
        "WindowBrokerError", "WindowRecord", "WindowOpenTransition", "WindowCloseTransition",
        "WindowDockChange",
    ]
    broker_impls = [
        "WindowOwner", "WindowId", "WindowSessionId", "WindowPlane", "WindowPlacement",
        "WindowInteraction", "WindowRecord",
    ]
    broker_functions = [
        "next_serial", "pack_handle", "unpack_handle", "collapse_state", "toggle_collapse",
        "collapse_at_cursor", "change_collapse", "collapsed_cursor_placement",
        "follow_collapsed_cursor", "center_restored_window_on_cursor",
        "docked_window_placement", "producer_resize_required", "change_window_dock_record", "change_window_dock_record_at",
    ]
    geometry_functions = [
        "DockZoneMetrics", "dock_zone_metrics", "scale_reference", "clamp_zone_metric",
        "dock_zones_with_reference", "dock_target_at_in_zones", "dock_zone_contains",
        "dock_zone_row_span", "dock_zone_column_span", "last_true_prefix", "first_true_suffix",
        "dock_zone_local_contains", "normalized_ellipse_contains", "visual_rect_contains",
        "Ui4VisualRect", "Ui4DockZone",
        "Ui4SpiritDockCircle", "spirit_circle_contains", "spirit_circle_row_span",
    ]
    geometry_tests = "\n".join(re.findall(
        r"    #\[test\]\n    fn (?:dock_|bottom_dock_).*?^    }\n",
        input_source, re.MULTILINE | re.DOTALL))
    assert geometry_tests.count("#[test]") >= 6, "production dock tests missing"
    geometry_constants = "\n".join(re.findall(r"^const DOCK_.*?;$", input_source, re.MULTILINE))
    title_constant = re.search(r"^pub\(crate\) const MAX_WINDOW_TITLE_BYTES:.*?;$", broker, re.MULTILINE)[0]
    # The shared item reader includes a tuple struct's following impl because
    # its terminator is a semicolon. Keep that impl in the explicit list below.
    broker_code = "\n".join(item(BROKER, name).split("\nimpl ", 1)[0] for name in broker_types)
    broker_code += "\n" + "\n".join(block(broker, f"impl {name} {{") for name in broker_impls)
    broker_code += "\n" + "\n".join(item(BROKER, name) for name in broker_functions)
    collapse_code = item(COLLAPSE, "Collapse") + "\n" + block(collapse, "impl Collapse {")
    follow_block = block(input_source, "if let Some(target) = self.cursors[index].collapse_drag {")
    absorb_block = block(input_source, "if self.cursors[index].absorb_select {")
    return r'''#![allow(dead_code)]
use std::sync::{Mutex, MutexGuard};
struct TestMutex<T>(Mutex<T>);
impl<T> TestMutex<T> {
    const fn new(value: T) -> Self { Self(Mutex::new(value)) }
    fn lock(&self) -> MutexGuard<'_, T> { self.0.lock().unwrap() }
}
mod ui4 {
    @APPLICATION_PLANES@
    pub(crate) const PRIMARY_PLANE_SLOT: usize = 0;
    pub(crate) const INTERACTION_OVERLAY_PLANE_SLOT: usize = 4;
    pub(crate) type FrameHandle = u64;
    pub(crate) type FrameReadLease = u64;
    pub(crate) type OutputId = u8;
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum FrameBuffering { Double, Triple }
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) struct DamageRegion;
    impl DamageRegion { pub(crate) const FULL: Self = Self; }
    #[derive(Copy, Clone, Debug)]
    pub(crate) enum CollapseError { NotFound, Busy, Ui4 }
    pub(crate) fn output_dimensions() -> Option<(u32, u32)> { Some((1920, 1080)) }
    pub(crate) use window_broker::*;
    mod cursor_frame_inout { pub(crate) fn selection_strip_stack_changed() {} }
    mod collapse_animation {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};
        pub(crate) static BEGINS: AtomicUsize = AtomicUsize::new(0);
        pub(crate) fn cancel(_: WindowOwner, _: WindowId) {}
        pub(crate) fn begin(source: WindowSnapshot, compact: WindowSnapshot) -> Result<(), ()> {
            assert_eq!(source.frame, 101);
            assert_eq!(compact.frame, 202);
            BEGINS.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        pub(crate) fn project(snapshot: WindowSnapshot) -> WindowSnapshot { snapshot }
    }
    mod collapse {
        use super::*;
        @COLLAPSE_CODE@
        pub(crate) fn ensure_window_collapse(owner: WindowOwner, id: WindowId) -> Result<(), CollapseError> {
            // Model only image allocation, which belongs to the GPU service.
            window_broker::install_test_icon(owner, id).map_err(|_| CollapseError::NotFound)
        }
    }
    mod window_broker {
        use super::*;
        use crate::TestMutex;
        @TITLE_CONSTANT@
        @BROKER_CODE@
        struct TestBroker { windows: Vec<WindowRecord>, changes: usize }
        impl TestBroker {
            fn checked_window_mut(&mut self, owner: WindowOwner, id: WindowId) -> Result<&mut WindowRecord, WindowBrokerError> {
                let (slot, generation) = unpack_handle(id.raw())?;
                let window = self.windows.get_mut(slot).ok_or(WindowBrokerError::InvalidHandle)?;
                if window.generation != generation { return Err(WindowBrokerError::InvalidHandle); }
                if window.owner != owner { return Err(WindowBrokerError::OwnerMismatch); }
                Ok(window)
            }
            fn mark_composition_changed(&mut self) { self.changes += 1; }
        }
        static WINDOW_BROKER: TestMutex<TestBroker> = TestMutex::new(TestBroker { windows: Vec::new(), changes: 0 });
        pub(crate) fn window_snapshot(owner: WindowOwner, id: WindowId) -> Option<WindowSnapshot> {
            let mut broker = WINDOW_BROKER.lock();
            let (slot, _) = unpack_handle(id.raw()).ok()?;
            broker.checked_window_mut(owner, id).ok()?.snapshot(slot)
        }
        pub(crate) fn install_test_icon(owner: WindowOwner, id: WindowId) -> Result<(), WindowBrokerError> {
            let mut broker = WINDOW_BROKER.lock();
            let window = broker.checked_window_mut(owner, id)?;
            if window.collapse.is_none() {
                window.collapse = Some(super::collapse::Collapse { frame: 202, width: 64, height: 64, tile: None });
            }
            Ok(())
        }
        fn setup(icon: bool) -> (WindowOwner, WindowId, WindowRecord) {
            let owner = WindowOwner::Vm(1);
            let id = WindowId(pack_handle(0, 1).unwrap());
            let mut window = WindowRecord::new(1, WindowCreate {
                owner, session: WindowSessionId(1), frame: 101, output: 0,
                plane: WindowPlane::Universal(1),
                placement: WindowPlacement { x: 100, y: 200, width: 800, height: 600, z: 7, opacity: 233, visible: true },
                interaction: WindowInteraction::APPLICATION,
            }, FrameBuffering::Triple);
            window.state = WindowState::Ready;
            window.publish_serial = 42;
            if icon { window.collapse = Some(super::collapse::Collapse { frame: 202, width: 64, height: 64, tile: None }); }
            *WINDOW_BROKER.lock() = TestBroker { windows: vec![window], changes: 0 };
            (owner, id, window)
        }
        fn assert_producer_unchanged(before: WindowRecord) {
            let broker = WINDOW_BROKER.lock();
            let current = broker.windows[0];
            assert_eq!(current.frame, before.frame);
            assert_eq!(current.placement, before.placement);
            assert_eq!(current.committed_resize_extent, before.committed_resize_extent);
            assert_eq!(current.pending_resize_extent, before.pending_resize_extent);
            assert_eq!(current.resize_epoch, before.resize_epoch);
            assert_eq!(current.publish_serial, before.publish_serial);
            assert_eq!(current.interaction, before.interaction);
        }
        #[test]
        fn collapse_follow_and_restore_keep_the_producer_intact() {
            for registered in [false, true] {
                let (owner, id, before) = setup(registered);
                assert_eq!(collapse_at_cursor(owner, id, 640, 1079), Ok(()));
                let compact = window_snapshot(owner, id).unwrap();
                assert_eq!(compact.frame, 202);
                assert_eq!((compact.placement.x, compact.placement.y), (608, 1016));
                assert_eq!((compact.placement.width, compact.placement.height), (64, 64));
                assert_eq!(compact.placement.opacity, 233);
                assert_eq!(compact.placement.z, 7);
                assert!(!compact.interaction.maximizable);
                assert_eq!(collapse_state(owner, id), Some(true));
                assert_producer_unchanged(before);
                assert_eq!(follow_collapsed_cursor(owner, id, 0, 0), Ok(()));
                assert_eq!((window_snapshot(owner, id).unwrap().placement.x, window_snapshot(owner, id).unwrap().placement.y), (0, 0));
                assert_eq!(follow_collapsed_cursor(owner, id, u32::MAX, u32::MAX), Ok(()));
                let compact = window_snapshot(owner, id).unwrap();
                assert_eq!((compact.placement.x, compact.placement.y), (1856, 1016));
                assert_producer_unchanged(before);
                let changes = WINDOW_BROKER.lock().changes;
                assert_eq!(follow_collapsed_cursor(owner, id, u32::MAX, u32::MAX), Ok(()));
                assert_eq!(WINDOW_BROKER.lock().changes, changes, "stationary follow should not republish");
                assert_eq!(toggle_collapse(owner, id), Ok(()));
                let restored = window_snapshot(owner, id).unwrap();
                assert_eq!(restored.frame, before.frame);
                assert_eq!(restored.placement, before.placement);
                assert_eq!(restored.interaction, before.interaction);
                assert_eq!(collapse_state(owner, id), Some(false));
                assert_eq!(follow_collapsed_cursor(owner, id, 100, 100), Err(WindowBrokerError::InteractionDenied));
                assert_producer_unchanged(before);
            }
        }
        #[test]
        fn repeating_dock_collapse_is_idempotent() {
            let (owner, id, before) = setup(true);
            assert_eq!(collapse_at_cursor(owner, id, 640, 1079), Ok(()));
            let compact = window_snapshot(owner, id).unwrap();
            let changes = WINDOW_BROKER.lock().changes;
            let begins = super::collapse_animation::BEGINS.load(std::sync::atomic::Ordering::SeqCst);
            assert_eq!(collapse_at_cursor(owner, id, 640, 1079), Ok(()));
            assert_eq!(window_snapshot(owner, id).unwrap(), compact);
            assert_eq!(WINDOW_BROKER.lock().changes, changes);
            assert_eq!(collapse_at_cursor(owner, id, 1200, 500), Ok(()));
            let compact = window_snapshot(owner, id).unwrap();
            assert_eq!(compact.frame, 202);
            assert_eq!((compact.placement.x, compact.placement.y), (1168, 468));
            assert_eq!(super::collapse_animation::BEGINS.load(std::sync::atomic::Ordering::SeqCst), begins);
            assert_producer_unchanged(before);
        }
        #[test]
        fn action_only_targets_cannot_commit_dock_geometry() {
            let (_, _, mut window) = setup(true);
            let before = window;
            for target in [WindowDockTarget::Collapse, WindowDockTarget::GenericResize, WindowDockTarget::SpiritAccess] {
                assert_eq!(docked_window_placement(window.interaction, window.placement, target, 1920, 1080), before.placement);
                assert_eq!(change_window_dock_record(&mut window, Some(target), 1920, 1080, None, None), Err(WindowBrokerError::InteractionDenied));
                assert_eq!(window.placement, before.placement);
                assert_eq!(window.resize_epoch, before.resize_epoch);
                assert_eq!(window.dock_target, None);
            }
        }
        #[test]
        fn compact_cursor_placement_handles_missing_or_tiny_outputs() {
            let (_, _, window) = setup(true);
            let tile = WindowPlacement { width: 64, height: 64, ..window.placement };
            let tiny = collapsed_cursor_placement(tile, 1000, 1000, Some((32, 24)));
            assert_eq!((tiny.x, tiny.y), (0, 0));
            assert_eq!((tiny.width, tiny.height), (64, 64));
            let unknown = collapsed_cursor_placement(tile, 800, 600, None);
            assert_eq!((unknown.x, unknown.y), (768, 568));
        }
        pub(crate) fn setup_for_input() -> (WindowOwner, WindowId) {
            let (owner, id, _) = setup(true);
            collapse_at_cursor(owner, id, 640, 1079).unwrap();
            (owner, id)
        }
    }
    mod input_broker {
        use super::*;
        @GEOMETRY_CONSTANTS@
        @GEOMETRY_CODE@
        @WINDOW_TARGET@
        struct Route {
            x: u32, y: u32, buttons_down: u32, visible_after_motion: bool,
            collapse_drag: Option<WindowTarget>, capture: Option<WindowTarget>,
            absorb_select: bool, suppress_context_menu_open: bool,
        }
        struct Event { buttons_down: u32 }
        struct Input { cursors: Vec<Route>, forwarded: usize }
        impl Input {
            fn pointer(&mut self, x: u32, y: u32, buttons_down: u32, pressed: u32) {
                let index = 0;
                let event = Event { buttons_down };
                @FOLLOW_BLOCK@
                @ABSORB_BLOCK@
                self.forwarded += 1;
            }
        }
        #[test]
        fn placing_click_moves_the_icon_and_absorbs_the_entire_gesture() {
            let (owner, window) = super::window_broker::setup_for_input();
            let target = WindowTarget { owner, window };
            let mut input = Input { cursors: vec![Route {
                x: 640, y: 1079, buttons_down: 0, visible_after_motion: true,
                collapse_drag: Some(target), capture: Some(target),
                absorb_select: false, suppress_context_menu_open: false,
            }], forwarded: 0 };
            input.pointer(800, 600, 0, 0);
            assert!(input.cursors[0].collapse_drag.is_some());
            assert_eq!(super::window_broker::window_snapshot(owner, window).unwrap().placement.x, 768);
            input.pointer(900, 700, 1, 1);
            assert!(input.cursors[0].collapse_drag.is_none());
            assert!(input.cursors[0].capture.is_none());
            assert!(input.cursors[0].absorb_select);
            let placement = super::window_broker::window_snapshot(owner, window).unwrap().placement;
            assert_eq!((placement.x, placement.y), (868, 668));
            input.pointer(920, 720, 1, 0);
            input.pointer(920, 720, 0, 0);
            assert!(!input.cursors[0].absorb_select);
            assert_eq!(input.forwarded, 0);
            assert_eq!(super::window_broker::window_snapshot(owner, window).unwrap().placement, placement);
            assert_eq!(super::window_broker::collapse_state(owner, window), Some(true));
            input.pointer(920, 720, 0, 0);
            assert_eq!(input.forwarded, 1);
        }
        #[cfg(test)] mod geometry_tests {
            use super::*;
            @GEOMETRY_TESTS@
        }
    }
}
'''.replace("@APPLICATION_PLANES@", "\n".join(
        constant("src/ui4/mod.rs", name) for name in ["MAX_APPLICATION_PLANE_SLOTS", "ADDITIONAL_APPLICATION_PLANE_SLOT", "DEFAULT_APPLICATION_PLANE_MASK", "SUPPORTED_APPLICATION_PLANE_MASK"]
    ) + "\n" + item("src/ui4/mod.rs", "is_application_plane_slot")).replace("@TITLE_CONSTANT@", title_constant).replace("@BROKER_CODE@", broker_code).replace(
        "@COLLAPSE_CODE@", collapse_code).replace("@GEOMETRY_CONSTANTS@", geometry_constants).replace(
        "@GEOMETRY_CODE@", "\n".join(item(INPUT, name) for name in geometry_functions)).replace(
        "@GEOMETRY_TESTS@", geometry_tests).replace("@FOLLOW_BLOCK@", follow_block).replace(
        "@ABSORB_BLOCK@", absorb_block).replace("@WINDOW_TARGET@", item(INPUT, "WindowTarget"))


def main():
    with tempfile.TemporaryDirectory(prefix="trueos-ui4-dock-collapse-") as temporary:
        directory = Path(temporary)
        source = directory / "tests.rs"
        binary = directory / "tests"
        source.write_text(harness_source())
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
