use alloc::vec::Vec;

pub(crate) fn publish_host_advertisement(advertisement: trueos_esp::gate::TrueOsHostAdvertisement) {
    let _ = advertisement;
}

pub(crate) fn take_peer_advertisement() -> Option<Vec<u8>> {
    None
}
