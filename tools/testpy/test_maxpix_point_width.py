#!/usr/bin/env python3
"""Exercise the production Maxpix width validator and native SF packet field."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_indexed_fs_remove import extract

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / 'src/intel/render/pipeline.rs').read_text()
    constants = '\n'.join(re.search(r'^const ' + name + r':[^;]+;', source, re.M).group()
                          for name in ('SF_POINT_WIDTH_MASK', 'MESA_SF_DW3', 'RESIDENT_POINT_WIDTH_U8_3'))
    production = '\n'.join(extract(source, name) for name in
                           ('maxpix_point_width_raw', 'resident_point_width_raw', 'mesa_sf_dw3'))
    tests = r'''
#[test]
fn complete_native_domain_preserves_other_sf_settings() {
    assert_eq!(maxpix_point_width_raw(0.0), Some(8));
    for raw in 1..=2047 {
        assert_eq!(maxpix_point_width_raw(raw as f32), Some(raw));
        let packet = mesa_sf_dw3(true, raw);
        assert_eq!(packet & SF_POINT_WIDTH_MASK, raw);
        assert_eq!(packet & !SF_POINT_WIDTH_MASK, MESA_SF_DW3 & !SF_POINT_WIDTH_MASK);
        assert_eq!(mesa_sf_dw3(false, raw), MESA_SF_DW3);
    }
}
#[test]
fn invalid_inputs_are_rejected_without_wrapping_or_truncating() {
    for value in [-1.0, 0.5, 8.5, 2048.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(maxpix_point_width_raw(value), None);
    }
}
#[test]
fn existing_integer_width_callers_keep_their_encoding() {
    assert_eq!(resident_point_width_raw(0), 32);
    for pixels in 1..=255 {
        assert_eq!(mesa_sf_dw3(true, resident_point_width_raw(pixels)) & SF_POINT_WIDTH_MASK, pixels * 8);
    }
}
'''
    with tempfile.TemporaryDirectory(prefix='maxpix-point-width-') as directory:
        rust = Path(directory) / 'test.rs'
        binary = Path(directory) / 'test'
        rust.write_text(constants + '\n' + production + tests)
        subprocess.run(['rustc', '--edition=2024', '--test', str(rust), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
