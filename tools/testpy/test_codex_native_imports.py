#!/usr/bin/env python3
"""Exercise new Codex native imports using the unchanged production functions.

The freestanding arithmetic and C signatures come from the real kernel source. The
math dependency is compiled from the Cargo.lock-selected libm source for host
ABI tests; these tests do not execute the native kernel.
"""

from pathlib import Path
import os
import re
import subprocess
import tempfile
import tomllib

from test_thread_control_abi import function


ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    source = (ROOT / "src/blueprint_shims.rs").read_text()
    functions = "\n".join(
        function(source, name)
        for name in ["abs", "ldexp"]
    )
    harness = r'''
#![allow(non_camel_case_types)]
use core::ffi::{c_double, c_int, c_void};
use core::sync::atomic::{AtomicI32, Ordering};
static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);
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
fn highlighting_ldexp_uses_the_mixed_float_integer_c_abi() {
    let call: unsafe extern "C" fn(c_double, c_int) -> c_double = ldexp;
    // Calling through the same untyped address shape as the import resolver
    // checks both the floating-point return register and integer exponent.
    let address = call as *const () as usize;
    let resolved: unsafe extern "C" fn(c_double, c_int) -> c_double =
        unsafe { core::mem::transmute(address) };
    TRUEOS_ERRNO.store(321, Ordering::Relaxed);
    for (value, exponent, expected) in [(0.75, 3, 6.0), (-3.0, -2, -0.75), (1.0, 10, 1024.0)] {
        assert_eq!(unsafe { resolved(value, exponent) }, expected);
    }
    assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed), 321);
}

#[test]
fn highlighting_ldexp_preserves_signed_zero_and_subnormal_values() {
    let call: unsafe extern "C" fn(c_double, c_int) -> c_double = ldexp;
    assert_eq!(unsafe { call(-0.0, 100) }.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(unsafe { call(0.0, -100) }.to_bits(), 0);
    assert_eq!(unsafe { call(1.0, -1074) }.to_bits(), 1);
    assert_eq!(unsafe { call(-1.0, -1074) }.to_bits(), (1_u64 << 63) | 1);
    assert_eq!(unsafe { call(-1.0, -1075) }.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(unsafe { call(f64::from_bits(1), 1074) }, 1.0);
}

#[test]
fn highlighting_ldexp_handles_overflow_and_nonfinite_values() {
    let call: unsafe extern "C" fn(c_double, c_int) -> c_double = ldexp;
    assert_eq!(unsafe { call(1.0, 1024) }, f64::INFINITY);
    assert_eq!(unsafe { call(-1.0, 1024) }, f64::NEG_INFINITY);
    assert_eq!(unsafe { call(f64::INFINITY, i32::MIN) }, f64::INFINITY);
    assert_eq!(unsafe { call(f64::NEG_INFINITY, i32::MAX) }, f64::NEG_INFINITY);
    assert!(unsafe { call(f64::NAN, 10) }.is_nan());
}

'''.replace("@FUNCTIONS@", functions)
    with tempfile.TemporaryDirectory(prefix="trueos-codex-native-imports-") as temporary:
        directory = Path(temporary)
        fixture = directory / "contracts.rs"
        fixture.write_text(harness)
        lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
        version = next(package["version"] for package in lock["package"] if package["name"] == "libm")
        cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
        math_sources = sorted((cargo_home / "registry/src").glob(f"*/libm-{version}/src/lib.rs"))
        if len(math_sources) != 1:
            raise RuntimeError(f"Expected one cached libm {version} source, found {math_sources}")
        math_library = directory / "liblibm.rlib"
        subprocess.run(
            ["rustc", "--edition=2021", "--crate-name=libm", "--crate-type=rlib",
             "--cap-lints=allow", str(math_sources[0]), "-o", str(math_library)],
            check=True,
        )
        executable = directory / "contracts"
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(fixture),
             "--extern", f"libm={math_library}", "-o", str(executable)],
            check=True,
        )
        subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
