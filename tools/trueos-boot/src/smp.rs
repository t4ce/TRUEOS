//! Native x86 INIT/SIPI startup, retaining the kernel's existing MpInfo ABI.
//!
//! All allocation happens before ExitBootServices. `start_aps` is called only
//! after the new page tables are active and firmware no longer owns the CPUs.
//! Both identity and HHDM mappings must cover the supplied physical allocations.

use core::arch::{asm, x86_64::__cpuid_count};
use core::ptr::{addr_of, addr_of_mut, read_unaligned, read_volatile, write_bytes, write_unaligned, write_volatile};
use core::sync::atomic::{fence, AtomicU32, AtomicU64, Ordering};

#[cfg(not(test))]
core::arch::global_asm!(include_str!("ap_trampoline.S"));

pub const MAX_CPUS: usize = 256;
pub const STORAGE_PAGES: usize = 16;
/// Match the kernel handoff PAT layout, including WC at index 5. Native
/// kernel MMIO mappings select these indices directly on every processor.
pub const HANDOFF_PAT: u64 = 0x0000_0105_0007_0406;
// The existing kernel requests 16 MiB stacks; AP workers run the same code.
pub const STACK_PAGES_PER_CPU: usize = 4096;
const MAX_MTRR_ENTRIES: usize = 524;

#[derive(Clone, Copy)]
pub struct Cpu {
    pub processor_id: u32,
    pub lapic_id: u32,
}

pub struct Topology {
    cpus: [Cpu; MAX_CPUS],
    count: usize,
    bsp_lapic_id: u32,
    apic_msr: u64,
    x2apic: bool,
    tsc_hz: u64,
}

impl Topology {
    pub fn cpu_count(&self) -> usize { self.count }
    pub fn bsp_lapic_id(&self) -> u32 { self.bsp_lapic_id }
    pub fn x2apic(&self) -> bool { self.x2apic }
    fn push(&mut self, cpu: Cpu) {
        if self.count < MAX_CPUS && !self.cpus[..self.count].iter().any(|c| c.lapic_id == cpu.lapic_id) {
            self.cpus[self.count] = cpu;
            self.count += 1;
        }
    }
}

#[repr(C)]
pub struct MpInfo {
    pub processor_id: u32,
    pub lapic_id: u32,
    pub reserved: u64,
    pub goto_addr: AtomicU64,
    pub extra_argument: AtomicU64,
}

#[repr(C)]
pub struct MpResponse {
    pub revision: u64,
    pub flags: u32,
    pub bsp_lapic_id: u32,
    pub cpu_count: u64,
    pub cpus: u64,
}

