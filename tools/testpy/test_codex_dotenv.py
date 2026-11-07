#!/usr/bin/env python3
"""Exercise actual dotenv filtering; native environment admission is checked by test_native_process_state.py."""
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
mod dotenvy { pub enum Error { Io(std::io::Error), Parse } }
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
    valid.insert(1, Err(dotenvy::Error::Parse));
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

    // The production stream filter skips consumed invalid UTF-8 lines and parse errors.
    let invalid_data_key = "DOTENV_FIXTURE_AFTER_INVALID_DATA";
    unsafe { std::env::remove_var(invalid_data_key); }
    set_filtered(vec![
        Err(dotenvy::Error::Io(std::io::Error::from(std::io::ErrorKind::InvalidData))),
        Ok((invalid_data_key.into(), "retained".into())),
    ]);
    assert_eq!(value(invalid_data_key).as_deref(), Some("retained"));
    assert!(marks().is_empty());

    // A persistent read failure must stop polling rather than spinning or applying later rows.
    for kind in [std::io::ErrorKind::Interrupted, std::io::ErrorKind::WouldBlock,
                 std::io::ErrorKind::UnexpectedEof, std::io::ErrorKind::Other] {
        let before = "DOTENV_FIXTURE_BEFORE_IO_ERROR";
        let after = "DOTENV_FIXTURE_AFTER_IO_ERROR";
        unsafe { std::env::remove_var(before); std::env::remove_var(after); }
        let polls = std::cell::Cell::new(0);
        let stream = vec![
            Ok((before.into(), "retained".into())),
            Err(dotenvy::Error::Io(std::io::Error::from(kind))),
            Ok((after.into(), "must not apply".into())),
        ].into_iter().inspect(|_| polls.set(polls.get()+1));
        set_filtered(stream);
        assert_eq!(polls.get(), 2);
        assert_eq!(value(before).as_deref(), Some("retained"));
        assert!(value(after).is_none());
    }
    let polls = std::cell::Cell::new(0);
    set_filtered(std::iter::from_fn(|| {
        polls.set(polls.get()+1);
        assert!(polls.get() <= 1, "persistent read failure was polled repeatedly");
        Some(Err(dotenvy::Error::Io(std::io::Error::from(std::io::ErrorKind::Other))))
    }));
    assert_eq!(polls.get(),1);
    assert!(marks().is_empty());
    println!("production dotenv filter child contract passed trueos={}", cfg!(target_os = "trueos"));
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
    with tempfile.TemporaryDirectory(prefix="trueos-codex-dotenv-") as temporary:
        directory = Path(temporary)
        source = directory / "dotenv.rs"
        source.write_text(dotenv)
        for trueos in (False, True):
            binary = directory / ("trueos" if trueos else "host")
            flags = ["--cfg", 'target_os="trueos"', "-Aexplicit_builtin_cfgs_in_flags"] if trueos else []
            subprocess.run(["rustc", "--edition=2024", *flags, str(source), "-o", str(binary)], check=True, timeout=60)
            subprocess.run([str(binary)], check=True, timeout=30)


if __name__ == "__main__":
    main()
