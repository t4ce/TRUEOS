#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{LazyLock, Mutex as StdMutex, MutexGuard};

pub struct Mutex<T>(StdMutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self(StdMutex::new(value))
    }
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

const AP: u32 = 7;
#[derive(Default)]
struct World {
    owners: BTreeMap<u8, shell3::MatrixSlotLease>,
    runs: BTreeMap<u8, u64>,
    live: BTreeSet<shell3::MatrixSlotLease>,
    reserved: Vec<u32>,
    released: Vec<u32>,
    dropped: Vec<shell3::MatrixSlotLease>,
    tty_created: usize,
    tty_input: Vec<Vec<u8>>,
    tty_resized: Vec<(usize, usize)>,
    tty_polled: usize,
    next_slot: u64,
    provider_calls: Vec<&'static str>,
    provider_payload_bytes: usize,
}
static WORLD: LazyLock<StdMutex<World>> = LazyLock::new(|| StdMutex::new(World::default()));
fn world() -> MutexGuard<'static, World> {
    WORLD.lock().unwrap_or_else(|e| e.into_inner())
}
fn lines(provider: &'static str) -> Vec<String> {
    let mut state = world();
    state.provider_calls.push(provider);
    vec![format!(
        "{provider}: Grüß § λ{}",
        "x".repeat(state.provider_payload_bytes)
    )]
}

mod hv {
    pub fn blueprint_console_target(vm: u8) -> Option<crate::shell3::MatrixTarget> {
        crate::world()
            .owners
            .get(&vm)
            .cloned()
            .map(crate::shell3::MatrixTarget)
    }
    pub fn blueprint_process_env_text(vm: u8) -> Option<String> {
        Some(format!("TRUEOS_VM_ID={vm}\nGREETING=Grüß § λ"))
    }
    pub fn vm_run_generation(vm: u8) -> Option<u64> {
        crate::world().runs.get(&vm).copied()
    }
}
mod allocators {
    pub fn with_host_alloc_domain_strong<T>(run: impl FnOnce() -> T) -> T {
        run()
    }
}
mod r {
    pub mod restart {
        pub fn startup_alias_names() -> Vec<String> {
            vec!["codex".into(), "calc".into()]
        }
    }
}

