//! A six-step, 240 ms retract/reveal. Time sampling skips missed frames.
use super::MetaFmtStr;
use alloc::{vec::Vec, string::String};

const STEP_NS: u64 = 40_000_000;
#[derive(Default)]
pub(super) struct RetractReveal { change: Option<Change> }
struct Change { old: Vec<MetaFmtStr>, target: Vec<MetaFmtStr>, width: usize, started: u64 }
pub(super) struct Frame { pub source: Vec<MetaFmtStr>, pub visible: usize }
fn width(runs: &[MetaFmtStr]) -> usize { runs.iter().map(|r| r.text.chars().count()).sum() }
impl RetractReveal {
    pub fn finish(&mut self) { self.change = None; }
    pub fn start(&mut self, old: &[MetaFmtStr], target: &[MetaFmtStr], now: u64) {
        if old == target { return; }
        let old = self.frame(now).map_or_else(|| old.to_vec(), |f| f.display());
        self.change = Some(Change { width: width(&old).max(width(target)), old, target: target.to_vec(), started: now });
    }
    pub fn frame(&self, now: u64) -> Option<Frame> {
        let change = self.change.as_ref()?;
        let step = now.saturating_sub(change.started) / STEP_NS;
        if step >= 6 { return None; }
        let (runs, visible) = if step <= 3 { (&change.old, 3 - step as usize) }
            else { (&change.target, step as usize - 3) };
        let mut source = Vec::new();
        source.push(MetaFmtStr::new(" ".repeat(change.width.saturating_sub(width(runs)))));
        source.extend_from_slice(runs);
        Some(Frame { source, visible })
    }
}
impl Frame {
    fn display(&self) -> Vec<MetaFmtStr> {
        let hidden = width(&self.source) - width(&self.source) * self.visible / 3;
        let mut remaining = hidden;
        let mut result = Vec::new();
        for run in &self.source {
            let masked = remaining.min(run.text.chars().count());
            if masked != 0 { result.push(MetaFmtStr::new(" ".repeat(masked))); }
            remaining -= masked;
            let text: String = run.text.chars().skip(masked).collect();
            if !text.is_empty() { result.push(MetaFmtStr { text, ..run.clone() }); }
        }
        result
    }
}
