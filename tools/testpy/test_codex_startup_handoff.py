#!/usr/bin/env python3
"""Exercise the actual Codex startup handoff methods with host rustc.

The production methods are copied unchanged into a small Rust module. Stubs
provide terminal writes, queue observations, and modal state; they contain no
handoff policy. This checks synchronous paint, one-shot admission, errors, and
a queue-arrival race without a runtime, input stream, timer, or full TUI build.
The Codex TUI tests separately cover real history and draft rendering.

Run with ``python3 tools/testpy/test_codex_startup_handoff.py``. Set
TRUEOS_CODEX_ROOT when the Codex checkout is not a sibling of TRUEOS.
"""

import os
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
CODEX = Path(os.environ.get("TRUEOS_CODEX_ROOT", ROOT.parent / "codex"))
SOURCE_PATH = CODEX / "codex-rs/tui/src/app/startup.rs"


def method(source: str, name: str) -> str:
    """Read a rustfmt-indented impl method without rewriting its body."""
    declarations = list(re.finditer(
        rf"^    pub\(super\) fn {re.escape(name)}\(", source, re.MULTILINE
    ))
    if len(declarations) != 1:
        raise ValueError(
            f"{SOURCE_PATH}: expected one method {name}, got {len(declarations)}"
        )
    declaration = declarations[0]
    ending = re.search(r"^    }\s*\n", source[declaration.end():], re.MULTILINE)
    if ending is None:
        raise ValueError(f"{SOURCE_PATH}: no method closing brace for {name}")
    return source[declaration.start():declaration.end() + ending.end()]


