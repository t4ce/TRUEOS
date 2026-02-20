// Common CDC descriptor helpers shared by CDC class drivers (e.g. ACM).
// Provides parsing of a CDC-ACM interface and minimal endpoint metadata.

#[derive(Clone, Copy, Debug)]
pub struct EndpointInfo {
    pub address: u8,
    pub max_packet: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct CdcInterface {
    pub configuration: u8,
    pub control_interface: u8,
    pub data_interface: u8,
    pub ep_in: EndpointInfo,
    pub ep_out: EndpointInfo,
}

pub const USB_CLASS_COMM: u8 = 0x02;
pub const USB_SUBCLASS_ACM: u8 = 0x02;
pub const USB_CLASS_DATA: u8 = 0x0A;

/// Parse a CDC-ACM interface (control + data) from a configuration descriptor.
/// Returns endpoint and interface metadata for bulk IN/OUT data pipes.
pub fn parse_cdc_interface(cfg: &[u8]) -> Option<CdcInterface> {
    let mut idx = 0usize;
    let mut config_value: u8 = 1;
    let mut current_iface: Option<u8> = None;
    let mut current_class: u8 = 0;
    let mut data_iface: Option<u8> = None;
    let mut data_alt: u8 = 0;
    let mut control_iface: Option<u8> = None;
    let mut ep_in: Option<EndpointInfo> = None;
    let mut ep_out: Option<EndpointInfo> = None;

    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        let ty = cfg[idx + 1];
        match ty {
            2 => {
                if len >= 6 {
                    config_value = cfg[idx + 5];
                }
            }
            4 => {
                if len >= 9 {
                    let iface = cfg[idx + 2];
                    current_iface = Some(iface);
                    current_class = cfg[idx + 5];
                    let subclass = cfg[idx + 6];
                    let protocol = cfg[idx + 7];
                    if current_class == USB_CLASS_COMM && subclass == USB_SUBCLASS_ACM {
                        control_iface = Some(iface);
                    } else if current_class == USB_CLASS_DATA {
                        data_iface = Some(iface);
                        data_alt = cfg[idx + 3];
                        let _ = protocol;
                        ep_in = None;
                        ep_out = None;
                    } else {
                        data_iface = None;
                    }
                } else {
                    current_iface = None;
                }
            }
            5 => {
                if let (Some(iface), Some(data_if)) = (current_iface, data_iface)
                    && iface == data_if && data_alt == 0 && current_class == USB_CLASS_DATA
                        && len >= 7 {
                            let attrs = cfg[idx + 3];
                            if (attrs & 0x3) == 0x2 {
                                let ep_addr = cfg[idx + 2];
                                let max_packet = u16::from_le_bytes([cfg[idx + 4], cfg[idx + 5]]);
                                if (ep_addr & 0x80) != 0 {
                                    if ep_in.is_none() {
                                        ep_in = Some(EndpointInfo {
                                            address: ep_addr,
                                            max_packet,
                                        });
                                    }
                                } else if ep_out.is_none() {
                                    ep_out = Some(EndpointInfo {
                                        address: ep_addr,
                                        max_packet,
                                    });
                                }
                                if let (Some(ctrl), Some(in_ep), Some(out_ep)) =
                                    (control_iface, ep_in, ep_out)
                                {
                                    return Some(CdcInterface {
                                        configuration: config_value,
                                        control_interface: ctrl,
                                        data_interface: data_if,
                                        ep_in: in_ep,
                                        ep_out: out_ep,
                                    });
                                }
                            }
                        }
            }
            _ => {}
        }
        idx += len;
    }

    None
}
