#!/usr/bin/env python3
"""Exercise the production OS action dispatcher without rebooting or writing disks."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = (ROOT / "src/shell3/os.rs").read_text()
dispatch = source[source.index("fn dispatch_admin_action("):]
harness = r'''extern crate alloc;
extern crate self as trueos_executor;
pub struct Spawner;
type MatrixTarget = ();
static EVENTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
fn event(s: &str) { EVENTS.lock().unwrap().push(s.into()); }
fn print_matrix_target_line(_: &MatrixTarget, s: &str) { event(s); }
mod efi { pub mod acpi { pub mod facp {
    pub fn enter_named_sleep_state(n: u8) -> Result<(), ()> { assert_eq!(n, 5); crate::event("shutdown"); Ok(()) }
    pub fn reset_system() -> Result<(), ()> { crate::event("reboot"); Ok(()) }
}}}
mod disc { pub mod install { pub mod pxeproc {
    pub enum InstallMode { Pxeproc }
}}}
mod shell3 {
    pub mod table {
        pub fn parse_disc_id_raw(s: &str) -> Option<u32> { s.parse().ok() }
        pub fn select_top_level_disk(id: u32) -> Option<u32> { (id == 7).then_some(id) }
    }
    pub mod os_install {
        pub fn submit_install_to_target(_: &crate::Spawner, _: (), disk: u32) { assert_eq!(disk, 7); crate::event("local"); }
        pub fn submit_install_mode_to_target(_: &crate::Spawner, _: (), disk: u32, _: crate::disc::install::pxeproc::InstallMode) { assert_eq!(disk, 7); crate::event("pxeproc-local"); }
    }
    pub mod os_update {
        pub const LAN_ISO_URL: &str = "lan";
        pub fn submit_live_update_to_target(_: &crate::Spawner, _: ()) { crate::event("live"); }
        pub fn submit_live_update_from_url_to_target(_: &crate::Spawner, _: (), url: &str) { assert_eq!(url, LAN_ISO_URL); crate::event("lan"); }
        pub fn submit_online_install_to_target(_: &crate::Spawner, _: (), disk: u32) { assert_eq!(disk, 7); crate::event("online"); }
        pub fn submit_online_install_mode_to_target(_: &crate::Spawner, _: (), disk: u32, _: crate::disc::install::pxeproc::InstallMode) { assert_eq!(disk, 7); crate::event("pxeproc-online"); }
    }
    pub mod os {
        use crate::{Spawner, MatrixTarget, print_matrix_target_line};
        __DISPATCH__
        pub fn run(reason: &str) { dispatch_admin_action(&Spawner, &(), reason); }
    }
}
fn main() {
    for (token, expected) in [
        ("os:quit", None), ("os:cancel", None),
        ("os:reboot", Some("reboot")), ("os:shutdown", Some("shutdown")),
        ("os:update:live", Some("live")), ("os:update:lan", Some("lan")),
        ("os:install:local:7", Some("local")), ("os:install:online:7", Some("online")),
        ("os:install:pxeproc-local:7", Some("pxeproc-local")),
        ("os:install:pxeproc-online:7", Some("pxeproc-online")),
        ("reboot", Some("os: rejected unknown administration action")),
        ("os:install:local", Some("os: rejected malformed install action")),
        ("os:install:local:bad", Some("os: selected disk id is invalid")),
        ("os:install:local:8", Some("os: selected disk is no longer available")),
        ("os:install:evil:7", Some("os: rejected unknown install source")),
    ] {
        EVENTS.lock().unwrap().clear();
        shell3::os::run(token);
        let events = EVENTS.lock().unwrap();
        assert_eq!(events.iter().map(String::as_str).collect::<Vec<_>>(), expected.into_iter().collect::<Vec<_>>(), "{token}");
    }
}
'''
with tempfile.TemporaryDirectory(prefix="trueos-os-actions-") as tmp:
    path = Path(tmp)
    (path / "test.rs").write_text(harness.replace("__DISPATCH__", dispatch))
    subprocess.run(["rustc", "--edition=2024", str(path / "test.rs"), "-o", str(path / "test")], check=True)
    subprocess.run([str(path / "test")], check=True)
print("OS administration action routing: 15 cases passed")