mod shell3 {
    pub const MAX_SHELL3_INSTANCES: usize = 256;
    pub const MIN_COLUMNS: usize = 20;
    pub const MIN_ROWS: usize = 5;
    #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
    pub(crate) struct MatrixSlotLease {
        pub id: String,
        pub generation: u64,
    }
    pub(crate) struct MatrixTarget(pub MatrixSlotLease);
    pub(crate) fn matrix_target_slot_lease(target: &MatrixTarget) -> MatrixSlotLease {
        target.0.clone()
    }
    pub(crate) struct MatrixSlots;
    impl MatrixSlots {
        pub(crate) fn drop_lease(lease: &MatrixSlotLease) -> bool {
            let mut state = crate::world();
            state.dropped.push(lease.clone());
            state.live.remove(lease)
        }
    }
    pub(crate) mod matrix_target {
        pub(crate) fn matrix_slot_is_live(lease: &super::MatrixSlotLease) -> bool {
            crate::world().live.contains(lease)
        }
    }
    pub(crate) mod service {
        pub(crate) fn refresh_appdb_names() {}
        pub(crate) fn reserve_terminal_slot() -> Result<u32, ()> {
            crate::world().reserved.push(crate::AP);
            Ok(crate::AP)
        }
        pub(crate) fn release_shell_on_executor(slot: u32) {
            crate::world().released.push(slot);
        }
        pub(crate) fn notify_work() {}
        pub(crate) fn hv_status_lines() -> Vec<String> {
            crate::lines("status")
        }
        pub(crate) fn peer_lines() -> Vec<String> {
            crate::lines("peer")
        }
        pub(crate) fn probe_lines() -> Vec<String> {
            crate::lines("probe")
        }
        pub(crate) fn read_appdb_names() -> Vec<String> {
            vec!["calc".into(), "paper".into()]
        }
    }
    pub(crate) mod ram {
        pub(crate) fn memory_modules_snapshot(width: usize) -> Vec<String> {
            assert_eq!(width, 80);
            crate::lines("modules")
        }
        pub(crate) fn usage_snapshot(width: usize) -> Vec<String> {
            assert_eq!(width, 80);
            crate::lines("ram")
        }
    }
    pub(crate) mod smp {
        pub(crate) fn snapshot(width: usize) -> Vec<String> {
            assert_eq!(width, 80);
            crate::lines("smp")
        }
    }
    pub(crate) mod gpu {
        pub(crate) fn snapshot(width: usize) -> Vec<String> {
            assert_eq!(width, 80);
            crate::lines("gpu")
        }
    }
    pub(crate) mod system_net {
        pub(crate) fn nic_snapshot(width: usize, interface: Option<usize>) -> Vec<String> {
            assert_eq!(width, 80);
            assert_eq!(interface, None);
            crate::lines("net")
        }
    }
    pub(crate) struct Shell3 {
        slot: u32,
        lease: MatrixSlotLease,
    }
    impl Shell3 {
        pub(crate) fn new_terminal_sized_reserved(
            slot: u32,
            _: Option<u16>,
            _: usize,
            _: usize,
        ) -> Self {
            let mut state = crate::world();
            state.next_slot += 1;
            let lease = MatrixSlotLease {
                id: format!("tty{}", state.next_slot),
                generation: state.next_slot,
            };
            state.live.insert(lease.clone());
            Self { slot, lease }
        }
        pub(crate) fn active_matrix_lease(&self) -> Option<MatrixSlotLease> {
            Some(self.lease.clone())
        }
    }
    impl Drop for Shell3 {
        fn drop(&mut self) {
            service::release_shell_on_executor(self.slot);
        }
    }
    pub(crate) mod tty {
        pub(crate) const OUTPUT_LIMIT: usize = 1024 * 1024;
        pub(crate) struct Terminal {
            shell: super::Shell3,
            pub output: Vec<u8>,
            pub closing: bool,
            pub overflow: bool,
        }
        impl Terminal {
            pub(crate) fn new_ssh(shell: super::Shell3) -> Self {
                crate::world().tty_created += 1;
                Self {
                    shell,
                    output: b"\x1b[?1049hTTY\n".to_vec(),
                    closing: false,
                    overflow: false,
                }
            }
            pub(crate) fn input(&mut self, bytes: &[u8]) {
                crate::world().tty_input.push(bytes.to_vec());
                self.output.extend_from_slice(bytes);
                if bytes == b"exit\r" {
                    self.closing = true;
                }
            }
            pub(crate) fn resize(&mut self, columns: usize, rows: usize) {
                crate::world().tty_resized.push((columns, rows));
            }
            pub(crate) fn reconcile_matrix_selection(&mut self) {
                crate::world().tty_polled += 1;
            }
        }
    }
    #[path = "@TRUEOS_ROOT@/src/shell3/captured.rs"]
    pub(crate) mod captured;
    #[path = "@TRUEOS_ROOT@/src/shell3/sessions.rs"]
    pub(crate) mod sessions;
    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::shell3;
        use crate::{AP, World, world};
        struct SessionTest {
            worker: sessions::Worker,
            handles: Vec<u64>,
        }
        impl SessionTest {
            fn new() -> Self {
                *world() = World::default();
                let mut state = world();
                for vm in [1, 2] {
                    let lease = shell3::MatrixSlotLease {
                        id: format!("owner{vm}"),
                        generation: 1,
                    };
                    state.live.insert(lease.clone());
                    state.owners.insert(vm, lease);
                    state.runs.insert(vm, 1);
                }
                drop(state);
                Self {
                    worker: sessions::Worker::new(AP),
                    handles: Vec::new(),
                }
            }
            fn open(&mut self, script: &[u8], size: (usize, usize), stdin_open: bool) -> u64 {
                let handle = sessions::open(1, script, size.0, size.1, stdin_open);
                assert!(handle > 0, "open returned {handle}");
                self.handles.push(handle as u64);
                handle as u64
            }
            fn poll(&mut self, count: usize) {
                for _ in 0..count {
                    self.worker.poll();
                }
            }
            fn read(&self, handle: u64, capacity: usize) -> (Vec<u8>, Option<i32>) {
                let mut bytes = vec![0; capacity];
                let count = sessions::read(1, handle, &mut bytes);
                assert!(count >= 8, "read returned {count}");
                bytes.truncate(count as usize);
                let exit = (u32::from_le_bytes(bytes[..4].try_into().unwrap()) != 0)
                    .then(|| i32::from_le_bytes(bytes[4..8].try_into().unwrap()));
                (bytes[8..].to_vec(), exit)
            }
            fn drain(&self, handle: u64) -> (String, Option<i32>) {
                let mut output = Vec::new();
                for _ in 0..8192 {
                    let (bytes, exit) = self.read(handle, 257);
                    let empty = bytes.is_empty();
                    output.extend(bytes);
                    if empty || exit.is_some() {
                        return (
                            String::from_utf8(output).expect("captured UTF-8 result"),
                            exit,
                        );
                    }
                }
                panic!("output did not drain");
            }
        }
        impl Drop for SessionTest {
            fn drop(&mut self) {
                for &handle in &self.handles {
                    sessions::control(1, handle, 5, 0, 0);
                }
                self.poll(3);
                assert!(
                    self.worker.is_empty(),
                    "session registry retained a retired owner"
                );
                let state = world();
                assert_eq!(
                    state.reserved, state.released,
                    "each reservation must be released exactly once on its owning AP"
                );
            }
        }

