#!/usr/bin/env python3
"""Execute the win parser/request tests and check the live Shell2 tool schema."""
import json
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import ROOT, item


def main():
    doc = ROOT / 'tools/trueos-doc'
    schema = json.loads(subprocess.check_output([str(doc), 'command', 'win']))['data']['parameters']
    assert set(schema['properties']) == {'action'}
    assert schema['properties']['action']['enum'] == ['start', 'status', 'stop']
    retired = subprocess.run([str(doc), 'command', 'cpp'], capture_output=True)
    assert retired.returncode != 0
    registry = (ROOT / 'src/shell2/shell2_cmd_registry.rs').read_text()
    assert 'cmds::cpp' not in registry and 'name: "cpp"' not in registry
    code = '#![allow(dead_code, non_camel_case_types)]\n' + item('src/shell2/cmds/win.rs', 'WinAction')
    code += item('src/shell2/cmds/win.rs', 'parse_action')
    code += item('src/shell2/cmds/win.rs', 'tests')
    code += item('src/ui4/gpgpu_preview_consumer.rs', 'request_win_demo_start')
    code += HARNESS
    with tempfile.TemporaryDirectory(prefix='trueos-win-') as temp:
        path = Path(temp) / 'tests.rs'; path.write_text(code)
        binary = Path(temp) / 'tests'
        subprocess.run(['rustc', '--edition=2024', '--test', str(path), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)
    print('Live Shell2 schema: win present, cpp retired')


HARNESS = r'''
#[derive(Debug,PartialEq)]
enum GpgpuPreviewPreset { Static30 }
struct GpgpuPreviewConfig {preset:GpgpuPreviewPreset,duration_ms:u64,cadence_ms:u64,publish_every:u32}
#[derive(Debug,PartialEq)]
enum PreviewRunPolicy { WIN_DEMO }
const GPGPU_PREVIEW_DEFAULT_CADENCE_MS:u64=33;
const GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY:u32=1;
fn request_gpgpu_preview_start_with_policy(c:GpgpuPreviewConfig,p:PreviewRunPolicy)->Result<u64,&'static str> {
    assert_eq!(c.preset,GpgpuPreviewPreset::Static30);assert_eq!(c.duration_ms,0);
    assert_eq!(c.publish_every,1);assert_eq!(p,PreviewRunPolicy::WIN_DEMO);Ok(42)
}
#[test]
fn win_requests_only_the_immutable_thirty_window_preset() { assert_eq!(request_win_demo_start(),Ok(42)); }
'''

if __name__ == '__main__':
    main()
