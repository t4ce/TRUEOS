//! Whitespace-delimited words change together, with at most four character
//! steps followed by one tail step. Time sampling skips missed frames.
use super::MetaFmtStr;
use alloc::vec::Vec;

const STEP_NS: u64 = 50_000_000;
const CHARACTER_STEPS: usize = 4;
#[derive(Default)]
pub(super) struct TokenSteps { change: Option<Change> }
struct Change {
    old: Vec<MetaFmtStr>, target: Vec<MetaFmtStr>, width: usize,
    started: u64, from_right: bool, out_steps: usize, in_steps: usize,
}
pub(super) struct Frame { pub source: Vec<MetaFmtStr>, pub visible: Vec<bool> }
fn width(runs: &[MetaFmtStr]) -> usize { runs.iter().map(|r| r.text.chars().count()).sum() }
fn steps(runs: &[MetaFmtStr]) -> usize {
    let text: alloc::string::String = runs.iter().map(|run| run.text.as_str()).collect();
    let longest = text.split_whitespace().map(|word| word.chars().count()).max().unwrap_or(0);
    longest.min(CHARACTER_STEPS) + usize::from(longest > CHARACTER_STEPS)
}
impl TokenSteps {
    pub fn finish(&mut self) { self.change = None; }
    pub fn start(&mut self, old: &[MetaFmtStr], target: &[MetaFmtStr], now: u64, from_right: bool) {
        if old == target { return; }
        let old = self.frame(now).map_or_else(|| old.to_vec(), |frame| frame.display());
        self.change = Some(Change { width: width(&old).max(width(target)),
            out_steps: steps(&old), in_steps: steps(target), old, target: target.to_vec(), started: now, from_right });
    }
    pub fn frame(&self, now: u64) -> Option<Frame> {
        let change = self.change.as_ref()?;
        let step = (now.saturating_sub(change.started) / STEP_NS) as usize;
        if step >= change.out_steps + change.in_steps { return None; }
        let (runs, count, removing) = if step <= change.out_steps {
            (&change.old, step, true)
        } else { (&change.target, step - change.out_steps, false) };
        let mut source = Vec::new();
        source.push(MetaFmtStr::new(" ".repeat(change.width.saturating_sub(width(runs)))));
        source.extend_from_slice(runs);
        let chars: Vec<char> = source.iter().flat_map(|run| run.text.chars()).collect();
        let mut visible = alloc::vec![false; chars.len()];
        let mut start = 0;
        while start < chars.len() {
            if chars[start].is_whitespace() { start += 1; continue; }
            let mut end = start + 1;
            while end < chars.len() && !chars[end].is_whitespace() { end += 1; }
            for index in start..end {
                let position = if change.from_right { end - 1 - index } else { index - start };
                let changed = count > CHARACTER_STEPS || position < count;
                visible[index] = if removing { !changed } else { changed };
            }
            start = end;
        }
        Some(Frame { source, visible })
    }
}
impl Frame {
    fn display(&self) -> Vec<MetaFmtStr> {
        let mut index = 0;
        self.source.iter().flat_map(|run| run.text.chars().map(|ch| {
            let cell = if self.visible[index] { MetaFmtStr { text: ch.into(), ..run.clone() } }
                else { MetaFmtStr::new(" ") };
            index += 1;
            cell
        }).collect::<Vec<_>>()).collect()
    }
}
