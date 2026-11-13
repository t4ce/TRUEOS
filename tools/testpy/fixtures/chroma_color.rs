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
        crate::STATE.with(|s| s.borrow().physical)
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
            let mut s = cell.borrow_mut();
            match off {
                0x70008 => 1 << 31,
                0x490a4 => s.control,
                0x490a8 => s.index,
                0x490ac => {
                    let i = (s.index & 8191) as usize;
                    let value = s.table[i];
                    if s.index & 8192 != 0 {
                        s.index = 8192 | ((i + 1) % 4913) as u32;
                    }
                    value
                }
                _ => 0,
            }
        })
    }
    pub fn mmio_write(_: Dev, off: usize, value: u32) {
        crate::STATE.with(|cell| {
            let mut s = cell.borrow_mut();
            s.writes.push((off, value));
            match off {
                0x490a4 => s.control = value,
                0x490a8 => s.index = value,
                0x490ac => {
                    let i = (s.index & 8191) as usize;
                    s.table[i] = value;
                    if s.index & 8192 != 0 {
                        s.index = 8192 | ((i + 1) % 4913) as u32;
                    }
                }
                _ => {}
            }
        });
    }
}
struct State {
    physical: u32,
    control: u32,
    index: u32,
    table: Box<[u32]>,
    writes: Vec<(usize, u32)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            physical: 0xa7808086,
            control: 1 << 29,
            index: 7,
            table: vec![0; 4913].into_boxed_slice(),
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
        assert_eq!(&*s.table, &table);
        assert_eq!(s.index, 7);
        assert_eq!(s.control, 0xe0000000);
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
        s.control = 0xa0000000;
        s.table[300] = 12345;
    });
    let snapshot = color::snapshot_pipe_a().unwrap();
    color::program_pipe_a(&[23; 4913]).unwrap();
    STATE.with(|c| c.borrow_mut().control &= !(1 << 30));
    color::restore_pipe_a(&snapshot).unwrap();
    STATE.with(|c| {
        let s = c.borrow();
        assert_eq!(s.table[300], 12345);
        assert_eq!(s.table[0], 0);
        assert_eq!(s.control, 0xe0000000);
    });
}

#[test]
fn status_accepts_a_pending_vblank_without_mutating_hardware() {
    assert_eq!(color::pipe_a_status(), Ok(color::PipeLutStatus::Disabled));
    STATE.with(|c| c.borrow_mut().control = 0xc000_0000);
    assert_eq!(color::pipe_a_status(), Ok(color::PipeLutStatus::Pending));
    STATE.with(|c| c.borrow_mut().control &= !(1 << 30));
    assert_eq!(color::pipe_a_status(), Ok(color::PipeLutStatus::Active));
    assert!(STATE.with(|c| c.borrow().writes.is_empty()));
}

#[test]
fn reset_detection_rearms_but_unchanged_state_never_rewrites_table() {
    let mut table = [0; 4913];
    color::Settings::default().build_table(&mut table);
    assert_eq!(color::rearm_pipe_a_if_lost(&table), Ok(true));
    STATE.with(|c| c.borrow_mut().control &= !(1 << 30));
    let writes_before = STATE.with(|c| c.borrow().writes.len());
    assert_eq!(color::rearm_pipe_a_if_lost(&table), Ok(false));
    STATE.with(|c| {
        let s = c.borrow();
        assert_eq!(s.index, 7);
        assert!(
            s.writes[writes_before..]
                .iter()
                .all(|(address, _)| *address == 0x490a8)
        );
    });
    STATE.with(|c| c.borrow_mut().table.fill(0));
    assert_eq!(color::rearm_pipe_a_if_lost(&table), Ok(true));
    STATE.with(|c| assert_eq!(&*c.borrow().table, &table));
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
    STATE.with(|c| c.borrow_mut().control &= !(1 << 30));
    color::restore_pipe_a(&snapshot).unwrap();
    STATE.with(|c| assert_eq!(c.borrow().control, 1 << 29));
}
