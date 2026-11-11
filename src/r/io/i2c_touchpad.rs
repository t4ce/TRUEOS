//! Physical HID-over-I2C touchpad, using the shared HID pointer input path.

use super::{
    hid_touchpad::{Feature, Pointer, Touchpad},
    i2c_hid::{self, Descriptor, Input},
    intel_gpio::GpioLine,
    intel_i2c::Controller,
};
use crate::efi::acpi::i2c_hid_firmware::{self, TrackpadFirmware};
use alloc::vec;
use trueos_time::{Duration, Instant, Timer};

const CONTROLLER_ID: u32 = 0x4932_4348; // "I2CH", physical transport namespace.
const SLOT_ID: u32 = 0x4932_0001;
const ENDPOINT: u32 = 0;
const POLL_MS: u64 = 2;
const RESET_TIMEOUT_MS: u64 = 1000;

fn read_nvs(base: u64, output: &mut [u8]) -> Result<(), &'static str> {
    if output.is_empty() || output.len() > 64 * 1024 {
        return Err("firmware NVS length invalid");
    }
    let end = base
        .checked_add(output.len() as u64)
        .ok_or("firmware NVS range overflow")?;
    let entries = crate::limine::memmap_entries().ok_or("boot memory map missing")?;
    if !entries.iter().any(|entry| {
        matches!(
            entry.type_,
            ::limine::memmap::MEMMAP_ACPI_NVS | ::limine::memmap::MEMMAP_ACPI_RECLAIMABLE
        ) && base >= entry.base
            && end <= entry.base.saturating_add(entry.length)
    }) {
        return Err("firmware field is outside ACPI NVS RAM");
    }
    let memory = crate::pci::mmio::map_mmio_region_exact(base, output.len())
        .map_err(|_| "firmware NVS mapping failed")?;
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = unsafe { core::ptr::read_volatile(memory.as_ptr().add(index)) };
    }
    Ok(())
}

fn firmware() -> Result<TrackpadFirmware, &'static str> {
    let tables = crate::efi::acpi::ensure_tables().ok_or("ACPI tables missing")?;
    let fadt = tables
        .find_table::<acpi::sdt::fadt::Fadt>()
        .ok_or("FADT missing")?;
    let fadt = unsafe { fadt.virtual_start.as_ref() };
    fadt.validate().map_err(|_| "FADT invalid")?;
    let physical = fadt.dsdt_address().map_err(|_| "DSDT address missing")?;
    let dsdt = crate::efi::acpi::map_table_bytes(physical as usize).ok_or("DSDT mapping failed")?;
    i2c_hid_firmware::discover(dsdt, read_nvs)
}

fn publish(pointer: Pointer) {
    crate::usb2::hid::handle_mouse_report_values(
        CONTROLLER_ID,
        SLOT_ID,
        ENDPOINT,
        pointer.buttons,
        pointer.dx,
        pointer.dy,
        pointer.wheel,
        pointer.wheel != 0,
    );
}

fn feature_value(feature: Feature, bytes: &[u8]) -> Option<u32> {
    let prefix = usize::from(feature.report_id != 0);
    if !(1..=32).contains(&feature.bit_size)
        || (prefix != 0 && bytes.first().copied()? != feature.report_id)
    {
        return None;
    }
    let mut value = 0;
    for bit in 0..usize::from(feature.bit_size) {
        let position = usize::from(feature.bit_offset) + bit;
        value |= u32::from((bytes.get(prefix + position / 8)? >> (position % 8)) & 1) << bit;
    }
    Some(value)
}

async fn select_input_mode(
    controller: &mut Controller,
    address: u8,
    descriptor: Descriptor,
    decoder: &Touchpad,
) -> Result<&'static str, &'static str> {
    let Some(feature) = decoder.input_mode_feature() else {
        return Ok("descriptor-default");
    };
    let length = usize::from(feature.report_len);
    if length == 0 || length > 256 || feature.report_id == 0 {
        return Err("unsupported Input Mode feature");
    }
    let mut bytes = [0; 256];
    let mut target = controller.target(address);
    if descriptor
        .get_feature(&mut target, feature.report_id, &mut bytes[..length])
        .await
        != Ok(length)
    {
        // With no other fields there is nothing to preserve. Otherwise a GET
        // failure must not lead to overwriting unrelated feature settings.
        if feature.bit_offset != 0 || usize::from(feature.bit_size) != (length - 1) * 8 {
            return Err("cannot read existing Input Mode feature");
        }
    }
    let mode = if decoder.has_precision_reports() {
        3
    } else {
        0
    };
    feature
        .set_value(mode, &mut bytes[..length])
        .ok_or("Input Mode feature cannot be encoded")?;
    descriptor
        .set_feature(&mut target, &bytes[..length])
        .await
        .map_err(|_| "setting Input Mode failed")?;
    let readback = descriptor
        .get_feature(&mut target, feature.report_id, &mut bytes[..length])
        .await
        .map_err(|_| "Input Mode readback failed")?;
    if readback != length || feature_value(feature, &bytes[..readback]) != Some(mode) {
        return Err("device rejected Input Mode");
    }
    Ok(if mode == 3 {
        "precision-relative"
    } else {
        "mouse-compatible"
    })
}

async fn wait_reset(
    controller: &mut Controller,
    address: u8,
    descriptor: Descriptor,
    line: &GpioLine,
) -> Result<(), &'static str> {
    let deadline = Instant::now().as_millis().saturating_add(RESET_TIMEOUT_MS);
    let mut input = [0; i2c_hid::MAX_INPUT];
    while Instant::now().as_millis() < deadline {
        if line.asserted()? {
            match descriptor
                .read_input(&mut controller.target(address), &mut input)
                .await
            {
                Ok(Input::ResetComplete) => return Ok(()),
                Ok(_) | Err(_) => {} // Device may NACK while its reset is in progress.
            }
        }
        Timer::after(Duration::from_millis(POLL_MS)).await;
    }
    Err("reset completion timeout")
}

