from pathlib import Path


def rep(path: str, old: str, new: str) -> None:
    p = Path(path)
    source = p.read_text()
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"{path}: anchor count {count}, expected 1")
    p.write_text(source.replace(old, new, 1))


def ins_after(path: str, anchor: str, text: str) -> None:
    rep(path, anchor, anchor + text)


# One VM-local readiness generation is the only wait rendezvous used by
# Blueprint poll/Mio. Token/interest registration remains userspace state.
ins_after(
    "src/wait.rs",
    "const PLATFORM_WAIT_HOST_SCOPE: u16 = 0;\n",
    "pub(crate) const BLUEPRINT_IO_WAIT_KEY: u64 = 0x4250_494f_0000_0001;\n",
)

anchor = '''pub fn platform_wake_vm_scope(vm_id: u8) -> usize {
    let scope = platform_wait_vm_scope(vm_id);
    let queues = {
        let queues = PLATFORM_WAIT_QUEUES.lock();
        queues
            .iter()
            .filter_map(|(&(queue_scope, _), queue)| (queue_scope == scope).then_some(*queue))
            .collect::<Vec<_>>()
    };
    let count = queues.len();
    for queue in queues {
        queue.notify_all();
    }
    count
}
'''
ins_after(
    "src/wait.rs",
    anchor,
    '''

/// Wake every currently registered Blueprint I/O waiter without creating new
/// queues for VMs that are not waiting. Device/network producers use this as
/// a coarse readiness edge; the userspace Mio registry re-probes exact sources.
pub fn platform_wake_all_blueprint_io_waiters() -> usize {
    let queues = {
        let queues = PLATFORM_WAIT_QUEUES.lock();
        queues
            .iter()
            .filter_map(|(&(scope, key), queue)| {
                (scope != PLATFORM_WAIT_HOST_SCOPE && key == BLUEPRINT_IO_WAIT_KEY).then_some(*queue)
            })
            .collect::<Vec<_>>()
    };
    let mut woke = 0usize;
    for queue in queues {
        woke = woke.saturating_add(queue.notify_all());
    }
    woke
}

#[inline]
pub fn platform_wake_blueprint_io_for_vm(vm_id: u8) -> usize {
    platform_wake_all_for_vm(vm_id, BLUEPRINT_IO_WAIT_KEY)
}
''',
)

old_poll = '''#[unsafe(no_mangle)]
pub unsafe extern "C" fn poll(fds: *mut PollFd, nfds: usize, timeout: c_int) -> c_int {
    if nfds != 0 && fds.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }

    let Some(pollfds) =
        abi_write_bytes(fds.cast::<u8>(), nfds.saturating_mul(core::mem::size_of::<PollFd>()))
    else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };
    let pollfds = unsafe { slice::from_raw_parts_mut(pollfds.as_mut_ptr().cast::<PollFd>(), nfds) };

    let mut remaining_ms = (timeout >= 0).then_some(timeout as u64);
    loop {
        let ready = {
            let mut ready = 0;
            for pollfd in pollfds.iter_mut() {
                pollfd.revents = 0;
                if pollfd.fd < 0 {
                    continue;
                }
                let mut revents = 0;
                let file_revents = {
                    let table = OPEN_FILES.lock();
                    table.get(pollfd.fd).map(|file| {
                        let mut revents = 0;
                        if pollfd.events & TRUEOS_POLLIN != 0 && open_file_read_ready(file) {
                            revents |= TRUEOS_POLLIN;
                        }
                        if pollfd.events & TRUEOS_POLLOUT != 0 && open_file_write_ready(file) {
                            revents |= TRUEOS_POLLOUT;
                        }
                        revents
                    })
                };
                if let Some(file_revents) = file_revents {
                    revents = file_revents;
                } else if let Some(socket_revents) =
                    crate::std_abi_shim::socket_poll_events(pollfd.fd, pollfd.events)
                {
                    revents = socket_revents;
                } else if (0..=2).contains(&pollfd.fd) {
                    if pollfd.fd == 0
                        && pollfd.events & TRUEOS_POLLIN != 0
                        && crate::r::io::fs_cabi::trueos_cabi_shell_attached_readable_len() != 0
                    {
                        revents |= TRUEOS_POLLIN;
                    }
                    if pollfd.events & TRUEOS_POLLOUT != 0 {
                        revents |= TRUEOS_POLLOUT;
                    }
                } else {
                    revents |= TRUEOS_POLLNVAL;
                }
                pollfd.revents = revents;
                if revents != 0 {
                    ready += 1;
                }
            }
            ready
        };

        if ready != 0 {
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            return ready;
        }

        let sleep_ms = match remaining_ms {
            Some(0) => {
                TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                return 0;
            }
            Some(remaining) => remaining.min(10),
            None => 10,
        };
        crate::r::io::fs_cabi::trueos_cabi_sleep_ms(sleep_ms);
        if let Some(remaining) = &mut remaining_ms {
            *remaining = remaining.saturating_sub(sleep_ms);
        }
    }
}
'''