        #[test]
        fn no_tty_session_returns_plain_existing_service_data_and_finite_exit() {
            let mut test = SessionTest::new();
            let handle = test.open(b"ram\nsmp\ngpu\nnet\n", (0, 0), false);
            assert!(test.worker.pending());
            test.poll(12);
            let (output, exit) = test.drain(handle);
            for provider in ["modules", "ram", "smp", "gpu", "net"] {
                assert!(
                    output.contains(&format!("{provider}: Grüß § λ")),
                    "missing {provider}: {output:?}"
                );
            }
            assert!(
                !output.contains('\u{1b}'),
                "captured output must not carry terminal controls"
            );
            assert_eq!(exit, Some(0));
            assert_eq!(world().tty_created, 0);
            assert_eq!(world().released, vec![AP]);
            assert_eq!(sessions::write(1, handle, b"ram\n"), -3);
        }

        #[test]
        fn open_stdin_preserves_a_batch_session_until_explicit_eof() {
            let mut test = SessionTest::new();
            let handle = test.open(b"status\nhv peer\n", (0, 0), true);
            test.poll(8);
            let (output, exit) = test.drain(handle);
            assert!(output.contains("status:"));
            assert!(output.contains("peer:"));
            assert_eq!(exit, None);
            assert_eq!(sessions::control(1, handle, 2, 0, 0), 0);
            test.poll(3);
            assert_eq!(test.drain(handle).1, Some(0));
        }

        #[test]
        fn worker_preserves_utf8_scalar_and_crlf_split_between_multiple_writes() {
            let mut test = SessionTest::new();
            let handle = test.open(b"", (0, 0), true);
            for fragment in [&b"ram\xe2"[..], &b"\x80"[..], &b"\x82\r"[..], &b"\n"[..]] {
                assert_eq!(
                    sessions::write(1, handle, fragment),
                    fragment.len() as isize
                );
                test.poll(1);
            }
            assert_eq!(sessions::control(1, handle, 2, 0, 0), 0);
            test.poll(3);
            let (output, exit) = test.drain(handle);
            assert!(output.contains("ram: Grüß § λ"));
            assert!(!output.contains("error:"));
            assert!(!output.contains('\u{1b}'));
            assert_eq!(exit, Some(0));
            assert_eq!(
                world()
                    .provider_calls
                    .iter()
                    .filter(|&&name| name == "ram")
                    .count(),
                1
            );
        }

