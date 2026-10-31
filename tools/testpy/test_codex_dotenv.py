#!/usr/bin/env python3
"""Exercise actual dotenv filtering and kernel environment admission.

Dotenv changes the host environment only in separate single-threaded child
programs. The C ABI harness uses the unchanged production Environment module
and setter/getter bodies with controlled VM selection and guest pointer reads.
No kernel namespace policy or libc symbol is replaced.
"""
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
CODEX = Path(os.environ.get("TRUEOS_CODEX_ROOT", ROOT.parent / "codex"))
POLICY = CODEX / "codex-rs/core/src/session/startup_prewarm_policy.rs"


def function(source, name):
    declaration = re.search(
        rf'^(?:pub )?(?:unsafe )?(?:extern "C" )?fn {re.escape(name)}(?:<[^>]*>)?\(',
        source, re.MULTILINE,
    )
    if declaration is None:
        raise ValueError(f"production function missing: {name}")
    body = source.index("{", declaration.end())
    depth = 0
    for index in range(body, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                # Starting at the declaration omits export attributes, so the
                # host harness cannot interpose kernel setters on host libc.
                return source[declaration.start():index + 1]
    raise ValueError(f"production function closing brace missing: {name}")


DOTENV_HARNESS = r'''#![allow(dead_code)]
#[path = @POLICY@] mod policy;
mod dotenvy { pub struct Error; }
mod startup_timing {
    pub static MARKS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    pub fn mark(stage: &str) { MARKS.lock().unwrap().push(stage.to_owned()); }
}
@PREFIX@
@FILTER@
fn value(key: &str) -> Option<String> { std::env::var(key).ok() }
fn marks() -> Vec<String> { std::mem::take(&mut *startup_timing::MARKS.lock().unwrap()) }
fn rows(items: &[(&str, &str)]) -> Vec<Result<(String, String), dotenvy::Error>> {
    items.iter().map(|(key, value)| Ok((key.to_string(), value.to_string()))).collect()
}
fn main() {
    // This is a plain child program, with no test harness or background threads.
    let prewarm = policy::STARTUP_PREWARM_ENV;
    assert_eq!(prewarm, "CDX_STARTUP_PREWARM");
    let allowed = [prewarm, "TOKIO_WORKER_THREADS"];
    for key in allowed { unsafe { std::env::remove_var(key) }; }
    let mut valid = rows(&[(prewarm, "1"), ("TOKIO_WORKER_THREADS", "2")]);
    valid.insert(1, Err(dotenvy::Error));
    set_filtered(valid);
    assert_eq!(value(prewarm).as_deref(), Some("1"));
    assert_eq!(value("TOKIO_WORKER_THREADS").as_deref(), Some("2"));
    assert!(marks().is_empty());

    for key in ["CODEX_STARTUP_TEST_GUARD", "cOdEx_STARTUP_TEST_MIXED"] {
        unsafe { std::env::set_var(key, "outer-value") };
        set_filtered(rows(&[(key, "private-dotenv-value")]));
        assert_eq!(value(key).as_deref(), Some("outer-value"));
    }
    assert!(marks().is_empty());

    unsafe {
        std::env::set_var("TRUEOS_FS_SCOPE", "kernel-owned");
        std::env::remove_var("TRUEOS_CODEX_STARTUP_PREWARM");
        std::env::remove_var("TRUEOS_DOTENV_TEST_CREATED");
    }
    set_filtered(rows(&[
        ("TRUEOS_FS_SCOPE", "private-dotenv-value"),
        ("TRUEOS_CODEX_STARTUP_PREWARM", "private-dotenv-value"),
        ("TRUEOS_DOTENV_TEST_CREATED", "private-dotenv-value"),
        (prewarm, "0"),
    ]));
    assert_eq!(value(prewarm).as_deref(), Some("0"));
    if cfg!(target_os = "trueos") {
        assert_eq!(value("TRUEOS_FS_SCOPE").as_deref(), Some("kernel-owned"));
        assert!(value("TRUEOS_CODEX_STARTUP_PREWARM").is_none());
        assert!(value("TRUEOS_DOTENV_TEST_CREATED").is_none());
        // Three ignored keys produce one static record, with no key or value.
        assert_eq!(marks(), ["dotenv_reserved_trueos_key_ignored"]);
    } else {
        for key in ["TRUEOS_FS_SCOPE", "TRUEOS_CODEX_STARTUP_PREWARM", "TRUEOS_DOTENV_TEST_CREATED"] {
            assert_eq!(value(key).as_deref(), Some("private-dotenv-value"));
        }
        assert!(marks().is_empty());
    }
    // TRUEOS reserves exact uppercase bytes; these distinct keys remain mutable.
    for key in ["trueos_DOTENV_TEST", "TrUeOs_DOTENV_TEST", "TRUEOS", "TRUEOSX_DOTENV_TEST"] {
        set_filtered(rows(&[(key, "case-value")]));
        assert_eq!(value(key).as_deref(), Some("case-value"));
    }
    assert!(marks().is_empty());
    println!("production dotenv filter child contract passed trueos={}", cfg!(target_os = "trueos"));
}
'''


CABI_HARNESS = r'''#![allow(dead_code)]
extern crate alloc;
#[path = @ENVIRONMENT@]
mod process_environment;
mod cabi {
    use super::process_environment;
    use core::ffi::{c_char, c_int};
    use core::ptr;
    use core::sync::atomic::{AtomicI32, Ordering};
    use std::sync::Mutex;
    const TRUEOS_EINVAL: c_int = process_environment::EINVAL;
    const TRUEOS_ENOMEM: c_int = process_environment::ENOMEM;
    const TRUEOS_EFAULT: c_int = 14;
    pub static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);
    static ENVIRONMENT: Mutex<process_environment::Environment> =
        Mutex::new(process_environment::Environment::new());
    fn environment_vm_id() -> Option<u8> { Some(0) }
    fn with_environment<R>(vm_id: Option<u8>, f: impl FnOnce(&mut process_environment::Environment) -> R) -> Result<R, c_int> {
        assert_eq!(vm_id, Some(0));
        Ok(f(&mut ENVIRONMENT.lock().unwrap()))
    }
    // Only valid test-owned CStrings are passed; guest address mapping is out of scope.
    fn abi_read_bytes(pointer: *const u8, length: usize) -> Option<Vec<u8>> {
        if pointer.is_null() { return None; }
        Some(unsafe { core::slice::from_raw_parts(pointer, length) }.to_vec())
    }
@WRAPPERS@
    pub fn reset(with_old_key: bool) {
        let mut env = process_environment::Environment::new();
        let mut values = vec![(b"TRUEOS_FS_SCOPE".as_slice(), b"private".as_slice())];
        if with_old_key {
            values.push((b"TRUEOS_CODEX_STARTUP_PREWARM".as_slice(), b"0".as_slice()));
        }
        env.initialize(values.into_iter()).unwrap();
        *ENVIRONMENT.lock().unwrap() = env;
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    }
    pub fn snapshot() -> Vec<Vec<u8>> {
        let env = ENVIRONMENT.lock().unwrap();
        let mut pointer = env.environ;
        let mut out = Vec::new();
        while !unsafe { *pointer }.is_null() {
            out.push(unsafe { core::ffi::CStr::from_ptr(*pointer) }.to_bytes().to_vec());
            pointer = unsafe { pointer.add(1) };
        }
        out
    }
}
use core::sync::atomic::Ordering;
use std::ffi::{CStr, CString};
fn value(key: &CString) -> Option<Vec<u8>> {
    let pointer = unsafe { cabi::getenv(key.as_ptr()) };
    if pointer.is_null() { None } else { Some(unsafe { CStr::from_ptr(pointer) }.to_bytes().to_vec()) }
}
#[test]
fn candidate_overrides_are_mutable_at_the_actual_c_abi_boundary() {
    for name in [@PREWARM_KEY@, "TOKIO_WORKER_THREADS"] {
        cabi::reset(true);
        let key = CString::new(name).unwrap();
        let zero = CString::new("0").unwrap();
        let one = CString::new("1").unwrap();
        assert!(value(&key).is_none());
        assert_eq!(unsafe { cabi::setenv(key.as_ptr(), zero.as_ptr(), 1) }, 0);
        assert_eq!(value(&key).as_deref(), Some(b"0".as_slice()));
        assert!(cabi::snapshot().contains(&format!("{name}=0").into_bytes()));
        assert_eq!(unsafe { cabi::setenv(key.as_ptr(), one.as_ptr(), 0) }, 0);
        assert_eq!(value(&key).as_deref(), Some(b"0".as_slice()));
        assert_eq!(unsafe { cabi::setenv(key.as_ptr(), one.as_ptr(), 1) }, 0);
        assert_eq!(value(&key).as_deref(), Some(b"1".as_slice()));
        assert!(cabi::snapshot().contains(&format!("{name}=1").into_bytes()));
        assert_eq!(unsafe { cabi::unsetenv(key.as_ptr()) }, 0);
        assert!(value(&key).is_none());
        assert!(!cabi::snapshot().iter().any(|entry| entry.starts_with(format!("{name}=").as_bytes())));
    }
}
#[test]
fn reserved_startup_override_cannot_be_replaced_removed_or_created() {
    let key = CString::new("TRUEOS_CODEX_STARTUP_PREWARM").unwrap();
    let one = CString::new("1").unwrap();
    cabi::reset(true);
    let original = cabi::snapshot();
    assert_eq!(unsafe { cabi::setenv(key.as_ptr(), one.as_ptr(), 1) }, -1);
    assert_eq!(cabi::TRUEOS_ERRNO.load(Ordering::Relaxed), process_environment::EPERM);
    assert_eq!(value(&key).as_deref(), Some(b"0".as_slice()));
    assert_eq!(cabi::snapshot(), original);
    assert_eq!(unsafe { cabi::unsetenv(key.as_ptr()) }, -1);
    assert_eq!(cabi::TRUEOS_ERRNO.load(Ordering::Relaxed), process_environment::EPERM);
    assert_eq!(cabi::snapshot(), original);
    cabi::reset(false);
    let original = cabi::snapshot();
    assert_eq!(unsafe { cabi::setenv(key.as_ptr(), one.as_ptr(), 1) }, -1);
    assert_eq!(cabi::TRUEOS_ERRNO.load(Ordering::Relaxed), process_environment::EPERM);
    assert!(value(&key).is_none());
    assert_eq!(cabi::snapshot(), original);
}
#[test]
fn mutable_override_does_not_weaken_other_reserved_kernel_variables() {
    cabi::reset(true);
    let allowed = CString::new(@PREWARM_KEY@).unwrap();
    let reserved = CString::new("TRUEOS_FS_SCOPE").unwrap();
    let value_one = CString::new("1").unwrap();
    assert_eq!(unsafe { cabi::setenv(allowed.as_ptr(), value_one.as_ptr(), 1) }, 0);
    assert_eq!(unsafe { cabi::setenv(reserved.as_ptr(), value_one.as_ptr(), 1) }, -1);
    assert_eq!(cabi::TRUEOS_ERRNO.load(Ordering::Relaxed), process_environment::EPERM);
    assert_eq!(value(&reserved).as_deref(), Some(b"private".as_slice()));
    assert_eq!(unsafe { cabi::unsetenv(reserved.as_ptr()) }, -1);
    assert_eq!(cabi::TRUEOS_ERRNO.load(Ordering::Relaxed), process_environment::EPERM);
    assert_eq!(value(&reserved).as_deref(), Some(b"private".as_slice()));
}
'''


def main():
    arg0 = (CODEX / "codex-rs/arg0/src/lib.rs").read_text()
    prefix = re.search(r'^const ILLEGAL_ENV_VAR_PREFIX: &str = "[^"]+";', arg0, re.MULTILINE)
    if prefix is None:
        raise ValueError("missing production dotenv namespace prefix")
    policy = POLICY.read_text()
    prewarm = re.search(r'const STARTUP_PREWARM_ENV: &str = "([^"]+)";', policy)
    if prewarm is None:
        raise ValueError("missing production startup override key")
    dotenv = DOTENV_HARNESS.replace("@POLICY@", json.dumps(str(POLICY)))
    dotenv = dotenv.replace("@PREFIX@", prefix.group()).replace("@FILTER@", function(arg0, "set_filtered"))
    shim = (ROOT / "src/std_abi_shim.rs").read_text()
    wrappers = "\n\n".join(function(shim, name) for name in (
        "environment_cstr", "getenv", "setenv", "unsetenv",
    ))
    cabi = CABI_HARNESS.replace("@ENVIRONMENT@", json.dumps(str(ROOT / "src/process_environment.rs")))
    cabi = cabi.replace("@WRAPPERS@", wrappers).replace("@PREWARM_KEY@", json.dumps(prewarm[1]))
    with tempfile.TemporaryDirectory(prefix="trueos-codex-dotenv-") as temporary:
        directory = Path(temporary)
        source = directory / "dotenv.rs"
        source.write_text(dotenv)
        for trueos in (False, True):
            binary = directory / ("trueos" if trueos else "host")
            flags = ["--cfg", 'target_os="trueos"', "-Aexplicit_builtin_cfgs_in_flags"] if trueos else []
            subprocess.run(["rustc", "--edition=2024", *flags, str(source), "-o", str(binary)], check=True, timeout=60)
            subprocess.run([str(binary)], check=True, timeout=30)
        source = directory / "environment.rs"
        source.write_text(cabi)
        binary = directory / "environment"
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], check=True, timeout=60)
        subprocess.run([str(binary), "--test-threads=1"], check=True, timeout=30)


if __name__ == "__main__":
    main()
