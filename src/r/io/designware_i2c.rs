//! Polling Synopsys DesignWare I2C master, without IRQs or DMA.
//!
//! The caller owns power, PCI admission and LPSS reset release. Register I/O
//! and cooperative delays are injected so the FIFO and timeout logic can be
//! tested without hardware. Every transfer is one optional write followed by
//! one optional read, with a repeated START between them and one final STOP.

const CON: u16 = 0x00;
const TAR: u16 = 0x04;
const DATA_CMD: u16 = 0x10;
const SS_SCL_HCNT: u16 = 0x14;
const SS_SCL_LCNT: u16 = 0x18;
const FS_SCL_HCNT: u16 = 0x1c;
const FS_SCL_LCNT: u16 = 0x20;
const INTR_MASK: u16 = 0x30;
const RAW_INTR_STAT: u16 = 0x34;
const RX_TL: u16 = 0x38;
const TX_TL: u16 = 0x3c;
const CLR_INTR: u16 = 0x40;
const CLR_TX_ABRT: u16 = 0x54;
const CLR_STOP_DET: u16 = 0x60;
const ENABLE: u16 = 0x6c;
const STATUS: u16 = 0x70;
const TXFLR: u16 = 0x74;
const RXFLR: u16 = 0x78;
const SDA_HOLD: u16 = 0x7c;
const TX_ABRT_SOURCE: u16 = 0x80;
const DMA_CR: u16 = 0x88;
const ENABLE_STATUS: u16 = 0x9c;
const COMP_PARAM_1: u16 = 0xf4;
const COMP_VERSION: u16 = 0xf8;
const COMP_TYPE: u16 = 0xfc;

const TYPE_VALUE: u32 = 0x4457_0140;
const MASTER: u32 = 1;
const SPEED_STD: u32 = 1 << 1;
const SPEED_FAST: u32 = 2 << 1;
const RESTART_ENABLE: u32 = 1 << 5;
const SLAVE_DISABLE: u32 = 1 << 6;
const BUS_CLEAR: u32 = 1 << 11;
const CMD_READ: u32 = 1 << 8;
const CMD_STOP: u32 = 1 << 9;
const CMD_RESTART: u32 = 1 << 10;
const INTR_RX_UNDER: u32 = 1;
const INTR_RX_OVER: u32 = 1 << 1;
const INTR_TX_OVER: u32 = 1 << 3;
const INTR_TX_ABRT: u32 = 1 << 6;
const INTR_STOP_DET: u32 = 1 << 9;
const INTR_MST_ON_HOLD: u32 = 1 << 13;
const STATUS_ACTIVITY: u32 = 1;
const STATUS_MASTER_ACTIVITY: u32 = 1 << 5;
const STATUS_MASTER_HOLD: u32 = 1 << 7;
const ENABLE_ABORT: u32 = 1 << 1;
const CONTROL_WAIT_MS: u64 = 50;
const CONTROL_POLLS: usize = 256;
const TRANSFER_POLLS: usize = 4096;
const MAX_TRANSFER_BYTES: usize = 4096;

