//! AX201/QuZ (22000 family) context-info and polled host-command transport.
//!
//! Register/wire contracts: Linux v6.14 iwlwifi iwl-{csr,fh,context-info}.h,
//! pcie/{ctxt-info,trans-gen2,rx,tx-gen2}.c and fw/api/txq.h. This is the
//! pre-AX210 context-info layout, not the AX211/gen3 layout.
//! DMA allocations are kept for the device lifetime after the first publication:
//! a timed-out firmware load is not proof that its bus-master has stopped.

use super::iwl_ax201_fw::{Firmware, SectionPart};
use alloc::{collections::VecDeque, vec::Vec};
use core::sync::atomic::{Ordering, fence};

#[path = "iwl_ax201_data.rs"]
pub(crate) mod data;

const RX_COUNT: usize = 512;
const RX_BYTES: usize = 4096;
const CMD_COUNT: usize = 32;
const CMD_BYTES: usize = 4096;
const TFD_BYTES: usize = 256;
const DATA_COUNT: usize = 32;
const DATA_BYTES: usize = 4096;
const MAX_DATA_QUEUES: usize = 4;
const CTXT_BYTES: usize = 1792;
const CTXT_DRAM: usize = 192;
const CSR_INT: usize = 0x008;
const CSR_INT_MASK: usize = 0x00c;
const CSR_RESET: usize = 0x020;
const CSR_GP: usize = 0x024;
const CSR_CTXT: usize = 0x040;
const ALIVE_IRQ: u32 = 1;
const FW_ERROR: u32 = (1 << 25) | (1 << 29);
const GP_CLOCK_READY: u32 = 1;
const GP_INIT_DONE: u32 = 4;
const GP_ACCESS: u32 = 8;
const GP_SLEEP: u32 = 16;
const RX_WPTR: usize = 0x1c80;
const TX_WPTR: usize = 0x460;

struct Dma {
    phys: u64,
    virt: usize,
    len: usize,
    pinned: bool,
}
impl Dma {
    fn zero(len: usize, align: usize) -> Result<Self, &'static str> {
        let (phys, virt) = crate::dma::alloc(len, align).ok_or("AX201 DMA allocation failed")?;
        unsafe {
            core::ptr::write_bytes(virt, 0, len);
        }
        Ok(Self {
            phys,
            virt: virt as usize,
            len,
            pinned: false,
        })
    }
    fn bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.virt as *const u8, self.len) }
    }
    fn bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.virt as *mut u8, self.len) }
    }
    fn flush(&self) {
        crate::intel::dma_cache_flush_range(self.virt as *const u8, self.len);
    }
    fn put16(&mut self, at: usize, n: u16) {
        self.bytes_mut()[at..at + 2].copy_from_slice(&n.to_le_bytes());
    }
    fn put32(&mut self, at: usize, n: u32) {
        self.bytes_mut()[at..at + 4].copy_from_slice(&n.to_le_bytes());
    }
    fn put64(&mut self, at: usize, n: u64) {
        self.bytes_mut()[at..at + 8].copy_from_slice(&n.to_le_bytes());
    }
    fn read16(&self, at: usize) -> u16 {
        unsafe { core::ptr::read_volatile((self.virt + at) as *const u16) }.to_le()
    }
    fn read32(&self, at: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.virt + at) as *const u32) }.to_le()
    }
}
impl Drop for Dma {
    fn drop(&mut self) {
        if !self.pinned {
            crate::dma::dealloc(self.virt as *mut u8, self.len);
        }
    }
}

/// Fixed basic rate is intentional until station rate-control is configured.
/// Software CCMP packets also set encrypt=false: their Protected bit, IV and
/// MIC are already present and must not be transformed by firmware.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TxOptions {
    pub band_2ghz: bool,
    /// Antenna bitmap, one chain only (1=A, 2=B), restricted by caller's NVM.
    pub antenna: u8,
    pub encrypt: bool,
    pub high_priority: bool,
}

struct DataQueue {
    id: Option<u16>,
    station: u8,
    tid: u8,
    tfd: Dma,
    byte_count: Dma,
    packets: Dma,
    read: u8,
    write: u8,
    outstanding: usize,
    busy: [bool; DATA_COUNT],
}

