//! Admission and lifetime of guest code running on native service lanes.
//!
//! Closing and reserving share one lock. A reservation covers both queued and
//! running closures, including their destruction in the guest allocation realm.

use spin::Mutex;

#[derive(Default)]
struct State {
    generation: u64,
    accepting: bool,
    paused: bool,
    in_flight: usize,
    parked: usize,
    retiring: usize,
    hull_stopped: bool,
}

impl State {
    const fn new() -> Self {
        Self {
            generation: 0,
            accepting: false,
            paused: false,
            in_flight: 0,
            parked: 0,
            retiring: 0,
            hull_stopped: false,
        }
    }

    fn open(&mut self, generation: u64) -> bool {
        if self.in_flight != 0 || self.paused || generation == 0 {
            return false;
        }
        self.generation = generation;
        self.accepting = true;
        self.hull_stopped = false;
        true
    }

    fn reserve(&mut self, generation: u64) -> bool {
        if !self.accepting || self.generation != generation {
            return false;
        }
        let Some(count) = self.in_flight.checked_add(1) else {
            return false;
        };
        self.in_flight = count;
        true
    }

    fn close(&mut self) -> usize {
        self.accepting = false;
        self.paused = false;
        self.in_flight
    }

    fn closed(&self) -> bool {
        !self.accepting && !self.paused
    }

    fn pause(&mut self) -> usize {
        if self.accepting {
            self.accepting = false;
            self.paused = true;
        }
        self.in_flight
    }

    fn resume(&mut self, generation: u64) -> bool {
        if !self.paused || generation == 0 || self.generation != generation {
            return false;
        }
        self.paused = false;
        self.accepting = true;
        true
    }

    fn park(&mut self, generation: u64, parked: &mut bool) -> bool {
        assert_eq!(self.generation, generation, "native job outlived its VM generation");
        if !self.paused {
            return false;
        }
        if !*parked {
            assert!(self.parked < self.in_flight, "native job parked without a reservation");
            self.parked += 1;
            *parked = true;
        }
        true
    }

    fn unpark(&mut self, generation: u64, parked: &mut bool) {
        assert_eq!(self.generation, generation, "native job outlived its VM generation");
        if *parked {
            assert!(self.parked != 0, "native job unparked twice");
            self.parked -= 1;
            *parked = false;
        }
    }

    fn retire(&mut self, generation: u64, retiring: &mut bool) {
        assert_eq!(self.generation, generation, "native job outlived its VM generation");
        if !*retiring {
            assert!(self.retiring < self.in_flight, "native job retired without a reservation");
            self.retiring += 1;
            *retiring = true;
        }
    }

    fn acknowledge_hull_stopped(&mut self, generation: u64) -> bool {
        if generation == 0 || self.generation != generation {
            return false;
        }
        self.hull_stopped = true;
        true
    }

    fn reclaimable(&self, generation: u64) -> bool {
        generation != 0
            && self.generation == generation
            && self.closed()
            && self.hull_stopped
            && self.in_flight == self.retiring
    }

    fn release(&mut self, generation: u64, parked: &mut bool, retiring: bool) {
        assert_eq!(self.generation, generation, "native job outlived its VM generation");
        assert!(self.in_flight != 0, "native job reservation released twice");
        self.unpark(generation, parked);
        if retiring {
            assert!(self.retiring != 0, "native job retirement released twice");
            self.retiring -= 1;
        }
        self.in_flight -= 1;
    }
}

static STATES: [Mutex<State>; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { Mutex::new(State::new()) }; crate::allcaps::hv::VM_ID_LIMIT];

pub(crate) struct GuestJobOwner {
    vm_id: u8,
    generation: u64,
    parked: bool,
    retiring: bool,
    pub(crate) diagnostic: super::diagnostics::JobDiagnostic,
}

