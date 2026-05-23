use crab_usb::{DeviceInfo, USBHost};
use embassy_executor::Spawner;

pub(crate) async fn maybe_start_skhynix_green(
    _host: &mut USBHost,
    dev_info: &DeviceInfo,
    _spawner: &Spawner,
    controller_id: u32,
) -> bool {
    let desc = dev_info.descriptor();
    if desc.vendor_id != 0x152E || desc.product_id != 0x7001 {
        return false;
    }

    let root_port_id = dev_info.root_port_id().unwrap_or(0);
    let transport_plan = super::mass::inspect_mass_transports(dev_info.configurations());
    crate::log!(
        "crabusb: skhynix-green {:04X}:{:04X} proof=detect ctrl={} root_port={} uas_candidates={} bot_present={} fallback=bot reason=stream-api-not-wired\n",
        desc.vendor_id,
        desc.product_id,
        controller_id,
        root_port_id,
        transport_plan.uas_candidate_count,
        transport_plan.bot.is_some()
    );
    false
}
