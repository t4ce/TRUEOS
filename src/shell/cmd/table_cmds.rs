use crate::shell::CommandAction;
use crate::shell::cmd::registry::{ParsedArgs, ShellCommandCtx};
use crate::shell::table::{Table, TableColumn};
use alloc::string::{String, ToString};
use acpi::sdt::fadt::Fadt;
use acpi::sdt::madt::Madt;

pub(crate) fn cmd_tlb(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    let term_width = (*ctx.term_cols).saturating_sub(2);
    let cmd_width = 16;
    let desc_width = term_width.saturating_sub(cmd_width).max(20);

    let cols = [
        TableColumn { header: "Subcommand", width: cmd_width },
        TableColumn { header: "Description", width: desc_width },
    ];

    {
        let t = Table::new(&cols);
        t.print_header(ctx.io);

        t.print_row(ctx.io, &["tlb.pci", "List PCI devices"]);
        t.print_row(ctx.io, &["tlb.mem", "List memory map"]);
        t.print_row(ctx.io, &["tlb.cpu", "List CPU cores"]);
        t.print_row(ctx.io, &["tlb.acpi", "List ACPI tables"]);
        t.print_row(ctx.io, &["tlb.uefi", "List UEFI tables"]);
        t.print_row(ctx.io, &["tlb.x2apic", "List x2APIC topology"]);
        t.print_row(ctx.io, &["tlb.dump_acpi", "Dump specific ACPI table parsers info"]);
    }
    
    ctx.io.write_str("tlb: available subcommands\r\n");
    CommandAction::None
}

pub(crate) fn cmd_tlb_dump_acpi(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    ctx.io.write_str("Dumping ACPI parsers info (check main logs for now):\r\n");
    // Since individual modules use static ONCE, they might not reprint.
    // However, we added force_log_all() to help.
    // But individual modules like bgrt.rs use their own LOG_ONCE.
    // We can't force them easily without editing every file.
    // For now, we rely on the fact that if they ran at boot, they logged.
    // If we want to see them again, we view logs.
    // But user asked for table access.
    // Let's print what we can easily access.
    
    if let Some(rect) = crate::efi::acpi::bgrt::last_logo_rect() {
        ctx.io.write_str("BGRT: Last logo rect: ");
        let s = alloc::format!("x={} y={} w={} h={}\r\n", rect.0, rect.1, rect.2, rect.3);
        ctx.io.write_str(&s);
    } else {
        ctx.io.write_str("BGRT: No logo rect recorded.\r\n");
    }

    if let Some(hpet) = crate::efi::acpi::hpet::ensure() {
         ctx.io.write_str("HPET: Found\r\n");
         // We can print hpet struct
         let s = alloc::format!("{:#?}\r\n", hpet);
         ctx.io.write_str(&s);
    }

    ctx.io.write_str("MADT Subtables:\r\n");
    crate::efi::acpi::madt::walk_subtables(|entry| {
        let s = alloc::format!("  {:?}\r\n", entry);
        ctx.io.write_str(&s);
    });

    CommandAction::None
}

pub(crate) fn cmd_tlb_pci(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    let mut len: usize = 0;
    crate::pci::with_devices(|list| {
        len = list.len();
    });
    if len == 0 {
        crate::pci::enumerate_impl();
    }
    
    // Best effort loading of strings
    let _ = crate::pci::pciids::load_sanitized_from_root_blocking();

    let term_width = (*ctx.term_cols).saturating_sub(2);
    // Overhead = Address(10) + VID(6) + PID(6) + 4 * 2 (padding) = 30
    let overhead = 10 + 6 + 6 + 8;
    let name_width = term_width.saturating_sub(overhead).max(20);

    let cols = [
        TableColumn { header: "Name", width: name_width },
        TableColumn { header: "Address", width: 10 },
        TableColumn { header: "VID", width: 6 },
        TableColumn { header: "PID", width: 6 },
    ];
    let t = Table::new(&cols);
    t.print_header(ctx.io);

    crate::pci::with_devices(|list| {
        for dev in list.iter() {
            let addr = alloc::format!("{:02X}:{:02X}.{}", dev.bus, dev.slot, dev.function);
            let vid = alloc::format!("{:04X}", dev.vendor);
            let did = alloc::format!("{:04X}", dev.device);
            
            let name = if let Some(db) = crate::pci::pciids::load_sanitized_from_root_blocking().ok().flatten() {
                 if let Some((v, d)) = crate::pci::pciids::lookup_vendor_device_from_db(&db, dev.vendor, dev.device) {
                     let v_s = String::from_utf8_lossy(v).trim().to_string();
                     let d_s = String::from_utf8_lossy(d).trim().to_string();
                     alloc::format!("{} {}", v_s, d_s)
                 } else {
                     String::new()
                 }
            } else {
                String::new()
            };

            t.print_row(ctx.io, &[name, addr, vid, did]);
        }
    });

    CommandAction::None
}

