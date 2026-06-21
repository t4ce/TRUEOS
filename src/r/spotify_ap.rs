extern crate alloc;

use alloc::{format, string::String, vec, vec::Vec};

use hmac::{Hmac, Mac};
use rsa::{BigUint, Pkcs1v15Sign, RsaPublicKey};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use v::vnet as api;

use crate::r::net::VNet;
use crate::r::spotify_zeroconf::SpotifyCredential;

type HmacSha1 = Hmac<Sha1>;

const SPOTIFY_VERSION: u64 = 124_200_290;
const PACKET_LOGIN: u8 = 0xab;
const PACKET_AP_WELCOME: u8 = 0xac;
const PACKET_AUTH_FAILURE: u8 = 0xad;
const PACKET_PING: u8 = 0x04;
const PACKET_PONG: u8 = 0x49;
const PACKET_PONG_ACK: u8 = 0x4a;
const PACKET_REQUEST_KEY: u8 = 0x0c;
const PACKET_AES_KEY: u8 = 0x0d;
const PACKET_AES_KEY_ERROR: u8 = 0x0e;
const PACKET_COUNTRY_CODE: u8 = 0x1b;
const PACKET_PRODUCT_INFO: u8 = 0x50;
const PACKET_LICENSE_VERSION: u8 = 0x76;
const PACKET_MERCURY_REQ: u8 = 0xb2;
const PACKET_MERCURY_SUB: u8 = 0xb3;
const PACKET_MERCURY_UNSUB: u8 = 0xb4;
const PACKET_MERCURY_EVENT: u8 = 0xb5;
const MAC_SIZE: usize = 4;
const INITIAL_PING_TIMEOUT_MS: u64 = 20_000;
const PING_TIMEOUT_MS: u64 = 80_000;
const PONG_DELAY_MS: u64 = 60_000;
const PONG_ACK_TIMEOUT_MS: u64 = 20_000;
const KEYMASTER_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";

const SERVER_KEY: [u8; 256] = [
    0xac, 0xe0, 0x46, 0x0b, 0xff, 0xc2, 0x30, 0xaf, 0xf4, 0x6b, 0xfe, 0xc3, 0xbf, 0xbf, 0x86, 0x3d,
    0xa1, 0x91, 0xc6, 0xcc, 0x33, 0x6c, 0x93, 0xa1, 0x4f, 0xb3, 0xb0, 0x16, 0x12, 0xac, 0xac, 0x6a,
    0xf1, 0x80, 0xe7, 0xf6, 0x14, 0xd9, 0x42, 0x9d, 0xbe, 0x2e, 0x34, 0x66, 0x43, 0xe3, 0x62, 0xd2,
    0x32, 0x7a, 0x1a, 0x0d, 0x92, 0x3b, 0xae, 0xdd, 0x14, 0x02, 0xb1, 0x81, 0x55, 0x05, 0x61, 0x04,
    0xd5, 0x2c, 0x96, 0xa4, 0x4c, 0x1e, 0xcc, 0x02, 0x4a, 0xd4, 0xb2, 0x0c, 0x00, 0x1f, 0x17, 0xed,
    0xc2, 0x2f, 0xc4, 0x35, 0x21, 0xc8, 0xf0, 0xcb, 0xae, 0xd2, 0xad, 0xd7, 0x2b, 0x0f, 0x9d, 0xb3,
    0xc5, 0x32, 0x1a, 0x2a, 0xfe, 0x59, 0xf3, 0x5a, 0x0d, 0xac, 0x68, 0xf1, 0xfa, 0x62, 0x1e, 0xfb,
    0x2c, 0x8d, 0x0c, 0xb7, 0x39, 0x2d, 0x92, 0x47, 0xe3, 0xd7, 0x35, 0x1a, 0x6d, 0xbd, 0x24, 0xc2,
    0xae, 0x25, 0x5b, 0x88, 0xff, 0xab, 0x73, 0x29, 0x8a, 0x0b, 0xcc, 0xcd, 0x0c, 0x58, 0x67, 0x31,
    0x89, 0xe8, 0xbd, 0x34, 0x80, 0x78, 0x4a, 0x5f, 0xc9, 0x6b, 0x89, 0x9d, 0x95, 0x6b, 0xfc, 0x86,
    0xd7, 0x4f, 0x33, 0xa6, 0x78, 0x17, 0x96, 0xc9, 0xc3, 0x2d, 0x0d, 0x32, 0xa5, 0xab, 0xcd, 0x05,
    0x27, 0xe2, 0xf7, 0x10, 0xa3, 0x96, 0x13, 0xc4, 0x2f, 0x99, 0xc0, 0x27, 0xbf, 0xed, 0x04, 0x9c,
    0x3c, 0x27, 0x58, 0x04, 0xb6, 0xb2, 0x19, 0xf9, 0xc1, 0x2f, 0x02, 0xe9, 0x48, 0x63, 0xec, 0xa1,
    0xb6, 0x42, 0xa0, 0x9d, 0x48, 0x25, 0xf8, 0xb3, 0x9d, 0xd0, 0xe8, 0x6a, 0xf9, 0x48, 0x4d, 0xa1,
    0xc2, 0xba, 0x86, 0x30, 0x42, 0xea, 0x9d, 0xb3, 0x08, 0x6c, 0x19, 0x0e, 0x48, 0xb3, 0x9d, 0x66,
    0xeb, 0x00, 0x06, 0xa2, 0x5a, 0xee, 0xa1, 0x1b, 0x13, 0x87, 0x3c, 0xd7, 0x19, 0xe6, 0x55, 0xbd,
];

const DH_PRIME: [u8; 96] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xc9, 0x0f, 0xda, 0xa2, 0x21, 0x68, 0xc2, 0x34,
    0xc4, 0xc6, 0x62, 0x8b, 0x80, 0xdc, 0x1c, 0xd1, 0x29, 0x02, 0x4e, 0x08, 0x8a, 0x67, 0xcc, 0x74,
    0x02, 0x0b, 0xbe, 0xa6, 0x3b, 0x13, 0x9b, 0x22, 0x51, 0x4a, 0x08, 0x79, 0x8e, 0x34, 0x04, 0xdd,
    0xef, 0x95, 0x19, 0xb3, 0xcd, 0x3a, 0x43, 0x1b, 0x30, 0x2b, 0x0a, 0x6d, 0xf2, 0x5f, 0x14, 0x37,
    0x4f, 0xe1, 0x35, 0x6d, 0x6d, 0x51, 0xc2, 0x45, 0xe4, 0x85, 0xb5, 0x76, 0x62, 0x5e, 0x7e, 0xc6,
    0xf4, 0x4c, 0x42, 0xe9, 0xa6, 0x3a, 0x36, 0x20, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
];

