//! Keyboard-only i8042 transport. Translate scan set 2 to the shared HID path.
//!
//! One BSP task owns ports 0x60/0x64. Polling deliberately leaves IRQ1 disabled;
//! neither interrupt handlers nor a second keyboard event pipeline are needed.

use super::ps2_controller::{self, Bus};
use super::ps2_keyboard::Decoder;
use trueos_time::{Duration, Instant, Timer};

const DATA: u16 = 0x60;
const STATUS_COMMAND: u16 = 0x64;
const OUTPUT_FULL: u8 = 1;
const AUX_DATA: u8 = 1 << 5;
const RECEIVE_ERROR: u8 = (1 << 6) | (1 << 7);
const POLL_MS: u64 = 2;
const DRAIN_LIMIT: usize = 64;

const CONTROLLER_ID: u32 = 0x4938_3034; // "I804", distinct from USB and mediated input.
// Legacy input events carry only a slot ID, so reserve it across transports too.
const SLOT_ID: u32 = 0x4938_0001;
const ENDPOINT: u32 = 0;

struct Ports;

impl Bus for Ports {
    fn status(&mut self) -> u8 {
        unsafe { crate::portio::inb(STATUS_COMMAND) }
    }

    fn read_data(&mut self) -> u8 {
        unsafe { crate::portio::inb(DATA) }
    }

    fn write_command(&mut self, byte: u8) {
        unsafe { crate::portio::outb(STATUS_COMMAND, byte) }
    }

    fn write_data(&mut self, byte: u8) {
        unsafe { crate::portio::outb(DATA, byte) }
    }

    fn now_ms(&self) -> u64 {
        Instant::now().as_millis()
    }

    async fn pause(&mut self) {
        Timer::after(Duration::from_millis(1)).await;
    }
}

/// The FADT flag is a firmware hint, not a reliable presence test: machines
/// with a working PNP keyboard can leave it clear. The bounded controller and
/// keyboard handshake below establishes whether a usable device exists.
fn firmware_has_controller() -> Option<bool> {
    let tables = crate::efi::acpi::ensure_tables()?;
    let mapping = tables.find_table::<acpi::sdt::fadt::Fadt>()?;
    let fadt = unsafe { mapping.virtual_start.as_ref() };
    fadt.validate().ok()?;
    if fadt.header.revision < 3 || fadt.header.length < 111 {
        return None;
    }
    let flags = fadt.iapc_boot_arch;
    Some(flags.motherboard_implements_8042())
}

fn publish(decoder: &Decoder) {
    let mut report = [0u8; 8];
    report[0] = decoder.modifiers();
    report[2..].copy_from_slice(&decoder.keys());
    // This is the same sink as USB keyboards: HUT registration, legacy input,
    // physical transitions, named keys and layout-aware text all stay shared.
    crate::usb2::hid::handle_keyboard_boot_report(CONTROLLER_ID, SLOT_ID, ENDPOINT, &report);
}

#[trueos_executor::task]
pub(crate) async fn keyboard_task() {
    if firmware_has_controller() == Some(false) {
        crate::log_warn!(target: "input";
            "i8042-keyboard: firmware reports no-controller; verifying hardware with bounded probe\n");
    }

    let mut ports = Ports;
    loop {
        if let Err(error) = ps2_controller::initialize(&mut ports).await {
            crate::log_warn!(target: "input";
                "i8042-keyboard: unavailable stage={} reason={}\n", error.stage, error.reason);
            return;
        }
        let mut decoder = Decoder::new();
        publish(&decoder); // Enroll before the first keystroke, just like USB HID.
        crate::log_important!(target: "input";
            "i8042-keyboard: ready controller={} slot={} ep={} scan_set=2 translation=off irq1=off poll_ms={} sink=hid\n",
            CONTROLLER_ID, SLOT_ID, ENDPOINT, POLL_MS);

        let mut receive_errors = 0u64;
        let mut first_change = true;
        let mut reset = false;
        while !reset {
            for _ in 0..DRAIN_LIMIT {
                let status = ports.status();
                if status == 0xff {
                    crate::usb2::hid::remove_hid_slot(CONTROLLER_ID, SLOT_ID);
                    crate::log_warn!(target: "input"; "i8042-keyboard: controller disappeared\n");
                    return;
                }
                if status & OUTPUT_FULL == 0 {
                    break;
                }
                let byte = ports.read_data();
                if status & AUX_DATA != 0 {
                    continue; // No PS/2 mouse support or mouse scan decoding here.
                }
                if status & RECEIVE_ERROR != 0 || matches!(byte, 0x00 | 0xff | 0xfc | 0xfd) {
                    // A lost break byte must not leave a modifier held forever.
                    if decoder.clear() {
                        publish(&decoder);
                    }
                    receive_errors = receive_errors.saturating_add(1);
                    if receive_errors.is_power_of_two() {
                        crate::log_warn!(target: "input";
                            "i8042-keyboard: receive-errors={} state=released\n", receive_errors);
                    }
                    continue;
                }
                if byte == 0xaa {
                    // A keyboard BAT means its scan/command state was reset.
                    crate::usb2::hid::remove_hid_slot(CONTROLLER_ID, SLOT_ID);
                    crate::log_info!(target: "input"; "i8042-keyboard: keyboard reset; renegotiating\n");
                    reset = true;
                    break;
                }
                if decoder.feed(byte) {
                    publish(&decoder);
                    if first_change {
                        first_change = false;
                        crate::log_info!(target: "input"; "i8042-keyboard: first-state-change payload=omitted\n");
                    }
                }
                // Pause has a make sequence and no hardware break sequence.
                if decoder.finish_momentary() {
                    publish(&decoder);
                }
            }
            Timer::after(Duration::from_millis(POLL_MS)).await;
        }
    }
}