impl DataQueue {
    fn new(station: u8, tid: u8) -> Result<Self, &'static str> {
        if station >= 32 || !(tid <= 7 || tid == 15 || tid == 16) {
            return Err("AX201 invalid queue station/TID");
        }
        Ok(Self {
            id: None,
            station,
            tid: if tid == 16 { 15 } else { tid },
            tfd: Dma::zero(DATA_COUNT * TFD_BYTES, 4096)?,
            // 22000's iwlagn_scd_bc_tbl includes 256 entries plus 64 duplicates.
            byte_count: Dma::zero(640, 1024)?,
            packets: Dma::zero(DATA_COUNT * DATA_BYTES, 4096)?,
            read: 0,
            write: 0,
            outstanding: 0,
            busy: [false; DATA_COUNT],
        })
    }
    fn config(&mut self, version: u8) -> Result<Vec<u8>, &'static str> {
        // Linux pcie/tx-gen2.c selects this ABI from DATA_PATH_GROUP/0x17
        // metadata, independently of the device family's long-TFD format.
        let payload = match version {
            0 => {
                let mut payload = alloc::vec![0; 24];
                payload[0] = self.station;
                payload[1] = self.tid;
                payload[2..4].copy_from_slice(&1u16.to_le_bytes()); // ENABLE_QUEUE
                payload[4..8].copy_from_slice(&2u32.to_le_bytes()); // log2(32)-3
                payload[8..16].copy_from_slice(&self.byte_count.phys.to_le_bytes());
                payload[16..24].copy_from_slice(&self.tfd.phys.to_le_bytes());
                payload
            }
            3 => {
                // iwl_scd_queue_cfg_cmd v3 ADD. ENABLE_QUEUE is invalid here:
                // the operation field requests addition, flags select long TFD.
                let mut payload = alloc::vec![0; 36];
                payload[4..8].copy_from_slice(&(1u32 << self.station).to_le_bytes());
                payload[8] = self.tid;
                payload[16..20].copy_from_slice(&2u32.to_le_bytes());
                payload[20..28].copy_from_slice(&self.byte_count.phys.to_le_bytes());
                payload[28..36].copy_from_slice(&self.tfd.phys.to_le_bytes());
                payload
            }
            _ => return Err("AX201 unsupported queue allocation command version"),
        };
        for dma in [&mut self.tfd, &mut self.byte_count, &mut self.packets] {
            dma.flush();
            dma.pinned = true;
        }
        fence(Ordering::Release);
        Ok(payload)
    }
}

#[derive(Debug)]
pub(crate) struct Notification {
    pub group: u8,
    pub id: u8,
    pub sequence: u16,
    pub payload: Vec<u8>,
}

pub(crate) struct Transport {
    base: usize,
    pci: (u8, u8, u8),
    context: Dma,
    free: Dma,
    used: Dma,
    status: Dma,
    rx: Vec<Dma>,
    tfd: Dma,
    commands: Dma,
    sections: Vec<Dma>,
    rx_read: usize,
    rx_write: usize,
    tx_write: u8,
    tx_busy: [bool; CMD_COUNT],
    pending: VecDeque<Notification>,
    restocked: bool,
    started: bool,
    failed: bool,
    last_command: Option<(u8, u8, u8, u16, usize)>,
    error_tables: [u32; 3],
    alive_version: u8,
    phy_config: u32,
    tx_command_version: u8,
    tx_notification_version: u8,
    queue_alloc_cmd_version: u8,
    pending_data_queue: Option<DataQueue>,
    data_queues: Vec<DataQueue>,
}

