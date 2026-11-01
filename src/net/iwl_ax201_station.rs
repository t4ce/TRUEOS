//! AX201 API77 managed-station command and management-frame wire contracts.
//!
//! References: Linux v6.14 iwlwifi/fw/api/{phy-ctxt,mac,binding,sta,
//! time-event,datapath,mac-cfg}.h and mvm/{phy-ctxt,mac-ctxt,binding,sta}.c.
//! This module serializes control requests; successful command replies are not
//! proof of authentication, association or WPA2 authorization.

use crate::net::iwl_ax201_fw::Firmware;
use alloc::{string::String, vec, vec::Vec};

pub(crate) const MAC_ID: u32 = 0;
pub(crate) const PHY_ID: u32 = 0;
pub(crate) const AP_STA_ID: u8 = 0;
pub(crate) const AUX_STA_ID: u8 = 1;
pub(crate) const SESSION_GROUP: u8 = 3;
pub(crate) const SESSION_NOTIFICATION: u8 = 0xfb;
const RATES: [u8; 12] = [2, 4, 11, 22, 12, 18, 24, 36, 48, 72, 96, 108];

#[derive(Clone, Debug)]
pub(crate) struct Bss {
    pub ssid: String,
    pub bssid: [u8; 6],
    pub channel: u8,
    pub beacon_interval: u16,
    pub capability: u16,
    pub dtim_count: u8,
    pub dtim_period: u8,
    pub timestamp: u64,
    pub rates: Vec<u8>,
    /// Selected RSN IE including element ID and length. Empty for open BSS.
    pub rsn_ie: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct StationConfig {
    pub mac: [u8; 6],
    pub bss: Bss,
    pub tx_chains: u32,
    pub rx_chains: u32,
    /// GP2 timestamp from the RX descriptor of the cached beacon.
    pub device_timestamp: u32,
    /// Host CCMP keeps ciphertext intact; hardware key mode must clear this.
    pub host_decrypt: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Response {
    Transport,
    ZeroStatus,
    AddStation,
}

#[derive(Clone, Debug)]
pub(crate) struct Command {
    pub group: u8,
    pub id: u8,
    pub version: u8,
    pub payload: Vec<u8>,
    pub response: Response,
}
impl Command {
    pub fn validate_response(&self, data: &[u8]) -> Result<(), &'static str> {
        match self.response {
            Response::Transport => Ok(()),
            Response::ZeroStatus if data.len() == 4 && u32_at(data, 0)? == 0 => Ok(()),
            Response::AddStation if data.len() == 4 && u32_at(data, 0)? & 0xff == 1 => Ok(()),
            _ => Err("AX201 station control command rejected"),
        }
    }
}

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn u16_at(bytes: &[u8], at: usize) -> Result<u16, &'static str> {
    Ok(u16::from_le_bytes(
        bytes
            .get(at..at + 2)
            .ok_or("AX201 short management data")?
            .try_into()
            .unwrap(),
    ))
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32, &'static str> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .ok_or("AX201 short control reply")?
            .try_into()
            .unwrap(),
    ))
}
fn valid_mac(mac: [u8; 6]) -> bool {
    mac != [0; 6] && mac[0] & 1 == 0
}

fn selected_rsn(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    if data.len() < 18 || u16_at(data, 0)? != 1 || data[2..6] != [0, 0x0f, 0xac, 4] {
        return Err("AX201 requires WPA2 CCMP group cipher");
    }
    let pairwise_count = u16_at(data, 6)? as usize;
    if pairwise_count == 0 || pairwise_count > 16 {
        return Err("AX201 invalid RSN pairwise count");
    }
    let end = 8 + pairwise_count * 4;
    let pairs = data
        .get(8..end)
        .ok_or("AX201 truncated RSN pairwise suites")?;
    if !pairs
        .chunks_exact(4)
        .any(|suite| suite == [0, 0x0f, 0xac, 4])
    {
        return Err("AX201 AP lacks CCMP pairwise cipher");
    }
    let akm_count = u16_at(data, end)? as usize;
    if akm_count == 0 || akm_count > 16 {
        return Err("AX201 invalid RSN AKM count");
    }
    let akm_end = end + 2 + akm_count * 4;
    let akms = data
        .get(end + 2..akm_end)
        .ok_or("AX201 truncated RSN AKM suites")?;
    if !akms
        .chunks_exact(4)
        .any(|suite| suite == [0, 0x0f, 0xac, 2])
    {
        return Err("AX201 requires WPA2 PSK authentication");
    }
    let capabilities = if data.len() == akm_end {
        0
    } else {
        u16_at(data, akm_end)?
    };
    if capabilities & (1 << 6) != 0 {
        return Err("AX201 required management-frame protection unsupported");
    }
    // WPA2-PSK/CCMP, no PMKID, no PMF, one replay counter per key.
    Ok(vec![
        48, 20, 1, 0, 0, 0x0f, 0xac, 4, 1, 0, 0, 0x0f, 0xac, 4, 1, 0, 0, 0x0f, 0xac, 2, 0, 0,
    ])
}

