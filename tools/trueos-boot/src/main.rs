#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![allow(unsafe_op_in_unsafe_fn)]

extern crate alloc;

mod elf;
mod handoff;
mod network;
mod paging;
mod smp;
mod time;

use alloc::vec::Vec;
use core::{alloc::{GlobalAlloc, Layout}, ffi::c_void, fmt::{self, Write}, mem, ptr};
use core::sync::atomic::{AtomicPtr, Ordering};
use font8x8::{UnicodeFonts, BASIC_FONTS};
use r_efi::{base::{Guid, Handle, Status}, protocols::{file, graphics_output as gop, loaded_image, simple_file_system}, system::{self, BootServices, SystemTable}};
use handoff::{Blob, BootFacts, Framebuffer, Handoff, MemoryRegion};

const PAGE: usize = 4096;
const STACK_BYTES: usize = 16 * 1024 * 1024;
const MAP_BYTES: usize = 512 * 1024;
const MAX_MAP_ENTRIES: usize = MAP_BYTES / 40;
static BOOT_SERVICES: AtomicPtr<BootServices> = AtomicPtr::new(ptr::null_mut());
static mut SYSTEM_TABLE: *mut SystemTable = ptr::null_mut();
static mut DISPLAY: Option<Framebuffer> = None;
static mut SCREEN_ROW: usize = 0;
static mut SCREEN_COLUMN: usize = 0;

struct FirmwareAllocator;
#[global_allocator]
static ALLOCATOR: FirmwareAllocator = FirmwareAllocator;

unsafe impl GlobalAlloc for FirmwareAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let bs = BOOT_SERVICES.load(Ordering::Acquire);
        if bs.is_null() { return ptr::null_mut(); }
        let alignment = layout.align().max(mem::align_of::<usize>());
        let Some(size) = layout.size().checked_add(alignment + mem::size_of::<usize>()) else { return ptr::null_mut(); };
        let mut raw = ptr::null_mut();
        if ((*bs).allocate_pool)(system::LOADER_DATA, size, &mut raw).is_error() { return ptr::null_mut(); }
        let aligned = (raw as usize + mem::size_of::<usize>() + alignment - 1) & !(alignment - 1);
        ((aligned - mem::size_of::<usize>()) as *mut usize).write(raw as usize);
        aligned as *mut u8
    }
    unsafe fn dealloc(&self, allocation: *mut u8, _: Layout) {
        let bs = BOOT_SERVICES.load(Ordering::Acquire);
        if !bs.is_null() {
            let raw = allocation.sub(mem::size_of::<usize>()).cast::<usize>().read();
            ((*bs).free_pool)(raw as *mut c_void);
        }
    }
}

struct Console;
impl Write for Console {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        unsafe {
            for byte in value.bytes() {
                // Bounded polling keeps absent serial hardware from blocking USB boot.
                for _ in 0..1000 {
                    let ready: u8;
                    core::arch::asm!("in al, dx", in("dx") 0x3fdu16, out("al") ready, options(nomem, nostack));
                    if ready & 0x20 != 0 { break; }
                }
                core::arch::asm!("out dx, al", in("dx") 0x3f8u16, in("al") byte, options(nomem, nostack));
            }
            if let Some(fb) = DISPLAY {
                let color = (0x90u32 << fb.red_mask_shift) | (0xffu32 << fb.green_mask_shift) | (0xb0u32 << fb.blue_mask_shift);
                for ch in value.chars() {
                    if ch == '\n' { SCREEN_ROW += 1; SCREEN_COLUMN = 0; continue; }
                    if 16 + (SCREEN_COLUMN + 1) * 8 > fb.width as usize {
                        SCREEN_ROW += 1;
                        SCREEN_COLUMN = 0;
                    }
                    if let Some(glyph) = BASIC_FONTS.get(ch) {
                        for (y, bits) in glyph.iter().enumerate() {
                            for x in 0..8 {
                                let px = 16 + SCREEN_COLUMN * 8 + x;
                                let py = 16 + SCREEN_ROW * 10 + y;
                                if px < fb.width as usize && py < fb.height as usize {
                                    let address = fb.phys as usize + py * fb.pitch as usize + px * 4;
                                    (address as *mut u32).write_volatile(if bits & (1 << x) != 0 { color } else { 0 });
                                }
                            }
                        }
                    }
                    SCREEN_COLUMN += 1;
                }
            } else if !BOOT_SERVICES.load(Ordering::Acquire).is_null() && !SYSTEM_TABLE.is_null() {
                let out = (*SYSTEM_TABLE).con_out;
                if !out.is_null() {
                    for byte in value.bytes() {
                        let text = [u16::from(byte), 0];
                        ((*out).output_string)(out, text.as_ptr() as *mut u16);
                    }
                }
            }
        }
        Ok(())
    }
}
macro_rules! log { ($($arg:tt)*) => {{ let _ = writeln!(Console, $($arg)*); }}; }

