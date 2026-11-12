#!/usr/bin/env python3
"""Exercise the production AVC source import cache without GPU hardware."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item


ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code)]

mod intel {
    pub mod ppgtt {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct PpgttRange {
            pub gpu: u64,
            pub phys: u64,
            pub bytes: usize,
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Policy { WriteBack, Uncached }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct Mapping {
            pub range: PpgttRange,
            pub policy: Policy,
        }

        #[derive(Default)]
        pub struct SparsePpgtt {
            pub calls: Vec<Mapping>,
            pub active: Option<Mapping>,
            pub fail_next: bool,
        }

        impl SparsePpgtt {
            pub fn map_range(&mut self, range: PpgttRange) -> Option<()> {
                self.map(range, Policy::WriteBack)
            }

            pub fn map_scanout_range(&mut self, range: PpgttRange) -> Option<()> {
                self.map(range, Policy::Uncached)
            }

            fn map(&mut self, range: PpgttRange, policy: Policy) -> Option<()> {
                let mapping = Mapping { range, policy };
                self.calls.push(mapping);
                if self.fail_next {
                    self.fail_next = false;
                    // Production mapping rollback can remove the prior
                    // aperture's leaves after a replacement attempt fails.
                    self.active = None;
                    return None;
                }
                self.active = Some(mapping);
                Some(())
            }
        }
    }
}

@PRODUCTION@

struct ProbeBacking {
    ppgtt: intel::ppgtt::SparsePpgtt,
    source_mapping: Option<(AvcRawSurfaceFormat, u64)>,
}

fn backing() -> ProbeBacking {
    ProbeBacking { ppgtt: Default::default(), source_mapping: None }
}

use intel::ppgtt::{Mapping, Policy, PpgttRange};

fn xyuv_mapping(phys: u64) -> Mapping {
    Mapping {
        range: PpgttRange { gpu: 0x5000_0000, phys, bytes: 14_745_600 },
        policy: Policy::Uncached,
    }
}

#[test]
fn continuous_wd_frames_import_once_and_preserve_uncached_policy() {
    let mut encoder = backing();
    let phys = 0x1_2000_0000;
    for _ in 0..330 {
        assert_eq!(encoder.ensure_source_mapping(AvcRawSurfaceFormat::Xyuv8888, phys), Some(()));
    }
    assert_eq!(encoder.ppgtt.calls, vec![xyuv_mapping(phys)]);
    assert_eq!(encoder.ppgtt.active, Some(xyuv_mapping(phys)));
}

#[test]
fn wd_allocation_replacement_imports_the_new_physical_pages() {
    let mut encoder = backing();
    let old_phys = 0x1_2000_0000;
    let new_phys = 0x1_3000_0000;
    for phys in [old_phys, new_phys, new_phys, old_phys] {
        assert_eq!(encoder.ensure_source_mapping(AvcRawSurfaceFormat::Xyuv8888, phys), Some(()));
    }
    assert_eq!(encoder.ppgtt.calls, vec![
        xyuv_mapping(old_phys), xyuv_mapping(new_phys), xyuv_mapping(old_phys),
    ]);
    assert_eq!(encoder.ppgtt.active, Some(xyuv_mapping(old_phys)));
}

#[test]
fn format_is_part_of_the_import_identity_and_selects_size_address_and_pat() {
    let mut encoder = backing();
    let phys = 0x1_2000_0000;
    for format in [AvcRawSurfaceFormat::Nv12, AvcRawSurfaceFormat::Nv12,
                   AvcRawSurfaceFormat::Xyuv8888, AvcRawSurfaceFormat::Nv12] {
        assert_eq!(encoder.ensure_source_mapping(format, phys), Some(()));
    }
    let nv12 = Mapping {
        range: PpgttRange { gpu: SOURCE_GPU, phys, bytes: 5_529_600 },
        policy: Policy::WriteBack,
    };
    assert_ne!(SOURCE_GPU, XYUV8888_SOURCE_GPU);
    assert_eq!(encoder.ppgtt.calls, vec![nv12, xyuv_mapping(phys), nv12]);
}

#[test]
fn failed_initial_import_is_retried_instead_of_cached() {
    let mut encoder = backing();
    let phys = 0x1_2000_0000;
    encoder.ppgtt.fail_next = true;
    assert_eq!(encoder.ensure_source_mapping(AvcRawSurfaceFormat::Xyuv8888, phys), None);
    assert_eq!(encoder.ensure_source_mapping(AvcRawSurfaceFormat::Xyuv8888, phys), Some(()));
    assert_eq!(encoder.ppgtt.calls, vec![xyuv_mapping(phys), xyuv_mapping(phys)]);
    assert_eq!(encoder.ppgtt.active, Some(xyuv_mapping(phys)));
}

#[test]
fn failed_replacement_does_not_reuse_a_rolled_back_previous_mapping() {
    let mut encoder = backing();
    let old_phys = 0x1_2000_0000;
    let new_phys = 0x1_3000_0000;
    assert_eq!(encoder.ensure_source_mapping(AvcRawSurfaceFormat::Xyuv8888, old_phys), Some(()));
    encoder.ppgtt.fail_next = true;
    assert_eq!(encoder.ensure_source_mapping(AvcRawSurfaceFormat::Xyuv8888, new_phys), None);
    assert_eq!(encoder.ppgtt.active, None);
    assert_eq!(encoder.ensure_source_mapping(AvcRawSurfaceFormat::Xyuv8888, old_phys), Some(()));
    assert_eq!(encoder.ppgtt.calls, vec![
        xyuv_mapping(old_phys), xyuv_mapping(new_phys), xyuv_mapping(old_phys),
    ]);
    assert_eq!(encoder.ppgtt.active, Some(xyuv_mapping(old_phys)));
}

#[test]
fn separate_encoder_backing_never_inherits_another_ppgtt_import() {
    let mut first = backing();
    let mut second = backing();
    let phys = 0x1_2000_0000;
    assert_eq!(first.ensure_source_mapping(AvcRawSurfaceFormat::Xyuv8888, phys), Some(()));
    assert_eq!(second.ensure_source_mapping(AvcRawSurfaceFormat::Xyuv8888, phys), Some(()));
    assert_eq!(first.ppgtt.calls, vec![xyuv_mapping(phys)]);
    assert_eq!(second.ppgtt.calls, vec![xyuv_mapping(phys)]);
}
'''


def main():
    production = (ROOT / 'src/intel/media/avc_encode_probe.rs').read_text()
    constants = []
    for name in ('ARENA_PPGTT_BASE', 'SOURCE_OFFSET', 'FRAME_WIDTH', 'FRAME_HEIGHT',
                 'SOURCE_BYTES', 'XYUV8888_SOURCE_BYTES', 'SOURCE_GPU', 'XYUV8888_SOURCE_GPU'):
        constants.append(re.search(
            rf'^(?:pub\(crate\) )?const {name}:[^\n]+;', production, re.MULTILINE
        )[0])
    definitions = '\n'.join(constants)
    definitions += '\n#[derive(Copy, Clone, Debug, Eq, PartialEq)]\n'
    definitions += '\n'.join(item(production, signature) for signature in (
        'enum AvcRawSurfaceFormat', 'impl AvcRawSurfaceFormat', 'impl ProbeBacking',
    ))
    channel = re.search(
        r'channel\s*=\s*"([^"]+)"', (ROOT / 'rust-toolchain.toml').read_text()
    )[1]
    with tempfile.TemporaryDirectory(prefix='trueos-avc-source-mapping-') as directory:
        directory = Path(directory)
        source = directory / 'tests.rs'
        executable = directory / 'tests'
        source.write_text(HARNESS.replace('@PRODUCTION@', definitions))
        subprocess.run([
            'rustc', '+' + channel, '--edition=2024', '--test', str(source),
            '-o', str(executable),
        ], check=True)
        subprocess.run([str(executable)], check=True)


if __name__ == '__main__':
    main()
