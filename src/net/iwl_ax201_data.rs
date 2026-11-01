//! AX201 (22000) raw MPDU, Ethernet framing, and TX completion wire formats.
//!
//! Descriptor contracts: Linux v6.14 iwlwifi/fw/api/{rx,tx,rs}.h and
//! mvm/{rxmq,tx,utils}.c. CCMP is deliberately left to the station's software
//! crypto owner: receive does not remove its IV/MIC or claim authentication.

use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub(crate) struct RawFrame {
    pub frame: Vec<u8>,
    pub channel: u8,
    pub signal_dbm: i8,
    pub protected: bool,
    pub hw_decrypted: bool,
    pub device_timestamp: u32,
}

/// Header layouts supported by the non-aggregating station path. Four-address,
/// HT-control, null-data, and aggregated subframes need distinct ownership.
pub(crate) fn header_len(frame: &[u8]) -> Result<usize, &'static str> {
    if frame.len() < 24 {
        return Err("AX201 short 802.11 header");
    }
    let fc = u16::from_le_bytes([frame[0], frame[1]]);
    if fc & 3 != 0 || fc & 0x0300 == 0x0300 || fc & 0x8000 != 0 {
        return Err("AX201 unsupported 802.11 header");
    }
    let len = match fc & 0x00fc {
        0x0088 => 26,                           // QoS data, no HT control.
        subtype if subtype & 0x000c == 0 => 24, // Management.
        0x0008 => 24,
        _ => return Err("AX201 unsupported 802.11 frame type"),
    };
    if frame.len() < len {
        return Err("AX201 short QoS header");
    }
    if len == 26 && frame[24] & 0x80 != 0 {
        return Err("AX201 A-MSDU not supported");
    }
    if fc & 0x0400 != 0 || frame[22] & 0x0f != 0 {
        return Err("AX201 fragmented MPDU not supported");
    }
    Ok(len)
}

/// RX_MPDU_RES_START_API_S_VER_3/4 on pre-AX210 uses the 48-byte descriptor.
/// Bad CRC/overrun is an ordinary drop; malformed lengths are errors. Padding
/// is removed after the MAC header/CCMP IV as described by mvm/rxmq.c.
pub(crate) fn rx_mpdu(payload: &[u8]) -> Result<Option<RawFrame>, &'static str> {
    const DESC: usize = 48;
    if payload.len() < DESC {
        return Err("AX201 short RX descriptor");
    }
    let length = u16::from_le_bytes([payload[0], payload[1]]) as usize;
    let status = u32::from_le_bytes(payload[12..16].try_into().unwrap());
    if status & 3 != 3 {
        return Ok(None);
    }
    if payload[3] & 0x40 != 0 {
        return Ok(None);
    } // A-MSDU requires deaggregation.
    let packet = payload
        .get(DESC..DESC + length)
        .ok_or("AX201 RX MPDU outside descriptor")?;
    let header = match header_len(packet) {
        Ok(len) => len,
        Err(_) => return Ok(None),
    };
    let protected = packet[1] & 0x40 != 0;
    let hw_decrypted = status & (1 << 11) != 0;
    let trailer = usize::from(payload[2] >> 4) * 2;
    // With software CCMP, firmware must leave the encrypted MIC in the MPDU.
    // Only the independently reported FCS (0 or 4 bytes) may be removed here.
    if !hw_decrypted && !matches!(trailer, 0 | 4) {
        return Err("AX201 RX unexpected software-crypto trailer");
    }
    let pad = if payload[3] & 0x20 != 0 { 2 } else { 0 };
    let crypt_header = if protected && pad != 0 {
        // The software path supports CCMP's eight-byte ExtIV only. With a
        // hardware-decrypted packet the caller will reject software decryption.
        let iv = packet
            .get(header..header + 8)
            .ok_or("AX201 short RX CCMP IV")?;
        if iv[3] & 0x20 == 0 {
            return Err("AX201 RX unsupported protected padding");
        }
        8
    } else {
        0
    };
    let split = header + crypt_header;
    if length < split + pad + trailer {
        return Err("AX201 RX trailer exceeds MPDU");
    }
    let end = length - trailer;
    let mut frame = Vec::with_capacity(end - pad);
    frame.extend_from_slice(&packet[..split]);
    frame.extend_from_slice(&packet[split + pad..end]);
    let channel = payload[34];
    if !(1..=14).contains(&channel) && !(36..=181).contains(&channel) {
        return Err("AX201 RX invalid channel");
    }
    let energy = [payload[32], payload[33]]
        .into_iter()
        .filter(|&v| v != 0)
        .min()
        .unwrap_or(127);
    Ok(Some(RawFrame {
        frame,
        channel,
        signal_dbm: -(energy.min(127) as i8),
        protected,
        hw_decrypted,
        device_timestamp: u32::from_le_bytes(payload[36..40].try_into().unwrap()),
    }))
}

