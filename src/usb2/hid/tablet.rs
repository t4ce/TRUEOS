use core::cmp::min;

#[inline]
pub(crate) fn matches_interface(class: u8, subclass: u8, protocol: u8) -> bool {
    class == 0x03 && subclass == 0x00 && protocol == 0x00
}

#[inline]
pub(crate) fn report_len(max_packet_size: u16) -> usize {
    usize::from(max_packet_size.max(8))
}

pub(crate) fn handle_packet(vendor_id: u16, product_id: u16, endpoint: u8, sample: &[u8]) {
    let nonzero = sample.iter().copied().any(|byte| byte != 0);
    if !nonzero {
        return;
    }

    let prefix_len = min(sample.len(), 12);
    crate::log!(
        "crabusb: hid tablet {:04X}:{:04X} packet ep=0x{:02X} len={} nonzero={} bytes={:02X?}\n",
        vendor_id,
        product_id,
        endpoint,
        sample.len(),
        nonzero,
        &sample[..prefix_len]
    );
}
