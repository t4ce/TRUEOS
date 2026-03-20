use crab_usb::usb_if;

const USB_CLASS_MASS_STORAGE: u8 = 0x08;
const USB_SUBCLASS_SCSI: u8 = 0x06;
const USB_PROTO_BULK_ONLY: u8 = 0x50;

#[derive(Copy, Clone, Debug)]
pub(crate) struct MassTarget {
    pub configuration_value: u8,
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub bulk_in: u8,
    pub bulk_out: u8,
    pub bulk_in_max_packet_size: u16,
    pub bulk_out_max_packet_size: u16,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
}

pub(crate) fn pick_mass_target(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Option<MassTarget> {
    let mut best: Option<(u32, MassTarget)> = None;

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                let mut bulk_in = None;
                let mut bulk_out = None;

                for ep in alt.endpoints.iter() {
                    if ep.transfer_type != usb_if::descriptor::EndpointType::Bulk {
                        continue;
                    }
                    match ep.direction {
                        usb_if::transfer::Direction::In if bulk_in.is_none() => {
                            bulk_in = Some((ep.address, ep.max_packet_size));
                        }
                        usb_if::transfer::Direction::Out if bulk_out.is_none() => {
                            bulk_out = Some((ep.address, ep.max_packet_size));
                        }
                        _ => {}
                    }
                }

                let (bulk_in_addr, bulk_in_mps) = bulk_in?;
                let (bulk_out_addr, bulk_out_mps) = bulk_out?;

                let mut score = 10u32;
                if alt.class == USB_CLASS_MASS_STORAGE {
                    score += 100;
                }
                if alt.subclass == USB_SUBCLASS_SCSI {
                    score += 50;
                }
                if alt.protocol == USB_PROTO_BULK_ONLY {
                    score += 50;
                }
                if alt.alternate_setting == 0 {
                    score += 10;
                }
                score += alt.endpoints.len() as u32;

                let target = MassTarget {
                    configuration_value: config.configuration_value,
                    interface_number: interface.interface_number,
                    alternate_setting: alt.alternate_setting,
                    bulk_in: bulk_in_addr,
                    bulk_out: bulk_out_addr,
                    bulk_in_max_packet_size: bulk_in_mps,
                    bulk_out_max_packet_size: bulk_out_mps,
                    class: alt.class,
                    subclass: alt.subclass,
                    protocol: alt.protocol,
                };

                match best {
                    Some((best_score, _)) if best_score >= score => {}
                    _ => best = Some((score, target)),
                }
            }
        }
    }

    best.map(|(_, target)| target).filter(|target| {
        target.class == USB_CLASS_MASS_STORAGE
            && target.subclass == USB_SUBCLASS_SCSI
            && target.protocol == USB_PROTO_BULK_ONLY
    })
}
