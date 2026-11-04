#!/usr/bin/env python3
"""Run production quad-dock transactions, click routing and handle raster on host."""
from pathlib import Path
import subprocess
import tempfile

from test_ui4_dock_collapse import harness_source as base_source
from test_clip_position3_uv_texture import item
from test_vgpu_batch_overlay import block

ROOT = Path(__file__).resolve().parents[2]


def harness_source():
    source = base_source()
    input_source = (ROOT / 'src/ui4/input_broker.rs').read_text()
    broker_code = '\n'.join(item('src/ui4/window_broker.rs', name) for name in
                            ['quad_dock_layout', 'quad_dock_rects', 'resize_quad_dock'])
    source = source.replace('        struct TestBroker {', broker_code + '\n        struct TestBroker {')
    source = source.replace('            fn mark_composition_changed(&mut self)', '''
            fn snapshots(&self, output: OutputId) -> Vec<WindowSnapshot> {
                self.windows.iter().enumerate().filter_map(|(slot, w)| w.snapshot(slot))
                    .filter(|w| w.output == output && w.placement.visible && w.state == WindowState::Ready).collect()
            }
            fn mark_composition_changed(&mut self)''')
    source = source.replace('        pub(crate) fn setup_for_input()', r'''
        pub(super) fn quad_layout() -> Option<[WindowSnapshot; 4]> {
            quad_dock_layout(&WINDOW_BROKER.lock().snapshots(0), 1921, 1081)
        }
        pub(super) fn move_one() { WINDOW_BROKER.lock().windows[0].placement.x += 1; }
        pub(super) fn setup_quad() -> [WindowSnapshot; 4] {
            let (_, _, template) = setup(false);
            let targets = [WindowDockTarget::TopLeft, WindowDockTarget::TopRight,
                WindowDockTarget::BottomLeft, WindowDockTarget::BottomRight];
            let mut windows = Vec::new();
            for target in targets {
                let mut w = template;
                change_window_dock_record(&mut w, Some(target), 1921, 1081, None, None).unwrap();
                windows.push(w);
            }
            *WINDOW_BROKER.lock() = TestBroker { windows, changes: 0 };
            super::input_broker::RESIZES.lock().clear();
            quad_layout().unwrap()
        }
        #[test]
        fn quad_commit_partitions_odd_output_and_preserves_dock_restore() {
            let before = setup_quad();
            let restore = WINDOW_BROKER.lock().windows[0].restore_placement;
            resize_quad_dock(before, 1921, 1081, 701, 403).unwrap();
            let after = quad_layout().unwrap();
            for (a, rect) in after.iter().zip(quad_dock_rects(1921, 1081, 701, 403)) {
                let p = a.placement;
                assert_eq!((p.x, p.y, p.width, p.height), rect);
            }
            let broker = WINDOW_BROKER.lock();
            assert_eq!(broker.changes, 1, "one atomic composition change");
            for (index, w) in broker.windows.iter().enumerate() {
                assert_eq!(w.restore_placement, restore);
                assert_eq!(w.dock_target, before[index].dock_target);
                assert_eq!(w.pending_resize_extent, Some((w.placement.width, w.placement.height)));
                assert!(w.replacement_presentation.is_some());
            }
            let events = super::input_broker::RESIZES.lock();
            assert_eq!(events.len(), 4);
            for (i, e) in events.iter().enumerate() {
                assert_eq!(e.0, after[i].id);
                assert_eq!((e.2, e.3), (after[i].placement.width, after[i].placement.height));
                assert_eq!(e.1, broker.windows[i].resize_epoch);
            }
        }
        #[test]
        fn stale_quad_commit_changes_none_of_the_other_frames() {
            let before = setup_quad();
            move_one();
            assert!(quad_layout().is_none());
            assert_eq!(resize_quad_dock(before, 1921, 1081, 700, 400), Err(WindowBrokerError::InteractionDenied));
            assert!(super::input_broker::RESIZES.lock().is_empty());
            let broker = WINDOW_BROKER.lock();
            for i in 1..4 { assert_eq!(broker.windows[i].placement, before[i].placement); }
        }
        #[test]
        fn quad_clamps_to_minimum_and_rejects_duplicate_quadrants() {
            let before = setup_quad();
            resize_quad_dock(before, 1921, 1081, 0, u32::MAX).unwrap();
            let after = quad_layout().unwrap();
            assert_eq!((after[0].placement.width, after[0].placement.height), (32, 1049));
            assert_eq!(after[3].placement.height, 32);
            let mut duplicate = after.to_vec();
            duplicate.push(after[0]);
            assert!(quad_dock_layout(&duplicate, 1921, 1081).is_none());
        }
        pub(crate) fn setup_for_input()''')
    source = source.replace('    mod input_broker {\n        use super::*;', '''
    mod input_broker {
        use super::*;
        pub(super) static RESIZES: crate::TestMutex<Vec<(WindowId, u64, u32, u32)>> = crate::TestMutex::new(Vec::new());
        pub(super) fn enqueue_window_resize(_: WindowOwner, id: WindowId, epoch: u64,
            _: u32, _: u32, width: u32, height: u32) {
            RESIZES.lock().push((id, epoch, width, height));
        }
''')
    start = input_source.index('        // The shared handle takes precedence')
    end = input_source.index('        let hit = snapped_window.or_else', start)
    routing = input_source[start:end]
    escape = block(input_source, 'if self.quad_resize.is_some()\n            && event.kind')
    code = r'''
mod graphics { pub mod primitives {
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub struct Rgba8 { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }
    impl Rgba8 { pub const fn new(r:u8,g:u8,b:u8,a:u8)->Self { Self {r,g,b,a} } }
} }
mod r { pub mod keyboard {
    pub const KEYBOARD_OUTPUT_KIND_KEY: u8 = 1;
    pub const KEYBOARD_KEY_ESCAPE: u8 = 27;
    pub const KEYBOARD_OUTPUT_FLAG_PRESS: u8 = 1;
} }
#[macro_export] macro_rules! log_warn { (target: $target:literal; $($arg:tt)*) => { let _ = format_args!($($arg)*); }; }
'''
    # Insert inside ui4 before its closing brace; render/routing share real broker state.
    inner = r'''
    mod context_menu { pub(super) fn dismiss_for_source(_: u32) {} }
    mod quad_input {
        use super::*;
        type Ui4CursorSource = u32;
        const PRIMARY_BUTTON_MASK: u32 = 1;
        const SECONDARY_BUTTON_MASK: u32 = 2;
        @TYPES@
        fn current_quad_layout(_: u32, _: u32) -> Option<[WindowSnapshot; 4]> {
            super::window_broker::quad_layout()
        }
        struct Route {
            source: u32, x: u32, y: u32, buttons_down: u32, visible_after_motion: bool,
            absorb_select: bool, color: crate::graphics::primitives::Rgba8,
        }
        impl Route { fn clear_window_interaction(&mut self) {} }
        struct Key { kind: u8, key_code: u8, flags: u8 }
        struct Input { quad_resize: Option<QuadResizeGesture>, cursors: Vec<Route>,
            forwarded: usize, consumed_keys: Vec<Key> }
        impl Input {
            fn new() -> Self {
                Self { quad_resize: None, cursors: (0..2).map(|source| Route {
                    source, x: 0, y: 0, buttons_down: 0, visible_after_motion: false,
                    absorb_select: false, color: crate::graphics::primitives::Rgba8::new(source as u8, 2, 3, 255),
                }).collect(), forwarded: 0, consumed_keys: Vec::new() }
            }
            fn pointer(&mut self, index: usize, raw_x: u32, raw_y: u32, buttons_down: u32) {
                let source = self.cursors[index].source;
                let pressed = buttons_down & !self.cursors[index].buttons_down;
                let (width, height) = (1921, 1081);
                @ROUTING@
                self.cursors[index].buttons_down = buttons_down;
                self.forwarded += 1;
            }
            fn escape(&mut self) {
                let event = Key { kind: 1, key_code: 27, flags: 1 };
                @ESCAPE@
                self.forwarded += 1;
            }
        }
        #[test]
        fn quad_click_preview_owner_lock_commit_and_cancellation() {
            super::window_broker::setup_quad();
            let mut input = Input::new();
            input.pointer(0, 960, 540, 2); // right click cannot select
            assert!(input.quad_resize.is_none());
            input.pointer(0, 960, 540, 0);
            input.pointer(0, 960, 540, 1);
            assert_eq!(input.quad_resize.unwrap().source, 0);
            input.pointer(0, 700, 400, 0); // release keeps it selected and follows
            assert_eq!((input.quad_resize.unwrap().x, input.quad_resize.unwrap().y), (700, 400));
            assert_eq!(super::window_broker::quad_layout().unwrap()[0].placement.width, 960);
            input.pointer(1, 700, 400, 1); // competing cursor cannot take or commit
            assert_eq!(input.quad_resize.unwrap().source, 0);
            assert!(super::input_broker::RESIZES.lock().is_empty());
            input.pointer(0, 700, 400, 1); // owner placing click
            assert!(input.quad_resize.is_none());
            assert_eq!(super::input_broker::RESIZES.lock().len(), 4);
            assert!(input.cursors[0].absorb_select);
            input.pointer(0, 700, 400, 0);
            input.pointer(0, 700, 400, 1);
            input.pointer(0, 500, 300, 0);
            input.pointer(0, 500, 300, 2); // cancel, no resize
            assert!(input.quad_resize.is_none());
            assert_eq!(super::input_broker::RESIZES.lock().len(), 4);
            input.pointer(0, 700, 400, 0);
            input.pointer(0, 700, 400, 1);
            input.escape();
            assert!(input.quad_resize.is_none());
            assert_eq!(input.consumed_keys.len(), 1);
            assert_eq!(super::input_broker::RESIZES.lock().len(), 4);
        }
        #[test]
        fn individual_move_invalidates_selected_handle() {
            super::window_broker::setup_quad();
            let mut input = Input::new();
            input.pointer(0, 960, 540, 1);
            super::window_broker::move_one();
            input.pointer(0, 700, 400, 0);
            assert!(input.quad_resize.is_none());
            assert!(super::input_broker::RESIZES.lock().is_empty());
        }
        @RASTER@
    }
'''
    types = '\n'.join(item('src/ui4/input_broker.rs', n) for n in
                      ['QuadResizeGesture', 'QuadResizeVisual', 'quad_handle_contains', 'quad_gesture_valid'])
    raster = r'''
        #[derive(Clone)]
        struct Rect { x: u32, y: u32, width: u32, height: u32, color: crate::graphics::primitives::Rgba8 }
        type Slot4Rects = Vec<Rect>;
        const SOFTWARE_CURSOR_LONG_PX: u32 = 27;
        const SOFTWARE_CURSOR_STROKE_PX: u32 = 5;
        fn push_overlay_rect(rects: &mut Slot4Rects, x:u32,y:u32,width:u32,height:u32,
            color: crate::graphics::primitives::Rgba8) {
            if width > 0 && height > 0 { rects.push(Rect {x,y,width,height,color}); }
        }
        @RASTER_FUNCTION@
        #[test]
        fn hollow_large_handle_has_two_pixel_edges_and_one_pixel_guides() {
            let color = crate::graphics::primitives::Rgba8::new(255, 55, 255, 128);
            let mut rects = Vec::new();
            let mut handle = QuadResizeVisual { x:100,y:100,color,selected:false };
            push_quad_resize_handle(&mut rects, handle, 201, 201);
            let mut pixels = vec![0u8; 201*201];
            for rect in &rects {
                assert_eq!(rect.color, color);
                for y in rect.y..rect.y+rect.height { for x in rect.x..rect.x+rect.width {
                    assert!(x < 201 && y < 201);
                    pixels[(y*201+x) as usize] += 1;
                } }
            }
            assert!(pixels.iter().all(|n| *n <= 1), "alpha must never double blend");
            assert_eq!(pixels[100*201+100], 0, "hollow center");
            assert_eq!(pixels[60*201+100], 1);
            assert_eq!(pixels[61*201+100], 1);
            assert_eq!(pixels[62*201+100], 0);
            assert_eq!(pixels[100*201+60], 1);
            assert_eq!(pixels[100*201+61], 1);
            assert_eq!(pixels[100*201+62], 0);
            rects.clear(); handle.selected = true;
            push_quad_resize_handle(&mut rects, handle, 201, 201);
            assert_eq!((rects[0].x, rects[0].width, rects[0].height), (0,201,1));
            assert_eq!((rects[1].y, rects[1].width), (0,1));
            for (x,y) in [(0,0),(200,0),(0,200),(200,200)] {
                rects.clear(); handle.x=x; handle.y=y;
                push_quad_resize_handle(&mut rects, handle, 201, 201);
                assert!(rects.iter().all(|r| r.x+r.width <= 201 && r.y+r.height <= 201));
            }
        }
'''.replace('@RASTER_FUNCTION@', item('src/ui4/slot4_service.rs', 'push_quad_resize_handle').replace(
        'super::input_broker::QuadResizeVisual', 'QuadResizeVisual'))
    inner = inner.replace('@TYPES@', types).replace('@ROUTING@', routing).replace('@ESCAPE@', escape).replace('@RASTER@', raster)
    return source.rsplit('}', 1)[0] + inner + '}\n' + code


def main():
    with tempfile.TemporaryDirectory(prefix='trueos-ui4-quad-') as temporary:
        source = Path(temporary) / 'tests.rs'
        binary = Path(temporary) / 'tests'
        source.write_text(harness_source())
        subprocess.run(['rustc', '--edition=2024', '--test', str(source), '-o', str(binary)], check=True)
        subprocess.run([str(binary), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