        #[test]
        fn closing_stdin_with_incomplete_utf8_reports_error_and_still_completes() {
            let mut test = SessionTest::new();
            let handle = test.open(b"", (0, 0), true);
            assert_eq!(sessions::write(1, handle, b"ram\xe2"), 4);
            test.poll(1);
            assert_eq!(sessions::control(1, handle, 2, 0, 0), 0);
            test.poll(3);
            let (output, exit) = test.drain(handle);
            assert!(output.contains("not valid UTF-8"), "{output:?}");
            assert_eq!(exit, Some(2));
            assert!(world().provider_calls.is_empty());
        }

        #[test]
        fn native_input_rejects_terminal_keys_without_executing_a_trimmed_command() {
            let mut test = SessionTest::new();
            let handle = test.open(b"\t\tram\nsmp", (0, 0), false);
            test.poll(6);
            let (output, exit) = test.drain(handle);
            assert!(output.contains("control input is not a native command"));
            assert!(output.contains("smp: Grüß § λ"));
            assert!(!output.contains('\u{1b}'));
            assert_eq!(exit, Some(2));
            assert_eq!(world().provider_calls, vec!["smp"]);
        }

        #[test]
        fn native_environment_is_the_calling_vm_environment_and_not_a_shell_model() {
            let mut test = SessionTest::new();
            let handle = test.open(b"env", (0, 0), false);
            test.poll(3);
            assert_eq!(
                test.drain(handle),
                ("TRUEOS_VM_ID=1\nGREETING=Grüß § λ\n".into(), Some(0))
            );
            assert_eq!(world().tty_created, 0);
        }

        #[test]
        fn empty_initial_script_supports_incremental_writes_and_close_stdin() {
            let mut test = SessionTest::new();
            let handle = test.open(b"", (0, 0), true);
            test.poll(2);
            assert_eq!(test.read(handle, 32), (vec![], None));
            assert_eq!(sessions::write(1, handle, b"ra"), 2);
            test.poll(1);
            assert_eq!(test.read(handle, 32), (vec![], None));
            assert_eq!(sessions::write(1, handle, b"m\r"), 2);
            test.poll(3);
            let (output, exit) = test.drain(handle);
            assert!(output.contains("ram:"));
            assert_eq!(exit, None);
            assert_eq!(sessions::write(1, handle, b"\nsmp"), 4);
            assert_eq!(sessions::control(1, handle, 2, 0, 0), 0);
            assert_eq!(sessions::write(1, handle, b"more"), -3);
            test.poll(5);
            let (output, exit) = test.drain(handle);
            assert!(output.contains("smp:"));
            assert_eq!(exit, Some(0));
            assert_eq!(
                world()
                    .provider_calls
                    .iter()
                    .filter(|&&value| value == "ram")
                    .count(),
                1
            );
            assert_eq!(
                world()
                    .provider_calls
                    .iter()
                    .filter(|&&value| value == "smp")
                    .count(),
                1
            );
        }

        #[test]
        fn explicit_tty_geometry_keeps_its_terminal_frontend_and_resize_contract() {
            let mut test = SessionTest::new();
            let handle = test.open(b"help", (100, 25), true);
            test.poll(2);
            let (output, exit) = test.drain(handle);
            assert!(output.starts_with("\x1b[?1049hTTY\n"));
            assert_eq!(exit, None);
            assert_eq!(world().tty_created, 1);
            assert_eq!(world().tty_input, vec![b"help\r".to_vec()]);
            assert_eq!(sessions::control(1, handle, 1, 120, 40), 0);
            test.poll(1);
            assert_eq!(world().tty_resized, vec![(120, 40)]);
            assert_eq!(sessions::write(1, handle, b"exit\r"), 5);
            test.poll(2);
            assert_eq!(test.drain(handle).1, Some(0));
            assert_eq!(world().released, vec![AP]);
            assert_eq!(world().dropped.len(), 1);
        }

