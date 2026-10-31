#!/usr/bin/env python3
"""Exercise production startup-skills origin, coalescing, and future ownership.

Compile the actual policy/App helper module, dispatch entry, bounded dispatch
sampler, and detached background task body unchanged. Only RPC/executor boundaries
are fixtures. A pending RPC proves the coalesced event can return while opt-out
and explicit refreshes retain their awaited path. No app/kernel build or rig.
"""
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
CODEX = Path(os.environ.get("TRUEOS_CODEX_ROOT", ROOT.parent / "codex"))
TUI = CODEX / "codex-rs/tui/src"


def item(source, pattern):
    start = re.search(pattern, source, re.M)
    assert start, pattern
    brace = source.index("{", start.end())
    end = re.search(r"^}\s*\n", source[brace:], re.M)
    assert end, pattern
    return source[start.start():brace + end.end()]


HARNESS = r'''#![allow(dead_code)]
extern crate self as tracing;
use std::future::Future;
use std::path::PathBuf;
use std::pin::{Pin, pin};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
type Result<T> = std::result::Result<T, String>;
static EVENTS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
fn record(event: &'static str) { EVENTS.lock().unwrap().push(event); }
fn take_events() -> Vec<&'static str> { std::mem::take(&mut *EVENTS.lock().unwrap()) }
#[macro_export]
macro_rules! warn { ($message:literal) => { crate::record("static_warning") }; }
mod codex_arg0 { pub mod startup_timing {
    pub fn mark(stage: &'static str) { crate::record(stage); }
    pub async fn measure<F: std::future::Future>(name: &'static str, future: F) -> F::Output {
        assert_eq!(name, "tui.startup_skills_refresh");
        crate::record("measure_begin");
        let value = future.await;
        crate::record("measure_end");
        value
    }
} }
#[derive(Debug, PartialEq)]
enum AppCommand { ListSkills { cwds: Vec<PathBuf>, force_reload: bool }, Manual }
impl AppCommand {
    fn list_skills(cwds: Vec<PathBuf>, force_reload: bool) -> Self {
        Self::ListSkills { cwds, force_reload }
    }
}
#[derive(Debug)]
struct SkillsListResponse;
#[derive(Debug)]
enum AppEvent {
    RefreshSessionSkills { cwd: PathBuf },
    CodexOp(AppCommand),
    SkillsListLoaded { cwd: PathBuf, result: Result<SkillsListResponse> },
}
struct Config { cwd: PathBuf }
struct App {
    config: Config,
    startup_skills_refresh_cwd: Option<PathBuf>,
    inline_requests: usize,
}
#[derive(Debug, PartialEq)]
enum AppRunControl { Continue }
impl App {
    fn new() -> Self {
        Self { config: Config { cwd: "/fixture".into() },
            startup_skills_refresh_cwd: None, inline_requests: 0 }
    }
    // Production dispatch admission, followed by an intentionally pending RPC boundary.
    async fn dispatch(&mut self, event: AppEvent) -> Result<AppRunControl> {
@DISPATCH_ENTRY@
        if let AppEvent::CodexOp(AppCommand::ListSkills { .. }) = event {
            self.inline_requests += 1;
            record("inline_rpc");
            std::future::pending::<()>().await;
        }
        Ok(AppRunControl::Continue)
    }
}
#[path = @POLICY_PATH@]
mod startup_skills;

static CLOCK_READS: AtomicUsize = AtomicUsize::new(0);
static CLOCK: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct Instant(u64);
impl Instant { fn now() -> Self {
    CLOCK_READS.fetch_add(1, Ordering::Relaxed);
    Self(CLOCK.load(Ordering::Relaxed))
} }
@SAMPLER@

struct RequestHandle { ready: Arc<AtomicBool>, failed: bool }
struct Sender(Arc<Mutex<Vec<AppEvent>>>);
impl Sender { fn send(&self, event: AppEvent) { self.0.lock().unwrap().push(event); } }
async fn fetch_skills_list(handle: RequestHandle, _: PathBuf) -> Result<SkillsListResponse> {
    std::future::poll_fn(|_| {
        record("fetch_polled");
        if handle.ready.load(Ordering::Relaxed) {
            Poll::Ready(if handle.failed { Err("fixture_failure".into()) } else { Ok(SkillsListResponse) })
        } else { Poll::Pending }
    }).await
}
fn background(cwd: PathBuf, ready: Arc<AtomicBool>, failed: bool, sent: Arc<Mutex<Vec<AppEvent>>>) -> impl Future<Output=()> {
    let request_handle = RequestHandle { ready, failed };
    let app_event_tx = Sender(sent);
    @BACKGROUND@
}
struct NoWake;
impl Wake for NoWake { fn wake(self: Arc<Self>) {} }
fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    let waker = Waker::from(Arc::new(NoWake));
    future.poll(&mut Context::from_waker(&waker))
}
fn automatic(cwd: &str) -> AppEvent { AppEvent::RefreshSessionSkills { cwd: cwd.into() } }
fn set_override(value: Option<&std::ffi::OsStr>) {
    // Tests execute serially and create no background threads.
    unsafe { match value {
        Some(value) => std::env::set_var("CDX_STARTUP_SKILLS_COALESCE", value),
        None => std::env::remove_var("CDX_STARTUP_SKILLS_COALESCE"),
    } }
}
fn arm(app: &mut App, value: Option<&str>) {
    set_override(value.map(std::ffi::OsStr::new));
    app.arm_startup_skills_refresh();
    set_override(None);
}

#[test]
fn platform_default_and_zero_control_select_real_arm_path() {
    let mut app = App::new();
    arm(&mut app, None);
    assert_eq!(app.startup_skills_refresh_cwd.is_some(), cfg!(target_os="trueos"));
    app.startup_skills_refresh_cwd = None;
    arm(&mut app, Some("0"));
    assert!(app.startup_skills_refresh_cwd.is_none());
}
#[test]
fn invalid_override_is_static_and_keeps_trueos_coalescing_enabled() {
    take_events();
    let mut app = App::new();
    arm(&mut app, Some("private_invalid_override"));
    assert_eq!(app.startup_skills_refresh_cwd.is_some(), cfg!(target_os="trueos"));
    let events = take_events();
    if cfg!(target_os="trueos") {
        assert!(events.contains(&"static_warning"));
        assert!(events.contains(&"tui_startup_skills_override_invalid"));
    } else { assert!(events.is_empty()); }
}
#[cfg(unix)]
#[test]
fn nonunicode_override_is_rejected_without_logging_value() {
    use std::os::unix::ffi::OsStrExt;
    take_events();
    let mut app = App::new();
    set_override(Some(std::ffi::OsStr::from_bytes(b"private\xff")));
    app.arm_startup_skills_refresh();
    set_override(None);
    assert_eq!(app.startup_skills_refresh_cwd.is_some(), cfg!(target_os="trueos"));
    if cfg!(target_os="trueos") { assert!(take_events().contains(&"static_warning")); }
}
#[test]
fn initial_event_returns_while_measured_background_rpc_is_pending() {
    take_events();
    let mut app = App::new();
    app.startup_skills_refresh_cwd = Some("/fixture".into());
    let done = Arc::new(AtomicBool::new(false));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let mut work = pin!(background("/fixture".into(), done.clone(), false, sent.clone()));
    assert!(poll(work.as_mut()).is_pending());
    {
        let mut foreground = pin!(app.dispatch(automatic("/fixture")));
        assert_eq!(poll(foreground.as_mut()), Poll::Ready(Ok(AppRunControl::Continue)));
    }
    assert_eq!(app.inline_requests, 0);
    assert!(app.startup_skills_refresh_cwd.is_none());
    assert!(sent.lock().unwrap().is_empty());
    assert_eq!(take_events(), ["measure_begin", "fetch_polled", "tui_startup_skills_coalesced"]);
    done.store(true, Ordering::Relaxed);
    assert!(poll(work.as_mut()).is_ready());
    assert_eq!(take_events(), ["fetch_polled", "measure_end"]);
    assert!(matches!(&sent.lock().unwrap()[0], AppEvent::SkillsListLoaded { cwd, result: Ok(_) } if cwd == &PathBuf::from("/fixture")));
}
#[test]
fn early_background_success_and_failure_do_not_erase_origin_token() {
    for failed in [false, true] {
        let mut app = App::new();
        app.startup_skills_refresh_cwd = Some("/fixture".into());
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut work = pin!(background("/fixture".into(), Arc::new(AtomicBool::new(true)), failed, sent.clone()));
        assert!(poll(work.as_mut()).is_ready());
        assert!(matches!(&sent.lock().unwrap()[0], AppEvent::SkillsListLoaded { result, .. } if result.is_err() == failed));
        assert!(app.normalize_session_skills_refresh(automatic("/fixture")).is_none());
        assert!(app.startup_skills_refresh_cwd.is_none());
    }
}
#[test]
fn duplicate_exemption_is_consumed_once_and_later_refresh_remains_inline() {
    let mut app = App::new();
    app.startup_skills_refresh_cwd = Some("/fixture".into());
    assert!(app.normalize_session_skills_refresh(automatic("/fixture")).is_none());
    let mut later = pin!(app.dispatch(automatic("/fixture")));
    assert!(poll(later.as_mut()).is_pending());
}
#[test]
fn changed_directory_consumes_token_without_suppressing_later_work() {
    let mut app = App::new();
    app.startup_skills_refresh_cwd = Some("/fixture".into());
    assert!(matches!(app.normalize_session_skills_refresh(automatic("/other")),
        Some(AppEvent::CodexOp(AppCommand::ListSkills { cwds, force_reload:true })) if cwds == [PathBuf::from("/other")]));
    assert!(app.startup_skills_refresh_cwd.is_none());
    assert!(app.normalize_session_skills_refresh(automatic("/fixture")).is_some());
}
#[test]
fn stale_initial_cwd_is_not_coalesced_after_workspace_changes() {
    let mut app = App::new();
    app.startup_skills_refresh_cwd = Some("/fixture".into());
    app.config.cwd = "/other".into();
    assert!(app.normalize_session_skills_refresh(automatic("/fixture")).is_some());
    assert!(app.startup_skills_refresh_cwd.is_none());
}
#[test]
fn explicit_manual_command_keeps_wait_and_does_not_consume_token() {
    let mut app = App::new();
    app.startup_skills_refresh_cwd = Some("/fixture".into());
    {
        let event = AppEvent::CodexOp(AppCommand::list_skills(vec!["/fixture".into()], true));
        let mut explicit = pin!(app.dispatch(event));
        assert!(poll(explicit.as_mut()).is_pending());
    }
    assert_eq!(app.inline_requests, 1);
    assert_eq!(app.startup_skills_refresh_cwd, Some(PathBuf::from("/fixture")));
}
#[test]
fn control_zero_restores_original_awaited_initial_request() {
    let mut app = App::new();
    arm(&mut app, Some("0"));
    let mut original = pin!(app.dispatch(automatic("/fixture")));
    assert!(poll(original.as_mut()).is_pending());
}
#[test]
fn dispatch_probe_reserves_two_budgets_and_stops_reading_clock_after_cap() {
    CLOCK.store(1, Ordering::Relaxed);
    CLOCK_READS.store(0, Ordering::Relaxed);
    let mut counts = [0;2];
    let mut deadline = Some(Instant(120));
    for sample in 1..=8 { assert_eq!(startup_dispatch_sample(&mut counts, false, &mut deadline), Some(sample)); }
    for _ in 0..100 { assert_eq!(startup_dispatch_sample(&mut counts, false, &mut deadline), None); }
    assert_eq!(CLOCK_READS.load(Ordering::Relaxed), 8);
    for sample in 9..=16 { assert_eq!(startup_dispatch_sample(&mut counts, true, &mut deadline), Some(sample)); }
    for _ in 0..100 { assert_eq!(startup_dispatch_sample(&mut counts, true, &mut deadline), None); }
    assert_eq!(CLOCK_READS.load(Ordering::Relaxed), 16);
}
#[test]
fn expiry_stops_both_budgets_and_early_handoff_leaves_number_gap() {
    CLOCK.store(1, Ordering::Relaxed);
    CLOCK_READS.store(0, Ordering::Relaxed);
    let mut counts = [0;2];
    let mut deadline = Some(Instant(120));
    assert_eq!(startup_dispatch_sample(&mut counts, false, &mut deadline), Some(1));
    assert_eq!(startup_dispatch_sample(&mut counts, true, &mut deadline), Some(9));
    CLOCK.store(120, Ordering::Relaxed);
    assert_eq!(startup_dispatch_sample(&mut counts, true, &mut deadline), None);
    for _ in 0..100 { assert_eq!(startup_dispatch_sample(&mut counts, false, &mut deadline), None); }
    assert_eq!(CLOCK_READS.load(Ordering::Relaxed), 3);
    assert!(deadline.is_none());
}
'''


