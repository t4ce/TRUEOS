// it’s basically an OS-level safety net.
// atleast for my cpus, if the mainboards are
// not updated, cpus will be now - intel mop update loader
// because bios updates dont arrive magically usually
#[cfg(target_arch = "x86_64")]
mod imp {
    use core::arch::x86_64::__cpuid;
    use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
    use x86_64::registers::model_specific::Msr;

    const INTEL_VENDOR_EBX: u32 = 0x756E_6547;
    const INTEL_VENDOR_EDX: u32 = 0x4965_6E69;
    const INTEL_VENDOR_ECX: u32 = 0x6C65_746E;

    const MSR_IA32_PLATFORM_ID: u32 = 0x17;
    const MSR_IA32_BIOS_UPDT_TRIG: u32 = 0x79;
    const MSR_IA32_BIOS_SIGN_ID: u32 = 0x8B;

    const HEADER_LEN: usize = 48;
    const DEFAULT_DATA_SIZE: usize = 2000;
    const DEFAULT_TOTAL_SIZE: usize = 2048;
    const EXT_HEADER_LEN: usize = 20;
    const EXT_ENTRY_LEN: usize = 12;

    #[repr(align(16))]
    struct AlignedBytes<const N: usize>([u8; N]);

    static SELECTED_PTR: AtomicUsize = AtomicUsize::new(0);
    static SELECTED_LEN: AtomicUsize = AtomicUsize::new(0);
    static SELECTED_REV: AtomicU32 = AtomicU32::new(0);

    static UCODE_06_BF_02: AlignedBytes<0x37800> =
        AlignedBytes(*include_bytes!("../tools/intel-ucode/06-bf-02"));
    static UCODE_06_B7_01: AlignedBytes<0x35800> =
        AlignedBytes(*include_bytes!("../tools/intel-ucode/06-b7-01"));
    static UCODE_06_8C_01: AlignedBytes<0x1B800> =
        AlignedBytes(*include_bytes!("../tools/intel-ucode/06-8c-01"));

    static EMBEDDED_MICROCODE: &[(&str, &[u8])] = &[
        ("intel-ucode/06-bf-02", &UCODE_06_BF_02.0),
        ("intel-ucode/06-b7-01", &UCODE_06_B7_01.0),
        ("intel-ucode/06-8c-01", &UCODE_06_8C_01.0),
    ];

    #[derive(Clone, Copy)]
    struct Target {
        signature: u32,
        platform_mask: u32,
        current_revision: u32,
    }