#[derive(Clone, Debug)]
pub struct ApAuthResult {
    pub canonical_username_len: usize,
    pub reusable_auth_type: Option<u32>,
    pub reusable_auth_data_len: usize,
}

pub struct ApSession {
    vnet: VNet,
    handle: api::NetHandle,
    codec: ApCodec,
    read_buf: Vec<u8>,
    welcome: ApAuthResult,
    keepalive: KeepAliveState,
    keepalive_deadline: embassy_time::Instant,
    mercury_seq: u64,
    pending_keymaster_seq: Option<u64>,
    keymaster_failure: Option<KeymasterFailure>,
    keymaster_token: Option<KeymasterToken>,
    audio_key_seq: u32,
    pending_audio_key_seq: Option<u32>,
    audio_key_result: Option<AudioKeyResult>,
}

#[derive(Clone, Debug)]
pub struct KeymasterToken {
    pub access_token: String,
    pub expires_in: u64,
    pub token_type: String,
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct KeymasterFailure {
    pub seq: Option<u64>,
    pub status_code: Option<i64>,
    pub payload_len: usize,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct AudioKeyResult {
    pub seq: u32,
    pub key: Option<[u8; 16]>,
    pub error_code: Option<(u8, u8)>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeymasterTokenData {
    access_token: String,
    expires_in: u64,
    token_type: String,
    scope: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeepAliveState {
    ExpectingPing,
    PendingPong,
    ExpectingPongAck,
}

#[derive(Clone, Debug)]
pub enum ApSessionEvent {
    Idle,
    PongSent,
    Packet {
        cmd: u8,
        payload_len: usize,
    },
    Mercury {
        cmd: u8,
        seq: Option<u64>,
        seq_len: usize,
        flags: u8,
        parts: usize,
        uri_len: usize,
        status_code: Option<i64>,
        first_payload_len: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MercuryMethod {
    Get,
    Sub,
    Unsub,
    Send,
}

pub async fn authenticate_probe(
    vnet: &VNet,
    handle: api::NetHandle,
    credential: &SpotifyCredential,
    device_id: &str,
) -> Result<ApAuthResult, String> {
    let (welcome, _, _) = authenticate_on_socket(vnet, handle, credential, device_id).await?;
    Ok(welcome)
}

pub async fn authenticate_session(
    vnet: VNet,
    handle: api::NetHandle,
    credential: &SpotifyCredential,
    device_id: &str,
) -> Result<ApSession, String> {
    let (welcome, codec, read_buf) =
        match authenticate_on_socket(&vnet, handle, credential, device_id).await {
            Ok(auth) => auth,
            Err(err) => {
                let _ = vnet.submit(api::Command::Close { handle });
                return Err(err);
            }
        };
    Ok(ApSession {
        vnet,
        handle,
        codec,
        read_buf,
        welcome,
        keepalive: KeepAliveState::ExpectingPing,
        keepalive_deadline: embassy_time::Instant::now()
            + embassy_time::Duration::from_millis(INITIAL_PING_TIMEOUT_MS),
        mercury_seq: 0,
        pending_keymaster_seq: None,
        keymaster_failure: None,
        keymaster_token: None,
        audio_key_seq: 0,
        pending_audio_key_seq: None,
        audio_key_result: None,
    })
}

async fn authenticate_on_socket(
    vnet: &VNet,
    handle: api::NetHandle,
    credential: &SpotifyCredential,
    device_id: &str,
) -> Result<(ApAuthResult, ApCodec, Vec<u8>), String> {
    let keys = DhLocalKeys::random()?;
    let public_key = keys.public_key();
    let mut accumulator = send_client_hello(vnet, handle, public_key).await?;
    crate::log!(
        "spotify-ap: client hello sent bytes={} accumulated={}\n",
        accumulator.len(),
        accumulator.len()
    );

    let mut read_buf = Vec::new();
    let response = read_plain_packet(vnet, handle, &mut read_buf, &mut accumulator, 8_000).await?;
    let challenge = parse_ap_challenge(response.as_slice())?;
    crate::log!(
        "spotify-ap: challenge received gs_len={} sig_len={} accumulated={}\n",
        challenge.gs.len(),
        challenge.gs_signature.len(),
        accumulator.len()
    );

    verify_server_key(challenge.gs.as_slice(), challenge.gs_signature.as_slice())?;
    crate::log!("spotify-ap: server signature verified\n");

    let shared_secret = keys.shared_secret(challenge.gs.as_slice());
    let (challenge_hmac, send_key, recv_key) =
        compute_keys(shared_secret.as_slice(), accumulator.as_slice())?;
    send_client_response(vnet, handle, challenge_hmac.as_slice()).await?;
    crate::log!(
        "spotify-ap: plaintext response sent hmac_len={} send_key_len={} recv_key_len={}\n",
        challenge_hmac.len(),
        send_key.len(),
        recv_key.len()
    );

    let mut codec = ApCodec::new(send_key.as_slice(), recv_key.as_slice());
    let login = encode_login(credential, device_id);
    let encrypted = codec.encode(PACKET_LOGIN, login.as_slice());
    vnet.send_tcp_all(handle, encrypted.as_slice())
        .map_err(|_| String::from("login-send"))?;
    crate::log!(
        "spotify-ap: login packet sent user_len={} auth_type={} auth_data_len={} frame_bytes={}\n",
        credential.username.len(),
        credential.auth_type,
        credential.auth_data.len(),
        encrypted.len()
    );

    let (cmd, payload) =
        read_encrypted_packet(vnet, handle, &mut read_buf, &mut codec, 10_000).await?;
    match cmd {
        PACKET_AP_WELCOME => {
            let welcome = parse_ap_welcome(payload.as_slice());
            crate::log!(
                "spotify-ap: auth welcome canonical_user_len={} reusable_auth_type={:?} reusable_auth_data_len={}\n",
                welcome.canonical_username_len,
                welcome.reusable_auth_type,
                welcome.reusable_auth_data_len
            );
            Ok((welcome, codec, read_buf))
        }
        PACKET_AUTH_FAILURE => {
            let code = parse_auth_failure_code(payload.as_slice());
            Err(format!("auth-failure code={:?}", code))
        }
        other => Err(format!("unexpected-login-response cmd=0x{other:02x}")),
    }
}

impl ApSession {
    pub fn welcome(&self) -> &ApAuthResult {
        &self.welcome
    }

    pub fn handle_id(&self) -> u32 {
        self.handle.0
    }

    pub fn keymaster_token(&self) -> Option<&KeymasterToken> {
        self.keymaster_token.as_ref()
    }

    pub fn take_keymaster_failure(&mut self) -> Option<KeymasterFailure> {
        self.keymaster_failure.take()
    }

    pub fn take_audio_key_result(&mut self) -> Option<AudioKeyResult> {
        self.audio_key_result.take()
    }

    pub fn clear_audio_key_state(&mut self) {
        self.pending_audio_key_seq = None;
        self.audio_key_result = None;
    }

    pub fn close(&self) {
        let _ = self.vnet.submit(api::Command::Close {
            handle: self.handle,
        });
    }

    pub async fn tick(&mut self) -> Result<ApSessionEvent, String> {
        while let Some((cmd, payload)) = self.try_read_packet()? {
            return self.dispatch_packet(cmd, payload).await;
        }

        if embassy_time::Instant::now() >= self.keepalive_deadline {
            match self.keepalive {
                KeepAliveState::PendingPong => {
                    self.send_packet(PACKET_PONG, &[0, 0, 0, 0])?;
                    self.keepalive = KeepAliveState::ExpectingPongAck;
                    self.keepalive_deadline = embassy_time::Instant::now()
                        + embassy_time::Duration::from_millis(PONG_ACK_TIMEOUT_MS);
                    crate::log!("spotify-ap: keepalive pong sent\n");
                    Ok(ApSessionEvent::PongSent)
                }
                KeepAliveState::ExpectingPing | KeepAliveState::ExpectingPongAck => {
                    Err(format!("keepalive-timeout state={:?}", self.keepalive))
                }
            }
        } else {
            embassy_time::Timer::after(embassy_time::Duration::from_millis(5)).await;
            Ok(ApSessionEvent::Idle)
        }
    }

    pub fn send_mercury(
        &mut self,
        method: MercuryMethod,
        uri: &str,
        content_type: Option<&str>,
        payloads: &[&[u8]],
    ) -> Result<u64, String> {
        self.mercury_seq = self.mercury_seq.wrapping_add(1).max(1);
        let seq = self.mercury_seq;
        let packet = encode_mercury_request(seq, method, uri, content_type, payloads)?;
        self.send_packet(method.packet_cmd(), packet.as_slice())?;
        crate::log!(
            "spotify-ap: mercury send method={:?} seq={} uri_len={} payload_parts={}\n",
            method,
            seq,
            uri.len(),
            payloads.len()
        );
        Ok(seq)
    }

    pub fn request_keymaster_token(
        &mut self,
        scopes: &str,
        device_id: &str,
    ) -> Result<u64, String> {
        let uri = format!(
            "hm://keymaster/token/authenticated?scope={}&client_id={}&device_id={}",
            scopes, KEYMASTER_CLIENT_ID, device_id
        );
        let seq = self.send_mercury(MercuryMethod::Get, uri.as_str(), None, &[])?;
        self.pending_keymaster_seq = Some(seq);
        self.keymaster_failure = None;
        Ok(seq)
    }

    pub fn request_audio_key(&mut self, track_gid: &[u8], file_id: &[u8]) -> Result<u32, String> {
        if track_gid.len() != 16 {
            return Err(format!("bad-track-gid-len {}", track_gid.len()));
        }
        if file_id.len() > 20 {
            return Err(format!("bad-file-id-len {}", file_id.len()));
        }

        self.audio_key_seq = self.audio_key_seq.wrapping_add(1);
        let seq = self.audio_key_seq;
        let mut normalized_file = [0u8; 20];
        normalized_file[..file_id.len()].copy_from_slice(file_id);

        let mut payload = Vec::with_capacity(20 + 16 + 4 + 2);
        payload.extend_from_slice(&normalized_file);
        payload.extend_from_slice(track_gid);
        push_u32_be(&mut payload, seq);
        payload.extend_from_slice(&[0, 0]);

        self.pending_audio_key_seq = Some(seq);
        self.audio_key_result = None;
        self.send_packet(PACKET_REQUEST_KEY, payload.as_slice())?;
        crate::log!(
            "spotify-ap: audio key request sent seq={} file_id_len={} track_gid_len={} payload_len={}\n",
            seq,
            file_id.len(),
            track_gid.len(),
            payload.len()
        );
        Ok(seq)
    }

    fn send_packet(&mut self, cmd: u8, payload: &[u8]) -> Result<(), String> {
        let encrypted = self.codec.encode(cmd, payload);
        self.vnet
            .send_tcp_all(self.handle, encrypted.as_slice())
            .map_err(|_| format!("packet-send cmd=0x{cmd:02x}"))
    }

    fn try_read_packet(&mut self) -> Result<Option<(u8, Vec<u8>)>, String> {
        while let Some(ev) = self.vnet.pop_event() {
            match ev {
                api::Event::TcpData { handle, data } if handle == self.handle => {
                    self.read_buf.extend_from_slice(data.as_slice());
                }
                api::Event::Closed { handle } if handle == self.handle => {
                    return Err(String::from("tcp-closed"));
                }
                api::Event::Error { msg } => return Err(format!("tcp-error {msg}")),
                _ => {}
            }
        }

        if self.read_buf.len() < 3 {
            return Ok(None);
        }
        let mut header = [0u8; 3];
        header.copy_from_slice(&self.read_buf[..3]);
        let (_, size) = self.codec.peek_header(&header);
        if size > 64 * 1024 {
            return Err(format!("encrypted-size {size}"));
        }
        let total = 3usize
            .checked_add(size)
            .and_then(|v| v.checked_add(MAC_SIZE))
            .ok_or_else(|| String::from("encrypted-size-overflow"))?;
        if self.read_buf.len() < total {
            return Ok(None);
        }

        let mut header: Vec<u8> = self.read_buf.drain(..3).collect();
        let (cmd, decoded_size) = self.codec.decode_header(header.as_mut_slice());
        if decoded_size != size {
            return Err(String::from("encrypted-header-race"));
        }
        let mut payload_and_mac: Vec<u8> = self.read_buf.drain(..size + MAC_SIZE).collect();
        let payload = self
            .codec
            .decode_payload(payload_and_mac.as_mut_slice(), size)?;
        Ok(Some((cmd, payload)))
    }

    async fn dispatch_packet(
        &mut self,
        cmd: u8,
        payload: Vec<u8>,
    ) -> Result<ApSessionEvent, String> {
        match cmd {
            PACKET_PING => {
                self.keepalive = KeepAliveState::PendingPong;
                self.keepalive_deadline = embassy_time::Instant::now()
                    + embassy_time::Duration::from_millis(PONG_DELAY_MS);
                let server_timestamp = payload.get(..4).map(read_u32_be).unwrap_or_default();
                crate::log!(
                    "spotify-ap: keepalive ping server_ts={} payload_len={}\n",
                    server_timestamp,
                    payload.len()
                );
                Ok(ApSessionEvent::Packet {
                    cmd,
                    payload_len: payload.len(),
                })
            }
            PACKET_PONG_ACK => {
                self.keepalive = KeepAliveState::ExpectingPing;
                self.keepalive_deadline = embassy_time::Instant::now()
                    + embassy_time::Duration::from_millis(PING_TIMEOUT_MS);
                crate::log!("spotify-ap: keepalive pong ack\n");
                Ok(ApSessionEvent::Packet {
                    cmd,
                    payload_len: payload.len(),
                })
            }
            PACKET_AES_KEY | PACKET_AES_KEY_ERROR => {
                let seq = payload.get(..4).map(read_u32_be).unwrap_or_default();
                if Some(seq) != self.pending_audio_key_seq {
                    crate::log!(
                        "spotify-ap: audio key unexpected cmd=0x{:02x} seq={} pending={:?} payload_len={}\n",
                        cmd,
                        seq,
                        self.pending_audio_key_seq,
                        payload.len()
                    );
                } else if cmd == PACKET_AES_KEY {
                    if payload.len() >= 20 {
                        let mut key = [0u8; 16];
                        key.copy_from_slice(&payload[4..20]);
                        self.audio_key_result = Some(AudioKeyResult {
                            seq,
                            key: Some(key),
                            error_code: None,
                        });
                        self.pending_audio_key_seq = None;
                        crate::log!("spotify-ap: audio key ready seq={} key_len=16\n", seq);
                    } else {
                        self.audio_key_result = Some(AudioKeyResult {
                            seq,
                            key: None,
                            error_code: None,
                        });
                        self.pending_audio_key_seq = None;
                        crate::log!(
                            "spotify-ap: audio key short seq={} payload_len={}\n",
                            seq,
                            payload.len()
                        );
                    }
                } else {
                    let error_code = (
                        payload.get(4).copied().unwrap_or_default(),
                        payload.get(5).copied().unwrap_or_default(),
                    );
                    self.audio_key_result = Some(AudioKeyResult {
                        seq,
                        key: None,
                        error_code: Some(error_code),
                    });
                    self.pending_audio_key_seq = None;
                    crate::log!(
                        "spotify-ap: audio key rejected seq={} err={:02x}:{:02x} payload_len={}\n",
                        seq,
                        error_code.0,
                        error_code.1,
                        payload.len()
                    );
                }
                Ok(ApSessionEvent::Packet {
                    cmd,
                    payload_len: payload.len(),
                })
            }
            PACKET_MERCURY_REQ | PACKET_MERCURY_SUB | PACKET_MERCURY_UNSUB
            | PACKET_MERCURY_EVENT => {
                let summary = parse_mercury_summary(payload.as_slice());
                if cmd == PACKET_MERCURY_REQ && summary.seq == self.pending_keymaster_seq {
                    if summary.status_code == Some(200) {
                        if let Some(first_payload) = summary.first_payload.as_deref() {
                            match parse_keymaster_token(first_payload) {
                                Ok(token) => {
                                    crate::log!(
                                        "spotify-ap: keymaster token ready seq={:?} token_type={} token_len={} expires_in={} scope_count={}\n",
                                        summary.seq,
                                        token.token_type.as_str(),
                                        token.access_token.len(),
                                        token.expires_in,
                                        token.scopes.len()
                                    );
                                    self.keymaster_token = Some(token);
                                    self.pending_keymaster_seq = None;
                                }
                                Err(err) => {
                                    crate::log!(
                                        "spotify-ap: keymaster token parse failed seq={:?} err={}\n",
                                        summary.seq,
                                        err.as_str()
                                    );
                                    self.keymaster_failure = Some(KeymasterFailure {
                                        seq: summary.seq,
                                        status_code: summary.status_code,
                                        payload_len: summary.first_payload_len,
                                        reason: format!("parse {}", err),
                                    });
                                    self.pending_keymaster_seq = None;
                                }
                            }
                        } else {
                            crate::log!(
                                "spotify-ap: keymaster token empty response seq={:?}\n",
                                summary.seq
                            );
                            self.keymaster_failure = Some(KeymasterFailure {
                                seq: summary.seq,
                                status_code: summary.status_code,
                                payload_len: 0,
                                reason: String::from("empty-response"),
                            });
                            self.pending_keymaster_seq = None;
                        }
                    } else {
                        crate::log!(
                            "spotify-ap: keymaster token rejected seq={:?} status={:?} payload_len={} preview={}\n",
                            summary.seq,
                            summary.status_code,
                            summary.first_payload_len,
                            summary.error_preview.as_deref().unwrap_or("")
                        );
                        self.keymaster_failure = Some(KeymasterFailure {
                            seq: summary.seq,
                            status_code: summary.status_code,
                            payload_len: summary.first_payload_len,
                            reason: summary
                                .error_preview
                                .clone()
                                .unwrap_or_else(|| String::from("status")),
                        });
                        self.pending_keymaster_seq = None;
                    }
                }
                crate::log!(
                    "spotify-ap: mercury packet kind={} seq={:?} seq_len={} flags=0x{:02x} parts={} uri_len={} content_type_len={} status={:?} first_payload_len={} error_preview={}\n",
                    packet_name(cmd),
                    summary.seq,
                    summary.seq_len,
                    summary.flags,
                    summary.parts,
                    summary.uri_len,
                    summary.content_type_len,
                    summary.status_code,
                    summary.first_payload_len,
                    summary.error_preview.as_deref().unwrap_or("")
                );
                Ok(ApSessionEvent::Mercury {
                    cmd,
                    seq: summary.seq,
                    seq_len: summary.seq_len,
                    flags: summary.flags,
                    parts: summary.parts,
                    uri_len: summary.uri_len,
                    status_code: summary.status_code,
                    first_payload_len: summary.first_payload_len,
                })
            }
            PACKET_COUNTRY_CODE | PACKET_PRODUCT_INFO | PACKET_LICENSE_VERSION => {
                crate::log!(
                    "spotify-ap: session packet kind={} payload_len={}\n",
                    packet_name(cmd),
                    payload.len()
                );
                Ok(ApSessionEvent::Packet {
                    cmd,
                    payload_len: payload.len(),
                })
            }
            _ => {
                crate::log!(
                    "spotify-ap: ignored packet cmd=0x{:02x} payload_len={}\n",
                    cmd,
                    payload.len()
                );
                Ok(ApSessionEvent::Packet {
                    cmd,
                    payload_len: payload.len(),
                })
            }
        }
    }
}

impl MercuryMethod {
    fn packet_cmd(self) -> u8 {
        match self {
            Self::Get | Self::Send => PACKET_MERCURY_REQ,
            Self::Sub => PACKET_MERCURY_SUB,
            Self::Unsub => PACKET_MERCURY_UNSUB,
        }
    }

    fn as_wire(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Sub => "SUB",
            Self::Unsub => "UNSUB",
            Self::Send => "SEND",
        }
    }
}

async fn send_client_hello(
    vnet: &VNet,
    handle: api::NetHandle,
    public_key: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let mut nonce = [0u8; 16];
    if !crate::tyche::fill_bytes(&mut nonce) {
        return Err(String::from("rng-client-nonce"));
    }

    let hello = encode_client_hello(public_key.as_slice(), &nonce);
    let size = 2usize
        .checked_add(4)
        .and_then(|v| v.checked_add(hello.len()))
        .ok_or_else(|| String::from("hello-size"))?;
    let mut frame = Vec::with_capacity(size);
    frame.extend_from_slice(&[0, 4]);
    push_u32_be(&mut frame, size as u32);
    frame.extend_from_slice(hello.as_slice());

    vnet.send_tcp_all(handle, frame.as_slice())
        .map_err(|_| String::from("hello-send"))?;
    Ok(frame)
}

async fn send_client_response(
    vnet: &VNet,
    handle: api::NetHandle,
    hmac: &[u8],
) -> Result<(), String> {
    let body = encode_client_response_plaintext(hmac);
    let mut frame = Vec::with_capacity(4 + body.len());
    push_u32_be(&mut frame, (4 + body.len()) as u32);
    frame.extend_from_slice(body.as_slice());
    vnet.send_tcp_all(handle, frame.as_slice())
        .map_err(|_| String::from("plain-response-send"))
}

async fn read_plain_packet(
    vnet: &VNet,
    handle: api::NetHandle,
    read_buf: &mut Vec<u8>,
    accumulator: &mut Vec<u8>,
    timeout_ms: u64,
) -> Result<Vec<u8>, String> {
    let header = read_exact(vnet, handle, read_buf, 4, timeout_ms).await?;
    accumulator.extend_from_slice(header.as_slice());
    let size = read_u32_be(header.as_slice()) as usize;
    if !(4..=32 * 1024).contains(&size) {
        return Err(format!("plain-size {size}"));
    }
    let payload = read_exact(vnet, handle, read_buf, size - 4, timeout_ms).await?;
    accumulator.extend_from_slice(payload.as_slice());
    Ok(payload)
}

async fn read_encrypted_packet(
    vnet: &VNet,
    handle: api::NetHandle,
    read_buf: &mut Vec<u8>,
    codec: &mut ApCodec,
    timeout_ms: u64,
) -> Result<(u8, Vec<u8>), String> {
    let mut header = read_exact(vnet, handle, read_buf, 3, timeout_ms).await?;
    let (cmd, size) = codec.decode_header(header.as_mut_slice());
    if size > 32 * 1024 {
        return Err(format!("encrypted-size {size}"));
    }
    let mut payload_and_mac =
        read_exact(vnet, handle, read_buf, size + MAC_SIZE, timeout_ms).await?;
    let payload = codec.decode_payload(payload_and_mac.as_mut_slice(), size)?;
    Ok((cmd, payload))
}

async fn read_exact(
    vnet: &VNet,
    handle: api::NetHandle,
    read_buf: &mut Vec<u8>,
    len: usize,
    timeout_ms: u64,
) -> Result<Vec<u8>, String> {
    let deadline = embassy_time::Instant::now() + embassy_time::Duration::from_millis(timeout_ms);
    let mut out = Vec::with_capacity(len);
    drain_read_buf(read_buf, &mut out, len);
    while out.len() < len {
        while let Some(ev) = vnet.pop_event() {
            match ev {
                api::Event::TcpData { handle: h, data } if h == handle => {
                    let needed = len - out.len();
                    let bytes = data.as_slice();
                    if bytes.len() <= needed {
                        out.extend_from_slice(bytes);
                    } else {
                        out.extend_from_slice(&bytes[..needed]);
                        read_buf.extend_from_slice(&bytes[needed..]);
                        return Ok(out);
                    }
                }
                api::Event::Closed { handle: h } if h == handle => {
                    return Err(String::from("tcp-closed"));
                }
                api::Event::Error { msg } => return Err(format!("tcp-error {msg}")),
                _ => {}
            }
        }
        if embassy_time::Instant::now() >= deadline {
            return Err(format!("tcp-read-timeout len={} got={}", len, out.len()));
        }
        embassy_time::Timer::after(embassy_time::Duration::from_millis(5)).await;
    }
    Ok(out)
}

fn drain_read_buf(read_buf: &mut Vec<u8>, out: &mut Vec<u8>, len: usize) {
    if read_buf.is_empty() || out.len() >= len {
        return;
    }
    let needed = len - out.len();
    let take = core::cmp::min(needed, read_buf.len());
    out.extend_from_slice(&read_buf[..take]);
    if take == read_buf.len() {
        read_buf.clear();
    } else {
        read_buf.drain(..take);
    }
}

struct DhLocalKeys {
    private_key: BigUint,
    public_key: BigUint,
}

impl DhLocalKeys {
    fn random() -> Result<Self, String> {
        let mut bytes = [0u8; 95];
        if !crate::tyche::fill_bytes(&mut bytes) {
            return Err(String::from("rng-dh"));
        }
        let private_key = BigUint::from_bytes_le(&bytes);
        let generator = BigUint::from(2u8);
        let prime = BigUint::from_bytes_be(&DH_PRIME);
        let public_key = generator.modpow(&private_key, &prime);
        Ok(Self {
            private_key,
            public_key,
        })
    }

    fn public_key(&self) -> Vec<u8> {
        self.public_key.to_bytes_be()
    }

    fn shared_secret(&self, remote_key: &[u8]) -> Vec<u8> {
        let remote = BigUint::from_bytes_be(remote_key);
        let prime = BigUint::from_bytes_be(&DH_PRIME);
        remote.modpow(&self.private_key, &prime).to_bytes_be()
    }
}

fn verify_server_key(remote_key: &[u8], remote_signature: &[u8]) -> Result<(), String> {
    let n = BigUint::from_bytes_be(&SERVER_KEY);
    let e = BigUint::from(65_537u32);
    let key = RsaPublicKey::new(n, e).map_err(|err| format!("rsa-key {err:?}"))?;
    let hash = Sha1::digest(remote_key);
    key.verify(Pkcs1v15Sign::new::<Sha1>(), hash.as_slice(), remote_signature)
        .map_err(|err| format!("rsa-verify {err:?}"))
}

fn compute_keys(
    shared_secret: &[u8],
    packets: &[u8],
) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), String> {
    let mut data = Vec::with_capacity(0x64);
    for i in 1..6 {
        let mut mac = <HmacSha1 as Mac>::new_from_slice(shared_secret)
            .map_err(|_| String::from("hmac-key"))?;
        mac.update(packets);
        mac.update(&[i]);
        data.extend_from_slice(&mac.finalize().into_bytes());
    }

    let mut mac =
        <HmacSha1 as Mac>::new_from_slice(&data[..0x14]).map_err(|_| String::from("hmac-key"))?;
    mac.update(packets);

    Ok((mac.finalize().into_bytes().to_vec(), data[0x14..0x34].to_vec(), data[0x34..0x54].to_vec()))
}

fn encode_client_hello(public_key: &[u8], nonce: &[u8; 16]) -> Vec<u8> {
    let mut build_info = Vec::new();
    pb_varint(&mut build_info, 10, 0);
    pb_varint(&mut build_info, 20, 0);
    pb_varint(&mut build_info, 30, 8);
    pb_varint(&mut build_info, 40, SPOTIFY_VERSION);

    let mut dh = Vec::new();
    pb_bytes(&mut dh, 10, public_key);
    pb_varint(&mut dh, 20, 1);

    let mut crypto_hello = Vec::new();
    pb_message(&mut crypto_hello, 10, dh.as_slice());

    let mut out = Vec::new();
    pb_message(&mut out, 10, build_info.as_slice());
    pb_varint(&mut out, 30, 0);
    pb_message(&mut out, 50, crypto_hello.as_slice());
    pb_bytes(&mut out, 60, nonce);
    pb_bytes(&mut out, 70, &[0x1e]);
    out
}

fn encode_client_response_plaintext(hmac: &[u8]) -> Vec<u8> {
    let mut dh = Vec::new();
    pb_bytes(&mut dh, 10, hmac);
    let mut crypto = Vec::new();
    pb_message(&mut crypto, 10, dh.as_slice());

    let mut out = Vec::new();
    pb_message(&mut out, 10, crypto.as_slice());
    pb_message(&mut out, 20, &[]);
    pb_message(&mut out, 30, &[]);
    out
}

fn encode_login(credential: &SpotifyCredential, device_id: &str) -> Vec<u8> {
    let mut login = Vec::new();
    pb_string(&mut login, 10, credential.username.as_str());
    pb_varint(&mut login, 20, credential.auth_type as u64);
    pb_bytes(&mut login, 30, credential.auth_data.as_slice());

    let mut system = Vec::new();
    pb_varint(&mut system, 10, 2);
    pb_varint(&mut system, 60, 0);
    pb_string(&mut system, 90, "librespot-trueos-kernel");
    pb_string(&mut system, 100, device_id);

    let mut out = Vec::new();
    pb_message(&mut out, 10, login.as_slice());
    pb_message(&mut out, 50, system.as_slice());
    pb_string(&mut out, 70, "librespot 0.8.0-trueos");
    out
}

struct ApChallenge {
    gs: Vec<u8>,
    gs_signature: Vec<u8>,
}

fn parse_ap_challenge(data: &[u8]) -> Result<ApChallenge, String> {
    let challenge = pb_find_len(data, 10).ok_or_else(|| String::from("ap-challenge-missing"))?;
    let login_crypto =
        pb_find_len(challenge, 10).ok_or_else(|| String::from("login-challenge-missing"))?;
    let dh = pb_find_len(login_crypto, 10).ok_or_else(|| String::from("dh-challenge-missing"))?;
    let gs = pb_find_len(dh, 10).ok_or_else(|| String::from("gs-missing"))?;
    let sig = pb_find_len(dh, 30).ok_or_else(|| String::from("gs-signature-missing"))?;
    Ok(ApChallenge {
        gs: gs.to_vec(),
        gs_signature: sig.to_vec(),
    })
}

fn parse_auth_failure_code(data: &[u8]) -> Option<u64> {
    pb_find_varint(data, 10)
}

fn parse_ap_welcome(data: &[u8]) -> ApAuthResult {
    ApAuthResult {
        canonical_username_len: pb_find_len(data, 10).map_or(0, |v| v.len()),
        reusable_auth_type: pb_find_varint(data, 30).map(|v| v as u32),
        reusable_auth_data_len: pb_find_len(data, 40).map_or(0, |v| v.len()),
    }
}

struct MercurySummary {
    seq: Option<u64>,
    seq_len: usize,
    flags: u8,
    parts: usize,
    uri_len: usize,
    content_type_len: usize,
    status_code: Option<i64>,
    first_payload_len: usize,
    first_payload: Option<Vec<u8>>,
    error_preview: Option<String>,
}

fn parse_mercury_summary(data: &[u8]) -> MercurySummary {
    let mut index = 0usize;
    let Some(seq_len) = read_u16_be_at(data, &mut index) else {
        return MercurySummary {
            seq: None,
            seq_len: 0,
            flags: 0,
            parts: 0,
            uri_len: 0,
            content_type_len: 0,
            status_code: None,
            first_payload_len: 0,
            first_payload: None,
            error_preview: None,
        };
    };
    let seq = if seq_len == 8 && data.len() >= index + 8 {
        Some(read_u64_be(&data[index..index + 8]))
    } else {
        None
    };
    index = index.saturating_add(seq_len as usize);
    if index >= data.len() {
        return MercurySummary {
            seq,
            seq_len: seq_len as usize,
            flags: 0,
            parts: 0,
            uri_len: 0,
            content_type_len: 0,
            status_code: None,
            first_payload_len: 0,
            first_payload: None,
            error_preview: None,
        };
    }
    let flags = data[index];
    index += 1;
    let parts = read_u16_be_at(data, &mut index).unwrap_or_default() as usize;
    let first_part = read_mercury_part(data, &mut index);
    let uri_len = first_part
        .and_then(|part| pb_find_len(part, 1))
        .map_or(0, |v| v.len());
    let content_type_len = first_part
        .and_then(|part| pb_find_len(part, 2))
        .map_or(0, |v| v.len());
    let status_code = first_part.and_then(|part| pb_find_sint(part, 4));
    let first_payload = read_mercury_part(data, &mut index);
    let first_payload_len = first_payload.map_or(0, |part| part.len());
    let error_preview = if status_code.unwrap_or(0) >= 400 {
        first_payload.map(error_text_preview)
    } else {
        None
    };
    MercurySummary {
        seq,
        seq_len: seq_len as usize,
        flags,
        parts,
        uri_len,
        content_type_len,
        status_code,
        first_payload_len,
        first_payload: first_payload.map(|part| part.to_vec()),
        error_preview,
    }
}

fn parse_keymaster_token(data: &[u8]) -> Result<KeymasterToken, String> {
    let parsed: KeymasterTokenData =
        serde_json::from_slice(data).map_err(|err| format!("json {}", err))?;
    Ok(KeymasterToken {
        access_token: parsed.access_token,
        expires_in: parsed.expires_in,
        token_type: parsed.token_type,
        scopes: parsed.scope,
    })
}

fn read_mercury_part<'a>(data: &'a [u8], index: &mut usize) -> Option<&'a [u8]> {
    let len = read_u16_be_at(data, index)? as usize;
    let end = index.checked_add(len)?;
    if end > data.len() {
        return None;
    }
    let part = &data[*index..end];
    *index = end;
    Some(part)
}

fn error_text_preview(data: &[u8]) -> String {
    const MAX: usize = 120;
    let mut out = String::new();
    for &byte in data.iter().take(MAX) {
        let ch = match byte {
            b'\n' | b'\r' | b'\t' => b' ',
            0x20..=0x7e => byte,
            _ => b'.',
        };
        out.push(ch as char);
    }
    out
}

fn encode_mercury_request(
    seq: u64,
    method: MercuryMethod,
    uri: &str,
    content_type: Option<&str>,
    payloads: &[&[u8]],
) -> Result<Vec<u8>, String> {
    let mut seq_bytes = [0u8; 8];
    write_u64_be(&mut seq_bytes, seq);

    let mut header = Vec::new();
    pb_string(&mut header, 1, uri);
    if let Some(content_type) = content_type {
        pb_string(&mut header, 2, content_type);
    }
    pb_string(&mut header, 3, method.as_wire());

    let part_count = 1usize
        .checked_add(payloads.len())
        .ok_or_else(|| String::from("mercury-part-count"))?;
    if part_count > u16::MAX as usize {
        return Err(String::from("mercury-too-many-parts"));
    }

    let mut out = Vec::new();
    push_u16_be(&mut out, seq_bytes.len() as u16);
    out.extend_from_slice(&seq_bytes);
    out.push(1);
    push_u16_be(&mut out, part_count as u16);
    push_len_prefixed(&mut out, header.as_slice())?;
    for payload in payloads {
        push_len_prefixed(&mut out, payload)?;
    }
    Ok(out)
}

fn push_len_prefixed(out: &mut Vec<u8>, data: &[u8]) -> Result<(), String> {
    if data.len() > u16::MAX as usize {
        return Err(String::from("mercury-part-too-large"));
    }
    push_u16_be(out, data.len() as u16);
    out.extend_from_slice(data);
    Ok(())
}

fn packet_name(cmd: u8) -> &'static str {
    match cmd {
        PACKET_PING => "ping",
        PACKET_PONG => "pong",
        PACKET_PONG_ACK => "pong-ack",
        PACKET_COUNTRY_CODE => "country-code",
        PACKET_PRODUCT_INFO => "product-info",
        PACKET_LICENSE_VERSION => "license-version",
        PACKET_MERCURY_REQ => "mercury-req",
        PACKET_MERCURY_SUB => "mercury-sub",
        PACKET_MERCURY_UNSUB => "mercury-unsub",
        PACKET_MERCURY_EVENT => "mercury-event",
        PACKET_AP_WELCOME => "ap-welcome",
        PACKET_AUTH_FAILURE => "auth-failure",
        _ => "unknown",
    }
}

fn pb_varint(out: &mut Vec<u8>, field: u32, value: u64) {
    write_varint(out, ((field as u64) << 3) | 0);
    write_varint(out, value);
}

fn pb_bytes(out: &mut Vec<u8>, field: u32, value: &[u8]) {
    write_varint(out, ((field as u64) << 3) | 2);
    write_varint(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn pb_string(out: &mut Vec<u8>, field: u32, value: &str) {
    pb_bytes(out, field, value.as_bytes());
}

fn pb_message(out: &mut Vec<u8>, field: u32, value: &[u8]) {
    pb_bytes(out, field, value);
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn read_varint(data: &[u8], index: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;
    while *index < data.len() && shift < 64 {
        let byte = data[*index];
        *index += 1;
        value |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }
    None
}

fn pb_find_len(data: &[u8], needle: u32) -> Option<&[u8]> {
    let mut index = 0usize;
    while index < data.len() {
        let key = read_varint(data, &mut index)?;
        let field = (key >> 3) as u32;
        let wire = key & 0x7;
        match wire {
            0 => {
                let _ = read_varint(data, &mut index)?;
            }
            2 => {
                let len = read_varint(data, &mut index)? as usize;
                let end = index.checked_add(len)?;
                if end > data.len() {
                    return None;
                }
                if field == needle {
                    return Some(&data[index..end]);
                }
                index = end;
            }
            5 => index = index.checked_add(4)?,
            1 => index = index.checked_add(8)?,
            _ => return None,
        }
    }
    None
}

fn pb_find_varint(data: &[u8], needle: u32) -> Option<u64> {
    let mut index = 0usize;
    while index < data.len() {
        let key = read_varint(data, &mut index)?;
        let field = (key >> 3) as u32;
        let wire = key & 0x7;
        match wire {
            0 => {
                let value = read_varint(data, &mut index)?;
                if field == needle {
                    return Some(value);
                }
            }
            2 => {
                let len = read_varint(data, &mut index)? as usize;
                index = index.checked_add(len)?;
            }
            5 => index = index.checked_add(4)?,
            1 => index = index.checked_add(8)?,
            _ => return None,
        }
        if index > data.len() {
            return None;
        }
    }
    None
}

fn pb_find_sint(data: &[u8], needle: u32) -> Option<i64> {
    pb_find_varint(data, needle).map(decode_zigzag)
}

fn decode_zigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ (-((value & 1) as i64))
}

#[derive(Clone)]
struct ApCodec {
    encode_nonce: u32,
    encode_cipher: Shannon,
    decode_nonce: u32,
    decode_cipher: Shannon,
}

impl ApCodec {
    fn new(send_key: &[u8], recv_key: &[u8]) -> Self {
        Self {
            encode_nonce: 0,
            encode_cipher: Shannon::new(send_key),
            decode_nonce: 0,
            decode_cipher: Shannon::new(recv_key),
        }
    }

    fn encode(&mut self, cmd: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + payload.len() + MAC_SIZE);
        out.push(cmd);
        push_u16_be(&mut out, payload.len() as u16);
        out.extend_from_slice(payload);
        self.encode_cipher.nonce_u32(self.encode_nonce);
        self.encode_nonce = self.encode_nonce.saturating_add(1);
        self.encode_cipher.encrypt(out.as_mut_slice());
        let mut mac = [0u8; MAC_SIZE];
        self.encode_cipher.finish(&mut mac);
        out.extend_from_slice(&mac);
        out
    }

    fn decode_header(&mut self, header: &mut [u8]) -> (u8, usize) {
        self.decode_cipher.nonce_u32(self.decode_nonce);
        self.decode_nonce = self.decode_nonce.saturating_add(1);
        self.decode_cipher.decrypt(header);
        (header[0], read_u16_be(&header[1..]) as usize)
    }

    fn peek_header(&self, header: &[u8; 3]) -> (u8, usize) {
        let mut codec = self.clone();
        let mut copy = *header;
        codec.decode_header(&mut copy)
    }

    fn decode_payload(&mut self, data: &mut [u8], size: usize) -> Result<Vec<u8>, String> {
        if data.len() < size + MAC_SIZE {
            return Err(String::from("encrypted-short"));
        }
        self.decode_cipher.decrypt(&mut data[..size]);
        let mac = &data[size..size + MAC_SIZE];
        self.decode_cipher.check_mac(mac)?;
        Ok(data[..size].to_vec())
    }
}

#[allow(non_snake_case)]
#[derive(Clone)]
struct Shannon {
    R: [u32; 16],
    CRC: [u32; 16],
    initR: [u32; 16],
    konst: u32,
    sbuf: u32,
    mbuf: u32,
    nbuf: usize,
}

impl Shannon {
    fn new(key: &[u8]) -> Self {
        let mut cipher = Self {
            R: [0; 16],
            CRC: [0; 16],
            initR: [0; 16],
            konst: 0x6996_c53a,
            sbuf: 0,
            mbuf: 0,
            nbuf: 0,
        };
        cipher.R[0] = 1;
        cipher.R[1] = 1;
        for i in 2..16 {
            cipher.R[i] = cipher.R[i - 1].wrapping_add(cipher.R[i - 2]);
        }
        cipher.loadkey(key);
        cipher.genkonst();
        cipher.savestate();
        cipher
    }

    fn savestate(&mut self) {
        self.initR = self.R;
    }

    fn reloadstate(&mut self) {
        self.R = self.initR;
    }

    fn genkonst(&mut self) {
        self.konst = self.R[0];
    }

    fn cycle(&mut self) {
        let mut t = self.R[12] ^ self.R[13] ^ self.konst;
        t = sbox1(t) ^ self.R[0].rotate_left(1);
        for i in 1..16 {
            self.R[i - 1] = self.R[i];
        }
        self.R[15] = t;
        t = sbox2(self.R[2] ^ self.R[15]);
        self.R[0] ^= t;
        self.sbuf = t ^ self.R[8] ^ self.R[12];
    }

    fn diffuse(&mut self) {
        for _ in 0..16 {
            self.cycle();
        }
    }

    fn loadkey(&mut self, key: &[u8]) {
        for word in key.chunks(4) {
            let mut padded = [0u8; 4];
            padded[..word.len()].copy_from_slice(word);
            self.R[13] ^= u32::from_le_bytes(padded);
            self.cycle();
        }
        self.R[13] ^= key.len() as u32;
        self.cycle();
        self.CRC = self.R;
        self.diffuse();
        for i in 0..16 {
            self.R[i] ^= self.CRC[i];
        }
    }

    fn nonce(&mut self, nonce: &[u8]) {
        self.reloadstate();
        self.konst = 0x6996_c53a;
        self.loadkey(nonce);
        self.genkonst();
        self.nbuf = 0;
    }

    fn nonce_u32(&mut self, n: u32) {
        self.nonce(&n.to_be_bytes());
    }

    fn crcfunc(&mut self, i: u32) {
        let t = self.CRC[0] ^ self.CRC[2] ^ self.CRC[15] ^ i;
        for j in 1..16 {
            self.CRC[j - 1] = self.CRC[j];
        }
        self.CRC[15] = t;
    }

    fn macfunc(&mut self, i: u32) {
        self.crcfunc(i);
        self.R[13] ^= i;
    }

    fn encrypt(&mut self, buf: &mut [u8]) {
        self.process(
            buf,
            |ctx, word| {
                ctx.macfunc(*word);
                *word ^= ctx.sbuf;
            },
            |ctx, b| {
                ctx.mbuf ^= (*b as u32) << (32 - ctx.nbuf);
                *b ^= ((ctx.sbuf >> (32 - ctx.nbuf)) & 0xff) as u8;
            },
        );
    }

    fn decrypt(&mut self, buf: &mut [u8]) {
        self.process(
            buf,
            |ctx, word| {
                *word ^= ctx.sbuf;
                ctx.macfunc(*word);
            },
            |ctx, b| {
                *b ^= ((ctx.sbuf >> (32 - ctx.nbuf)) & 0xff) as u8;
                ctx.mbuf ^= (*b as u32) << (32 - ctx.nbuf);
            },
        );
    }

    fn process<F, G>(&mut self, buf: &mut [u8], full_word: F, partial: G)
    where
        F: Fn(&mut Self, &mut u32),
        G: Fn(&mut Self, &mut u8),
    {
        let mut offset = 0usize;
        if self.nbuf != 0 {
            while self.nbuf > 0 && offset < buf.len() {
                partial(self, &mut buf[offset]);
                self.nbuf -= 8;
                offset += 1;
            }
            if self.nbuf == 0 {
                self.macfunc(self.mbuf);
            } else {
                return;
            }
        }

        while offset + 4 <= buf.len() {
            self.cycle();
            let mut word = u32::from_le_bytes([
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            ]);
            full_word(self, &mut word);
            buf[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
            offset += 4;
        }

        if offset < buf.len() {
            self.cycle();
            self.mbuf = 0;
            self.nbuf = 32;
            while offset < buf.len() {
                partial(self, &mut buf[offset]);
                self.nbuf -= 8;
                offset += 1;
            }
        }
    }

    fn finish(&mut self, buf: &mut [u8]) {
        if self.nbuf != 0 {
            self.macfunc(self.mbuf);
        }
        self.cycle();
        self.R[13] ^= 0x6996_c53a ^ ((self.nbuf as u32) << 3);
        self.nbuf = 0;
        for i in 0..16 {
            self.R[i] ^= self.CRC[i];
        }
        self.diffuse();
        for word in buf.chunks_mut(4) {
            self.cycle();
            let bytes = self.sbuf.to_le_bytes();
            word.copy_from_slice(&bytes[..word.len()]);
        }
    }

    fn check_mac(&mut self, expected: &[u8]) -> Result<(), String> {
        let mut actual = vec![0u8; expected.len()];
        self.finish(actual.as_mut_slice());
        if actual == expected {
            Ok(())
        } else {
            Err(String::from("mac-mismatch"))
        }
    }
}

fn sbox1(mut w: u32) -> u32 {
    w ^= w.rotate_left(5) | w.rotate_left(7);
    w ^= w.rotate_left(19) | w.rotate_left(22);
    w
}

fn sbox2(mut w: u32) -> u32 {
    w ^= w.rotate_left(7) | w.rotate_left(22);
    w ^= w.rotate_left(5) | w.rotate_left(19);
    w
}

fn push_u16_be(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_u32_be(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn write_u64_be(out: &mut [u8; 8], value: u64) {
    out.copy_from_slice(&value.to_be_bytes());
}

fn read_u16_be(data: &[u8]) -> u16 {
    u16::from_be_bytes([data[0], data[1]])
}

fn read_u16_be_at(data: &[u8], index: &mut usize) -> Option<u16> {
    let end = index.checked_add(2)?;
    if end > data.len() {
        return None;
    }
    let value = read_u16_be(&data[*index..end]);
    *index = end;
    Some(value)
}

fn read_u32_be(data: &[u8]) -> u32 {
    u32::from_be_bytes([data[0], data[1], data[2], data[3]])
}

fn read_u64_be(data: &[u8]) -> u64 {
    u64::from_be_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ])
}