new_poll = '''#[unsafe(no_mangle)]
pub unsafe extern "C" fn poll(fds: *mut PollFd, nfds: usize, timeout: c_int) -> c_int {
    if nfds != 0 && fds.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }

    let Some(pollfds) =
        abi_write_bytes(fds.cast::<u8>(), nfds.saturating_mul(core::mem::size_of::<PollFd>()))
    else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };
    let pollfds = unsafe { slice::from_raw_parts_mut(pollfds.as_mut_ptr().cast::<PollFd>(), nfds) };

    let blueprint_wait = crate::hv::current_hull_guest_context_vm_id().is_some()
        || crate::hv::current_guest_execution_context_vm_id().is_some();
    let deadline_ns = (timeout >= 0).then(|| {
        crate::r::platform::trueos_platform_monotonic_nanos()
            .saturating_add((timeout as u64).saturating_mul(1_000_000))
    });
    let mut woke_without_ready = false;

    loop {
        // Observe before probing. A producer racing with the scan advances this
        // generation, so wait_after cannot lose the readiness edge.
        let observed = blueprint_wait.then(|| {
            crate::r::platform::trueos_tokio_platform_wait_observe(
                crate::wait::BLUEPRINT_IO_WAIT_KEY,
            )
        });

        let ready = {
            let mut ready = 0;
            for pollfd in pollfds.iter_mut() {
                pollfd.revents = 0;
                if pollfd.fd < 0 {
                    continue;
                }
                let mut revents = 0;
                let file_revents = {
                    let table = OPEN_FILES.lock();
                    table.get(pollfd.fd).map(|file| {
                        let mut revents = 0;
                        if pollfd.events & TRUEOS_POLLIN != 0 && open_file_read_ready(file) {
                            revents |= TRUEOS_POLLIN;
                        }
                        if pollfd.events & TRUEOS_POLLOUT != 0 && open_file_write_ready(file) {
                            revents |= TRUEOS_POLLOUT;
                        }
                        revents
                    })
                };
                if let Some(file_revents) = file_revents {
                    revents = file_revents;
                } else if let Some(socket_revents) =
                    crate::std_abi_shim::socket_poll_events(pollfd.fd, pollfd.events)
                {
                    revents = socket_revents;
                } else if (0..=2).contains(&pollfd.fd) {
                    if pollfd.fd == 0
                        && pollfd.events & TRUEOS_POLLIN != 0
                        && crate::r::io::fs_cabi::trueos_cabi_shell_attached_readable_len() != 0
                    {
                        revents |= TRUEOS_POLLIN;
                    }
                    if pollfd.events & TRUEOS_POLLOUT != 0 {
                        revents |= TRUEOS_POLLOUT;
                    }
                } else {
                    revents |= TRUEOS_POLLNVAL;
                }
                pollfd.revents = revents;
                if revents != 0 {
                    ready += 1;
                }
            }
            ready
        };

        if ready != 0 {
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            return ready;
        }
        if timeout == 0 || woke_without_ready {
            // A readiness-generation wake with no fd event is intentionally a
            // spurious poll return. It lets typed terminal/control-plane state
            // (for example a surface resize) reach the userspace event source
            // without inventing a fake readable byte.
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            return 0;
        }

        if !blueprint_wait {
            let sleep_ms = match deadline_ns {
                Some(deadline) => {
                    let now = crate::r::platform::trueos_platform_monotonic_nanos();
                    if now >= deadline {
                        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                        return 0;
                    }
                    deadline.saturating_sub(now).div_ceil(1_000_000).min(10).max(1)
                }
                None => 10,
            };
            crate::r::io::fs_cabi::trueos_cabi_sleep_ms(sleep_ms);
            continue;
        }

        let wait_ms = match deadline_ns {
            Some(deadline) => {
                let now = crate::r::platform::trueos_platform_monotonic_nanos();
                if now >= deadline {
                    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                    return 0;
                }
                deadline.saturating_sub(now).div_ceil(1_000_000).max(1)
            }
            None => u64::MAX,
        };
        woke_without_ready = crate::r::platform::trueos_tokio_platform_wait_after(
            crate::wait::BLUEPRINT_IO_WAIT_KEY,
            observed.unwrap_or_default(),
            wait_ms,
        );
    }
}
'''
rep("src/unix_abi_shim.rs", old_poll, new_poll)

