use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum KernelTaskDomain {
    Unknown = 0,
    HostKernel = 1,
    HostService = 2,
    Ui2Service = 3,
    NetService = 4,
    GfxService = 5,
    ComputeWorker = 6,
    TokioCarrier = 7,
    VmRun = 8,
    VmBroker = 9,
    VmGuestOwnedAlloc = 10,
}

impl KernelTaskDomain {
    #[inline]
    fn from_raw(raw: u32) -> Self {
        match raw {
            1 => Self::HostKernel,
            2 => Self::HostService,
            3 => Self::Ui2Service,
            4 => Self::NetService,
            5 => Self::GfxService,
            6 => Self::ComputeWorker,
            7 => Self::TokioCarrier,
            8 => Self::VmRun,
            9 => Self::VmBroker,
            10 => Self::VmGuestOwnedAlloc,
            _ => Self::Unknown,
        }
    }

    #[inline]
    fn raw(self) -> u32 {
        self as u32
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct KernelTaskDomainSnapshot {
    pub domain: KernelTaskDomain,
    pub vm_id: Option<u8>,
    pub cpu_slot: u32,
}

pub struct KernelTaskDomainGuard {
    slot: Option<usize>,
    previous_domain: u32,
    previous_vm_tag: u8,
}

const SLOT_LIMIT: usize = crate::allcaps::hv::VM_CPU_SLOT_LIMIT;
const NO_VM_TAG: u8 = u8::MAX;

static DOMAIN_BY_CPU: [AtomicU32; SLOT_LIMIT] =
    [const { AtomicU32::new(KernelTaskDomain::Unknown as u32) }; SLOT_LIMIT];
static VM_TAG_BY_CPU: [AtomicU8; SLOT_LIMIT] = [const { AtomicU8::new(NO_VM_TAG) }; SLOT_LIMIT];

#[inline]
fn current_slot() -> Option<usize> {
    let slot = crate::percpu::current_slot_via_cpuid();
    if slot < SLOT_LIMIT { Some(slot) } else { None }
}

#[inline]
fn vm_tag(vm_id: Option<u8>) -> u8 {
    vm_id
        .filter(|vm_id| (*vm_id as usize) < crate::allcaps::hv::VM_ID_LIMIT)
        .unwrap_or(NO_VM_TAG)
}

#[inline]
fn vm_id_from_tag(tag: u8) -> Option<u8> {
    if (tag as usize) < crate::allcaps::hv::VM_ID_LIMIT {
        Some(tag)
    } else {
        None
    }
}

pub fn enter(domain: KernelTaskDomain, vm_id: Option<u8>) -> KernelTaskDomainGuard {
    let Some(slot) = current_slot() else {
        return KernelTaskDomainGuard {
            slot: None,
            previous_domain: KernelTaskDomain::Unknown.raw(),
            previous_vm_tag: NO_VM_TAG,
        };
    };
    let previous_domain = DOMAIN_BY_CPU[slot].swap(domain.raw(), Ordering::AcqRel);
    let previous_vm_tag = VM_TAG_BY_CPU[slot].swap(vm_tag(vm_id), Ordering::AcqRel);
    KernelTaskDomainGuard {
        slot: Some(slot),
        previous_domain,
        previous_vm_tag,
    }
}

pub fn with<T>(domain: KernelTaskDomain, vm_id: Option<u8>, f: impl FnOnce() -> T) -> T {
    let _guard = enter(domain, vm_id);
    f()
}

pub fn current() -> KernelTaskDomainSnapshot {
    let Some(slot) = current_slot() else {
        return KernelTaskDomainSnapshot {
            domain: KernelTaskDomain::Unknown,
            vm_id: None,
            cpu_slot: u32::MAX,
        };
    };
    KernelTaskDomainSnapshot {
        domain: KernelTaskDomain::from_raw(DOMAIN_BY_CPU[slot].load(Ordering::Acquire)),
        vm_id: vm_id_from_tag(VM_TAG_BY_CPU[slot].load(Ordering::Acquire)),
        cpu_slot: slot as u32,
    }
}

#[inline]
pub fn guest_owned_alloc_vm_id() -> Option<u8> {
    let snapshot = current();
    match snapshot.domain {
        KernelTaskDomain::VmGuestOwnedAlloc => snapshot.vm_id,
        _ => None,
    }
}

impl Drop for KernelTaskDomainGuard {
    fn drop(&mut self) {
        if let Some(slot) = self.slot {
            DOMAIN_BY_CPU[slot].store(self.previous_domain, Ordering::Release);
            VM_TAG_BY_CPU[slot].store(self.previous_vm_tag, Ordering::Release);
        }
    }
}