#[repr(C, align(16))]
struct ApState {
    lapic_id: u32,
    stack_top_low: u32,
    info: u64,
    stack_top: u64,
    ready: AtomicU32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MtrrEntry { msr: u32, padding: u32, value: u64 }

#[repr(C)]
struct MtrrSnapshot {
    count: u32,
    supported: u32,
    entries: [MtrrEntry; MAX_MTRR_ENTRIES],
}

#[repr(C)]
struct Storage {
    response: MpResponse,
    pointers: [u64; MAX_CPUS],
    infos: [MpInfo; MAX_CPUS],
    states: [ApState; MAX_CPUS],
    mtrrs: MtrrSnapshot,
}

const _: () = assert!(core::mem::size_of::<Storage>() <= STORAGE_PAGES * 4096);
const _: () = assert!(core::mem::size_of::<ApState>() == 32);
const _: () = assert!(core::mem::offset_of!(ApState, ready) == 24);
const _: () = assert!(core::mem::offset_of!(MpInfo, goto_addr) == 16);

#[repr(C)]
struct Parameters {
    cr3: u32,
    padding: u32,
    apic_msr: u64,
    efer: u64,
    pat: u64,
    states: u32,
    count: u32,
    hhdm: u64,
    mtrrs: u32,
    reserved: u32,
}

unsafe extern "C" {
    static trueos_ap_trampoline_start: u8;
    static trueos_ap_trampoline_end: u8;
    static trueos_ap_far32: u8;
    static trueos_ap_gdtr32: u8;
    static trueos_ap_gdtr64: u8;
    static trueos_ap_parameters: u8;
    static trueos_ap_mode32: u8;
    static trueos_ap_gdt: u8;
}

pub struct Prepared {
    storage: u64,
    trampoline: u64,
    count: usize,
    bsp_lapic_id: u32,
    apic_msr: u64,
    x2apic: bool,
    tsc_hz: u64,
    hhdm: u64,
}

pub struct StartSummary {
    /// Successfully parked APs; excludes the BSP.
    pub started: usize,
    pub failed: usize,
}

/// Discover enabled logical processors. P cores, E cores and SMT threads use
/// the same MADT/APIC mechanism; no core-type assumptions enter this code.
/// Missing or malformed ACPI falls back to a BSP-only response.
pub unsafe fn discover(rsdp_phys: u64) -> Topology {
    let apic_msr = unsafe { rdmsr(0x1b) };
    let x2apic = apic_msr & (1 << 10) != 0;
    let bsp_lapic_id = if x2apic {
        unsafe { rdmsr(0x802) as u32 }
    } else {
        let base = apic_msr & 0xffff_f000;
        unsafe { read_volatile((base + 0x20) as *const u32) >> 24 }
    };
    let mut topology = Topology {
        cpus: [Cpu { processor_id: 0, lapic_id: 0 }; MAX_CPUS],
        count: 0, bsp_lapic_id, apic_msr, x2apic, tsc_hz: tsc_frequency(),
    };
    // BSP first means even a completely failed AP bring-up has a valid response.
    topology.push(Cpu { processor_id: bsp_lapic_id, lapic_id: bsp_lapic_id });
    let Some(madt) = (unsafe { find_madt(rsdp_phys) }) else { return topology };
    unsafe { add_madt_processors(&mut topology, madt) };
    topology
}

unsafe fn add_madt_processors(topology: &mut Topology, madt: u64) {
    let length = unsafe { read_unaligned((madt + 4) as *const u32) } as u64;
    let mut cursor = madt + 44;
    while cursor + 2 <= madt + length {
        let kind = unsafe { read_unaligned(cursor as *const u8) };
        let size = unsafe { read_unaligned((cursor + 1) as *const u8) } as u64;
        if size < 2 || cursor + size > madt + length { break; }
        let candidate = if kind == 0 && size >= 8 {
            let flags = unsafe { read_unaligned((cursor + 4) as *const u32) };
            (flags & 1 != 0).then(|| Cpu {
                processor_id: unsafe { read_unaligned((cursor + 2) as *const u8) } as u32,
                lapic_id: unsafe { read_unaligned((cursor + 3) as *const u8) } as u32,
            })
        } else if kind == 9 && size >= 16 {
            let flags = unsafe { read_unaligned((cursor + 8) as *const u32) };
            let id = unsafe { read_unaligned((cursor + 4) as *const u32) };
            (flags & 1 != 0 && (topology.x2apic || id <= 255)).then(|| Cpu {
                processor_id: unsafe { read_unaligned((cursor + 12) as *const u32) }, lapic_id: id,
            })
        } else { None };
        if let Some(cpu) = candidate {
            if cpu.lapic_id == topology.bsp_lapic_id { topology.cpus[0] = cpu; }
            else { topology.push(cpu); }
        }
        cursor += size;
    }
}

/// Allocate one trampoline page below 1 MiB, STORAGE_PAGES below 4 GiB, and
/// `topology.cpu_count() * STACK_PAGES_PER_CPU` contiguous stack pages below
/// 4 GiB. All are permanent boot allocations. The root page table is below
/// 4 GiB and implements 4-level paging with identity and HHDM mappings.
pub unsafe fn prepare(
    topology: &Topology,
    trampoline_phys: u64,
    storage_phys: u64,
    stacks_phys: u64,
    cr3_phys: u64,
    hhdm: u64,
) -> Result<Prepared, &'static str> {
    let image_start = addr_of!(trueos_ap_trampoline_start) as usize;
    let image_size = addr_of!(trueos_ap_trampoline_end) as usize - image_start;
    let stacks_end = stacks_phys.checked_add((topology.count * STACK_PAGES_PER_CPU * 4096) as u64)
        .ok_or("AP stack address overflow")?;
    if image_size > 4096 || trampoline_phys == 0 || trampoline_phys >= 0x10_0000
        || trampoline_phys & 4095 != 0 || storage_phys & 4095 != 0
        || stacks_phys & 4095 != 0 || cr3_phys & 4095 != 0
        || storage_phys + (STORAGE_PAGES * 4096) as u64 > 0x1_0000_0000
        || stacks_end >= 0x1_0000_0000 || cr3_phys >= 0x1_0000_0000 {
        return Err("Invalid low-memory AP allocation");
    }
    unsafe {
        write_bytes(storage_phys as *mut u8, 0, STORAGE_PAGES * 4096);
        core::ptr::copy_nonoverlapping(image_start as *const u8, trampoline_phys as *mut u8, image_size);
    }
    let storage = unsafe { &mut *(storage_phys as *mut Storage) };
    storage.response = MpResponse {
        revision: 0, flags: u32::from(topology.x2apic), bsp_lapic_id: topology.bsp_lapic_id,
        cpu_count: 1, cpus: addr_of!(storage.pointers) as u64 + hhdm,
    };
    for (i, cpu) in topology.cpus[..topology.count].iter().enumerate() {
        let stack_top = stacks_phys + ((i + 1) * STACK_PAGES_PER_CPU * 4096) as u64 + hhdm;
        storage.infos[i] = MpInfo {
            processor_id: cpu.processor_id, lapic_id: cpu.lapic_id,
            reserved: stack_top, goto_addr: AtomicU64::new(0), extra_argument: AtomicU64::new(0),
        };
        storage.states[i] = ApState {
            lapic_id: cpu.lapic_id, stack_top_low: (stack_top - hhdm) as u32,
            info: addr_of!(storage.infos[i]) as u64 + hhdm,
            stack_top, ready: AtomicU32::new(u32::from(cpu.lapic_id == topology.bsp_lapic_id)), padding: 0,
        };
    }
    storage.pointers[0] = addr_of!(storage.infos[0]) as u64 + hhdm;
    unsafe { capture_mtrrs(&mut storage.mtrrs) };
    let offset = |symbol: *const u8| symbol as u64 - image_start as u64;
    let at = |symbol: *const u8| trampoline_phys + offset(symbol);
    unsafe {
        write_unaligned(at(addr_of!(trueos_ap_far32)) as *mut u32,
            (trampoline_phys + offset(addr_of!(trueos_ap_mode32))) as u32);
        write_unaligned((at(addr_of!(trueos_ap_gdtr32)) + 2) as *mut u32,
            at(addr_of!(trueos_ap_gdt)) as u32);
        write_unaligned((at(addr_of!(trueos_ap_gdtr64)) + 2) as *mut u64,
            at(addr_of!(trueos_ap_gdt)) + hhdm);
        write_unaligned(at(addr_of!(trueos_ap_parameters)) as *mut Parameters, Parameters {
            cr3: cr3_phys as u32, padding: 0, apic_msr: topology.apic_msr,
            efer: (1 << 8) | if __cpuid_count(0x8000_0000, 0).eax >= 0x8000_0001
                && __cpuid_count(0x8000_0001, 0).edx & (1 << 20) != 0 { 1 << 11 } else { 0 },
            pat: HANDOFF_PAT,
            states: addr_of!(storage.states) as u32, count: topology.count as u32,
            hhdm, mtrrs: addr_of!(storage.mtrrs) as u32, reserved: 0,
        });
    }
    fence(Ordering::SeqCst);
    Ok(Prepared { storage: storage_phys, trampoline: trampoline_phys, count: topology.count,
        bsp_lapic_id: topology.bsp_lapic_id, apic_msr: topology.apic_msr,
        x2apic: topology.x2apic, tsc_hz: topology.tsc_hz, hhdm })
}