impl Bss {
    /// A raw beacon/probe response without FCS and without its firmware descriptor.
    pub fn from_beacon(frame: &[u8], expected_ssid: &str) -> Result<Self, &'static str> {
        if frame.len() < 36
            || !matches!(u16_at(frame, 0)? & 0x00fc, 0x0080 | 0x0050)
            || u16_at(frame, 0)? & 0xc703 != 0
            || u16_at(frame, 22)? & 0xf != 0
        {
            return Err("AX201 invalid BSS management frame");
        }
        let bssid: [u8; 6] = frame[16..22].try_into().unwrap();
        if !valid_mac(bssid) || frame[10..16] != bssid {
            return Err("AX201 invalid BSS source");
        }
        let capability = u16_at(frame, 34)?;
        let beacon_interval = u16_at(frame, 32)?;
        if capability & 3 != 1 || beacon_interval == 0 {
            return Err("AX201 requires infrastructure BSS");
        }
        let mut ssid = None;
        let mut channel = None;
        let mut rates = Vec::new();
        let mut rsn = None;
        let (mut dtim_count, mut dtim_period) = (0, 1);
        let mut at = 36;
        while at < frame.len() {
            let id = *frame.get(at).ok_or("AX201 truncated BSS IE")?;
            let len = *frame.get(at + 1).ok_or("AX201 truncated BSS IE")? as usize;
            let data = frame
                .get(at + 2..at + 2 + len)
                .ok_or("AX201 truncated BSS IE")?;
            match id {
                0 => {
                    if ssid.is_some() || len > 32 {
                        return Err("AX201 invalid BSS SSID");
                    }
                    ssid = Some(data);
                }
                1 | 50 => {
                    if rates.len() + len > 32 {
                        return Err("AX201 too many BSS rates");
                    }
                    for &rate in data {
                        if rate & 0x80 != 0 && !RATES.contains(&(rate & 0x7f)) {
                            return Err("AX201 unsupported mandatory BSS rate");
                        }
                        if RATES.contains(&(rate & 0x7f))
                            && !rates.iter().any(|r: &u8| r & 0x7f == rate & 0x7f)
                        {
                            rates.push(rate);
                        }
                    }
                }
                3 if len == 1 => channel = Some(data[0]),
                5 if len >= 4 => {
                    dtim_count = data[0];
                    dtim_period = data[1];
                    if dtim_period == 0 {
                        return Err("AX201 invalid BSS DTIM");
                    }
                }
                48 => {
                    if rsn.is_some() {
                        return Err("AX201 duplicate RSN IE");
                    }
                    rsn = Some(selected_rsn(data)?);
                }
                61 if len >= 1 && channel.is_none() => channel = Some(data[0]),
                _ => {}
            }
            at += 2 + len;
        }
        if ssid != Some(expected_ssid.as_bytes()) || expected_ssid.is_empty() {
            return Err("AX201 SSID does not match BSS");
        }
        let channel = channel.ok_or("AX201 BSS channel unavailable")?;
        if !((1..=14).contains(&channel) || (36..=181).contains(&channel)) {
            return Err("AX201 unsupported BSS channel");
        }
        if rates.is_empty() {
            return Err("AX201 BSS lacks supported legacy rates");
        }
        let rsn_ie = if capability & 0x10 != 0 {
            rsn.ok_or("AX201 protected BSS lacks WPA2 RSN")?
        } else {
            if rsn.is_some() {
                return Err("AX201 inconsistent BSS security");
            }
            Vec::new()
        };
        Ok(Self {
            ssid: String::from(expected_ssid),
            bssid,
            channel,
            beacon_interval,
            capability,
            dtim_count,
            dtim_period,
            timestamp: u64::from_le_bytes(frame[24..32].try_into().unwrap()),
            rates,
            rsn_ie,
        })
    }
}

