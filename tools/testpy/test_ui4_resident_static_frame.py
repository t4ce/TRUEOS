#!/usr/bin/env python3
"""Exercise production direct admission for resident one-shot glyph frames."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item


source = '''#![allow(dead_code)]
#[derive(Clone, Copy)] enum FrameContent { BlueprintScene, RenderScene3d, FontScene2d }
#[derive(Clone, Copy)] enum FrameCadence { Immutable, Dirty, Streaming }
#[derive(Clone, Copy)] enum FrameBuffering { Single, Double, Triple }
mod compositor {
use super::FrameContent;
'''
source += item('src/ui4/compositor_service.rs', 'resident_release_direct_contract')
source += '''
#[test] fn immutable_boxdraw_surface_has_direct_admission() {
    assert!(resident_release_direct_contract(FrameContent::BlueprintScene,
        super::FrameCadence::Immutable, super::FrameBuffering::Single));
}
#[test] fn streaming_contract_and_unsupported_plans_are_preserved() {
    use super::{FrameCadence as C, FrameBuffering as B};
    for content in [FrameContent::BlueprintScene, FrameContent::RenderScene3d] {
        assert!(resident_release_direct_contract(content, C::Streaming, B::Triple));
        assert!(!resident_release_direct_contract(content, C::Streaming, B::Single));
        assert!(!resident_release_direct_contract(content, C::Dirty, B::Double));
    }
}
}
'''
with tempfile.TemporaryDirectory(prefix='ui4-resident-static-') as directory:
    path = Path(directory) / 'tests.rs'
    path.write_text(source)
    binary = Path(directory) / 'tests'
    subprocess.run(['rustc', '--edition=2024', '--test', str(path), '-o', str(binary)], check=True)
    subprocess.run([str(binary)], check=True)