impl Prepared {
    pub fn response_phys(&self) -> u64 { self.storage }

    /// Firmware services must be exited and the loader's own page tables active.
    /// Failed APs are omitted; successful APs remain parked until the kernel
    /// publishes their entry functions with MpInfo::bootstrap().
    /// `report` receives every discovered APIC ID (including the BSP) and
    /// whether it was included, so physical tests can identify failed cores.
    pub unsafe fn start_aps(&mut self, mut report: impl FnMut(u32, bool)) -> StartSummary {
        // APs concurrently access their atomic state and info. Keep references
        // confined to individual fields instead of borrowing all storage mutably.
        let storage = (self.storage + self.hhdm) as *mut Storage;
        let mut summary = StartSummary { started: 0, failed: 0 };
        let vector = (self.trampoline >> 12) as u32;
        for i in 0..self.count {
            let state = unsafe { &(*storage).states[i] };
            let id = state.lapic_id;
            if id == self.bsp_lapic_id { continue; }
            if !unsafe { self.send_ipi(id, 0x4500) } {
                summary.failed += 1;
                continue;
            }
            self.delay_us(10_000);
            let mut delivered = true;
            for _ in 0..2 {
                delivered &= unsafe { self.send_ipi(id, 0x4600 | vector) };
                self.delay_us(200);
            }
            let deadline = read_tsc().wrapping_add(self.tsc_hz);
            while state.ready.load(Ordering::Acquire) == 0 && before_deadline(deadline) {
                core::hint::spin_loop();
            }
            if delivered && state.ready.load(Ordering::Acquire) != 0 {
                summary.started += 1;
            } else { summary.failed += 1; }
        }
        // Build once after all startup attempts so a late AP can be included
        // safely. Each timed-out AP retains its own state and stack regardless.
        let mut count = 0;
        for i in 0..self.count {
            let state = unsafe { &(*storage).states[i] };
            let ready = state.ready.load(Ordering::Acquire) != 0;
            if ready {
                unsafe { addr_of_mut!((*storage).pointers[count]).write(addr_of!((*storage).infos[i]) as u64) };
                count += 1;
            }
            report(state.lapic_id, ready);
        }
        unsafe { addr_of_mut!((*storage).response.cpu_count).write(count as u64) };
        summary.started = count.saturating_sub(1);
        summary.failed = self.count.saturating_sub(count);
        fence(Ordering::SeqCst);
        summary
    }

