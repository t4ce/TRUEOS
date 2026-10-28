extern crate alloc;
extern crate self as trueos_vm;
extern crate self as v;

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};

#[path = "@TRUEOS_ROOT@/crates/trueos-v/src/spirit_agents.rs"]
pub mod spirit_agents;

#[derive(Clone, Copy)]
pub struct VmState {
    supported: bool,
    running: bool,
    stop_requested: bool,
}

struct State {
    generations: VecDeque<Option<u64>>,
    generation: Option<u64>,
    vm: VmState,
    identity: bool,
    direct_owner: Option<u8>,
    hull_owner: Option<u8>,
    ingested: Vec<(u8, u64, spirit_agents::AgentSnapshot)>,
    ingest_result: Result<(), i32>,
    hull_payload: Option<Vec<u8>>,
    hull_response: (u32, u64),
    hull_status_override: Option<u32>,
    hull_op: Option<u32>,
    host_depth: usize,
    host_entries: usize,
}

static STATE: Mutex<State> = Mutex::new(State::new());
static SERIAL: Mutex<()> = Mutex::new(());

impl State {
    const fn new() -> Self {
        Self {
            generations: VecDeque::new(),
            generation: Some(7),
            vm: VmState { supported: true, running: true, stop_requested: false },
            identity: true,
            direct_owner: Some(2),
            hull_owner: None,
            ingested: Vec::new(),
            ingest_result: Ok(()),
            hull_payload: None,
            hull_response: (0, 0),
            hull_status_override: None,
            hull_op: None,
            host_depth: 0,
            host_entries: 0,
        }
    }
}

mod hv {
    use super::*;
    pub fn vm_run_generation(_: u8) -> Option<u64> {
        let mut state = STATE.lock().unwrap();
        state.generations.pop_front().unwrap_or(state.generation)
    }
    pub fn vm_state(_: u8) -> VmState { STATE.lock().unwrap().vm }
    pub fn blueprint_instance_identity(_: u8) -> Option<()> {
        STATE.lock().unwrap().identity.then_some(())
    }
    pub fn current_hull_guest_context_vm_id() -> Option<u8> { STATE.lock().unwrap().hull_owner }
    pub fn current_guest_execution_context_vm_id() -> Option<u8> { STATE.lock().unwrap().direct_owner }
}

mod allocators {
    pub fn with_host_alloc_domain_strong<T>(f: impl FnOnce() -> T) -> T {
        {
            let mut state = crate::STATE.lock().unwrap();
            state.host_depth += 1;
            state.host_entries += 1;
        }
        let result = f();
        crate::STATE.lock().unwrap().host_depth -= 1;
        result
    }
}

mod spirit {
    mod spirit_thoughts {
        pub fn ingest(owner: u8, run: u64, snapshot: crate::spirit_agents::AgentSnapshot) -> Result<(), i32> {
            let mut state = crate::STATE.lock().unwrap();
            assert!(state.host_depth > 0, "Spirit retained caller-owned allocations");
            state.ingested.push((owner, run, snapshot));
            state.ingest_result
        }
    }

    @INGRESS@

    @EXPORT@
}

pub mod vmcall {
    use super::*;
    pub const OP_BP_SPIRIT_AGENTS_PUBLISH_V1: u32 = @OPCODE@;
    pub const STATUS_OK: u32 = 0;
    pub const STATUS_BAD_ARG: u32 = 2;
    pub const PAYLOAD_CAP: usize = @COMM_PAGE_BYTES@ - 56;
    #[derive(Debug, PartialEq)]
    enum DispatchOutcome { Resume }

