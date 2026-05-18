#[derive(Copy, Clone, Debug)]
pub(crate) struct CssInfo {
    pub(crate) offset: usize,
    pub(crate) module_type: u32,
    pub(crate) header_size_dw: u32,
    pub(crate) header_version: u32,
    pub(crate) module_id: u32,
    pub(crate) vendor: u32,
    pub(crate) date: u32,
    pub(crate) size_dw: u32,
    pub(crate) key_size_dw: u32,
    pub(crate) modulus_size_dw: u32,
    pub(crate) exponent_size_dw: u32,
    pub(crate) sw_version: u32,
    pub(crate) vf_version: u32,
    pub(crate) private_data_size: u32,
    pub(crate) xfer_len: usize,
    pub(crate) rsa_offset: usize,
    pub(crate) rsa_size: usize,
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct UcCssHeader {
    module_type: u32,
    header_size_dw: u32,
    header_version: u32,
    module_id: u32,
    vendor: u32,
    date: u32,
    size_dw: u32,
    key_size_dw: u32,
    modulus_size_dw: u32,
    exponent_size_dw: u32,
    _time: u32,
    _user: [u8; 8],
    _build: [u8; 12],
    sw_version: u32,
    vf_version: u32,
    _r: [u32; 12],
    private_data_size: u32,
    _info: u32,
}

pub(crate) fn parse_css(blob: &[u8]) -> Option<CssInfo> {
    let end = blob
        .len()
        .checked_sub(core::mem::size_of::<UcCssHeader>())?;
    for off in (0..=end).step_by(4) {
        let css = unsafe { (blob.as_ptr().add(off) as *const UcCssHeader).read_unaligned() };
        if css.module_type != 5 && css.module_type != 6 {
            continue;
        }
        let fixed = css
            .header_size_dw
            .checked_sub(css.key_size_dw)?
            .checked_sub(css.modulus_size_dw)?
            .checked_sub(css.exponent_size_dw)?;
        if fixed != 32 {
            continue;
        }
        let header = css.header_size_dw.checked_mul(4)? as usize;
        let total = css.size_dw.checked_mul(4)? as usize;
        let xfer_len = 128usize.checked_add(total.checked_sub(header)?)?;
        let rsa_size = css.key_size_dw.checked_mul(4)? as usize;
        let rsa_offset = off.checked_add(xfer_len)?;
        if off.checked_add(xfer_len)? <= blob.len()
            && rsa_offset.checked_add(rsa_size)? <= blob.len()
        {
            return Some(CssInfo {
                offset: off,
                module_type: css.module_type,
                header_size_dw: css.header_size_dw,
                header_version: css.header_version,
                module_id: css.module_id,
                vendor: css.vendor,
                date: css.date,
                size_dw: css.size_dw,
                key_size_dw: css.key_size_dw,
                modulus_size_dw: css.modulus_size_dw,
                exponent_size_dw: css.exponent_size_dw,
                sw_version: css.sw_version,
                vf_version: css.vf_version,
                private_data_size: css.private_data_size,
                xfer_len,
                rsa_offset,
                rsa_size,
            });
        }
    }
    None
}

pub(crate) fn version_triplet(sw_version: u32) -> (u32, u32, u32) {
    ((sw_version >> 16) & 0xFF, (sw_version >> 8) & 0xFF, sw_version & 0xFF)
}

pub(crate) fn read_le_u32(bytes: &[u8], offset: usize) -> u32 {
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

pub(crate) fn byte_signature(bytes: &[u8]) -> u32 {
    let mut sig = 0x811C_9DC5u32;
    for byte in bytes {
        sig ^= *byte as u32;
        sig = sig.wrapping_mul(0x0100_0193);
    }
    sig
}
