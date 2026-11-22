#!/usr/bin/env python3
"""Exercise native overlay admission and explicit and implicit opaque copies."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_vgpu_batch_overlay import block

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / 'src/ui4/blueprint_ui4.rs').read_text()
    constants = '\n'.join(re.findall(
        r'^(?:pub\(crate\) )?const (?:MAX_SPRITE_QUADS|UI4_SPRITE_BATCH_TIMEOUT_NS|SPRITE_QUAD_FLAG_(?:BCS0_COPY|SRC_OVER)|'
        r'ERROR_INVALID|ERROR_NOT_FOUND|ERROR_STATE|ERROR_UI4|ERROR_BUSY):[^;]+;',
        source, re.M))
    harness = '''#![allow(dead_code)]
use std::sync::Mutex;
type WindowOwner = u32;
struct SpriteSceneUpload { expected: usize, quads: Vec<()> }
struct BlueprintSceneSurface {
    write_lease: Option<()>, sprite_clear_rgba: Option<u32>,
    pending_render_release: Option<u64>, gpu_submission_unretired: bool,
    sprite_scene_upload: Option<SpriteSceneUpload>,
}
struct Surfaces(Mutex<Vec<BlueprintSceneSurface>>);
impl Surfaces { fn lock(&self) -> std::sync::MutexGuard<'_, Vec<BlueprintSceneSurface>> { self.0.lock().unwrap() } }
static SURFACES: Surfaces = Surfaces(Mutex::new(Vec::new()));
fn surface_mut(s: &mut [BlueprintSceneSurface], _: WindowOwner, _: u32) -> Option<&mut BlueprintSceneSurface> { s.first_mut() }
'''
    harness += constants + '\n' + block(source, 'fn sprite_scene_clear_rgba(')
    harness += '\n' + block(source, 'pub(crate) fn begin_sprite_scene(')
    harness += '''
#[test]
fn fresh_gpu_and_retired_native_frames_admit_sprites_but_cpu_frames_do_not() {
    for (clear, release, lease, busy, expected) in [
        (Some(0), None, Some(()), false, 0),
        (None, Some(42), Some(()), false, 0),
        (None, None, Some(()), false, ERROR_STATE),
        (None, Some(42), None, false, ERROR_STATE),
        (None, Some(42), Some(()), true, ERROR_BUSY),
    ] {
        *SURFACES.lock() = vec![BlueprintSceneSurface {
            write_lease: lease, sprite_clear_rgba: clear, pending_render_release: release,
            gpu_submission_unretired: busy, sprite_scene_upload: None,
        }];
        assert_eq!(begin_sprite_scene(0, 0, 1), expected);
        let surfaces = SURFACES.lock();
        assert_eq!(surfaces[0].sprite_scene_upload.is_some(), expected == 0);
        assert_eq!(surfaces[0].pending_render_release, release);
        if expected == 0 { assert!(sprite_scene_clear_rgba(&surfaces[0]).is_some()); }
    }
    assert_eq!(begin_sprite_scene(0, 0, MAX_SPRITE_QUADS + 1), ERROR_INVALID);
}
'''
    finish_scene = block(source, 'pub(crate) fn finish_sprite_scene(')
    opaque_copy = block(finish_scene, 'if !render_overlay\n        && let [quad]')
    worklists = (ROOT / 'src/intel/gpgpu/types/worklists.rs').read_text()
    copy_flags = '\n'.join(re.findall(
        r'^pub\(crate\) const ALPHA_BLEND_WORKLIST_FLAG_(?:COPY|SRC_OVER|TINT_ALPHA|TINT_RGB):[^;]+;',
        worklists, re.M))
    harness += '''
#[macro_export]
macro_rules! log_important { ($($args:tt)*) => {} }
#[macro_export]
macro_rules! log_trace { ($($args:tt)*) => {} }

mod chronos {
    pub fn monotonic_nanos() -> u64 { 0 }
}

mod intel {
    use std::cell::Cell;

    #[derive(Clone, Copy)]
    pub struct GucBcs0RgbaSurface {
        pub phys: u64, pub gpu: u64, pub bytes: usize, pub width: u32, pub height: u32,
        pub pitch_bytes: u32,
    }

    pub struct GucBcs0RgbaCopy {
        pub source: GucBcs0RgbaSurface, pub source_x: u32, pub source_y: u32,
        pub destination_x: u32, pub destination_y: u32, pub width: u32, pub height: u32,
    }

    #[derive(Clone, Copy, Debug)]
    pub enum GucBcs0CopySubmitError { Unavailable, Busy, InvalidRequest, SubmitFailed }

    thread_local! {
        pub static QUEUE_RESULT: Cell<Option<Result<(), GucBcs0CopySubmitError>>> =
            const { Cell::new(None) };
    }

    pub fn queue_guc_bcs0_rgba_copies(
        destination: GucBcs0RgbaSurface, copies: &[GucBcs0RgbaCopy],
    ) -> Result<(), GucBcs0CopySubmitError> {
        let [copy] = copies else { panic!("opaque viewport needs exactly one copy") };
        assert_eq!((copy.destination_x, copy.destination_y), (0, 0));
        assert_eq!((copy.width, copy.height), (destination.width, destination.height));
        QUEUE_RESULT.with(|result| result.take().unwrap())
    }

    pub mod gpgpu {
        pub enum Ui4SpriteSceneCompletion { Complete { release: u64 }, Pending, Failed }

        pub fn poll_ui4_bcs0_sprite_copy(
            _: (), _: super::GucBcs0RgbaSurface,
        ) -> Ui4SpriteSceneCompletion {
            Ui4SpriteSceneCompletion::Complete { release: 42 }
        }
    }
}

mod opaque_viewport {
    use super::{
        ERROR_BUSY, ERROR_UI4, SPRITE_QUAD_FLAG_BCS0_COPY, SPRITE_QUAD_FLAG_SRC_OVER,
        UI4_SPRITE_BATCH_TIMEOUT_NS,
    };
    use crate::intel::{GucBcs0CopySubmitError, GucBcs0RgbaSurface as GpgpuRgba8Surface, QUEUE_RESULT};
    use crate::intel::gpgpu::Ui4SpriteSceneCompletion;
    use std::sync::atomic::Ordering;
'''
    harness += '\n'.join([
        copy_flags,
        '#[derive(Clone, Copy)]\n' + block(source, 'pub struct TrueosUi4SpriteQuad {'),
        block(worklists, 'pub(crate) struct GpgpuAlphaBlendWorklistDesc {'),
        block(source, 'enum AlphaRectConversion {'),
        block(source, 'fn rounded_sprite_coordinate('),
        block(source, 'fn alpha_rect_descriptor('),
    ])
    harness += '''
    mod libm {
        pub fn roundf(value: f32) -> f32 { value.round() }
    }

    struct RetainedSprite {
        opaque: bool,
        surface: GpgpuRgba8Surface,
    }

    enum BlueprintSpriteSource {
        Uploaded(RetainedSprite),
    }

    struct Upload {
        quads: Vec<TrueosUi4SpriteQuad>,
    }

    struct Frame {
        destination: GpgpuRgba8Surface,
        sprites: [(u32, BlueprintSpriteSource); 1],
        write_lease: Option<()>,
        pending_gpu_release: Option<u64>,
        sprite_clear_rgba: Option<u32>,
        quarantine_reason: Option<&'static str>,
    }

    impl Frame {
        fn leased() -> Self {
            Self::leased_at(2560, 1440)
        }

        fn leased_at(width: u32, height: u32) -> Self {
            let destination = GpgpuRgba8Surface {
                phys: 0x10000000, gpu: 0x12000000, bytes: (width * height * 4) as usize,
                width, height, pitch_bytes: width * 4,
            };
            let source = GpgpuRgba8Surface {
                phys: 0x20000000, gpu: 0x38001000, ..destination
            };
            Self {
                destination,
                sprites: [(1, BlueprintSpriteSource::Uploaded(RetainedSprite {
                    opaque: true, surface: source,
                }))],
                write_lease: Some(()),
                pending_gpu_release: None,
                sprite_clear_rgba: Some(u32::MAX),
                quarantine_reason: None,
            }
        }

        fn viewport_quad(&self, explicit: bool) -> TrueosUi4SpriteQuad {
            let width = self.destination.width as f32;
            let height = self.destination.height as f32;
            TrueosUi4SpriteQuad {
                sprite_id: 1,
                c0_x: 0.0, c0_y: 0.0, c0_u: 0.0, c0_v: 0.0,
                c1_x: width, c1_y: 0.0, c1_u: 1.0, c1_v: 0.0,
                c2_x: width, c2_y: height, c2_u: 1.0, c2_v: 1.0,
                c3_x: 0.0, c3_y: height, c3_u: 0.0, c3_v: 1.0,
                color_rgba: u32::MAX,
                flags: if explicit { SPRITE_QUAD_FLAG_BCS0_COPY } else { 0 },
            }
        }
    }

    fn cancel_blueprint_sprite_frame_without_live_gpu(surface: &mut Frame) {
        assert_eq!(surface.write_lease.take(), Some(()));
        surface.sprite_clear_rgba = None;
    }

    fn quarantine_blueprint_sprite_submission(
        surface: &mut Frame, _: u32, _: u32, _: (), _: usize, _: usize, reason: &'static str,
    ) {
        surface.quarantine_reason = Some(reason);
    }

    const GENERAL_SPRITE_PATH: i32 = 1;

    fn draw(
        surface: &mut Frame, explicit: bool, result: Result<(), GucBcs0CopySubmitError>,
    ) -> i32 {
        draw_quad(surface, surface.viewport_quad(explicit), result)
    }

    #[allow(unused_variables)] // The trace macros ignore hardware addresses and errors.
    fn draw_quad(
        surface: &mut Frame, quad: TrueosUi4SpriteQuad, result: Result<(), GucBcs0CopySubmitError>,
    ) -> i32 {
        let owner = 0;
        let window_id = 0;
        let lease = surface.write_lease.unwrap();
        let destination = surface.destination;
        let upload = Upload { quads: vec![quad] };
        let render_overlay = false;
        QUEUE_RESULT.with(|slot| slot.set(Some(result)));
'''
    harness += opaque_copy + '''
        GENERAL_SPRITE_PATH
    }

    #[test]
    fn busy_opaque_copy_cancels_lease_and_asks_for_a_new_frame() {
        for explicit in [false, true] {
            let mut frame = Frame::leased();
            assert_eq!(draw(&mut frame, explicit, Err(GucBcs0CopySubmitError::Busy)), ERROR_BUSY);
            assert_eq!(frame.write_lease, None);
            assert_eq!(frame.pending_gpu_release, None);
            assert_eq!(frame.quarantine_reason, None);
        }
    }

    #[test]
    fn opaque_copy_falls_back_on_rejection_only_for_implicit_bcs0() {
        for error in [GucBcs0CopySubmitError::Unavailable, GucBcs0CopySubmitError::InvalidRequest] {
            let mut frame = Frame::leased();
            assert_eq!(draw(&mut frame, false, Err(error)), GENERAL_SPRITE_PATH);
            assert_eq!(frame.write_lease, Some(()));
            assert_eq!(frame.quarantine_reason, None);

            let mut frame = Frame::leased();
            assert_eq!(draw(&mut frame, true, Err(error)), ERROR_UI4);
            assert_eq!(frame.write_lease, None);
            assert_eq!(frame.pending_gpu_release, None);
            assert_eq!(frame.quarantine_reason, None);
        }
    }

    #[test]
    fn uncertain_opaque_copy_preserves_lease_in_quarantine() {
        for explicit in [false, true] {
            let mut frame = Frame::leased();
            assert_eq!(draw(&mut frame, explicit, Err(GucBcs0CopySubmitError::SubmitFailed)), ERROR_UI4);
            assert_eq!(frame.write_lease, Some(()));
            assert_eq!(frame.quarantine_reason, Some("bcs0-copy-submit-uncertain"));
        }
    }

    #[test]
    fn retired_opaque_copy_keeps_the_publish_lease_and_release() {
        for (explicit, width, height) in [(false, 2560, 1440), (true, 2560, 1440), (true, 3840, 2160)] {
            let mut frame = Frame::leased_at(width, height);
            assert_eq!(draw(&mut frame, explicit, Ok(())), 0);
            assert_eq!(frame.write_lease, Some(()));
            assert_eq!(frame.pending_gpu_release, Some(42));
            assert_eq!(frame.sprite_clear_rgba, None);
            assert_eq!(frame.quarantine_reason, None);
        }
    }

    #[test]
    fn noncanonical_explicit_copy_keeps_general_bcs0_validation() {
        let mut frame = Frame::leased();
        let mut quad = frame.viewport_quad(true);
        quad.c0_x = 0.005;
        quad.c3_x = 0.005;
        // Near-integer coordinates can round to a full implicit copy, but
        // explicit requests outside the canonical shape retain their path.
        assert_eq!(draw_quad(&mut frame, quad, Ok(())), GENERAL_SPRITE_PATH);
        assert_eq!(frame.pending_gpu_release, None);
        quad.flags = 0;
        assert_eq!(draw_quad(&mut frame, quad, Ok(())), 0);
    }
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-sprite-overlay-') as temporary:
        path = Path(temporary)
        (path / 'test.rs').write_text(harness)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'test.rs'), '-o', str(path / 'tests')], check=True)
        subprocess.run([str(path / 'tests')], check=True)


if __name__ == '__main__':
    main()