fn command(
    fw: &Firmware<'_>,
    group: u8,
    id: u8,
    version: u8,
    payload: Vec<u8>,
    response: Response,
) -> Result<Command, &'static str> {
    if fw.command_version(group, id).is_some_and(|v| v != version) {
        return Err("AX201 unsupported station command version");
    }
    Ok(Command {
        group,
        id,
        version,
        payload,
        response,
    })
}
fn lmac(fw: &Firmware<'_>, bss: &Bss) -> u32 {
    if fw.has_capability(40) && bss.channel > 14 {
        1
    } else {
        0
    }
}

pub(crate) fn prepare(
    fw: &Firmware<'_>,
    config: &StationConfig,
) -> Result<Vec<Command>, &'static str> {
    if !valid_mac(config.mac)
        || !valid_mac(config.bss.bssid)
        || config.tx_chains == 0
        || config.rx_chains == 0
        || (config.tx_chains | config.rx_chains) & !7 != 0
    {
        return Err("AX201 invalid station configuration");
    }
    if fw.command_version(1, 8) != Some(4)
        || fw.command_version(1, 0x28) != Some(5)
        || fw.command_version(1, 0x18) != Some(12)
        || fw.command_version(3, 5) != Some(1)
        || !fw.has_capability(54)
    {
        return Err("AX201 unsupported station control API");
    }
    let mut commands = Vec::new();
    // Antenna/DQA mode are established once by post-NVM bootstrap before
    // scanning. They must not be reconfigured for each station request.
    let mut phy = vec![0; 32];
    put32(&mut phy, 0, PHY_ID);
    put32(&mut phy, 4, 1);
    put32(&mut phy, 8, config.bss.channel as u32);
    phy[12] = u8::from(config.bss.channel <= 14);
    put32(&mut phy, 16, lmac(fw, &config.bss)); // rxchain_info at20 is reserved for PHY v4
    commands.push(command(fw, 1, 8, 4, phy, Response::Transport)?);
    // RLC v2 carries antenna selection separately from PHY v4.
    let mut rlc = vec![0; 32];
    put32(&mut rlc, 0, PHY_ID);
    let count = config.rx_chains.count_ones().min(2);
    put32(
        &mut rlc,
        4,
        (config.rx_chains << 1) | (count << 10) | (count << 12),
    );
    commands.push(command(fw, 5, 8, 2, rlc, Response::Transport)?);
    commands.push(mac_context(fw, config, None)?);
    let mut binding = vec![0; if fw.has_capability(39) { 28 } else { 24 }];
    put32(&mut binding, 0, PHY_ID);
    put32(&mut binding, 4, 1);
    put32(&mut binding, 8, MAC_ID);
    put32(&mut binding, 12, u32::MAX);
    put32(&mut binding, 16, u32::MAX);
    put32(&mut binding, 20, PHY_ID);
    if binding.len() == 28 {
        put32(&mut binding, 24, lmac(fw, &config.bss));
    }
    commands.push(command(fw, 1, 0x2b, 2, binding, Response::ZeroStatus)?);
    commands.push(ap_station(fw, config, false)?);
    if !fw.has_capability(44) {
        let mut quota = vec![0; 64];
        for at in [0, 16, 32, 48] {
            put32(&mut quota, at, u32::MAX);
        }
        put32(&mut quota, 0, PHY_ID);
        put32(&mut quota, 4, 128);
        commands.push(command(fw, 1, 0x2c, 2, quota, Response::ZeroStatus)?);
    }
    commands.push(session_protection(fw, 10_000)?);
    Ok(commands)
}