    #[derive(Clone, Copy)]
    struct Update<'a> {
        bytes: &'a [u8],
        revision: u32,
        date: u32,
    }

    #[derive(Clone, Copy)]
    struct Best {
        ptr: usize,
        len: usize,
        revision: u32,
        date: u32,
    }

    #[derive(Clone, Copy)]
    pub(crate) struct EmbeddedSource {
        pub(crate) name: &'static str,
        pub(crate) len: usize,
    }

    #[derive(Clone, Copy)]
    pub(crate) struct Snapshot {
        pub(crate) intel: bool,
        pub(crate) target_name: &'static str,
        pub(crate) signature: u32,
        pub(crate) fms: FmsName,
        pub(crate) platform_mask: u32,
        pub(crate) current_revision: u32,
        pub(crate) selected_revision: u32,
        pub(crate) selected_len: usize,
        pub(crate) embedded_sources: [EmbeddedSource; 3],
    }

    pub(crate) fn snapshot() -> Snapshot {
        let embedded_sources = [
            EmbeddedSource {
                name: EMBEDDED_MICROCODE[0].0,
                len: EMBEDDED_MICROCODE[0].1.len(),
            },
            EmbeddedSource {
                name: EMBEDDED_MICROCODE[1].0,
                len: EMBEDDED_MICROCODE[1].1.len(),
            },
            EmbeddedSource {
                name: EMBEDDED_MICROCODE[2].0,
                len: EMBEDDED_MICROCODE[2].1.len(),
            },
        ];
        let selected_revision = SELECTED_REV.load(Ordering::Acquire);
        let selected_len = SELECTED_LEN.load(Ordering::Acquire);

        if let Some(target) = detect_target() {
            Snapshot {
                intel: true,
                target_name: known_target_name(target.signature),
                signature: target.signature,
                fms: fms_name(target.signature),
                platform_mask: target.platform_mask,
                current_revision: target.current_revision,
                selected_revision,
                selected_len,
                embedded_sources,
            }
        } else {
            Snapshot {
                intel: false,
                target_name: "non-intel-or-missing-msr",
                signature: 0,
                fms: FmsName(0),
                platform_mask: 0,
                current_revision: 0,
                selected_revision,
                selected_len,
                embedded_sources,
            }
        }
    }

    pub fn init_from_limine_bsp() {
        let Some(target) = detect_target() else {
            crate::log!("microcode/intel: action=skip reason=non-intel-or-missing-msr\n");
            return;
        };

        let mut best: Option<Best> = None;
        let mut embedded_seen = 0usize;
        let mut boot_modules_seen = 0usize;
        let mut records_seen = 0usize;
        let mut records_matching = 0usize;

        for (name, bytes) in EMBEDDED_MICROCODE {
            embedded_seen = embedded_seen.saturating_add(1);
            records_seen = records_seen.saturating_add(scan_blob(bytes, target, |update| {
                records_matching = records_matching.saturating_add(1);
                consider_best_update(&mut best, target, update);
            }));
            crate::log!("microcode/intel: embedded-source name={} len=0x{:X}\n", name, bytes.len());
        }

        crate::limine::for_each_module(|module| {
            boot_modules_seen = boot_modules_seen.saturating_add(1);
            records_seen = records_seen.saturating_add(scan_blob(module.bytes, target, |update| {
                records_matching = records_matching.saturating_add(1);
                consider_best_update(&mut best, target, update);
            }));
        });

        crate::log!(
            "microcode/intel: cpu={} sig=0x{:08X} pf=0x{:02X} current=0x{:08X} embedded={} boot_modules={} records={} matches={} target={}\n",
            known_target_name(target.signature),
            target.signature,
            target.platform_mask,
            target.current_revision,
            embedded_seen,
            boot_modules_seen,
            records_seen,
            records_matching,
            fms_name(target.signature)
        );

        let Some(best) = best else {
            crate::log!(
                "microcode/intel: action=skip reason=no-newer-update records={} current=0x{:08X}\n",
                records_seen,
                target.current_revision
            );
            return;
        };

        SELECTED_PTR.store(best.ptr, Ordering::Release);
        SELECTED_LEN.store(best.len, Ordering::Release);
        SELECTED_REV.store(best.revision, Ordering::Release);

        crate::log!(
            "microcode/intel: selected rev=0x{:08X} date={} len=0x{:X} action=load-bsp\n",
            best.revision,
            fmt_date(best.date),
            best.len
        );
        apply_selected_to_current_cpu("bsp");
    }

    fn consider_best_update(best: &mut Option<Best>, target: Target, update: Update<'static>) {
        if update.revision <= target.current_revision {
            return;
        }
        if best.map(|b| update.revision > b.revision).unwrap_or(true) {
            *best = Some(Best {
                ptr: update.bytes.as_ptr() as usize,
                len: update.bytes.len(),
                revision: update.revision,
                date: update.date,
            });
        }
    }

    pub fn apply_selected_to_current_cpu(tag: &str) {
        let ptr = SELECTED_PTR.load(Ordering::Acquire);
        let len = SELECTED_LEN.load(Ordering::Acquire);
        let selected_rev = SELECTED_REV.load(Ordering::Acquire);
        if ptr == 0 || len < HEADER_LEN || selected_rev == 0 {
            return;
        }

        let Some(target) = detect_target() else {
            return;
        };
        if target.current_revision >= selected_rev {
            return;
        }

        let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len) };
        let Some(update) = parse_update_at(bytes, target, 0) else {
            crate::log!(
                "microcode/intel: cpu={} action=skip reason=selected-record-no-longer-valid\n",
                tag
            );
            return;
        };

        unsafe {
            Msr::new(MSR_IA32_BIOS_UPDT_TRIG).write(update.bytes.as_ptr().add(HEADER_LEN) as u64);
        }
        let after = read_current_revision();
        crate::log!(
            "microcode/intel: cpu={} before=0x{:08X} selected=0x{:08X} after=0x{:08X} status={}\n",
            tag,
            target.current_revision,
            selected_rev,
            after,
            if after >= selected_rev {
                "loaded"
            } else {
                "not-accepted"
            }
        );
    }

    fn detect_target() -> Option<Target> {
        let leaf0 = __cpuid(0);
        if leaf0.ebx != INTEL_VENDOR_EBX
            || leaf0.edx != INTEL_VENDOR_EDX
            || leaf0.ecx != INTEL_VENDOR_ECX
        {
            return None;
        }
        if leaf0.eax < 1 {
            return None;
        }

        let signature = __cpuid(1).eax;
        let platform_id = unsafe { (Msr::new(MSR_IA32_PLATFORM_ID).read() >> 50) & 0x7 } as u32;
        let platform_mask = 1u32 << platform_id;
        Some(Target {
            signature,
            platform_mask,
            current_revision: read_current_revision(),
        })
    }

    fn read_current_revision() -> u32 {
        unsafe {
            Msr::new(MSR_IA32_BIOS_SIGN_ID).write(0);
            let _ = __cpuid(1);
            (Msr::new(MSR_IA32_BIOS_SIGN_ID).read() >> 32) as u32
        }
    }

    fn scan_blob(
        bytes: &'static [u8],
        target: Target,
        mut f: impl FnMut(Update<'static>),
    ) -> usize {
        let mut offset = 0usize;
        let mut records_seen = 0usize;
        while offset.saturating_add(HEADER_LEN) <= bytes.len() {
            let Some(record) = parse_record_at(bytes, offset) else {
                break;
            };
            records_seen = records_seen.saturating_add(1);
            if let Some(update) = match_record(record, target) {
                f(update);
            }
            offset = offset.saturating_add(record.bytes.len());
        }
        records_seen
    }

    fn parse_update_at(
        bytes: &'static [u8],
        target: Target,
        offset: usize,
    ) -> Option<Update<'static>> {
        match_record(parse_record_at(bytes, offset)?, target)
    }

    #[derive(Clone, Copy)]
    struct Record<'a> {
        bytes: &'a [u8],
        revision: u32,
        date: u32,
        processor_signature: u32,
        processor_flags: u32,
        data_size: usize,
    }

    fn parse_record_at(bytes: &'static [u8], offset: usize) -> Option<Record<'static>> {
        let remaining = bytes.get(offset..)?;
        if remaining.len() < HEADER_LEN {
            return None;
        }

        let header_version = le_u32(remaining, 0)?;
        let revision = le_u32(remaining, 4)?;
        let date = le_u32(remaining, 8)?;
        let processor_signature = le_u32(remaining, 12)?;
        let loader_revision = le_u32(remaining, 20)?;
        let processor_flags = le_u32(remaining, 24)?;
        let data_size_raw = le_u32(remaining, 28)? as usize;
        let total_size_raw = le_u32(remaining, 32)? as usize;

        if header_version != 1 || loader_revision != 1 {
            return None;
        }

        let data_size = if data_size_raw == 0 {
            DEFAULT_DATA_SIZE
        } else {
            data_size_raw
        };
        let total_size = if total_size_raw == 0 {
            DEFAULT_TOTAL_SIZE
        } else {
            total_size_raw
        };
        if total_size < HEADER_LEN
            || total_size % 4 != 0
            || HEADER_LEN.checked_add(data_size)? > total_size
            || total_size > remaining.len()
        {
            return None;
        }

        let record = &remaining[..total_size];
        if !checksum_zero(record) {
            return None;
        }

        Some(Record {
            bytes: record,
            revision,
            date,
            processor_signature,
            processor_flags,
            data_size,
        })
    }

    fn match_record(record: Record<'static>, target: Target) -> Option<Update<'static>> {
        let base_match = record.processor_signature == target.signature
            && (record.processor_flags & target.platform_mask) != 0;
        let ext_match = extended_signature_match(
            record.bytes,
            HEADER_LEN + record.data_size,
            target.signature,
            target.platform_mask,
        );
        if !base_match && !ext_match {
            return None;
        }

        Some(Update {
            bytes: record.bytes,
            revision: record.revision,
            date: record.date,
        })
    }

    fn extended_signature_match(
        record: &[u8],
        ext_offset: usize,
        signature: u32,
        platform_mask: u32,
    ) -> bool {
        let Some(ext) = record.get(ext_offset..) else {
            return false;
        };
        if ext.len() < EXT_HEADER_LEN {
            return false;
        }
        let Some(count) = le_u32(ext, 0).map(|v| v as usize) else {
            return false;
        };
        let Some(table_len) = EXT_HEADER_LEN.checked_add(count.saturating_mul(EXT_ENTRY_LEN))
        else {
            return false;
        };
        if table_len > ext.len() || !checksum_zero(&ext[..table_len]) {
            return false;
        }
        for idx in 0..count {
            let entry = EXT_HEADER_LEN + idx * EXT_ENTRY_LEN;
            let Some(entry_sig) = le_u32(ext, entry) else {
                return false;
            };
            let Some(entry_flags) = le_u32(ext, entry + 4) else {
                return false;
            };
            if entry_sig == signature && (entry_flags & platform_mask) != 0 {
                return true;
            }
        }
        false
    }

    fn checksum_zero(bytes: &[u8]) -> bool {
        if bytes.len() % 4 != 0 {
            return false;
        }
        let mut sum = 0u32;
        let mut offset = 0usize;
        while offset < bytes.len() {
            let Some(word) = le_u32(bytes, offset) else {
                return false;
            };
            sum = sum.wrapping_add(word);
            offset += 4;
        }
        sum == 0
    }

    fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
        let chunk = bytes.get(offset..offset.checked_add(4)?)?;
        Some(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
    }

    fn known_target_name(signature: u32) -> &'static str {
        match signature {
            0x000B_06F2 => "core-i5-14500t/rpl-hx-s-c0",
            0x000B_0671 => "core-i9-13900k/rpl-e-hx-s-b0",
            0x0008_06C1 => "core-i7-1185g7/tgl-b0-b1",
            _ => "unknown-intel",
        }
    }

    fn fms_name(signature: u32) -> FmsName {
        FmsName(signature)
    }

    fn fmt_date(date: u32) -> UcodeDate {
        UcodeDate(date)
    }

    #[derive(Clone, Copy)]
    pub(crate) struct FmsName(u32);

    impl core::fmt::Display for FmsName {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let stepping = self.0 & 0xF;
            let model = ((self.0 >> 4) & 0xF) | ((self.0 >> 12) & 0xF0);
            let family = ((self.0 >> 8) & 0xF) + ((self.0 >> 20) & 0xFF);
            write!(f, "{:02x}-{:02x}-{:02x}", family, model, stepping)
        }
    }

    struct UcodeDate(u32);

    impl core::fmt::Display for UcodeDate {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let month = (self.0 >> 24) & 0xFF;
            let day = (self.0 >> 16) & 0xFF;
            let year = self.0 & 0xFFFF;
            write!(f, "0x{:08X}({:04X}-{:02X}-{:02X})", self.0, year, month, day)
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
mod imp {
    #[derive(Clone, Copy)]
    pub(crate) struct EmbeddedSource {
        pub(crate) name: &'static str,
        pub(crate) len: usize,
    }

    #[derive(Clone, Copy)]
    pub(crate) struct FmsName(pub(crate) u32);

    impl core::fmt::Display for FmsName {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let _ = self;
            write!(f, "--")
        }
    }

    #[derive(Clone, Copy)]
    pub(crate) struct Snapshot {
        pub(crate) intel: bool,
        pub(crate) target_name: &'static str,
        pub(crate) signature: u32,
        pub(crate) fms: FmsName,
        pub(crate) platform_mask: u32,
        pub(crate) current_revision: u32,
        pub(crate) selected_revision: u32,
        pub(crate) selected_len: usize,
        pub(crate) embedded_sources: [EmbeddedSource; 3],
    }

    pub fn init_from_limine_bsp() {}
    pub fn apply_selected_to_current_cpu(_tag: &str) {}

    pub(crate) fn snapshot() -> Snapshot {
        Snapshot {
            intel: false,
            target_name: "non-x86",
            signature: 0,
            fms: FmsName(0),
            platform_mask: 0,
            current_revision: 0,
            selected_revision: 0,
            selected_len: 0,
            embedded_sources: [
                EmbeddedSource { name: "-", len: 0 },
                EmbeddedSource { name: "-", len: 0 },
                EmbeddedSource { name: "-", len: 0 },
            ],
        }
    }
}

pub(crate) use imp::snapshot;
pub use imp::{apply_selected_to_current_cpu, init_from_limine_bsp};