/// Convert an Ethernet II frame into a three-address station data MPDU. The
/// station's CCMP owner must protect it before TX once the port is secured.
pub(crate) fn encapsulate_ethernet(
    ethernet: &[u8],
    local: [u8; 6],
    bssid: [u8; 6],
    sequence: u16,
) -> Result<Vec<u8>, &'static str> {
    if !(14..=1514).contains(&ethernet.len()) || ethernet[6..12] != local {
        return Err("AX201 invalid station Ethernet frame");
    }
    if u16::from_be_bytes([ethernet[12], ethernet[13]]) < 0x0600 {
        return Err("AX201 Ethernet length framing unsupported");
    }
    let mut frame = alloc::vec![0; 24];
    frame[0..2].copy_from_slice(&0x0108u16.to_le_bytes()); // Data, ToDS.
    frame[4..10].copy_from_slice(&bssid);
    frame[10..16].copy_from_slice(&local);
    frame[16..22].copy_from_slice(&ethernet[..6]);
    frame[22..24].copy_from_slice(&((sequence & 0x0fff) << 4).to_le_bytes());
    frame.extend_from_slice(&[0xaa, 0xaa, 3, 0, 0, 0]);
    frame.extend_from_slice(&ethernet[12..]);
    Ok(frame)
}

/// Only cleartext/authenticated-decrypted ordinary FromDS data may enter the
/// Ethernet stack. Management, protected ciphertext and foreign BSSs stay out.
pub(crate) fn decapsulate_ethernet(
    frame: &[u8],
    local: [u8; 6],
    bssid: [u8; 6],
) -> Result<Vec<u8>, &'static str> {
    let hdr = header_len(frame)?;
    let fc = u16::from_le_bytes([frame[0], frame[1]]);
    if fc & 0x030c != 0x0208 || fc & 0x4000 != 0 || frame[10..16] != bssid {
        return Err("AX201 MPDU is not authenticated station data");
    }
    if frame[4..10] != local && frame[4] & 1 == 0 {
        return Err("AX201 MPDU for another station");
    }
    let llc = frame
        .get(hdr..hdr + 8)
        .ok_or("AX201 short LLC/SNAP header")?;
    if llc[..6] != [0xaa, 0xaa, 3, 0, 0, 0] || u16::from_be_bytes([llc[6], llc[7]]) < 0x0600 {
        return Err("AX201 unsupported LLC/SNAP");
    }
    if frame.len() - hdr - 8 > 1500 {
        return Err("AX201 Ethernet payload too large");
    }
    let mut ethernet = Vec::with_capacity(frame.len() - hdr + 6);
    ethernet.extend_from_slice(&frame[4..10]);
    ethernet.extend_from_slice(&frame[16..22]);
    ethernet.extend_from_slice(&llc[6..8]);
    ethernet.extend_from_slice(&frame[hdr + 8..]);
    Ok(ethernet)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TxReport {
    /// Sequence token returned by send_raw80211(), carried in agg_tx_status.
    pub sequence: u16,
    pub queue: u16,
    /// First descriptor not owned by the device anymore, modulo 256 on AX201.
    pub next_index: u8,
    pub success: bool,
    pub status: u16,
}

