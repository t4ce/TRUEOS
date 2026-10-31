#!/usr/bin/env python3
"""Check initial-session prewarm policy and real caller/future wiring with rustc.

The policy module and public idle prewarm methods are production source. The
initial wrapper, missing-prewarm first-turn handoff, and detached task body are
also copied unchanged. Small session/model/telemetry boundaries replace the
large core dependency graph. This verifies policy and measurement ownership;
it does not contact a model or simulate WebSocket transport.
"""

import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
CODEX = Path(os.environ.get("TRUEOS_CODEX_ROOT", ROOT.parent / "codex"))
SESSION = CODEX / "codex-rs/core/src/session"


def method(source: str, name: str) -> str:
    declaration = re.search(
        rf"^    (?:pub(?:\([^)]*\))? )?async fn {re.escape(name)}\(",
        source, re.MULTILINE,
    )
    if declaration is None:
        raise ValueError(f"missing production method {name}")
    ending = re.search(r"^    }\s*\n", source[declaration.end():], re.MULTILINE)
    if ending is None:
        raise ValueError(f"missing closing brace for {name}")
    return source[declaration.start():declaration.end() + ending.end()]


def detached_future(source: str) -> str:
    start = source.index("async move {", source.index("let startup_prewarm = tokio::spawn("))
    ending = re.search(r"^            }\s*\n", source[start:], re.MULTILINE)
    if ending is None:
        raise ValueError("missing detached prewarm task closing brace")
    return source[start:start + ending.end()]


HARNESS = r'''#![allow(dead_code)]
use std::future::Future;
use std::pin::Pin;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

#[path = @POLICY@]
mod startup_prewarm_policy;
use startup_prewarm_policy::{PrewarmSchedule, STARTUP_PREWARM_ENV};

static EVENTS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
fn record(value: &'static str) { EVENTS.lock().unwrap().push(value); }
fn take_events() -> Vec<&'static str> { std::mem::take(&mut *EVENTS.lock().unwrap()) }
macro_rules! warn { ($message:literal) => { record("static_warning") }; }

mod codex_startup_timing {
    pub fn mark(stage: &'static str) { crate::record(stage); }
    pub async fn measure<F: std::future::Future>(name: &'static str, future: F) -> F::Output {
        assert_eq!(name, "core.startup_websocket_prewarm");
        crate::record("measure_begin");
        let output = future.await;
        crate::record("measure_end");
        output
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PrewarmInput { Base, History }
impl PrewarmInput {
    fn as_str(self) -> &'static str { match self { Self::Base => "base", Self::History => "history" } }
}
#[derive(Clone, Default)]
struct Telemetry;
impl Telemetry {
    fn record_startup_phase(&self, name: &'static str, _: Duration, _: Option<&str>) {
        record(name);
    }
    fn record_duration(&self, _: &str, _: Duration, _: &[(&str, &str)]) { record("duration"); }
}
#[derive(Default)]
struct Services { session_telemetry: Telemetry }
#[derive(Default)]
struct Session {
    scheduled: Mutex<Option<(PrewarmInput, PrewarmSchedule)>>,
    services: Services,
}
impl Session {
    async fn schedule_prewarm(self: &Arc<Self>, input: PrewarmInput, policy: PrewarmSchedule) {
        *self.scheduled.lock().unwrap() = Some((input, policy));
    }
    async fn take_session_startup_prewarm(&self) -> Option<SessionStartupPrewarmHandle> { None }
@SESSION_METHODS@
}
struct CodexThread { session: Arc<Session> }
impl CodexThread {
@IDLE_METHODS@
}

struct CancellationToken;
enum SessionStartupPrewarmResolution {
    Unavailable { status: &'static str, prewarm_duration: Option<Duration> },
}
struct SessionStartupPrewarmHandle;
impl SessionStartupPrewarmHandle {
    async fn resolve(self, _: &Telemetry, _: &CancellationToken) -> SessionStartupPrewarmResolution {
        panic!("a missing startup task must not wait for or resolve a handle")
    }
}
struct ModelClientSession;
type CodexResult<T> = Result<T, ()>;
const STARTUP_PREWARM_DURATION_METRIC: &str = "test_duration";
async fn schedule_startup_prewarm_inner(_: Arc<Session>, _: PrewarmInput) -> CodexResult<ModelClientSession> {
    record("prepare_context_tools_and_prompt");
    Ok(ModelClientSession)
}
async fn run_detached(measure_startup: bool) -> CodexResult<ModelClientSession> {
    let startup_prewarm_session = Arc::new(Session::default());
    let session_telemetry = Telemetry;
    let started_at = Instant::now();
    let input = PrewarmInput::Base;
    let future = @DETACHED_FUTURE@;
    future.await
}

struct NoWake;
impl Wake for NoWake { fn wake(self: Arc<Self>) { panic!("unexpected executor wake"); } }
fn ready<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(NoWake));
    let mut context = Context::from_waker(&waker);
    match pin!(future).as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("the stub boundary should complete without input, I/O or a runtime"),
    }
}
fn set_override(value: Option<&str>) {
    // This harness runs tests sequentially and starts no background threads.
    unsafe {
        match value {
            Some(value) => std::env::set_var(STARTUP_PREWARM_ENV, value),
            None => std::env::remove_var(STARTUP_PREWARM_ENV),
        }
    }
}

#[test]
fn initial_wrapper_selects_platform_policy_and_fails_closed_without_values_in_logs() {
    for value in [None, Some("0"), Some("1"), Some("private-invalid-value")] {
        set_override(value);
        take_events();
        let session = Arc::new(Session::default());
        ready(session.schedule_initial_prewarm(PrewarmInput::Base));
        let (input, policy) = session.scheduled.lock().unwrap().take().unwrap();
        assert_eq!(input, PrewarmInput::Base);
        assert!(policy.is_initial());
        assert_eq!(policy.websocket_enabled(true), !cfg!(target_os = "trueos") || value == Some("1"));
        assert!(!policy.websocket_enabled(false));
        let events = take_events();
        if cfg!(target_os = "trueos") && value == Some("private-invalid-value") {
            assert_eq!(events, vec!["core_startup_prewarm_override_invalid", "static_warning"]);
        } else {
            assert!(events.is_empty());
        }
    }
    set_override(None);
}

#[test]
fn explicit_idle_apis_preserve_full_and_history_warmup_despite_initial_override() {
    set_override(Some("0"));
    let session = Arc::new(Session::default());
    let thread = CodexThread { session: Arc::clone(&session) };
    ready(thread.prewarm());
    let (input, policy) = session.scheduled.lock().unwrap().take().unwrap();
    assert_eq!(input, PrewarmInput::Base);
    assert_eq!(policy, PrewarmSchedule::Explicit);
    assert!(policy.websocket_enabled(true));
    ready(thread.prewarm_with_history());
    let (input, policy) = session.scheduled.lock().unwrap().take().unwrap();
    assert_eq!(input, PrewarmInput::History);
    assert_eq!(policy, PrewarmSchedule::Explicit);
    assert!(policy.websocket_enabled(true));
    set_override(None);
}

#[test]
fn absent_startup_handle_uses_existing_first_turn_unavailable_path_without_waiting() {
    let session = Session::default();
    match ready(session.consume_startup_prewarm_for_regular_turn(&CancellationToken)) {
        SessionStartupPrewarmResolution::Unavailable { status, prewarm_duration } => {
            assert_eq!(status, "not_scheduled");
            assert!(prewarm_duration.is_none());
        }
    }
}

#[test]
fn initial_detached_measurement_owns_context_preparation_and_existing_telemetry() {
    take_events();
    assert!(ready(run_detached(true)).is_ok());
    assert_eq!(take_events(), vec!["measure_begin", "prepare_context_tools_and_prompt",
        "startup_prewarm_total", "duration", "measure_end"]);
}

#[test]
fn explicit_idle_detached_task_avoids_startup_measurements() {
    take_events();
    assert!(ready(run_detached(false)).is_ok());
    assert_eq!(take_events(), vec!["prepare_context_tools_and_prompt", "startup_prewarm_total", "duration"]);
}

#[test]
fn detached_tasks_preserve_the_spawn_send_contract() {
    fn assert_send<T: Send>(_: T) {}
    assert_send(run_detached(true));
    assert_send(run_detached(false));
}
'''


