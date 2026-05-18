const DMC_MODULE_STRING: &[u8] = b"trueos.fw.dmc";
const HUC_CANDIDATE_MODULE_STRING: &[u8] = b"trueos.fw.huc.candidate.tgl";

pub(crate) fn log_probe_modules(device_id: u16) {
    log_module_probe("dmc", DMC_MODULE_STRING, "adls-probe", device_id);
    log_module_probe("huc", HUC_CANDIDATE_MODULE_STRING, "tgl-candidate-probe-only", device_id);
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
            crate::log!(
                "intel/fw-probe: kind={} present=1 source={} device=0x{:04X} len=0x{:X} sig=0x{:08X} first=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] action=module-present-only next=auth-load-path\n",
                kind,
                source,
                device_id,
                bytes.len(),
                byte_signature(bytes),
                w0,
                w1,
                w2,
                w3
            );
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
    (read_le_u32(bytes, 0), read_le_u32(bytes, 4), read_le_u32(bytes, 8), read_le_u32(bytes, 12))
}

fn read_le_u32(bytes: &[u8], offset: usize) -> u32 {
    let mut out = 0u32;
    let mut i = 0usize;
    while i < 4 {
        if let Some(byte) = bytes.get(offset + i) {
            out |= (*byte as u32) << (i * 8);
        }
        i += 1;
    }
    out
}

fn byte_signature(bytes: &[u8]) -> u32 {
    let mut sig = 0x811C_9DC5u32;
    for byte in bytes {
        sig ^= *byte as u32;
        sig = sig.wrapping_mul(0x0100_0193);
    }
    sig
}