    unsafe fn send_ipi(&self, id: u32, command: u32) -> bool {
        if self.x2apic {
            unsafe { wrmsr(0x830, ((id as u64) << 32) | command as u64) };
            true
        } else {
            if id > 255 || !unsafe { self.wait_icr() } { return false; }
            let base = (self.apic_msr & 0xffff_f000) + self.hhdm;
            unsafe {
                write_volatile((base + 0x310) as *mut u32, id << 24);
                write_volatile((base + 0x300) as *mut u32, command);
                self.wait_icr()
            }
        }
    }

    unsafe fn wait_icr(&self) -> bool {
        let base = (self.apic_msr & 0xffff_f000) + self.hhdm;
        let deadline = read_tsc().wrapping_add(self.tsc_hz / 10);
        while unsafe { read_volatile((base + 0x300) as *const u32) } & (1 << 12) != 0 {
            if !before_deadline(deadline) { return false; }
            core::hint::spin_loop();
        }
        true
    }

    fn delay_us(&self, us: u64) {
        let deadline = read_tsc().wrapping_add(self.tsc_hz / 1_000_000 * us);
        while before_deadline(deadline) { core::hint::spin_loop(); }
    }
}

fn read_tsc() -> u64 { unsafe { core::arch::x86_64::_rdtsc() } }
fn before_deadline(deadline: u64) -> bool { (read_tsc().wrapping_sub(deadline) as i64) < 0 }