pub(crate) fn mac_context(
    fw: &Firmware<'_>,
    config: &StationConfig,
    aid: Option<u16>,
) -> Result<Command, &'static str> {
    if aid.is_some_and(|aid| aid == 0 || aid > 2007) {
        return Err("AX201 invalid association ID");
    }
    let mut mac = vec![0; 144];
    put32(&mut mac, 0, MAC_ID);
    put32(&mut mac, 4, if aid.is_some() { 2 } else { 1 });
    put32(&mut mac, 8, 5); // FW_MAC_TYPE_BSS_STA
    mac[16..22].copy_from_slice(&config.mac);
    mac[24..30].copy_from_slice(&config.bss.bssid);
    let (mut cck, mut ofdm) = (0u32, 0u32);
    for &rate in &config.bss.rates {
        if rate & 0x80 != 0 {
            if let Some(i) = RATES.iter().position(|r| *r == rate & 0x7f) {
                if i < 4 {
                    cck |= 1 << i;
                } else {
                    ofdm |= 1 << (i - 4);
                }
            }
        }
    }
    if cck == 0 && config.bss.channel <= 14 {
        cck = 1;
    }
    if ofdm == 0 {
        ofdm = 1;
    }
    put32(&mut mac, 32, cck);
    put32(&mut mac, 36, ofdm);
    put32(
        &mut mac,
        44,
        if config.bss.capability & (1 << 5) != 0 {
            0x20
        } else {
            0
        },
    );
    put32(
        &mut mac,
        48,
        if config.bss.capability & (1 << 10) != 0 {
            0x10
        } else {
            0
        },
    );
    let filter = 2
        | 4
        | if aid.is_none() { 1 << 6 } else { 0 }
        | if config.host_decrypt { 8 | 16 } else { 0 };
    put32(&mut mac, 52, filter);
    // Legacy/non-QoS station defaults, AC BK/BE/VI/VO and multicast.
    for (i, (cwmin, cwmax, aifs, txop, fifo)) in [
        (15, 1023, 7, 0, 1),
        (15, 1023, 3, 0, 2),
        (7, 15, 2, 3008, 3),
        (3, 7, 2, 1504, 4),
        (15, 1023, 3, 0, 2),
    ]
    .into_iter()
    .enumerate()
    {
        let at = 60 + i * 8;
        put16(&mut mac, at, cwmin);
        put16(&mut mac, at + 2, cwmax);
        mac[at + 4] = aifs;
        mac[at + 5] = 1 << fifo;
        put16(&mut mac, at + 6, txop);
    }
    let b = &config.bss;
    put32(&mut mac, 116, b.beacon_interval as u32);
    put32(
        &mut mac,
        124,
        b.beacon_interval as u32 * b.dtim_period as u32,
    );
    put32(&mut mac, 132, 10); // listen interval in beacon intervals
    if let Some(aid) = aid {
        let offset = b.dtim_count as u32 * b.beacon_interval as u32 * 1024;
        put32(&mut mac, 100, 1);
        put32(&mut mac, 104, config.device_timestamp.wrapping_add(offset));
        put64(&mut mac, 108, b.timestamp.wrapping_add(offset as u64));
        put32(&mut mac, 136, aid as u32);
        put32(&mut mac, 140, config.device_timestamp);
    }
    command(fw, 1, 0x28, 5, mac, Response::Transport)
}

pub(crate) fn ap_station(
    fw: &Firmware<'_>,
    config: &StationConfig,
    modify: bool,
) -> Result<Command, &'static str> {
    let mut station = vec![0; 48];
    station[0] = u8::from(modify);
    put16(&mut station, 2, 0xffff);
    put32(&mut station, 4, MAC_ID);
    station[8..14].copy_from_slice(&config.bss.bssid);
    station[16] = AP_STA_ID;
    // Legacy 20-MHz/SISO station, aggregation disabled. No false authorization.
    put32(&mut station, 24, (3 << 26) | (3 << 28) | (1 << 17));
    command(fw, 1, 0x18, 12, station, Response::AddStation)
}

pub(crate) fn associated(
    fw: &Firmware<'_>,
    config: &StationConfig,
    aid: u16,
) -> Result<Vec<Command>, &'static str> {
    Ok(vec![
        mac_context(fw, config, Some(aid))?,
        ap_station(fw, config, true)?,
    ])
}