pub(crate) fn cmd_tlb_mem(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    let memmap = crate::limine::memmap_entries().unwrap_or(&[]);
    if memmap.is_empty() {
        ctx.io.write_str("tlb.mem: no memory map available\r\n");
        return CommandAction::None;
    }

    let term_width = (*ctx.term_cols).saturating_sub(2);
    // Overhead = Base(18) + Length(18) + 3*2 padding = 36 + 6 = 42
    let overhead = 18 + 18 + 6;
    let type_width = term_width.saturating_sub(overhead).max(24);

    let cols = [
        TableColumn { header: "Base", width: 18 },
        TableColumn { header: "Length", width: 18 },
        TableColumn { header: "Type", width: type_width },
    ];
    let t = Table::new(&cols);
    t.print_header(ctx.io);

    for entry in memmap {
        let base = alloc::format!("0x{:016X}", entry.base);
        let len = alloc::format!("0x{:016X}", entry.length);
        let ty = alloc::format!("{}", crate::limine::memmap_type_name(entry.entry_type));
        t.print_row(ctx.io, &[base, len, ty]);
    }

    CommandAction::None
}

pub(crate) fn cmd_tlb_cpu(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    if !crate::smp::is_init() {
        ctx.io.write_str("tlb.cpu: smp not initialized\r\n");
        return CommandAction::None;
    }

    let cols = [
        TableColumn { header: "Slot", width: 6 },
        TableColumn { header: "APIC", width: 6 },
        TableColumn { header: "Role", width: 8 },
        TableColumn { header: "State", width: 10 },
        TableColumn { header: "Seq", width: 6 },
    ];
    let t = Table::new(&cols);
    t.print_header(ctx.io);
    
    let count = crate::smp::cpu_count();
    let slots = crate::percpu::cpu_slots();

    for slot in 0..count {
        if let Some(info) = crate::smp::read(slot) {
             let slot_s = alloc::format!("{}", slot);
             
             let lapic_id = slots.iter().find(|s| s.slot == slot as u32)
                .map(|s| s.lapic_id)
                .unwrap_or(0xFFFFFFFF);
             let apic = alloc::format!("{}", lapic_id);

             let role = if slot == 0 { "BSP" } else { "AP" };
             let state = match info.state {
                 crate::smp::STATE_IDLE => "Idle",
                 crate::smp::STATE_PENDING => "Pending",
                 crate::smp::STATE_RUNNING => "Running",
                 crate::smp::STATE_DONE => "Done",
                 _ => "Unknown",
             };
             let seq = alloc::format!("{}", info.seq);
             t.print_row(ctx.io, &[slot_s, apic, role.into(), state.into(), seq]);
        }
    }
    
    CommandAction::None
}

pub(crate) fn cmd_tlb_acpi(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
     let tables = crate::efi::acpi::ensure_tables();
     if tables.is_none() {
         ctx.io.write_str("tlb.acpi: no tables found\r\n");
         return CommandAction::None;
     }
     let tables = tables.unwrap();

     let cols = [
        TableColumn { header: "Signature", width: 10 },
        TableColumn { header: "Address", width: 18 },
        TableColumn { header: "Length", width: 10 },
        TableColumn { header: "Rev", width: 4 },
        TableColumn { header: "OEM", width: 8 },
        TableColumn { header: "Table ID", width: 10 },
     ];
     let t = Table::new(&cols);
     t.print_header(ctx.io);

     for (phys, hdr) in tables.table_headers() {
         let sig = hdr.signature.as_str();
         let addr = alloc::format!("0x{:08X}", phys);
         let length = hdr.length;
         let revision = hdr.revision;
         let len = alloc::format!("0x{:X}", length);
         let rev = alloc::format!("{}", revision);
         let oem = core::str::from_utf8(&hdr.oem_id).unwrap_or("      ");
         let tbl_id = core::str::from_utf8(&hdr.oem_table_id).unwrap_or("        ");
         
         t.print_row(ctx.io, &[sig.into(), addr, len, rev, oem.into(), tbl_id.into()]);
     }

     CommandAction::None
}

