//! AX201 QuZ firmware and defensive Intel TLV decoding.
//!
//! Wire format reference: Linux v6.12 iwlwifi/fw/file.h and iwl-drv.c.
//! Firmware remains immutable and section payloads borrow the embedded asset;
//! no firmware instructions are decoded or modified here.

use alloc::vec::Vec;

pub(crate) const EMBEDDED: &[u8] =
    include_bytes!("../../firmware/iwlwifi/iwlwifi-QuZ-a0-hr-b0-77.ucode");
const HEADER_BYTES: usize = 88;
const MAGIC: u32 = 0x0a4c_5749;
const CPU_SEPARATOR: u32 = 0xffff_cccc;
const PAGING_SEPARATOR: u32 = 0xaaaa_bbbb;
const MAX_SECTIONS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SectionPart {
    Lmac,
    Umac,
    Paging,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Section<'a> {
    pub offset: u32,
    pub data: &'a [u8],
    pub part: SectionPart,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Version {
    pub major: u32,
    pub minor: u32,
    pub local: u32,
}

#[derive(Debug)]
pub(crate) struct Firmware<'a> {
    pub runtime: Vec<Section<'a>>,
    pub num_cpus: u32,
    pub paging_bytes: u32,
    pub version: Version,
    pub flags: u32,
    pub phy_config: u32,
    /// Runtime calibration [flow_trigger, event_trigger], TLV22 ucode_type=0.
    pub default_calib_runtime: Option<[u32; 2]>,
    api: Vec<(u32, u32)>,
    capabilities: Vec<(u32, u32)>,
    command_versions: &'a [u8],
}

impl Firmware<'_> {
    pub fn has_capability(&self, bit: u32) -> bool {
        flag(&self.capabilities, bit)
    }
    pub fn has_api(&self, bit: u32) -> bool {
        flag(&self.api, bit)
    }
    pub fn notification_version(&self, group: u8, command: u8) -> Option<u8> {
        self.command_versions
            .chunks_exact(4)
            .find(|entry| entry[0] == command && entry[1] == group)
            .map(|entry| entry[3])
    }
    pub fn command_version(&self, group: u8, command: u8) -> Option<u8> {
        self.command_versions
            .chunks_exact(4)
            .find(|entry| entry[0] == command && entry[1] == group)
            .map(|entry| entry[2])
    }
}

fn flag(words: &[(u32, u32)], bit: u32) -> bool {
    words
        .iter()
        .any(|&(index, flags)| index == bit / 32 && flags & (1 << (bit % 32)) != 0)
}

fn word(bytes: &[u8], offset: usize) -> Result<u32, &'static str> {
    let end = offset.checked_add(4).ok_or("AX201 TLV offset overflow")?;
    let bytes = bytes.get(offset..end).ok_or("AX201 truncated TLV value")?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn scalar(data: &[u8]) -> Result<u32, &'static str> {
    if data.len() != 4 {
        return Err("AX201 invalid scalar TLV length");
    }
    word(data, 0)
}

