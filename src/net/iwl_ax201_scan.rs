//! AX201 API77 UMAC passive scan wire formats and bounded beacon decoding.
//!
//! References: Linux v6.14 iwlwifi/fw/api/scan.h, phy-ctxt.h, and mvm/scan.c.
//! Only SCAN_CFG v5 and SCAN_REQ_UMAC v15 are admitted. All scans are passive;
//! channel selection retains NVM VALID/SKU restrictions and never enables TX.

use crate::net::iwl_ax201_fw::Firmware;
use crate::net::wifi::{WifiNetwork, WifiSecurity};
use alloc::{string::String, vec, vec::Vec};

pub(super) const GROUP: u8 = 1;
pub(super) const CONFIG_COMMAND: u8 = 0x0c;
pub(super) const REQUEST_COMMAND: u8 = 0x0d;
pub(super) const COMPLETE_NOTIFICATION: u8 = 0x0f;
const MAX_CHANNELS: usize = 67;
const GENERAL: usize = 8;
const CHANNELS: usize = GENERAL + 36;
const PERIODIC: usize = CHANNELS + 4 + MAX_CHANNELS * 8;
const PROBE: usize = PERIODIC + 12;
const REQUEST_BYTES: usize = PROBE + 532 + 4 + 20 * 34 + 8 * 4 + 16 * 6;
const FORCE_PASSIVE: u16 = 1 << 11;
const PASS_ALL: u16 = 1 << 1;
const ITERATION_COMPLETE: u16 = 1 << 2;

// iwl_ext_nvm_channels[]: AX201/22000 uses the extended 51-entry channel map.
const NVM_CHANNELS: [u8; 51] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 36, 40, 44, 48, 52, 56, 60, 64, 68, 72, 76, 80,
    84, 88, 92, 96, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144, 149, 153, 157, 161,
    165, 169, 173, 177, 181,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ScanChannel {
    pub number: u8,
    /// Firmware PHY band: 1=2.4GHz, 0=5GHz (not nl80211 band numbering).
    pub band: u8,
}

/// Restrict receive-only scanning to NVM-valid entries and enabled SKU bands.
/// A V4 NVM map includes UHB channels after index50; AX201 never scans those.
pub(super) fn channels_from_nvm(
    profiles: &[u32],
    sku: u32,
) -> Result<Vec<ScanChannel>, &'static str> {
    if profiles.len() != 51 && profiles.len() != 110 {
        return Err("AX201 unsupported NVM scan channel map");
    }
    let channels: Vec<_> = NVM_CHANNELS
        .iter()
        .enumerate()
        .filter_map(|(index, &number)| {
            let band_enabled = if index < 14 {
                sku & 1 != 0
            } else {
                sku & 2 != 0
            };
            (band_enabled && profiles[index] & 1 != 0).then_some(ScanChannel {
                number,
                band: if index < 14 { 1 } else { 0 },
            })
        })
        .collect();
    if channels.is_empty() {
        return Err("AX201 no NVM-permitted passive scan channels");
    }
    Ok(channels)
}

/// First station implementation transmits only on NVM active, non-DFS channels.
/// This does not relax NVM/SKU limits based on an AP's advertised country IE.
pub(super) fn transmit_allowed(profiles: &[u32], sku: u32, channel: u8) -> bool {
    let Some(index) = NVM_CHANNELS.iter().position(|&number| number == channel) else {
        return false;
    };
    let enabled = if index < 14 {
        sku & 1 != 0
    } else {
        sku & 2 != 0
    };
    enabled
        && profiles
            .get(index)
            .is_some_and(|flags| flags & 9 == 9 && flags & 16 == 0)
}