impl Transport {
    /// `base` must remain an exclusively owned, uncached BAR0 mapping for life.
    pub(crate) fn new(
        base: usize,
        bar_len: usize,
        pci: (u8, u8, u8),
        fw: &Firmware<'_>,
    ) -> Result<Self, &'static str> {
        if base == 0 || base & 3 != 0 || bar_len < 0x2000 {
            return Err("AX201 invalid MMIO mapping");
        }
        let mut context = Dma::zero(CTXT_BYTES, 4096)?;
        let free = Dma::zero(RX_COUNT * 8, 4096)?;
        let used = Dma::zero(RX_COUNT * 4, 4096)?;
        let status = Dma::zero(16, 64)?;
        let tfd = Dma::zero(CMD_COUNT * TFD_BYTES, 4096)?;
        let commands = Dma::zero(CMD_COUNT * CMD_BYTES, 4096)?;
        let mut rx = Vec::new();
        for _ in 0..RX_COUNT - 1 {
            rx.push(Dma::zero(RX_BYTES, 4096)?);
        }
        // version.size is in dwords. Context-info flags: long TFD, 512 RBDs,
        // 4-KiB receive buffers. Firmware configures the RFH from these fields.
        context.put16(4, (CTXT_BYTES / 4) as u16);
        context.put32(8, 0x0100 | (9 << 4) | (4 << 9));
        context.put64(24, free.phys);
        context.put64(32, used.phys);
        context.put64(40, status.phys);
        context.put64(48, tfd.phys);
        context.bytes_mut()[56] = 2; // ilog2(IWL_CMD_QUEUE_SIZE=32) - 3
        let mut counts = [0usize; 3];
        let mut sections = Vec::new();
        for section in &fw.runtime {
            let part = match section.part {
                SectionPart::Umac => 0,
                SectionPart::Lmac => 1,
                SectionPart::Paging => 2,
            };
            if counts[part] >= 64 {
                return Err("AX201 context DRAM map overflow");
            }
            let mut dma = Dma::zero(section.data.len(), 4096)?;
            dma.bytes_mut().copy_from_slice(section.data);
            context.put64(CTXT_DRAM + part * 64 * 8 + counts[part] * 8, dma.phys);
            counts[part] += 1;
            sections.push(dma);
        }
        Ok(Self {
            base,
            pci,
            context,
            free,
            used,
            status,
            rx,
            tfd,
            commands,
            sections,
            rx_read: 0,
            rx_write: 0,
            tx_write: 0,
            tx_busy: [false; CMD_COUNT],
            pending: VecDeque::new(),
            restocked: false,
            started: false,
            failed: false,
            last_command: None,
            error_tables: [0; 3],
            alive_version: fw.notification_version(0, 1).unwrap_or(0),
            phy_config: fw.phy_config,
            tx_command_version: fw.command_version(1, 0x1c).unwrap_or(7),
            tx_notification_version: fw.notification_version(1, 0x1c).unwrap_or(6),
            queue_alloc_cmd_version: fw.command_version(5, 0x17).unwrap_or(0),
            pending_data_queue: None,
            data_queues: Vec::new(),
        })
    }

    #[inline]
    fn read(&self, offset: usize) -> u32 {
        // Force the physical bus access to MOV: KVM cannot emulate folded
        // BMI2 SHRX [device-MMIO] generated from volatile-load/bit-extract.
        let value: u32;
        unsafe {
            core::arch::asm!("mov {v:e}, dword ptr [{p}]", v=out(reg) value,
            p=in(reg) (self.base + offset), options(nostack, preserves_flags, readonly));
        }
        value
    }
    #[inline]
    fn write(&self, offset: usize, value: u32) {
        unsafe {
            core::arch::asm!("mov dword ptr [{p}], {v:e}", p=in(reg) (self.base + offset),
            v=in(reg) value, options(nostack, preserves_flags));
        }
    }
    fn bits(&self, offset: usize, bits: u32) {
        self.write(offset, self.read(offset) | bits);
    }
    fn poll_bits(&self, offset: usize, mask: u32, expected: u32, ms: u64) -> bool {
        crate::wait::spin_until_timeout_no_exec(ms, || self.read(offset) & mask == expected)
    }
    fn nic_access(&self) -> Result<(), &'static str> {
        self.bits(CSR_GP, GP_ACCESS);
        if self.poll_bits(CSR_GP, GP_CLOCK_READY | GP_SLEEP, GP_CLOCK_READY, 25) {
            Ok(())
        } else {
            Err("AX201 MAC access timeout")
        }
    }
    fn prph_write(&self, address: u32, value: u32) {
        self.write(0x444, (address & 0x000f_ffff) | (3 << 24));
        self.write(0x44c, value);
    }
    fn pin_and_flush(&mut self) {
        for dma in [
            &mut self.context,
            &mut self.free,
            &mut self.used,
            &mut self.status,
            &mut self.tfd,
            &mut self.commands,
        ] {
            dma.flush();
            dma.pinned = true;
        }
        for dma in self.rx.iter_mut().chain(self.sections.iter_mut()) {
            dma.flush();
            dma.pinned = true;
        }
        fence(Ordering::SeqCst);
    }

    pub(crate) fn start(&mut self) -> Result<Notification, &'static str> {
        if self.started || self.failed {
            return Err("AX201 transport already attempted");
        }
        self.started = true;
        let result = self.start_inner(self.phy_config);
        if let Err(error) = result.as_ref() {
            self.failed = true;
            self.write(CSR_INT_MASK, 0);
            crate::log_warn!(target: "net";
                "iwl-ax201: stage=transport-stop reason={} hw_if=0x{:08x} gp=0x{:08x} interrupt=0x{:08x} dma=retained\n",
                error, self.read(0), self.read(CSR_GP), self.read(CSR_INT));
        }
        result
    }

    fn start_inner(&mut self, phy_config: u32) -> Result<Notification, &'static str> {
        let (bus, slot, function) = self.pci;
        // No IRQ handler is installed: keep PCI INTx disabled and poll CSR/RX.
        let command = crate::pci::config_read_u16(bus, slot, function, 4);
        crate::pci::config_write_u16(bus, slot, function, 4, (command | 2 | (1 << 10)) & !4);
        self.write(CSR_INT_MASK, 0);
        self.bits(0x000, 0x0040_0000); // PCI_OWN_SET
        if !self.poll_bits(0, 0x0040_0000, 0x0040_0000, 50) {
            return Err("AX201 PCI ownership timeout");
        }
        self.bits(0x088, 1 << 5); // OS_ALIVE
        self.bits(CSR_RESET, 0x80);
        crate::wait::spin_until_timeout_no_exec(6, || false);
        self.bits(0, 0x0040_0000);
        if !self.poll_bits(0, 0x0040_0000, 0x0040_0000, 50) {
            return Err("AX201 reset ownership timeout");
        }
        self.bits(0x088, 1 << 5);
        self.write(CSR_INT_MASK, 0);
        self.write(CSR_INT, u32::MAX);
        self.write(0x010, u32::MAX); // FH_INT_STATUS
        self.bits(0x100, 0x0080_0000); // L1A_NO_L0S_RX
        self.bits(0x240, 0xffff_0000); // FH wait threshold
        self.bits(0x03c, 2); // L0s disabled, including 22000 devices
        self.bits(0, 0x0008_0000); // HAP_WAKE
        self.bits(CSR_GP, GP_INIT_DONE);
        if !self.poll_bits(CSR_GP, GP_CLOCK_READY, GP_CLOCK_READY, 25) {
            return Err("AX201 MAC clock timeout");
        }
        // Linux mvm_nic_config maps the firmware PHY config into CSR fields.
        let radio_type = phy_config & 3;
        let radio_step = (phy_config >> 2) & 3;
        let radio_dash = (phy_config >> 4) & 3;
        let hw_rev = self.read(0x028);
        let radio = (hw_rev & 0xf) | (radio_type << 10) | (radio_step << 14) | (radio_dash << 12);
        self.write(0, (self.read(0) & !0xff0f) | radio);
        self.context.put16(0, hw_rev as u16);
        self.write(0x05c, 2 | 4); // clear software RF-kill / command-blocked
        if self.read(CSR_GP) & 0x0800_0000 == 0 {
            return Err("AX201 hardware RF-kill active");
        }
        self.bits(0x0a8, 0x800f_ffff); // shadow registers
        self.pin_and_flush();
        crate::pci::config_write_u16(bus, slot, function, 4, command | 2 | 4 | (1 << 10));
        self.write(CSR_CTXT, self.context.phys as u32);
        self.write(CSR_CTXT + 4, (self.context.phys >> 32) as u32);
        self.nic_access()?;
        self.prph_write(0xa05c44, 1); // UREG_CPU_INIT_RUN
        self.write(CSR_GP, self.read(CSR_GP) & !GP_ACCESS);
        crate::log_warn!(target: "net"; "iwl-ax201: stage=context-published dma=0x{:x} rx={} cmd=32\n", self.context.phys, RX_COUNT);
        let mut result = None;
        let mut failure = None;
        let mut deferred = VecDeque::new();
        crate::wait::spin_until_timeout_no_exec(2500, || match self.poll() {
            Ok(Some(notification)) if notification.group == 0 && notification.id == 1 => {
                self.error_tables = alive_error_tables(&notification.payload, self.alive_version);
                result = Some(notification);
                true
            }
            Ok(Some(notification)) => {
                deferred.push_back(notification);
                false
            }
            Ok(None) => false,
            Err(error) => {
                failure = Some(error);
                true
            }
        });
        self.pending.extend(deferred);
        if let Some(error) = failure {
            return Err(error);
        }
        result.ok_or("AX201 firmware ALIVE timeout")
    }

    fn report_error_tables(&self) {
        // Read only the fixed metadata prefix of ALIVE-advertised error
        // records. Linux fw/dump.c: LMAC hcmd=dword23, UMAC hcmd=dword13.
        // No packet buffers or host command payloads are included in logs.
        let access_was_set = self.read(CSR_GP) & GP_ACCESS != 0;
        if self.nic_access().is_err() {
            if !access_was_set {
                self.write(CSR_GP, self.read(CSR_GP) & !GP_ACCESS);
            }
            crate::log_warn!(target: "net"; "iwl-ax201: stage=assert-record unavailable=mac-access\n");
            return;
        }
        for (index, &address) in self.error_tables.iter().enumerate() {
            let words = if index == 2 { 14 } else { 30 };
            if !error_address_valid(address, words * 4) {
                continue;
            }
            self.write(0x40c, address); // HBUS_TARG_MEM_RADDR, auto-incrementing
            let mut record = [0u32; 30];
            for word in &mut record[..words] {
                *word = self.read(0x41c);
            }
            let core = ["lmac0", "lmac1", "umac"][index];
            let hcmd = record[if index == 2 { 13 } else { 23 }];
            crate::log_warn!(target: "net";
                "iwl-ax201: stage=assert-record core={} valid={} error=0x{:08x} hcmd=0x{:08x} last_cmd=0x{:08x}\n",
                core, record[0], record[1], hcmd, if index == 2 { 0 } else { record[29] });
        }
        if !access_was_set {
            self.write(CSR_GP, self.read(CSR_GP) & !GP_ACCESS);
        }
    }

    fn restock_initial(&mut self) {
        for (index, buffer) in self.rx.iter().enumerate() {
            self.free.put64(index * 8, buffer.phys | (index + 1) as u64);
        }
        self.rx_write = self.rx.len();
        self.free.flush();
        fence(Ordering::Release);
        self.write(RX_WPTR, (self.rx_write & !7) as u32);
        self.restocked = true;
        crate::log_warn!(target: "net"; "iwl-ax201: stage=rom-alive rx-published={}\n", self.rx_write & !7);
    }

    pub(crate) fn poll(&mut self) -> Result<Option<Notification>, &'static str> {
        if self.failed {
            return Err("AX201 transport faulted");
        }
        if let Some(notification) = self.pending.pop_front() {
            self.retire_tx_notification(&notification)?;
            return Ok(Some(notification));
        }
        let interrupts = self.read(CSR_INT);
        if interrupts == u32::MAX {
            self.failed = true;
            return Err("AX201 MMIO unavailable");
        }
        if interrupts & FW_ERROR != 0 {
            self.failed = true;
            crate::log_warn!(target: "net"; "iwl-ax201: stage=firmware-fault csr_int=0x{:08x} reset=0x{:08x} gp=0x{:08x} last_hcmd={:?}\n", interrupts, self.read(CSR_RESET), self.read(CSR_GP), self.last_command);
            self.report_error_tables();
            return Err("AX201 firmware hardware/software error");
        }
        if interrupts & ALIVE_IRQ != 0 && !self.restocked {
            self.restock_initial();
        }
        if interrupts != 0 {
            self.write(CSR_INT, interrupts);
        }
        self.status.flush();
        fence(Ordering::Acquire);
        let closed = usize::from(self.status.read16(0)) & 0xfff;
        if closed >= RX_COUNT {
            self.failed = true;
            return Err("AX201 invalid RX producer");
        }
        if closed == self.rx_read {
            return Ok(None);
        }
        self.used.flush();
        let id = self.used.read32(self.rx_read * 4) as usize & 0xfff;
        if id == 0 || id > self.rx.len() {
            self.failed = true;
            return Err("AX201 invalid RX buffer ID");
        }
        let buffer = &mut self.rx[id - 1];
        buffer.flush();
        fence(Ordering::Acquire);
        // Materialize before mutating queue ownership or returning the DMA page.
        let notifications = decode_notifications(buffer.bytes())?;
        for notification in notifications {
            let sequence = notification.sequence;
            // Data TX replies are unsolicited and can carry misleading queue
            // bits. Never reclaim the host command queue for RX/data events.
            if !unsolicited(&notification) && sequence & 0x8000 == 0 && (sequence >> 8) & 0x1f == 0
            {
                self.tx_busy[usize::from(sequence & 0xff) & (CMD_COUNT - 1)] = false;
            }
            self.pending.push_back(notification);
        }
        // Firmware can pack several notifications in a single receive page.
        // Copy them before returning this same page to its free ring.
        buffer.bytes_mut().fill(0);
        buffer.flush();
        self.free.put64(self.rx_write * 8, buffer.phys | id as u64);
        self.rx_write = (self.rx_write + 1) & (RX_COUNT - 1);
        self.rx_read = (self.rx_read + 1) & (RX_COUNT - 1);
        self.free.flush();
        fence(Ordering::Release);
        self.write(RX_WPTR, (self.rx_write & !7) as u32);
        let notification = self.pending.pop_front();
        if let Some(ref notification) = notification {
            self.retire_tx_notification(notification)?;
        }
        Ok(notification)
    }

    /// Select the queue command using firmware metadata as Linux does. The
    /// advertised v3 chooses its body and opcode; WIDE_ID sends wire version0.
    pub(crate) fn queue_allocation_command(&self) -> Result<(u8, u8, u8), &'static str> {
        match self.queue_alloc_cmd_version {
            0 => Ok((1, 0x1d, 0)),
            3 => Ok((5, 0x17, 0)),
            _ => Err("AX201 unsupported queue allocation command version"),
        }
    }

    /// Prepare DMA and return the body for queue_allocation_command. Caller
    /// supplies its response to activate_tx_queue. One allocation may be pending.
    pub(crate) fn prepare_tx_queue(
        &mut self,
        station: u8,
        tid: u8,
    ) -> Result<Vec<u8>, &'static str> {
        if self.failed || !self.restocked {
            return Err("AX201 data transport unavailable");
        }
        if self.pending_data_queue.is_some() {
            return Err("AX201 queue allocation pending");
        }
        if self.data_queues.len() >= MAX_DATA_QUEUES {
            return Err("AX201 data queue capacity");
        }
        self.queue_allocation_command()?;
        let mut queue = DataQueue::new(station, tid)?;
        let config = queue.config(self.queue_alloc_cmd_version)?;
        self.pending_data_queue = Some(queue);
        Ok(config)
    }

    /// Empty software window is necessary for station teardown, but is not
    /// itself a firmware queue-disable acknowledgement. This implementation
    /// retains every published queue until device reset and does not reuse it.
    pub(crate) fn tx_queue_idle(&self, queue_id: u16) -> bool {
        self.data_queues
            .iter()
            .find(|queue| queue.id == Some(queue_id))
            .is_some_and(|queue| queue.outstanding == 0)
    }

    pub(crate) fn activate_tx_queue(&mut self, response: &[u8]) -> Result<u16, &'static str> {
        if response.len() != 8 {
            return Err("AX201 malformed queue allocation response");
        }
        let id = u16::from_le_bytes([response[0], response[1]]);
        let flags = u16::from_le_bytes([response[2], response[3]]);
        let write = u16::from_le_bytes([response[4], response[5]]);
        if flags != 0
            || id == 0
            || id >= 512
            || write >= 256
            || self.data_queues.iter().any(|queue| queue.id == Some(id))
        {
            return Err("AX201 queue allocation rejected");
        }
        let mut queue = self
            .pending_data_queue
            .take()
            .ok_or("AX201 no pending queue allocation")?;
        queue.id = Some(id);
        queue.read = write as u8;
        queue.write = write as u8;
        self.data_queues.push(queue);
        Ok(id)
    }

    /// Publish a single nonaggregated raw 802.11 frame. Returned sequence is a
    /// token only: completion/ACK is established by TX_CMD notification v7.
    pub(crate) fn send_raw80211(
        &mut self,
        queue_id: u16,
        frame: &[u8],
        options: TxOptions,
    ) -> Result<u16, &'static str> {
        if self.failed || !self.restocked {
            return Err("AX201 data transport unavailable");
        }
        if !matches!(self.tx_command_version, 7 | 9) || self.tx_notification_version != 7 {
            return Err("AX201 unsupported TX command/notification version");
        }
        if options.encrypt {
            return Err("AX201 firmware encryption not configured");
        }
        if !matches!(options.antenna, 1 | 2) {
            return Err("AX201 invalid TX antenna");
        }
        let hdr_len = data::header_len(frame)?;
        if frame.len() > 2304 + 36 || frame.len() < hdr_len {
            return Err("AX201 invalid TX MPDU size");
        }
        let queue = self
            .data_queues
            .iter_mut()
            .find(|queue| queue.id == Some(queue_id))
            .ok_or("AX201 TX queue not allocated")?;
        let frame_tid = if frame[0] & 0x0c == 0 {
            15
        } else if hdr_len == 26 {
            frame[24] & 0x0f
        } else {
            0
        };
        if frame_tid != queue.tid {
            return Err("AX201 frame TID does not match TX queue");
        }
        let index = usize::from(queue.write) & (DATA_COUNT - 1);
        if queue.outstanding >= DATA_COUNT || queue.busy[index] {
            return Err("AX201 TX queue full");
        }
        let sequence = ((queue_id & 0x1f) << 8) | index as u16;
        // Unlike host commands, TX_CMD's gen2 data path uses the short header.
        let start = index * DATA_BYTES;
        let packet = &mut queue.packets.bytes_mut()[start..start + DATA_BYTES];
        packet.fill(0);
        packet[0] = 0x1c;
        packet[2..4].copy_from_slice(&sequence.to_le_bytes());
        packet[4..6].copy_from_slice(&(frame.len() as u16).to_le_bytes());
        let pad = (4 - (hdr_len & 3)) & 3;
        let offload = ((hdr_len / 2) as u16) << 8 | if pad != 0 { 1 << 13 } else { 0 };
        packet[6..8].copy_from_slice(&offload.to_le_bytes());
        let flags = 1u32 | 2 | if options.high_priority { 4 } else { 0 }; // CMD_RATE | ENCRYPT_DIS
        packet[8..12].copy_from_slice(&flags.to_le_bytes());
        // dram_info[8] is firmware-owned and zero on first publication.
        let rate = if self.tx_command_version > 8 {
            if options.band_2ghz { 0 } else { 1 << 8 }
        } else if options.band_2ghz {
            10 | (1 << 9)
        } else {
            13
        };
        let rate = rate | (u32::from(options.antenna) << 14);
        packet[20..24].copy_from_slice(&rate.to_le_bytes());
        packet[24..24 + hdr_len].copy_from_slice(&frame[..hdr_len]);
        let body_at = 24 + hdr_len + pad;
        packet[body_at..body_at + frame.len() - hdr_len].copy_from_slice(&frame[hdr_len..]);
        let descriptor = index * TFD_BYTES;
        queue.tfd.bytes_mut()[descriptor..descriptor + TFD_BYTES].fill(0);
        let count = if frame.len() == hdr_len { 2 } else { 3 };
        queue.tfd.put16(descriptor, count);
        queue.tfd.put16(descriptor + 2, 20);
        queue
            .tfd
            .put64(descriptor + 4, queue.packets.phys + start as u64);
        queue.tfd.put16(descriptor + 12, (4 + hdr_len + pad) as u16);
        queue
            .tfd
            .put64(descriptor + 14, queue.packets.phys + (start + 20) as u64);
        if count == 3 {
            queue
                .tfd
                .put16(descriptor + 22, (frame.len() - hdr_len) as u16);
            queue.tfd.put64(
                descriptor + 24,
                queue.packets.phys + (start + body_at) as u64,
            );
        }
        // 22000 byte-count table uses dwords and a fetch-chunk count in bits12+.
        // At most three transfer buffers fit in the first64-byte TFD chunk.
        queue
            .byte_count
            .put16(index * 2, frame.len().div_ceil(4) as u16);
        queue.packets.flush();
        queue.tfd.flush();
        queue.byte_count.flush();
        queue.busy[index] = true;
        queue.outstanding += 1;
        queue.write = queue.write.wrapping_add(1);
        let doorbell = u32::from(queue.write) | (u32::from(queue_id) << 16);
        fence(Ordering::Release);
        self.write(TX_WPTR, doorbell);
        Ok(sequence)
    }

    fn retire_tx_notification(&mut self, notification: &Notification) -> Result<(), &'static str> {
        if notification.id != 0x1c || !matches!(notification.group, 0 | 1) {
            return Ok(());
        }
        if self.tx_notification_version != 7 {
            return Err("AX201 unsupported TX response version");
        }
        let report = data::tx_report(&notification.payload)?;
        let queue = self
            .data_queues
            .iter_mut()
            .find(|queue| queue.id == Some(report.queue))
            .ok_or("AX201 TX response for unknown queue")?;
        let distance = report.next_index.wrapping_sub(queue.read) as usize;
        let completed_index = usize::from(report.sequence & 0xff);
        if distance == 0
            || distance > queue.outstanding
            || (report.sequence >> 8) & 0x1f != report.queue & 0x1f
            || completed_index >= DATA_COUNT
            || !(0..distance)
                .any(|step| (usize::from(queue.read) + step) & (DATA_COUNT - 1) == completed_index)
        {
            return Err("AX201 TX retirement outside outstanding window");
        }
        for _ in 0..distance {
            let index = usize::from(queue.read) & (DATA_COUNT - 1);
            if !queue.busy[index] {
                return Err("AX201 TX retirement of unowned slot");
            }
            queue.busy[index] = false;
            queue.outstanding -= 1;
            queue.read = queue.read.wrapping_add(1);
        }
        Ok(())
    }

    /// Submit one wide host command. Payload is bounded to one 4-KiB slot.
    /// Completion arrives as a notification carrying the returned sequence.
    pub(crate) fn send_command(
        &mut self,
        group: u8,
        id: u8,
        version: u8,
        payload: &[u8],
    ) -> Result<u16, &'static str> {
        if self.failed || !self.restocked {
            return Err("AX201 command transport unavailable");
        }
        if payload.len() > CMD_BYTES - 8 {
            return Err("AX201 command too large");
        }
        let index = usize::from(self.tx_write) & (CMD_COUNT - 1);
        if self.tx_busy[index] {
            return Err("AX201 command queue full");
        }
        let sequence = u16::from(self.tx_write); // DQA command queue ID is 0
        let start = index * CMD_BYTES;
        let target = &mut self.commands.bytes_mut()[start..start + CMD_BYTES];
        target.fill(0);
        target[0] = id;
        target[1] = if group == 0 { 1 } else { group }; // LONG_GROUP / DEF_ID
        target[2..4].copy_from_slice(&sequence.to_le_bytes());
        target[4..6].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        target[7] = version;
        target[8..8 + payload.len()].copy_from_slice(payload);
        let length = payload.len() + 8;
        let first = length.min(20); // IWL_FIRST_TB_SIZE
        let descriptor = index * TFD_BYTES;
        self.tfd.bytes_mut()[descriptor..descriptor + TFD_BYTES].fill(0);
        self.tfd
            .put16(descriptor, if length > first { 2 } else { 1 });
        self.tfd.put16(descriptor + 2, first as u16);
        self.tfd
            .put64(descriptor + 4, self.commands.phys + start as u64);
        if length > first {
            self.tfd.put16(descriptor + 12, (length - first) as u16);
            self.tfd
                .put64(descriptor + 14, self.commands.phys + (start + first) as u64);
        }
        self.commands.flush();
        self.tfd.flush();
        self.tx_busy[index] = true;
        self.last_command = Some((
            if group == 0 { 1 } else { group },
            id,
            version,
            sequence,
            payload.len(),
        ));
        self.tx_write = self.tx_write.wrapping_add(1);
        fence(Ordering::Release);
        self.write(TX_WPTR, u32::from(self.tx_write));
        Ok(sequence)
    }
}

