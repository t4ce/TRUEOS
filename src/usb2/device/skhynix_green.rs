use crab_usb::{DeviceInfo, USBHost};
use embassy_executor::Spawner;

pub(crate) async fn maybe_start_skhynix_green(
    _host: &mut USBHost,
    dev_info: &DeviceInfo,
    _spawner: &Spawner,
    controller_id: u32,
) -> bool {
    let desc = dev_info.descriptor();
    if desc.vendor_id == 0x152E && desc.product_id == 0x7001 {
        crate::log!(
            "crabusb: skhynix-green {:04X}:{:04X} ctrl={} uas=disabled fallback=bot\n",
            desc.vendor_id,
            desc.product_id,
            controller_id
        );
    }
    false
}
