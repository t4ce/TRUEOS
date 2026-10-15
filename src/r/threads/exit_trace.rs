//! Bounded cooperative slice-exit call stacks, not interrupted-PC samples.
//! Capture runs on the live child stack without allocation, locks or logging.
//! Only validated executable return addresses leave the fixed snapshot buffer.

pub(super) const MAX_FRAMES: usize = 12;
const SLOW_MS: u64 = 100;
const PERIOD_MS: u64 = 1_000;
pub(super) type Ranges = [(usize, usize); 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reason { Yield, Sleep, Wait, Complete }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stop { End, Bounds, Unaligned, NonIncreasing, NonExecutable, Limit }

#[derive(Clone, Copy, Debug)]
pub(super) struct Frames {
    pub addresses: [usize; MAX_FRAMES],
    pub count: usize,
    pub stop: Stop,
}

/// `low..high` must be a live readable stack mapping exclusively executing on
/// this carrier. Checks precede *every* two-word frame read. RSP constrains the
/// walk to the active portion of that real stack; monotonic links reject loops.
/// Corrupt or non-executable words are never emitted as frame addresses.
pub(super) unsafe fn walk(low: usize, high: usize, rsp: usize, mut rbp: usize, executable: Ranges) -> Frames {
    let mut result = Frames { addresses: [0; MAX_FRAMES], count: 0, stop: Stop::Bounds };
    if low >= high || rsp < low || rsp >= high { return result; }
    loop {
        if rbp < rsp || rbp < low || rbp.checked_add(2 * core::mem::size_of::<usize>()).is_none_or(|end| end > high) {
            result.stop = Stop::Bounds; break;
        }
        if rbp % core::mem::align_of::<usize>() != 0 {
            result.stop = Stop::Unaligned; break;
        }
        let frame = rbp as *const usize;
        let (next, rip) = unsafe { (frame.read(), frame.add(1).read()) };
        if !executable.iter().any(|&(start, end)| start <= rip && rip < end) {
            result.stop = Stop::NonExecutable; break;
        }
        result.addresses[result.count] = rip;
        result.count += 1;
        if next == 0 { result.stop = Stop::End; break; }
        if next <= rbp { result.stop = Stop::NonIncreasing; break; }
        if result.count == MAX_FRAMES { result.stop = Stop::Limit; break; }
        rbp = next;
    }
    result
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Snapshot {
    pub ended_ms: u64,
    pub duration_ms: u64,
    pub reason: Reason,
    pub frames: Frames,
}

pub(super) struct State {
    executable: Ranges,
    started_ms: Option<u64>,
    last_emitted_ms: Option<u64>,
    pub last_slice_ms: u64,
    pending: Option<Snapshot>,
}

impl State {
    pub fn new() -> Self {
        Self { executable: [(0, 0); 3], started_ms: None, last_emitted_ms: None, last_slice_ms: 0, pending: None }
    }
    /// Parent-side setup only: executable-range discovery may acquire a lock.
    pub fn configure(&mut self, vm: Option<u8>) {
        #[cfg(not(target_os = "linux"))]
        {
            unsafe extern "C" { static __text_start: u8; static __text_end: u8; }
            let hull = crate::hv::guest::hull_image_layout();
            let blueprint = vm.and_then(crate::hv::memory::active_guest_rel_exec_range_for_vm).unwrap_or((0, 0));
            self.executable = [
                (core::ptr::addr_of!(__text_start) as usize, core::ptr::addr_of!(__text_end) as usize),
                (hull.text_start as usize, hull.text_end as usize),
                (blueprint.0 as usize, blueprint.1 as usize),
            ];
        }
        #[cfg(target_os = "linux")]
        {
            let _ = vm;
            unsafe extern "C" { static __executable_start: u8; static etext: u8; }
            self.executable = [(core::ptr::addr_of!(__executable_start) as usize, core::ptr::addr_of!(etext) as usize), (0, 0), (0, 0)];
        }
    }
    pub fn begin(&mut self, now_ms: u64) { self.started_ms = Some(now_ms); }
    pub fn due(&self, now_ms: u64) -> bool {
        self.started_ms.is_some_and(|start| now_ms.saturating_sub(start) > SLOW_MS)
            && self.last_emitted_ms.is_none_or(|last| now_ms.saturating_sub(last) >= PERIOD_MS)
    }
    /// No I/O here. Called just before the child switches to its parent.
    pub unsafe fn capture(&mut self, now_ms: u64, reason: Reason, low: usize, high: usize, rsp: usize, rbp: usize) {
        let due = self.due(now_ms);
        self.last_slice_ms = self.started_ms.take().map_or(0, |start| now_ms.saturating_sub(start));
        if !due { return; }
        let frames = unsafe { walk(low, high, rsp, rbp, self.executable) };
        self.pending = Some(Snapshot { ended_ms: now_ms, duration_ms: self.last_slice_ms, reason, frames });
    }
    /// Parent-side consumption records the actual emission boundary, so slow
    /// context restoration cannot compress two capture timestamps into a burst.
    pub fn take(&mut self, emitted_ms: u64) -> Option<Snapshot> {
        let trace = self.pending.take()?;
        self.last_emitted_ms = Some(emitted_ms);
        Some(trace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ([usize; 8], Ranges) { ([0; 8], [(0x1000, 0x2000), (0, 0), (0, 0)]) }

    #[test]
    fn walks_only_valid_frames_and_stops_at_terminator() {
        let (mut stack, ranges) = sample();
        let low = stack.as_mut_ptr() as usize;
        stack[0] = low + 16; stack[1] = 0x1100;
        stack[2] = low + 32; stack[3] = 0x1200;
        stack[4] = 0; stack[5] = 0x1300;
        core::hint::black_box(&stack);
        let frames = unsafe { walk(low, low + 64, low, low, ranges) };
        assert_eq!(&frames.addresses[..frames.count], &[0x1100, 0x1200, 0x1300]);
        assert_eq!(frames.stop, Stop::End);
    }

    #[test]
    fn rejects_oob_overflow_unaligned_and_dead_stack_portion_before_read() {
        let (stack, ranges) = sample(); let low = stack.as_ptr() as usize;
        for (rsp, rbp, stop) in [(low, low + 56, Stop::Bounds), (low, usize::MAX - 7, Stop::Bounds), (low, low + 1, Stop::Unaligned), (low + 16, low, Stop::Bounds), (low - 1, low, Stop::Bounds)] {
            let frames = unsafe { walk(low, low + 64, rsp, rbp, ranges) };
            assert_eq!(frames.count, 0); assert_eq!(frames.stop, stop);
        }
    }

    #[test]
    fn rejects_cycles_and_payload_words_and_caps_depth() {
        let (mut stack, ranges) = sample(); let low = stack.as_mut_ptr() as usize;
        stack[0] = low; stack[1] = 0x1100;
        let frames = unsafe { walk(low, low + 64, low, low, ranges) };
        assert_eq!(frames.count, 1); assert_eq!(frames.stop, Stop::NonIncreasing);
        stack[0] = 0; stack[1] = 0x50524f4d5054;
        core::hint::black_box(&stack);
        let frames = unsafe { walk(low, low + 64, low, low, ranges) };
        assert_eq!(frames.count, 0); assert_eq!(frames.stop, Stop::NonExecutable);
        let mut long = [0usize; 2 * (MAX_FRAMES + 2)]; let low = long.as_mut_ptr() as usize;
        for i in 0..MAX_FRAMES + 2 { long[i * 2] = low + (i + 1) * 16; long[i * 2 + 1] = 0x1100; }
        let frames = unsafe { walk(low, low + core::mem::size_of_val(&long), low, low, ranges) };
        assert_eq!(frames.count, MAX_FRAMES); assert_eq!(frames.stop, Stop::Limit);
    }

    #[test]
    fn records_each_slice_duration_but_limits_trace_to_one_per_second() {
        let (mut stack, ranges) = sample(); let low = stack.as_mut_ptr() as usize;
        stack[1] = 0x1100;
        core::hint::black_box(&stack);
        let mut state = State::new(); state.executable = ranges;
        state.begin(10);
        unsafe { state.capture(110, Reason::Yield, low, low + 64, low, low); }
        assert_eq!(state.last_slice_ms, 100); assert!(state.take(110).is_none());
        state.begin(110);
        unsafe { state.capture(211, Reason::Wait, low, low + 64, low, low); }
        let trace = state.take(211).unwrap();
        assert_eq!((trace.duration_ms, trace.ended_ms, trace.reason), (101, 211, Reason::Wait));
        assert_eq!(trace.frames.count, 1); assert!(state.take(211).is_none());
        state.begin(211);
        unsafe { state.capture(1000, Reason::Sleep, low, low + 64, low, low); }
        assert_eq!(state.last_slice_ms, 789); assert!(state.take(1000).is_none());
        state.begin(1000);
        unsafe { state.capture(1211, Reason::Complete, low, low + 64, low, low); }
        assert_eq!(state.take(1500).unwrap().reason, Reason::Complete);
        state.begin(1500);
        unsafe { state.capture(2400, Reason::Wait, low, low + 64, low, low); }
        assert!(state.take(2400).is_none(), "rate limit uses parent emission, not child capture timestamp");
        state.begin(2000);
        unsafe { state.capture(1999, Reason::Yield, low, low + 64, low, low); }
        assert_eq!(state.last_slice_ms, 0); assert!(state.take(1999).is_none());
    }
}