// Linux's no-reclaim list is needed even when firmware omits SEQ_RX_FRAME
// on legacy unsolicited receive events. Queue0 belongs only to host commands.
fn unsolicited(notification: &Notification) -> bool {
    (notification.id == 0x1c && matches!(notification.group, 0 | 1))
        || notification.group == 0
            && matches!(
                notification.id,
                0x1c | 0xc0 | 0xc1 | 0xc2 | 0xc3 | 0xc4 | 0xc5 | 0x4e | 0x4f
            )
}

/// RX page records have a 4-byte size/flags word followed by the 4-byte
/// notification header. Each whole record advances to a 64-byte boundary.
fn decode_notifications(bytes: &[u8]) -> Result<Vec<Notification>, &'static str> {
    let mut notifications = Vec::new();
    let mut offset = 0usize;
    while offset + 8 <= bytes.len() {
        let length = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        if length == 0x5555_0000 || length == 0 {
            break;
        }
        let count = (length & 0x3fff) as usize;
        if count < 4 || offset + 4 + count > bytes.len() {
            return Err("AX201 malformed RX notification");
        }
        let header = &bytes[offset + 4..offset + 8];
        notifications.push(Notification {
            id: header[0],
            group: header[1],
            sequence: u16::from_le_bytes([header[2], header[3]]),
            payload: bytes[offset + 8..offset + 4 + count].to_vec(),
        });
        offset += (count + 4 + 63) & !63;
    }
    Ok(notifications)
}