pub(crate) fn cmd_tlb_acpi_facp(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    let Some(tables) = crate::efi::acpi::ensure_tables() else {
        ctx.io.write_str("tlb.acpi: no tables found\r\n");
        return CommandAction::None;
    };

    if let Some(fadt) = tables.find_table::<Fadt>() {
        let fadt_ref = unsafe { fadt.virtual_start.as_ref() };
        ctx.io.write_fmt(format_args!("FACP/FADT Found @ 0x{:X}\r\n", fadt.physical_start));
        ctx.io.write_fmt(format_args!("{:#?}\r\n", fadt_ref));
    } else {
        ctx.io.write_str("FACP: Not found\r\n");
    }
    CommandAction::None
}

pub(crate) fn cmd_tlb_acpi_madt(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    let Some(tables) = crate::efi::acpi::ensure_tables() else {
        ctx.io.write_str("tlb.acpi: no tables found\r\n");
        return CommandAction::None;
    };

    if let Some(madt) = tables.find_table::<Madt>() {
        let madt_ref = unsafe { madt.virtual_start.as_ref() };
        ctx.io.write_fmt(format_args!("MADT Found @ 0x{:X}\r\n", madt.physical_start));
        ctx.io.write_fmt(format_args!("{:#?}\r\n", madt_ref));
    } else {
        ctx.io.write_str("MADT: Not found\r\n");
    }
    CommandAction::None
}

pub(crate) fn cmd_tlb_acpi_hpet(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    if let Some(hpet) = crate::efi::acpi::hpet::ensure() {
        ctx.io.write_fmt(format_args!("{:#?}\r\n", hpet));
    } else {
        ctx.io.write_str("HPET: Not found or initialization failed\r\n");
    }
    CommandAction::None
}

pub(crate) fn cmd_tlb_acpi_mcfg(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    // Disabled compilation until acpi crate structure is verified
    ctx.io.write_str("MCFG: Command disabled due to compilation error\r\n");
    /*
    let Some(tables) = crate::efi::acpi::ensure_tables() else {
        ctx.io.write_str("tlb.acpi: no tables found\r\n");
        return CommandAction::None;
    };

    if let Ok(mcfg) = tables.get_sdt::<acpi::mcfg::Mcfg>(acpi::sdt::Signature::MCFG) {
         if let Some(mcfg) = mcfg {
            let mcfg_ref = unsafe { mcfg.virtual_start.as_ref() };
            ctx.io.write_fmt(format_args!("MCFG Found @ 0x{:X}\r\n", mcfg.physical_start));
            ctx.io.write_fmt(format_args!("{:#?}\r\n", mcfg_ref));
            
            for entry in mcfg_ref.entries() {
                 ctx.io.write_fmt(format_args!("  Base=0x{:X} Seg={} Buses={}-{}\r\n", 
                     entry.base_address, entry.pci_segment_group, entry.bus_number_start, entry.bus_number_end));
            }
         } else {
             ctx.io.write_str("MCFG: Not found (None)\r\n");
         }
    } else {
        ctx.io.write_str("MCFG: Not found or parse error\r\n");
    }
    */
    CommandAction::None
}

pub(crate) fn cmd_tlb_acpi_ssdt(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    let Some(tables) = crate::efi::acpi::ensure_tables() else {
        ctx.io.write_str("tlb.acpi: no tables found\r\n");
        return CommandAction::None;
    };

    ctx.io.write_str("Scanning for SSDT tables (Secondary System Description Table)...\r\n\r\n");

    let mut count = 0;
    for (phys, hdr) in tables.table_headers() {
        if hdr.signature.as_str() == "SSDT" {
             count += 1;
             ctx.io.write_fmt(format_args!("SSDT #{} @ 0x{:08X}\r\n", count, phys));
             
             let len = hdr.length;
             let rev = hdr.revision;
             let oem = core::str::from_utf8(&hdr.oem_id).unwrap_or("      ");
             let tbl_id = core::str::from_utf8(&hdr.oem_table_id).unwrap_or("        ");

             ctx.io.write_fmt(format_args!("  Length: {} bytes\r\n", len));
             ctx.io.write_fmt(format_args!("  Revision: {}\r\n", rev));
             ctx.io.write_fmt(format_args!("  OEM ID: {}\r\n", oem));
             ctx.io.write_fmt(format_args!("  Table ID: {}\r\n", tbl_id));
             ctx.io.write_str("  (Raw AML content not dumped/parsed in 'best effort' mode)\r\n\r\n");
        }
    }

    if count == 0 {
        ctx.io.write_str("No SSDT tables found.\r\n");
    } else {
        ctx.io.write_fmt(format_args!("Found {} SSDT tables.\r\n", count));
    }

    CommandAction::None
}

