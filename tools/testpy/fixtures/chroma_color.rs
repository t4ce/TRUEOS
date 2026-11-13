extern crate alloc;
// Use host transcendental functions and mutexes, but include the production
// color model and transport unchanged. The modeled indirect port tracks exact
// blue-fastest table addresses, auto-increment, index restoration and arming.
extern crate self as libm;
extern crate self as spin;
pub fn expf(x: f32) -> f32 {
    x.exp()
}
pub fn sqrtf(x: f32) -> f32 {
    x.sqrt()
}
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self(std::sync::Mutex::new(value))
    }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }
}
mod pci {
    pub fn config_read_u32(_: u8, _: u8, _: u8, _: u16) -> u32 {
        crate::STATE.with(|cell| {
            let mut state = cell.borrow_mut();
            state.pci_reads += 1;
            state.physical
        })
    }
}
mod intel {
    #[derive(Clone, Copy)]
    pub struct Dev {
        pub bus: u8,
        pub slot: u8,
        pub function: u8,
        pub mmio_len: usize,
    }
    pub fn claimed_device() -> Option<Dev> {
        Some(Dev {
            bus: 0,
            slot: 0,
            function: 0,
            mmio_len: 0x100000,
        })
    }
    pub fn mmio_read(_: Dev, off: usize) -> u32 {
        crate::STATE.with(|cell| {
            let mut state = cell.borrow_mut();
            state.reads.push(off);
            let (slot, reg) = match off {
                0x70008 => return if state.pipes[0].enabled { 1 << 31 } else { 0 },
                0x71008 => return if state.pipes[1].enabled { 1 << 31 } else { 0 },
                0x420b0 => return 0,
                0x490a4 | 0x490a8 | 0x490ac => (0, off),
                0x491a4 | 0x491a8 | 0x491ac => (1, off - 0x100),
                _ => panic!("unexpected MMIO read {off:x}"),
            };
            let pipe = &mut state.pipes[slot];
            match reg {
                0x490a4 => pipe.control,
                0x490a8 => pipe.index,
                0x490ac => {
                    let i = (pipe.index & 8191) as usize;
                    let value = pipe.table[i];
                    if pipe.index & 8192 != 0 {
                        pipe.index = 8192 | ((i + 1) % 4913) as u32;
                    }
                    value
                }
                _ => unreachable!(),
            }
        })
    }
    pub fn mmio_write(_: Dev, off: usize, value: u32) {
        crate::STATE.with(|cell| {
            let mut state = cell.borrow_mut();
            state.writes.push((off, value));
            let (slot, reg) = match off {
                0x420b0 => return,
                0x490a4 | 0x490a8 | 0x490ac => (0, off),
                0x491a4 | 0x491a8 | 0x491ac => (1, off - 0x100),
                _ => panic!("unexpected MMIO write {off:x}"),
            };
            let pipe = &mut state.pipes[slot];
            match reg {
                0x490a4 => pipe.control = value,
                0x490a8 => pipe.index = value,
                0x490ac => {
                    let i = (pipe.index & 8191) as usize;
                    pipe.table[i] = value;
                    if pipe.index & 8192 != 0 {
                        pipe.index = 8192 | ((i + 1) % 4913) as u32;
                    }
                }
                _ => unreachable!(),
            }
        });
    }
}
struct Pipe {
    enabled: bool,
    control: u32,
    index: u32,
    table: Box<[u32]>,
}
impl Default for Pipe {
    fn default() -> Self {
        Self {
            enabled: true,
            control: 1 << 29,
            index: 7,
            table: vec![0; 4913].into_boxed_slice(),
        }
    }
}
struct State {
    physical: u32,
    pipes: [Pipe; 2],
    pci_reads: usize,
    reads: Vec<usize>,
    writes: Vec<(usize, u32)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            physical: 0xa7808086,
            pipes: core::array::from_fn(|_| Pipe::default()),
            pci_reads: 0,
            reads: Vec::new(),
            writes: Vec::new(),
        }
    }
}
std::thread_local! { static STATE:std::cell::RefCell<State>=std::cell::RefCell::new(State::default()); }
#[path = "@CHROMA_COLOR_PATH@"]
mod color;
#[test]
fn hardware_upload_order_ready_and_busy() {
    let mut table = [0; 4913];
    color::Settings::default().build_table(&mut table);
    assert_eq!(color::program_pipe_a(&table), Ok(()));
    STATE.with(|cell| {
        let s = cell.borrow();
        assert_eq!(&*s.pipes[0].table, &table);
        assert_eq!(s.pipes[0].index, 7);
        assert_eq!(s.pipes[0].control, 0xe0000000);
        assert_eq!(s.writes[0], (0x490a8, 8192));
        assert_eq!(s.writes[1], (0x490ac, 0));
    });
    let writes = STATE.with(|c| c.borrow().writes.len());
    assert_eq!(color::program_pipe_a(&table), Err("chroma-3dlut-busy"));
    assert_eq!(STATE.with(|c| c.borrow().writes.len()), writes);
}
#[test]
fn unsupported_never_writes() {
    STATE.with(|c| c.borrow_mut().physical = 0x12348086);
    assert_eq!(
        color::program_pipe_a(&[0; 4913]),
        Err("chroma-3dlut-device-unsupported")
    );
    assert!(STATE.with(|c| c.borrow().writes.is_empty()));
}
#[test]
fn failure_rollback_restores_preexisting_enabled_cube() {
    STATE.with(|c| {
        let mut s = c.borrow_mut();
        s.pipes[0].control = 0xa0000000;
        s.pipes[0].table[300] = 12345;
    });
    let snapshot = color::snapshot_pipe_a().unwrap();
    color::program_pipe_a(&[23; 4913]).unwrap();
    STATE.with(|c| c.borrow_mut().pipes[0].control &= !(1 << 30));
    color::restore_pipe_a(&snapshot).unwrap();
    STATE.with(|c| {
        let s = c.borrow();
        assert_eq!(s.pipes[0].table[300], 12345);
        assert_eq!(s.pipes[0].table[0], 0);
        assert_eq!(s.pipes[0].control, 0xe0000000);
    });
}

