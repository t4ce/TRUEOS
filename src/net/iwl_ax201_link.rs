//! Managed station and WPA2 controlled port above AX201's firmware transport.
//!
//! Firmware configuration, over-the-air association, and cryptographic key
//! confirmation are separate stages. Only confirmed M4 transmission opens the
//! Ethernet port. The first implementation uses software CCMP and no A-MPDU.

use crate::net::iwl_ax201_fw::Firmware;
use crate::net::iwl_ax201_station::{self as station, Command, StationConfig};
use crate::net::iwl_ax201_transport::{Notification, Transport, TxOptions, data};
use crate::net::iwl_ax201_wpa2::{CcmpKey, Session};
use crate::net::wifi::WifiState;
use alloc::{collections::VecDeque, vec::Vec};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Configure,
    Queue,
    Session,
    Authenticate,
    Associate,
    AssociatedConfig,
    Keys,
    Connected,
    Failed,
    Disconnected,
}

pub(crate) struct Connection {
    config: StationConfig,
    state: State,
    commands: VecDeque<Command>,
    pending: Option<(u16, Command)>,
    queue: Option<u16>,
    data_queue: Option<u16>,
    queue_request: Option<u16>,
    session_started: bool,
    sequence: u16,
    deadline: u64,
    handshake: Option<Session>,
    pairwise: Option<CcmpKey>,
    group: Option<CcmpKey>,
    key_confirmation: Option<u16>,
    early_eapol: VecDeque<Vec<u8>>,
    eapol_seen: bool,
    rx: VecDeque<Vec<u8>>,
    last_error: Option<&'static str>,
    signal_dbm: i8,
    dropped: u64,
    tx_packets: u64,
    rx_packets: u64,
}
fn now() -> u64 {
    trueos_time::Instant::now().as_millis()
}