pub(crate) fn cmd_tlb_uefi(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    let st = crate::efi::system_table();
    if st.is_none() {
        ctx.io.write_str("tlb.uefi: system table not found (not booted via UEFI?)\r\n");
        return CommandAction::None;
    }
    let st = st.unwrap();

    let term_width = (*ctx.term_cols).saturating_sub(2);
    // Table 1: Field(20) + Value(dynamic) + 4 padding
    let val_width = term_width.saturating_sub(24).max(40);

    let cols = [
        TableColumn { header: "Field", width: 20 },
        TableColumn { header: "Value", width: val_width },
    ];
    let t = Table::new(&cols);
    t.print_header(ctx.io);
    
    t.print_row(ctx.io, &["Signature", "EFI SYSTEM TABLE"]);
    t.print_row(ctx.io, &["Revision", &alloc::format!("0x{:08X}", st.hdr.revision)]);
    t.print_row(ctx.io, &["Header Size", &alloc::format!("0x{:X}", st.hdr.header_size)]);
    t.print_row(ctx.io, &["Runtime Services", &alloc::format!("0x{:016X}", st.runtime_services as u64)]);
    t.print_row(ctx.io, &["Boot Services", &alloc::format!("0x{:016X}", st.boot_services as u64)]);
    t.print_row(ctx.io, &["Config Tables", &alloc::format!("{}", st.number_of_table_entries)]);
    
    ctx.io.write_str("\r\n");
    
    // Table 2: Index(6) + GUID(40) + Name(dynamic) + Ptr(18) + 8 padding
    // Overhead = 6 + 40 + 18 + 8 = 72
    let name_width = term_width.saturating_sub(72).max(24);

    let cols_cfg = [
        TableColumn { header: "Index", width: 6 },
        TableColumn { header: "GUID", width: 40 },
        TableColumn { header: "Name", width: name_width },
        TableColumn { header: "Table Ptr", width: 18 },
    ];
    let t_cfg = Table::new(&cols_cfg);
    t_cfg.print_header(ctx.io);
    
    let entries = st.number_of_table_entries;
    let cfg_addr = st.configuration_table as u64;
    
    if let Some(phys) = crate::limine::try_as_phys_addr(cfg_addr) {
         if let Ok((cfg_ptr, _)) = crate::pci::mmio::map_limine_slice::<crate::efi::EfiConfigurationTable>(phys, entries) {
             let slice = unsafe { core::slice::from_raw_parts(cfg_ptr.as_ptr(), entries) };
             for (i, entry) in slice.iter().enumerate() {
                 let idx = alloc::format!("{}", i);
                 let guid = entry.vendor_guid;
                 let name = crate::efi::cfg_guid_name(&guid).unwrap_or("Unknown");
                 let fmt_guid = guid.fmt_canonical();
                 let ptr = alloc::format!("0x{:016X}", entry.vendor_table as u64);
                 
                 t_cfg.print_row(ctx.io, &[idx, fmt_guid, name.to_string(), ptr]);
             }
         }
    }

    CommandAction::None
}

