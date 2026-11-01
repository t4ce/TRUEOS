//! Unified QuZ firmware initialization above the PCIe command transport.
//!
//! Protocol reference: Linux v6.14 iwlwifi/mvm/fw.c
//! iwl_run_unified_mvm_ucode(), fw/api/{alive,config,nvm-reg}.h.
//! Initialization is not association: no carrier is advertised by this module.

use crate::net::iwl_ax201_fw::Firmware;
use crate::net::iwl_ax201_station::{Bss, StationConfig};
use crate::net::iwl_ax201_transport::data;
use crate::net::iwl_ax201_transport::{Notification, Transport};
use crate::net::wifi::{WifiNetwork, WifiState};
#[path = "iwl_ax201_link.rs"]
mod link;
struct Beacon {
    bssid: [u8; 6],
    frame: Vec<u8>,
    timestamp: u32,
    signal: i8,
}
use alloc::vec::Vec;

#[path = "iwl_ax201_scan.rs"]
mod scan;

const SYSTEM: u8 = 2;
const NVM: u8 = 12;
const CAPA_DQA_SUPPORT: u32 = 12;
const COMMAND_TIMEOUT_MS: u64 = 2000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage {
    InitConfig,
    NvmComplete,
    InitComplete,
    NvmInfo,
    TxAntenna,
    DqaEnable,
    Ready,
    ScanConfig,
    ScanRequest,
    Scanning,
    ScanFinished,
    Failed,
}

pub(super) struct Runtime {
    transport: Transport,
    firmware: Firmware<'static>,
    stage: Stage,
    pending: Option<u16>,
    pending_command: Option<(u8, u8)>,
    deadline_ms: u64,
    init_complete: bool,
    pub nvm: Option<NvmInfo>,
    scan_uid: u32,
    scan_request: Vec<u8>,
    networks: Vec<WifiNetwork>,
    beacons: Vec<Beacon>,
    connection: Option<link::Connection>,
}

#[derive(Debug)]
pub(super) struct NvmInfo {
    pub version: u16,
    pub reserved_macs: u8,
    pub tx_chains: u32,
    pub rx_chains: u32,
    pub sku: u32,
    pub channels: Vec<u32>,
}

fn u32_at(data: &[u8], at: usize) -> Result<u32, &'static str> {
    let bytes = data.get(at..at + 4).ok_or("AX201 short NVM response")?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

/// V3 has 51 u16 channel profiles; V4 has a count and 110 u32 profiles.
fn decode_nvm(data: &[u8]) -> Result<NvmInfo, &'static str> {
    let flags = u32_at(data, 0)?;
    if flags & 1 != 0 {
        return Err("AX201 NVM OTP empty");
    }
    let (count, start, stride) = match data.len() {
        128 => (51, 24, 2),
        468 => {
            let count = u32_at(data, 24)? as usize;
            if count == 0 || count > 110 {
                return Err("AX201 invalid NVM channel count");
            }
            (count, 28, 4)
        }
        _ => return Err("AX201 unsupported NVM response version"),
    };
    let mut channels = Vec::with_capacity(count);
    for i in 0..count {
        let at = start + i * stride;
        let profile = if stride == 4 {
            u32_at(data, at)?
        } else {
            u16::from_le_bytes([data[at], data[at + 1]]) as u32
        };
        channels.push(profile);
    }
    let tx_chains = u32_at(data, 12)?;
    let rx_chains = u32_at(data, 16)?;
    if tx_chains == 0 || rx_chains == 0 {
        return Err("AX201 missing NVM antenna chains");
    }
    Ok(NvmInfo {
        version: u16::from_le_bytes([data[4], data[5]]),
        reserved_macs: data[7],
        sku: u32_at(data, 8)?,
        tx_chains,
        rx_chains,
        channels,
    })
}

