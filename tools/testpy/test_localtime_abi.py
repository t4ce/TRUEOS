#!/usr/bin/env python3
"""Exercise production localtime_r and UTC clock APIs with real locale policy.

Only ABI pointer translation and the clock source are mocked. Civil conversion,
timezone selection, UTC clock exports, and struct layout come from production.
"""
from datetime import datetime, timedelta, timezone
from pathlib import Path
import re
import subprocess
import tempfile

from test_posix_renameat import function
from test_timezone_export import CASES, ROOT, harness as timezone_harness

SHIM = ROOT / "src/std_abi_shim.rs"


def harness():
    source = SHIM.read_text()
    structures = source[source.index("#[repr(C)]\n#[derive(Clone, Copy)]\nstruct TrueosTimeval {"):
                        source.index("static GAI_STRERROR_SYSTEM:")]
    constants = '\n'.join(re.findall(
        r'^(?:static TRUEOS_(?:UTC|CET|CEST)_TZ:.*|(?:pub\(crate\) )?const TRUEOS_(?:EINVAL|EOVERFLOW):.*)$',
        source, re.MULTILINE))
    definitions = '\n'.join(function(SHIM, name) for name in (
        'time', 'gettimeofday', 'localtime_r', 'unix_seconds_to_utc_tm',
        'civil_from_days', 'day_of_year', 'is_leap_year'))
    return timezone_harness().split('const CASES:')[0] + r'''
use std::{ffi::{c_char,c_int,c_long,c_void,CStr},ptr,slice,
          sync::atomic::{AtomicI32,Ordering}};
static TRUEOS_ERRNO:AtomicI32=AtomicI32::new(0);
fn trueos_time_unix_seconds()->u64 {1773576000}
fn abi_read_struct<T:Copy>(p:*const T)->Option<T>{
 if p.is_null(){None}else{Some(unsafe{p.read_unaligned()})}
}
fn copy_to_abi_out(p:*mut u8,b:&[u8])->bool{
 if p.is_null(){return false}
 unsafe{ptr::copy_nonoverlapping(b.as_ptr(),p,b.len())};true
}
''' + structures + constants + '\n' + definitions


def expected_fields(utc, offset, dst, name):
    wall = datetime.fromtimestamp(utc, timezone.utc) + timedelta(seconds=offset)
    return f'''({wall.year-1900},{wall.month-1},{wall.day},{wall.hour},{wall.minute},{wall.second},
               {(wall.weekday()+1)%7},{wall.timetuple().tm_yday-1},{int(dst)},{offset},"{name}")'''