pub(crate) fn session_protection(
    fw: &Firmware<'_>,
    duration_tu: u32,
) -> Result<Command, &'static str> {
    if duration_tu == 0 || duration_tu > 30_000 {
        return Err("AX201 invalid session protection duration");
    }
    let mut payload = vec![0; 24];
    put32(&mut payload, 0, MAC_ID);
    put32(&mut payload, 4, 1);
    put32(&mut payload, 12, duration_tu);
    command(fw, SESSION_GROUP, 5, 1, payload, Response::Transport)
}

/// SESSION_PROTECTION_NOTIFICATION v2. Do not send auth before a matching start.
pub(crate) fn session_started(data: &[u8]) -> Result<bool, &'static str> {
    if data.len() != 16 || u32_at(data, 0)? != MAC_ID || u32_at(data, 12)? != 0 {
        return Err("AX201 invalid session protection notification");
    }
    if u32_at(data, 4)? != 1 {
        return Err("AX201 session protection failed");
    }
    match u32_at(data, 8)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err("AX201 invalid session protection start flag"),
    }
}

fn management_header(subtype: u8, config: &StationConfig, sequence: u16) -> Vec<u8> {
    let mut frame = vec![0; 24];
    frame[0] = subtype;
    frame[4..10].copy_from_slice(&config.bss.bssid);
    frame[10..16].copy_from_slice(&config.mac);
    frame[16..22].copy_from_slice(&config.bss.bssid);
    put16(&mut frame, 22, (sequence & 0xfff) << 4);
    frame
}
pub(crate) fn authentication(config: &StationConfig, sequence: u16) -> Vec<u8> {
    let mut frame = management_header(0xb0, config, sequence);
    frame.extend_from_slice(&[0, 0, 1, 0, 0, 0]);
    frame
}
pub(crate) fn association(config: &StationConfig, sequence: u16) -> Result<Vec<u8>, &'static str> {
    let mut frame = management_header(0, config, sequence);
    let capabilities = config.bss.capability & ((1 << 0) | (1 << 4) | (1 << 5) | (1 << 10));
    frame.extend_from_slice(&capabilities.to_le_bytes());
    frame.extend_from_slice(&10u16.to_le_bytes());
    if config.bss.ssid.is_empty() || config.bss.ssid.len() > 32 || config.bss.rates.is_empty() {
        return Err("AX201 invalid association request");
    }
    frame.extend_from_slice(&[0, config.bss.ssid.len() as u8]);
    frame.extend_from_slice(config.bss.ssid.as_bytes());
    for (i, chunk) in config.bss.rates.chunks(8).enumerate() {
        frame.extend_from_slice(&[if i == 0 { 1 } else { 50 }, chunk.len() as u8]);
        frame.extend_from_slice(chunk);
    }
    frame.extend_from_slice(&config.bss.rsn_ie);
    Ok(frame)
}
fn response_header(frame: &[u8], config: &StationConfig, subtype: u16) -> Result<(), &'static str> {
    if frame.len() < 24
        || u16_at(frame, 0)? & 0x00fc != subtype
        || u16_at(frame, 0)? & 0xc703 != 0
        || u16_at(frame, 22)? & 0xf != 0
        || frame[4..10] != config.mac
        || frame[10..16] != config.bss.bssid
        || frame[16..22] != config.bss.bssid
    {
        return Err("AX201 unrelated or malformed management response");
    }
    Ok(())
}
pub(crate) fn authenticated(frame: &[u8], config: &StationConfig) -> Result<(), &'static str> {
    response_header(frame, config, 0xb0)?;
    if frame.len() < 30
        || u16_at(frame, 24)? != 0
        || u16_at(frame, 26)? != 2
        || u16_at(frame, 28)? != 0
    {
        return Err("AX201 AP rejected authentication");
    }
    Ok(())
}
pub(crate) fn association_id(frame: &[u8], config: &StationConfig) -> Result<u16, &'static str> {
    response_header(frame, config, 0x10)?;
    if frame.len() < 30 || u16_at(frame, 26)? != 0 {
        return Err("AX201 AP rejected association");
    }
    let aid = u16_at(frame, 28)? & 0x3fff;
    if !(1..=2007).contains(&aid) {
        return Err("AX201 invalid AP association ID");
    }
    Ok(aid)
}

