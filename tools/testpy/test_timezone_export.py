#!/usr/bin/env python3
"""Exercise production timezone policy and Blueprint exports without zoneinfo.

The generated TZ rules are also parsed by the host's POSIX TZ implementation
at representative instants and the exact modern Berlin transition boundaries.
"""
from datetime import datetime, timezone
from pathlib import Path
import os
import subprocess
import tempfile
import time

from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]
CASES = [
    ("2026-01-15T12:00:00", 3600, False, "CET"),
    ("2026-07-15T12:00:00", 7200, True, "CEST"),
    ("2026-03-29T00:59:59", 3600, False, "CET"),
    ("2026-03-29T01:00:00", 7200, True, "CEST"),
    ("2026-10-25T00:59:59", 7200, True, "CEST"),
    ("2026-10-25T01:00:00", 3600, False, "CET"),
]


def harness():
    cases = [(int(datetime.fromisoformat(date).replace(tzinfo=timezone.utc).timestamp()),
              offset, dst, abbreviation) for date, offset, dst, abbreviation in CASES]
    source = r'''
extern crate alloc;
use std::collections::BTreeMap;
mod limine {pub fn executable_cmdline()->Option<&'static str>{None}}
#[path="@ROOT@/src/locale.rs"]mod locale;
mod net {
 pub fn primary_device_index()->usize{0}pub fn device_count()->usize{0}
 pub mod adapter {
  pub fn get_hostname()->String{"test".into()}
  pub fn ipv4_at(_:usize)->Option<[u8;4]>{None}
  pub fn primary_ipv4_router_snapshot()->Option<[u8;4]>{None}
 }
}
@BUILD_HOST_ENV@
const CASES:&[(u64,i32,bool,&str)]=&[@CASES@];
#[test]fn berlin_exports_portable_rules_and_matches_civil_time_at_dst_boundaries(){
 assert!(locale::set_timezone_from_network("Europe/Berlin"));
 assert_eq!(locale::current_timezone_name(),"Europe/Berlin");
 let rule=locale::current_posix_timezone();assert_eq!(rule,"CET-1CEST,M3.5.0/2,M10.5.0/3");
 let vars=build_host_env();assert_eq!(vars["TZ"],rule);assert_eq!(vars["TRUEOS_TIMEZONE"],"Europe/Berlin");
 assert_eq!(locale::env_var("TZ"),Some(rule));assert_eq!(locale::env_var("TRUEOS_TIMEZONE"),Some("Europe/Berlin"));
 for &(utc,offset,dst,name)in CASES {
  assert_eq!(locale::utc_offset_seconds(utc),offset);assert_eq!(locale::timezone_is_dst(utc),dst);
  assert_eq!(locale::timezone_abbreviation(utc),name);assert_eq!(locale::local_unix_time_seconds(utc),utc+offset as u64);
 }
 assert!(locale::set_timezone_from_network(rule));assert_eq!(locale::current_timezone_name(),"Europe/Berlin");
 assert!(!locale::set_timezone_from_network("unsupported-zone"));assert_eq!(locale::current_posix_timezone(),rule);
 println!("EXPORTED_BERLIN_TZ={rule}");
}
#[test]fn berlin_policy_handles_years_beyond_i32_without_truncation(){
 assert!(locale::set_timezone_from_network("Europe/Berlin"));
 // Gregorian weekdays and leap dates repeat every 400 years. Shift the
 // independently known 2026 boundary fixtures to year 2,147,485,226: the
 // civil year exceeds i32, while its C tm_year (year - 1900) still fits.
 let shift=5_368_708u64*146_097*86_400;
 for &(utc,offset,dst,name)in CASES {
  let future=utc+shift;
  assert_eq!(locale::utc_offset_seconds(future),offset);
  assert_eq!(locale::timezone_is_dst(future),dst);
  assert_eq!(locale::timezone_abbreviation(future),name);
  assert_eq!(locale::local_unix_time_seconds(future),future+offset as u64);
 }
}
#[test]fn utc_rule_roundtrips_and_preserves_iana_identity(){
 assert!(locale::set_timezone_from_network("UTC0"));
 assert_eq!(locale::current_timezone_name(),"UTC");assert_eq!(locale::current_posix_timezone(),"UTC0");
 let vars=build_host_env();assert_eq!(vars["TZ"],"UTC0");assert_eq!(vars["TRUEOS_TIMEZONE"],"UTC");
 assert_eq!(locale::env_var("TZ"),Some("UTC0"));assert_eq!(locale::env_var("TRUEOS_TIMEZONE"),Some("UTC"));
 for &(utc,_,_,_)in CASES {assert_eq!(locale::utc_offset_seconds(utc),0);assert!(!locale::timezone_is_dst(utc));assert_eq!(locale::timezone_abbreviation(utc),"UTC");assert_eq!(locale::local_unix_time_seconds(utc),utc);}
 println!("EXPORTED_UTC_TZ={}",locale::current_posix_timezone());
}
'''.replace('@ROOT@', str(ROOT)).replace('@BUILD_HOST_ENV@', function(ROOT / 'src/hv/blueprint/blueprint.rs', 'build_host_env')).replace(
        '@CASES@', ','.join(f'({utc},{offset},{str(dst).lower()},"{name}")' for utc, offset, dst, name in cases))
    return source


def main():
    cases = [(int(datetime.fromisoformat(date).replace(tzinfo=timezone.utc).timestamp()),
              offset, dst, abbreviation) for date, offset, dst, abbreviation in CASES]
    source = harness()
    with tempfile.TemporaryDirectory(prefix='trueos-timezone-') as directory:
        path = Path(directory)
        lib = path / 'libtrueos_locale.rlib'
        subprocess.run(['rustc', '--edition=2024', '--crate-name', 'trueos_locale', '--crate-type=rlib', str(ROOT / 'crates/trueos-locale/src/lib.rs'), '-o', str(lib)], check=True)
        (path / 'test.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', '-Awarnings', str(path / 'test.rs'), '--extern', f'trueos_locale={lib}', '-o', str(path / 'test')], check=True)
        result = subprocess.run([str(path / 'test'), '--test-threads=1', '--nocapture'], check=True, text=True, capture_output=True)
        print(result.stdout)
        exported = {}
        for line in result.stdout.splitlines():
            for zone in ('BERLIN', 'UTC'):
                marker = f'EXPORTED_{zone}_TZ='
                if marker in line:
                    exported[zone] = line.split(marker, 1)[1]
        previous = os.environ.get('TZ')
        try:
            for zone, rule in exported.items():
                os.environ['TZ'] = rule
                time.tzset()
                for utc, offset, dst, name in cases:
                    local = time.localtime(utc)
                    expected = (offset, dst, name) if zone == 'BERLIN' else (0, False, 'UTC')
                    actual = (local.tm_gmtoff, bool(local.tm_isdst), local.tm_zone)
                    assert actual == expected, (zone, utc, actual, expected)
            assert set(exported) == {'BERLIN', 'UTC'}
            print('POSIX consumer: UTC and Berlin rules match all six instants')
        finally:
            if previous is None:
                os.environ.pop('TZ', None)
            else:
                os.environ['TZ'] = previous
            time.tzset()


if __name__ == '__main__':
    main()