impl Connection {
    pub fn new(
        fw: &Firmware<'_>,
        config: StationConfig,
        password: &[u8],
        ap_rsn: &[u8],
        signal_dbm: i8,
    ) -> Result<Self, &'static str> {
        let handshake = if config.bss.rsn_ie.is_empty() {
            if !password.is_empty() {
                return Err("AX201 open network does not use a password");
            }
            None
        } else {
            let mut nonce = [0; 32];
            if !crate::tyche::fill_bytes(&mut nonce) {
                return Err("AX201 WPA2 requires secure entropy");
            }
            Some(Session::with_ies(
                config.bss.ssid.as_bytes(),
                password,
                config.mac,
                config.bss.bssid,
                nonce,
                &config.bss.rsn_ie,
                ap_rsn,
            )?)
        };
        let commands = station::prepare(fw, &config)?.into();
        Ok(Self {
            config,
            state: State::Configure,
            commands,
            pending: None,
            queue: None,
            data_queue: None,
            queue_request: None,
            session_started: false,
            sequence: 0,
            deadline: now() + 2000,
            handshake,
            pairwise: None,
            group: None,
            key_confirmation: None,
            early_eapol: VecDeque::new(),
            eapol_seen: false,
            rx: VecDeque::new(),
            last_error: None,
            signal_dbm,
            dropped: 0,
            tx_packets: 0,
            rx_packets: 0,
        })
    }
    pub(super) fn transport_failed(&mut self, reason: &'static str) {
        self.fail(reason);
    }

    pub fn connected(&self) -> bool {
        self.state == State::Connected
    }
    pub fn state(&self) -> WifiState {
        match self.state {
            State::Connected => WifiState::Connected,
            State::Keys => WifiState::Authenticating,
            State::Failed => WifiState::Failed,
            State::Disconnected => WifiState::Disconnected,
            _ => WifiState::Connecting,
        }
    }
    pub fn ssid(&self) -> &str {
        &self.config.bss.ssid
    }
    pub fn signal(&self) -> i8 {
        self.signal_dbm
    }
    pub fn last_error(&self) -> Option<&'static str> {
        self.last_error
    }
    pub fn receive(&mut self) -> Option<Vec<u8>> {
        self.rx.pop_front()
    }

    fn fail(&mut self, reason: &'static str) {
        if !matches!(self.state, State::Failed | State::Disconnected) {
            crate::log_warn!(target: "net"; "iwl-ax201: stage=station-error waiting={:?} command={:?} queue_pending={} reason={}\n",
                self.state, self.pending.as_ref().map(|(_, command)| (command.group, command.id)),
                self.queue_request.is_some(), reason);
        }
        self.state = State::Failed;
        self.last_error = Some(reason);
        self.rx.clear();
        self.early_eapol.clear();
        self.handshake = None;
        self.pairwise = None;
        self.group = None;
    }
    pub fn disconnect(&mut self) {
        // Close the controlled port immediately. DMA stays retained and queues
        // are not reused: removing a station while hardware still owns a TX
        // descriptor needs the firmware's newer queue-remove protocol.
        self.state = State::Disconnected;
        self.rx.clear();
        self.early_eapol.clear();
        self.handshake = None;
        self.pairwise = None;
        self.group = None;
        crate::log_warn!(target: "net"; "iwl-ax201: stage=port-closed reconnect=device-reset-required\n");
    }
    fn next_sequence(&mut self) -> u16 {
        let result = self.sequence;
        self.sequence = (result + 1) & 0x0fff;
        result
    }
    fn transmit_raw(
        &mut self,
        transport: &mut Transport,
        frame: &[u8],
    ) -> Result<u16, &'static str> {
        let antenna = 1u8 << self.config.tx_chains.trailing_zeros();
        let queue = if frame[0] & 0x0c == 8 {
            self.data_queue
        } else {
            self.queue
        };
        transport.send_raw80211(
            queue.ok_or("AX201 station TX queue unavailable")?,
            frame,
            TxOptions {
                band_2ghz: self.config.bss.channel <= 14,
                antenna,
                encrypt: false,
                high_priority: true,
            },
        )
    }
    fn allocate_queue(&mut self, transport: &mut Transport, tid: u8) -> Result<(), &'static str> {
        let body = transport.prepare_tx_queue(station::AP_STA_ID, tid)?;
        let (group, id, version) = transport.queue_allocation_command()?;
        let sequence = transport.send_command(group, id, version, &body)?;
        self.queue_request = Some(sequence);
        crate::log_warn!(target: "net";
            "iwl-ax201: stage=tx-queue-request tid={} cmd={:02x}:{:02x} version={} bytes={} seq=0x{:04x}\n",
            tid, group, id, version, body.len(), sequence);
        Ok(())
    }

    fn send_authentication(&mut self, transport: &mut Transport) -> Result<(), &'static str> {
        let sequence = self.next_sequence();
        let frame = station::authentication(&self.config, sequence);
        self.transmit_raw(transport, &frame)?;
        self.state = State::Authenticate;
        self.deadline = now() + 3000;
        crate::log_warn!(target: "net"; "iwl-ax201: stage=authentication-sent channel={}\n", self.config.bss.channel);
        Ok(())
    }
    fn send_eapol(&mut self, transport: &mut Transport, eapol: &[u8]) -> Result<u16, &'static str> {
        let mut ethernet = Vec::with_capacity(14 + eapol.len());
        ethernet.extend_from_slice(&self.config.bss.bssid);
        ethernet.extend_from_slice(&self.config.mac);
        ethernet.extend_from_slice(&[0x88, 0x8e]);
        ethernet.extend_from_slice(eapol);
        let sequence = self.next_sequence();
        let frame = data::encapsulate_ethernet(
            &ethernet,
            self.config.mac,
            self.config.bss.bssid,
            sequence,
        )?;
        // EAPOL-Key replies carry their own authenticated MIC. Match the
        // normal supplicant ordering: send M4 clear, then open protected data
        // only after the radio confirms its delivery.
        self.transmit_raw(transport, &frame)
    }
    fn handle_eapol(
        &mut self,
        transport: &mut Transport,
        bytes: &[u8],
    ) -> Result<(), &'static str> {
        let Some(handshake) = self.handshake.as_mut() else {
            return Ok(());
        };
        // A forged/replayed EAPOL frame must never install keys or open a port.
        let action = match handshake.handle_eapol(bytes) {
            Ok(action) => action,
            Err(_) => {
                self.dropped += 1;
                return Ok(());
            }
        };
        self.eapol_seen = true;
        if let Some(keys) = action.keys {
            self.pairwise = Some(CcmpKey::new(keys.temporal, 0, self.config.bss.bssid)?);
            self.group = Some(CcmpKey::with_receive_pn(
                keys.gtk.temporal,
                keys.gtk.id,
                self.config.bss.bssid,
                keys.gtk.receive_pn,
            )?);
        }
        let sequence = self.send_eapol(transport, &action.reply)?;
        if self.pairwise.is_some() && self.state != State::Connected {
            self.key_confirmation = Some(sequence);
            crate::log_warn!(target: "net"; "iwl-ax201: stage=wpa2-m4-sent awaiting=radio-ack\n");
        } else if self.state != State::Connected {
            crate::log_warn!(target: "net"; "iwl-ax201: stage=wpa2-m2-sent awaiting=authenticated-m3\n");
        }
        Ok(())
    }
    fn management(
        &mut self,
        transport: &mut Transport,
        fw: &Firmware<'_>,
        frame: &[u8],
    ) -> Result<(), &'static str> {
        if frame.len() < 24
            || frame[4..10] != self.config.mac
            || frame[10..16] != self.config.bss.bssid
            || frame[16..22] != self.config.bss.bssid
        {
            return Ok(());
        }
        match (frame[0] & 0xfc, self.state) {
            (0xb0, State::Authenticate) => {
                station::authenticated(frame, &self.config)?;
                let sequence = self.next_sequence();
                let request = station::association(&self.config, sequence)?;
                self.transmit_raw(transport, &request)?;
                self.state = State::Associate;
                self.deadline = now() + 3000;
                crate::log_warn!(target: "net"; "iwl-ax201: stage=association-sent\n");
            }
            (0x10, State::Associate) => {
                let aid = station::association_id(frame, &self.config)?;
                self.commands = station::associated(fw, &self.config, aid)?.into();
                self.state = State::AssociatedConfig;
                self.deadline = now() + 2000;
                crate::log_warn!(target: "net"; "iwl-ax201: stage=associated aid={} port=closed\n", aid);
            }
            (0xc0 | 0xa0, _) => return Err("AX201 AP disconnected station"),
            _ => {}
        }
        Ok(())
    }
    fn frame(
        &mut self,
        transport: &mut Transport,
        fw: &Firmware<'_>,
        raw: data::RawFrame,
    ) -> Result<(), &'static str> {
        if raw.frame.len() < 24 || raw.frame[10..16] != self.config.bss.bssid {
            return Ok(());
        }
        self.signal_dbm = raw.signal_dbm;
        if raw.frame[0] & 0x0c == 0 {
            if raw.protected || raw.hw_decrypted {
                return Ok(());
            }
            return self.management(transport, fw, &raw.frame);
        }
        if raw.hw_decrypted {
            self.dropped += 1;
            return Ok(());
        }
        let protected = raw.protected;
        let frame = if protected {
            let header = match data::header_len(&raw.frame) {
                Ok(len) => len,
                Err(_) => return Ok(()),
            };
            let Some(_iv) = raw.frame.get(header..header + 8) else {
                return Ok(());
            };
            // The receiver address selects the security association. A GTK
            // must never authenticate a unicast packet, even if its IV names
            // an installed group key. decrypt() validates that key's IV ID.
            let key = if raw.frame[4] & 1 == 0 {
                self.pairwise.as_mut()
            } else {
                self.group.as_mut()
            };
            let Some(key) = key else {
                return Ok(());
            };
            match key.decrypt(&raw.frame) {
                Ok(frame) => frame,
                Err(_) => {
                    self.dropped += 1;
                    return Ok(());
                }
            }
        } else {
            raw.frame
        };
        let ethernet =
            match data::decapsulate_ethernet(&frame, self.config.mac, self.config.bss.bssid) {
                Ok(frame) => frame,
                Err(_) => return Ok(()),
            };
        if ethernet[12..14] == [0x88, 0x8e]
            && ethernet[..6] == self.config.mac
            && ethernet[6..12] == self.config.bss.bssid
        {
            if self.state == State::AssociatedConfig && self.early_eapol.len() < 8 {
                self.early_eapol.push_back(ethernet[14..].to_vec());
            } else if matches!(self.state, State::Keys | State::Connected) {
                self.handle_eapol(transport, &ethernet[14..])?;
            }
            return Ok(());
        }
        if self.state == State::Connected
            && (self.handshake.is_none() || protected)
            && self.rx.len() < 256
        {
            self.rx_packets += 1;
            if self.rx_packets == 1 {
                crate::log_warn!(target: "net"; "iwl-ax201: stage=ethernet-rx bytes={} authenticated={}\n", ethernet.len(), protected);
            }
            self.rx.push_back(ethernet);
        }
        Ok(())
    }

    pub fn notification(
        &mut self,
        transport: &mut Transport,
        fw: &Firmware<'_>,
        notification: &Notification,
    ) {
        if matches!(self.state, State::Failed | State::Disconnected) {
            return;
        }
        if let Err(reason) = self.notification_inner(transport, fw, notification) {
            self.fail(reason);
        }
    }
    fn notification_inner(
        &mut self,
        transport: &mut Transport,
        fw: &Firmware<'_>,
        notification: &Notification,
    ) -> Result<(), &'static str> {
        if notification.group == station::SESSION_GROUP
            && notification.id == station::SESSION_NOTIFICATION
        {
            self.session_started = station::session_started(&notification.payload)?;
        }
        if let Some((sequence, command)) = self.pending.as_ref() {
            if notification.sequence == *sequence {
                if notification.group != command.group || notification.id != command.id {
                    return Err("AX201 station response mismatch");
                }
                command.validate_response(&notification.payload)?;
                self.pending = None;
            }
        }
        if self.queue_request == Some(notification.sequence) {
            let (group, id, _) = transport.queue_allocation_command()?;
            if notification.group != group || notification.id != id {
                return Err("AX201 TX queue response mismatch");
            }
            let queue = transport.activate_tx_queue(&notification.payload)?;
            self.queue_request = None;
            if self.queue.is_none() {
                self.queue = Some(queue);
                self.allocate_queue(transport, 0)?;
            } else {
                self.data_queue = Some(queue);
                self.state = State::Session;
            }
            self.deadline = now() + 2000;
        }
        if matches!(notification.group, 0 | 1) && notification.id == 0x1c {
            let report = data::tx_report(&notification.payload)?;
            if self.data_queue == Some(report.queue) {
                if let Some(success) = self
                    .key_confirmation
                    .and_then(|token| transport.tx_completion_status(report.queue, token))
                {
                    if !success {
                        return Err("AX201 WPA2 confirmation was not acknowledged");
                    }
                    self.key_confirmation = None;
                    self.state = State::Connected;
                    crate::log_warn!(target: "net"; "iwl-ax201: stage=link-up security=wpa2-ccmp data=ethernet\n");
                }
            }
        }
        if notification.group == 0 && notification.id == 0xc1 {
            if let Ok(Some(raw)) = data::rx_mpdu(&notification.payload) {
                self.frame(transport, fw, raw)?;
            }
        }
        Ok(())
    }

    pub fn poll(&mut self, transport: &mut Transport) {
        if matches!(
            self.state,
            State::Failed | State::Disconnected | State::Connected
        ) {
            return;
        }
        let result = self.poll_inner(transport);
        if let Err(reason) = result {
            self.fail(reason);
        }
    }
    fn poll_inner(&mut self, transport: &mut Transport) -> Result<(), &'static str> {
        if self.pending.is_none()
            && matches!(self.state, State::Configure | State::AssociatedConfig)
        {
            if let Some(command) = self.commands.pop_front() {
                let sequence = transport.send_command(
                    command.group,
                    command.id,
                    command.version,
                    &command.payload,
                )?;
                crate::log_warn!(target: "net";
                    "iwl-ax201: stage=station-command cmd={:02x}:{:02x} version={} bytes={} seq=0x{:04x}\n",
                    command.group, command.id, command.version, command.payload.len(), sequence);
                self.pending = Some((sequence, command));
                self.deadline = now() + 2000;
            } else if self.state == State::Configure {
                self.allocate_queue(transport, 16)?;
                self.state = State::Queue;
                self.deadline = now() + 2000;
                crate::log_warn!(target: "net"; "iwl-ax201: stage=station-context-ready\n");
            } else if self.handshake.is_some() {
                self.state = State::Keys;
                self.deadline = now() + 10_000;
                while let Some(packet) = self.early_eapol.pop_front() {
                    self.handle_eapol(transport, &packet)?;
                }
                if !self.eapol_seen {
                    self.send_eapol(transport, &[2, 1, 0, 0])?; // IEEE 802.1X EAPOL-Start
                }
            } else {
                self.state = State::Connected;
                crate::log_warn!(target: "net"; "iwl-ax201: stage=link-up security=open data=ethernet\n");
            }
        }
        if self.state == State::Session && self.session_started {
            self.send_authentication(transport)?;
        }
        if now() >= self.deadline && self.state != State::Connected {
            return Err("AX201 station protocol timeout");
        }
        Ok(())
    }
    pub fn send(&mut self, transport: &mut Transport, ethernet: &[u8]) -> Result<(), &'static str> {
        if !self.connected() {
            return Err("AX201 controlled port closed");
        }
        let sequence = self.next_sequence();
        let frame =
            data::encapsulate_ethernet(ethernet, self.config.mac, self.config.bss.bssid, sequence)?;
        let frame = match self.pairwise.as_mut() {
            Some(key) => key.encrypt(&frame)?,
            None => frame,
        };
        self.transmit_raw(transport, &frame)?;
        self.tx_packets += 1;
        if self.tx_packets == 1 {
            crate::log_warn!(target: "net"; "iwl-ax201: stage=ethernet-tx bytes={} protected={}\n", ethernet.len(), self.pairwise.is_some());
        }
        Ok(())
    }
}
