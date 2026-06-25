const DMC_MODULE_STRING: &[u8] = b"trueos.fw.dmc";
const HUC_MODULE_STRING: &[u8] = b"trueos.fw.huc.tgl";

pub(crate) fn log_probe_modules(device_id: u16) {
    log_module_probe("dmc", DMC_MODULE_STRING, "adls-probe", device_id);
    log_module_probe("huc", HUC_MODULE_STRING, "adls-rkl-platform-maps-to-tgl", device_id);
}

fn log_module_probe(
    kind: &'static str,
    module_string: &'static [u8],
    source: &'static str,
    device_id: u16,
) {
    match crate::limine::module_bytes_by_string(module_string) {
        Some(bytes) => {
            let (w0, w1, w2, w3) = first_words(bytes);
            if let Some(css) = crate::intel::uc_fw::parse_css(bytes) {
                let (major, minor, patch) = crate::intel::uc_fw::version_triplet(css.sw_version);
                crate::log!(
                    "intel/fw-probe: kind={} present=1 source={} device=0x{:04X} len=0x{:X} sig=0x{:08X} first=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] css_type={} vendor=0x{:04X} header_version=0x{:08X} sw={}.{}.{} raw=0x{:08X} rsa=0x{:X}+0x{:X} action=module-present-only next=auth-load-path\n",
                    kind,
                    source,
                    device_id,
                    bytes.len(),
                    crate::intel::uc_fw::byte_signature(bytes),
                    w0,
                    w1,
                    w2,
                    w3,
                    css.module_type,
                    css.vendor,
                    css.header_version,
                    major,
                    minor,
                    patch,
                    css.sw_version,
                    css.rsa_offset,
                    css.rsa_size
                );
            } else {
                crate::log!(
                    "intel/fw-probe: kind={} present=1 source={} device=0x{:04X} len=0x{:X} sig=0x{:08X} first=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] action=module-present-only next=auth-load-path\n",
                    kind,
                    source,
                    device_id,
                    bytes.len(),
                    crate::intel::uc_fw::byte_signature(bytes),
                    w0,
                    w1,
                    w2,
                    w3
                );
            }
        }
        None => {
            crate::log!(
                "intel/fw-probe: kind={} present=0 source={} device=0x{:04X} action=skip reason=module-missing\n",
                kind,
                source,
                device_id
            );
        }
    }
}

fn first_words(bytes: &[u8]) -> (u32, u32, u32, u32) {
    (
        crate::intel::uc_fw::read_le_u32(bytes, 0),
        crate::intel::uc_fw::read_le_u32(bytes, 4),
        crate::intel::uc_fw::read_le_u32(bytes, 8),
        crate::intel::uc_fw::read_le_u32(bytes, 12),
    )
}