fn tsc_frequency() -> u64 {
    let maximum = __cpuid_count(0, 0).eax;
    if maximum >= 0x15 {
        let r = __cpuid_count(0x15, 0);
        if r.eax != 0 && r.ebx != 0 && r.ecx != 0 { return r.ecx as u64 * r.ebx as u64 / r.eax as u64; }
    }
    if maximum >= 0x16 {
        let mhz = __cpuid_count(0x16, 0).eax & 0xffff;
        if mhz != 0 { return mhz as u64 * 1_000_000; }
    }
    // Conservative timing on older firmware: a 10 ms INIT delay is a minimum,
    // so overestimating the clock is safe. Known hybrid Intel CPUs expose 0x15.
    10_000_000_000
}

unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32; let high: u32;
    unsafe { asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high, options(nomem, nostack)) };
    ((high as u64) << 32) | low as u64
}
unsafe fn wrmsr(msr: u32, value: u64) {
    unsafe { asm!("wrmsr", in("ecx") msr, in("eax") value as u32, in("edx") (value >> 32) as u32,
        options(nomem, nostack)) };
}

unsafe fn capture_mtrrs(snapshot: &mut MtrrSnapshot) {
    if __cpuid_count(1, 0).edx & (1 << 12) == 0 { return; }
    snapshot.supported = 1;
    let capabilities = unsafe { rdmsr(0xfe) };
    let variables = (capabilities & 0xff) as u32;
    let mut push = |msr: u32| {
        snapshot.entries[snapshot.count as usize] = MtrrEntry { msr, padding: 0, value: unsafe { rdmsr(msr) } };
        snapshot.count += 1;
    };
    for i in 0..variables * 2 { push(0x200 + i); }
    if capabilities & (1 << 8) != 0 {
        for msr in [0x250, 0x258, 0x259, 0x268, 0x269, 0x26a, 0x26b, 0x26c, 0x26d, 0x26e, 0x26f] { push(msr); }
    }
    push(0x2ff);
}

