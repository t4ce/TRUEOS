//! Intel High Definition Audio family ownership stub.
//!
//! Only reserve enumerated PCI functions. Hardware initialization belongs to
//! future work; claiming does not enable bus mastering, map BARs or touch codecs.

const PCI_OWNER: &str = "intel-hda";
const HDA_SUBCLASS: u8 = 0x03;
const MULTIMEDIA_AUDIO_SUBCLASS: u8 = 0x01;
// LPJB: Tiger Lake-LP Smart Sound can expose the DSP-style 04:01 class.
const TIGER_LAKE_LP_SMART_SOUND: u16 = 0xA0C8;

pub(crate) fn claim_devices() {
    let mut claimed = 0usize;
    for device in crate::pci::find_by_class(crate::pci::class::MULTIMEDIA) {
        let family_class = device.subclass == HDA_SUBCLASS
            || (device.subclass == MULTIMEDIA_AUDIO_SUBCLASS
                && device.device_id == TIGER_LAKE_LP_SMART_SOUND);
        if device.vendor_id != super::INTEL_VENDOR_ID || !family_class {
            continue;
        }
        match crate::pci::claim_device(&device, PCI_OWNER) {
            Ok(()) => {
                claimed += 1;
                crate::log_info!(target: "global";
                    "hda: ownership acquired bdf={:02X}:{:02X}.{} vendor=0x{:04X} device=0x{:04X} class={:02X}:{:02X} owner={} worker_slot={} mode=stub\n",
                    device.bus, device.slot, device.function, device.vendor_id,
                    device.device_id, device.class, device.subclass,
                    PCI_OWNER, crate::percpu::current_slot()
                );
            }
            Err(error) => crate::log_warn!(target: "global";
                "hda: ownership unavailable bdf={:02X}:{:02X}.{} error={:?}\n",
                device.bus, device.slot, device.function, error
            ),
        }
    }
    if claimed == 0 {
        crate::log_info!(target: "global";
            "hda: no Intel HDA functions claimed worker_slot={} mode=stub\n",
            crate::percpu::current_slot()
        );
    }
}