pub(crate) trait Bus {
    fn read(&mut self, offset: u16) -> u32;
    fn write(&mut self, offset: u16, value: u32);
    fn now_ms(&self) -> u64;
    async fn pause(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Error {
    pub stage: &'static str,
    pub reason: &'static str,
    /// Capture before clearing TX_ABRT; it distinguishes NACK from bus faults.
    pub abort_source: u32,
}

fn error(stage: &'static str, reason: &'static str) -> Error {
    Error {
        stage,
        reason,
        abort_source: 0,
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Timing {
    pub clock_hz: u32,
    pub bus_hz: u32,
    pub sda_hold_ns: u32,
}

impl Timing {
    /// Tiger Lake's a0e8/a0e9 LPSS controllers use the 120 MHz SPT profile.
    pub(crate) const fn intel_lpss(bus_hz: u32) -> Self {
        Self {
            clock_hz: 120_000_000,
            bus_hz,
            sda_hold_ns: 230,
        }
    }

    fn counts(self) -> Result<(u32, u32, u32), Error> {
        if !(10_000_000..=500_000_000).contains(&self.clock_hz)
            || !matches!(self.bus_hz, 100_000 | 400_000)
        {
            return Err(error("timing", "unsupported controller clock or bus speed"));
        }
        // Include fall time in both periods. Allocate a full requested period
        // in addition to that margin, so the fallback never exceeds bus_hz.
        let period_ns = 1_000_000_000u64 / u64::from(self.bus_hz);
        let minimum_low = if self.bus_hz == 100_000 { 4700 } else { 1300 };
        let low_ns = (period_ns / 2).max(minimum_low);
        let high_ns = period_ns - low_ns;
        let cycles = |ns: u64| (u64::from(self.clock_hz) * ns + 999_999_999) / 1_000_000_000;
        let high = cycles(high_ns + 300).saturating_sub(3).max(8);
        let low = cycles(low_ns + 300).saturating_sub(1).max(8);
        let hold = cycles(u64::from(self.sda_hold_ns)).max(1);
        if high > 0xffff || low > 0xffff || hold > 0xffff {
            return Err(error("timing", "timing count exceeds hardware field"));
        }
        Ok((high as u32, low as u32, hold as u32))
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Master {
    pub tx_depth: u16,
    pub rx_depth: u16,
    bus_hz: u32,
}

async fn wait_bit(
    bus: &mut impl Bus,
    register: u16,
    mask: u32,
    set: bool,
    stage: &'static str,
) -> Result<(), Error> {
    let deadline = bus.now_ms().saturating_add(CONTROL_WAIT_MS);
    for _ in 0..CONTROL_POLLS {
        let value = bus.read(register);
        if value == u32::MAX {
            return Err(error(stage, "controller unavailable"));
        }
        if (value & mask != 0) == set {
            return Ok(());
        }
        if bus.now_ms() >= deadline {
            break;
        }
        bus.pause().await;
    }
    Err(error(stage, "controller wait timed out"))
}

async fn disable(bus: &mut impl Bus) -> Result<(), Error> {
    let raw = bus.read(RAW_INTR_STAT);
    let status = bus.read(STATUS);
    let enabled = bus.read(ENABLE);
    if raw == u32::MAX || status == u32::MAX || enabled == u32::MAX {
        return Err(error("disable", "controller unavailable"));
    }
    // A master holding SCL while waiting for FIFO data will not disable until
    // its transaction is aborted. This affects only this admitted controller.
    if raw & INTR_MST_ON_HOLD != 0 || status & (STATUS_MASTER_HOLD | STATUS_MASTER_ACTIVITY) != 0 {
        if enabled & 1 == 0 {
            bus.write(ENABLE, 1);
            wait_bit(bus, ENABLE_STATUS, 1, true, "abort-enable").await?;
        }
        bus.write(ENABLE, 1 | ENABLE_ABORT);
        wait_bit(bus, ENABLE, ENABLE_ABORT, false, "abort").await?;
    }
    bus.write(ENABLE, 0);
    wait_bit(bus, ENABLE_STATUS, 1, false, "disable").await?;
    bus.write(INTR_MASK, 0);
    Ok(())
}

/// Initialize after the platform has powered/mapped/released this controller.
/// The controller remains disabled until a transaction is requested.
pub(crate) async fn initialize(bus: &mut impl Bus, timing: Timing) -> Result<Master, Error> {
    let (high, low, hold) = timing.counts()?;
    if bus.read(COMP_TYPE) != TYPE_VALUE {
        return Err(error("identify", "not a DesignWare I2C controller"));
    }
    let params = bus.read(COMP_PARAM_1);
    if params == u32::MAX {
        return Err(error("identify", "controller unavailable"));
    }
    let tx_depth = (((params >> 16) & 0xff) + 1) as u16;
    let rx_depth = (((params >> 8) & 0xff) + 1) as u16;
    if tx_depth < 2 || rx_depth < 2 {
        return Err(error("identify", "unsupported FIFO depth"));
    }
    disable(bus).await?;
    let speed = if timing.bus_hz == 100_000 {
        SPEED_STD
    } else {
        SPEED_FAST
    };
    let con = MASTER | speed | RESTART_ENABLE | SLAVE_DISABLE | (bus.read(CON) & BUS_CLEAR);
    bus.write(CON, con);
    let (hcnt, lcnt) = if timing.bus_hz == 100_000 {
        (SS_SCL_HCNT, SS_SCL_LCNT)
    } else {
        (FS_SCL_HCNT, FS_SCL_LCNT)
    };
    bus.write(hcnt, high);
    bus.write(lcnt, low);
    // RX hold of at least one cycle avoids arbitration loss when a target
    // pulls SDA down soon after SCL. Preserve a larger firmware RX hold.
    if bus.read(COMP_VERSION) >= 0x3131_312a {
        let old = bus.read(SDA_HOLD);
        let rx_hold = ((old >> 16) & 0xff).max(1);
        bus.write(SDA_HOLD, (old & 0xff00_0000) | (rx_hold << 16) | hold);
    }
    bus.write(RX_TL, 0);
    bus.write(TX_TL, 0);
    bus.write(DMA_CR, 0);
    let _ = bus.read(CLR_INTR);
    if bus.read(CON) & (MASTER | 0x6 | RESTART_ENABLE | SLAVE_DISABLE) != con & 0x67 {
        return Err(error("configure", "master configuration rejected"));
    }
    Ok(Master {
        tx_depth,
        rx_depth,
        bus_hz: timing.bus_hz,
    })
}

impl Master {
    /// Fixed-length I2C transaction. Buffers contain raw bus bytes; HID framing
    /// belongs to the caller. No allocation or I2C address scan is performed.
    pub(crate) async fn transfer(
        &self,
        bus: &mut impl Bus,
        address: u16,
        write: &[u8],
        read: &mut [u8],
    ) -> Result<(), Error> {
        if address == 0 || address >= 0x80 {
            return Err(error("transaction", "invalid 7-bit target address"));
        }
        let length = write.len().checked_add(read.len()).unwrap_or(usize::MAX);
        if length == 0 || length > MAX_TRANSFER_BYTES {
            return Err(error("transaction", "invalid transfer length"));
        }
        let result = self.transfer_inner(bus, address, write, read).await;
        if result.is_err() {
            // Preserve the original abort source before this cleanup clears it.
            let _ = disable(bus).await;
            let _ = bus.read(CLR_INTR);
        }
        result
    }

    async fn transfer_inner(
        &self,
        bus: &mut impl Bus,
        address: u16,
        write: &[u8],
        read: &mut [u8],
    ) -> Result<(), Error> {
        disable(bus).await?;
        wait_bit(bus, STATUS, STATUS_ACTIVITY, false, "bus-idle").await?;
        bus.write(TAR, u32::from(address));
        let _ = bus.read(CLR_INTR);
        // Defensive drain: disabled hardware should have no retained input.
        for _ in 0..self.rx_depth {
            if bus.read(RXFLR) == 0 {
                break;
            }
            let _ = bus.read(DATA_CMD);
        }
        if bus.read(RXFLR) != 0 {
            return Err(error("drain", "receive FIFO did not drain"));
        }
        bus.write(ENABLE, 1);
        wait_bit(bus, ENABLE_STATUS, 1, true, "enable").await?;

        let wire_ms = ((write.len() + read.len()) as u64 * 10_000).div_ceil(u64::from(self.bus_hz));
        // A cooperative 1 ms pause can be longer than a FIFO's wire time.
        // Budget those batches too, including small (two-entry) legal FIFOs.
        let batch_depth = usize::from(self.tx_depth.min(self.rx_depth));
        let batches = (write.len() + read.len()).div_ceil(batch_depth) as u64;
        let deadline = bus.now_ms().saturating_add(50 + wire_ms * 2 + batches * 2);
        let mut written = 0usize;
        let mut requested = 0usize;
        let mut received = 0usize;
        let mut stopped = false;
        for _ in 0..TRANSFER_POLLS {
            check_interrupts(bus)?;
            let available = bus.read(RXFLR) as usize;
            if available > usize::from(self.rx_depth) || available > requested - received {
                return Err(error("receive", "unexpected receive FIFO length"));
            }
            for _ in 0..available {
                read[received] = bus.read(DATA_CMD) as u8;
                received += 1;
            }

            let level = bus.read(TXFLR) as usize;
            if level > usize::from(self.tx_depth) {
                return Err(error("transmit", "invalid transmit FIFO length"));
            }
            let mut room = usize::from(self.tx_depth) - level;
            while room != 0 && written < write.len() {
                let mut command = u32::from(write[written]);
                if written + 1 == write.len() && read.is_empty() {
                    command |= CMD_STOP;
                }
                bus.write(DATA_CMD, command);
                written += 1;
                room -= 1;
            }
            while room != 0
                && written == write.len()
                && requested < read.len()
                && requested - received < usize::from(self.rx_depth)
            {
                let mut command = CMD_READ;
                if requested == 0 && !write.is_empty() {
                    command |= CMD_RESTART;
                }
                if requested + 1 == read.len() {
                    command |= CMD_STOP;
                }
                bus.write(DATA_CMD, command);
                requested += 1;
                room -= 1;
            }

            let raw = check_interrupts(bus)?;
            stopped |= raw & INTR_STOP_DET != 0;
            if written == write.len()
                && received == read.len()
                && stopped
                && bus.read(TXFLR) == 0
                && bus.read(STATUS) & (STATUS_ACTIVITY | STATUS_MASTER_ACTIVITY) == 0
            {
                let _ = bus.read(CLR_STOP_DET);
                return disable(bus).await;
            }
            if bus.now_ms() >= deadline {
                break;
            }
            bus.pause().await;
        }
        Err(error("transaction", "transfer timed out"))
    }

    pub(crate) async fn shutdown(&self, bus: &mut impl Bus) -> Result<(), Error> {
        disable(bus).await
    }
}

fn check_interrupts(bus: &mut impl Bus) -> Result<u32, Error> {
    let raw = bus.read(RAW_INTR_STAT);
    if raw == u32::MAX {
        return Err(error("transaction", "controller unavailable"));
    }
    if raw & INTR_TX_ABRT != 0 {
        let abort_source = bus.read(TX_ABRT_SOURCE);
        let _ = bus.read(CLR_TX_ABRT);
        return Err(Error {
            stage: "transaction",
            reason: "transmit aborted",
            abort_source,
        });
    }
    if raw & (INTR_RX_OVER | INTR_TX_OVER | INTR_RX_UNDER) != 0 {
        return Err(error("transaction", "FIFO overrun or underrun"));
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        future::Future,
        task::{Context, Poll, Waker},
    };

    fn run<F: Future>(future: F) -> F::Output {
        let mut future = core::pin::pin!(future);
        let mut context = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
                return value;
            }
        }
    }

    struct FakeBus {
        regs: [u32; 64],
        time: u64,
        pauses: usize,
        writes: Vec<(u16, u32)>,
        commands: Vec<u32>,
        tx: VecDeque<u32>,
        rx: VecDeque<u8>,
        response: VecDeque<u8>,
        tx_depth: usize,
        rx_depth: usize,
        max_tx: usize,
        max_rx: usize,
        raw: u32,
        abort_source: u32,
        abort_after: Option<usize>,
        completed: usize,
        steps_per_pause: usize,
        stalled: bool,
        missing_stop: bool,
        refuse_disable: bool,
        abort_stuck: bool,
        frozen_clock: bool,
        held_master: bool,
    }

    impl FakeBus {
        fn new(tx_depth: usize, rx_depth: usize) -> Self {
            let mut regs = [0; 64];
            regs[usize::from(COMP_TYPE / 4)] = TYPE_VALUE;
            regs[usize::from(COMP_VERSION / 4)] = 0x3131_312a;
            regs[usize::from(COMP_PARAM_1 / 4)] =
                (((tx_depth - 1) as u32) << 16) | (((rx_depth - 1) as u32) << 8);
            Self {
                regs,
                time: 0,
                pauses: 0,
                writes: Vec::new(),
                commands: Vec::new(),
                tx: VecDeque::new(),
                rx: VecDeque::new(),
                response: VecDeque::new(),
                tx_depth,
                rx_depth,
                max_tx: 0,
                max_rx: 0,
                raw: 0,
                abort_source: 0,
                abort_after: None,
                completed: 0,
                steps_per_pause: 64,
                stalled: false,
                missing_stop: false,
                refuse_disable: false,
                abort_stuck: false,
                frozen_clock: false,
                held_master: false,
            }
        }

        fn step(&mut self) {
            if self.stalled {
                return;
            }
            for _ in 0..self.steps_per_pause {
                let Some(command) = self.tx.pop_front() else {
                    break;
                };
                self.completed += 1;
                if self.abort_after == Some(self.completed) {
                    self.raw |= INTR_TX_ABRT;
                    self.abort_source = 1; // seven-bit address NACK
                    self.tx.clear();
                    self.rx.clear();
                    break;
                }
                if command & CMD_READ != 0 {
                    if self.rx.len() == self.rx_depth {
                        self.raw |= INTR_RX_OVER;
                    }
                    self.rx.push_back(self.response.pop_front().unwrap_or(0xff));
                    self.max_rx = self.max_rx.max(self.rx.len());
                }
                if command & CMD_STOP != 0 && !self.missing_stop {
                    self.raw |= INTR_STOP_DET;
                }
            }
        }
    }

    impl Bus for FakeBus {
        fn read(&mut self, offset: u16) -> u32 {
            match offset {
                RAW_INTR_STAT => self.raw,
                TX_ABRT_SOURCE => self.abort_source,
                TXFLR => self.tx.len() as u32,
                RXFLR => self.rx.len() as u32,
                STATUS => {
                    if self.held_master {
                        STATUS_ACTIVITY | STATUS_MASTER_ACTIVITY | STATUS_MASTER_HOLD
                    } else if !self.tx.is_empty() {
                        STATUS_ACTIVITY | STATUS_MASTER_ACTIVITY
                    } else {
                        0
                    }
                }
                DATA_CMD => match self.rx.pop_front() {
                    Some(byte) => u32::from(byte),
                    None => {
                        self.raw |= INTR_RX_UNDER;
                        0
                    }
                },
                ENABLE_STATUS => self.regs[usize::from(ENABLE / 4)] & 1,
                CLR_INTR => {
                    self.raw = 0;
                    self.abort_source = 0;
                    0
                }
                CLR_TX_ABRT => {
                    self.raw &= !INTR_TX_ABRT;
                    self.abort_source = 0;
                    0
                }
                CLR_STOP_DET => {
                    self.raw &= !INTR_STOP_DET;
                    0
                }
                _ => self.regs[usize::from(offset / 4)],
            }
        }
        fn write(&mut self, offset: u16, value: u32) {
            self.writes.push((offset, value));
            match offset {
                ENABLE if value & ENABLE_ABORT != 0 => {
                    self.tx.clear();
                    self.rx.clear();
                    self.held_master = false;
                    self.regs[usize::from(ENABLE / 4)] = if self.abort_stuck {
                        value
                    } else {
                        value & !ENABLE_ABORT
                    };
                }
                ENABLE if value == 0 => {
                    if !self.refuse_disable {
                        self.regs[usize::from(ENABLE / 4)] = 0;
                        self.tx.clear();
                    }
                }
                DATA_CMD => {
                    if self.tx.len() == self.tx_depth {
                        self.raw |= INTR_TX_OVER;
                    }
                    self.commands.push(value);
                    self.tx.push_back(value);
                    self.max_tx = self.max_tx.max(self.tx.len());
                }
                _ => self.regs[usize::from(offset / 4)] = value,
            }
        }
        fn now_ms(&self) -> u64 {
            self.time
        }
        async fn pause(&mut self) {
            self.pauses += 1;
            if !self.frozen_clock {
                self.time += 1;
            }
            self.step();
        }
    }

    fn master(bus: &mut FakeBus) -> Master {
        run(initialize(bus, Timing::intel_lpss(400_000))).unwrap()
    }

    #[test]
    fn initialization_checks_identity_before_writes() {
        let mut bus = FakeBus::new(16, 16);
        bus.regs[usize::from(COMP_TYPE / 4)] = u32::MAX;
        assert_eq!(
            run(initialize(&mut bus, Timing::intel_lpss(400_000))).unwrap_err(),
            error("identify", "not a DesignWare I2C controller")
        );
        assert!(bus.writes.is_empty());
    }

    #[test]
    fn initialization_discovers_fifos_and_disables_irq_dma() {
        let mut bus = FakeBus::new(64, 32);
        bus.regs[usize::from(CON / 4)] = BUS_CLEAR;
        bus.regs[usize::from(SDA_HOLD / 4)] = 5 << 16;
        let master = master(&mut bus);
        assert_eq!((master.tx_depth, master.rx_depth), (64, 32));
        assert_eq!(
            bus.regs[usize::from(CON / 4)],
            BUS_CLEAR | MASTER | SPEED_FAST | RESTART_ENABLE | SLAVE_DISABLE
        );
        assert_eq!(bus.regs[usize::from(INTR_MASK / 4)], 0);
        assert_eq!(bus.regs[usize::from(DMA_CR / 4)], 0);
        assert_eq!(bus.regs[usize::from(ENABLE / 4)], 0);
        assert_eq!(bus.regs[usize::from(SDA_HOLD / 4)], (5 << 16) | 28);
        assert!(bus.writes.iter().all(|(offset, _)| *offset < 0x200));
    }

    #[test]
    fn combined_read_has_one_restart_and_one_stop() {
        let mut bus = FakeBus::new(4, 4);
        let master = master(&mut bus);
        bus.response.extend([30, 0, 0x00, 0x01]);
        let mut read = [0; 4];
        run(master.transfer(&mut bus, 0x2c, &[0x20, 0], &mut read)).unwrap();
        assert_eq!(read, [30, 0, 0, 1]);
        assert_eq!(
            bus.commands,
            [
                0x20,
                0,
                CMD_READ | CMD_RESTART,
                CMD_READ,
                CMD_READ,
                CMD_READ | CMD_STOP
            ]
        );
        assert_eq!(bus.regs[usize::from(TAR / 4)], 0x2c);
        assert_eq!(bus.regs[usize::from(ENABLE / 4)], 0);
    }

    #[test]
    fn pure_read_and_write_use_only_final_stop() {
        let mut bus = FakeBus::new(2, 2);
        let master = master(&mut bus);
        run(master.transfer(&mut bus, 0x2c, &[1, 2, 3], &mut [])).unwrap();
        assert_eq!(bus.commands, [1, 2, 3 | CMD_STOP]);
        bus.commands.clear();
        bus.response.extend([4, 5, 6]);
        let mut read = [0; 3];
        run(master.transfer(&mut bus, 0x2c, &[], &mut read)).unwrap();
        assert_eq!(read, [4, 5, 6]);
        assert_eq!(bus.commands, [CMD_READ, CMD_READ, CMD_READ | CMD_STOP]);
    }

    #[test]
    fn long_reads_never_overcommit_small_receive_fifo() {
        let mut bus = FakeBus::new(16, 2);
        let master = master(&mut bus);
        let expected: Vec<u8> = (0..=255).cycle().take(665).collect();
        bus.response.extend(expected.iter().copied());
        let mut read = vec![0; expected.len()];
        run(master.transfer(&mut bus, 0x2c, &[0x20, 0], &mut read)).unwrap();
        assert_eq!(read, expected);
        assert!(bus.max_rx <= 2);
        assert!(bus.max_tx <= 16);
        assert_eq!(
            bus.commands
                .iter()
                .filter(|&&command| command & CMD_STOP != 0)
                .count(),
            1
        );
    }

    #[test]
    fn slow_transmit_fifo_does_not_overflow() {
        let mut bus = FakeBus::new(2, 2);
        let master = master(&mut bus);
        bus.steps_per_pause = 1;
        let write: Vec<u8> = (0..130).collect();
        run(master.transfer(&mut bus, 0x2c, &write, &mut [])).unwrap();
        assert_eq!(bus.commands.len(), write.len());
        assert!(bus.max_tx <= 2);
    }

    #[test]
    fn abort_source_survives_cleanup_and_next_transfer_recovers() {
        let mut bus = FakeBus::new(4, 4);
        let master = master(&mut bus);
        bus.abort_after = Some(1);
        let failure = run(master.transfer(&mut bus, 0x2c, &[0x20, 0], &mut [0; 2])).unwrap_err();
        assert_eq!(failure.abort_source, 1);
        assert_eq!(failure.reason, "transmit aborted");
        assert_eq!(bus.raw, 0);
        assert_eq!(bus.regs[usize::from(ENABLE / 4)], 0);
        bus.abort_after = None;
        bus.response.extend([10, 11]);
        let mut read = [0; 2];
        run(master.transfer(&mut bus, 0x2c, &[], &mut read)).unwrap();
        assert_eq!(read, [10, 11]);
    }

    #[test]
    fn missing_stop_cannot_report_success() {
        let mut bus = FakeBus::new(4, 4);
        let master = master(&mut bus);
        bus.missing_stop = true;
        assert_eq!(
            run(master.transfer(&mut bus, 0x2c, &[1], &mut [])),
            Err(error("transaction", "transfer timed out"))
        );
        assert!(bus.time < 100);
        assert_eq!(bus.regs[usize::from(ENABLE / 4)], 0);
    }

    #[test]
    fn stalled_transfer_is_aborted_and_bounded() {
        let mut bus = FakeBus::new(4, 4);
        let master = master(&mut bus);
        bus.stalled = true;
        assert_eq!(
            run(master.transfer(&mut bus, 0x2c, &[1], &mut [])),
            Err(error("transaction", "transfer timed out"))
        );
        assert!(bus.writes.contains(&(ENABLE, 1 | ENABLE_ABORT)));
        assert!(bus.time < 100);
        assert!(bus.tx.is_empty());
    }

    #[test]
    fn unresponsive_disable_and_abort_are_bounded() {
        let mut bus = FakeBus::new(4, 4);
        bus.regs[usize::from(ENABLE / 4)] = 1;
        bus.refuse_disable = true;
        assert_eq!(
            run(initialize(&mut bus, Timing::intel_lpss(400_000))).unwrap_err(),
            error("disable", "controller wait timed out")
        );
        assert_eq!(bus.time, CONTROL_WAIT_MS);
        let mut bus = FakeBus::new(4, 4);
        bus.regs[usize::from(ENABLE / 4)] = 1;
        bus.held_master = true;
        bus.abort_stuck = true;
        assert_eq!(
            run(initialize(&mut bus, Timing::intel_lpss(400_000))).unwrap_err(),
            error("abort", "controller wait timed out")
        );
        assert_eq!(bus.time, CONTROL_WAIT_MS);
    }

    #[test]
    fn invalid_requests_do_not_touch_hardware() {
        let mut bus = FakeBus::new(4, 4);
        let master = master(&mut bus);
        bus.writes.clear();
        assert!(run(master.transfer(&mut bus, 0, &[1], &mut [])).is_err());
        assert!(run(master.transfer(&mut bus, 0x80, &[1], &mut [])).is_err());
        assert!(run(master.transfer(&mut bus, 0x2c, &[], &mut [])).is_err());
        assert!(
            run(master.transfer(&mut bus, 0x2c, &[0; MAX_TRANSFER_BYTES + 1], &mut [])).is_err()
        );
        assert!(bus.writes.is_empty());
    }

    #[test]
    fn timing_fallback_is_at_or_below_requested_bus_rate() {
        for bus_hz in [100_000, 400_000] {
            let timing = Timing::intel_lpss(bus_hz);
            let (high, low, hold) = timing.counts().unwrap();
            assert!(u64::from(high + low + 4) * u64::from(bus_hz) >= u64::from(timing.clock_hz));
            assert_eq!(hold, 28);
        }
    }
}
