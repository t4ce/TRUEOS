use core::fmt::Write;

use alloc::string::String;
use spin::Mutex;
use x86_64::instructions::interrupts;

use super::tlb_platform::NctManagementHint;

const SUPERIO_PORTS: [u16; 2] = [0x2E, 0x4E];
const SIO_REG_LDSEL: u8 = 0x07;
const SIO_REG_DEVID: u8 = 0x20;
const SIO_REG_ENABLE: u8 = 0x30;
const SIO_REG_ADDR: u8 = 0x60;
const NCT6791_REG_HM_IO_SPACE_LOCK_ENABLE: u8 = 0x28;
const NCT_HWM_LOGICAL_DEVICE: u8 = 0x0B;
const NCT6798_FAMILY_ID: u16 = 0xD428;
const NCT_FAMILY_MASK: u16 = 0xFFF8;
const HWM_INDEX_OFFSET: u16 = 5;
const HWM_DATA_OFFSET: u16 = 6;

static NCT_PROBE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy)]
struct SuperIoSnapshot {
    config_port: u16,
    chip_id: u16,
    previous_ldn: u8,
    enable: u8,
    raw_base: u16,
    mapping_lock: u8,
}

impl SuperIoSnapshot {
    fn family_match(self) -> bool {
        self.chip_id & NCT_FAMILY_MASK == NCT6798_FAMILY_ID
    }

    fn base(self) -> u16 {
        self.raw_base & !0x0007
    }

    fn enabled(self) -> bool {
        self.enable & 1 != 0
    }

    fn mapping_locked(self) -> bool {
        self.mapping_lock & 0x10 != 0
    }

    fn response_present(self) -> bool {
        !matches!(self.chip_id, 0x0000 | 0xFFFF)
    }
}

pub(crate) fn build_probe_text() -> String {
    let mut out = String::new();
    writeln!(out, "NCT5585D Super-I/O identity probe").unwrap();
    writeln!(
        out,
        "policy=explicit transient config-mode probe; reads identity/LDN state only; never enables the device, changes its base, unlocks mappings, or touches fan/PWM/GPIO data"
    )
    .unwrap();

    let smbios = super::tlb_platform::nct_management_hint();
    append_smbios_hint(&mut out, smbios);

    let _probe_guard = NCT_PROBE_LOCK.lock();
    let snapshots = interrupts::without_interrupts(|| {
        [probe_superio(SUPERIO_PORTS[0]), probe_superio(SUPERIO_PORTS[1])]
    });

    let mut verified = 0usize;
    for snapshot in snapshots {
        append_snapshot(&mut out, snapshot, smbios);
        if snapshot.family_match() {
            verified = verified.saturating_add(1);
        }
    }

    match verified {
        0 => writeln!(
            out,
            "result=not-verified reason=no NCT6798-compatible/NCT5585D family ID observed"
        )
        .unwrap(),
        1 => writeln!(
            out,
            "result=verified confidence=superio-family+logical-device-state"
        )
        .unwrap(),
        count => writeln!(
            out,
            "result=ambiguous verified_candidates={} reason=family ID appeared on multiple config ports",
            count
        )
        .unwrap(),
    }

    out
}

fn append_smbios_hint(out: &mut String, hint: Option<NctManagementHint>) {
    let Some(hint) = hint else {
        writeln!(out, "smbios_hint=unavailable").unwrap();
        return;
    };

    writeln!(
        out,
        "smbios_hint handle=0x{:04X} device_type={} address={} address_type={}",
        hint.handle,
        hint.device_type
            .map(|value| alloc::format!("0x{:02X}", value))
            .unwrap_or_else(|| String::from("-")),
        hint.address
            .map(|value| alloc::format!("0x{:08X}", value))
            .unwrap_or_else(|| String::from("-")),
        hint.address_type
            .map(|value| alloc::format!("0x{:02X}", value))
            .unwrap_or_else(|| String::from("-"))
    )
    .unwrap();
}