unsafe fn initialize_serial() {
    for (port, byte) in [(0x3f9u16,0u8),(0x3fb,0x80),(0x3f8,1),(0x3f9,0),(0x3fb,3),(0x3fa,0xc7),(0x3fc,0x0b)] {
        core::arch::asm!("out dx, al", in("dx") port, in("al") byte, options(nomem, nostack));
    }
}

fn stop(reason: &str) -> ! {
    log!("TRUEOS BOOT STOP: {reason}");
    loop { core::hint::spin_loop(); }
}
fn require<T>(result: Result<T, &'static str>) -> T {
    match result { Ok(value) => value, Err(reason) => stop(reason) }
}

unsafe fn pages(count: usize, maximum: u64) -> Result<u64, &'static str> {
    if count == 0 { return Err("zero-page allocation"); }
    let bs = BOOT_SERVICES.load(Ordering::Acquire);
    let mut address = maximum;
    if ((*bs).allocate_pages)(system::ALLOCATE_MAX_ADDRESS, system::LOADER_DATA, count, &mut address).is_error() {
        return Err("firmware could not reserve loader memory");
    }
    ptr::write_bytes(address as *mut u8, 0, count * PAGE);
    Ok(address)
}

unsafe fn protocol<T>(handle: Handle, mut guid: Guid) -> Result<*mut T, &'static str> {
    let bs = BOOT_SERVICES.load(Ordering::Acquire);
    let mut raw = ptr::null_mut();
    if ((*bs).handle_protocol)(handle, &mut guid, &mut raw).is_error() || raw.is_null() {
        return Err("UEFI device protocol unavailable");
    }
    Ok(raw.cast())
}

unsafe fn read_file(root: *mut file::Protocol, name: &str) -> Result<&'static [u8], &'static str> {
    let mut path: Vec<u16> = name.encode_utf16().map(|ch| if ch == b'/' as u16 { b'\\' as u16 } else { ch }).collect();
    path.push(0);
    let mut handle: *mut file::Protocol = ptr::null_mut();
    if ((*root).open)(root, &mut handle, path.as_mut_ptr(), file::MODE_READ, 0).is_error() {
        return Err("boot file was not found on the boot volume");
    }
    let guid = Guid::from_fields(0x09576e92, 0x6d3f, 0x11d2, 0x8e, 0x39, &[0x00,0xa0,0xc9,0x69,0x72,0x3b]);
    let mut info = [0u64; 128];
    let mut info_size = mem::size_of_val(&info);
    let status = ((*handle).get_info)(handle, &guid as *const _ as *mut _, &mut info_size, info.as_mut_ptr().cast());
    if status.is_error() { ((*handle).close)(handle); return Err("could not read boot file metadata"); }
    let length = info[1] as usize;
    if length == 0 || length > 1024 * 1024 * 1024 {
        ((*handle).close)(handle); return Err("boot file size outside supported range");
    }
    let allocation = match pages(length.div_ceil(PAGE), u32::MAX as u64) {
        Ok(p) => p, Err(e) => { ((*handle).close)(handle); return Err(e); }
    };
    let mut offset = 0usize;
    while offset < length {
        let mut amount = length - offset;
        let status = ((*handle).read)(handle, &mut amount, (allocation as *mut u8).add(offset).cast());
        if status.is_error() || amount == 0 || amount > length - offset {
            ((*handle).close)(handle); return Err("short boot file read");
        }
        offset += amount;
    }
    ((*handle).close)(handle);
    Ok(core::slice::from_raw_parts(allocation as *const u8, length))
}

