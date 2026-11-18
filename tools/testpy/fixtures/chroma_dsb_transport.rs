extern crate self as spin;

use std::cell::RefCell;
use std::collections::BTreeMap;

pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self(std::sync::Mutex::new(value))
    }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
        self.0.lock().expect("unpoisoned mock lock")
    }
}

// Independent addresses and command masks for the ADL-S Pipe A DSB.
const HEAD: usize = 0x70b00;
const TAIL: usize = 0x70b04;
const CTRL: usize = 0x70b08;
const INTERRUPT: usize = 0x70b28;
const CURRENT_HEAD: usize = 0x70b2c;
const PIPE_A_CONF: usize = 0x70008;
const ENABLE: u32 = 1 << 31;
const BUSY: u32 = 1;
const GGTT_FAULT: u32 = 1 << 3;
const TABLE_INDEX: u32 = 0x490a8;
const TABLE_DATA: u32 = 0x490ac;
const ENTRY_COUNT: usize = 4913;

struct Hardware {
    registers: BTreeMap<usize, u32>,
    ptes: BTreeMap<u64, u64>,
    ggtt_invalidated: bool,
    delay_reset: bool,
    pci_id: u32,
    revision: u32,
    writes: Vec<(usize, u32)>,
    flushed: Vec<u32>,
    flushes: usize,
    allocated: usize,
}

impl Hardware {
    fn new() -> Self {
        Self {
            registers: BTreeMap::from([(PIPE_A_CONF, 0xc000_0024)]),
            ptes: BTreeMap::new(),
            ggtt_invalidated: false,
            delay_reset: false,
            pci_id: 0x4680_8086,
            revision: 0x0c,
            writes: Vec::new(),
            flushed: Vec::new(),
            flushes: 0,
            allocated: 0,
        }
    }

    fn finish(&mut self, current_head: u32, errors: u32) {
        self.registers.insert(CTRL, ENABLE);
        self.registers.insert(CURRENT_HEAD, current_head);
        self.registers.insert(INTERRUPT, errors);
    }
}

thread_local! {
    static HARDWARE: RefCell<Hardware> = RefCell::new(Hardware::new());
}

mod pci {
    pub fn config_read_u32(_: u8, _: u8, _: u8, offset: u16) -> u32 {
        crate::HARDWARE.with(|hardware| {
            let hardware = hardware.borrow();
            match offset {
                0 => hardware.pci_id,
                8 => hardware.revision,
                _ => panic!("unexpected PCI config read {offset:x}"),
            }
        })
    }
}

mod intel {
    use crate::{BUSY, CTRL, HARDWARE, HEAD, INTERRUPT, TAIL};

    pub(crate) use self::display::lut_dsb::{
        LutDsbPoll, check_lut_dsb_idle, lut_dsb_port_blocked, start_lut_dsb,
    };

    pub(crate) const DISPLAY_INTERACTION_GGTT_LIMIT: u64 = 0x4800_0000;
    pub(crate) const AVC_CONTROL_GGTT_BASE: u64 = 0x4f00_0000;
    pub(crate) const WARM_ALIGN: usize = 4096;
    const GGTT_PAGE_BYTES: u64 = 4096;
    const GGTT_ALIAS_BASE_OFF: usize = 0x0080_0000;
    const GEN8_PAGE_PRESENT: u64 = 1;
    pub(crate) static DISPLAY_COLOR_ROUTE_LOCK: crate::Mutex<()> = crate::Mutex::new(());

    #[derive(Clone, Copy)]
    pub(crate) struct Dev {
        pub(crate) bus: u8,
        pub(crate) slot: u8,
        pub(crate) function: u8,
        pub(crate) device_id: u16,
        pub(crate) revision_id: u8,
        pub(crate) mmio: *mut u8,
        pub(crate) mmio_len: usize,
    }

    pub(crate) fn mmio_read(_: Dev, offset: usize) -> u32 {
        HARDWARE.with(|h| h.borrow().registers.get(&offset).copied().unwrap_or(0))
    }

