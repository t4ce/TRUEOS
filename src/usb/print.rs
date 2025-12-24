use crate::debugconf;

const USB_DESC_TYPE_INTERFACE: u8 = 4;
const USB_CLASS_PRINTER: u8 = 0x07;

pub fn try_handle(cfg: &[u8], port: u8) -> bool {
    if has_interface_class(cfg, USB_CLASS_PRINTER) {
        debugconf!("usb: printer detected on port {}\n", port);
        true
    } else {
        false
    }
}

fn has_interface_class(cfg: &[u8], class_code: u8) -> bool {
    let mut idx = 0usize;
    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        if cfg[idx + 1] == USB_DESC_TYPE_INTERFACE && len >= 9 {
            let iface_class = cfg[idx + 5];
            if iface_class == class_code {
                return true;
            }
        }
        idx += len;
    }
    false
}