pub(crate) fn session_stop(fw: &Firmware<'_>) -> Result<Command, &'static str> {
    let mut bytes = vec![0; 24];
    put32(&mut bytes, 0, MAC_ID);
    put32(&mut bytes, 4, 3);
    command(fw, SESSION_GROUP, 5, 1, bytes, Response::Transport)
}

/// Call only after data-queue disable/drain has completed. This tears down a
/// fully installed station; partial setup must retain which commands succeeded.
pub(crate) fn teardown(
    fw: &Firmware<'_>,
    config: &StationConfig,
) -> Result<Vec<Command>, &'static str> {
    let prepared = prepare(fw, config)?;
    let mut remove = vec![
        session_stop(fw)?,
        command(
            fw,
            1,
            0x19,
            2,
            vec![AP_STA_ID, 0, 0, 0],
            Response::Transport,
        )?,
    ];
    for id in [0x2b, 0x28, 8] {
        let mut cmd = prepared
            .iter()
            .find(|command| command.group == 1 && command.id == id)
            .ok_or("AX201 missing teardown context")?
            .clone();
        put32(&mut cmd.payload, 4, 3);
        remove.push(cmd);
    }
    Ok(remove)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn firmware() -> Firmware<'static> {
        crate::net::iwl_ax201_fw::parse(crate::net::iwl_ax201_fw::EMBEDDED).unwrap()
    }
    fn beacon() -> Vec<u8> {
        let mut frame = vec![0; 36];
        frame[0] = 0x80;
        frame[10..16].copy_from_slice(&[2, 3, 4, 5, 6, 7]);
        frame[16..22].copy_from_slice(&[2, 3, 4, 5, 6, 7]);
        put16(&mut frame, 32, 100);
        put16(&mut frame, 34, 0x431);
        frame.extend_from_slice(&[0, 8]);
        frame.extend_from_slice(b"easyeasy");
        frame.extend_from_slice(&[
            1, 8, 0x82, 0x84, 0x8b, 0x96, 12, 24, 48, 108, 3, 1, 6, 5, 4, 0, 2, 0, 0,
        ]);
        frame.extend_from_slice(&[
            48, 20, 1, 0, 0, 0x0f, 0xac, 4, 1, 0, 0, 0x0f, 0xac, 4, 1, 0, 0, 0x0f, 0xac, 2, 0, 0,
        ]);
        frame
    }
    fn config() -> StationConfig {
        StationConfig {
            mac: [4, 5, 6, 7, 8, 9],
            bss: Bss::from_beacon(&beacon(), "easyeasy").unwrap(),
            tx_chains: 3,
            rx_chains: 3,
            device_timestamp: 0x1234,
            host_decrypt: true,
        }
    }
    #[test]
    fn beacon_security_mandatory_rates_and_identity_are_bounded() {
        let bss = config().bss;
        assert_eq!(bss.channel, 6);
        assert_eq!(bss.dtim_period, 2);
        assert_eq!(bss.rsn_ie.len(), 22);
        assert!(Bss::from_beacon(&beacon(), "another").is_err());
        let mut bad = beacon();
        bad[10] ^= 1;
        assert!(Bss::from_beacon(&bad, "easyeasy").is_err());
        let mut bad = beacon();
        bad.pop();
        assert!(Bss::from_beacon(&bad, "easyeasy").is_err());
        let mut bad = beacon();
        let at = bad.len() - 2;
        bad[at] = 0x40;
        assert!(Bss::from_beacon(&bad, "easyeasy").is_err());
        let mut bad = beacon();
        let at = bad.len() - 3;
        bad[at] = 8;
        assert!(Bss::from_beacon(&bad, "easyeasy").is_err());
        let mut bad = beacon();
        bad[48] = 0xff;
        assert!(Bss::from_beacon(&bad, "easyeasy").is_err());
    }
    #[test]
    fn actual_api77_station_commands_match_wire_contract() {
        let fw = firmware();
        let cfg = config();
        let commands = prepare(&fw, &cfg).unwrap();
        assert_eq!(
            commands
                .iter()
                .map(|c| (c.group, c.id, c.version, c.payload.len()))
                .collect::<Vec<_>>(),
            vec![
                (1, 8, 4, 32),
                (5, 8, 2, 32),
                (1, 0x28, 5, 144),
                (1, 0x2b, 2, 28),
                (1, 0x18, 12, 48),
                (3, 5, 1, 24)
            ]
        );
        assert!(
            !commands
                .iter()
                .any(|command| matches!((command.group, command.id), (1, 0x98) | (5, 0)))
        );
        let phy = &commands[0].payload;
        assert_eq!(u32_at(phy, 8).unwrap(), 6);
        assert_eq!(phy[12], 1);
        assert_eq!(u32_at(phy, 16).unwrap(), 0);
        assert_eq!(u32_at(phy, 20).unwrap(), 0);
        let rlc = &commands[1].payload;
        assert_eq!(u32_at(rlc, 4).unwrap(), (3 << 1) | (2 << 10) | (2 << 12));
        let mac = &commands[2].payload;
        assert_eq!(&mac[16..22], &cfg.mac);
        assert_eq!(&mac[24..30], &cfg.bss.bssid);
        assert_eq!(u32_at(mac, 8).unwrap(), 5);
        assert_eq!(u32_at(mac, 100).unwrap(), 0);
        assert_eq!(u32_at(mac, 52).unwrap(), 0x5e);
        assert_eq!(u32_at(&commands[3].payload, 12).unwrap(), u32::MAX);
        assert!(commands[3].validate_response(&0u32.to_le_bytes()).is_ok());
        assert!(commands[3].validate_response(&1u32.to_le_bytes()).is_err());
        assert!(commands[4].validate_response(&1u32.to_le_bytes()).is_ok());
        assert!(commands[4].validate_response(&0u32.to_le_bytes()).is_err());
    }
    #[test]
    fn auth_and_assoc_requests_and_responses_validate_station_identity() {
        let cfg = config();
        let request = authentication(&cfg, 7);
        assert_eq!(&request[24..], &[0, 0, 1, 0, 0, 0]);
        assert_eq!(u16_at(&request, 22).unwrap(), 7 << 4);
        let mut response = request.clone();
        response[4..10].copy_from_slice(&cfg.mac);
        response[10..16].copy_from_slice(&cfg.bss.bssid);
        put16(&mut response, 26, 2);
        assert!(authenticated(&response, &cfg).is_ok());
        response[4] ^= 2;
        assert!(authenticated(&response, &cfg).is_err());
        let request = association(&cfg, 8).unwrap();
        assert!(request.ends_with(&cfg.bss.rsn_ie));
        assert_eq!(request[0], 0);
        assert_eq!(u16_at(&request, 26).unwrap(), 10);
        let mut response = vec![0; 30];
        response[0] = 0x10;
        response[4..10].copy_from_slice(&cfg.mac);
        response[10..16].copy_from_slice(&cfg.bss.bssid);
        response[16..22].copy_from_slice(&cfg.bss.bssid);
        put16(&mut response, 28, 0xc123);
        assert_eq!(association_id(&response, &cfg).unwrap(), 0x123);
        put16(&mut response, 26, 17);
        assert!(association_id(&response, &cfg).is_err());
    }
    #[test]
    fn session_notification_and_association_update_require_confirmation() {
        let cfg = config();
        let fw = firmware();
        let mut n = vec![0; 16];
        put32(&mut n, 4, 1);
        put32(&mut n, 8, 1);
        assert!(session_started(&n).unwrap());
        put32(&mut n, 8, 0);
        assert!(!session_started(&n).unwrap());
        put32(&mut n, 4, 0);
        assert!(session_started(&n).is_err());
        let update = associated(&fw, &cfg, 42).unwrap();
        assert_eq!(u32_at(&update[0].payload, 4).unwrap(), 2);
        assert_eq!(u32_at(&update[0].payload, 100).unwrap(), 1);
        assert_eq!(u32_at(&update[0].payload, 136).unwrap(), 42);
        assert_eq!(update[1].payload[0], 1);
        let remove = teardown(&fw, &cfg).unwrap();
        assert_eq!(
            remove.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![5, 0x19, 0x2b, 0x28, 8]
        );
        for cmd in [&remove[0], &remove[2], &remove[3], &remove[4]] {
            assert_eq!(u32_at(&cmd.payload, 4).unwrap(), 3);
        }
    }
}
