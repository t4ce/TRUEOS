#!/usr/bin/env python3
"""Exercise production colour parsers and shader math on the host."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, item
from test_rdp_pipeline import block


def main():
    avc = ROOT / 'src/intel/media/h264_cmd.rs'
    rust = f'''#![allow(dead_code, unfulfilled_lint_expectations)]
extern crate alloc;
mod avc {{ include!(r"{avc}");
#[test]
fn vui_prefix_variants() {{
    fn bits(s: &str) -> Vec<u8> {{
        let mut out = vec![0; s.len().div_ceil(8)];
        for (i, b) in s.bytes().enumerate() {{ out[i/8] |= (b-b'0') << (7-i%8); }}
        out
    }}
    for (text, expected) in [
        ("000", (false, 2)), // No aspect, overscan or video signal.
        ("00110110", (true, 2)), // Full range without colour_description.
        ("00110101000000010000000100000001", (false, 1)),
        // Extended SAR 1:1, overscan, full-range BT.709.
        (concat!("1", "11111111", "0000000000000001", "0000000000000001", "11", "1", "101", "1", "1", "00000001", "00000001", "00000001"), (true, 1)),
    ] {{
        let data = bits(text);
        assert_eq!(parse_vui_colour_description(&mut H264BitReader::new(&data)).unwrap(), expected);
    }}
    assert!(parse_vui_colour_description(&mut H264BitReader::new(&[0xff])).is_err());
    assert!(parse_vui_colour_description(&mut H264BitReader::new(&[0x37])).is_err());
}}
}}
'''
    vid = 'src/intel/media/hw_vid.rs'
    for name in ['H264ColourDescription', 'mp4_read_u16', 'mp4_fourcc', 'mp4_parse_colour']:
        rust += item(vid, name)
    rust += block(vid, 'impl H264ColourDescription {')
    rust += r'''
#[test]
fn colr_range_precedence_and_truncation() {
    let nclx = b"nclx\0\x01\0\x01\0\x01\x80";
    let colour = mp4_parse_colour(nclx).unwrap().unwrap();
    assert_eq!(colour.resolve(false, 6), (true, 1));
    for end in 0..nclx.len() { assert!(mp4_parse_colour(&nclx[..end]).is_err()); }
    let nclc = mp4_parse_colour(b"nclc\0\x01\0\x01\0\x06").unwrap().unwrap();
    assert_eq!(nclc.resolve(true, 1), (true, 6));
    assert_eq!(nclc.resolve(false, 1), (false, 6));
    let unspecified = mp4_parse_colour(b"nclc\0\x02\0\x02\0\x02").unwrap().unwrap();
    assert_eq!(unspecified.resolve(true, 1), (true, 1));
    assert!(mp4_parse_colour(b"profignored").unwrap().is_none());
}
'''
    kernel = (ROOT / 'crates/trueos-shader/gpgpu/kernels/ui4_nv12_tile64_to_rgba8_frame.clcpp').read_text()
    helpers = kernel[kernel.index('inline uint ui4_clamped_yuv_channel'):kernel.index('__attribute__')]
    cpp = '#include <algorithm>\n#include <cmath>\n#include <cassert>\nusing uint=unsigned int; using uchar=unsigned char; using std::clamp; using std::max;\n'
    cpp += helpers
    cpp += r'''
int main() {
    for (bool full : {false, true}) for (bool hd : {false, true}) {
        const uint mode = (full ? 256u : 0u) | (hd ? 1u : 6u);
        const double kr = hd ? .2126 : .299, kb = hd ? .0722 : .114;
        const double kg = 1 - kr - kb;
        for (int y = full ? 0 : 16; y <= (full ? 255 : 235); ++y) {
            for (int u : {16, 64, 128, 192, 240}) for (int v : {16, 64, 128, 192, 240}) {
                const uint actual = ui4_yuv_to_rgba(y, u, v, mode);
                const double Y = full ? y : (y - 16) * 255.0 / 219;
                const double U = (u - 128) * (full ? 1 : 255.0 / 224);
                const double V = (v - 128) * (full ? 1 : 255.0 / 224);
                const double expected[3] = {Y + 2*(1-kr)*V,
                    Y - 2*kb*(1-kb)/kg*U - 2*kr*(1-kr)/kg*V, Y + 2*(1-kb)*U};
                for (int c = 0; c < 3; ++c) {
                    const int value = (actual >> (8*c)) & 255;
                    assert(std::abs(value - clamp((int)std::lround(expected[c]), 0, 255)) <= 1);
                }
                assert((actual >> 24) == 255);
            }
        }
        assert(ui4_yuv_to_rgba(full ? 0 : 16, 128, 128, mode) == 0xff000000u);
        assert(ui4_yuv_to_rgba(full ? 255 : 235, 128, 128, mode) == 0xffffffffu);
    }
    // The legacy unspecified matrix is still BT.601.
    assert(ui4_yuv_to_rgba(90, 54, 200, 2) == ui4_yuv_to_rgba(90, 54, 200, 6));
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-video-colour-') as directory:
        directory = Path(directory)
        (directory/'probe.rs').write_text(rust)
        (directory/'probe.cpp').write_text(cpp)
        subprocess.run(['rustc', '--edition=2024', '--test', str(directory/'probe.rs'), '-o', str(directory/'rust-test')], check=True)
        subprocess.run([str(directory/'rust-test')], check=True)
        subprocess.run(['c++', '-std=c++17', '-O2', str(directory/'probe.cpp'), '-o', str(directory/'cpp-test')], check=True)
        subprocess.run([str(directory/'cpp-test')], check=True)
    print('BT.601/709 full/limited shader checks passed (<=1 code value error)')


if __name__ == '__main__':
    main()
