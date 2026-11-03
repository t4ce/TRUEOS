#!/usr/bin/env python3
"""Exercise Codex's real browser helper, usage action, and SDK launch packet.

Compile the production code unchanged for both platform branches, using the
real URL parser. Only the native browser and kernel C ABI are fixtures; no
browser is launched and no network request or rig mutation is performed.
"""

import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
CODEX = Path(os.environ.get("TRUEOS_CODEX_ROOT", ROOT.parent / "codex"))


def item(source, needle):
    at = source.index(needle)
    start = source.rfind("\n", 0, at) + 1
    indent = re.match(r" *", source[start:]).group()
    brace = source.index("{", at)
    end = re.search(r"^" + indent + r"}\s*\n", source[brace:], re.M)
    assert end, needle
    return source[start:brace + end.end()]


FIXTURE = r'''#![allow(dead_code, unexpected_cfgs)]
extern crate self as trueos_v;
extern crate self as webbrowser;
extern crate self as codex_utils_process;
use std::sync::{Mutex, atomic::{AtomicI32, Ordering}};
static CALLS: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
static STATUS: AtomicI32 = AtomicI32::new(0);
#[path = @BROWSER@] mod browser;
pub use browser::open_browser;
fn reset(status: i32) { CALLS.lock().unwrap().clear(); STATUS.store(status, Ordering::SeqCst); }
pub fn open(target: &str) -> std::io::Result<()> {
    CALLS.lock().unwrap().push(format!("native\0{target}").into_bytes());
    let status = STATUS.load(Ordering::SeqCst);
    if status == 0 { Ok(()) } else { Err(std::io::Error::other(format!("browser error {status}"))) }
}
pub mod vshell {
    mod vcabi {
        pub unsafe fn trueos_cabi_blueprint_launch_script_v1(p: *const u8, n: usize) -> i32 {
            let payload = unsafe { std::slice::from_raw_parts(p, n) };
            crate::CALLS.lock().unwrap().push(payload.to_vec());
            crate::STATUS.load(std::sync::atomic::Ordering::SeqCst)
        }
    }
@DESTINATION@
@SDK@
}
mod app {
    #[derive(Default)] pub struct ChatWidget { pub errors: Vec<String> }
    impl ChatWidget { pub fn add_error_message(&mut self, e: String) { self.errors.push(e); } }
    #[derive(Default)] pub struct App { pub chat_widget: ChatWidget }
    impl App {
@USAGE_ACTION@
    }
}
#[cfg(target_os = "trueos")]
#[test]
fn usage_link_reaches_hull_in_a_new_shell_without_native_browser_detection() {
    reset(0);
    let mut app = app::App::default();
    app.open_url_in_browser("https://chatgpt.com/settings/usage".into());
    assert!(app.chat_widget.errors.is_empty());
    assert_eq!(*CALLS.lock().unwrap(), [b"hull\0open https://chatgpt.com/settings/usage\n\0sh3".to_vec()]);
}
#[cfg(target_os = "trueos")]
#[test]
fn invalid_target_never_reaches_the_launch_boundary() {
    reset(0);
    for url in ["https://example.com/\nopen https://other.example/", "https://example.com/\0", "file:///tmp/a"] {
        assert_eq!(open_browser(url).unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
    }
    assert!(CALLS.lock().unwrap().is_empty());
}
#[cfg(not(target_os = "trueos"))]
#[test]
fn other_platforms_keep_their_default_browser_and_url_handling() {
    reset(0);
    open_browser("mailto:person@example.com").unwrap();
    assert_eq!(*CALLS.lock().unwrap(), [b"native\0mailto:person@example.com".to_vec()]);
}
#[test]
fn usage_action_reports_a_launch_failure_instead_of_silent_success() {
    reset(-73);
    let mut app = app::App::default();
    app.open_url_in_browser("https://chatgpt.com/settings/usage".into());
    assert_eq!(app.chat_widget.errors.len(), 1);
    assert!(app.chat_widget.errors[0].starts_with("Failed to open browser for https://chatgpt.com/settings/usage:"));
    assert!(app.chat_widget.errors[0].contains("-73"));
    assert_eq!(CALLS.lock().unwrap().len(), 1);
}
'''


def main():
    sdk = (ROOT.parent / "TRUEOS-Blueprints/crates/trueos-v/src/vshell.rs").read_text()
    ui = (CODEX / "codex-rs/tui/src/app/history_ui.rs").read_text()
    text = FIXTURE.replace("@BROWSER@", json.dumps(str(CODEX / "codex-rs/utils/process/src/browser.rs")))
    text = text.replace("@DESTINATION@", item(sdk, "pub enum LaunchDestination"))
    text = text.replace("@SDK@", item(sdk, "pub fn launch_with_destination("))
    text = text.replace("@USAGE_ACTION@", item(ui, "pub(super) fn open_url_in_browser("))
    lock = tomllib.loads((CODEX / "codex-rs/Cargo.lock").read_text())
    url_version = next(p["version"] for p in lock["package"] if p["name"] == "url")
    results = []
    with tempfile.TemporaryDirectory(prefix="trueos-codex-hull-browser-") as temp:
        temp = Path(temp)
        (temp / "src").mkdir()
        (temp / "src/lib.rs").write_text(text)
        (temp / "Cargo.toml").write_text(
            '[package]\nname="codex-hull-browser-check"\nversion="0.0.0"\nedition="2024"\n'
            f'[workspace]\n[dependencies]\nurl="={url_version}"\n'
        )
        for trueos in (False, True):
            command = ["cargo", "test", "--lib", "--no-run", "--offline", "--message-format=json",
                       "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(ROOT / "tgt/host-tools/codex-hull-browser")]
            env = dict(os.environ)
            if trueos:
                flags = env.get("CARGO_ENCODED_RUSTFLAGS", "").split("\x1f") if env.get("CARGO_ENCODED_RUSTFLAGS") else env.get("RUSTFLAGS", "").split()
                env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags + ["--cfg", 'target_os="trueos"', "-Aexplicit_builtin_cfgs_in_flags"])
            build = subprocess.run(command, cwd=temp, env=env, capture_output=True, text=True)
            assert build.returncode == 0, build.stderr + build.stdout
            artifacts = [json.loads(l) for l in build.stdout.splitlines() if l.startswith("{")]
            binary = next(a["executable"] for a in artifacts if a.get("executable") and a.get("profile", {}).get("test"))
            run = subprocess.run([binary, "--test-threads=1"], capture_output=True, text=True)
            assert run.returncode == 0, run.stdout + run.stderr
            passed = int(re.search(r"test result: ok\. (\d+) passed", run.stdout)[1])
            results.append({"platform": "trueos_cfg" if trueos else "host", "passed": passed})
    print(json.dumps({"status": "PASS", "results": results}))


if __name__ == "__main__":
    main()