        #[test]
        fn wrong_vm_and_recreated_slot_cannot_read_write_or_control_old_session() {
            let mut test = SessionTest::new();
            let handle = test.open(b"", (0, 0), true);
            test.poll(1);
            let mut bytes = [0; 32];
            assert_eq!(sessions::read(2, handle, &mut bytes), -3);
            assert_eq!(sessions::write(2, handle, b"ram\n"), -3);
            assert_eq!(sessions::control(2, handle, 4, 0, 0), -3);
            let old = world().owners.get(&1).cloned().unwrap();
            let replacement = shell3::MatrixSlotLease {
                generation: 2,
                ..old.clone()
            };
            {
                let mut state = world();
                state.live.remove(&old);
                state.live.insert(replacement.clone());
                state.owners.insert(1, replacement);
            }
            assert_eq!(sessions::read(1, handle, &mut bytes), -3);
            assert_eq!(sessions::write(1, handle, b"ram\n"), -3);
            assert_eq!(sessions::control(1, handle, 4, 0, 0), -3);
            test.poll(3);
            assert!(test.worker.is_empty());
            assert_eq!(world().released, vec![AP]);
        }

        #[test]
        fn retirement_before_worker_claim_releases_once_without_constructing_tty() {
            let mut test = SessionTest::new();
            let handle = test.open(b"ram", (0, 0), false);
            assert_eq!(sessions::control(1, handle, 5, 0, 0), 0);
            test.poll(3);
            assert_eq!(world().released, vec![AP]);
            assert_eq!(world().tty_created, 0);
            assert!(world().provider_calls.is_empty());
            assert!(test.worker.is_empty());
        }

        #[test]
        fn retiring_claimed_capture_preserves_its_callers_matrix_slot() {
            let mut test = SessionTest::new();
            let handle = test.open(b"", (0, 0), true);
            test.poll(1);
            assert_eq!(sessions::control(1, handle, 5, 0, 0), 0);
            test.poll(3);
            let state = world();
            assert_eq!(state.released, vec![AP]);
            assert!(state.dropped.is_empty());
            assert!(
                state
                    .owners
                    .values()
                    .all(|lease| state.live.contains(lease))
            );
            assert!(test.worker.is_empty());
        }

        #[test]
        fn another_ap_worker_cannot_claim_or_release_the_reserved_session() {
            let mut test = SessionTest::new();
            let handle = test.open(b"smp", (0, 0), false);
            let mut another_ap = sessions::Worker::new(AP + 1);
            assert!(!another_ap.pending());
            another_ap.poll();
            assert!(another_ap.is_empty());
            assert!(world().released.is_empty());
            assert!(world().provider_calls.is_empty());
            test.poll(3);
            assert_eq!(test.drain(handle).1, Some(0));
            assert_eq!(world().released, vec![AP]);
        }

        #[test]
        fn restarted_vm_cannot_reuse_old_handle_even_when_matrix_lease_is_identical() {
            let mut test = SessionTest::new();
            let handle = test.open(b"", (0, 0), true);
            test.poll(1);
            world().runs.insert(1, 2);
            let mut bytes = [0; 32];
            assert_eq!(sessions::read(1, handle, &mut bytes), -3);
            assert_eq!(sessions::write(1, handle, b"ram\n"), -3);
            assert_eq!(sessions::control(1, handle, 4, 0, 0), -3);
            test.poll(3);
            assert!(test.worker.is_empty());
            assert_eq!(world().released, vec![AP]);
        }

        #[test]
        fn dead_vm_before_claim_releases_its_reserved_ap_slot_exactly_once() {
            let mut test = SessionTest::new();
            test.open(b"ram", (0, 0), false);
            world().runs.remove(&1);
            test.poll(3);
            assert!(test.worker.is_empty());
            assert_eq!(world().released, vec![AP]);
            assert!(world().provider_calls.is_empty());
        }