pub(crate) fn reserve(vm_id: u8) -> Option<GuestJobOwner> {
    let generation = crate::hv::vm_run_generation(vm_id)?;
    let reserved = STATES.get(vm_id as usize)?.lock().reserve(generation);
    if !reserved {
        return None;
    }
    // Construct the RAII token only after reserving and releasing the lock.
    // Eager then_some(token) would drop a non-reservation on the failure path.
    Some(GuestJobOwner {
        vm_id, generation, parked: false, retiring: false,
        diagnostic: super::diagnostics::JobDiagnostic::new(vm_id, generation),
    })
}

impl GuestJobOwner {
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn matches(&self, vm_id: u8, generation: u64) -> bool {
        self.vm_id == vm_id && self.generation == generation
    }

    pub(super) fn kill_requested(&self) -> bool {
        crate::hv::vm_run_generation(self.vm_id) == Some(self.generation)
            && crate::hv::guest_kill_requested(self.vm_id)
    }

    /// Mark this stopped continuation without releasing its reservation or
    /// stack. Peers can still hold scoped borrows into that stack.
    pub(crate) fn retire(&mut self) {
        let reclaimable = {
            let mut state = STATES[self.vm_id as usize].lock();
            state.retire(self.generation, &mut self.retiring);
            state.reclaimable(self.generation)
        };
        wake_reclaimable(self.vm_id, reclaimable);
    }

    pub(crate) fn reclaimable(&self) -> bool {
        guest_jobs_reclaimable(self.vm_id, self.generation)
    }

    /// Only the scheduler may count a continuation, after its guest stack has
    /// returned to the host. Ordinary wait diagnostics are not a checkpoint.
    pub(crate) fn park(&mut self) -> bool {
        STATES[self.vm_id as usize].lock().park(self.generation, &mut self.parked)
    }

    pub(crate) fn unpark(&mut self) {
        STATES[self.vm_id as usize].lock().unpark(self.generation, &mut self.parked);
    }
}

impl Drop for GuestJobOwner {
    fn drop(&mut self) {
        let reclaimable = {
            let mut state = STATES[self.vm_id as usize].lock();
            state.release(self.generation, &mut self.parked, self.retiring);
            state.reclaimable(self.generation)
        };
        wake_reclaimable(self.vm_id, reclaimable);
    }
}

pub(crate) fn open_guest_jobs(vm_id: u8, generation: u64) -> bool {
    STATES
        .get(vm_id as usize)
        .is_some_and(|state| state.lock().open(generation))
}

/// Stop accepting work and return the number of outstanding reservations.
pub(crate) fn close_guest_jobs(vm_id: u8) -> usize {
    let Some(state) = STATES.get(vm_id as usize) else { return 0; };
    let (count, reclaimable) = {
        let mut state = state.lock();
        (state.close(), state.reclaimable(state.generation))
    };
    wake_reclaimable(vm_id, reclaimable);
    count
}

fn wake_reclaimable(vm_id: u8, reclaimable: bool) {
    if reclaimable {
        // Never take the control-waker lock while holding a lifetime lock.
        crate::r::threads::wake_guest_control(vm_id);
    }
}

/// The Hull launch future has returned; no Hull frame can borrow a native
/// continuation's stack. A stale VM incarnation cannot open this barrier.
pub(crate) fn acknowledge_guest_hull_stopped(vm_id: u8, generation: u64) -> bool {
    let Some(state) = STATES.get(vm_id as usize) else { return false; };
    let (matched, reclaimable) = {
        let mut state = state.lock();
        let matched = state.acknowledge_hull_stopped(generation);
        (matched, state.reclaimable(generation))
    };
    wake_reclaimable(vm_id, reclaimable);
    matched
}

/// Stopped stacks remain backed until the Hull and every accepted native
/// operation are stopped. Real reservations are released by their owners.
pub(crate) fn guest_jobs_reclaimable(vm_id: u8, generation: u64) -> bool {
    STATES.get(vm_id as usize).is_some_and(|state| state.lock().reclaimable(generation))
}