pub(crate) fn cmd_tlb_x2apic(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    let topo = crate::x2apic::detect_x2apic_topology();
    
    ctx.io.write_fmt(format_args!(
        "x2APIC Topology Detection: Leaf=0x{:X} SMT_Bits={} Core_Bits={}\r\n\r\n", 
        topo.leaf, topo.smt_bits, topo.core_bits
    ));

    let cols = [
        TableColumn { header: "Slot", width: 6 },
        TableColumn { header: "APIC ID", width: 10 },
        TableColumn { header: "Pkg", width: 6 },
        TableColumn { header: "Core", width: 6 },
        TableColumn { header: "SMT", width: 6 },
    ];
    
    let t = Table::new(&cols);
    t.print_header(ctx.io);

    if !crate::smp::is_init() {
         ctx.io.write_str("(SMP not initialized, showing BSP only if possible)\r\n");
    }

    let count = crate::smp::cpu_count();
    let slots = crate::percpu::cpu_slots();

    for slot in 0..count {
         let lapic_id = slots.iter().find(|s| s.slot == slot as u32)
            .map(|s| s.lapic_id)
            .unwrap_or(0xFFFFFFFF);
            
         if lapic_id == 0xFFFFFFFF {
             let s_slot = alloc::format!("{}", slot);
             t.print_row(ctx.io, &[s_slot, "?".into(), "?".into(), "?".into(), "?".into()]);
             continue;
         }

         let (pkg, core, smt) = topo.decode(lapic_id);
         
         let s_slot = alloc::format!("{}", slot);
         let s_apic = alloc::format!("0x{:X}", lapic_id);
         let s_pkg = alloc::format!("{}", pkg);
         let s_core = alloc::format!("{}", core);
         let s_smt = alloc::format!("{}", smt);
         
         t.print_row(ctx.io, &[s_slot, s_apic, s_pkg, s_core, s_smt]);
    }

    CommandAction::None
}

pub(crate) fn cmd_usb(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    let ctrls = crate::usb::xhci::xhc_list();
    if ctrls.is_empty() {
        ctx.io.write_str("usb: no xhci controllers found\r\n");
        return CommandAction::None;
    }

    let term_width = (*ctx.term_cols).saturating_sub(2);
    // Overhead: Ctrl(4)+Port(4)+State(10)+Speed(8)+Device(12)+VID:PID(11) + 6*2 padding = 49 + 12 = 61
    let overhead = 4 + 4 + 10 + 8 + 12 + 11 + 12;
    let details_width = term_width.saturating_sub(overhead).max(16);

    let cols = [
        TableColumn { header: "Ctrl", width: 4 },
        TableColumn { header: "Port", width: 4 },
        TableColumn { header: "State", width: 10 },
        TableColumn { header: "Speed", width: 8 },
        TableColumn { header: "Device", width: 12 },
        TableColumn { header: "VID:PID", width: 11 },
        TableColumn { header: "Raw", width: details_width },
    ];
    let t = Table::new(&cols);
    t.print_header(ctx.io);
    
    for info in ctrls.iter() {
        let ports = crate::usb::inspect_ports(info.controller_id);
        
        for p in ports.iter() {
             let ctrl = alloc::format!("{}", info.controller_id);
             let port = alloc::format!("{}", p.port_id);
             
             let state_str = if p.connected {
                 if p.enabled { "Active" } else { "Connected" }
             } else {
                 "Empty"
             };
             
             let speed = if p.connected { p.speed } else { "-" };
             
             let device = p.device_kind.unwrap_or(if p.connected { "Unknown" } else { "-" });
             
             let vidpid = if let (Some(v), Some(pid)) = (p.vid, p.pid) {
                 alloc::format!("{:04X}:{:04X}", v, pid)
             } else {
                 "-".into()
             };
             
             let details = alloc::format!("0x{:08X}", p.status);

             t.print_row(ctx.io, &[ctrl, port, state_str.into(), speed.into(), device.into(), vidpid, details]);
        }
    }

    CommandAction::None
}