# Local pipe/socketpair writes are Mio's internal wake mechanism on the Unix
# backend. Make them advance the same VM-local generation after the byte is
# visible, never while holding the process table/pipe lock.
ins_after(
    "src/std_abi_shim.rs",
    '''fn active_abi_guest_vm_id() -> Option<u8> {
    crate::hv::current_guest_execution_context_vm_id()
        .or_else(crate::hv::current_vm_id_by_lapic_low)
}
''',
    '''

#[inline]
fn wake_current_blueprint_io() {
    if crate::hv::current_hull_guest_context_vm_id().is_some()
        || crate::hv::current_guest_execution_context_vm_id().is_some()
    {
        let _ = crate::r::platform::trueos_tokio_platform_wake_all(
            crate::wait::BLUEPRINT_IO_WAIT_KEY,
        );
    }
}
''',
)

rep(
    "src/std_abi_shim.rs",
    '''    let mut table = OPEN_FILES.lock();
    let Some(file) = table.get_mut(fd) else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    };
    match file {
''',
    '''    let mut table = OPEN_FILES.lock();
    let Some(file) = table.get_mut(fd) else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    };
    let mut notify_io = false;
    match file {
''',
)
rep(
    "src/std_abi_shim.rs",
    '''            pipe.bytes.extend_from_slice(input);
        }
        OpenFile::UnixSocket { tx, .. } => {
''',
    '''            pipe.bytes.extend_from_slice(input);
            notify_io = true;
        }
        OpenFile::UnixSocket { tx, .. } => {
''',
)
rep(
    "src/std_abi_shim.rs",
    '''            tx.bytes.extend_from_slice(input);
        }
        OpenFile::PipeRead { .. } => {
''',
    '''            tx.bytes.extend_from_slice(input);
            notify_io = true;
        }
        OpenFile::PipeRead { .. } => {
''',
)
rep(
    "src/std_abi_shim.rs",
    '''    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    input.len() as isize
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize {
''',
    '''    }
    drop(table);
    if notify_io {
        wake_current_blueprint_io();
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    input.len() as isize
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize {
''',
)

rep(
    "src/std_abi_shim.rs",
    '''    if fd == 0 {
        loop {
            let n = unsafe { sys_read(fd as u32, buf.cast::<u8>(), count) };
            if n != 0 {
                TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                return n as isize;
            }
            if STD_FD_FLAGS[0].load(Ordering::Relaxed) & TRUEOS_O_NONBLOCK != 0 {
                TRUEOS_ERRNO.store(TRUEOS_EAGAIN, Ordering::Relaxed);
                return -1;
            }

            // An attached console with no queued bytes is not at EOF. Yield the
            // guest until input arrives, matching blocking Unix terminal reads.
            crate::r::io::fs_cabi::trueos_cabi_sleep_ms(10);
        }
    }
''',
    '''    if fd == 0 {
        loop {
            let observed = crate::r::platform::trueos_tokio_platform_wait_observe(
                crate::wait::BLUEPRINT_IO_WAIT_KEY,
            );
            let n = unsafe { sys_read(fd as u32, buf.cast::<u8>(), count) };
            if n != 0 {
                TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                return n as isize;
            }
            if STD_FD_FLAGS[0].load(Ordering::Relaxed) & TRUEOS_O_NONBLOCK != 0 {
                TRUEOS_ERRNO.store(TRUEOS_EAGAIN, Ordering::Relaxed);
                return -1;
            }

            // The attached console is a stream, not EOF. Park on the same
            // readiness generation used by poll/Mio until RX/control state moves.
            let _ = crate::r::platform::trueos_tokio_platform_wait_after(
                crate::wait::BLUEPRINT_IO_WAIT_KEY,
                observed,
                u64::MAX,
            );
        }
    }
''',
)

rep(
    "src/std_abi_shim.rs",
    '''        tx.bytes.extend_from_slice(input);
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        return input.len() as isize;
    }
''',
    '''        tx.bytes.extend_from_slice(input);
        let written = input.len();
        drop(tx);
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        wake_current_blueprint_io();
        return written as isize;
    }
''',
)

