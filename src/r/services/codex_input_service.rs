//! One-way whole-message input for an already running Codex TUI.
//!
//! The native TUI advertises its writable root conversation through a VM-local
//! virtual read. A context-menu frame captures that exact receiver; it can
//! neither start an app nor silently follow a later conversation or VM run.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

pub(crate) const VFILE_PREFIX: &str = "vFile:codex-input/";
pub(crate) const MAX_INPUT_BYTES: usize = 16 * 1024;
const READY_LEASE_MS: u64 = 2_000;
const VM_LIMIT: usize = crate::allcaps::hv::VM_ID_LIMIT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Scope {
    New,
    Thread([u8; 16]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Identity {
    vm: u8,
    run: u64,
    instance: [u8; 16],
}

/// Opaque destination captured when the user opens the composer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Target {
    identity: Identity,
    scope: Scope,
    receiver_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SubmitError {
    Unavailable,
    Busy,
    Empty,
    TooLarge,
}

struct Receiver {
    target: Target,
    ready_until_ms: u64,
    pending: Option<Pending>,
}

struct Pending {
    scope: Scope,
    text: String,
}

struct Registry {
    receivers: [Option<Receiver>; VM_LIMIT],
    next_generation: u64,
}

impl Registry {
    const fn new() -> Self {
        Self {
            receivers: [const { None }; VM_LIMIT],
            next_generation: 1,
        }
    }

    fn poll(&mut self, identity: Identity, scope: Scope, now: u64) -> Result<Vec<u8>, i32> {
        let index = identity.vm as usize;
        let Some(slot) = self.receivers.get(index) else {
            return Err(crate::r::cabi_codes::FS_ERR_NOT_FOUND);
        };
        let same_identity = slot
            .as_ref()
            .is_some_and(|receiver| receiver.target.identity == identity);
        let same_scope = same_identity
            && slot
                .as_ref()
                .is_some_and(|receiver| receiver.target.scope == scope);
        let same_receiver = same_scope
            && slot
                .as_ref()
                .is_some_and(|receiver| now < receiver.ready_until_ms);
        if !same_receiver {
            let generation = self.next_generation;
            self.next_generation = generation
                .checked_add(1)
                .ok_or(crate::r::cabi_codes::FS_ERR_NO_SPACE)?;
            // Readiness changes invalidate open frames, but accepted text
            // retains its original conversation until that scope returns.
            let pending = if same_identity {
                self.receivers[index]
                    .as_mut()
                    .and_then(|receiver| receiver.pending.take())
            } else {
                None
            };
            self.receivers[index] = Some(Receiver {
                target: Target {
                    identity,
                    scope,
                    receiver_generation: generation,
                },
                ready_until_ms: now.saturating_add(READY_LEASE_MS),
                pending,
            });
        }
        let receiver = self.receivers[index].as_mut().expect("registered receiver");
        receiver.ready_until_ms = now.saturating_add(READY_LEASE_MS);
        if receiver
            .pending
            .as_ref()
            .is_some_and(|pending| pending.scope == scope)
        {
            Ok(receiver
                .pending
                .take()
                .expect("matching pending input")
                .text
                .into_bytes())
        } else {
            Ok(Vec::new())
        }
    }

    fn offline(&mut self, identity: Identity) {
        if let Some(slot) = self.receivers.get_mut(identity.vm as usize) {
            if let Some(receiver) = slot
                .as_mut()
                .filter(|receiver| receiver.target.identity == identity)
            {
                receiver.ready_until_ms = 0;
            } else {
                *slot = None;
            }
        }
    }

    fn is_live(&self, target: &Target, now: u64) -> bool {
        self.receivers
            .get(target.identity.vm as usize)
            .and_then(Option::as_ref)
            .is_some_and(|receiver| receiver.target == *target && now < receiver.ready_until_ms)
    }

    fn candidates(&self, now: u64) -> [Option<Target>; VM_LIMIT] {
        core::array::from_fn(|index| {
            self.receivers[index]
                .as_ref()
                .and_then(|receiver| (now < receiver.ready_until_ms).then_some(receiver.target))
        })
    }

    fn can_accept(&self, target: &Target, now: u64) -> bool {
        self.is_live(target, now)
            && self.receivers[target.identity.vm as usize]
                .as_ref()
                .is_some_and(|receiver| receiver.pending.is_none())
    }

    fn submit(&mut self, target: &Target, text: &str, now: u64) -> Result<(), SubmitError> {
        if !self.is_live(target, now) {
            return Err(SubmitError::Unavailable);
        }
        if text.trim().is_empty() {
            return Err(SubmitError::Empty);
        }
        if text.len() > MAX_INPUT_BYTES {
            return Err(SubmitError::TooLarge);
        }
        let receiver = self.receivers[target.identity.vm as usize]
            .as_mut()
            .expect("live receiver");
        if receiver.pending.is_some() {
            return Err(SubmitError::Busy);
        }
        receiver.pending = Some(Pending {
            scope: target.scope,
            text: String::from(text),
        });
        Ok(())
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());

fn live_identity(vm: u8) -> Option<Identity> {
    let run = crate::hv::vm_run_generation(vm).filter(|run| *run != 0)?;
    let state = crate::hv::vm_state(vm);
    if !state.supported
        || !state.running
        || state.starting
        || state.stop_requested
        || state.pause_requested
        || state.paused
        || state.restore_inflight
    {
        return None;
    }
    // This archive comes from the host-owned launch template, not guest argv,
    // mutable environment, display names, or an app-supplied registration tag.
    if crate::hv::blueprint_current_run_archive(vm, run)?.as_str() != "codex.bp" {
        return None;
    }
    let instance = crate::hv::blueprint_instance_identity(vm)?.instance;
    (crate::hv::vm_run_generation(vm) == Some(run)).then_some(Identity { vm, run, instance })
}

/// Return the single ready receiver. Multiple Codex instances are deliberately
/// ambiguous: a context click must not choose a user's conversation for them.
pub(crate) fn current_target() -> Option<Target> {
    let now = crate::r::input_timing::now_ms();
    let candidates = REGISTRY.lock().candidates(now);
    let mut selected = None;
    for target in candidates.into_iter().flatten() {
        if live_identity(target.identity.vm) != Some(target.identity) {
            continue;
        }
        if selected.is_some() {
            return None;
        }
        selected = Some(target);
    }
    selected.filter(|target| {
        is_target_live(target)
            && REGISTRY
                .lock()
                .can_accept(target, crate::r::input_timing::now_ms())
    })
}

pub(crate) fn is_target_live(target: &Target) -> bool {
    live_identity(target.identity.vm) == Some(target.identity)
        && REGISTRY
            .lock()
            .is_live(target, crate::r::input_timing::now_ms())
}

pub(crate) fn submit(target: &Target, text: &str) -> Result<(), SubmitError> {
    if live_identity(target.identity.vm) != Some(target.identity) {
        return Err(SubmitError::Unavailable);
    }
    // The retained user text must outlive the calling UI's allocation domain.
    crate::allocators::with_host_alloc_domain_strong(|| {
        REGISTRY
            .lock()
            .submit(target, text, crate::r::input_timing::now_ms())
    })
}

fn parse_scope(value: &str) -> Option<Scope> {
    if value == "new" {
        return Some(Scope::New);
    }
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return None;
    }
    let mut uuid = [0u8; 16];
    let mut nibble = 0usize;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return None;
            }
            continue;
        }
        let value = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return None,
        };
        uuid[nibble / 2] |= value << (4 * (1 - nibble % 2));
        nibble += 1;
    }
    Some(Scope::Thread(uuid))
}