        #[test]
        fn terminate_stops_queued_commands_and_reports_137() {
            let mut test = SessionTest::new();
            let handle = test.open(b"", (0, 0), true);
            test.poll(1);
            assert_eq!(sessions::write(1, handle, b"ram\nsmp\n"), 8);
            assert_eq!(sessions::control(1, handle, 4, 0, 0), 0);
            test.poll(3);
            assert_eq!(test.drain(handle).1, Some(137));
            assert!(world().provider_calls.is_empty());
            assert_eq!(world().released, vec![AP]);
        }

        #[test]
        fn interrupt_reports_130_and_does_not_run_queued_commands() {
            let mut test = SessionTest::new();
            let handle = test.open(b"", (0, 0), true);
            test.poll(1);
            assert_eq!(sessions::write(1, handle, b"ram\nsmp\n"), 8);
            assert_eq!(sessions::control(1, handle, 3, 0, 0), 0);
            test.poll(3);
            let (output, exit) = test.drain(handle);
            assert!(output.contains("interrupted"));
            assert_eq!(exit, Some(130));
            assert!(world().provider_calls.is_empty());
            assert_eq!(world().released, vec![AP]);
        }

        #[test]
        fn queue_capacity_has_explicit_partial_acceptance_and_can_continue_without_loss() {
            let mut test = SessionTest::new();
            let handle = test.open(b"", (0, 0), true);
            let blanks = vec![b'\n'; 64 * 1024 + 99];
            assert_eq!(sessions::write(1, handle, &blanks), 64 * 1024);
            assert_eq!(sessions::write(1, handle, b"smp\n"), 0);
            test.poll(1);
            assert_eq!(sessions::write(1, handle, b"smp\n"), 4);
            assert_eq!(sessions::control(1, handle, 2, 0, 0), 0);
            test.poll(20);
            let (output, exit) = test.drain(handle);
            assert_eq!(output, "smp: Grüß § λ\n");
            assert_eq!(exit, Some(0));
            assert_eq!(world().provider_calls, vec!["smp"]);
        }

        #[test]
        fn unread_session_output_overflow_reports_an_explicit_error_and_releases_admission() {
            let mut test = SessionTest::new();
            world().provider_payload_bytes = 200 * 1024;
            let handle = test.open(b"smp\nsmp\nsmp\nsmp\nsmp\nsmp\n", (0, 0), false);
            test.poll(10);
            let (output, exit) = test.drain(handle);
            assert!(
                output.contains("exceeded") || output.contains("overflow"),
                "overflow must not silently discard an output batch"
            );
            assert_eq!(exit, Some(1));
            assert_eq!(world().released, vec![AP]);
        }

        #[test]
        fn read_reports_exit_only_after_all_output_bytes_have_been_delivered() {
            let mut test = SessionTest::new();
            let handle = test.open(b"ram", (0, 0), false);
            test.poll(4);
            let (first, exit) = test.read(handle, 9);
            assert_eq!(first.len(), 1);
            assert_eq!(exit, None);
            let (remaining, exit) = test.drain(handle);
            assert!(!remaining.is_empty());
            assert_eq!(exit, Some(0));
            assert_eq!(test.read(handle, 16), (vec![], Some(0)));
        }

        #[test]
        fn invalid_geometry_and_invalid_initial_bytes_are_rejected_before_admission() {
            let _test = SessionTest::new();
            for size in [
                (0, 25),
                (100, 0),
                (19, 25),
                (100, 4),
                (513, 256),
                (512, 257),
            ] {
                assert_eq!(sessions::open(1, b"ram", size.0, size.1, false), -1);
            }
            for script in [&b"ra\0m"[..], &b"\xff"[..], &vec![b'x'; 64 * 1024][..]] {
                assert_eq!(sessions::open(1, script, 0, 0, false), -1);
            }
            assert_eq!(sessions::open(99, b"ram", 0, 0, false), -3);
            assert!(world().reserved.is_empty());
        }
    }
}