# Network adapter progress is a coarse readiness edge. Wake only already
# registered Blueprint I/O queues; Mio performs exact fd/token filtering after
# the VM lane resumes.
rep(
    "src/mio_compat.rs",
    '''pub(crate) fn notify_net_event() {
    MIO_SELECTOR_WAIT.notify_all();
}

pub(crate) unsafe fn mio_selector_wake_host(_selector_id: usize) -> i32 {
    MIO_SELECTOR_WAIT.notify_all();
    STATUS_OK
}
''',
    '''pub(crate) fn notify_net_event() {
    MIO_SELECTOR_WAIT.notify_all();
    let _ = crate::wait::platform_wake_all_blueprint_io_waiters();
}

pub(crate) unsafe fn mio_selector_wake_host(_selector_id: usize) -> i32 {
    MIO_SELECTOR_WAIT.notify_all();
    if let Some(vm_id) = current_owner_vm() {
        let _ = crate::wait::platform_wake_blueprint_io_for_vm(vm_id);
    } else {
        let _ = crate::wait::platform_wake_all_blueprint_io_waiters();
    }
    STATUS_OK
}
''',
)

# NetShell direct RX and typed surface changes are both terminal readiness
# edges. They wake the owner VM but remain separate terminal state/data planes.
rep(
    "src/shell2/backends/net_tcp.rs",
    '''impl NetShellOwnershipSnapshot {
    pub(crate) const fn direct_active(self) -> bool {
        self.owner != 0
    }

    pub(crate) const fn direct_passthrough_active(self) -> bool {
        (self.owner & TerminalHandoffOwner::STREAM_KIND) != 0
    }
}
''',
    '''impl NetShellOwnershipSnapshot {
    pub(crate) const fn direct_active(self) -> bool {
        self.owner != 0
    }

    pub(crate) const fn direct_passthrough_active(self) -> bool {
        (self.owner & TerminalHandoffOwner::STREAM_KIND) != 0
    }

    pub(crate) const fn blueprint_vm(self) -> Option<u8> {
        if self.owner != 0 && (self.owner & TerminalHandoffOwner::STREAM_KIND) == 0 {
            Some(self.owner.saturating_sub(1) as u8)
        } else {
            None
        }
    }
}
''',
)

rep(
    "src/shell2/backends/net_tcp.rs",
    '''pub(crate) fn update_net_shell_surface_size(cols: usize, rows: usize) -> bool {
    if cols == 0 || rows == 0 {
        return false;
    }
    let cols = cols.min(u32::MAX as usize) as u32;
    let rows = rows.min(u32::MAX as usize) as u32;
    let mut st = NET_SHELL_STATE.lock();
    if st.surface_cols == cols && st.surface_rows == rows {
        return false;
    }
    st.surface_cols = cols;
    st.surface_rows = rows;
    true
}
''',
    '''pub(crate) fn update_net_shell_surface_size(cols: usize, rows: usize) -> bool {
    if cols == 0 || rows == 0 {
        return false;
    }
    let cols = cols.min(u32::MAX as usize) as u32;
    let rows = rows.min(u32::MAX as usize) as u32;
    let owner = {
        let mut st = NET_SHELL_STATE.lock();
        if st.surface_cols == cols && st.surface_rows == rows {
            return false;
        }
        st.surface_cols = cols;
        st.surface_rows = rows;
        NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire)
    };
    if owner != 0 && (owner & TerminalHandoffOwner::STREAM_KIND) == 0 {
        let _ = crate::wait::platform_wake_blueprint_io_for_vm(owner.saturating_sub(1) as u8);
    }
    true
}
''',
)

rep(
    "src/shell2/backends/net_tcp.rs",
    '''    for &byte in bytes {
        if st.rx.len() >= NET_SHELL_RX_CAP {
            let _ = st.rx.pop_front();
        }
        st.rx.push_back(byte);
    }
    true
}
''',
    '''    for &byte in bytes {
        if st.rx.len() >= NET_SHELL_RX_CAP {
            let _ = st.rx.pop_front();
        }
        st.rx.push_back(byte);
    }
    drop(st);
    if !bytes.is_empty()
        && let Some(vm_id) = snapshot.blueprint_vm()
    {
        let _ = crate::wait::platform_wake_blueprint_io_for_vm(vm_id);
    }
    true
}
''',
)

print("Blueprint Mio registration convergence materialized")