#[test]
fn status_accepts_a_pending_vblank_without_mutating_hardware() {
    assert_eq!(color::pipe_a_status(), Ok(color::PipeLutStatus::Disabled));
    STATE.with(|c| c.borrow_mut().pipes[0].control = 0xc000_0000);
    assert_eq!(color::pipe_a_status(), Ok(color::PipeLutStatus::Pending));
    STATE.with(|c| c.borrow_mut().pipes[0].control &= !(1 << 30));
    assert_eq!(color::pipe_a_status(), Ok(color::PipeLutStatus::Active));
    assert!(STATE.with(|c| c.borrow().writes.is_empty()));
}

#[test]
fn reset_detection_rearms_but_unchanged_state_never_rewrites_table() {
    let mut table = [0; 4913];
    color::Settings::default().build_table(&mut table);
    assert_eq!(color::rearm_pipe_a_if_lost(&table), Ok(true));
    STATE.with(|c| c.borrow_mut().pipes[0].control &= !(1 << 30));
    let writes_before = STATE.with(|c| c.borrow().writes.len());
    assert_eq!(color::rearm_pipe_a_if_lost(&table), Ok(false));
    STATE.with(|c| {
        let s = c.borrow();
        assert_eq!(s.pipes[0].index, 7);
        assert!(
            s.writes[writes_before..]
                .iter()
                .all(|(address, _)| *address == 0x490a8)
        );
    });
    STATE.with(|c| c.borrow_mut().pipes[0].table.fill(0));
    assert_eq!(color::rearm_pipe_a_if_lost(&table), Ok(true));
    STATE.with(|c| assert_eq!(&*c.borrow().pipes[0].table, &table));
}

#[test]
fn out_of_range_node_is_rejected_before_any_register_write() {
    let mut table = [0; 4913];
    table[4899] = 1 << 30;
    assert_eq!(
        color::program_pipe_a(&table),
        Err("chroma-3dlut-entry-range")
    );
    assert!(STATE.with(|c| c.borrow().writes.is_empty()));
}

#[test]
fn disabled_rollback_does_not_arm_a_never_consumed_ready_bit() {
    let snapshot = color::snapshot_pipe_a().unwrap();
    color::program_pipe_a(&[3; 4913]).unwrap();
    STATE.with(|c| c.borrow_mut().pipes[0].control &= !(1 << 30));
    color::restore_pipe_a(&snapshot).unwrap();
    STATE.with(|c| assert_eq!(c.borrow().pipes[0].control, 1 << 29));
}