pub(crate) fn guest_jobs_retiring(vm_id: u8) -> usize {
    STATES.get(vm_id as usize).map_or(0, |state| state.lock().retiring)
}

/// Close admission while retaining existing native continuations in RAM.
pub(crate) fn pause_guest_jobs(vm_id: u8) -> usize {
    STATES.get(vm_id as usize).map_or(0, |state| state.lock().pause())
}

/// Resume the same VM generation. An irreversible close cannot be reopened.
pub(crate) fn resume_guest_jobs(vm_id: u8, generation: u64) -> bool {
    STATES.get(vm_id as usize).is_some_and(|state| state.lock().resume(generation))
}

pub(crate) fn guest_jobs_quiescent(vm_id: u8) -> bool {
    STATES.get(vm_id as usize).is_some_and(|state| {
        let state = state.lock();
        state.paused && state.parked == state.in_flight
    })
}

pub(crate) fn guest_jobs_in_flight(vm_id: u8) -> usize {
    STATES
        .get(vm_id as usize)
        .map_or(0, |state| state.lock().in_flight)
}

pub(super) fn accepts_guest_jobs(vm_id: u8) -> bool {
    STATES
        .get(vm_id as usize)
        .is_some_and(|state| state.lock().accepting)
}

/// Pause only prevents new admission; accepted finite work must finish its
/// current operation before the Hull can acknowledge a checkpoint.
pub(super) fn guest_jobs_closed(vm_id: u8) -> bool {
    STATES.get(vm_id as usize).is_none_or(|state| state.lock().closed())
}