fn append_snapshot(
    out: &mut String,
    snapshot: SuperIoSnapshot,
    smbios: Option<NctManagementHint>,
) {
    if !snapshot.response_present() {
        writeln!(
            out,
            "sio=0x{:02X} state=no-response raw_id=0x{:04X} previous_ldn=0x{:02X}",
            snapshot.config_port, snapshot.chip_id, snapshot.previous_ldn
        )
        .unwrap();
        return;
    }

    let base = snapshot.base();
    let expected_base = smbios
        .and_then(|hint| hint.address)
        .filter(|address| *address <= u16::MAX as u32)
        .map(|address| address as u16);
    let smbios_match = expected_base
        .map(|expected| expected == base)
        .map(yes_no)
        .unwrap_or("-");
    let address_type_io = smbios
        .and_then(|hint| hint.address_type)
        .map(|kind| kind == 0x03)
        .map(yes_no)
        .unwrap_or("-");
    let runtime_state = if base == 0 {
        "unassigned"
    } else if !snapshot.enabled() {
        "disabled"
    } else if snapshot.mapping_locked() {
        "mapping-locked"
    } else {
        "reachable-candidate"
    };

    writeln!(
        out,
        "sio=0x{:02X} raw_id=0x{:04X} family=0x{:04X} revision=0x{:X} identity={} previous_ldn=0x{:02X}",
        snapshot.config_port,
        snapshot.chip_id,
        snapshot.chip_id & NCT_FAMILY_MASK,
        snapshot.chip_id & !NCT_FAMILY_MASK,
        if snapshot.family_match() {
            "NCT6798-compatible/NCT5585D"
        } else {
            "unsupported-or-other"
        },
        snapshot.previous_ldn
    )
    .unwrap();
    writeln!(
        out,
        "  hwm_ld=0x{:02X} enable=0x{:02X} enabled={} raw_base=0x{:04X} base=0x{:04X} index=0x{:04X} data=0x{:04X}",
        NCT_HWM_LOGICAL_DEVICE,
        snapshot.enable,
        yes_no(snapshot.enabled()),
        snapshot.raw_base,
        base,
        base.saturating_add(HWM_INDEX_OFFSET),
        base.saturating_add(HWM_DATA_OFFSET)
    )
    .unwrap();
    writeln!(
        out,
        "  mapping_reg_0x28=0x{:02X} mapping_locked={} runtime_state={} smbios_base_match={} smbios_address_type_io={}",
        snapshot.mapping_lock,
        yes_no(snapshot.mapping_locked()),
        runtime_state,
        smbios_match,
        address_type_io
    )
    .unwrap();
}

fn probe_superio(config_port: u16) -> SuperIoSnapshot {
    unsafe { superio_enter(config_port) };

    let chip_id = u16::from(unsafe { superio_read(config_port, SIO_REG_DEVID) }) << 8
        | u16::from(unsafe {
            superio_read(config_port, SIO_REG_DEVID.wrapping_add(1))
        });
    let previous_ldn = unsafe { superio_read(config_port, SIO_REG_LDSEL) };

    unsafe { superio_write(config_port, SIO_REG_LDSEL, NCT_HWM_LOGICAL_DEVICE) };
    let enable = unsafe { superio_read(config_port, SIO_REG_ENABLE) };
    let raw_base = u16::from(unsafe { superio_read(config_port, SIO_REG_ADDR) }) << 8
        | u16::from(unsafe {
            superio_read(config_port, SIO_REG_ADDR.wrapping_add(1))
        });
    let mapping_lock =
        unsafe { superio_read(config_port, NCT6791_REG_HM_IO_SPACE_LOCK_ENABLE) };

    unsafe {
        superio_write(config_port, SIO_REG_LDSEL, previous_ldn);
        superio_exit(config_port);
    }

    SuperIoSnapshot {
        config_port,
        chip_id,
        previous_ldn,
        enable,
        raw_base,
        mapping_lock,
    }
}

unsafe fn superio_enter(config_port: u16) {
    crate::outb(config_port, 0x87);
    crate::outb(config_port, 0x87);
}

unsafe fn superio_exit(config_port: u16) {
    crate::outb(config_port, 0xAA);
    crate::outb(config_port, 0x02);
    crate::outb(config_port + 1, 0x02);
}

unsafe fn superio_read(config_port: u16, register: u8) -> u8 {
    crate::outb(config_port, register);
    crate::inb(config_port + 1)
}

unsafe fn superio_write(config_port: u16, register: u8, value: u8) {
    crate::outb(config_port, register);
    crate::outb(config_port + 1, value);
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