    fn request_payload(_: u8, req_len: u32) -> Option<&'static [u8]> {
        let bytes = STATE.lock().unwrap().hull_payload.clone()?;
        if bytes.len() != req_len as usize || bytes.len() > PAYLOAD_CAP { return None; }
        Some(Box::leak(bytes.into_boxed_slice()))
    }
    fn write_response(_: u8, _: u32, status: u32, data: u64, _: u32) {
        STATE.lock().unwrap().hull_response = (status, data);
    }
    fn dispatch(vm_id: u8, seq: u32, req_len: u32) -> DispatchOutcome {
        @DISPATCH@
    }
    pub fn call_with_payload(op: u32, _: u64, _: u64, bytes: &[u8], _: &mut [u8]) -> (u32, u64) {
        let owner = {
            let mut state = STATE.lock().unwrap();
            state.hull_op = Some(op);
            if let Some(status) = state.hull_status_override { return (status, 0); }
            state.hull_payload = Some(bytes.to_vec());
            state.hull_owner.unwrap()
        };
        assert_eq!(op, OP_BP_SPIRIT_AGENTS_PUBLISH_V1);
        assert_eq!(dispatch(owner, 1, bytes.len() as u32), DispatchOutcome::Resume);
        STATE.lock().unwrap().hull_response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spirit_agents::{AgentEntry, AgentLifecycle, AgentSnapshot, SNAPSHOT_VERSION};

    fn reset() -> MutexGuard<'static, ()> {
        let guard = SERIAL.lock().unwrap();
        *STATE.lock().unwrap() = State::new();
        guard
    }
    fn snapshot() -> AgentSnapshot {
        AgentSnapshot {
            version: SNAPSHOT_VERSION, epoch: 1, revision: 1, complete: true, connected: true,
            agents: vec![AgentEntry {
                id: "child-1".into(), parent_id: Some("root".into()), name: None,
                nickname: Some("Scout".into()), role: None,
                lifecycle: Some(AgentLifecycle::Running), runtime: None,
                waiting_approval: false, waiting_input: false, loaded: true,
            }],
        }
    }
    fn invoke(bytes: &[u8]) -> i32 {
        unsafe { spirit::trueos_cabi_spirit_agents_publish_v1(bytes.as_ptr(), bytes.len()) }
    }

    #[test]
    fn direct_ingress_uses_execution_identity_and_preserves_exact_snapshot() {
        let _serial = reset();
        let expected = snapshot();
        assert_eq!(invoke(&spirit_agents::encode(&expected).unwrap()), 0);
        let state = STATE.lock().unwrap();
        assert_eq!(state.ingested, vec![(2, 7, expected)]);
        assert_eq!(state.hull_op, None);
        assert_eq!(state.host_entries, 1);
        assert_eq!(state.host_depth, 0, "caller allocation domain must be restored");
    }

    #[test]
    fn missing_stopped_unsupported_or_reused_owner_never_reaches_ingest() {
        let _serial = reset();
        let bytes = spirit_agents::encode(&snapshot()).unwrap();
        for failure in 0..7 {
            *STATE.lock().unwrap() = State::new();
            {
                let mut state = STATE.lock().unwrap();
                match failure {
                    0 => state.direct_owner = None,
                    1 => state.generation = None,
                    2 => state.generation = Some(0),
                    3 => state.vm.stop_requested = true,
                    4 => state.vm.running = false,
                    5 => state.vm.supported = false,
                    _ => state.identity = false,
                }
            }
            assert_eq!(invoke(&bytes), -1, "failure case {failure}");
            assert!(STATE.lock().unwrap().ingested.is_empty());
        }
        *STATE.lock().unwrap() = State::new();
        STATE.lock().unwrap().generations = VecDeque::from([Some(7), Some(8)]);
        assert_eq!(invoke(&bytes), -1);
        assert!(STATE.lock().unwrap().ingested.is_empty());
    }

    #[test]
    fn export_rejects_pointer_length_and_json_errors_before_ingest() {
        let _serial = reset();
        unsafe {
            assert_eq!(spirit::trueos_cabi_spirit_agents_publish_v1(core::ptr::null(), 1), -3);
            assert_eq!(spirit::trueos_cabi_spirit_agents_publish_v1(1usize as *const u8, 0), -3);
            assert_eq!(spirit::trueos_cabi_spirit_agents_publish_v1(1usize as *const u8, spirit_agents::MAX_SNAPSHOT_BYTES + 1), -4);
        }
        assert_eq!(invoke(b"{"), -3);
        let mut invalid = snapshot();
        invalid.agents.push(invalid.agents[0].clone());
        assert_eq!(invoke(&serde_json::to_vec(&invalid).unwrap()), -3);
        assert!(STATE.lock().unwrap().ingested.is_empty());
    }

    #[test]
    fn capacity_validation_rejects_whole_payload_before_ingest() {
        let _serial = reset();
        let mut oversized = snapshot();
        oversized.agents.resize(spirit_agents::MAX_AGENTS + 1, oversized.agents[0].clone());
        assert_eq!(invoke(&serde_json::to_vec(&oversized).unwrap()), -4);
        oversized = snapshot();
        oversized.agents[0].name = Some("x".repeat(spirit_agents::MAX_LABEL_BYTES + 1));
        assert_eq!(invoke(&serde_json::to_vec(&oversized).unwrap()), -4);
        assert!(STATE.lock().unwrap().ingested.is_empty());
        assert!(spirit_agents::MAX_SNAPSHOT_BYTES <= vmcall::PAYLOAD_CAP);
    }

    #[test]
    fn hull_export_uses_real_dispatch_arm_and_relays_signed_ingress_status() {
        let _serial = reset();
        {
            let mut state = STATE.lock().unwrap();
            state.hull_owner = Some(4);
            state.direct_owner = None;
        }
        let expected = snapshot();
        let bytes = spirit_agents::encode(&expected).unwrap();
        assert_eq!(invoke(&bytes), 0);
        assert_eq!(STATE.lock().unwrap().ingested, vec![(4, 7, expected)]);
        assert_eq!(STATE.lock().unwrap().hull_op, Some(vmcall::OP_BP_SPIRIT_AGENTS_PUBLISH_V1));
        assert_eq!(STATE.lock().unwrap().host_entries, 1);
        assert_eq!(STATE.lock().unwrap().host_depth, 0);
        for error in [-4, -9] {
            STATE.lock().unwrap().ingest_result = Err(error);
            assert_eq!(invoke(&bytes), error);
        }
        STATE.lock().unwrap().hull_status_override = Some(vmcall::STATUS_BAD_ARG);
        assert_eq!(invoke(&bytes), -7);
    }

    #[test]
    fn direct_export_propagates_ingress_capacity_and_stale_statuses() {
        let _serial = reset();
        let bytes = spirit_agents::encode(&snapshot()).unwrap();
        for error in [-4, -9] {
            STATE.lock().unwrap().ingest_result = Err(error);
            assert_eq!(invoke(&bytes), error);
        }
    }
}