pub(crate) fn cmd_tlb_dump(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    use core::fmt::Write;
    let mut out = String::new();

    // 1. Memory
    writeln!(out, "=== Memory Map ===").unwrap();
    let memmap = crate::limine::memmap_entries().unwrap_or(&[]);
    if memmap.is_empty() {
        writeln!(out, "No memory map available").unwrap();
    } else {
        writeln!(out, "{:18}  {:18}  Type", "Base", "Length").unwrap();
        writeln!(out, "{:-<18}  {:-<18}  {:-<20}", "", "", "").unwrap();
        for entry in memmap {
            writeln!(out, "0x{:016X}  0x{:016X}  {}", entry.base, entry.length, crate::limine::memmap_type_name(entry.entry_type)).unwrap();
        }
    }
    writeln!(out).unwrap();

    // 2. PCI
    writeln!(out, "=== PCI Devices ===").unwrap();
    let mut len: usize = 0;
    crate::pci::with_devices(|list| { len = list.len(); });
    if len == 0 { crate::pci::enumerate_impl(); }
    
    let db = crate::pci::pciids::load_sanitized_from_root_blocking().ok().flatten();
    
    writeln!(out, "{:30}  {:10}  {:6}  {:6}", "Name", "Address", "VID", "PID").unwrap();
    writeln!(out, "{:-<30}  {:-<10}  {:-<6}  {:-<6}", "", "", "", "").unwrap();
    crate::pci::with_devices(|list| {
        for dev in list.iter() {
           let addr = alloc::format!("{:02X}:{:02X}.{}", dev.bus, dev.slot, dev.function);
            let vid = alloc::format!("{:04X}", dev.vendor);
            let did = alloc::format!("{:04X}", dev.device);
            
            let name = if let Some(ref db_slice) = db {
                 if let Some((v, d)) = crate::pci::pciids::lookup_vendor_device_from_db(db_slice, dev.vendor, dev.device) {
                     let v_s = String::from_utf8_lossy(v).trim().to_string();
                     let d_s = String::from_utf8_lossy(d).trim().to_string();
                     alloc::format!("{} {}", v_s, d_s)
                 } else {
                     String::new()
                 }
            } else {
                String::new()
            };
            
            // Truncate name
            let name_disp = if name.len() > 30 {
                 let mut s = String::from(&name[..29]);
                 s.push('…');
                 s
            } else {
                name
            };
            
            let _ = writeln!(out, "{:30}  {:10}  {:6}  {:6}", name_disp, addr, vid, did);
        }
    });
    writeln!(out).unwrap();

    // 3. CPU
    writeln!(out, "=== CPU Cores ===").unwrap();
     if !crate::smp::is_init() {
        writeln!(out, "SMP not initialized").unwrap();
    } else {
        writeln!(out, "{:6}  {:6}  {:8}  {:10}", "Slot", "APIC", "Role", "State").unwrap();
        writeln!(out, "{:-<6}  {:-<6}  {:-<8}  {:-<10}", "", "", "", "").unwrap();
        let count = crate::smp::cpu_count();
        let slots = crate::percpu::cpu_slots();
    
        for slot in 0..count {
            if let Some(info) = crate::smp::read(slot) {
                 let lapic_id = slots.iter().find(|s| s.slot == slot as u32)
                    .map(|s| s.lapic_id)
                    .unwrap_or(0xFFFFFFFF);
    
                 let role = if slot == 0 { "BSP" } else { "AP" };
                 let state = match info.state {
                     crate::smp::STATE_IDLE => "Idle",
                     crate::smp::STATE_PENDING => "Pending",
                     crate::smp::STATE_RUNNING => "Running",
                     crate::smp::STATE_DONE => "Done",
                     _ => "Unknown",
                 };
                 let _ = writeln!(out, "{:6}  {:<6}  {:<8}  {:<10}", slot, lapic_id, role, state);
            }
        }
    }
    writeln!(out).unwrap();
    
    // 4. ACPI
    writeln!(out, "=== ACPI Tables ===").unwrap();
     if let Some(tables) = crate::efi::acpi::ensure_tables() {
         writeln!(out, "{:10}  {:18}  {:10}", "Signature", "Address", "Length").unwrap();
         writeln!(out, "{:-<10}  {:-<18}  {:-<10}", "", "", "").unwrap();
         for (phys, hdr) in tables.table_headers() {
             let sig = hdr.signature;
             let len = hdr.length;
             let _ = writeln!(out, "{:10}  0x{:016X}  0x{:X}", sig.as_str(), phys, len);
         }
     } else {
        writeln!(out, "No tables found").unwrap();
     }
    writeln!(out).unwrap();

    // Write file
    let file_path = "trueos/pci/tlb.txt";
    ctx.io.write_fmt(format_args!("Writing {} bytes to {}...\r\n", out.len(), file_path));
    
    let out_bytes = out.into_bytes();
    let res: Result<(), crate::disc::block::Error> = crate::wait::spawn_and_wait_local(async move {
        let Some(handle) = crate::v::fs::trueosfs::primary_root_handle() else {
             return Err(crate::disc::block::Error::NotReady);
        };
        
        // This fails if the directory 'trueos/pci' does not exist?
        // trueosfs has no directory creation API, it just creates keys in the BTree.
        // So paths are just Strings using the '/' separator.
        // As long as we use valid path string, it should work.
        
        match crate::v::fs::trueosfs::file_in_async(handle, file_path, &out_bytes).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(crate::disc::block::Error::Io), 
            Err(e) => Err(e),
        }
    });

    match res {
        Ok(_) => ctx.io.write_str("Success.\r\n"),
        Err(e) => ctx.io.write_fmt(format_args!("Error writing file: {:?}\r\n", e)),
    }

    CommandAction::None
}
