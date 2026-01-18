use super::{TrbRing, XhciContext};

pub const USB_CLASS_HUB: u8 = 0x09;
pub const USB_SUBCLASS_HUB: u8 = 0x00;

pub struct AttachParams<'a> {
    pub ctx: &'a XhciContext,
    pub ep0_ring: &'a mut TrbRing,
    pub slot_id: u32,
    pub cfg: &'a [u8],
    pub target_port: u8,
}

pub fn is_hub_device(dev_cls: u8, dev_sub: u8, _dev_prot: u8, cfg: &[u8]) -> bool {
    if dev_cls == USB_CLASS_HUB {
        return true;
    }

    // Composite device: look for a hub interface.
    let mut idx = 0usize;
    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        let ty = cfg[idx + 1];
        if ty == 4 && len >= 9 {
            let if_cls = cfg[idx + 5];
            let if_sub = cfg[idx + 6];
            if if_cls == USB_CLASS_HUB && if_sub == USB_SUBCLASS_HUB {
                return true;
            }
        }
        idx += len;
    }

    false
}

pub async fn attach_device(params: AttachParams<'_>) -> Result<(), ()> {
    let AttachParams {
        ctx: _,
        ep0_ring: _,
        slot_id,
        cfg: _,
        target_port,
    } = params;

    // TODO: Issue hub class requests, read hub descriptor, and enumerate downstream ports.
    crate::log!(
        "usb: hub claimed slot={} port={} (downstream scan not implemented)\n",
        slot_id,
        target_port
    );

    Ok(())
}
