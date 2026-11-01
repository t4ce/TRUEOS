#!/usr/bin/env python3
"""Exercise actual sprite admission after a native fill/border consumed its clear."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_vgpu_batch_overlay import block

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / 'src/ui4/blueprint_ui4.rs').read_text()
    constants = '\n'.join(re.findall(
        r'^(?:pub\(crate\) )?const (?:MAX_SPRITE_QUADS|ERROR_INVALID|ERROR_NOT_FOUND|ERROR_STATE|ERROR_BUSY):[^;]+;', source, re.M))
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
    with tempfile.TemporaryDirectory(prefix='trueos-sprite-overlay-') as temporary:
        path = Path(temporary)
        (path / 'test.rs').write_text(harness)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'test.rs'), '-o', str(path / 'tests')], check=True)
        subprocess.run([str(path / 'tests')], check=True)


if __name__ == '__main__':
    main()
