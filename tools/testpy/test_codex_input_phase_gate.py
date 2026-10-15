#!/usr/bin/env python3
"""Compile the real TRUEOS-only OTel phase gate with a numeric capture sink."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class CodexInputPhaseGateTests(unittest.TestCase):
    def test_trueos_branch_emits_input_subphases_and_rejects_unknown_values(self):
        root = Path(__file__).resolve().parents[3] / 'codex' / 'codex-rs'
        source = root / 'otel/src/events/session_telemetry.rs'
        if not source.is_file() or not shutil.which('rustc'):
            self.skipTest('Codex source tree and rustc are required')
        text = source.read_text()
        start = text.index('    pub fn record_step_context_phase(')
        end = text.index('    /// Records a coarse startup phase', start)
        method = text[start:end]
        harness = r'''
use std::cell::RefCell;
use std::time::Duration;
#[derive(Default)]
struct SessionTelemetry {
    records: RefCell<Vec<(&'static str, u64, &'static str, u128, u128, &'static str)>>,
}
macro_rules! log_and_trace_event {
    ($owner:expr, common: {
        event.name = $event:expr,
        capture_id = $capture:expr,
        phase = $phase:expr,
        offset_ms = %$offset:expr,
        duration_ms = %$duration:expr,
        status = $status:expr,
    }, log: {}, trace: {},) => {
        $owner.records.borrow_mut().push(($event, $capture, $phase, $offset, $duration, $status));
    };
}
impl SessionTelemetry {
''' + method + r'''
}
#[test]
fn input_subphases_are_recorded_without_private_fields() {
    let telemetry = SessionTelemetry::default();
    let phases = ["input_inspect_hooks", "input_record", "hook_transcript_path",
        "hook_transcript_persist", "input_prepare", "input_record_history",
        "input_emit_items", "input_persist", "context_world_state", "context_render",
        "context_record_items", "context_persist_snapshot", "context_checkpoint"];
    for phase in phases {
        for status in ["started", "complete", "cancelled", "error"] {
            telemetry.record_step_context_phase(42, phase,
                Duration::from_millis(3), Duration::from_millis(7), status);
        }
    }
    let records = telemetry.records.borrow();
    assert_eq!(records.len(), phases.len() * 4);
    for record in records.iter() {
        assert_eq!(record.0, "codex.step_context_phase");
        assert_eq!((record.1, record.3, record.4), (42, 3, 7));
    }
}
#[test]
fn unknown_phase_and_status_emit_nothing() {
    let telemetry = SessionTelemetry::default();
    telemetry.record_step_context_phase(42, "PRIVATE_PHASE", Duration::ZERO,
        Duration::ZERO, "complete");
    telemetry.record_step_context_phase(42, "input_record", Duration::ZERO,
        Duration::ZERO, "PRIVATE_STATUS");
    assert!(telemetry.records.borrow().is_empty());
}
'''
        with tempfile.TemporaryDirectory(prefix='codex-input-gate-') as directory:
            directory = Path(directory)
            rust = directory / 'gate.rs'
            executable = directory / 'gate-tests'
            rust.write_text(harness)
            subprocess.run(
                ['rustc', '--edition=2024', '--cfg', 'target_os="trueos"',
                 '-Aexplicit_builtin_cfgs_in_flags', '--test', str(rust), '-o', str(executable)],
                check=True, capture_output=True, text=True,
            )
            result = subprocess.run([str(executable)], check=True, capture_output=True, text=True)
            self.assertIn('2 passed', result.stdout)


if __name__ == '__main__':
    unittest.main()
