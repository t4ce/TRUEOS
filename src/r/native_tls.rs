//! Execution-local values keyed by the address of a static Rust key.
//!
//! Keys need no allocation, registration, numeric sequence, or deletion. Values
//! belong to one logical execution in its process-shared state, independently
//! of the physical carrier currently running it.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

pub(crate) type Destructor = unsafe extern "C" fn(*mut u8);
pub(crate) type Cleanup = unsafe extern "C" fn();
const DESTRUCTOR_ROUNDS: usize = 4;

#[derive(Clone, Copy)]
struct Value {
    pointer: usize,
    destructor: Option<Destructor>,
}

struct Execution {
    values: BTreeMap<usize, Value>,
    cleanup: Option<Cleanup>,
}

impl Execution {
    fn new() -> Self { Self { values: BTreeMap::new(), cleanup: None } }
}

pub(crate) struct State {
    executions: Mutex<BTreeMap<usize, Execution>>,
}

impl State {
    pub(crate) const fn new() -> Self {
        Self { executions: Mutex::new(BTreeMap::new()) }
    }

    fn get(&self, owner: usize, key: usize) -> *mut u8 {
        self.executions.lock().get(&owner).and_then(|e| e.values.get(&key))
            .map_or(core::ptr::null_mut(), |v| v.pointer as *mut u8)
    }

    fn set(&self, owner: usize, key: usize, pointer: *mut u8, destructor: Option<Destructor>) -> i32 {
        if owner == 0 || key == 0 { return 22; }
        let mut executions = self.executions.lock();
        if pointer.is_null() {
            if let Some(execution) = executions.get_mut(&owner) { execution.values.remove(&key); }
        } else {
            executions.entry(owner).or_insert_with(Execution::new).values
                .insert(key, Value { pointer: pointer as usize, destructor });
        }
        0
    }

    fn register_cleanup(&self, owner: usize, cleanup: Cleanup) -> i32 {
        if owner == 0 { return 22; }
        self.executions.lock().entry(owner).or_insert_with(Execution::new).cleanup = Some(cleanup);
        0
    }

    pub(crate) fn run_destructors(&self, owner: usize) {
        for _ in 0..DESTRUCTOR_ROUNDS {
            if !self.run_destructor_round(owner) { break; }
        }
        self.run_cleanup(owner);
        self.retire_owner(owner);
    }

    /// One round so native and compatibility destructors can initialize each
    /// other's values before the common execution cleanup finishes.
    pub(crate) fn run_destructor_round(&self, owner: usize) -> bool {
        let keys = {
            let executions = self.executions.lock();
            let Some(execution) = executions.get(&owner) else { return false; };
            execution.values.iter()
                .filter_map(|(&key, value)| value.destructor.map(|_| key))
                .collect::<Vec<_>>()
        };
        let mut ran = false;
        for key in keys {
            // Keep other pending keys readable until their own turn. A
            // preceding destructor can access, replace, or clear them.
            let value = self.executions.lock().get_mut(&owner).and_then(|e| e.values.remove(&key));
            if let Some(Value { pointer, destructor: Some(destructor) }) = value {
                // No native storage lock is held while running user code.
                unsafe { destructor(pointer as *mut u8) };
                ran = true;
            }
        }
        ran
    }

    pub(crate) fn run_cleanup(&self, owner: usize) {
        let cleanup = self.executions.lock().get_mut(&owner).and_then(|e| e.cleanup.take());
        if let Some(cleanup) = cleanup { unsafe { cleanup() }; }
    }

    pub(crate) fn retire_owner(&self, owner: usize) {
        // Destructor-free pointers (including std's current Thread handle)
        // remain available throughout cleanup, then retire with the identity.
        self.executions.lock().remove(&owner);
    }

    /// Called only once all executions in the old VM have stopped. Forced
    /// teardown drops bookkeeping without re-entering retired guest code.
    pub(crate) fn reset(&self) {
        self.executions.lock().clear();
    }
}

/// Stable identity shared with compatibility adapters; carrier changes never
/// make two live logical threads share execution-local values.
pub(crate) fn current_identity() -> usize {
    if let Some(thread_id) = crate::r::threads::current_id() {
        return match crate::r::threads::current_vm_id() {
            Some(vm_id) => 0x4_0000_0000usize
                .saturating_add((vm_id as usize) << 32)
                .saturating_add(thread_id),
            None => 0x5_0000_0000usize.saturating_add(thread_id),
        };
    }
    if let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() {
        return 0x2_0000usize.saturating_add(vm_id as usize);
    }
    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        if let Some(thread_id) = crate::wls::current_blueprint_thread_id() {
            return 0x4_0000_0000usize
                .saturating_add((vm_id as usize) << 32)
                .saturating_add(thread_id as usize);
        }
        let worker_id = crate::wls::current_worker_id().unwrap_or(0);
        return 0x3_0000usize
            .saturating_add((vm_id as usize).saturating_mul(crate::wls::WORKER_SLOT_COUNT))
            .saturating_add(worker_id);
    }
    if let Some(worker_id) = crate::wls::current_worker_id() {
        return 0x1_0000usize.saturating_add(worker_id);
    }
    crate::percpu::current_slot_via_cpuid().saturating_add(1)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_thread_local_get(key: usize) -> *mut u8 {
    crate::std_abi_shim::native_tls_state().get(current_identity(), key)
}

fn mutate_current_state(f: impl FnOnce(&State) -> i32) -> i32 {
    let state = crate::std_abi_shim::native_tls_state();
    match crate::r::threads::current_vm_id()
        .or_else(crate::hv::current_guest_execution_context_vm_id)
    {
        // Shared map nodes must be visible in both the Hull and its host
        // carriers. An ambient host allocation would leave an EPT-inaccessible
        // pointer in this otherwise process-shared state.
        Some(vm_id) => crate::allocators::with_hv_guest_alloc_domain(vm_id, || f(state)).unwrap_or(12),
        None => f(state),
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_thread_local_set(
    key: usize,
    pointer: *mut u8,
    destructor: Option<Destructor>,
) -> i32 {
    mutate_current_state(|state| state.set(current_identity(), key, pointer, destructor))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_thread_local_register_cleanup(cleanup: Cleanup) -> i32 {
    mutate_current_state(|state| state.register_cleanup(current_identity(), cleanup))
}