/// Parse the unified MVM runtime image. CPU and paging separators classify
/// sections but are never returned as DMA payloads. Paging starts with its CSS
/// authentication section; retain it separately from pages when building DMA.
pub(crate) fn parse(bytes: &[u8]) -> Result<Firmware<'_>, &'static str> {
    if bytes.len() < HEADER_BYTES || word(bytes, 0)? != 0 || word(bytes, 4)? != MAGIC {
        return Err("AX201 invalid TLV firmware header");
    }
    let mut firmware = Firmware {
        runtime: Vec::new(),
        num_cpus: 1,
        paging_bytes: 0,
        version: Version {
            major: word(bytes, 72)?,
            minor: word(bytes, 76)?,
            local: 0,
        },
        flags: 0,
        phy_config: 0,
        default_calib_runtime: None,
        api: Vec::new(),
        capabilities: Vec::new(),
        command_versions: &[],
    };
    let mut part = SectionPart::Lmac;
    let mut cpu_separator = false;
    let mut paging_separator = false;
    let mut cursor = HEADER_BYTES;
    while cursor < bytes.len() {
        let kind = word(bytes, cursor)?;
        let length = word(bytes, cursor.checked_add(4).ok_or("AX201 TLV overflow")?)? as usize;
        let start = cursor.checked_add(8).ok_or("AX201 TLV overflow")?;
        let end = start.checked_add(length).ok_or("AX201 TLV overflow")?;
        let data = bytes.get(start..end).ok_or("AX201 truncated TLV payload")?;
        cursor = end
            .checked_add((4 - length % 4) % 4)
            .ok_or("AX201 TLV overflow")?;
        if cursor > bytes.len() {
            return Err("AX201 truncated TLV padding");
        }
        match kind {
            19 | 24 => {
                let offset = word(data, 0)?;
                if offset == CPU_SEPARATOR {
                    if cpu_separator || paging_separator {
                        return Err("AX201 invalid CPU separator");
                    }
                    cpu_separator = true;
                    part = SectionPart::Umac;
                } else if offset == PAGING_SEPARATOR {
                    if paging_separator {
                        return Err("AX201 duplicate paging separator");
                    }
                    paging_separator = true;
                    part = SectionPart::Paging;
                } else {
                    let payload = &data[4..];
                    if payload.is_empty() {
                        return Err("AX201 empty firmware section");
                    }
                    if firmware.runtime.len() >= MAX_SECTIONS {
                        return Err("AX201 excessive firmware sections");
                    }
                    if payload.len() > u32::MAX as usize
                        || offset.checked_add(payload.len() as u32).is_none()
                    {
                        return Err("AX201 firmware section address overflow");
                    }
                    firmware.runtime.push(Section {
                        offset,
                        data: payload,
                        part,
                    });
                }
            }
            18 => {
                // FLAGS may include extension words; Linux consumes the first.
                if data.is_empty() || data.len() % 4 != 0 {
                    return Err("AX201 invalid flags TLV");
                }
                firmware.flags = word(data, 0)?;
            }
            22 => {
                if data.len() != 12 {
                    return Err("AX201 invalid default calibration TLV");
                }
                let image = word(data, 0)?;
                if image > 3 {
                    return Err("AX201 invalid calibration image type");
                }
                if image == 0 {
                    firmware.default_calib_runtime = Some([word(data, 4)?, word(data, 8)?]);
                }
            }
            23 => firmware.phy_config = scalar(data)?,
            27 => {
                firmware.num_cpus = scalar(data)?;
                if !(1..=2).contains(&firmware.num_cpus) {
                    return Err("AX201 unsupported firmware CPU count");
                }
            }
            29 | 30 => {
                if data.len() != 8 {
                    return Err("AX201 invalid bitmap TLV");
                }
                let entry = (word(data, 0)?, word(data, 4)?);
                let list = if kind == 29 {
                    &mut firmware.api
                } else {
                    &mut firmware.capabilities
                };
                if list.len() >= 128 {
                    return Err("AX201 excessive firmware bitmaps");
                }
                list.push(entry);
            }
            32 => {
                firmware.paging_bytes = scalar(data)?;
                if firmware.paging_bytes > 1024 * 1024 || firmware.paging_bytes % 4096 != 0 {
                    return Err("AX201 invalid paging image size");
                }
            }
            36 => {
                if data.len() != 12 {
                    return Err("AX201 invalid version TLV");
                }
                firmware.version = Version {
                    major: word(data, 0)?,
                    minor: word(data, 4)?,
                    local: word(data, 8)?,
                };
            }
            48 => {
                if data.len() % 4 != 0 {
                    return Err("AX201 invalid command versions");
                }
                firmware.command_versions = data;
            }
            // Debug, calibration, init/WoWLAN and future TLVs do not become
            // runtime image sections. Their framing is still validated above.
            _ => {}
        }
    }
    if !firmware.runtime.iter().any(|s| s.part == SectionPart::Lmac) {
        return Err("AX201 runtime LMAC image missing");
    }
    if firmware.num_cpus == 2
        && (!cpu_separator || !firmware.runtime.iter().any(|s| s.part == SectionPart::Umac))
    {
        return Err("AX201 runtime UMAC image missing");
    }
    if firmware.paging_bytes != 0 {
        let mut paging = firmware
            .runtime
            .iter()
            .filter(|s| s.part == SectionPart::Paging);
        let css = paging.next().ok_or("AX201 paging CSS missing")?;
        if css.data.is_empty() {
            return Err("AX201 paging CSS empty");
        }
        let size = paging
            .try_fold(0usize, |sum, s| sum.checked_add(s.data.len()))
            .ok_or("AX201 paging size overflow")?;
        if size != firmware.paging_bytes as usize {
            return Err("AX201 paging data size mismatch");
        }
    } else if paging_separator {
        return Err("AX201 paging data without size");
    }
    Ok(firmware)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn header() -> Vec<u8> {
        let mut bytes = vec![0; HEADER_BYTES];
        bytes[4..8].copy_from_slice(&MAGIC.to_le_bytes());
        bytes
    }
    fn tlv(bytes: &mut Vec<u8>, kind: u32, data: &[u8]) {
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(data);
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
    }
    fn section(bytes: &mut Vec<u8>, offset: u32, data: &[u8]) {
        let mut payload = offset.to_le_bytes().to_vec();
        payload.extend_from_slice(data);
        tlv(bytes, 19, &payload);
    }

    #[test]
    fn shipped_image_keeps_upload_groups_and_metadata() {
        let fw = parse(EMBEDDED).unwrap();
        assert_eq!(
            fw.version,
            Version {
                major: 77,
                minor: 0xf39c_c7f9,
                local: 0
            }
        );
        assert_eq!(fw.num_cpus, 2);
        assert_eq!(fw.default_calib_runtime, Some([0x5f16_01d3, 0x5b06_958b]));
        assert_eq!(fw.paging_bytes, 0x97000);
        assert!(fw.runtime.iter().any(|s| s.part == SectionPart::Lmac));
        assert!(fw.runtime.iter().any(|s| s.part == SectionPart::Umac));
        assert_eq!(fw.command_version(0, 1), Some(99));
        assert!(
            fw.runtime
                .iter()
                .all(|s| s.offset != CPU_SEPARATOR && s.offset != PAGING_SEPARATOR)
        );
    }
    #[test]
    fn truncated_payload_and_padding_are_rejected() {
        let mut bytes = header();
        section(&mut bytes, 0x100, &[1, 2, 3]);
        assert!(parse(&bytes).is_ok());
        bytes.pop();
        assert!(parse(&bytes).is_err());
        bytes.truncate(HEADER_BYTES + 9);
        assert!(parse(&bytes).is_err());
    }
    #[test]
    fn duplicate_or_missing_cpu_delimiter_cannot_make_dual_cpu_image() {
        let mut bytes = header();
        tlv(&mut bytes, 27, &2u32.to_le_bytes());
        section(&mut bytes, 0x100, &[1]);
        assert!(parse(&bytes).is_err());
        section(&mut bytes, CPU_SEPARATOR, &[0; 4]);
        section(&mut bytes, 0x200, &[2]);
        assert!(parse(&bytes).is_ok());
        section(&mut bytes, CPU_SEPARATOR, &[0; 4]);
        assert!(parse(&bytes).is_err());
    }
    #[test]
    fn overflowing_section_address_and_wrong_paging_size_are_rejected() {
        let mut bytes = header();
        section(&mut bytes, u32::MAX - 1, &[1, 2, 3]);
        assert!(parse(&bytes).is_err());
        let mut bytes = header();
        section(&mut bytes, 0x100, &[1]);
        tlv(&mut bytes, 32, &4096u32.to_le_bytes());
        section(&mut bytes, PAGING_SEPARATOR, &[0; 4]);
        section(&mut bytes, 0, &[1]);
        section(&mut bytes, 0x1000, &[2; 4095]);
        assert!(parse(&bytes).is_err());
    }
    #[test]
    fn bitmap_and_command_version_lookup_follow_wire_ids() {
        let mut bytes = header();
        section(&mut bytes, 0x100, &[1]);
        tlv(&mut bytes, 30, &[1, 0, 0, 0, 4, 0, 0, 0]);
        tlv(&mut bytes, 48, &[0x42, 3, 7, 9]);
        let fw = parse(&bytes).unwrap();
        assert!(fw.has_capability(34));
        assert!(!fw.has_capability(2));
        assert!(!fw.has_api(34));
        assert_eq!(fw.command_version(3, 0x42), Some(7));
        assert_eq!(fw.notification_version(3, 0x42), Some(9));
        assert_eq!(fw.command_version(0x42, 3), None);
    }
}