impl Runtime {
    pub fn start(
        mut transport: Transport,
        firmware: Firmware<'static>,
    ) -> Result<Self, &'static str> {
        let alive = transport.start()?;
        if alive.group != 0
            || alive.id != 1
            || alive.payload.len() < 4
            || u16::from_le_bytes([alive.payload[0], alive.payload[1]]) != 0xcafe
        {
            return Err("AX201 invalid firmware ALIVE notification");
        }
        let mut runtime = Self {
            transport,
            firmware,
            stage: Stage::InitConfig,
            pending: None,
            pending_command: None,
            deadline_ms: 0,
            init_complete: false,
            nvm: None,
            scan_uid: 0,
            scan_request: Vec::new(),
            networks: Vec::new(),
            beacons: Vec::new(),
            connection: None,
        };
        // IWL_INIT_NVM is bit 1; NVM remains onboard rather than being overwritten.
        runtime.submit(SYSTEM, 3, &2u32.to_le_bytes())?;
        crate::log_warn!(target: "net"; "iwl-ax201: stage=unified-init firmware_alive=1\n");
        Ok(runtime)
    }

    fn submit(&mut self, group: u8, id: u8, payload: &[u8]) -> Result<(), &'static str> {
        // Linux emits version 0 for legacy headers; long-command version comes
        // from firmware metadata, never from the firmware API number (77).
        let version = if group == 0 {
            0
        } else {
            self.firmware.command_version(group, id).unwrap_or(0)
        };
        self.submit_version(group, id, version, payload)
    }

    fn submit_version(
        &mut self,
        group: u8,
        id: u8,
        version: u8,
        payload: &[u8],
    ) -> Result<(), &'static str> {
        self.pending = Some(self.transport.send_command(group, id, version, payload)?);
        self.pending_command = Some((if group == 0 { 1 } else { group }, id));
        self.deadline_ms = trueos_time::Instant::now().as_millis() + COMMAND_TIMEOUT_MS;
        Ok(())
    }

    fn notification(&mut self, notification: Notification) -> Result<(), &'static str> {
        if notification.group == 0 && notification.id == 2 {
            return Err("AX201 firmware REPLY_ERROR");
        }
        if let Some(connection) = self.connection.as_mut() {
            connection.notification(&mut self.transport, &self.firmware, &notification);
            return Ok(());
        }
        if matches!(self.stage, Stage::ScanRequest | Stage::Scanning) {
            if notification.group == 0 && notification.id == scan::COMPLETE_NOTIFICATION {
                let complete = scan::complete(&notification.payload)?;
                if complete.uid != self.scan_uid {
                    return Err("AX201 unexpected scan UID");
                }
                if complete.aborted {
                    return Err("AX201 firmware aborted scan");
                }
                self.stage = Stage::ScanFinished;
                self.pending = None;
                self.pending_command = None;
                let target = self
                    .networks
                    .iter()
                    .find(|network| network.ssid == "easyeasy");
                crate::log_warn!(target: "net";
                    "iwl-ax201: stage=scan-complete networks={} target_easyeasy={} security={:?}\n",
                    self.networks.len(), target.is_some(), target.map(|network| network.security));
                return Ok(());
            }
            if notification.group == 0 && notification.id == 0xc1 {
                if let Some(network) = scan::mpdu_beacon(&notification.payload) {
                    if let Ok(Some(raw)) = data::rx_mpdu(&notification.payload) {
                        let beacon = Beacon {
                            bssid: network.bssid,
                            frame: raw.frame,
                            timestamp: raw.device_timestamp,
                            signal: raw.signal_dbm,
                        };
                        if let Some(previous) = self
                            .beacons
                            .iter_mut()
                            .find(|entry| entry.bssid == network.bssid)
                        {
                            *previous = beacon;
                        } else if self.beacons.len() < 128 {
                            self.beacons.push(beacon);
                        }
                    }
                    if let Some(previous) =
                        self.networks.iter_mut().find(|n| n.bssid == network.bssid)
                    {
                        *previous = network;
                    } else if self.networks.len() < 128 {
                        self.networks.push(network);
                    }
                }
                return Ok(());
            }
        }
        if notification.group == 0 && notification.id == 4 {
            self.init_complete = true;
        }
        // REPLY_ERROR is the modern firmware's explicit command-failure
        // notification; byte 1 of the header is group_id, not legacy flags.
        if notification.group == 0 && notification.id == 2 {
            return Err("AX201 firmware REPLY_ERROR");
        }
        if self.pending != Some(notification.sequence) {
            return Ok(());
        }
        if self.pending_command != Some((notification.group, notification.id)) {
            return Err("AX201 firmware command response mismatch");
        }
        self.pending = None;
        self.pending_command = None;
        match self.stage {
            Stage::InitConfig => {
                self.stage = Stage::NvmComplete;
                self.submit(NVM, 0, &[0; 4])?;
            }
            Stage::NvmComplete => {
                // Unified QuZ, tx_with_siso_diversity=false: Linux explicitly
                // skips PHY_CONFIGURATION; only INIT_NVM was requested.
                self.stage = Stage::InitComplete;
                self.deadline_ms = trueos_time::Instant::now().as_millis() + COMMAND_TIMEOUT_MS;
            }
            Stage::NvmInfo => {
                let info = decode_nvm(&notification.payload)?;
                crate::log_warn!(target: "net";
                    "iwl-ax201: stage=nvm-ready version=0x{:x} reserved_macs={} tx_chains=0x{:x} rx_chains=0x{:x} sku=0x{:x} channels={} association=not-started\n",
                    info.version, info.reserved_macs, info.tx_chains, info.rx_chains,
                    info.sku, info.channels.len());
                // Match iwl_mvm_up(): configure the antenna before scanning.
                // The legacy DQA enable command is conditional on CAPA12;
                // QuZ API77 does not advertise it (its queues use gen2).
                // TX_ANT's layout is v1, but Linux sends the legacy command
                // constant with wire-header version 0 (the two are distinct).
                if self.firmware.command_version(1, 0x98) != Some(1) {
                    return Err("AX201 unsupported antenna command layout");
                }
                let chains = info.tx_chains.to_le_bytes();
                self.nvm = Some(info);
                self.stage = Stage::TxAntenna;
                crate::log_warn!(target: "net"; "iwl-ax201: stage=radio-bootstrap command=tx-antenna group=1 id=0x98 wire_version=0\n");
                self.submit_version(1, 0x98, 0, &chains)?;
            }
            Stage::TxAntenna => {
                if self.firmware.has_capability(CAPA_DQA_SUPPORT) {
                    self.stage = Stage::DqaEnable;
                    crate::log_warn!(target: "net"; "iwl-ax201: stage=radio-bootstrap command=dqa-enable group=5 id=0 wire_version=0\n");
                    self.submit_version(5, 0, 0, &0u32.to_le_bytes())?;
                } else {
                    self.stage = Stage::Ready;
                    crate::log_warn!(target: "net"; "iwl-ax201: stage=radio-ready antenna=confirmed legacy_dqa=not-advertised queue_api=gen2\n");
                }
            }
            Stage::DqaEnable => {
                self.stage = Stage::Ready;
                crate::log_warn!(target: "net"; "iwl-ax201: stage=radio-ready antenna=confirmed dqa=confirmed\n");
            }
            Stage::ScanConfig => {
                self.stage = Stage::ScanRequest;
                let request = core::mem::take(&mut self.scan_request);
                self.submit(scan::GROUP, scan::REQUEST_COMMAND, &request)?;
            }
            Stage::ScanRequest => {
                self.stage = Stage::Scanning;
                self.deadline_ms = trueos_time::Instant::now().as_millis() + 30_000;
                crate::log_warn!(target: "net"; "iwl-ax201: stage=passive-scan uid={} target_ssid=easyeasy\n", self.scan_uid);
            }
            _ => {}
        }
        Ok(())
    }

    pub fn begin_scan(&mut self, mac: [u8; 6]) -> Result<(), &'static str> {
        if self.connection.is_some() {
            return Err("AX201 scan while station configured requires device reset");
        }
        if !matches!(self.stage, Stage::Ready | Stage::ScanFinished) {
            return Err("AX201 initialization or scan pending");
        }
        let nvm = self.nvm.as_ref().ok_or("AX201 NVM not ready")?;
        let channels = scan::channels_from_nvm(&nvm.channels, nvm.sku)?;
        let config = scan::config(&self.firmware, nvm.tx_chains, nvm.rx_chains)?;
        self.scan_uid = self.scan_uid.wrapping_add(1);
        self.scan_request = scan::passive_request(&self.firmware, self.scan_uid, mac, &channels)?;
        self.networks.clear();
        self.beacons.clear();
        self.stage = Stage::ScanConfig;
        self.submit(scan::GROUP, scan::CONFIG_COMMAND, &config)
    }

    pub fn scan_finished(&self) -> bool {
        self.stage == Stage::ScanFinished
    }
    pub fn scan_results(&self) -> Vec<WifiNetwork> {
        self.networks.clone()
    }

    pub fn connect(
        &mut self,
        ssid: &str,
        password: &[u8],
        mac: [u8; 6],
    ) -> Result<(), &'static str> {
        if self.connection.is_some() {
            return Err("AX201 reconnect requires device reset");
        }
        if !matches!(self.stage, Stage::Ready | Stage::ScanFinished) {
            return Err("AX201 wait for scan completion");
        }
        let nvm = self.nvm.as_ref().ok_or("AX201 NVM not ready")?;
        let mut selected: Option<(&Beacon, Bss)> = None;
        let mut rejection = "AX201 SSID not found; run a scan first";
        let mut eligible = 0;
        let mut skipped = 0;
        // A dual-band SSID may have a stronger AP on a channel this initial
        // station path cannot transmit on. Rank only candidates whose cached
        // beacon and receive channel agree and pass the existing NVM limits.
        for network in self.networks.iter().filter(|n| n.ssid == ssid) {
            let candidate = (|| {
                let beacon = self
                    .beacons
                    .iter()
                    .find(|entry| entry.bssid == network.bssid)
                    .ok_or("AX201 AP beacon unavailable")?;
                let bss = Bss::from_beacon(&beacon.frame, ssid)?;
                if bss.channel != network.channel {
                    return Err("AX201 AP beacon and receive channel disagree");
                }
                if !scan::transmit_allowed(&nvm.channels, nvm.sku, bss.channel) {
                    return Err("AX201 AP channel not permitted for initial station TX");
                }
                Ok((beacon, bss))
            })();
            match candidate {
                Ok((beacon, bss)) => {
                    eligible += 1;
                    if selected
                        .as_ref()
                        .is_none_or(|(best, _)| beacon.signal > best.signal)
                    {
                        selected = Some((beacon, bss));
                    }
                }
                Err(reason) => {
                    rejection = reason;
                    if skipped < 8 {
                        crate::log_warn!(target: "net";
                            "iwl-ax201: stage=ap-selection skip_channel={} signal_dbm={} reason={}\n",
                            network.channel, network.signal_dbm, reason);
                    }
                    skipped += 1;
                }
            }
        }
        let (beacon, bss) = selected.ok_or(rejection)?;
        crate::log_warn!(target: "net";
            "iwl-ax201: stage=ap-selected channel={} signal_dbm={} eligible={} skipped={}\n",
            bss.channel, beacon.signal, eligible, skipped);
        let config = StationConfig {
            mac,
            bss,
            tx_chains: nvm.tx_chains,
            rx_chains: nvm.rx_chains,
            device_timestamp: beacon.timestamp,
            host_decrypt: true,
        };
        let mut ap_rsn = &[][..];
        let mut at = 36;
        while at + 2 <= beacon.frame.len() {
            let end = at + 2 + beacon.frame[at + 1] as usize;
            let ie = beacon
                .frame
                .get(at..end)
                .ok_or("AX201 malformed cached beacon IE")?;
            if ie[0] == 48 {
                ap_rsn = ie;
                break;
            }
            at = end;
        }
        self.connection = Some(link::Connection::new(
            &self.firmware,
            config,
            password,
            ap_rsn,
            beacon.signal,
        )?);
        crate::log_warn!(target: "net"; "iwl-ax201: stage=station-configuration ssid={}
", ssid);
        Ok(())
    }
    pub fn link_up(&self) -> bool {
        self.stage != Stage::Failed && self.connection.as_ref().is_some_and(|c| c.connected())
    }
    pub fn connection_state(&self) -> Option<WifiState> {
        self.connection.as_ref().map(|c| c.state())
    }
    pub fn connection_error(&self) -> Option<&'static str> {
        self.connection.as_ref().and_then(|c| c.last_error())
    }
    pub fn connected_ssid(&self) -> Option<alloc::string::String> {
        self.connection
            .as_ref()
            .filter(|c| c.connected())
            .map(|c| c.ssid().into())
    }
    pub fn signal(&self) -> Option<i8> {
        self.connection.as_ref().map(|c| c.signal())
    }
    pub fn disconnect(&mut self) {
        if let Some(connection) = self.connection.as_mut() {
            connection.disconnect();
        }
    }
    pub fn send(&mut self, frame: &[u8]) -> Result<(), &'static str> {
        if self.stage == Stage::Failed {
            return Err("AX201 firmware failed");
        }
        self.connection
            .as_mut()
            .ok_or("AX201 not associated")?
            .send(&mut self.transport, frame)
    }
    pub fn receive(&mut self) -> Option<Vec<u8>> {
        self.connection.as_mut().and_then(|c| c.receive())
    }

    pub fn poll(&mut self) -> Result<bool, &'static str> {
        if self.stage == Stage::Failed {
            return Ok(false);
        }
        let result = self.poll_inner();
        if result.is_err() {
            self.stage = Stage::Failed;
        }
        result
    }

    fn poll_inner(&mut self) -> Result<bool, &'static str> {
        for _ in 0..32 {
            let received = self.transport.poll().map_err(|reason| {
                if let Some(connection) = self.connection.as_mut() {
                    connection.transport_failed(reason);
                }
                reason
            })?;
            let Some(notification) = received else {
                break;
            };
            self.notification(notification)?;
        }
        if let Some(connection) = self.connection.as_mut() {
            connection.poll(&mut self.transport);
        }
        if self.stage == Stage::InitComplete && self.init_complete {
            self.stage = Stage::NvmInfo;
            self.submit(NVM, 2, &[0; 4])?;
        }
        if !matches!(
            self.stage,
            Stage::Ready | Stage::ScanFinished | Stage::Failed
        ) && trueos_time::Instant::now().as_millis() >= self.deadline_ms
        {
            crate::log_warn!(target: "net"; "iwl-ax201: stage=command-timeout waiting={:?}\n", self.stage);
            return Err("AX201 initialization command timeout");
        }
        Ok(matches!(
            self.stage,
            Stage::Ready
                | Stage::ScanConfig
                | Stage::ScanRequest
                | Stage::Scanning
                | Stage::ScanFinished
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nvm_shape_empty_otp_and_channel_limits_are_validated() {
        let mut v4 = alloc::vec![0; 468];
        v4[12..16].copy_from_slice(&3u32.to_le_bytes());
        v4[16..20].copy_from_slice(&3u32.to_le_bytes());
        v4[24..28].copy_from_slice(&110u32.to_le_bytes());
        assert_eq!(decode_nvm(&v4).unwrap().channels.len(), 110);
        v4[24..28].copy_from_slice(&111u32.to_le_bytes());
        assert!(decode_nvm(&v4).is_err());
        v4[0] = 1;
        assert!(decode_nvm(&v4).is_err());
        assert!(decode_nvm(&v4[..467]).is_err());
        let mut v3 = alloc::vec![0; 128];
        v3[12] = 1;
        v3[16] = 1;
        v3[24..26].copy_from_slice(&0x1234u16.to_le_bytes());
        assert_eq!(decode_nvm(&v3).unwrap().channels[0], 0x1234);
    }
}