#[derive(Debug)]
enum FileError {
    Volume(&'static str),
    Network(network::Error),
}

/// Both transports leave file bytes in retained LoaderData pages. Everything
/// after downloading uses the same ELF parser, mappings and kernel handoff.
enum BootFiles {
    Volume(*mut file::Protocol),
    Network(network::Network),
}

impl BootFiles {
    unsafe fn open(device: Handle, bs: *mut BootServices) -> Self {
        if let Ok(fs) = protocol::<simple_file_system::Protocol>(device, simple_file_system::PROTOCOL_GUID) {
            let mut root = ptr::null_mut();
            if ((*fs).open_volume)(fs, &mut root).is_error() || root.is_null() {
                stop("could not open the boot volume");
            }
            log!("Boot source: FAT volume");
            return Self::Volume(root);
        }
        match network::Network::open(device, bs) {
            Ok(network) => {
                let [a, b, c, d] = network.server_ipv4();
                log!("Boot source: PXE/TFTP {a}.{b}.{c}.{d}");
                Self::Network(network)
            }
            Err(error) => {
                log!("PXE initialization: {error:?}");
                stop("boot device has no usable FAT volume or IPv4 PXE session");
            }
        }
    }

    unsafe fn read(&self, path: &str) -> Result<&'static [u8], FileError> {
        log!("Loading {path}");
        let bytes = match self {
            Self::Volume(root) => read_file(*root, path).map_err(FileError::Volume),
            Self::Network(network) => network.read(path, |length| pages(length.div_ceil(PAGE), u32::MAX as u64)).map_err(FileError::Network),
        }?;
        log!("Loaded {} bytes", bytes.len());
        Ok(bytes)
    }

    fn server_ipv4(&self) -> Option<[u8; 4]> {
        match self {
            Self::Volume(_) => None,
            Self::Network(network) => Some(network.server_ipv4()),
        }
    }

    unsafe fn close(self) {
        if let Self::Volume(root) = self { ((*root).close)(root); }
    }
}

unsafe fn framebuffer() -> Option<Framebuffer> {
    let bs = BOOT_SERVICES.load(Ordering::Acquire);
    let mut guid = gop::PROTOCOL_GUID;
    let mut raw = ptr::null_mut();
    if ((*bs).locate_protocol)(&mut guid, ptr::null_mut(), &mut raw).is_error() || raw.is_null() { return None; }
    let protocol = raw.cast::<gop::Protocol>();
    let mode = (*protocol).mode;
    if mode.is_null() || (*mode).info.is_null() { return None; }
    let info = &*(*mode).info;
    let (red, green, blue) = match info.pixel_format {
        gop::PIXEL_RED_GREEN_BLUE_RESERVED_8_BIT_PER_COLOR => (0, 8, 16),
        gop::PIXEL_BLUE_GREEN_RED_RESERVED_8_BIT_PER_COLOR => (16, 8, 0),
        _ => return None,
    };
    let needed = (info.pixels_per_scan_line as usize).checked_mul(info.vertical_resolution as usize)?.checked_mul(4)?;
    if needed > (*mode).frame_buffer_size || info.horizontal_resolution > info.pixels_per_scan_line { return None; }
    Some(Framebuffer { phys: (*mode).frame_buffer_base, width: info.horizontal_resolution as u64,
        height: info.vertical_resolution as u64, pitch: info.pixels_per_scan_line as u64 * 4,
        bpp: 32, red_mask_size: 8, red_mask_shift: red,
        green_mask_size: 8, green_mask_shift: green, blue_mask_size: 8, blue_mask_shift: blue })
}

unsafe fn configuration(st: *mut SystemTable, guid: Guid) -> u64 {
    for index in 0..(*st).number_of_table_entries {
        let entry = &*(*st).configuration_table.add(index);
        if entry.vendor_guid == guid { return entry.vendor_table as u64; }
    }
    0
}

unsafe fn timestamp(st: *mut SystemTable) -> Option<i64> {
    let runtime = (*st).runtime_services;
    if runtime.is_null() { return None; }
    let mut t = system::Time::default();
    if ((*runtime).get_time)(&mut t, ptr::null_mut()).is_error() { return None; }
    time::unix_seconds(t.year, t.month, t.day, t.hour, t.minute, t.second)
}

unsafe fn single_cpu(handoff: &mut Handoff, topology: &smp::Topology) -> Result<u64, handoff::Error> {
    let info = handoff.allocate_bytes(mem::size_of::<smp::MpInfo>(), mem::align_of::<smp::MpInfo>())?;
    (info.physical as *mut smp::MpInfo).write(smp::MpInfo {
        processor_id: topology.bsp_lapic_id(), lapic_id: topology.bsp_lapic_id(), reserved: 0,
        goto_addr: core::sync::atomic::AtomicU64::new(0), extra_argument: core::sync::atomic::AtomicU64::new(0),
    });
    let pointers = handoff.store([info.virtual_address])?;
    let response = handoff.allocate_bytes(mem::size_of::<smp::MpResponse>(), mem::align_of::<smp::MpResponse>())?;
    response.pointer.cast::<smp::MpResponse>().write(smp::MpResponse {
        revision: 0, flags: u32::from(topology.x2apic()), bsp_lapic_id: topology.bsp_lapic_id(),
        cpu_count: 1, cpus: pointers.virtual_address,
    });
    Ok(response.virtual_address)
}