/// Never free executable pages, heap, or process resources on a timeout. Jobs
/// are finite/cooperative in v1; an unfinished job keeps teardown pending.
pub(crate) async fn drain_guest_jobs(vm_id: u8) {
    let count = close_guest_jobs(vm_id);
    if count != 0 {
        crate::log_warn!(target: "service";
            "native-worker: draining vm={} jobs={} resources=retained\n", vm_id, count);
        super::diagnostics::report(vm_id);
    }
    let mut last_report_ms = super::now_ms();
    while guest_jobs_in_flight(vm_id) != 0 {
        trueos_time::Timer::after(trueos_time::Duration::from_millis(1)).await;
        let now_ms = super::now_ms();
        if now_ms.saturating_sub(last_report_ms) >= 5000 {
            last_report_ms = now_ms;
            crate::log_os::blueprint_important_line(format_args!(
                "native-worker: drain pending vm={} jobs={}\n", vm_id, guest_jobs_in_flight(vm_id)));
            super::diagnostics::report(vm_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::State;

    #[test]
    fn admission_before_stop_is_retained_until_release() {
        let mut state = State::new();
        assert!(state.open(1));
        assert!(state.reserve(1));
        assert_eq!(state.close(), 1);
        assert!(!state.reserve(1));
        assert!(!state.open(2));
        state.release(1, &mut false, false);
        assert!(state.open(2));
        assert!(!state.reserve(1));
        assert!(state.reserve(2));
        state.release(2, &mut false, false);
    }

    #[test]
    fn stop_before_admission_and_rejected_enqueue() {
        let mut state = State::new();
        assert!(!state.reserve(1));
        assert!(state.open(1));
        assert_eq!(state.close(), 0);
        assert!(!state.reserve(1));
        assert!(state.open(2));
        assert!(state.reserve(2));
        state.release(2, &mut false, false); // rollback when no physical lane can be leased
        assert_eq!(state.close(), 0);
    }

    #[test]
    fn pause_counts_only_suspended_jobs_and_resumes_the_same_generation() {
        let mut state = State::new();
        assert!(state.open(7));
        assert!(state.reserve(7));
        assert!(state.reserve(7));
        let mut thread_parked = false;
        let mut finite_parked = false;
        assert_eq!(state.pause(), 2);
        assert!(!state.reserve(7));
        assert!(!state.open(8));
        assert!(state.park(7, &mut thread_parked));
        assert!(state.park(7, &mut thread_parked));
        assert_eq!(state.parked, 1, "polling a parked task must not count it twice");
        assert_ne!(state.parked, state.in_flight);
        state.release(7, &mut finite_parked, false);
        assert_eq!(state.parked, state.in_flight);
        assert!(!state.resume(8));
        assert!(state.resume(7));
        assert!(!state.park(7, &mut thread_parked));
        state.unpark(7, &mut thread_parked);
        state.unpark(7, &mut thread_parked);
        assert_eq!(state.parked, 0);
        assert!(state.reserve(7));
        state.release(7, &mut thread_parked, false);
        state.release(7, &mut false, false);
    }

    #[test]
    fn kill_after_pause_retires_parked_reservations_without_reopening() {
        let mut state = State::new();
        assert!(state.open(12));
        assert!(state.reserve(12));
        let mut parked = false;
        assert_eq!(state.pause(), 1);
        assert!(state.park(12, &mut parked));
        assert_eq!(state.close(), 1);
        assert!(!state.resume(12));
        assert!(!state.park(12, &mut parked));
        state.release(12, &mut parked, false);
        assert_eq!(state.parked, 0);
        assert_eq!(state.in_flight, 0);
        assert!(state.open(13));
    }

    #[test]
    fn resume_before_scheduler_acknowledgement_cannot_park_a_live_job() {
        let mut state = State::new();
        assert!(state.open(17));
        assert!(state.reserve(17));
        state.pause();
        assert!(state.resume(17));
        let mut parked = false;
        assert!(!state.park(17, &mut parked));
        assert_eq!(state.parked, 0);
        state.release(17, &mut parked, false);
    }

    #[test]
    fn pause_closes_admission_without_cancelling_already_queued_work() {
        let mut state = State::new();
        assert!(state.open(23));
        assert!(state.reserve(23));
        state.pause();
        assert!(!state.accepting);
        assert!(!state.closed(), "pause cancelled an admitted finite job");
        assert!(!state.reserve(23));
        state.release(23, &mut false, false);
        assert!(state.resume(23));
        assert!(!state.closed());
        state.close();
        assert!(state.closed());
        assert!(!state.resume(23));
    }

    #[test]
    fn cancelled_stacks_wait_for_the_hull_and_every_native_peer() {
        let mut state = State::new();
        assert!(state.open(31));
        assert!(state.reserve(31)); // cancelled thread
        assert!(state.reserve(31)); // still executing service closure
        state.close();
        let mut retiring = false;
        state.retire(31, &mut retiring);
        state.retire(31, &mut retiring);
        assert_eq!(state.retiring, 1, "repeated cancellation counted twice");
        assert!(!state.reclaimable(31));
        assert!(state.acknowledge_hull_stopped(31));
        assert!(!state.reclaimable(31), "running peer still owns stack borrows");
        state.release(31, &mut false, false);
        assert!(state.reclaimable(31));
        state.release(31, &mut false, retiring);
        assert_eq!(state.in_flight, 0);
        assert_eq!(state.retiring, 0);
        assert!(state.open(32));
        state.close();
        assert!(!state.reclaimable(32), "old Hull acknowledgement crossed generations");
        assert!(!state.acknowledge_hull_stopped(31));
        assert!(!state.reclaimable(31));
    }

    #[test]
    fn hull_acknowledgement_does_not_allow_live_or_paused_admission_to_retire_stacks() {
        let mut state = State::new();
        assert!(state.open(37));
        assert!(state.reserve(37));
        let mut retiring = false;
        state.retire(37, &mut retiring); // kill observed before close is published
        assert!(state.acknowledge_hull_stopped(37));
        assert!(!state.reclaimable(37));
        state.pause();
        assert!(!state.reclaimable(37));
        state.close();
        assert!(state.reclaimable(37));
        state.release(37, &mut false, retiring);
    }
}