HARNESS = r'''#![allow(dead_code)]
mod production {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    type Result<T> = std::result::Result<T, &'static str>;
    struct AppEvent;

    mod mpsc {
        use super::*;

        pub struct UnboundedReceiver<T> {
            empty: bool,
            observations: RefCell<VecDeque<bool>>,
            kind: std::marker::PhantomData<T>,
        }

        impl<T> UnboundedReceiver<T> {
            pub fn new(empty: bool) -> Self {
                Self { empty, observations: Default::default(), kind: Default::default() }
            }

            pub fn with_incoming_event() -> Self {
                let receiver = Self::new(true);
                // The queue is empty at the handoff precheck, then receives an event
                // before the owned-screen render guard. Later calls observe a drain.
                receiver.observations.borrow_mut().extend([true, false]);
                receiver
            }

            pub fn is_empty(&self) -> bool {
                self.observations.borrow_mut().pop_front().unwrap_or(self.empty)
            }
        }
    }

    mod tui {
        #[derive(Default)]
        pub struct Terminal { pub last_known_screen_size: u8 }

        #[derive(Default)]
        pub struct Tui {
            pub terminal: Terminal,
            pub owned: bool,
            pub paints: usize,
            pub fail: bool,
            pub quarantines: usize,
        }

        impl Tui {
            pub fn is_owned_screen(&self) -> bool { self.owned }
        }
    }

    mod codex_arg0 {
        pub mod startup_timing {
            use std::cell::RefCell;

            std::thread_local! {
                static EVENTS: RefCell<Vec<(&'static str, &'static str)>> = const { RefCell::new(Vec::new()) };
            }

            pub fn record(kind: &'static str, value: &'static str) {
                EVENTS.with(|events| events.borrow_mut().push((kind, value)));
            }

            pub fn take() -> Vec<(&'static str, &'static str)> {
                EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
            }

            pub fn mark(stage: &'static str) { record("milestone", stage); }

            pub struct Span { ended: bool }

            pub fn span(name: &'static str) -> Span {
                record("begin", name);
                Span { ended: false }
            }

            impl Span {
                pub fn finish(mut self, outcome: &'static str) {
                    record("end", outcome);
                    self.ended = true;
                }
            }

            impl Drop for Span {
                fn drop(&mut self) {
                    if !self.ended { record("end", "cancelled"); }
                }
            }
        }
    }

    #[derive(Default)]
    struct Chat { active_view: bool, active_modal: bool }

    impl Chat {
        fn has_active_view(&self) -> bool { self.active_view }
        fn has_active_modal(&self) -> bool { self.active_modal }
        fn pre_draw_tick(&mut self) {
            codex_arg0::startup_timing::record("step", "tick");
        }
    }

    #[derive(Default)]
    struct App {
        primary_session_configured: Option<()>,
        pending_startup_thread_start: bool,
        startup_protected_input_boundary: bool,
        chat_widget: Chat,
        queued_protected: bool,
    }

    impl App {
        fn has_queued_startup_protected_request(&self) -> bool { self.queued_protected }

        fn render_chat_widget_frame(&mut self, tui: &mut tui::Tui, _: u8) -> Result<()> {
            codex_arg0::startup_timing::record("step", "paint");
            if tui.fail { return Err("paint failed"); }
            tui.paints += 1;
            Ok(())
        }

        fn discard_startup_modal_input(&mut self, tui: &mut tui::Tui) -> Result<()> {
            tui.quarantines += 1;
            Ok(())
        }

@METHODS@
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn configured_app() -> App {
            App { primary_session_configured: Some(()), ..Default::default() }
        }

        #[test]
        fn waits_for_configuration() {
            let mut app = App::default();
            let mut tui = tui::Tui::default();
            let events = mpsc::UnboundedReceiver::new(true);
            let mut pending = true;
            assert!(!app.render_pending_startup_handoff(&mut tui, &events, &mut pending).unwrap());
            assert!(pending);
            assert_eq!(tui.paints, 0);
        }

        #[test]
        fn waits_for_finished_thread_start() {
            let mut app = configured_app();
            app.pending_startup_thread_start = true;
            let mut tui = tui::Tui::default();
            let events = mpsc::UnboundedReceiver::new(true);
            let mut pending = true;
            assert!(!app.render_pending_startup_handoff(&mut tui, &events, &mut pending).unwrap());
            assert!(pending);
            assert_eq!(tui.paints, 0);
        }

        #[test]
        fn preserves_queued_history() {
            let mut app = configured_app();
            let mut tui = tui::Tui { owned: true, ..Default::default() };
            let events = mpsc::UnboundedReceiver::new(false);
            let mut pending = true;
            assert!(!app.render_pending_startup_handoff(&mut tui, &events, &mut pending).unwrap());
            assert!(pending);
            assert_eq!(tui.paints, 0);
        }

        #[test]
        fn paints_once_without_input_or_scheduler() {
            let mut app = configured_app();
            let mut tui = tui::Tui::default();
            let events = mpsc::UnboundedReceiver::new(true);
            let mut pending = true;
            assert!(app.render_pending_startup_handoff(&mut tui, &events, &mut pending).unwrap());
            assert!(!pending);
            assert_eq!(tui.paints, 1);
            assert!(!app.render_pending_startup_handoff(&mut tui, &events, &mut pending).unwrap());
            assert_eq!(tui.paints, 1);
            assert_eq!(codex_arg0::startup_timing::take(), vec![
                ("begin", "tui.startup_handoff_frame"),
                ("step", "tick"),
                ("step", "paint"),
                ("milestone", "session_prompt_ready"),
                ("end", "painted"),
                ("milestone", "startup_handoff_frame_rendered"),
            ]);
        }

        #[test]
        fn arrival_between_checks_retains_handoff() {
            let mut app = configured_app();
            let mut tui = tui::Tui { owned: true, ..Default::default() };
            let events = mpsc::UnboundedReceiver::with_incoming_event();
            let mut pending = true;
            assert!(!app.render_pending_startup_handoff(&mut tui, &events, &mut pending).unwrap());
            assert!(pending);
            assert_eq!(tui.paints, 0);
            assert_eq!(codex_arg0::startup_timing::take(), vec![
                ("begin", "tui.startup_handoff_frame"),
                ("end", "skipped"),
            ]);
            assert!(app.render_pending_startup_handoff(&mut tui, &events, &mut pending).unwrap());
            assert!(!pending);
            assert_eq!(tui.paints, 1);
        }

        #[test]
        fn paint_failure_preserves_handoff() {
            let mut app = configured_app();
            let mut tui = tui::Tui { fail: true, ..Default::default() };
            let events = mpsc::UnboundedReceiver::new(true);
            let mut pending = true;
            assert_eq!(
                app.render_pending_startup_handoff(&mut tui, &events, &mut pending),
                Err("paint failed"),
            );
            assert!(pending);
            assert_eq!(tui.paints, 0);
            assert_eq!(codex_arg0::startup_timing::take(), vec![
                ("begin", "tui.startup_handoff_frame"),
                ("step", "tick"),
                ("step", "paint"),
                ("end", "error"),
            ]);
        }

        #[test]
        fn protected_request_is_retained() {
            let mut app = configured_app();
            app.queued_protected = true;
            let mut tui = tui::Tui::default();
            let events = mpsc::UnboundedReceiver::new(true);
            let mut pending = true;
            assert!(!app.render_pending_startup_handoff(&mut tui, &events, &mut pending).unwrap());
            assert!(pending);
            assert!(app.queued_protected);
            assert_eq!(tui.paints, 0);
            assert!(codex_arg0::startup_timing::take().is_empty());
        }

        #[test]
        fn visible_modal_keeps_input_quarantine() {
            let mut app = configured_app();
            app.startup_protected_input_boundary = true;
            app.chat_widget.active_view = true;
            app.chat_widget.active_modal = true;
            let mut tui = tui::Tui { owned: true, ..Default::default() };
            let events = mpsc::UnboundedReceiver::new(true);
            let mut pending = true;
            assert!(app.render_pending_startup_handoff(&mut tui, &events, &mut pending).unwrap());
            assert!(!pending);
            assert_eq!(tui.paints, 1);
            assert_eq!(tui.quarantines, 1);
        }
    }
}
'''


def main() -> None:
    if not SOURCE_PATH.is_file():
        raise SystemExit(f"missing production source: {SOURCE_PATH}")
    production = SOURCE_PATH.read_text()
    methods = "\n".join(method(production, name) for name in (
        "render_startup_frame", "render_pending_startup_handoff"
    ))
    with tempfile.TemporaryDirectory(prefix="trueos-codex-startup-handoff-") as temporary:
        directory = Path(temporary)
        source = directory / "harness.rs"
        source.write_text(HARNESS.replace("@METHODS@", methods))
        binary = directory / "harness"
        subprocess.run([
            "rustc", "--edition=2024", "--test", str(source), "-o", str(binary),
        ], check=True, timeout=60)
        subprocess.run([str(binary)], check=True, timeout=30)


if __name__ == "__main__":
    main()