unsafe fn get_map(bs: *mut BootServices, buffer: u64) -> Result<(usize, usize, usize), &'static str> {
    let mut size = MAP_BYTES;
    let mut key = 0usize;
    let mut stride = 0usize;
    let mut version = 0u32;
    if ((*bs).get_memory_map)(&mut size, buffer as *mut _, &mut key, &mut stride, &mut version).is_error()
        || stride < mem::size_of::<system::MemoryDescriptor>() || size % stride != 0
        || size / stride > MAX_MAP_ENTRIES
    { return Err("UEFI memory map exceeds loader capacity"); }
    Ok((size / stride, stride, key))
}

#[repr(C, packed)]
struct Gdtr { length: u16, address: u64 }
static mut GDT: [u64; 5] = [0, 0x00af_9a00_0000_ffff, 0x00cf_9200_0000_ffff, 0, 0];
static mut TSS: [u8; 104] = [0; 104];

unsafe fn activate_tables(root: u64) {
    // Replace the inherited firmware TR as well as the GDT. VMX later reads
    // the active TSS descriptor, so it must remain valid in the kernel's map.
    let tss = paging::HHDM + ptr::addr_of!(TSS) as u64;
    ptr::addr_of_mut!(TSS).cast::<u8>().add(102).cast::<u16>().write_unaligned(104);
    let descriptor = 103 | ((tss & 0xffff) << 16) | (((tss >> 16) & 0xff) << 32)
        | (0x89 << 40) | (((tss >> 24) & 0xff) << 56);
    ptr::addr_of_mut!(GDT).cast::<u64>().add(3).write(descriptor);
    ptr::addr_of_mut!(GDT).cast::<u64>().add(4).write(tss >> 32);
    let gdtr = Gdtr { length: 39, address: paging::HHDM + ptr::addr_of!(GDT) as u64 };
    let cr0: u64;
    core::arch::asm!("cli", "mov {}, cr0", out(reg) cr0);
    core::arch::asm!("mov cr0, {}", "wbinvd", in(reg) (cr0 | (1 << 30)) & !(1 << 29));
    core::arch::asm!("wrmsr", in("ecx") 0x277u32,
        in("eax") smp::HANDOFF_PAT as u32, in("edx") (smp::HANDOFF_PAT >> 32) as u32);
    let mut cr4: u64;
    core::arch::asm!("mov {}, cr4", out(reg) cr4);
    cr4 &= !(1 << 7);
    core::arch::asm!("cli", "mov cr4, {}", in(reg) cr4);
    core::arch::asm!(
        "cld", "mov cr3, {root}", "lgdt [{gdt}]", "push 8", "lea rax, [rip + 2f]", "push rax", "retfq", "2:",
        "mov ax, 16", "mov ds, ax", "mov es, ax", "mov ss, ax", "xor eax, eax", "mov fs, ax", "mov gs, ax",
        "mov ax, 24", "ltr ax",
        gdt = in(reg) &gdtr, root = in(reg) root, out("rax") _,
    );
    cr4 &= !(1 << 17);
    core::arch::asm!("mov cr4, {}", in(reg) cr4);
    core::arch::asm!("wbinvd", "mov cr0, {}", in(reg) cr0 | (1 << 16));
    for msr in [0xc0000100u32, 0xc0000101, 0xc0000102] {
        core::arch::asm!("wrmsr", in("ecx") msr, in("eax") 0u32, in("edx") 0u32);
    }
    // Both the kernel and AP handoff may use non-executable mappings.
    let mut low: u32;
    let high: u32;
    core::arch::asm!("rdmsr", in("ecx") 0xc0000080u32, out("eax") low, out("edx") high);
    if core::arch::x86_64::__cpuid(0x80000000).eax >= 0x80000001
        && core::arch::x86_64::__cpuid(0x80000001).edx & (1 << 20) != 0 { low |= 1 << 11; }
    core::arch::asm!("wrmsr", in("ecx") 0xc0000080u32, in("eax") low, in("edx") high);
    core::arch::asm!("out dx, al", in("dx") 0x21u16, in("al") 0xffu8);
    core::arch::asm!("out dx, al", in("dx") 0xa1u16, in("al") 0xffu8);
}