def main() -> None:
    production = (SESSION / "startup_prewarm.rs").read_text()
    initial_setup = (SESSION / "session.rs").read_text()
    assert initial_setup.count("sess.schedule_initial_prewarm(") == 1
    assert "sess.schedule_startup_prewarm(" not in initial_setup
    shared = method(production, "schedule_prewarm")
    # Provider capability still gates both initial and explicit modes. The
    # original auth-only task remains before the return, without a cache handle.
    provider = shared.index("self.services.model_client.responses_websocket_enabled()")
    gate = shared.index("if !schedule.websocket_enabled(websocket_enabled)")
    auth = shared.index("model_client.prewarm_auth().await", gate)
    fallback_return = shared.index("return;", auth)
    handle = shared.index("state.set_session_startup_prewarm(")
    assert provider < gate < auth < fallback_return < handle
    source = HARNESS.replace("@POLICY@", json.dumps(str(SESSION / "startup_prewarm_policy.rs")))
    source = source.replace("@SESSION_METHODS@", "\n".join(method(production, name) for name in (
        "schedule_initial_prewarm", "schedule_startup_prewarm", "consume_startup_prewarm_for_regular_turn",
    )))
    thread = (CODEX / "codex-rs/core/src/codex_thread.rs").read_text()
    source = source.replace("@IDLE_METHODS@", "\n".join(method(thread, name) for name in (
        "prewarm", "prewarm_with_history",
    )))
    source = source.replace("@DETACHED_FUTURE@", detached_future(production))
    env = dict(os.environ)
    env.pop("CDX_STARTUP_PREWARM", None)
    with tempfile.TemporaryDirectory(prefix="trueos-codex-startup-prewarm-") as temporary:
        directory = Path(temporary)
        harness = directory / "harness.rs"
        harness.write_text(source)
        for trueos in (False, True):
            binary = directory / ("trueos" if trueos else "host")
            flags = ["--cfg", 'target_os="trueos"', "-Aexplicit_builtin_cfgs_in_flags"] if trueos else []
            subprocess.run([
                "rustc", "--edition=2024", "--test", *flags, str(harness), "-o", str(binary),
            ], check=True, timeout=60)
            subprocess.run([str(binary), "--test-threads=1"], env=env, check=True, timeout=30)


if __name__ == "__main__":
    main()
