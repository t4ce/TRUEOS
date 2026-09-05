//! Admission and lifetime of guest code running on native service lanes.
//!
//! Closing and reserving share one lock. A reservation covers both queued and
//! running closures, including their destruction in the guest allocation realm.

use spin::Mutex;

#[derive(Default)]
struct State {
    generation: u64,
    accepting: bool,
    in_flight: usize,
}

impl State {
    const fn new() -> Self {
        Self {
            generation: 0,
            accepting: false,
            in_flight: 0,
        }
    }

    fn open(&mut self, generation: u64) -> bool {
        if self.in_flight != 0 || generation == 0 {
            return false;
        }
        self.generation = generation;
        self.accepting = true;
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
        self.in_flight
    }

    fn release(&mut self, generation: u64) {
        assert_eq!(self.generation, generation, "native job outlived its VM generation");
        assert!(self.in_flight != 0, "native job reservation released twice");
        self.in_flight -= 1;
    }
}

static STATES: [Mutex<State>; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { Mutex::new(State::new()) }; crate::allcaps::hv::VM_ID_LIMIT];

pub(super) struct GuestJobOwner {
    vm_id: u8,
    generation: u64,
}

pub(super) fn reserve(vm_id: u8) -> Option<GuestJobOwner> {
    let generation = crate::hv::vm_run_generation(vm_id)?;
    let reserved = STATES.get(vm_id as usize)?.lock().reserve(generation);
    if !reserved {
        return None;
    }
    // Construct the RAII token only after reserving and releasing the lock.
    // Eager then_some(token) would drop a non-reservation on the failure path.
    Some(GuestJobOwner { vm_id, generation })
}

impl Drop for GuestJobOwner {
    fn drop(&mut self) {
        STATES[self.vm_id as usize].lock().release(self.generation);
    }
}

pub(crate) fn open_guest_jobs(vm_id: u8, generation: u64) -> bool {
    STATES
        .get(vm_id as usize)
        .is_some_and(|state| state.lock().open(generation))
}

/// Stop accepting work and return the number of outstanding reservations.
pub(crate) fn close_guest_jobs(vm_id: u8) -> usize {
    STATES
        .get(vm_id as usize)
        .map_or(0, |state| state.lock().close())
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

/// Never free executable pages, heap, or process resources on a timeout. Jobs
/// are finite/cooperative in v1; an unfinished job keeps teardown pending.
pub(crate) async fn drain_guest_jobs(vm_id: u8) {
    let count = close_guest_jobs(vm_id);
    if count != 0 {
        crate::log_warn!(target: "service";
            "native-worker: draining vm={} jobs={} resources=retained\n", vm_id, count);
    }
    while guest_jobs_in_flight(vm_id) != 0 {
        trueos_time::Timer::after(trueos_time::Duration::from_millis(1)).await;
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
        state.release(1);
        assert!(state.open(2));
        assert!(!state.reserve(1));
        assert!(state.reserve(2));
        state.release(2);
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
        state.release(2); // rollback when no physical lane can be leased
        assert_eq!(state.close(), 0);
    }
}
