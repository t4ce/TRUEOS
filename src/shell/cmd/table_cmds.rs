use crate::shell::{ShellIo, CommandAction};
use crate::shell::cmd::registry::{ParsedArgs, ShellCommandCtx};
use crate::shell::table::{Table, TableColumn};
use alloc::string::{String, ToString};

pub(crate) fn cmd_tlb(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> CommandAction {
    ctx.io.write_str("tlb: available subcommands\r\n");
    ctx.io.write_str("  tlb.pci         - List PCI devices\r\n");
    ctx.io.write_str("  tlb.mem         - List memory map\r\n");
    ctx.io.write_str("  tlb.cpu         - List CPU cores\r\n");
    ctx.io.write_str("  tlb.acpi        - List ACPI tables\r\n");
    ctx.io.write_str("  tlb.uefi        - List UEFI tables\r\n");
    ctx.io.write_str("  tlb.dump_acpi   - Dump specific ACPI table parsers info\r\n");
    CommandAction::None
}

pub(crate) fn cmd_tlb_dump_acpi(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> CommandAction {
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

pub(crate) fn cmd_tlb_pci(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> CommandAction {
    let mut len: usize = 0;
    crate::pci::with_devices(|list| {
        len = list.len();
    });
    if len == 0 {
        crate::pci::enumerate_silent();
    }
    
    // Best effort loading of strings
    let _ = crate::pci::pciids::load_sanitized_from_root_blocking();

    let cols = [
        TableColumn { header: "Address", width: 10 },
        TableColumn { header: "Vendor", width: 6 },
        TableColumn { header: "Device", width: 6 },
        TableColumn { header: "Class", width: 8 },
        TableColumn { header: "Subsys", width: 11 },
        TableColumn { header: "Name", width: 40 },
    ];
    let t = Table::new(&cols);
    t.print_header(ctx.io);

    crate::pci::with_devices(|list| {
        for dev in list.iter() {
            let addr = alloc::format!("{:02X}:{:02X}.{}", dev.bus, dev.slot, dev.function);
            let vid = alloc::format!("{:04X}", dev.vendor);
            let did = alloc::format!("{:04X}", dev.device);
            let subsys_vid = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x2C);
            let subsys_did = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x2E);
            let subsys = alloc::format!("{:04X}:{:04X}", subsys_vid, subsys_did);
            let cls = alloc::format!("{:02X}/{:02X}/{:02X}", dev.class, dev.subclass, dev.prog_if);
            
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

            t.print_row(ctx.io, &[addr, vid, did, cls, subsys, name]);
        }
    });

    CommandAction::None
}

pub(crate) fn cmd_tlb_mem(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> CommandAction {
    let memmap = crate::limine::memmap_entries().unwrap_or(&[]);
    if memmap.is_empty() {
        ctx.io.write_str("tlb.mem: no memory map available\r\n");
        return CommandAction::None;
    }

    let cols = [
        TableColumn { header: "Base", width: 18 },
        TableColumn { header: "Length", width: 18 },
        TableColumn { header: "Type", width: 24 },
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

pub(crate) fn cmd_tlb_cpu(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> CommandAction {
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

pub(crate) fn cmd_tlb_acpi(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> CommandAction {
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

pub(crate) fn cmd_tlb_uefi(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> CommandAction {
    let st = crate::efi::system_table();
    if st.is_none() {
        ctx.io.write_str("tlb.uefi: system table not found (not booted via UEFI?)\r\n");
        return CommandAction::None;
    }
    let st = st.unwrap();

    let cols = [
        TableColumn { header: "Field", width: 20 },
        TableColumn { header: "Value", width: 40 },
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
    let cols_cfg = [
        TableColumn { header: "Index", width: 6 },
        TableColumn { header: "GUID", width: 40 },
        TableColumn { header: "Name", width: 24 },
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