/// API77 advertises LONG_GROUP TX notification v7. Non-aggregate only: the
/// firmware's per-frame status and trailing scheduler SSN prove retirement.
pub(crate) fn tx_report(payload: &[u8]) -> Result<TxReport, &'static str> {
    if payload.len() != 48 || payload[0] != 1 {
        return Err("AX201 unsupported TX response");
    }
    let status = u16::from_le_bytes([payload[40], payload[41]]);
    let queue = u16::from_le_bytes([payload[36], payload[37]]);
    if queue == 0 || queue >= 512 {
        return Err("AX201 invalid TX response queue");
    }
    let ssn = u32::from_le_bytes(payload[44..48].try_into().unwrap()) & 0x0fff;
    Ok(TxReport {
        sequence: u16::from_le_bytes([payload[42], payload[43]]),
        queue,
        next_index: ssn as u8,
        success: matches!(status & 0xff, 1 | 2),
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const LOCAL: [u8; 6] = [2, 3, 4, 5, 6, 7];
    const AP: [u8; 6] = [2, 8, 9, 10, 11, 12];
    #[test]
    fn ethernet_encapsulation_and_strict_bss_decapsulation() {
        let ethernet = [LOCAL.as_slice(), AP.as_slice(), &[8, 0, 1, 2, 3]].concat();
        let mut frame = encapsulate_ethernet(&ethernet, AP, LOCAL, 0x123).unwrap();
        assert_eq!(&frame[22..24], &[0x30, 0x12]);
        // A ToDS uplink cannot accidentally be fed back to our IP stack.
        assert!(decapsulate_ethernet(&frame, AP, LOCAL).is_err());
        frame[1] = 2;
        frame[4..10].copy_from_slice(&LOCAL);
        frame[10..16].copy_from_slice(&AP);
        frame[16..22].copy_from_slice(&AP);
        let received = decapsulate_ethernet(&frame, LOCAL, AP).unwrap();
        assert_eq!(received, ethernet);
        frame[1] |= 0x40;
        assert!(decapsulate_ethernet(&frame, LOCAL, AP).is_err());
    }
    #[test]
    fn rx_ciphertext_keeps_ccmp_mic_and_removes_only_reported_fcs() {
        let mut packet = alloc::vec![0; 48];
        let mut frame = alloc::vec![0; 24];
        frame[0] = 8;
        frame[1] = 0x42;
        frame.extend_from_slice(&[1, 2, 0, 0x20, 0, 0, 0, 0]);
        frame.extend_from_slice(&[4; 24]); // Ciphertext and MIC, untouched.
        let expected = frame.clone();
        frame.extend_from_slice(&[0; 4]);
        packet[..2].copy_from_slice(&(frame.len() as u16).to_le_bytes());
        packet[2] = 0x20;
        packet[12] = 3;
        packet[32] = 40;
        packet[34] = 6;
        packet.extend_from_slice(&frame);
        let raw = rx_mpdu(&packet).unwrap().unwrap();
        assert_eq!(raw.frame, expected);
        assert!(raw.protected);
        assert!(!raw.hw_decrypted);
        packet[12] = 0;
        assert!(rx_mpdu(&packet).unwrap().is_none());
    }
    #[test]
    fn rx_qos_padding_is_removed_after_ccmp_iv() {
        let mut frame = alloc::vec![0; 26];
        frame[0] = 0x88;
        frame[1] = 0x42;
        frame.extend_from_slice(&[1, 2, 0, 0x20, 0, 0, 0, 0]);
        frame.extend_from_slice(&[3; 24]);
        let mut packet = alloc::vec![0; 48];
        packet[2] = 0x20;
        packet[3] = 0x20;
        packet[12] = 3;
        packet[32] = 45;
        packet[34] = 36;
        packet[36..40].copy_from_slice(&0x12345678u32.to_le_bytes());
        packet.extend_from_slice(&frame[..34]);
        packet.extend_from_slice(&[0, 0]);
        packet.extend_from_slice(&frame[34..]);
        packet.extend_from_slice(&[0; 4]);
        let size = packet.len() - 48;
        packet[..2].copy_from_slice(&(size as u16).to_le_bytes());
        let raw = rx_mpdu(&packet).unwrap().unwrap();
        assert_eq!(raw.frame, frame);
        assert_eq!(raw.device_timestamp, 0x12345678);
        assert_eq!(raw.channel, 36);
        assert_eq!(raw.signal_dbm, -45);
        packet.pop();
        assert!(rx_mpdu(&packet).is_err());
    }

    #[test]
    fn tx_completion_requires_one_frame_and_exact_payload() {
        let mut payload = [0u8; 48];
        payload[0] = 1;
        payload[36] = 4;
        payload[40] = 1;
        payload[42] = 3;
        payload[43] = 4;
        payload[44] = 4;
        let report = tx_report(&payload).unwrap();
        assert!(report.success);
        assert_eq!(report.sequence, 0x403);
        assert_eq!(report.next_index, 4);
        payload[0] = 2;
        assert!(tx_report(&payload).is_err());
        assert!(tx_report(&payload[..47]).is_err());
    }
}