/// Called only after async-FS has reserved a completed result operation. This
/// ordering means operation-capacity errors cannot consume pending user text.
pub(crate) fn read_for_owner(owner: u32, path: &str) -> Result<Vec<u8>, i32> {
    use crate::r::cabi_codes::{FS_ERR_BAD_PARAM, FS_ERR_NOT_FOUND};
    let Some(scope) = path.strip_prefix(VFILE_PREFIX) else {
        return Err(FS_ERR_BAD_PARAM);
    };
    let vm = owner & !0x8000_0000;
    if owner & 0x8000_0000 == 0 || vm > u8::MAX as u32 {
        return Err(FS_ERR_NOT_FOUND);
    }
    crate::allocators::with_host_alloc_domain_strong(|| {
        let identity = live_identity(vm as u8).ok_or(FS_ERR_NOT_FOUND)?;
        if scope == "offline" {
            REGISTRY.lock().offline(identity);
            return Ok(Vec::new());
        }
        let scope = parse_scope(scope).ok_or(FS_ERR_BAD_PARAM)?;
        REGISTRY
            .lock()
            .poll(identity, scope, crate::r::input_timing::now_ms())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(run: u64) -> Identity {
        Identity {
            vm: 0,
            run,
            instance: [run as u8; 16],
        }
    }

    fn target(registry: &Registry, now: u64) -> Target {
        registry.candidates(now)[0].expect("ready receiver")
    }

    #[test]
    fn absent_receiver_cannot_be_opened_or_started() {
        let registry = Registry::new();
        assert!(
            registry
                .candidates(0)
                .into_iter()
                .all(|target| target.is_none())
        );
    }

    #[test]
    fn preserves_one_whole_literal_message_and_consumes_once() {
        let mut registry = Registry::new();
        registry.poll(identity(1), Scope::New, 0).unwrap();
        let target = target(&registry, 0);
        let text = "/literal user text\n!not a shell injection\n§ Lilly 🐈";
        assert_eq!(registry.submit(&target, text, 100), Ok(()));
        assert_eq!(
            registry.poll(identity(1), Scope::New, 200).unwrap(),
            text.as_bytes()
        );
        assert!(
            registry
                .poll(identity(1), Scope::New, 400)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn busy_does_not_replace_an_accepted_message() {
        let mut registry = Registry::new();
        registry.poll(identity(1), Scope::New, 0).unwrap();
        let target = target(&registry, 0);
        assert_eq!(registry.submit(&target, "first", 100), Ok(()));
        assert_eq!(
            registry.submit(&target, "second", 100),
            Err(SubmitError::Busy)
        );
        assert!(!registry.can_accept(&target, 100));
        assert_eq!(
            registry.poll(identity(1), Scope::New, 200).unwrap(),
            b"first"
        );
    }

    #[test]
    fn empty_and_utf8_byte_limit_are_checked_before_queueing() {
        let mut registry = Registry::new();
        registry.poll(identity(1), Scope::New, 0).unwrap();
        let target = target(&registry, 0);
        assert_eq!(
            registry.submit(&target, " \n\t", 100),
            Err(SubmitError::Empty)
        );
        let too_long = "§".repeat(MAX_INPUT_BYTES / 2 + 1);
        assert_eq!(
            registry.submit(&target, &too_long, 100),
            Err(SubmitError::TooLarge)
        );
        let exact = "§".repeat(MAX_INPUT_BYTES / 2);
        assert_eq!(registry.submit(&target, &exact, 100), Ok(()));
        assert_eq!(
            registry.poll(identity(1), Scope::New, 200).unwrap().len(),
            MAX_INPUT_BYTES
        );
    }

    #[test]
    fn conversation_change_never_retargets_an_open_frame_or_old_text() {
        let mut registry = Registry::new();
        registry.poll(identity(1), Scope::New, 0).unwrap();
        let old = target(&registry, 0);
        registry.submit(&old, "old conversation only", 100).unwrap();
        assert!(
            registry
                .poll(identity(1), Scope::Thread([4; 16]), 200)
                .unwrap()
                .is_empty()
        );
        assert!(!registry.is_live(&old, 200));
        assert_eq!(
            registry.submit(&old, "must not redirect", 200),
            Err(SubmitError::Unavailable)
        );
        let new_scope = registry.receivers[0].as_ref().unwrap().target;
        assert!(!registry.can_accept(&new_scope, 200));
        assert_ne!(new_scope, old);
        assert_eq!(
            registry.submit(&new_scope, "cannot replace old input", 200),
            Err(SubmitError::Busy)
        );
        assert!(
            registry
                .poll(identity(1), Scope::Thread([4; 16]), 300)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            registry.poll(identity(1), Scope::New, 400).unwrap(),
            b"old conversation only"
        );
        assert!(
            registry
                .poll(identity(1), Scope::New, 500)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn offline_invalidates_old_target_even_after_same_conversation_returns() {
        let mut registry = Registry::new();
        registry.poll(identity(1), Scope::New, 0).unwrap();
        let old = target(&registry, 0);
        registry.offline(identity(1));
        assert!(!registry.is_live(&old, 100));
        registry.poll(identity(1), Scope::New, 200).unwrap();
        assert_ne!(target(&registry, 200), old);
    }

    #[test]
    fn offline_preserves_accepted_text_until_original_conversation_returns() {
        let mut registry = Registry::new();
        registry.poll(identity(1), Scope::New, 0).unwrap();
        let old = target(&registry, 0);
        registry.submit(&old, "accepted before modal", 100).unwrap();
        registry.offline(identity(1));
        assert!(!registry.is_live(&old, 200));
        assert!(
            registry
                .poll(identity(1), Scope::Thread([7; 16]), 300)
                .unwrap()
                .is_empty()
        );
        assert!(!registry.can_accept(&registry.receivers[0].as_ref().unwrap().target, 300));
        assert_eq!(
            registry.poll(identity(1), Scope::New, 400).unwrap(),
            b"accepted before modal"
        );
        assert!(
            registry
                .poll(identity(1), Scope::New, 500)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn expired_receiver_cannot_be_revived_by_the_same_scope() {
        let mut registry = Registry::new();
        registry.poll(identity(1), Scope::New, 0).unwrap();
        let old = target(&registry, 0);
        assert!(!registry.is_live(&old, READY_LEASE_MS));
        assert_eq!(
            registry.submit(&old, "late", READY_LEASE_MS),
            Err(SubmitError::Unavailable)
        );
        registry
            .poll(identity(1), Scope::New, READY_LEASE_MS)
            .unwrap();
        assert_ne!(target(&registry, READY_LEASE_MS), old);
    }

    #[test]
    fn restarted_vm_and_same_slot_other_instance_do_not_inherit_text() {
        let mut registry = Registry::new();
        registry.poll(identity(1), Scope::New, 0).unwrap();
        let old = target(&registry, 0);
        registry.submit(&old, "old VM only", 100).unwrap();
        assert!(
            registry
                .poll(identity(2), Scope::New, 200)
                .unwrap()
                .is_empty()
        );
        assert!(!registry.is_live(&old, 200));
        let second = target(&registry, 200);
        let changed_instance = Identity {
            instance: [8; 16],
            ..identity(2)
        };
        registry.poll(changed_instance, Scope::New, 300).unwrap();
        assert!(!registry.is_live(&second, 300));
    }

    #[test]
    fn heartbeat_keeps_generation_and_pending_message_intact() {
        let mut registry = Registry::new();
        registry.poll(identity(1), Scope::New, 0).unwrap();
        let original = target(&registry, 0);
        assert!(
            registry
                .poll(identity(1), Scope::New, 1_900)
                .unwrap()
                .is_empty()
        );
        assert_eq!(target(&registry, 2_500), original);
        registry.submit(&original, "once", 2_500).unwrap();
        assert_eq!(
            registry.poll(identity(1), Scope::New, 2_600).unwrap(),
            b"once"
        );
    }

    #[test]
    fn delayed_poll_keeps_accepted_text_for_same_conversation_only() {
        let mut registry = Registry::new();
        registry.poll(identity(1), Scope::New, 0).unwrap();
        let old = target(&registry, 0);
        registry.submit(&old, "accepted while ready", 100).unwrap();
        assert_eq!(
            registry
                .poll(identity(1), Scope::New, READY_LEASE_MS + 1)
                .unwrap(),
            b"accepted while ready"
        );
        assert!(!registry.is_live(&old, READY_LEASE_MS + 1));
        assert!(
            registry
                .poll(identity(1), Scope::New, READY_LEASE_MS + 200)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn scopes_are_bounded_exact_uuid_or_new() {
        assert_eq!(parse_scope("new"), Some(Scope::New));
        assert_eq!(
            parse_scope("01234567-89ab-cdef-0123-456789abcdef"),
            parse_scope("01234567-89AB-CDEF-0123-456789ABCDEF")
        );
        for invalid in [
            "",
            "offline",
            "/root",
            "new/extra",
            "00000000000000000000000000000000",
            "01234567_89ab-cdef-0123-456789abcdef",
            "g1234567-89ab-cdef-0123-456789abcdef",
        ] {
            assert_eq!(parse_scope(invalid), None, "{invalid}");
        }
    }
}
