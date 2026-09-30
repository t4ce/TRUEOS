//! Descriptor-only CDC ECM selection. Kept independent of USB I/O for testing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EcmTarget {
    pub configuration: u8,
    pub control: u8,
    pub control_alt: u8,
    pub data: u8,
    pub data_alt: u8,
    pub notification: u8,
    pub bulk_in: u8,
    pub bulk_out: u8,
    pub out_packet: u16,
    pub mac_index: u8,
}

pub(super) fn target(raw: &[u8]) -> Option<EcmTarget> {
    if raw.len() < 9 || raw[0] != 9 || raw[1] != 2 {
        return None;
    }
    let total = u16::from_le_bytes([raw[2], raw[3]]) as usize;
    if total != raw.len() {
        return None;
    }
    let mut t = EcmTarget {
        configuration: raw[5],
        control: 0,
        control_alt: 0,
        data: 0,
        data_alt: 0,
        notification: 0,
        bulk_in: 0,
        bulk_out: 0,
        out_packet: 0,
        mac_index: 0,
    };
    let mut control = false;
    let mut found = false;
    let mut union = false;
    let mut pos = 9;
    while pos < total {
        let len = *raw.get(pos)? as usize;
        if len < 2 {
            return None;
        }
        let d = raw.get(pos..pos.checked_add(len)?)?;
        match d[1] {
            4 if len >= 9 => {
                control = d[5..8] == [2, 6, 0];
                if control {
                    if found {
                        return None;
                    } // Do not guess between multiple ECM functions.
                    found = true;
                    t.control = d[2];
                    t.control_alt = d[3];
                }
            }
            0x24 if control && len >= 3 => match d[2] {
                6 if len == 5 && d[3] == t.control => {
                    t.data = d[4];
                    union = true;
                }
                15 if len >= 13 => {
                    t.mac_index = d[3];
                    if u16::from_le_bytes([d[8], d[9]]) < 1514 {
                        return None;
                    }
                }
                _ => {}
            },
            5 if control && len >= 7 && d[2] & 0x80 != 0 && d[3] & 3 == 3 => {
                if u16::from_le_bytes([d[4], d[5]]) < 8 {
                    return None;
                }
                t.notification = d[2];
            }
            _ => {}
        }
        pos += len;
    }
    if !found || !union || t.data == t.control || t.mac_index == 0 || t.notification == 0 {
        return None;
    }
    let mut data = false;
    pos = 9;
    while pos < total {
        let len = raw[pos] as usize;
        let d = &raw[pos..pos + len]; // Lengths validated in the first pass.
        if d[1] == 4 && len >= 9 {
            if t.bulk_in != 0 && t.bulk_out != 0 {
                return Some(t);
            }
            data = d[2] == t.data && d[5..8] == [10, 0, 0];
            t.bulk_in = 0;
            t.bulk_out = 0;
            if data {
                t.data_alt = d[3];
            }
        } else if data && d[1] == 5 && len >= 7 && d[3] & 3 == 2 {
            let packet = u16::from_le_bytes([d[4], d[5]]) & 0x7ff;
            if packet == 0 {
                return None;
            }
            if d[2] & 0x80 != 0 {
                t.bulk_in = d[2];
            } else {
                t.bulk_out = d[2];
                t.out_packet = packet;
            }
        }
        pos += len;
    }
    (t.bulk_in != 0 && t.bulk_out != 0).then_some(t)
}

pub(super) fn mac(text: &str) -> Option<[u8; 6]> {
    if text.len() != 12 || !text.is_ascii() {
        return None;
    }
    let mut out = [0; 6];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    (out != [0; 6] && out[0] & 1 == 0).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut raw = vec![
            9, 2, 0, 0, 2, 2, 0, 0x80, 50, 9, 4, 0, 0, 1, 2, 6, 0, 0, 5, 0x24, 0, 0x10, 1, 5, 0x24,
            6, 0, 1, 13, 0x24, 15, 3, 0, 0, 0, 0, 0xea, 5, 0, 0, 0, 7, 5, 0x83, 3, 16, 0, 8, 9, 4,
            1, 0, 0, 10, 0, 0, 0, 9, 4, 1, 1, 2, 10, 0, 0, 0, 7, 5, 0x81, 2, 0, 2, 0, 7, 5, 0x02,
            2, 0, 2, 0,
        ];
        let len = (raw.len() as u16).to_le_bytes();
        raw[2..4].copy_from_slice(&len);
        raw
    }
    #[test]
    fn selects_ecm_data_alternate_and_functional_mac() {
        assert_eq!(
            target(&fixture()),
            Some(EcmTarget {
                configuration: 2,
                control: 0,
                control_alt: 0,
                data: 1,
                data_alt: 1,
                notification: 0x83,
                bulk_in: 0x81,
                bulk_out: 2,
                out_packet: 512,
                mac_index: 3,
            })
        );
    }
    #[test]
    fn rejects_truncated_and_zero_length_descriptors() {
        let raw = fixture();
        for end in 0..raw.len() {
            assert!(target(&raw[..end]).is_none());
        }
        let mut raw = raw;
        raw[18] = 0;
        assert!(target(&raw).is_none());
    }
    #[test]
    fn requires_union_to_match_data_and_ethernet_class() {
        let mut raw = fixture();
        raw[27] = 7; // Union's data interface is absent.
        assert!(target(&raw).is_none());
        let mut raw = fixture();
        raw[14] = 0xff;
        assert!(target(&raw).is_none());
    }
    #[test]
    fn validates_factory_mac() {
        assert_eq!(mac("001122aAbBcC"), Some([0, 0x11, 0x22, 0xaa, 0xbb, 0xcc]));
        for bad in [
            "",
            "000000000000",
            "FFFFFFFFFFFF",
            "011122334455",
            "00112233445Z",
            "00:11:22:33:44:55",
            "é1122334455",
        ] {
            assert_eq!(mac(bad), None);
        }
    }
}