fn put16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub(super) fn config(
    firmware: &Firmware<'_>,
    tx_chains: u32,
    rx_chains: u32,
) -> Result<Vec<u8>, &'static str> {
    if firmware.command_version(GROUP, CONFIG_COMMAND) != Some(5) {
        return Err("AX201 unsupported SCAN_CFG command version");
    }
    if tx_chains == 0 || rx_chains == 0 || (tx_chains | rx_chains) & !7 != 0 {
        return Err("AX201 invalid scan antenna chains");
    }
    // SCAN_CONFIG_DB_CMD_API_S_5: CAM/promiscuous disabled, station byte
    // deprecated in v5, reserved zero. No legacy dynamic channel array.
    let mut command = vec![0; 12];
    put32(&mut command, 4, tx_chains);
    put32(&mut command, 8, rx_chains);
    Ok(command)
}

pub(super) fn passive_request(
    firmware: &Firmware<'_>,
    uid: u32,
    mac: [u8; 6],
    channels: &[ScanChannel],
) -> Result<Vec<u8>, &'static str> {
    if firmware.command_version(GROUP, REQUEST_COMMAND) != Some(15) {
        return Err("AX201 unsupported SCAN_REQ_UMAC command version");
    }
    if channels.is_empty() || channels.len() > MAX_CHANNELS {
        return Err("AX201 invalid passive scan channel count");
    }
    if mac[0] & 1 != 0 || mac == [0; 6] {
        return Err("AX201 missing station MAC for scan");
    }
    if channels.iter().any(|ch| {
        !NVM_CHANNELS
            .iter()
            .enumerate()
            .any(|(i, &n)| ch.number == n && ch.band == if i < 14 { 1 } else { 0 })
    }) {
        return Err("AX201 unsupported passive scan channel");
    }
    let mut command = vec![0; REQUEST_BYTES];
    put32(&mut command, 0, uid);
    put32(&mut command, 4, 6); // IWL_SCAN_PRIORITY_EXT_6, regular scan.
    put16(
        &mut command,
        GENERAL,
        FORCE_PASSIVE | PASS_ALL | ITERATION_COMPLETE,
    );
    // general_params_v11: no MAC context exists yet, scan_start_mac_id=0.
    command[GENERAL + 4..GENERAL + 6].copy_from_slice(&[10, 10]);
    command[GENERAL + 6..GENERAL + 9].copy_from_slice(&[2, 8, 10]);
    put16(&mut command, GENERAL + 10, 300);
    put32(&mut command, GENERAL + 28, 6);
    command[GENERAL + 32..GENERAL + 34].copy_from_slice(&[110, 110]);
    command[CHANNELS + 1] = channels.len() as u8;
    for (index, channel) in channels.iter().enumerate() {
        let at = CHANNELS + 4 + index * 8;
        // v15 uses channel_config v2: flags0, channel, band, iter_count1,
        // iter_interval0. No SSID bits, adaptive dwell, EBS, or TX flags.
        command[at + 4] = channel.number;
        command[at + 5] = channel.band;
        command[at + 6] = 1;
    }
    command[PERIODIC + 2] = 1; // Single scan plan, exactly one iteration.
    // Keep a valid wildcard probe template although FORCE_PASSIVE prevents
    // firmware transmitting it. mac_header segment includes wildcard SSID IE.
    put16(&mut command, PROBE + 2, 26);
    let frame = PROBE + 20;
    command[frame] = 0x40; // Probe request management frame.
    command[frame + 4..frame + 10].fill(0xff);
    command[frame + 10..frame + 16].copy_from_slice(&mac);
    command[frame + 16..frame + 22].fill(0xff);
    Ok(command)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ScanComplete {
    pub uid: u32,
    pub aborted: bool,
}

/// SCAN_COMPLETE_NTF_UMAC_API_S_VER_1, command0xf. Notifications for other
/// command IDs, including per-iteration0xb5, must not call this decoder.
pub(super) fn complete(data: &[u8]) -> Result<ScanComplete, &'static str> {
    if data.len() != 16 || !matches!(data[6], 1 | 2) {
        return Err("AX201 invalid UMAC scan completion");
    }
    Ok(ScanComplete {
        uid: u32::from_le_bytes(data[..4].try_into().unwrap()),
        aborted: data[6] == 2,
    })
}

