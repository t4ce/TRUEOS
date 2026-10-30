#!/usr/bin/env python3
"""Exercise Edit's production compiler acquisition decision with a fake VM broker."""

from pathlib import Path
import subprocess
import tempfile

from test_blueprint_archive_scope import function


ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    edit = (ROOT.parent / "TRUEOS-Buildins/edit/crates/edit/src/bin/edit/ide.rs").read_text()
    source = "use std::{io, cell::RefCell};\n"
    source += function(edit, "struct CompilerJob {") + "\n"
    source += function(edit, "impl CompilerJob {") + "\n"
    source += r'''
thread_local! {
    static CALLS: RefCell<Vec<(String, Vec<String>, Option<String>)>> = const { RefCell::new(Vec::new()) };
    static RESULTS: RefCell<Vec<io::ErrorKind>> = const { RefCell::new(Vec::new()) };
}
struct OwnedChild;
impl OwnedChild {
    fn record(program: &str, args: &[&str], script: Option<&str>) -> io::Result<Self> {
        CALLS.with(|calls| calls.borrow_mut().push((program.into(), args.iter().map(|arg| arg.to_string()).collect(), script.map(str::to_owned))));
        RESULTS.with(|results| {
            if results.borrow().is_empty() { return Ok(Self); }
            Err(io::Error::new(results.borrow_mut().remove(0), "broker rejected launch"))
        })
    }
    fn spawn(program: &str, args: &[&str], _: bool) -> io::Result<Self> {
        Self::record(program, args, None)
    }
    fn spawn_script(program: &str, script: &str) -> io::Result<Self> {
        Self::record(program, &[], Some(script))
    }
}

#[test]
fn installed_compiler_runs_directly_without_network_helper() {
    let job = CompilerJob::start("odin-son.bp", "/apps/common/ide/builds/7/request.json").unwrap();
    assert!(job.pending.is_none());
    CALLS.with(|calls| {
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "odin-son.bp");
        assert_eq!(calls[0].1, ["--ide-build", "/apps/common/ide/builds/7/request.json"]);
        assert!(calls[0].2.is_none());
    });
}

#[test]
fn missing_compiler_uses_apps_install_and_keeps_exact_snapshot() {
    RESULTS.with(|results| results.borrow_mut().push(io::ErrorKind::NotFound));
    let path = "/apps/common/ide/builds/project with spaces/8/request.json";
    let job = CompilerJob::start("odin-son.bp", path).unwrap();
    assert_eq!(job.pending, Some(("odin-son.bp".into(), path.into())));
    CALLS.with(|calls| {
        let calls = calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].0, "apps.bp");
        assert!(calls[1].1.is_empty());
        assert_eq!(calls[1].2.as_deref(), Some("install odin-son\n"));
    });
}

#[test]
fn launch_failure_does_not_trigger_a_download() {
    RESULTS.with(|results| results.borrow_mut().push(io::ErrorKind::Other));
    assert!(CompilerJob::start("odin-son.bp", "request.json").is_err());
    CALLS.with(|calls| assert_eq!(calls.borrow().len(), 1));
}

#[test]
fn unavailable_downloader_is_reported_without_repeating_the_build() {
    RESULTS.with(|results| results.borrow_mut().extend([io::ErrorKind::NotFound, io::ErrorKind::PermissionDenied]));
    let error = CompilerJob::start("rustc-min.bp", "request.json").err().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    CALLS.with(|calls| {
        assert_eq!(calls.borrow().len(), 2);
        assert_eq!(calls.borrow()[1].2.as_deref(), Some("install rustc-min\n"));
    });
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-compiler-pull-") as temp:
        root = Path(temp)
        (root / "test.rs").write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(root / "test.rs"), "-o", str(root / "tests")], check=True)
        subprocess.run([str(root / "tests")], check=True)


if __name__ == "__main__":
    main()
