//! Native process state shared by the Hull and its logical thread carriers.
//!
//! This page contains native services only. Opening TLS, environment, or a
//! thread handle does not initialize the compatibility descriptor/signal/TLS
//! tables. Dynamic pointees remain in the owning VM's EPT-readable heap.

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU8, Ordering};
use spin::Mutex;

#[path = "../process_environment.rs"]
pub(crate) mod environment;

#[repr(C, align(4096))]
pub(crate) struct State {
    pub(crate) tls: crate::r::native_tls::State,
    pub(crate) environment: Mutex<environment::Environment>,
    pub(crate) threads: crate::r::native_thread::State,
}

impl State {
    const fn new() -> Self {
        Self {
            tls: crate::r::native_tls::State::new(),
            environment: Mutex::new(environment::Environment::new()),
            threads: crate::r::native_thread::State::new(),
        }
    }
}

const _: () = assert!(core::mem::size_of::<State>() == 4096);
const _: () = assert!(core::mem::align_of::<State>() == 4096);
const EMPTY: u8 = 0;
const INITIALIZING: u8 = 1;
const READY: u8 = 2;
static HOST: State = State::new();
static TEMPLATE: State = State::new();

#[repr(C, align(4096))]
struct Storage {
    slots: [UnsafeCell<MaybeUninit<State>>; crate::allcaps::hv::VM_ID_LIMIT],
}

// Published states synchronize access with their own locks and atomics.
unsafe impl Sync for Storage {}

#[unsafe(link_section = ".bss.blueprint_native_states")]
static STATES: Storage = Storage {
    slots: [const { UnsafeCell::new(MaybeUninit::uninit()) }; crate::allcaps::hv::VM_ID_LIMIT],
};
static INITIALIZED: [AtomicU8; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { AtomicU8::new(EMPTY) }; crate::allcaps::hv::VM_ID_LIMIT];

pub(crate) fn for_vm(vm_id: u8) -> Option<&'static State> {
    let slot = STATES.slots.get(vm_id as usize)?.get().cast::<State>();
    let initialized = INITIALIZED.get(vm_id as usize)?;
    loop {
        match initialized.load(Ordering::Acquire) {
            READY => return Some(unsafe { &*slot }),
            EMPTY => {
                if initialized.compare_exchange(EMPTY, INITIALIZING, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                    // A single canonical empty image avoids a large stack
                    // temporary and keeps all per-VM slots in NOBITS storage.
                    unsafe { core::ptr::copy_nonoverlapping(core::ptr::addr_of!(TEMPLATE), slot, 1) };
                    initialized.store(READY, Ordering::Release);
                    return Some(unsafe { &*slot });
                }
            }
            INITIALIZING => core::hint::spin_loop(),
            _ => return None,
        }
    }
}

pub(crate) fn current_vm() -> Option<u8> {
    crate::r::threads::current_vm_id().or_else(crate::hv::current_guest_execution_context_vm_id)
}

pub(crate) fn current() -> &'static State {
    current_vm().and_then(for_vm).unwrap_or(&HOST)
}

pub(crate) fn with_current<R>(f: impl FnOnce(&State) -> R) -> Result<R, i32> {
    match current_vm() {
        Some(vm) => {
            let state = for_vm(vm).ok_or(22)?;
            crate::allocators::with_hv_guest_alloc_domain(vm, || f(state)).ok_or(12)
        }
        None => Ok(crate::allocators::with_host_alloc_domain(|| f(&HOST))),
    }
}

pub(crate) fn span(vm_id: u8) -> Option<(u64, usize)> {
    let state = for_vm(vm_id)?;
    Some((state as *const State as u64, core::mem::size_of::<State>()))
}

pub(crate) fn ready_flag(vm_id: u8) -> Option<(u64, u8)> {
    for_vm(vm_id)?;
    Some((INITIALIZED.get(vm_id as usize)? as *const AtomicU8 as u64, READY))
}

/// Called only after the Hull and all native jobs are quiescent, before the
/// VM heap is released. Forced retirement never invokes old guest callbacks.
pub(crate) fn reset(vm_id: u8) {
    let Some(initialized) = INITIALIZED.get(vm_id as usize) else { return; };
    if initialized.load(Ordering::Acquire) != READY { return; }
    let state = unsafe { &*STATES.slots[vm_id as usize].get().cast::<State>() };
    state.tls.reset();
    state.threads.reset();
    *state.environment.lock() = environment::Environment::new();
}

pub(crate) fn initialize_environment<'a>(vm_id: u8, initial: impl Iterator<Item = (&'a [u8], &'a [u8])>) -> Result<(), i32> {
    let state = for_vm(vm_id).ok_or(22)?;
    crate::allocators::with_hv_guest_alloc_domain(vm_id, || {
        let mut next = environment::Environment::new();
        next.initialize(initial)?;
        *state.environment.lock() = next;
        Ok(())
    }).ok_or(12)?
}

pub(crate) fn with_environment<R>(vm_id: Option<u8>, f: impl FnOnce(&mut environment::Environment) -> R) -> Result<R, i32> {
    let state = match vm_id { Some(vm) => for_vm(vm).ok_or(22)?, None => &HOST };
    let mut env = state.environment.lock();
    let initial = if env.initialized() { None } else {
        // Guest code must never follow host-owned launch-map pointers.
        if vm_id.is_some() { return Err(22); }
        Some(crate::r::io::env::initial_snapshot())
    };
    let operate = || {
        if let Some(initial) = initial {
            env.initialize(initial.iter().map(|(key, value)| (key.as_bytes(), value.as_bytes())))?;
        }
        Ok(f(&mut env))
    };
    match vm_id {
        Some(vm) => crate::allocators::with_hv_guest_alloc_domain(vm, operate).ok_or(12)?,
        None => crate::allocators::with_host_alloc_domain(operate),
    }
}

pub(crate) fn environment_bytes(vm_id: Option<u8>, key: &[u8]) -> Option<alloc::vec::Vec<u8>> {
    with_environment(vm_id, |env| env.get(key).map(<[u8]>::to_vec)).ok().flatten()
}

pub(crate) fn environment_pairs(vm_id: u8) -> Option<alloc::vec::Vec<(alloc::vec::Vec<u8>, alloc::vec::Vec<u8>)>> {
    with_environment(Some(vm_id), |env| env.pairs().map(|(key, value)| (key.to_vec(), value.to_vec())).collect()).ok()
}

#[cfg(feature = "legacy_vm_checkpoint")]
pub(crate) fn refresh_reserved_environment(vm_id: u8, key: &str, value: &str) {
    let Some(state) = for_vm(vm_id) else { return; };
    let mut env = state.environment.lock();
    if !env.initialized() { return; }
    let result = crate::allocators::with_hv_guest_alloc_domain(vm_id,
        || env.refresh_reserved(key.as_bytes(), value.as_bytes()));
    if !matches!(result, Some(Ok(()))) {
        crate::log_important!(target: "hv"; "native environment reserved refresh failed vm={}\n", vm_id);
    }
}