    pub(crate) fn mmio_write(_: Dev, offset: usize, value: u32) {
        HARDWARE.with(|h| {
            let mut hardware = h.borrow_mut();
            hardware.writes.push((offset, value));
            if offset == TAIL {
                let head = hardware.registers[&HEAD];
                assert!(hardware.ggtt_invalidated);
                assert_eq!(head & 63, 0);
                assert_eq!(value & 63, 0);
                assert!(value > head);
                assert_eq!(hardware.flushed.len() * 4, (value - head) as usize);
                let control = hardware.registers[&CTRL];
                assert_eq!(control, crate::ENABLE);
                for page in (head..value).step_by(WARM_ALIGN) {
                    assert_eq!(hardware.ptes[&u64::from(page)] & 1, 1);
                }
                hardware.registers.insert(CTRL, control | BUSY);
            }
            if offset == CTRL && value == 0 && hardware.delay_reset {
                hardware.registers.insert(CTRL, BUSY);
            } else if offset == INTERRUPT {
                let current = hardware.registers.get(&offset).copied().unwrap_or(0);
                let retained = current & 0xffff & !(value & 0xffff);
                hardware
                    .registers
                    .insert(offset, retained | (value & 0xffff_0000));
            } else {
                hardware.registers.insert(offset, value);
            }
        });
    }

    pub(crate) fn alloc_ggtt_backing(bytes: usize, align: usize) -> Option<(u64, *mut u8)> {
        HARDWARE.with(|h| h.borrow_mut().allocated += 1);
        let layout = std::alloc::Layout::from_size_align(bytes, align).ok()?;
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        (!ptr.is_null()).then_some((0x1_0000_0000, ptr))
    }

    pub(crate) fn map_ggtt(_: Dev, phys: u64, bytes: usize, gpu: u64) -> bool {
        HARDWARE.with(|h| {
            let mut hardware = h.borrow_mut();
            hardware.ggtt_invalidated = false;
            for offset in (0..bytes).step_by(WARM_ALIGN) {
                hardware
                    .ptes
                    .insert(gpu + offset as u64, (phys + offset as u64) | 1);
            }
        });
        true
    }

    pub(crate) fn read_ggtt_pte(_: Dev, gpu: u64) -> Option<u64> {
        Some(HARDWARE.with(|h| h.borrow().ptes.get(&gpu).copied().unwrap_or(0)))
    }

    pub(crate) fn ggtt_invalidate(_: Dev) {
        HARDWARE.with(|h| h.borrow_mut().ggtt_invalidated = true);
    }

    pub(crate) fn dma_flush(ptr: *mut u8, bytes: usize) {
        HARDWARE.with(|h| {
            let mut hardware = h.borrow_mut();
            let words = unsafe { std::slice::from_raw_parts(ptr.cast::<u32>(), bytes / 4) };
            hardware.flushed = words.to_vec();
            hardware.flushes += 1;
        });
    }

    mod display {
        const PIPECONF_A: usize = crate::PIPE_A_CONF;
        const PIPECONF_STATE: u32 = 1 << 30;

        #[path = "@TRANSPORT_PATH@"]
        pub(super) mod lut_dsb;
    }
}

use intel::{LutDsbPoll, check_lut_dsb_idle, lut_dsb_port_blocked, start_lut_dsb};

const DEV: intel::Dev = intel::Dev {
    bus: 0,
    slot: 2,
    function: 0,
    device_id: 0x4680,
    revision_id: 0x0c,
    mmio: 0x1000 as *mut u8,
    mmio_len: 0x0100_0000,
};

fn table() -> Vec<u32> {
    (0..ENTRY_COUNT as u32)
        .map(|index| index.wrapping_mul(0x9e37_79b9) & 0x3fff_ffff)
        .collect()
}

