#!/usr/bin/env python3
"""Exercise new Codex native imports using the unchanged production functions.

Only descriptor-table ownership is modeled. Arithmetic, pointer handling, errno
selection and the C function signatures come from the real kernel source.
"""

from pathlib import Path
import re
import subprocess
import tempfile

from test_thread_control_abi import function


ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    source = (ROOT / "src/std_abi_shim.rs").read_text()
    functions = "\n".join(
        function(source, name)
        for name in ["abs", "reject_fd_timestamp_update", "futimens", "futimes"]
    )
    constants = "\n".join(
        re.search(rf"^(?:pub\(crate\) )?const {name}:.*$", source, re.MULTILINE).group(0)
        for name in ["TRUEOS_ENOSYS", "TRUEOS_EBADF"]
    )
    harness = r'''
#![allow(non_camel_case_types)]
use core::ffi::{c_int, c_void};
use core::sync::atomic::{AtomicI32, Ordering};
static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);
struct Descriptors(i32);
impl Descriptors {
    fn lock(&self) -> &Self { self }
    fn get(&self, fd: c_int) -> Option<()> { (fd == self.0).then_some(()) }
}
static OPEN_FILES: Descriptors = Descriptors(10);
static SOCKET_FDS: Descriptors = Descriptors(20);
@CONSTANTS@
@FUNCTIONS@

#[test]
fn quickjs_abs_preserves_integer_magnitude_without_touching_errno() {
    let call: unsafe extern "C" fn(c_int) -> c_int = abs;
    TRUEOS_ERRNO.store(123, Ordering::Relaxed);
    for (value, expected) in [(-1,1),(-123456,123456),(0,0),(1,1),(i32::MAX,i32::MAX)] {
        assert_eq!(unsafe { call(value) }, expected);
        assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed), 123);
    }
}

#[test]
fn valid_descriptor_timestamp_requests_report_unsupported_without_reading_times() {
    let calls: [unsafe extern "C" fn(c_int, *const c_void) -> c_int; 2] = [futimens, futimes];
    for call in calls {
        for fd in [0,1,2,10,20] {
            for pointer in [core::ptr::null(), core::ptr::without_provenance(1)] {
                TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                assert_eq!(unsafe { call(fd, pointer) }, -1);
                assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed), TRUEOS_ENOSYS);
            }
        }
    }
}

#[test]
fn unknown_descriptors_report_bad_fd_in_both_timestamp_abis() {
    let calls: [unsafe extern "C" fn(c_int, *const c_void) -> c_int; 2] = [futimens, futimes];
    for call in calls {
        for fd in [-1,3,11,21,i32::MAX] {
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            assert_eq!(unsafe { call(fd, core::ptr::null()) }, -1);
            assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed), TRUEOS_EBADF);
        }
    }
}
'''.replace("@CONSTANTS@", constants).replace("@FUNCTIONS@", functions)
    with tempfile.TemporaryDirectory(prefix="trueos-codex-native-imports-") as temporary:
        directory = Path(temporary)
        fixture = directory / "contracts.rs"
        fixture.write_text(harness)
        executable = directory / "contracts"
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(fixture), "-o", str(executable)],
            check=True,
        )
        subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
