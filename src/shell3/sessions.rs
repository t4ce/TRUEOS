//! Blueprint-owned byte sessions, driven by Shell3's existing AP service pool.
//! Captured command I/O and interactive terminals share admission and ownership;
//! only explicit terminal geometry opts into the human cockpit frontend.
use super::{MatrixSlotLease, Shell3, captured::Captured, matrix_target_slot_lease, tty::Terminal};
use alloc::{collections::VecDeque, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};

const INPUT_LIMIT: usize = 64 * 1024;
const OUTPUT_LIMIT: usize = super::tty::OUTPUT_LIMIT;
static NEXT: AtomicU64 = AtomicU64::new(1);
static SESSIONS: spin::Mutex<Vec<Entry>> = spin::Mutex::new(Vec::new());

struct State {
    input: VecDeque<u8>,
    output: VecDeque<u8>,
    size: (usize, usize),
    captured: bool,
    resize: bool,
    stdin_closed: bool,
    interrupt: bool,
    terminate: bool,
    retire: bool,
    exit: Option<i32>,
}
struct Entry {
    id: u64,
    vm: u8,
    run: u64,
    owner: MatrixSlotLease,
    slot: u32,
    claimed: bool,
    state: Arc<spin::Mutex<State>>,
}
fn owner(vm: u8) -> Option<MatrixSlotLease> {
    crate::hv::blueprint_console_target(vm).map(|target| matrix_target_slot_lease(&target))
}
fn state(vm: u8, id: u64) -> Result<Arc<spin::Mutex<State>>, i32> {
    let owner = owner(vm).ok_or(-3)?;
    let run = crate::hv::vm_run_generation(vm).ok_or(-3)?;
    SESSIONS
        .lock()
        .iter()
        .find(|entry| entry.id == id && entry.vm == vm && entry.run == run && entry.owner == owner)
        .map(|entry| Arc::clone(&entry.state))
        .ok_or(-3)
}
fn valid_size(cols: usize, rows: usize) -> bool {
    (super::MIN_COLUMNS..=512).contains(&cols) && (super::MIN_ROWS..=256).contains(&rows)
}
pub(crate) fn open(vm: u8, script: &[u8], cols: usize, rows: usize, stdin_open: bool) -> i64 {
    // Native callers may allocate in their VM heap; queued sessions and output
    // belong to the host and must remain valid during owner teardown.
    crate::allocators::with_host_alloc_domain_strong(|| {
        open_inner(vm, script, cols, rows, stdin_open)
    })
}
fn open_inner(vm: u8, script: &[u8], cols: usize, rows: usize, stdin_open: bool) -> i64 {
    let captured = cols == 0 && rows == 0;
    if (!captured && !valid_size(cols, rows))
        || script.len() > INPUT_LIMIT - 1
        || script.contains(&0)
        || core::str::from_utf8(script).is_err()
    {
        return -1;
    }
    let Some(owner) = owner(vm) else {
        return -3;
    };
    let Some(run) = crate::hv::vm_run_generation(vm).filter(|run| *run != 0) else {
        return -3;
    };
    super::service::refresh_appdb_names();
    let mut registry = SESSIONS.lock();
    if registry.len() >= super::MAX_SHELL3_INSTANCES {
        return -4;
    }
    let Ok(slot) = super::service::reserve_terminal_slot() else {
        return -4;
    };
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    if id == 0 || id > i64::MAX as u64 {
        super::service::release_shell_on_executor(slot);
        return -4;
    }
    let mut input: VecDeque<u8> = script.iter().copied().collect();
    // A launch script is submitted, while later writes remain raw keyboard input.
    if !script.is_empty() && !matches!(script.last(), Some(b'\r' | b'\n')) {
        input.push_back(b'\r');
    }
    registry.push(Entry {
        id,
        vm,
        run,
        owner,
        slot,
        claimed: false,
        state: Arc::new(spin::Mutex::new(State {
            input,
            output: VecDeque::new(),
            size: (cols, rows),
            captured,
            resize: false,
            stdin_closed: !stdin_open,
            interrupt: false,
            terminate: false,
            retire: false,
            exit: None,
        })),
    });
    drop(registry);
    super::service::notify_work();
    id as i64
}
/// Header is exit flag (u32) and signed exit code (i32), followed by console bytes.
pub(crate) fn read(vm: u8, id: u64, out: &mut [u8]) -> isize {
    if out.len() < 8 {
        return -1;
    }
    let shared = match state(vm, id) {
        Ok(shared) => shared,
        Err(code) => return code as isize,
    };
    let mut shared = shared.lock();
    if shared.retire {
        return -3;
    }
    let count = (out.len() - 8).min(shared.output.len());
    for byte in &mut out[8..8 + count] {
        *byte = shared.output.pop_front().unwrap();
    }
    let exit = if shared.output.is_empty() {
        shared.exit
    } else {
        None
    };
    out[..4].copy_from_slice(&u32::from(exit.is_some()).to_le_bytes());
    out[4..8].copy_from_slice(&exit.unwrap_or(0).to_le_bytes());
    (8 + count) as isize
}
pub(crate) fn write(vm: u8, id: u64, bytes: &[u8]) -> isize {
    crate::allocators::with_host_alloc_domain_strong(|| write_inner(vm, id, bytes))
}
fn write_inner(vm: u8, id: u64, bytes: &[u8]) -> isize {
    let shared = match state(vm, id) {
        Ok(shared) => shared,
        Err(code) => return code as isize,
    };
    let mut shared = shared.lock();
    if shared.retire || shared.exit.is_some() || shared.stdin_closed {
        return -3;
    }
    let count = bytes
        .len()
        .min(INPUT_LIMIT.saturating_sub(shared.input.len()));
    shared.input.extend(bytes[..count].iter().copied());
    drop(shared);
    super::service::notify_work();
    count as isize
}
pub(crate) fn control(vm: u8, id: u64, op: u32, cols: usize, rows: usize) -> i32 {
    let shared = match state(vm, id) {
        Ok(shared) => shared,
        Err(code) => return code,
    };
    let mut shared = shared.lock();
    match op {
        1 if !shared.captured && valid_size(cols, rows) => {
            shared.size = (cols, rows);
            shared.resize = true;
        }
        2 => shared.stdin_closed = true,
        3 => shared.interrupt = true,
        4 => shared.terminate = true,
        5 => {
            shared.retire = true;
            shared.terminate = true;
        }
        _ => return -1,
    }
    drop(shared);
    super::service::notify_work();
    0
}
struct Owned {
    id: u64,
    vm: u8,
    run: u64,
    slot: u32,
    owner: MatrixSlotLease,
    own_slot: Option<MatrixSlotLease>,
    frontend: Frontend,
    state: Arc<spin::Mutex<State>>,
}
enum Frontend {
    Terminal(Terminal),
    Captured(Captured),
}
impl Drop for Owned {
    fn drop(&mut self) {
        // Do not dispose of an arbitrary slot selected later by § navigation.
        if let Some(lease) = &self.own_slot {
            super::MatrixSlots::drop_lease(lease);
            // Terminal's Shell3 model releases its existing AP admission.
        } else {
            // Captured sessions never create a UI model or Matrix view, but
            // reserve and release the very same AP shell-pool admission.
            super::service::release_shell_on_executor(self.slot);
        }
    }
}
pub(super) struct Worker {
    slot: u32,
    sessions: Vec<Owned>,
}
impl Worker {
    pub fn new(slot: u32) -> Self {
        Self {
            slot,
            sessions: Vec::new(),
        }
    }
    pub fn pending(&self) -> bool {
        SESSIONS
            .lock()
            .iter()
            .any(|entry| entry.slot == self.slot && !entry.claimed)
    }
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty() && !SESSIONS.lock().iter().any(|entry| entry.slot == self.slot)
    }
    pub fn poll(&mut self) {
        let pending: Vec<_> = {
            let mut registry = SESSIONS.lock();
            registry
                .iter_mut()
                .filter(|entry| entry.slot == self.slot && !entry.claimed)
                .map(|entry| {
                    entry.claimed = true;
                    (
                        entry.id,
                        entry.vm,
                        entry.run,
                        entry.owner.clone(),
                        Arc::clone(&entry.state),
                    )
                })
                .collect()
        };
        for (id, vm, run, owner, shared) in pending {
            let (size, captured) = {
                let state = shared.lock();
                (state.size, state.captured)
            };
            if shared.lock().retire
                || !super::matrix_target::matrix_slot_is_live(&owner)
                || crate::hv::vm_run_generation(vm) != Some(run)
            {
                super::service::release_shell_on_executor(self.slot);
                SESSIONS.lock().retain(|entry| entry.id != id);
                continue;
            }
            let (own_slot, frontend) = if captured {
                (None, Frontend::Captured(Captured::new(vm)))
            } else {
                let shell = Shell3::new_terminal_sized_reserved(self.slot, None, size.0, size.1);
                let own_slot = shell
                    .active_matrix_lease()
                    .expect("fresh terminal owns a Matrix slot");
                (Some(own_slot), Frontend::Terminal(Terminal::new_ssh(shell)))
            };
            self.sessions.push(Owned {
                id,
                vm,
                run,
                slot: self.slot,
                owner,
                own_slot,
                frontend,
                state: shared,
            });
        }
        self.sessions.retain_mut(|session| {
            let (input, resize, interrupt, terminate, retire, eof) = {
                let mut shared = session.state.lock();
                if !super::matrix_target::matrix_slot_is_live(&session.owner)
                    || crate::hv::vm_run_generation(session.vm) != Some(session.run)
                {
                    shared.retire = true;
                    shared.terminate = true;
                }
                let count = shared.input.len().min(4096);
                let input: Vec<u8> = shared.input.drain(..count).collect();
                let resize = core::mem::take(&mut shared.resize).then_some(shared.size);
                let eof = shared.stdin_closed && shared.input.is_empty();
                (
                    input,
                    resize,
                    core::mem::take(&mut shared.interrupt),
                    shared.terminate,
                    shared.retire,
                    eof,
                )
            };
            let (mut output, mut exit) = match &mut session.frontend {
                Frontend::Terminal(terminal) => {
                    if let Some(size) = resize {
                        terminal.resize(size.0, size.1);
                    }
                    if interrupt {
                        terminal.input(&[3]);
                    }
                    if !input.is_empty() && !terminate {
                        terminal.input(&input);
                    }
                    if terminate {
                        terminal.closing = true;
                    }
                    terminal.reconcile_matrix_selection();
                    let exit = terminal.closing.then_some(if terminal.overflow {
                        1
                    } else if terminate {
                        137
                    } else {
                        0
                    });
                    (core::mem::take(&mut terminal.output), exit)
                }
                Frontend::Captured(captured) => {
                    if !terminate {
                        if interrupt {
                            captured.interrupt();
                        }
                        if !input.is_empty() {
                            captured.input(&input);
                        }
                        if eof {
                            captured.close_stdin();
                        }
                        captured.poll();
                    }
                    (
                        captured.drain_output(),
                        if terminate {
                            Some(137)
                        } else {
                            captured.exit_code()
                        },
                    )
                }
            };
            let mut shared = session.state.lock();
            const NOTICE: &[u8] = b"\ncommand session: output capacity exceeded\n";
            let payload_limit = OUTPUT_LIMIT.saturating_sub(NOTICE.len());
            if output.len() > payload_limit.saturating_sub(shared.output.len()) {
                // Keep room for the failure without clipping already accepted
                // output, which could otherwise split a UTF-8 scalar.
                shared.output.extend(NOTICE.iter().copied());
                output.clear();
                exit = Some(1);
            } else {
                shared.output.extend(output.drain(..));
            }
            if let Frontend::Terminal(terminal) = &mut session.frontend {
                // Preserve the human terminal's reusable redraw allocation.
                terminal.output = output;
            }
            if let Some(code) = exit {
                shared.exit = Some(code);
                if retire {
                    drop(shared);
                    SESSIONS.lock().retain(|entry| entry.id != session.id);
                }
                return false;
            }
            true
        });
        // Closed sessions may already have left the worker, but their handles
        // remain readable until the client releases them or the VM lease expires.
        SESSIONS.lock().retain(|entry| {
            let shared = entry.state.lock();
            !(entry.claimed
                && shared.exit.is_some()
                && (shared.retire
                    || !super::matrix_target::matrix_slot_is_live(&entry.owner)
                    || crate::hv::vm_run_generation(entry.vm) != Some(entry.run)))
        });
    }
}