TESTS = r'''
fn convert(t:i64)->TrueosTm {
 let mut out=std::mem::MaybeUninit::<TrueosTm>::uninit();
 let p=unsafe{localtime_r(&t,out.as_mut_ptr().cast())};
 assert_eq!(p,out.as_mut_ptr().cast());assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),0);
 unsafe{out.assume_init()}
}
fn check(t:i64,want:(i32,i32,i32,i32,i32,i32,i32,i32,i32,i64,&str)){
 let got=convert(t);
 let name=unsafe{CStr::from_ptr(got.tm_zone)}.to_str().unwrap();
 assert_eq!((got.tm_year,got.tm_mon,got.tm_mday,got.tm_hour,got.tm_min,got.tm_sec,
             got.tm_wday,got.tm_yday,got.tm_isdst,got.tm_gmtoff,name),want,"UTC timestamp {t}");
}
#[test]fn berlin_winter_summer_and_dst_transition_fields(){
 assert!(locale::set_timezone_from_network("Europe/Berlin"));
 @BERLIN_CASES@
}
#[test]fn local_midnight_year_and_leap_day_rollovers(){
 assert!(locale::set_timezone_from_network("Europe/Berlin"));
 @ROLLOVER_CASES@
}
#[test]fn utc_and_negative_epoch_fields(){
 assert!(locale::set_timezone_from_network("UTC"));
 @UTC_CASES@
}
#[test]fn null_arguments_fail_and_success_clears_errno(){
 assert!(locale::set_timezone_from_network("UTC"));
 let t=0i64;let mut out=std::mem::MaybeUninit::<TrueosTm>::uninit();
 assert!(unsafe{localtime_r(ptr::null(),out.as_mut_ptr().cast())}.is_null());
 assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EINVAL);
 assert!(unsafe{localtime_r(&t,ptr::null_mut())}.is_null());
 assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EINVAL);
 assert_eq!(convert(0).tm_year,70);
}
#[test]fn unrepresentable_dates_fail_without_writing_output(){
 for zone in ["UTC","Europe/Berlin"]{
  assert!(locale::set_timezone_from_network(zone));
  for t in [i64::MIN,i64::MAX]{
   let mut bytes=[0xa5u8;std::mem::size_of::<TrueosTm>()];
   assert!(unsafe{localtime_r(&t,bytes.as_mut_ptr().cast())}.is_null());
   assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EOVERFLOW);
   assert!(bytes.iter().all(|&b|b==0xa5));
  }
 }
}
#[test]fn realtime_apis_keep_utc_while_localtime_applies_zone(){
 assert!(locale::set_timezone_from_network("Europe/Berlin"));
 let now=trueos_time_unix_seconds() as i64;let mut stored=-1;
 assert_eq!(unsafe{time(&mut stored)},now);assert_eq!(stored,now);
 assert_eq!(unsafe{time(ptr::null_mut())},now);
 let mut tv=TrueosTimeval{tv_sec:-1,tv_usec:-1};
 assert_eq!(unsafe{gettimeofday((&mut tv as *mut TrueosTimeval).cast(),ptr::null_mut())},0);
 assert_eq!((tv.tv_sec,tv.tv_usec),(now,0));
 let utc=unix_seconds_to_utc_tm(now).unwrap();let local=convert(now);
 assert_eq!(local.tm_hour,utc.tm_hour+1);assert_eq!(local.tm_gmtoff,3600);
}
'''


def main():
    berlin = [(int(datetime.fromisoformat(date).replace(tzinfo=timezone.utc).timestamp()),
               offset, dst, name) for date, offset, dst, name in CASES]
    rollovers = [(int(datetime.fromisoformat(date).replace(tzinfo=timezone.utc).timestamp()),
                  3600, False, 'CET') for date in (
                      '2025-12-31T23:30:00', '2024-02-28T23:30:00', '1969-12-31T23:59:59')]
    utc = [(timestamp, 0, False, 'UTC') for timestamp in (-1, 0, berlin[0][0], berlin[1][0])]
    tests = TESTS
    for marker, cases in [('BERLIN', berlin), ('ROLLOVER', rollovers), ('UTC', utc)]:
        checks = '\n'.join(f'check({t},{expected_fields(t,offset,dst,name)});'
                           for t, offset, dst, name in cases)
        tests = tests.replace(f'@{marker}_CASES@', checks)
    with tempfile.TemporaryDirectory(prefix='trueos-localtime-') as directory:
        path = Path(directory)
        lib = path / 'libtrueos_locale.rlib'
        subprocess.run(['rustc', '--edition=2024', '--crate-name', 'trueos_locale',
                        '--crate-type=rlib', str(ROOT / 'crates/trueos-locale/src/lib.rs'),
                        '-o', str(lib)], check=True)
        (path / 'test.rs').write_text(harness() + tests)
        subprocess.run(['rustc', '--edition=2024', '--test', '-Awarnings',
                        str(path / 'test.rs'), '--extern', f'trueos_locale={lib}',
                        '-o', str(path / 'test')], check=True)
        subprocess.run([str(path / 'test'), '--test-threads=1'], check=True, timeout=30)


if __name__ == '__main__':
    main()
