#!/usr/bin/env python3
"""Exercise live logging policies with the real, sparse log-area IDs."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    harness = r'''
#![allow(dead_code)]
extern crate self as log_os_core;
#[path = "@FLAGS@"] mod real_flags;
pub use real_flags::*;

mod flags {
    // Different defaults expose shifted seeding as well as out-of-bounds IDs.
    pub const fn build_area_log_policy(area: super::LogArea) -> super::LogLevelPolicy {
        let levels = super::LogLevelFilter::ALL;
        super::LogLevelPolicy::up(levels[area.index() as usize % levels.len()])
    }
}
#[path = "@POLICY@"] mod policy;

#[test]
fn sparse_area_ids_keep_defaults_edits_and_profiles_independent() {
    for area in LogArea::ALL {
        assert_eq!(policy::area_log_policy(area), flags::build_area_log_policy(area),
            "incorrect initial policy for {area:?}");
    }
    for changed in LogArea::ALL {
        let previous = policy::area_log_policy(changed);
        let custom = LogLevelPolicy::only(LogLevelSet::NONE);
        policy::set_area_log_policy(changed, custom);
        for area in LogArea::ALL {
            let expected = if area == changed { custom } else { flags::build_area_log_policy(area) };
            assert_eq!(policy::area_log_policy(area), expected, "editing {changed:?} affected {area:?}");
        }
        policy::set_area_log_policy(changed, previous);
    }
    for (index, profile) in policy::Profile::ALL.into_iter().enumerate() {
        policy::apply_profile(index);
        assert_eq!(policy::profile_name(), profile.name());
        for area in LogArea::ALL {
            assert_eq!(policy::area_log_policy(area), profile.policy(area),
                "incorrect {} policy for {area:?}", profile.name());
        }
    }
    policy::apply_profile(0);
    for area in LogArea::ALL {
        assert_eq!(policy::area_log_policy(area), flags::build_area_log_policy(area));
    }
}
'''.replace('@FLAGS@', str(ROOT / 'vendor/log-os/src/flags.rs')).replace(
        '@POLICY@', str(ROOT / 'src/log_policy.rs'))
    with tempfile.TemporaryDirectory(prefix='trueos-log-policy-') as work:
        source = Path(work) / 'test.rs'
        binary = Path(work) / 'test'
        source.write_text(harness)
        subprocess.run(['rustup', 'run', 'nightly-2026-07-10', 'rustc',
                        '--edition=2024', '--test', str(source), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