#[test]
fn full_indexed_upload_and_completion_proof() {
    let entries = table();
    let saved_index = 0x2023;
    let _route = intel::DISPLAY_COLOR_ROUTE_LOCK.lock();
    assert!(check_lut_dsb_idle(DEV).is_ok());
    let upload = start_lut_dsb(DEV, &entries, saved_index).expect("valid DSB upload");
    assert!(lut_dsb_port_blocked());
    assert!(matches!(upload.poll(DEV), LutDsbPoll::Pending));

    HARDWARE.with(|h| {
        let mut hardware = h.borrow_mut();
        let mut expected = vec![
            1 << 13,
            0x01f0_0000 | TABLE_INDEX,
            4913,
            0x0900_0000 | TABLE_DATA,
        ];
        expected.extend(entries);
        expected.extend([0, saved_index, 0x01f0_0000 | TABLE_INDEX]);
        expected.resize(expected.len().next_multiple_of(16), 0);
        assert_eq!(hardware.flushed, expected);
        assert_eq!(hardware.allocated, 1);
        let tail = hardware.registers[&TAIL];
        hardware.finish(tail, 0);
    });

    assert!(matches!(upload.poll(DEV), LutDsbPoll::Complete));
    assert!(!lut_dsb_port_blocked());
    drop(upload);
    assert!(check_lut_dsb_idle(DEV).is_ok());

    let last_tail = HARDWARE.with(|h| h.borrow().registers[&TAIL]);
    let mut second = table();
    second[0] = 1;
    let upload = start_lut_dsb(DEV, &second, saved_index).expect("reuse idle DSB buffer");
    HARDWARE.with(|h| {
        let mut hardware = h.borrow_mut();
        let next_tail = hardware.registers[&TAIL];
        assert_ne!(
            next_tail, last_tail,
            "the previous head cannot prove completion"
        );
        assert_eq!(hardware.flushes, 2);
        assert_eq!(hardware.allocated, 1);
        assert_eq!(hardware.flushed[4], 1);
        hardware.finish(last_tail, 0);
    });
    assert!(matches!(upload.poll(DEV), LutDsbPoll::Pending));
    HARDWARE.with(|h| {
        let mut hardware = h.borrow_mut();
        let next_tail = hardware.registers[&TAIL];
        hardware.finish(next_tail, 0);
    });
    assert!(matches!(upload.poll(DEV), LutDsbPoll::Complete));
}

#[test]
fn busy_to_idle_proves_completion_without_exclusive_head() {
    let _route = intel::DISPLAY_COLOR_ROUTE_LOCK.lock();
    let upload = start_lut_dsb(DEV, &table(), 0).expect("initial DSB upload");
    assert!(matches!(upload.poll(DEV), LutDsbPoll::Pending));
    HARDWARE.with(|h| {
        let mut hardware = h.borrow_mut();
        let tail = hardware.registers[&TAIL];
        hardware.finish(tail - 64, 0);
    });
    assert!(matches!(upload.poll(DEV), LutDsbPoll::Complete));
    assert!(!lut_dsb_port_blocked());
}

#[test]
fn command_buffer_remains_reserved_while_the_engine_resets() {
    let entries = table();
    let _route = intel::DISPLAY_COLOR_ROUTE_LOCK.lock();
    let upload = start_lut_dsb(DEV, &entries, 0).expect("initial DSB upload");
    HARDWARE.with(|h| {
        let mut hardware = h.borrow_mut();
        hardware.delay_reset = true;
        let tail = hardware.registers[&TAIL];
        hardware.finish(tail, 0);
    });
    assert!(matches!(upload.poll(DEV), LutDsbPoll::Pending));
    assert!(matches!(upload.poll(DEV), LutDsbPoll::Pending));
    assert!(lut_dsb_port_blocked());
    assert!(start_lut_dsb(DEV, &entries, 0).is_err());
    HARDWARE.with(|h| {
        let mut hardware = h.borrow_mut();
        assert_eq!(hardware.flushes, 1);
        hardware.registers.insert(CTRL, 0);
    });
    assert!(matches!(upload.poll(DEV), LutDsbPoll::Complete));
    assert!(!lut_dsb_port_blocked());
}