def main():
    dispatch = (TUI / "app/event_dispatch.rs").read_text()
    start = dispatch.index("        let Some(event) = self.normalize_session_skills_refresh(event)")
    finish = dispatch.index("        let from_agents_overview", start)
    entry = dispatch[start:finish]
    background_source = (TUI / "app/background_requests.rs").read_text()
    start = background_source.index("async move {", background_source.index("fn refresh_startup_skills("))
    finish = re.search(r"^        }\);", background_source[start:], re.M)
    assert finish
    detached = background_source[start:start + finish.start()] + "        }"
    startup = (TUI / "app/startup.rs").read_text()
    sampler = item(startup, r"^fn startup_dispatch_sample\(")
    flow = (TUI / "chatwidget/session_flow.rs").read_text()
    assert flow.index("self.refresh_plugin_mentions();") < flow.index("self.apply_session_info_cell(") < flow.index("AppEvent::RefreshSessionSkills {")
    assert "self.refresh_skills_for_current_cwd(/*force_reload*/ true);" in flow
    assert startup.index("app.arm_startup_skills_refresh();") < startup.index("app.refresh_startup_skills(&app_server);")
    assert startup.count("startup_dispatch_sample(") == 3  # Definition + app/server callers.
    assert "Duration::from_secs(/*secs*/ 120)" in startup
    assert "Ok(painted) => startup_handoff_rendered |= painted" in startup
    assert "let event_name: &'static str = (&event).into();" in startup
    assert "event={event_name} op={op}" in startup
    # Preserve the existing stale-result guard and explicit/manual RPC path.
    assert "AppEvent::SkillsListLoaded { ref cwd, .. }\n                if cwds_differ(cwd, self.config.cwd.as_path())" in dispatch
    routing = (TUI / "app/thread_routing.rs").read_text()
    arm = routing[routing.index("            AppCommand::ListSkills { cwds, force_reload } => {"):]
    arm = arm[:arm.index("            AppCommand::Compact =>")]
    assert ".skills_list(" in arm and ".await," in arm
    policy = TUI / "app/startup_skills.rs"
    source = HARNESS.replace("@POLICY_PATH@", json.dumps(str(policy)))
    source = source.replace("@DISPATCH_ENTRY@", entry).replace("@BACKGROUND@", detached).replace("@SAMPLER@", sampler)
    with tempfile.TemporaryDirectory(prefix="codex-startup-skills-") as td:
        source_path = Path(td) / "harness.rs"
        source_path.write_text(source)
        for trueos in (False, True):
            binary = Path(td) / ("trueos" if trueos else "host")
            command = ["rustc", "--edition=2024", "--test", str(source_path), "-o", str(binary)]
            if trueos:
                command += ["--cfg", 'target_os="trueos"', "-Aexplicit_builtin_cfgs_in_flags"]
            subprocess.run(command, check=True)
            subprocess.run([str(binary), "--test-threads=1"], check=True)
    print("Production source origin/order, real background future, caller entry, manual/stale guards PASS")


if __name__ == "__main__":
    main()
