use super::hid;
use super::hid_descripto as hid_desc;
use super::mass;
use super::xhci::{TrbRing, XhciContext};

pub fn log_mass_non_generic_descriptor_table(port: u8, slot_id: u32, cfg: &[u8]) {
    let Some(pair) = mass::parse_mass_interface(cfg) else {
        return;
    };

    // For BOT mass-storage, the only "extra" per-interface structure we care about is
    // endpoint bundling with the SuperSpeed Endpoint Companion (0x30) for max-burst.
    crate::log!(
        "usb: mass non-generic port={} slot={} if{} cfg={} ep_in=0x{:02X} mps_in={} burst_in={} ep_out=0x{:02X} mps_out={} burst_out={}\n",
        port,
        slot_id,
        pair.interface,
        pair.configuration,
        pair.ep_in,
        pair.max_packet_in,
        pair.ss_max_burst_in,
        pair.ep_out,
        pair.max_packet_out,
        pair.ss_max_burst_out
    );
}

pub async fn log_hid_non_generic_descriptor_tables(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    port: u8,
    cfg: &[u8],
) {
    let mut cur_if: Option<hid_desc::InterfaceDescriptor> = None;
    let mut cur_if_is_hid: bool = false;

    for raw in hid_desc::DescriptorIter::new(cfg) {
        let raw_len = raw.len;
        let raw_bytes = raw.bytes;
        match hid_desc::parse_any_descriptor(raw) {
            hid_desc::ParsedDescriptor::Interface(id) => {
                cur_if = Some(id);
                cur_if_is_hid = id.interface_class == 0x03;
            }
            hid_desc::ParsedDescriptor::Hid(h) => {
                if !cur_if_is_hid {
                    continue;
                }
                let Some(iface) = cur_if else {
                    continue;
                };

                crate::log!(
                    "usb: hid non-generic port={} slot={} if{} alt={} cls={:02X}/{:02X}/{:02X}\n",
                    port,
                    slot_id,
                    iface.interface_number,
                    iface.alternate_setting,
                    iface.interface_class,
                    iface.interface_subclass,
                    iface.interface_protocol
                );
                crate::log!("usb:  ty    len  details\n");
                crate::log!(
                    "usb:  0x21  {:<3} bcd=0x{:04X} country={} numDesc={} repLen={:?}\n",
                    raw_len,
                    h.hid_bcd,
                    h.country_code,
                    h.num_descriptors,
                    h.report_desc_len
                );

                let bytes = raw_bytes;
                let num_desc = h.num_descriptors as usize;
                for j in 0..num_desc {
                    let base = 6 + j * 3;
                    if base + 2 >= bytes.len() {
                        break;
                    }
                    let dt = bytes[base];
                    let dl = u16::from_le_bytes([bytes[base + 1], bytes[base + 2]]);

                    if dt == 0x22 || dt == 0x23 {
                        match hid::fetch_hid_descriptor(
                            ctx,
                            ep0_ring,
                            slot_id,
                            iface.interface_number,
                            dt,
                            dl as usize,
                        )
                        .await
                        {
                            Ok(desc) => {
                                crate::log!(
                                    "usb:  0x{:02X}  {:<3} ok bytes={}\n",
                                    dt,
                                    dl,
                                    desc.len()
                                );
                                let which = if dt == 0x22 {
                                    "Report(0x22)"
                                } else {
                                    "Physical(0x23)"
                                };
                                hid_desc::log_hid_report_like_descriptor_table(
                                    &desc,
                                    port,
                                    slot_id,
                                    iface.interface_number,
                                    which,
                                );
                            }
                            Err(err) => {
                                crate::log!("usb:  0x{:02X}  {:<3} err={:?}\n", dt, dl, err);
                            }
                        }
                    } else {
                        crate::log!("usb:  0x{:02X}  {:<3} (not fetched)\n", dt, dl);
                    }
                }
            }
            _ => {}
        }
    }
}