#[test]
fn abandoning_live_upload_quarantines_the_command_buffer() {
    let entries = table();
    let _route = intel::DISPLAY_COLOR_ROUTE_LOCK.lock();
    let upload = start_lut_dsb(DEV, &entries, 0).expect("initial DSB upload");
    assert!(matches!(upload.poll(DEV), LutDsbPoll::Pending));
    drop(upload);
    assert!(lut_dsb_port_blocked());
    let before = HARDWARE.with(|h| {
        let hardware = h.borrow();
        (
            hardware.writes.clone(),
            hardware.flushed.clone(),
            hardware.flushes,
            hardware.ptes.clone(),
        )
    });
    assert!(check_lut_dsb_idle(DEV).is_err());
    assert!(start_lut_dsb(DEV, &entries, 3).is_err());
    HARDWARE.with(|h| {
        let hardware = h.borrow();
        assert_eq!(hardware.writes, before.0);
        assert_eq!(hardware.flushed, before.1);
        assert_eq!(hardware.flushes, before.2);
        assert_eq!(hardware.ptes, before.3);
        assert_eq!(hardware.allocated, 1);
    });
}

#[test]
fn fault_before_exclusive_tail_quarantines_the_port() {
    let entries = table();
    let _route = intel::DISPLAY_COLOR_ROUTE_LOCK.lock();
    let upload = start_lut_dsb(DEV, &entries, 0).expect("initial DSB upload");
    assert!(matches!(upload.poll(DEV), LutDsbPoll::Pending));
    HARDWARE.with(|h| {
        let mut hardware = h.borrow_mut();
        let tail = hardware.registers[&TAIL];
        hardware.finish(tail - 64, GGTT_FAULT);
    });
    assert!(matches!(
        upload.poll(DEV),
        LutDsbPoll::Uncertain("chroma-dsb-ggtt-fault")
    ));
    assert!(lut_dsb_port_blocked());
}

#[test]
fn fault_with_proven_drain_releases_pipe_without_reporting_success() {
    let entries = table();
    let _route = intel::DISPLAY_COLOR_ROUTE_LOCK.lock();
    let upload = start_lut_dsb(DEV, &entries, 0).expect("initial DSB upload");
    HARDWARE.with(|h| {
        let mut hardware = h.borrow_mut();
        let tail = hardware.registers[&TAIL];
        hardware.finish(tail, GGTT_FAULT);
    });
    assert!(matches!(
        upload.poll(DEV),
        LutDsbPoll::Failed("chroma-dsb-ggtt-fault")
    ));
    assert!(!lut_dsb_port_blocked());
    drop(upload);
    assert!(check_lut_dsb_idle(DEV).is_ok());
    HARDWARE.with(|h| {
        let hardware = h.borrow();
        assert_eq!(hardware.registers[&CTRL], 0);
        assert_eq!(hardware.registers[&INTERRUPT] & GGTT_FAULT, 0);
    });
}

#[test]
fn unsupported_device_and_invalid_table_never_submit() {
    let _route = intel::DISPLAY_COLOR_ROUTE_LOCK.lock();
    HARDWARE.with(|h| h.borrow_mut().revision = 0x0d);
    assert!(check_lut_dsb_idle(DEV).is_err());
    assert!(start_lut_dsb(DEV, &table(), 0).is_err());
    HARDWARE.with(|h| h.borrow_mut().revision = 0x0c);
    assert!(start_lut_dsb(DEV, &[], 0).is_err());
    let mut bad = table();
    bad[ENTRY_COUNT - 1] |= 1 << 30;
    assert!(start_lut_dsb(DEV, &bad, 0).is_err());
    HARDWARE.with(|h| {
        let hardware = h.borrow();
        assert_eq!(hardware.allocated, 0);
        assert!(hardware.writes.is_empty());
    });
}
