use alloc::collections::VecDeque;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_sync::signal::Signal;

use crate::shell2::{ShellBackend2, ShellIo2, TerminalHandoffOwner};

const SESSION_INPUT_CAP: usize = 256 * 1024;
const SESSION_REPLAY_CAP: usize = 256 * 1024;
const FRONTEND_CAPACITY_ERROR: i32 = -4;
const FRONTEND_BUSY_ERROR: i32 = -5;
pub(crate) const FRONTEND_FLAG_DROPPED: u32 = 1 << 0;
pub(crate) const FRONTEND_FLAG_HANDOFF: u32 = 1 << 1;
const TERMINAL_RESET: &[u8] = b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1004l\x1b[?1006l\x1b[?1015l\x1b[?2004l\x1b[?1049l\x1b[?7h\x1b[0m\x1b[39;49m\x1b[r\x1b[?25h";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LeaseState {
    Free,
    Active {
        creator_vm: u8,
        creator_run_generation: u64,
        generation: u64,
        frontend_attached: bool,
    },
    Closing {
        creator_vm: u8,
        creator_run_generation: u64,
        generation: u64,
    },
}

struct SessionIoState {
    lease: LeaseState,
    rx: VecDeque<u8>,
    replay: VecDeque<u8>,
    replay_base_seq: u64,
    replay_next_seq: u64,
    epoch: u64,
    cols: usize,
    rows: usize,
    handoff_owner: Option<TerminalHandoffOwner>,
    repaint_requested: bool,
}

impl SessionIoState {
    const fn new() -> Self {
        Self {
            lease: LeaseState::Free,
            rx: VecDeque::new(),
            replay: VecDeque::new(),
            replay_base_seq: 0,
            replay_next_seq: 0,
            epoch: 0,
            cols: 0,
            rows: 0,
            handoff_owner: None,
            repaint_requested: false,
        }
    }

    fn reset_replay(&mut self) {
        self.epoch = NEXT_EPOCH.fetch_add(1, Ordering::AcqRel).max(1);
        self.replay.clear();
        self.replay_base_seq = 0;
        self.replay_next_seq = 0;
    }

    fn active_generation(&self) -> Option<u64> {
        match self.lease {
            LeaseState::Active { generation, .. } => Some(generation),
            LeaseState::Free | LeaseState::Closing { .. } => None,
        }
    }
}

pub(crate) struct LocalShellSessionBackend {
    index: usize,
    state: spin::Mutex<SessionIoState>,
    last_was_cr: AtomicBool,
    wake: Signal<crate::wait::EmbassySpinRawMutex, u64>,
}

impl LocalShellSessionBackend {
    const fn new(index: usize) -> Self {
        Self {
            index,
            state: spin::Mutex::new(SessionIoState::new()),
            last_was_cr: AtomicBool::new(false),
            wake: Signal::new(),
        }
    }

    fn append_replay(state: &mut SessionIoState, bytes: &[u8]) {
        for &byte in bytes {
            if state.replay.len() >= SESSION_REPLAY_CAP {
                let _ = state.replay.pop_front();
                state.replay_base_seq = state.replay_base_seq.wrapping_add(1);
            }
            state.replay.push_back(byte);
            state.replay_next_seq = state.replay_next_seq.wrapping_add(1);
        }
    }

    /// Append shell-owned output only while the interpreter owns the terminal.
    /// Holding the state lock across CRLF conversion closes the claim/write race
    /// with direct terminal handoff.
    fn push_shell_output(&self, bytes: &[u8]) {
        let mut state = self.state.lock();
        if !matches!(state.lease, LeaseState::Active { .. }) || state.handoff_owner.is_some() {
            return;
        }
        crate::shell2::crlf::write_bytes_crlf(bytes, &self.last_was_cr, |chunk| {
            Self::append_replay(&mut state, chunk)
        });
    }

    /// Append output only when it still belongs to the expected lease.
    /// Generation validation and replay mutation share the state lock so a
    /// recycled backend can never receive bytes from its prior incarnation.
    fn push_shell_output_for_generation(&self, generation: u64, bytes: &[u8]) -> bool {
        let mut state = self.state.lock();
        if state.active_generation() != Some(generation) || state.handoff_owner.is_some() {
            return false;
        }
        crate::shell2::crlf::write_bytes_crlf(bytes, &self.last_was_cr, |chunk| {
            Self::append_replay(&mut state, chunk)
        });
        true
    }
}

static LOCAL_SESSION_BACKENDS: [LocalShellSessionBackend; crate::shell2::LOCAL_SHELL_SESSION_CAP] = [
    LocalShellSessionBackend::new(0),
    LocalShellSessionBackend::new(1),
    LocalShellSessionBackend::new(2),
    LocalShellSessionBackend::new(3),
    LocalShellSessionBackend::new(4),
    LocalShellSessionBackend::new(5),
    LocalShellSessionBackend::new(6),
    LocalShellSessionBackend::new(7),
    LocalShellSessionBackend::new(8),
];

static ALLOCATION_LOCK: spin::Mutex<()> = spin::Mutex::new(());
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);
static CAPACITY_ERROR_LATCHED: AtomicBool = AtomicBool::new(false);
static POOL_READY: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FrontendRead {
    pub(crate) len: usize,
    pub(crate) next_seq: u64,
    pub(crate) epoch: u64,
    pub(crate) flags: u32,
}

pub(crate) fn backend(index: usize) -> Option<&'static LocalShellSessionBackend> {
    LOCAL_SESSION_BACKENDS.get(index)
}

pub(crate) fn set_pool_ready(ready: bool) {
    POOL_READY.store(ready, Ordering::Release);
}

pub(crate) fn backend_for_output_mask(
    output_mask: crate::shell2::OutputMask,
) -> Option<&'static LocalShellSessionBackend> {
    let local = output_mask & crate::shell2::OUTPUT_LOCAL_MASK;
    if local == 0 || local.count_ones() != 1 {
        return None;
    }
    let bit = local.trailing_zeros() as usize;
    let index = bit.checked_sub(crate::shell2::LOCAL_SHELL_SESSION_FIRST_BIT)?;
    backend(index)
}

pub(crate) fn readable_len_for_output_mask_generation(
    output_mask: crate::shell2::OutputMask,
    generation: u64,
) -> Option<usize> {
    let state = backend_for_output_mask(output_mask)?.state.lock();
    (state.active_generation() == Some(generation)).then_some(state.rx.len())
}

/// Run a Matrix scope operation while the current local lease is pinned.
///
/// The closure may take the Matrix lock, but it must not re-enter this local
/// backend. Session allocation and reuse take the backend state lock before
/// changing its generation, so they cannot cross this operation's boundary.
pub(crate) fn with_active_generation_for_output_mask<R>(
    output_mask: crate::shell2::OutputMask,
    operation: impl FnOnce(u64) -> R,
) -> Option<R> {
    let state = backend_for_output_mask(output_mask)?.state.lock();
    let generation = state.active_generation()?;
    let result = operation(generation);
    drop(state);
    Some(result)
}

/// Run an operation only while `generation` is the active incarnation of the
/// selected local backend. The lease remains pinned until the closure returns.
pub(crate) fn with_generation_for_output_mask<R>(
    output_mask: crate::shell2::OutputMask,
    generation: u64,
    operation: impl FnOnce((usize, usize)) -> R,
) -> Option<R> {
    let state = backend_for_output_mask(output_mask)?.state.lock();
    if state.active_generation() != Some(generation) {
        return None;
    }
    let result = operation((state.cols, state.rows));
    drop(state);
    Some(result)
}

pub(crate) fn write_for_output_mask_generation(
    output_mask: crate::shell2::OutputMask,
    generation: u64,
    bytes: &[u8],
) -> bool {
    backend_for_output_mask(output_mask)
        .is_some_and(|backend| backend.push_shell_output_for_generation(generation, bytes))
}

pub(crate) fn read_byte_for_output_mask_generation(
    output_mask: crate::shell2::OutputMask,
    generation: u64,
) -> Option<u8> {
    let mut state = backend_for_output_mask(output_mask)?.state.lock();
    if state.active_generation() != Some(generation) || state.handoff_owner.is_some() {
        return None;
    }
    state.rx.pop_front()
}

fn backend_for_owner(vm_id: u8, run_generation: u64) -> Option<&'static LocalShellSessionBackend> {
    LOCAL_SESSION_BACKENDS.iter().find(|backend| {
        matches!(
            backend.state.lock().lease,
            LeaseState::Active { creator_vm, creator_run_generation, .. }
                | LeaseState::Closing { creator_vm, creator_run_generation, .. }
                if creator_vm == vm_id && creator_run_generation == run_generation
        )
    })
}

fn owner_run_generation(vm_id: u8) -> Result<u64, i32> {
    crate::hv::vm_run_generation(vm_id)
        .filter(|generation| *generation != 0)
        .ok_or(-3)
}

pub(crate) fn attach(vm_id: u8, cols: usize, rows: usize) -> i32 {
    if cols == 0 || rows == 0 || cols > 4_096 || rows > 4_096 {
        return -1;
    }
    if !POOL_READY.load(Ordering::Acquire) {
        return -3;
    }
    let Ok(run_generation) = owner_run_generation(vm_id) else {
        return -3;
    };

    let allocation = ALLOCATION_LOCK.lock();
    if let Some(existing) = backend_for_owner(vm_id, run_generation) {
        let mut state = existing.state.lock();
        if let LeaseState::Active {
            creator_vm,
            creator_run_generation,
            generation,
            ..
        } = state.lease
        {
            // The UI frontend remains the geometry authority while a direct
            // terminal owner is active. Window resize and private zoom both
            // update this existing session; ownership and input routing stay
            // unchanged while the new grid is reported to the handoff guest.
            state.lease = LeaseState::Active {
                creator_vm,
                creator_run_generation,
                generation,
                frontend_attached: true,
            };
            state.cols = cols;
            state.rows = rows;
            state.reset_replay();
            state.repaint_requested = true;
            drop(state);
            crate::shell2::configure_local_shell_session_view(
                existing.index,
                generation,
                cols,
                rows,
            );
            drop(allocation);
            return 0;
        }
        return FRONTEND_BUSY_ERROR;
    }

    let Some(backend) = LOCAL_SESSION_BACKENDS
        .iter()
        .find(|backend| backend.state.lock().lease == LeaseState::Free)
    else {
        let active = LOCAL_SESSION_BACKENDS
            .iter()
            .filter(|backend| matches!(backend.state.lock().lease, LeaseState::Active { .. }))
            .count()
            + 1;
        let closing = LOCAL_SESSION_BACKENDS
            .iter()
            .filter(|backend| matches!(backend.state.lock().lease, LeaseState::Closing { .. }))
            .count();
        drop(allocation);
        if !CAPACITY_ERROR_LATCHED.swap(true, Ordering::AcqRel) {
            crate::log_error!(target: "shell2";
                "shell2-session: global host-shell soft cap reached active={} closing={} cap=10 requester_vm={} action=reject\n",
                active,
                closing,
                vm_id
            );
            crate::shell2::matrix::record_line_in_default(
                "shell2-session: maximum shell sessions reached (10)",
            );
        }
        return FRONTEND_CAPACITY_ERROR;
    };

    let generation = NEXT_GENERATION.fetch_add(1, Ordering::AcqRel).max(1);
    {
        let mut state = backend.state.lock();
        state.lease = LeaseState::Active {
            creator_vm: vm_id,
            creator_run_generation: run_generation,
            generation,
            frontend_attached: true,
        };
        state.rx.clear();
        state.reset_replay();
        state.cols = cols;
        state.rows = rows;
        state.handoff_owner = None;
        state.repaint_requested = false;
    }
    backend.last_was_cr.store(false, Ordering::Release);
    let index = backend.index;
    crate::shell2::initialize_local_shell_session_view(index, generation, cols, rows);
    drop(allocation);

    // The worker may paint immediately after its wake. Publish its Matrix view
    // geometry and default-page selection before making this generation live.
    backend.wake.signal(generation);
    0
}

pub(crate) fn read(vm_id: u8, read_seq: u64, out: &mut [u8]) -> Result<FrontendRead, i32> {
    let run_generation = owner_run_generation(vm_id)?;
    let Some(backend) = backend_for_owner(vm_id, run_generation) else {
        return Err(-2);
    };
    let state = backend.state.lock();
    if !matches!(
        state.lease,
        LeaseState::Active {
            creator_vm,
            creator_run_generation,
            frontend_attached: true,
            ..
        } if creator_vm == vm_id && creator_run_generation == run_generation
    ) {
        return Err(-2);
    }

    let mut flags = if state.handoff_owner.is_some() {
        FRONTEND_FLAG_HANDOFF
    } else {
        0
    };
    let start_seq = if read_seq < state.replay_base_seq || read_seq > state.replay_next_seq {
        flags |= FRONTEND_FLAG_DROPPED;
        state.replay_base_seq
    } else {
        read_seq
    };
    let offset = start_seq.saturating_sub(state.replay_base_seq) as usize;
    let len = out.len().min(state.replay.len().saturating_sub(offset));
    for (dst, byte) in out[..len].iter_mut().zip(state.replay.iter().skip(offset)) {
        *dst = *byte;
    }
    Ok(FrontendRead {
        len,
        next_seq: start_seq.wrapping_add(len as u64),
        epoch: state.epoch,
        flags,
    })
}

pub(crate) fn submit_input(vm_id: u8, bytes: &[u8]) -> Result<usize, i32> {
    if bytes.is_empty() {
        return Ok(0);
    }
    let run_generation = owner_run_generation(vm_id)?;
    let Some(backend) = backend_for_owner(vm_id, run_generation) else {
        return Err(-2);
    };
    let mut state = backend.state.lock();
    if !matches!(
        state.lease,
        LeaseState::Active {
            creator_vm,
            creator_run_generation,
            frontend_attached: true,
            ..
        } if creator_vm == vm_id && creator_run_generation == run_generation
    ) {
        return Err(-2);
    }
    if bytes.len() > SESSION_INPUT_CAP
        || state.rx.len().saturating_add(bytes.len()) > SESSION_INPUT_CAP
    {
        return Err(FRONTEND_BUSY_ERROR);
    }
    state.rx.extend(bytes.iter().copied());
    Ok(bytes.len())
}

pub(crate) fn detach(vm_id: u8) -> i32 {
    let Ok(run_generation) = owner_run_generation(vm_id) else {
        return -3;
    };
    let Some(backend) = backend_for_owner(vm_id, run_generation) else {
        return -2;
    };
    let mut state = backend.state.lock();
    match state.lease {
        LeaseState::Active {
            creator_vm,
            creator_run_generation,
            generation,
            ..
        } if creator_vm == vm_id && creator_run_generation == run_generation => {
            state.lease = LeaseState::Active {
                creator_vm,
                creator_run_generation,
                generation,
                frontend_attached: false,
            };
            0
        }
        LeaseState::Free | LeaseState::Active { .. } | LeaseState::Closing { .. } => -2,
    }
}

pub(crate) fn close_owner(vm_id: u8) -> i32 {
    let Ok(run_generation) = owner_run_generation(vm_id) else {
        return -3;
    };
    let _allocation = ALLOCATION_LOCK.lock();
    let Some(backend) = backend_for_owner(vm_id, run_generation) else {
        return -2;
    };
    let mut state = backend.state.lock();
    match state.lease {
        LeaseState::Active {
            creator_vm,
            creator_run_generation,
            generation,
            ..
        } if creator_vm == vm_id && creator_run_generation == run_generation => {
            state.lease = LeaseState::Closing {
                creator_vm,
                creator_run_generation,
                generation,
            };
            state.rx.clear();
            state.reset_replay();
            state.handoff_owner = None;
            state.repaint_requested = false;
            drop(state);
            backend.wake.signal(generation);
            0
        }
        LeaseState::Free | LeaseState::Active { .. } | LeaseState::Closing { .. } => -2,
    }
}

pub(crate) async fn wait_for_lease(index: usize) -> u64 {
    let Some(backend) = backend(index) else {
        core::future::pending().await
    };
    loop {
        let generation = backend.wake.wait().await;
        if generation_active(index, generation) {
            return generation;
        }
        acknowledge_closed(index, generation);
    }
}

pub(crate) fn generation_active(index: usize, generation: u64) -> bool {
    backend(index)
        .is_some_and(|backend| backend.state.lock().active_generation() == Some(generation))
}

pub(crate) fn take_repaint_request(index: usize, generation: u64) -> bool {
    let Some(backend) = backend(index) else {
        return false;
    };
    let mut state = backend.state.lock();
    if state.active_generation() != Some(generation) || !state.repaint_requested {
        return false;
    }
    state.repaint_requested = false;
    true
}

pub(crate) fn acknowledge_closed(index: usize, generation: u64) {
    let _allocation = ALLOCATION_LOCK.lock();
    let Some(backend) = backend(index) else {
        return;
    };
    let mut state = backend.state.lock();
    if matches!(state.lease, LeaseState::Closing { generation: closing, .. } if closing == generation)
    {
        *state = SessionIoState::new();
        backend.last_was_cr.store(false, Ordering::Release);
        CAPACITY_ERROR_LATCHED.store(false, Ordering::Release);
    }
}

impl ShellIo2 for LocalShellSessionBackend {
    fn output_mask(&self) -> crate::shell2::OutputMask {
        crate::shell2::local_shell_session_output_mask(self.index)
    }

    fn transport_scope(&self) -> u8 {
        crate::shell2::TRANSPORT_LOCAL_SCOPE
    }

    fn raw_write_str(&self, value: &str) {
        self.push_shell_output(value.as_bytes());
    }

    fn raw_write_fmt(&self, args: core::fmt::Arguments<'_>) {
        struct Writer<'a>(&'a LocalShellSessionBackend);
        impl Write for Writer<'_> {
            fn write_str(&mut self, value: &str) -> core::fmt::Result {
                self.0.raw_write_str(value);
                Ok(())
            }
        }
        let _ = Writer(self).write_fmt(args);
    }

    fn raw_write_char(&self, ch: char) {
        let mut encoded = [0u8; 4];
        self.raw_write_str(ch.encode_utf8(&mut encoded));
    }

    fn raw_write_byte(&self, byte: u8) {
        self.push_shell_output(&[byte]);
    }
}

impl ShellBackend2 for LocalShellSessionBackend {
    fn read_byte(&self) -> Option<u8> {
        let mut state = self.state.lock();
        (matches!(state.lease, LeaseState::Active { .. }) && state.handoff_owner.is_none())
            .then(|| state.rx.pop_front())
            .flatten()
    }

    fn claim_terminal_handoff(&self, owner: TerminalHandoffOwner) -> bool {
        let mut state = self.state.lock();
        if state.active_generation() != owner.local_session_generation()
            || state.handoff_owner.is_some_and(|current| current != owner)
        {
            return false;
        }
        state.rx.clear();
        state.reset_replay();
        state.handoff_owner = Some(owner);
        self.last_was_cr.store(false, Ordering::Release);
        Self::append_replay(&mut state, TERMINAL_RESET);
        true
    }

    fn release_terminal_handoff(&self, owner: TerminalHandoffOwner) -> bool {
        let mut state = self.state.lock();
        if !matches!(state.lease, LeaseState::Active { .. }) || state.handoff_owner != Some(owner) {
            return false;
        }
        state.handoff_owner = None;
        state.rx.clear();
        state.reset_replay();
        state.repaint_requested = true;
        self.last_was_cr.store(false, Ordering::Release);
        Self::append_replay(&mut state, TERMINAL_RESET);
        true
    }

    fn terminal_handoff_active(&self) -> bool {
        self.state.lock().handoff_owner.is_some()
    }

    fn supports_terminal_handoff(&self) -> bool {
        true
    }

    fn terminal_handoff_read(&self, owner: TerminalHandoffOwner, out: &mut [u8]) -> usize {
        let mut state = self.state.lock();
        if !matches!(state.lease, LeaseState::Active { .. }) || state.handoff_owner != Some(owner) {
            return 0;
        }
        let len = out.len().min(state.rx.len());
        for byte in &mut out[..len] {
            *byte = state.rx.pop_front().unwrap_or_default();
        }
        len
    }

    fn terminal_handoff_readable_len(&self, owner: TerminalHandoffOwner) -> usize {
        let state = self.state.lock();
        if !matches!(state.lease, LeaseState::Active { .. }) || state.handoff_owner != Some(owner) {
            return 0;
        }
        state.rx.len()
    }

    fn terminal_handoff_write(&self, owner: TerminalHandoffOwner, bytes: &[u8]) -> bool {
        let mut state = self.state.lock();
        if !matches!(state.lease, LeaseState::Active { .. }) || state.handoff_owner != Some(owner) {
            return false;
        }
        Self::append_replay(&mut state, bytes);
        true
    }
}
