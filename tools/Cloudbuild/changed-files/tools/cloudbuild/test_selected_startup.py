"""Run the production cold-start launch decision with a host-only API harness."""
from __future__ import annotations

from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from build_plan import ROOT


@unittest.skipUnless(shutil.which("rustc"), "host rustc is required to execute the production startup decision")
class SelectedStartupTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="trueos-selected-startup-")
        cls.addClassCleanup(cls.temporary.cleanup)
        directory = Path(cls.temporary.name)
        production = (ROOT / "src/r/restart.rs").read_text()
        method_start = production.index("impl ColdStartBlueprint {")
        method_end = production.index("\n#[derive", method_start)
        methods = production[method_start:method_end]
        closure_start = production.index("let result = (|| {", production.index("pub(crate) async fn restart_apps_task"))
        closure_end = production.index("})();", closure_start) + len("})();")
        decision = production[closure_start:closure_end]
        # Only the kernel interfaces are stubbed. The decision and pullbot
        # construction below come from production, without a duplicate model.
        harness = r'''
extern crate alloc;
use alloc::{string::String, vec::Vec};

struct ColdStartBlueprint {
    archive: String,
    online_selector: Option<String>,
    instance: Option<String>,
    args: Vec<String>,
    launch_script: Option<String>,
}

mod app_db {
    use std::sync::Mutex;
    pub static APPS: Mutex<Vec<(String, Vec<u8>)>> = Mutex::new(Vec::new());
    pub fn get(archive: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(APPS.lock().unwrap().iter().find(|(name, _)| name == archive).map(|(_, bytes)| bytes.clone()))
    }
}

mod hv {
    #[derive(Clone, Debug, Default)]
    pub struct BlueprintInstanceRequest { pub name: Option<String> }
    impl BlueprintInstanceRequest {
        pub fn named(name: String) -> Self { Self { name: Some(name) } }
    }
    pub mod launcher {
        use std::sync::Mutex;
        #[derive(Clone, Debug)]
        pub struct LaunchRequest {
            pub archive: String,
            pub bytes: Vec<u8>,
            pub args: Vec<String>,
            pub script: Option<String>,
            pub instance: super::BlueprintInstanceRequest,
        }
        impl LaunchRequest {
            pub fn new(archive: String, bytes: Vec<u8>) -> Self {
                Self { archive, bytes, args: Vec::new(), script: None, instance: Default::default() }
            }
        }
        pub static QUEUE: Mutex<Vec<LaunchRequest>> = Mutex::new(Vec::new());
        pub fn enqueue(request: LaunchRequest, _: Option<()>) -> Result<(), String> {
            QUEUE.lock().unwrap().push(request);
            Ok(())
        }
    }
}

PRODUCTION_METHODS

fn launch(app: ColdStartBlueprint, embedded_defaults: bool) -> Result<bool, String> {
    PRODUCTION_DECISION
    result
}

fn app() -> ColdStartBlueprint {
    ColdStartBlueprint { archive: "hull.bp".into(), online_selector: None,
                         instance: None, args: Vec::new(), launch_script: None }
}

fn reset(names: &[&str]) {
    *app_db::APPS.lock().unwrap() = names.iter().map(|name| (name.to_string(), name.as_bytes().to_vec())).collect();
    hv::launcher::QUEUE.lock().unwrap().clear();
}

fn only_request() -> hv::launcher::LaunchRequest {
    let queue = hv::launcher::QUEUE.lock().unwrap();
    assert_eq!(queue.len(), 1);
    queue[0].clone()
}

fn main() {
    // Appstore being selected must not undo deselecting Hull at default boot.
    reset(&["apps.bp"]);
    assert_eq!(launch(app(), true).unwrap(), false);
    assert!(hv::launcher::QUEUE.lock().unwrap().is_empty());

    // A selected local Hull still launches, preserving its request parameters.
    reset(&["hull.bp", "apps.bp"]);
    let mut local = app();
    local.args.push("--once".into());
    local.launch_script = Some("test local script".into());
    local.instance = Some("named-hull".into());
    assert_eq!(launch(local, true).unwrap(), true);
    let request = only_request();
    assert_eq!(request.archive, "hull.bp");
    assert_eq!(request.bytes, b"hull.bp");
    assert_eq!(request.args, vec!["--once"]);
    assert_eq!(request.script.as_deref(), Some("test local script"));
    assert_eq!(request.instance.name.as_deref(), Some("named-hull"));

    // An explicit/custom startup configuration retains online fallback.
    reset(&["apps.bp"]);
    assert_eq!(launch(app(), false).unwrap(), true);
    let request = only_request();
    assert_eq!(request.archive, "apps.bp");
    assert_eq!(request.bytes, b"apps.bp");
    assert_eq!(request.script.as_deref(), Some("pull --headless hull\n"));

    // An explicit online selector still uses Appstore even if Hull is local.
    reset(&["hull.bp", "apps.bp"]);
    let mut online = app();
    online.online_selector = Some("hull-next".into());
    online.launch_script = Some("run chosen script".into());
    assert_eq!(launch(online, false).unwrap(), true);
    let request = only_request();
    assert_eq!(request.archive, "apps.bp");
    assert_eq!(request.script.as_deref(), Some("pull --headless hull-next -- run chosen script\n"));

    // Custom online startup fails clearly if the user also omitted Appstore.
    reset(&[]);
    assert_eq!(launch(app(), false).unwrap_err(), "Appstore builtin unavailable");
    assert!(hv::launcher::QUEUE.lock().unwrap().is_empty());

    // The ordinary kernel-only default is a valid empty startup.
    reset(&[]);
    assert_eq!(launch(app(), true).unwrap(), false);
    assert!(hv::launcher::QUEUE.lock().unwrap().is_empty());
    println!("production startup decision: six scenarios passed");
}
'''.replace("PRODUCTION_METHODS", methods).replace("PRODUCTION_DECISION", decision)
        source = directory / "startup.rs"
        source.write_text(harness)
        cls.binary = directory / "startup"
        subprocess.run([shutil.which("rustc"), "--edition=2021", str(source), "-o", str(cls.binary)],
                       cwd=directory, check=True, capture_output=True, text=True, timeout=30)

    def test_selected_defaults_and_explicit_online_startup(self):
        result = subprocess.run([str(self.binary)], capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("six scenarios passed", result.stdout)


if __name__ == "__main__":
    unittest.main()