/// AX201 (family22000, before AX210) RX_MPDU payload, descriptor v1=48
/// bytes. Linux rxmq.c chooses this descriptor by device family, not command
/// notification version. Accept unencrypted, unpadded management MPDUs only.
///
/// mac_flags1[7:4] reports trailing MIC/CRC length in two-byte units. Linux
/// iwl_mvm_create_skb strips that count for a non-monitor interface; therefore
/// do not blindly remove four bytes. Here plaintext beacons may carry either
/// no trailer (already stripped by RADA) or a four-byte FCS, and other trailer
/// formats are rejected rather than misparsed as IEs.
pub(super) fn mpdu_beacon(payload: &[u8]) -> Option<WifiNetwork> {
    const DESCRIPTOR_BYTES: usize = 48;
    if payload.len() < DESCRIPTOR_BYTES {
        return None;
    }
    let mpdu_len = u16::from_le_bytes(payload[..2].try_into().ok()?) as usize;
    let flags1 = payload[2];
    let flags2 = payload[3];
    let status = u32::from_le_bytes(payload[12..16].try_into().ok()?);
    // CRC_OK + OVERRUN_OK, and no encryption algorithm in status[10:8].
    if status & 3 != 3 || status & (7 << 8) != 0 {
        return None;
    }
    // Firmware-inserted padding and A-MSDU need data-specific reconstruction.
    // Beacon/probe response MAC headers are always24 bytes, never QoS/A4.
    if flags2 & 0x60 != 0 || (flags2 & 0x1f) as usize * 2 != 24 {
        return None;
    }
    let trailer = ((flags1 & 0xf0) >> 3) as usize;
    if trailer != 0 && trailer != 4 {
        return None;
    }
    let frame_len = mpdu_len.checked_sub(trailer)?;
    if !(36..=4096).contains(&frame_len) {
        return None;
    }
    let mpdu_end = DESCRIPTOR_BYTES.checked_add(mpdu_len)?;
    let mpdu = payload.get(DESCRIPTOR_BYTES..mpdu_end)?;
    let frame = mpdu.get(..frame_len)?;
    let channel_number = payload[34];
    let channel = ScanChannel {
        number: channel_number,
        band: if channel_number <= 14 { 1 } else { 0 },
    };
    // Firmware energy magnitudes: zero means unavailable. Linux maps absent
    // chains to S8_MIN and uses the strongest available chain for signal.
    let energy = [payload[32], payload[33]]
        .into_iter()
        .filter(|&v| v != 0)
        .min();
    let signal = energy
        .map(|value| (-(value as i16)).max(-128) as i8)
        .unwrap_or(-128);
    beacon(frame, channel, signal)
}

fn rsn_security(data: &[u8]) -> Option<WifiSecurity> {
    if data.len() < 8 || data[..2] != [1, 0] {
        return None;
    }
    let mut at = 6;
    let pairwise = u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?) as usize;
    if pairwise == 0 || pairwise > 16 {
        return None;
    }
    at += 2;
    at = at.checked_add(pairwise.checked_mul(4)?)?;
    let count = u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?) as usize;
    if count == 0 || count > 16 {
        return None;
    }
    at += 2;
    let suites = data.get(at..at.checked_add(count * 4)?)?;
    let sae = suites
        .chunks_exact(4)
        .any(|suite| suite[..3] == [0, 0x0f, 0xac] && matches!(suite[3], 8 | 9 | 18));
    Some(if sae {
        WifiSecurity::WPA3
    } else {
        WifiSecurity::WPA2
    })
}