async fn serve(
    controller: &mut Controller,
    config: &TrackpadFirmware,
    line: &GpioLine,
) -> Result<(), &'static str> {
    let address = config.slave_address;
    let descriptor = Descriptor::read(&mut controller.target(address), config.hid_descriptor_register)
        .await.map_err(|error| {
            crate::log_warn!(target: "input"; "i2c-trackpad: HID descriptor read failed error={:?}\n", error);
            "HID descriptor unavailable"
        })?;
    descriptor
        .power_on(&mut controller.target(address))
        .await
        .map_err(|_| "power-on failed")?;
    Timer::after(Duration::from_millis(60)).await;
    descriptor
        .reset(&mut controller.target(address))
        .await
        .map_err(|_| "reset command failed")?;
    wait_reset(controller, address, descriptor, line).await?;
    // Complete the standard wake-up sequence after reset as well: some HID
    // devices return to low power while acknowledging the reset command.
    descriptor
        .power_on(&mut controller.target(address))
        .await
        .map_err(|_| "post-reset power-on failed")?;
    Timer::after(Duration::from_millis(60)).await;

    let mut report_descriptor = vec![0; usize::from(descriptor.report_descriptor_length)];
    descriptor
        .report_descriptor(&mut controller.target(address), &mut report_descriptor)
        .await
        .map_err(|_| "report descriptor read failed")?;
    let mut decoder = Touchpad::from_descriptor(&report_descriptor)
        .map_err(|_| "unsupported HID pointer descriptor")?;
    let mode = select_input_mode(controller, address, descriptor, &decoder).await?;
    publish(Pointer::default()); // Physical Human device, visible before movement.
    let (tx, rx) = controller.fifo_depths();
    crate::log_important!(target: "input";
        "i2c-trackpad: ready pci={:02x}:{:02x}.{} address=0x{:02x} hid={:04x}:{:04x} gpio={} mode={} fifo={}/{} poll_ms={} sink=hid-human\n",
        config.pci_bus, config.pci_slot, config.pci_function, address,
        descriptor.vendor_id, descriptor.product_id, config.gpio_number, mode, tx, rx, POLL_MS);

    let mut input = [0; i2c_hid::MAX_INPUT];
    let mut first_activity = true;
    let mut errors = 0u32;
    loop {
        for _ in 0..8 {
            if !line.asserted()? {
                break;
            }
            let frame = descriptor
                .read_input(&mut controller.target(address), &mut input)
                .await;
            match frame {
                Ok(Input::Report { length }) => {
                    errors = 0;
                    if let Some(pointer) = decoder.decode_i2c(&input[..length]) {
                        publish(pointer);
                        if first_activity {
                            first_activity = false;
                            crate::log_info!(target: "input"; "i2c-trackpad: first-input payload=omitted\n");
                        }
                    }
                }
                Ok(Input::Empty) => {
                    errors = 0;
                }
                Ok(Input::ResetComplete) => return Err("device reset during input"),
                Err(error) => {
                    if let Some(release) = decoder.reset() {
                        publish(release);
                    }
                    errors += 1;
                    crate::log_warn!(target: "input"; "i2c-trackpad: input-error count={} error={:?}\n", errors, error);
                    if errors >= 3 {
                        return Err("repeated input transfer failures");
                    }
                    break;
                }
            }
        }
        Timer::after(Duration::from_millis(POLL_MS)).await;
    }
}

#[trueos_executor::task]
pub(crate) async fn trackpad_task() {
    let config = match firmware() {
        Ok(config) => config,
        Err(reason) => {
            crate::log_warn!(target: "input"; "i2c-trackpad: discovery unavailable reason={}\n", reason);
            return;
        }
    };
    crate::log_info!(target: "input";
        "i2c-trackpad: discovered pci={:02x}:{:02x}.{} address=0x{:02x} descriptor=0x{:04x} gpio={} speed_hz={}\n",
        config.pci_bus, config.pci_slot, config.pci_function, config.slave_address,
        config.hid_descriptor_register, config.gpio_number, config.speed_hz);
    let line = match GpioLine::open(
        &config.gpio_hid,
        &config.gpio_regions,
        config.gpio_number,
        config.gpio_polarity,
        config.gpio_trigger,
    ) {
        Ok(line) => line,
        Err(reason) => {
            crate::log_warn!(target: "input"; "i2c-trackpad: GPIO unavailable reason={}\n", reason);
            return;
        }
    };
    let device = crate::pci::with_devices(|list| {
        list.iter()
            .find(|device| {
                (device.bus, device.slot, device.function)
                    == (config.pci_bus, config.pci_slot, config.pci_function)
            })
            .copied()
    });
    let Some(device) = device else {
        crate::log_warn!(target: "input"; "i2c-trackpad: firmware PCI function missing\n");
        return;
    };
    for attempt in 1..=3 {
        let mut controller = match Controller::open(device, config.speed_hz).await {
            Ok(controller) => controller,
            Err(error) => {
                crate::log_warn!(target: "input"; "i2c-trackpad: controller unavailable error={:?}\n", error);
                return;
            }
        };
        let result = serve(&mut controller, &config, &line).await;
        crate::usb2::hid::remove_hid_slot(CONTROLLER_ID, SLOT_ID);
        let _ = controller.shutdown().await;
        drop(controller);
        if let Err(reason) = result {
            crate::log_warn!(target: "input"; "i2c-trackpad: stopped attempt={} reason={}\n", attempt, reason);
        }
        Timer::after(Duration::from_millis(100)).await;
    }
}
