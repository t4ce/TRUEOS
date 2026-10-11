//! Typed text collection and exact-slot output for system diagnostics.
use super::{MatrixSlotLease, MatrixSlots};
use alloc::{string::String, vec::Vec};
use spin::Mutex;
#[derive(Clone)]
pub(crate) struct DumpTarget(pub(super) Option<MatrixSlotLease>);
impl DumpTarget {
    pub(crate) fn line(&self, text: &str) {
        match &self.0 {
            Some(lease) => MatrixSlots::echo_output(lease, text.into()),
            None => MatrixSlots::echo(None, None, text.into()),
        }
    }
}
pub(crate) struct Output {
    lines: Mutex<Vec<String>>,
    destination: Option<DumpTarget>,
    width: usize,
}
impl Output {
    pub(super) fn collect(width: usize) -> Self {
        Self {
            lines: Mutex::new(Vec::new()),
            destination: None,
            width,
        }
    }
    pub(super) fn slot(destination: DumpTarget, width: usize) -> Self {
        Self {
            lines: Mutex::new(Vec::new()),
            destination: Some(destination),
            width,
        }
    }
    pub(crate) fn line(&self, text: &str) {
        if let Some(target) = &self.destination {
            target.line(text);
        } else {
            self.lines.lock().push(text.into());
        }
    }
    pub(crate) fn width(&self) -> usize {
        self.width
    }
    pub(crate) fn target(&self) -> DumpTarget {
        self.destination
            .clone()
            .expect("dump output requires a destination")
    }
    pub(super) fn finish(self) -> Vec<String> {
        self.lines.into_inner()
    }
}
pub(crate) fn print_target_line(target: &DumpTarget, text: &str) {
    target.line(text);
}