/// Decode raw, FCS-stripped 802.11 beacon/probe response from a validated RX
/// MPDU. Channel and RSSI must come from the hardware RX descriptor. No raw
/// management packet is forwarded into the Ethernet/IP interface.
pub(super) fn beacon(frame: &[u8], channel: ScanChannel, signal_dbm: i8) -> Option<WifiNetwork> {
    if !(36..=4096).contains(&frame.len()) || frame[0] & 0xfc != 0x80 && frame[0] & 0xfc != 0x50 {
        return None;
    }
    // Do not accept protected/fragmented management frames or invalid BSSIDs.
    if frame[0] & 3 != 0 || frame[1] & 0x47 != 0 || frame[22] & 0x0f != 0 {
        return None;
    }
    let bssid: [u8; 6] = frame[16..22].try_into().ok()?;
    if bssid == [0; 6] || bssid[0] & 1 != 0 {
        return None;
    }
    let frequency_mhz = match (channel.band, channel.number) {
        (1, 1..=13) => 2407 + channel.number as u16 * 5,
        (1, 14) => 2484,
        (0, 36..=181) => 5000 + channel.number as u16 * 5,
        _ => return None,
    };
    let privacy = frame[34] & 0x10 != 0;
    let mut ssid = None;
    let mut rsn = None;
    let mut wpa = false;
    let mut at = 36;
    while at < frame.len() {
        let header = frame.get(at..at + 2)?;
        let end = at.checked_add(2 + header[1] as usize)?;
        let data = frame.get(at + 2..end)?;
        match header[0] {
            0 => {
                if ssid.is_some() || data.len() > 32 {
                    return None;
                }
                ssid = Some(String::from_utf8_lossy(data).into_owned());
            }
            48 => {
                if rsn.is_some() {
                    return None;
                }
                rsn = Some(rsn_security(data)?);
            }
            221 if data.starts_with(&[0, 0x50, 0xf2, 1]) => {
                // WPA uses the same version/group/pairwise/AKM shape after OUI.
                rsn_security(&data[4..])?;
                wpa = true;
            }
            _ => {}
        }
        at = end;
    }
    Some(WifiNetwork {
        ssid: ssid?,
        bssid,
        channel: channel.number,
        signal_dbm,
        frequency_mhz,
        security: rsn.unwrap_or(if wpa {
            WifiSecurity::WPA
        } else if privacy {
            WifiSecurity::WEP
        } else {
            WifiSecurity::Open
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::iwl_ax201_fw::{EMBEDDED, parse};

    #[test]
    fn api77_wire_shape_and_force_passive_flags() {
        let fw = parse(EMBEDDED).unwrap();
        assert_eq!(
            config(&fw, 3, 3).unwrap(),
            [0, 0, 0, 0, 3, 0, 0, 0, 3, 0, 0, 0]
        );
        let channels = [
            ScanChannel { number: 1, band: 1 },
            ScanChannel {
                number: 36,
                band: 0,
            },
        ];
        let req = passive_request(&fw, 0x1234, [2, 1, 2, 3, 4, 5], &channels).unwrap();
        assert_eq!(req.len(), 1940);
        assert_eq!(&req[..4], &[0x34, 0x12, 0, 0]);
        assert_eq!(
            u16::from_le_bytes(req[8..10].try_into().unwrap()) & FORCE_PASSIVE,
            FORCE_PASSIVE
        );
        assert_eq!(&req[CHANNELS + 4..CHANNELS + 12], &[0, 0, 0, 0, 1, 1, 1, 0]);
        assert_eq!(
            &req[CHANNELS + 12..CHANNELS + 20],
            &[0, 0, 0, 0, 36, 0, 1, 0]
        );
        assert_eq!(req[PERIODIC + 2], 1);
        assert!(passive_request(&fw, 0, [0; 6], &channels).is_err());
    }
    #[test]
    fn only_nvm_valid_and_sku_enabled_channels_are_selected() {
        let mut profiles = vec![0; 110];
        profiles[0] = 1;
        profiles[14] = 1;
        profiles[51] = 1;
        assert_eq!(
            channels_from_nvm(&profiles, 1).unwrap(),
            [ScanChannel { number: 1, band: 1 }]
        );
        assert_eq!(channels_from_nvm(&profiles, 3).unwrap().len(), 2);
        assert!(channels_from_nvm(&profiles, 0).is_err());
        assert!(channels_from_nvm(&profiles[..50], 3).is_err());
    }
    fn raw_beacon() -> Vec<u8> {
        let mut f = vec![0; 36];
        f[0] = 0x80;
        f[16..22].copy_from_slice(&[2, 3, 4, 5, 6, 7]);
        f.extend_from_slice(&[0, 8]);
        f.extend_from_slice(b"easyeasy");
        f
    }
    #[test]
    fn beacon_bounds_and_security_are_checked() {
        let channel = ScanChannel { number: 6, band: 1 };
        let mut frame = raw_beacon();
        let net = beacon(&frame, channel, -45).unwrap();
        assert_eq!(net.ssid, "easyeasy");
        assert_eq!(net.frequency_mhz, 2437);
        assert_eq!(net.security, WifiSecurity::Open);
        frame.extend_from_slice(&[
            48, 18, 1, 0, 0, 0x0f, 0xac, 4, 1, 0, 0, 0x0f, 0xac, 4, 1, 0, 0, 0x0f, 0xac, 8,
        ]);
        assert_eq!(
            beacon(&frame, channel, -45).unwrap().security,
            WifiSecurity::WPA3
        );
        frame.pop();
        assert!(beacon(&frame, channel, -45).is_none());
        let mut frame = raw_beacon();
        frame.extend_from_slice(&[0, 0]);
        assert!(beacon(&frame, channel, -45).is_none());
    }
    fn mpdu(frame: &[u8], fcs: bool) -> Vec<u8> {
        let mut packet = vec![0; 48];
        let length = frame.len() + if fcs { 4 } else { 0 };
        packet[..2].copy_from_slice(&(length as u16).to_le_bytes());
        packet[2] = if fcs { 0x20 } else { 0 };
        packet[3] = 12;
        packet[12] = 3;
        packet[32] = 56;
        packet[33] = 42;
        packet[34] = 6;
        packet.extend_from_slice(frame);
        if fcs {
            packet.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        }
        packet
    }
    #[test]
    fn mpdu_descriptor_reported_fcs_and_energy_are_honored() {
        let frame = raw_beacon();
        for fcs in [false, true] {
            let packet = mpdu(&frame, fcs);
            let network = mpdu_beacon(&packet).unwrap();
            assert_eq!(network.ssid, "easyeasy");
            assert_eq!(network.signal_dbm, -42);
            assert_eq!(network.channel, 6);
        }
    }
    #[test]
    fn mpdu_rejects_bad_crc_padding_amsdu_and_encryption() {
        let original = mpdu(&raw_beacon(), true);
        for (offset, value) in [
            (12, 1),
            (12, 2),
            (3, 12 | 0x20),
            (3, 12 | 0x40),
            (3, 11),
            (2, 0x10),
            (13, 1),
        ] {
            let mut packet = original.clone();
            packet[offset] = value;
            assert!(
                mpdu_beacon(&packet).is_none(),
                "offset{offset} value{value}"
            );
        }
    }
    #[test]
    fn mpdu_rejects_truncated_descriptor_body_and_trailer() {
        let original = mpdu(&raw_beacon(), true);
        assert!(mpdu_beacon(&original[..47]).is_none());
        assert!(mpdu_beacon(&original[..original.len() - 1]).is_none());
        let mut packet = original;
        packet[..2].copy_from_slice(&3u16.to_le_bytes());
        assert!(mpdu_beacon(&packet).is_none());
    }
    #[test]
    fn completion_status_and_uid_are_preserved() {
        let mut data = [0; 16];
        data[0] = 7;
        data[6] = 1;
        assert_eq!(
            complete(&data).unwrap(),
            ScanComplete {
                uid: 7,
                aborted: false
            }
        );
        data[6] = 2;
        assert!(complete(&data).unwrap().aborted);
        data[6] = 0;
        assert!(complete(&data).is_err());
        assert!(complete(&data[..15]).is_err());
    }
}