unsafe fn find_madt(rsdp: u64) -> Option<u64> {
    if rsdp == 0 || unsafe { core::slice::from_raw_parts(rsdp as *const u8, 8) } != b"RSD PTR "
        || !unsafe { checksum(rsdp, 20) } { return None; }
    let revision = unsafe { read_unaligned((rsdp + 15) as *const u8) };
    let xsdt = if revision >= 2 {
        let length = unsafe { read_unaligned((rsdp + 20) as *const u32) };
        if !(36..=4096).contains(&length) || !unsafe { checksum(rsdp, length as usize) } { return None; }
        unsafe { read_unaligned((rsdp + 24) as *const u64) }
    } else { 0 };
    let (root, stride, signature) = if xsdt != 0 { (xsdt, 8, b"XSDT") }
        else { (unsafe { read_unaligned((rsdp + 16) as *const u32) } as u64, 4, b"RSDT") };
    if !unsafe { valid_sdt(root, signature) } { return None; }
    let length = unsafe { read_unaligned((root + 4) as *const u32) } as u64;
    let mut cursor = root + 36;
    while cursor + stride <= root + length {
        let address = if stride == 8 { unsafe { read_unaligned(cursor as *const u64) } }
            else { (unsafe { read_unaligned(cursor as *const u32) }) as u64 };
        if unsafe { valid_sdt(address, b"APIC") } {
            let len = unsafe { read_unaligned((address + 4) as *const u32) };
            if len >= 44 { return Some(address); }
        }
        cursor += stride;
    }
    None
}
unsafe fn valid_sdt(address: u64, signature: &[u8; 4]) -> bool {
    if address == 0 || unsafe { core::slice::from_raw_parts(address as *const u8, 4) } != signature { return false; }
    let length = unsafe { read_unaligned((address + 4) as *const u32) };
    (36..=1024*1024).contains(&length) && unsafe { checksum(address, length as usize) }
}
unsafe fn checksum(address: u64, length: usize) -> bool {
    unsafe { core::slice::from_raw_parts(address as *const u8, length) }.iter()
        .fold(0u8, |sum, byte| sum.wrapping_add(*byte)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topology(x2apic: bool) -> Topology {
        let mut topology = Topology {
            cpus: [Cpu { processor_id: 0, lapic_id: 0 }; MAX_CPUS], count: 0,
            bsp_lapic_id: 0, apic_msr: 0xfee0_0800, x2apic, tsc_hz: 1_000_000_000,
        };
        topology.push(Cpu { processor_id: 0, lapic_id: 0 });
        topology
    }

    fn madt(entries: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; 44];
        bytes[..4].copy_from_slice(b"APIC");
        bytes.extend_from_slice(entries);
        let length = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&length.to_le_bytes());
        bytes
    }

    fn x2entry(uid: u32, lapic: u32, enabled: bool) -> [u8; 16] {
        let mut entry = [9, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        entry[4..8].copy_from_slice(&lapic.to_le_bytes());
        entry[8..12].copy_from_slice(&u32::from(enabled).to_le_bytes());
        entry[12..16].copy_from_slice(&uid.to_le_bytes());
        entry
    }

    #[test]
    fn enabled_processors_keep_every_logical_id_independent_of_core_kind() {
        let bytes = madt(&[
            0, 8, 9, 0, 1, 0, 0, 0, // BSP ACPI UID 9.
            0, 8, 2, 2, 1, 0, 0, 0, // A P core thread.
            0, 8, 3, 3, 1, 0, 0, 0, // Its SMT sibling.
            0, 8, 8, 16, 1, 0, 0, 0, // An E core with a sparse APIC ID.
            0, 8, 4, 4, 0, 0, 0, 0, // Disabled entry.
            0, 8, 2, 2, 1, 0, 0, 0, // Duplicate entry.
        ]);
        let mut topology = topology(false);
        unsafe { add_madt_processors(&mut topology, bytes.as_ptr() as u64) };
        assert_eq!(topology.cpu_count(), 4);
        assert_eq!(topology.cpus[0].processor_id, 9);
        assert_eq!(topology.cpus[..4].map_ids(), [0, 2, 3, 16]);
    }

    trait CpuIds { fn map_ids(&self) -> Vec<u32>; }
    impl CpuIds for [Cpu] {
        fn map_ids(&self) -> Vec<u32> { self.iter().map(|cpu| cpu.lapic_id).collect() }
    }

    #[test]
    fn full_x2apic_ids_are_retained_only_in_x2apic_mode() {
        let mut entries = Vec::new();
        entries.extend_from_slice(&x2entry(21, 0x102, true));
        entries.extend_from_slice(&x2entry(22, 7, true));
        entries.extend_from_slice(&x2entry(23, 0x103, false));
        let bytes = madt(&entries);
        for (x2apic, expected) in [(true, vec![0, 0x102, 7]), (false, vec![0, 7])] {
            let mut topology = topology(x2apic);
            unsafe { add_madt_processors(&mut topology, bytes.as_ptr() as u64) };
            assert_eq!(topology.cpus[..topology.count].map_ids(), expected);
        }
    }

    #[test]
    fn malformed_madt_entry_terminates_without_looping_or_reading_past_table() {
        for entries in [&[0, 0][..], &[0, 8, 1, 1][..], &[0, 1][..]] {
            let bytes = madt(entries);
            let mut topology = topology(false);
            unsafe { add_madt_processors(&mut topology, bytes.as_ptr() as u64) };
            assert_eq!(topology.cpu_count(), 1);
        }
    }

    #[test]
    fn storage_layout_matches_real_mode_and_limine_abis() {
        assert_eq!(core::mem::size_of::<MpInfo>(), 32);
        assert_eq!(core::mem::size_of::<MpResponse>(), 32);
        assert_eq!(core::mem::offset_of!(MpResponse, flags), 8);
        assert_eq!(core::mem::offset_of!(MpResponse, cpu_count), 16);
        assert_eq!(core::mem::size_of::<Parameters>(), 56);
        assert_eq!(core::mem::offset_of!(Parameters, states), 32);
        assert_eq!(core::mem::offset_of!(Parameters, hhdm), 40);
        assert_eq!(core::mem::offset_of!(Parameters, mtrrs), 48);
        assert_eq!(core::mem::offset_of!(MtrrSnapshot, entries), 8);
        assert_eq!(core::mem::size_of::<MtrrEntry>(), 16);
        assert!(core::mem::size_of::<Storage>() <= STORAGE_PAGES * 4096);
    }
}
