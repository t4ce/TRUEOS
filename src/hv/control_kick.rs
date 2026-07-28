//! Exact AP lifecycle-kick transport for VM execution lanes.
//!
//! The sender commits a per-CPU mailbox before raising the dedicated IPI. The
//! interrupt path only records the committed sequence and acknowledges the
//! local APIC. Lifecycle policy and VM task unwinding remain in the VM loop.

use core::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};

use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

pub(crate) const LIFECYCLE_KICK_VECTOR: u8 = 0x42;

const BSP_CPU_SLOT: u32 = 0;
const EMPTY_SEQUENCE: u64 = 0;
const MAILBOX_WRITE_ATTEMPTS: usize = 64;

static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static KICK_REQUESTS: AtomicU64 = AtomicU64::new(0);
static KICK_SENT: AtomicU64 = AtomicU64::new(0);
static KICK_SEND_FAILED: AtomicU64 = AtomicU64::new(0);
static KICK_DELIVERED: AtomicU64 = AtomicU64::new(0);

static MAILBOXES: [LifecycleKickMailbox; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { LifecycleKickMailbox::new() }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum LifecycleKickAction {
    Stop = 1,
    PreserveStop = 2,
    PreservePause = 3,
    /// Force/observe an exact VM exit without changing lifecycle state.
    Nudge = 4,
}

impl LifecycleKickAction {
    fn from_wire(value: u8) -> Option<Self> {
        match value {
            value if value == Self::Stop as u8 => Some(Self::Stop),
            value if value == Self::PreserveStop as u8 => Some(Self::PreserveStop),
            value if value == Self::PreservePause as u8 => Some(Self::PreservePause),
            value if value == Self::Nudge as u8 => Some(Self::Nudge),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LifecycleKick {
    pub sequence: u64,
    pub vm_id: usize,
    pub generation: u64,
    pub action: LifecycleKickAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KickSendError {
    BspTarget,
    CpuSlotOutOfRange,
    VmIdOutOfRange,
    MailboxBusy,
    DeliveryUnavailable,
}

struct LifecycleKickMailbox {
    revision: AtomicU64,
    payload_sequence: AtomicU64,
    published_sequence: AtomicU64,
    vm_id: AtomicU32,
    generation: AtomicU64,
    action: AtomicU8,
    delivered_sequence: AtomicU64,
    consumed_sequence: AtomicU64,
}

impl LifecycleKickMailbox {
    const fn new() -> Self {
        Self {
            revision: AtomicU64::new(0),
            payload_sequence: AtomicU64::new(EMPTY_SEQUENCE),
            published_sequence: AtomicU64::new(EMPTY_SEQUENCE),
            vm_id: AtomicU32::new(0),
            generation: AtomicU64::new(0),
            action: AtomicU8::new(0),
            delivered_sequence: AtomicU64::new(EMPTY_SEQUENCE),
            consumed_sequence: AtomicU64::new(EMPTY_SEQUENCE),
        }
    }

    fn publish(
        &self,
        sequence: u64,
        vm_id: u32,
        generation: u64,
        action: LifecycleKickAction,
    ) -> Result<(), KickSendError> {
        let mut write_revision = None;
        for _ in 0..MAILBOX_WRITE_ATTEMPTS {
            let revision = self.revision.load(Ordering::Acquire);
            if revision & 1 != 0 {
                core::hint::spin_loop();
                continue;
            }
            if self
                .revision
                .compare_exchange_weak(
                    revision,
                    revision.wrapping_add(1),
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                write_revision = Some(revision);
                break;
            }
        }

        let Some(revision) = write_revision else {
            return Err(KickSendError::MailboxBusy);
        };

        self.payload_sequence.store(sequence, Ordering::Relaxed);
        self.vm_id.store(vm_id, Ordering::Relaxed);
        self.generation.store(generation, Ordering::Relaxed);
        self.action.store(action as u8, Ordering::Relaxed);
        self.revision
            .store(revision.wrapping_add(2), Ordering::Release);
        self.published_sequence.store(sequence, Ordering::Release);
        Ok(())
    }

    fn snapshot(&self) -> Option<LifecycleKick> {
        for _ in 0..MAILBOX_WRITE_ATTEMPTS {
            let before = self.revision.load(Ordering::Acquire);
            if before & 1 != 0 {
                core::hint::spin_loop();
                continue;
            }

            let sequence = self.payload_sequence.load(Ordering::Relaxed);
            let vm_id = self.vm_id.load(Ordering::Relaxed);
            let generation = self.generation.load(Ordering::Relaxed);
            let action = self.action.load(Ordering::Relaxed);

            let after = self.revision.load(Ordering::Acquire);
            if before == after && after & 1 == 0 {
                return Some(LifecycleKick {
                    sequence,
                    vm_id: vm_id as usize,
                    generation,
                    action: LifecycleKickAction::from_wire(action)?,
                });
            }
        }
        None
    }
}

pub(crate) fn interrupt_install(idt: &mut InterruptDescriptorTable) {
    idt[LIFECYCLE_KICK_VECTOR].set_handler_fn(lifecycle_kick_isr);
}

/// Publish an exact VM lifecycle request and interrupt its owning AP.
///
/// `generation` is selected by the VM owner and must change whenever a CPU
/// slot or VM ID can be reused. The dedicated mailbox deliberately rejects the
/// BSP so this transport cannot accidentally become a local kernel abort.
pub(crate) fn publish_and_send(
    cpu_slot: u32,
    vm_id: usize,
    generation: u64,
    action: LifecycleKickAction,
) -> Result<u64, KickSendError> {
    KICK_REQUESTS.fetch_add(1, Ordering::Relaxed);

    if cpu_slot == BSP_CPU_SLOT {
        KICK_SEND_FAILED.fetch_add(1, Ordering::Relaxed);
        return Err(KickSendError::BspTarget);
    }
    let Some(mailbox) = MAILBOXES.get(cpu_slot as usize) else {
        KICK_SEND_FAILED.fetch_add(1, Ordering::Relaxed);
        return Err(KickSendError::CpuSlotOutOfRange);
    };
    let Ok(vm_id) = u32::try_from(vm_id) else {
        KICK_SEND_FAILED.fetch_add(1, Ordering::Relaxed);
        return Err(KickSendError::VmIdOutOfRange);
    };

    let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    if let Err(error) = mailbox.publish(sequence, vm_id, generation, action) {
        KICK_SEND_FAILED.fetch_add(1, Ordering::Relaxed);
        return Err(error);
    }

    if !crate::remote_work_wake::send_fixed_x2apic_ipi(cpu_slot, LIFECYCLE_KICK_VECTOR) {
        KICK_SEND_FAILED.fetch_add(1, Ordering::Relaxed);
        return Err(KickSendError::DeliveryUnavailable);
    }

    KICK_SENT.fetch_add(1, Ordering::Relaxed);
    Ok(sequence)
}

/// Record delivery when the kick was intercepted as an external-interrupt
/// VM-exit rather than dispatched through the host IDT.
///
/// Returns `false` without touching APIC state when this is not the dedicated
/// lifecycle vector, allowing the VM-exit handler to dispatch it elsewhere.
pub(crate) fn mark_vmexit_delivery(vector: u8) -> bool {
    if vector != LIFECYCLE_KICK_VECTOR {
        return false;
    }
    record_local_delivery_and_eoi();
    true
}

/// Return the newest delivered request for this CPU without consuming it.
pub(crate) fn pending_for_current_cpu() -> Option<LifecycleKick> {
    let mailbox = MAILBOXES.get(crate::percpu::current_slot())?;
    let delivered = mailbox.delivered_sequence.load(Ordering::Acquire);
    let consumed = mailbox.consumed_sequence.load(Ordering::Acquire);
    if delivered == EMPTY_SEQUENCE || delivered <= consumed {
        return None;
    }

    let kick = mailbox.snapshot()?;
    (kick.sequence == delivered).then_some(kick)
}

/// Consume a request previously returned by [`pending_for_current_cpu`].
pub(crate) fn consume_for_current_cpu(sequence: u64) -> bool {
    let Some(mailbox) = MAILBOXES.get(crate::percpu::current_slot()) else {
        return false;
    };
    if mailbox.delivered_sequence.load(Ordering::Acquire) < sequence {
        return false;
    }

    let mut consumed = mailbox.consumed_sequence.load(Ordering::Acquire);
    loop {
        if consumed >= sequence {
            return false;
        }
        match mailbox.consumed_sequence.compare_exchange_weak(
            consumed,
            sequence,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(observed) => consumed = observed,
        }
    }
}

fn record_local_delivery_and_eoi() {
    if let Some(mailbox) = MAILBOXES.get(crate::percpu::current_slot()) {
        let sequence = mailbox.published_sequence.load(Ordering::Acquire);
        if sequence != EMPTY_SEQUENCE {
            mailbox
                .delivered_sequence
                .fetch_max(sequence, Ordering::AcqRel);
            KICK_DELIVERED.fetch_add(1, Ordering::Relaxed);
        }
    }
    crate::remote_work_wake::local_eoi();
}

extern "x86-interrupt" fn lifecycle_kick_isr(_stack_frame: InterruptStackFrame) {
    record_local_delivery_and_eoi();
}