#[unsafe(no_mangle)]
pub unsafe extern "efiapi" fn efi_main(image_handle: Handle, st: *mut SystemTable) -> Status {
    SYSTEM_TABLE = st;
    let bs = (*st).boot_services;
    BOOT_SERVICES.store(bs, Ordering::Release);
    initialize_serial();
    ((*bs).set_watchdog_timer)(0, 0, 0, ptr::null_mut());
    DISPLAY = framebuffer();
    log!("TRUEOS native UEFI loader v0.1");
    let cr4: u64;
    core::arch::asm!("mov {}, cr4", out(reg) cr4);
    if cr4 & (1 << 12) != 0 { stop("five-level firmware paging is unsupported"); }
    let loaded = require(protocol::<loaded_image::Protocol>(image_handle, loaded_image::PROTOCOL_GUID));
    let files = BootFiles::open((*loaded).device_handle, bs);
    let tftp_server = files.server_ipv4();
    let kernel = match files.read("TRUEOS.elf") {
        Ok(bytes) => bytes,
        Err(error) => {
            log!("Kernel download/read: {error:?}");
            stop("could not load TRUEOS.elf from the boot device");
        }
    };
    let elf = require(elf::parse(kernel));
    log!("ELF loaded: {} bytes; mapped image {} bytes", kernel.len(), elf.size);
    let kernel_phys = require(pages(elf.size.div_ceil(PAGE), u32::MAX as u64));
    for segment in &elf.segments {
        ptr::copy_nonoverlapping(kernel.as_ptr().add(segment.file_offset),
            (kernel_phys + segment.virtual_address - elf.base) as *mut u8, segment.file_size);
    }
    let mut modules = Vec::new();
    match files.read("EFI/BOOT/tgl_guc_70.bin") {
        Ok(bytes) => modules.push(Blob { phys: bytes.as_ptr() as u64, len: bytes.len(),
            path: "/EFI/BOOT/tgl_guc_70.bin", cmdline: "trueos.fw.guc" }),
        Err(error) => log!("GuC firmware unavailable ({error:?}): Intel GPU submission may be unavailable"),
    }
    match files.read("EFI/BOOT/BOOTX64.EFI") {
        Ok(bytes) => modules.push(Blob { phys: bytes.as_ptr() as u64, len: bytes.len(),
            path: "/EFI/BOOT/BOOTX64.EFI", cmdline: "trueos.install.bootx64" }),
        Err(error) => log!("Installer EFI module unavailable: {error:?}"),
    }
    files.close();
    let mut rsdp = configuration(st, system::ACPI_20_TABLE_GUID);
    if rsdp == 0 { rsdp = configuration(st, system::ACPI_10_TABLE_GUID); }
    if rsdp == 0 { stop("firmware did not provide ACPI RSDP"); }
    let topology = smp::discover(rsdp);
    log!("ACPI CPU threads: {}; BSP APIC {}; x2APIC {}", topology.cpu_count(), topology.bsp_lapic_id(), topology.x2apic());
    let map_buffer = require(pages(MAP_BYTES / PAGE, u32::MAX as u64));
    let tables_phys = require(pages(paging::ARENA_PAGES, u32::MAX as u64));
    let stack = require(pages(STACK_BYTES / PAGE, u32::MAX as u64));
    let arena = require(pages(512, u32::MAX as u64));
    let mut handoff = match Handoff::new(arena, 512 * PAGE, paging::HHDM, MAX_MAP_ENTRIES * 2) {
        Ok(h) => h, Err(_) => stop("could not prepare boot handoff arena"),
    };
    let mp = match (pages(1, 0xff000), pages(smp::STORAGE_PAGES, u32::MAX as u64),
        pages(topology.cpu_count() * smp::STACK_PAGES_PER_CPU, 0xffff_efff)) {
        (Ok(trampoline), Ok(storage), Ok(stacks)) => match smp::prepare(&topology, trampoline, storage, stacks, tables_phys, paging::HHDM) {
            Ok(prepared) => Some(prepared), Err(e) => { log!("SMP preparation: {e}; using BSP"); None }
        },
        _ => { log!("SMP memory unavailable; using BSP"); None }
    };
    let facts = BootFacts { kernel: Blob { phys: kernel.as_ptr() as u64, len: kernel.len(), path: "/TRUEOS.elf",
            cmdline: "timezone=Europe/Berlin keyboard=de trueos.boot=native-uefi" },
        modules: &modules, tftp_server, kernel_physical_base: kernel_phys, kernel_virtual_base: elf.base,
        framebuffer: DISPLAY, rsdp, efi_system_table: st as u64,
        smbios32: configuration(st, system::SMBIOS_TABLE_GUID), smbios64: configuration(st, system::SMBIOS3_TABLE_GUID),
        timestamp: timestamp(st), stack_size: STACK_BYTES as u64,
        mp_response: mp.as_ref().map_or(0, |m| paging::HHDM + m.response_phys()),
    };
    if handoff.prepare(&facts).is_err() { stop("could not encode kernel boot responses"); }
    if mp.is_none() {
        let response = match single_cpu(&mut handoff, &topology) {
            Ok(response) => response, Err(_) => stop("could not encode BSP identity"),
        };
        handoff.set_mp_response(response);
    }
    let mut regions: Vec<MemoryRegion> = Vec::with_capacity(MAX_MAP_ENTRIES);
    let mut tables = paging::Tables::new(tables_phys);
    let (count, stride, _) = require(get_map(bs, map_buffer));
    for index in 0..count {
        let d = ptr::read_unaligned((map_buffer as *const u8).add(index * stride).cast::<system::MemoryDescriptor>());
        // Map RAM, firmware and ordinary MMIO descriptors, without expanding
        // enormous device address windows that the kernel maps on demand.
        if d.r#type != system::MEMORY_MAPPED_IO && d.r#type != system::MEMORY_MAPPED_IO_PORT_SPACE {
            require(tables.physical_range(d.physical_start, d.number_of_pages * PAGE as u64));
        }
    }
    require(tables.physical_range(0, 0x10_0000));
    require(tables.uncached_physical_range(0xfee0_0000, PAGE as u64));
    if let Some(fb) = DISPLAY { require(tables.physical_range(fb.phys, fb.pitch * fb.height)); }
    require(tables.kernel(elf.base, kernel_phys, elf.size));
    let requests = (kernel_phys + elf.requests - elf.base) as *mut u8;
    if handoff.patch_requests(requests, elf.requests_size).is_err() { stop("kernel boot request contract is unsupported"); }
    log!("Boot responses ready; leaving firmware services");
    // After the first ExitBootServices attempt, use only GetMemoryMap and
    // ExitBootServices. In particular, never print through firmware or allocate.
    BOOT_SERVICES.store(ptr::null_mut(), Ordering::Release);
    let mut exited = false;
    for _ in 0..3 {
        let (count, stride, key) = require(get_map(bs, map_buffer));
        regions.clear();
        for index in 0..count {
            let d = ptr::read_unaligned((map_buffer as *const u8).add(index * stride).cast::<system::MemoryDescriptor>());
            let region = match MemoryRegion::from_uefi(d.r#type, d.physical_start, d.number_of_pages, d.attribute) {
                Ok(r) => r, Err(_) => stop("invalid firmware memory descriptor"),
            };
            regions.push(region);
        }
        if handoff.finalize_memory_map(&regions, &[]).is_err() { stop("could not encode final memory map"); }
        let status = ((*bs).exit_boot_services)(image_handle, key);
        if !status.is_error() { exited = true; break; }
        if status != Status::INVALID_PARAMETER { break; }
    }
    if !exited { BOOT_SERVICES.store(ptr::null_mut(), Ordering::Release); stop("ExitBootServices failed"); }
    BOOT_SERVICES.store(ptr::null_mut(), Ordering::Release);
    activate_tables(tables.root());
    log!("Firmware exited; initial page tables active");
    if let Some(mut mp) = mp {
        log!("Starting secondary CPU threads");
        let summary = mp.start_aps(|id, ready| log!("CPU APIC {}: {}", id, if ready {"ready"} else {"failed"}));
        log!("CPU threads ready: {}; failed: {}", summary.started + 1, summary.failed);
    }
    log!("Entering TrueOS kernel");
    core::arch::asm!("cli", "cld", "mov rsp, rdx", "xor rbp, rbp", "jmp rax",
        in("rdx") stack + STACK_BYTES as u64 - 16, in("rax") elf.entry, options(noreturn));
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    log!("UEFI loader panic: {info}");
    loop { core::hint::spin_loop(); }
}

#[alloc_error_handler]
fn allocation_failed(_: Layout) -> ! { stop("loader allocation failed") }