// Linux fw/api/alive.h v3..v6. Reject unknown wire layouts rather than
// reading an arbitrary device address as an error record.
fn alive_error_tables(payload: &[u8], version: u8) -> [u32; 3] {
    let umac_at = match (version, payload.len()) {
        (3, 68) => 60,
        (4, 116) | (5, 128) | (6, 144) => 108,
        _ => return [0; 3],
    };
    let word = |at| u32::from_le_bytes(payload[at..at + 4].try_into().unwrap());
    [
        word(20),
        if version >= 4 { word(68) } else { 0 },
        word(umac_at) & !0xc000_0000,
    ]
}
fn error_address_valid(address: u32, bytes: usize) -> bool {
    address & 3 == 0
        && [
            (0x400000u32, 0x4d0000u32),
            (0x800000, 0x810000),
            (0x880000, 0x888000),
        ]
        .iter()
        .any(|&(start, end)| {
            address >= start
                && address
                    .checked_add(bytes as u32)
                    .is_some_and(|last| last <= end)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alive_error_records_require_known_layouts_and_device_memory() {
        let mut payload = alloc::vec![0; 144];
        payload[20..24].copy_from_slice(&0x800100u32.to_le_bytes());
        payload[68..72].copy_from_slice(&0x880100u32.to_le_bytes());
        payload[108..112].copy_from_slice(&0xc0400100u32.to_le_bytes());
        assert_eq!(
            alive_error_tables(&payload, 6),
            [0x800100, 0x880100, 0x400100]
        );
        assert_eq!(alive_error_tables(&payload[..140], 6), [0; 3]);
        assert_eq!(alive_error_tables(&payload, 7), [0; 3]);
        assert!(error_address_valid(0x400100, 120));
        assert!(error_address_valid(0x800100, 120));
        assert!(!error_address_valid(0, 120));
        assert!(!error_address_valid(0x800101, 120));
        assert!(!error_address_valid(0x80fff0, 120));
        assert!(!error_address_valid(0xc0400100, 120));
    }

    fn record(bytes: &mut [u8], at: usize, group: u8, id: u8, seq: u16, payload: &[u8]) {
        bytes[at..at + 4].copy_from_slice(&((payload.len() + 4) as u32).to_le_bytes());
        bytes[at + 4] = id;
        bytes[at + 5] = group;
        bytes[at + 6..at + 8].copy_from_slice(&seq.to_le_bytes());
        bytes[at + 8..at + 8 + payload.len()].copy_from_slice(payload);
    }

    #[test]
    fn receive_page_keeps_all_aligned_notifications() {
        let mut bytes = [0u8; 192];
        record(&mut bytes, 0, 0, 1, 0x8000, &[0xfe, 0xca, 0, 0]);
        record(&mut bytes, 64, 12, 2, 7, &[1, 2, 3]);
        bytes[128..132].copy_from_slice(&0x5555_0000u32.to_le_bytes());
        let notifications = decode_notifications(&bytes).unwrap();
        assert_eq!(notifications.len(), 2);
        assert_eq!(
            (
                notifications[0].group,
                notifications[0].id,
                notifications[0].sequence
            ),
            (0, 1, 0x8000)
        );
        assert_eq!(notifications[1].payload, [1, 2, 3]);
    }

    #[test]
    fn receive_rejects_lengths_outside_dma_page() {
        let mut bytes = [0u8; 64];
        bytes[..4].copy_from_slice(&65u32.to_le_bytes());
        assert!(decode_notifications(&bytes).is_err());
        bytes[..4].copy_from_slice(&3u32.to_le_bytes());
        assert!(decode_notifications(&bytes).is_err());
    }

    #[test]
    fn receive_size_masks_transport_flags() {
        let mut bytes = [0u8; 64];
        record(&mut bytes, 0, 2, 3, 4, &[5]);
        bytes[3] = 0x80; // flush-RB transport flag, not part of record size
        let notification = decode_notifications(&bytes).unwrap().pop().unwrap();
        assert_eq!(notification.payload, [5]);
    }
}