#[test]
fn pipe_b_upload_is_independent_and_uses_its_own_ports() {
    let mut table = [0; 4913];
    color::Settings {
        saturation: 0.5,
        ..color::Settings::default()
    }
    .build_table(&mut table);
    assert_eq!(color::program_pipe(1, &table), Ok(()));
    STATE.with(|cell| {
        let s = cell.borrow();
        assert_eq!(&*s.pipes[1].table, &table);
        assert_eq!(s.pipes[1].index, 7);
        assert_eq!(s.pipes[1].control, 0xe0000000);
        assert!(s.pipes[0].table.iter().all(|&entry| entry == 0));
        assert_eq!(s.pipes[0].control, 1 << 29);
        assert!(
            s.writes
                .iter()
                .all(|(off, _)| matches!(off, 0x491a4 | 0x491a8 | 0x491ac))
        );
        assert!(
            s.reads
                .iter()
                .all(|off| matches!(off, 0x71008 | 0x491a4 | 0x491a8))
        );
    });
}

#[test]
fn missing_c_and_d_lut_blocks_never_access_hardware() {
    assert!(color::supports_pipe_lut(0));
    assert!(color::supports_pipe_lut(1));
    for pipe in [2, 3, 4, usize::MAX] {
        assert!(!color::supports_pipe_lut(pipe));
        assert_eq!(
            color::pipe_status(pipe),
            Err("chroma-3dlut-pipe-unsupported")
        );
        assert_eq!(
            color::program_pipe(pipe, &[0; 4913]),
            Err("chroma-3dlut-pipe-unsupported")
        );
        assert_eq!(
            color::rearm_pipe_if_lost(pipe, &[0; 4913]),
            Err("chroma-3dlut-pipe-unsupported")
        );
        assert_eq!(
            color::snapshot_pipe(pipe).err(),
            Some("chroma-3dlut-pipe-unsupported")
        );
    }
    STATE.with(|cell| {
        let s = cell.borrow();
        assert_eq!(s.pci_reads, 0);
        assert!(s.reads.is_empty() && s.writes.is_empty());
    });
}

#[test]
fn inactive_b_never_arms_or_uploads_but_does_not_block_a() {
    STATE.with(|cell| cell.borrow_mut().pipes[1].enabled = false);
    assert_eq!(
        color::program_pipe(1, &[0; 4913]),
        Err("chroma-pipe-unavailable")
    );
    assert_eq!(color::pipe_status(1), Err("chroma-pipe-unavailable"));
    STATE.with(|cell| {
        let s = cell.borrow();
        assert!(s.reads.iter().all(|off| *off == 0x71008));
        assert!(s.writes.is_empty());
    });
    assert_eq!(color::program_pipe(0, &[0; 4913]), Ok(()));
}

#[test]
fn busy_a_and_b_statuses_are_independent() {
    STATE.with(|cell| cell.borrow_mut().pipes[0].control |= 1 << 30);
    assert_eq!(color::pipe_status(0), Ok(color::PipeLutStatus::Pending));
    assert_eq!(color::pipe_status(1), Ok(color::PipeLutStatus::Disabled));
    assert_eq!(color::program_pipe(0, &[1; 4913]), Err("chroma-3dlut-busy"));
    assert_eq!(color::program_pipe(1, &[2; 4913]), Ok(()));
    STATE.with(|cell| {
        let s = cell.borrow();
        assert!(s.pipes[0].table.iter().all(|&entry| entry == 0));
        assert!(s.pipes[1].table.iter().all(|&entry| entry == 2));
    });
}

#[test]
fn b_reset_recovery_and_rollback_never_touch_a() {
    let mut identity = [0; 4913];
    color::Settings::default().build_table(&mut identity);
    assert_eq!(color::rearm_pipe_if_lost(1, &identity), Ok(true));
    STATE.with(|cell| cell.borrow_mut().pipes[1].control &= !(1 << 30));
    assert_eq!(color::rearm_pipe_if_lost(1, &identity), Ok(false));
    let snapshot = color::snapshot_pipe(1).unwrap();
    assert_eq!(
        color::restore_pipe_a(&snapshot),
        Err("chroma-3dlut-snapshot-pipe")
    );
    assert_eq!(color::program_pipe(1, &[3; 4913]), Ok(()));
    STATE.with(|cell| cell.borrow_mut().pipes[1].control &= !(1 << 30));
    assert_eq!(color::restore_pipe(&snapshot), Ok(()));
    STATE.with(|cell| {
        let s = cell.borrow();
        assert_eq!(&*s.pipes[1].table, &identity);
        assert!(s.pipes[0].table.iter().all(|&entry| entry == 0));
        assert!(
            s.writes
                .iter()
                .all(|(off, _)| matches!(off, 0x491a4 | 0x491a8 | 0x491ac))
        );
    });
}
