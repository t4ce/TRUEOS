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
pub(crate) const DESTRUCTOR_ROUNDS: usize = 4;

#[derive(Clone, Copy)]
struct Value {
    pointer: usize,
    destructor: Option<Destructor>,
}

struct Execution {
    values: BTreeMap<usize, Value>,
    cleanups: Vec<Cleanup>,
    cleanups_run: usize,
    cleanup_running: bool,
}

impl Execution {
    fn new() -> Self {
        Self {
            values: BTreeMap::new(), cleanups: Vec::new(),
            cleanups_run: 0, cleanup_running: false,
        }
    }
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
        let mut executions = self.executions.lock();
        let execution = executions.entry(owner).or_insert_with(Execution::new);
        // std and native runtime metadata can coexist. Repeated guard enable
        // calls must not schedule the same finalizer more than once.
        if !execution.cleanups.iter().any(|&registered| core::ptr::fn_addr_eq(registered, cleanup)) {
            execution.cleanups.push(cleanup);
        }
        0
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
        {
            let mut executions = self.executions.lock();
            let Some(execution) = executions.get_mut(&owner) else { return; };
            if execution.cleanup_running { return; }
            execution.cleanup_running = true;
        }
        loop {
            let cleanup = {
                let mut executions = self.executions.lock();
                let Some(execution) = executions.get_mut(&owner) else { return; };
                let cleanup = execution.cleanups.get(execution.cleanups_run).copied();
                if cleanup.is_some() {
                    // Publish consumption before entering guest/runtime code.
                    // Keep registered addresses for idempotence during teardown.
                    execution.cleanups_run += 1;
                } else {
                    execution.cleanup_running = false;
                }
                cleanup
            };
            let Some(cleanup) = cleanup else { break; };
            // Finalizers can read/clear their metadata, register a different
            // finalizer, or drop owned context without holding the TLS lock.
            unsafe { cleanup() };
        }
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
    crate::r::native_process::current().tls.get(current_identity(), key)
}

fn mutate_current_state(f: impl FnOnce(&State) -> i32) -> i32 {
    crate::r::native_process::with_current(|process| f(&process.tls)).unwrap_or_else(|error| error)
}

/// Tear down values and registered cleanups in the native execution namespace.
pub(crate) fn run_execution_destructors(owner: usize) {
    for _ in 0..DESTRUCTOR_ROUNDS {
        let ran = crate::r::native_process::current().tls.run_destructor_round(owner);
        if !ran { break; }
    }
    let state = &crate::r::native_process::current().tls;
    state.run_cleanup(owner);
    state.retire_owner(owner);
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_thread_local_set(
    key: usize,
    pointer: *mut u8,
    destructor: Option<Destructor>,
) -> i32 {
    mutate_current_state(|state| state.set(current_identity(), key, pointer, destructor))
}

/// Keep runtime metadata alive until all ordinary C/Rust TLS destructor rounds
/// finish. Each distinct callback runs once per execution, in registration
/// order, before identity retirement. A finalizer may append another finalizer.
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_thread_local_register_cleanup(cleanup: Cleanup) -> i32 {
    mutate_current_state(|state| state.register_cleanup(current_identity(), cleanup))
}
