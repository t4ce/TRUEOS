extern crate alloc;

use alloc::{collections::BTreeMap, format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use aes::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use base64::Engine as _;
use embassy_time::{Duration as EmbassyDuration, Instant};
use serde::Deserialize;
use spin::Mutex;
use v::vnet as api;

use crate::r::net::{NetProfile, VNet};
use crate::r::spotify_zeroconf::SpotifyCredential;

pub const TASK_NAME: &str = "spotify-service";

const READY_MASK: u32 = crate::r::readiness::NET_SOCKET_READY
    | crate::r::readiness::INTEL_HDA_READY
    | crate::r::readiness::BACKGROUND_AP_WORKER_READY;
const HEARTBEAT_MS: u64 = 5_000;
const SPOTIFY_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const KEYMASTER_PRIMARY_SCOPES: &str = "streaming,app-remote-control";
const KEYMASTER_FALLBACK_SCOPES: &str = "streaming";
const DEALER_REQUEST_MAX_BYTES: usize = 256 * 1024;
const CDN_WARMUP_BYTES: usize = 512 * 1024;
const CDN_WARMUP_AUDIO_PACKETS: usize = 24;
const CDN_STREAM_CHUNK_BYTES: usize = 64 * 1024;
const SPOTIFY_OGG_HEADER_END: usize = 0xa7;
const AUDIO_AESIV: [u8; 16] = [
    0x72, 0xe0, 0x67, 0xfb, 0xdd, 0xcb, 0xcf, 0x77, 0xeb, 0xe8, 0xbc, 0x64, 0x3f, 0x63, 0x0d, 0x93,
];

type SpotifyAudioCtr = ctr::Ctr128BE<aes::Aes128>;

static SERVICE_EPOCH: AtomicU64 = AtomicU64::new(0);
static SERVICE_CPU_SLOT: AtomicU32 = AtomicU32::new(u32::MAX);
static PENDING_CREDENTIAL: Mutex<Option<SpotifyCredential>> = Mutex::new(None);
#[used]
static SPOTIFY_SERVICE_KERNEL_PROBE: fn() = spotify_service_kernel_probe;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpotifyServiceStatus {
    pub epoch: u64,
    pub cpu_slot: Option<u32>,
    pub ready_mask: u32,
}

pub fn status() -> SpotifyServiceStatus {
    let slot = SERVICE_CPU_SLOT.load(Ordering::Acquire);
    SpotifyServiceStatus {
        epoch: SERVICE_EPOCH.load(Ordering::Acquire),
        cpu_slot: (slot != u32::MAX).then_some(slot),
        ready_mask: READY_MASK,
    }
}

pub fn submit_zeroconf_credential(credential: SpotifyCredential) {
    let username_len = credential.username.len();
    let auth_type = credential.auth_type;
    let auth_data_len = credential.auth_data.len();
    *PENDING_CREDENTIAL.lock() = Some(credential);
    crate::log!(
        "spotify-service: zeroconf credential queued user_len={} auth_type={} auth_data_len={}\n",
        username_len,
        auth_type,
        auth_data_len
    );
}

fn take_pending_credential() -> Option<SpotifyCredential> {
    PENDING_CREDENTIAL.lock().take()
}

fn spotify_service_runtime_probe() {
    crate::log!(
        "spotify-service: kernel service probe net_socket=1 tls_provider=rustcrypto librespot_client=not-linked vendor_only=1\n"
    );
}

#[cold]
fn spotify_service_kernel_probe() {
    core::hint::black_box(TASK_NAME.as_ptr());
}

#[embassy_executor::task]
pub async fn spotify_service_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(TASK_NAME);
    let epoch = SERVICE_EPOCH
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    let slot = crate::cpu::CpuProfile::current()
        .map(|profile| profile.slot())
        .unwrap_or(u32::MAX);
    SERVICE_CPU_SLOT.store(slot, Ordering::Release);

    crate::log!(
        "spotify-service: task start epoch={} slot={} waiting mask=0x{:08X}\n",
        epoch,
        slot,
        READY_MASK
    );

    crate::r::readiness::wait_for(READY_MASK).await;

    crate::log!(
        "spotify-service: online epoch={} slot={} net_socket=1 hda=1 owner=service\n",
        epoch,
        slot
    );
    crate::net::tls::ensure_rustls_provider_installed();
    spotify_service_runtime_probe();

    let mut discovery = crate::r::spotify_discovery::SpotifyDiscoveryService::new();
    let added = discovery.add_endpoints();
    crate::log!(
        "spotify-service: discovery transport init endpoints={} added={}\n",
        discovery.endpoint_count(),
        added
    );

    let heartbeat = EmbassyDuration::from_millis(HEARTBEAT_MS);
    let mut next_heartbeat = Instant::now() + heartbeat;
    let mut spotify_session: Option<SpotifyRuntimeSession> = None;
    loop {
        discovery.tick().await;
        if spotify_session.is_none() {
            if let Some(credential) = take_pending_credential() {
                spotify_session = run_session_probe(credential).await;
            }
        }
        if let Some(session) = spotify_session.as_mut() {
            match session.ap.tick().await {
                Ok(crate::r::spotify_ap::ApSessionEvent::Idle) => {}
                Ok(_) => {}
                Err(err) => {
                    crate::log!(
                        "spotify-session: ap session ended handle={} err={}\n",
                        session.ap.handle_id(),
                        err
                    );
                    session.ap.close();
                    spotify_session = None;
                    continue;
                }
            }
            session.tick_dealer().await;
        }
        if Instant::now() >= next_heartbeat {
            crate::log!(
                "spotify-service: idle epoch={} slot={} ready=0x{:08X} discovery_endpoints={} ap_session={} dealer={} connect={} playback={}\n",
                epoch,
                slot,
                crate::r::readiness::mask(),
                discovery.endpoint_count(),
                spotify_session
                    .as_ref()
                    .map(|s| s.ap.handle_id())
                    .unwrap_or(0),
                spotify_session
                    .as_ref()
                    .map(|s| s.dealer_label())
                    .unwrap_or("none"),
                spotify_session
                    .as_ref()
                    .map(|s| s.connect_state_label())
                    .unwrap_or("none"),
                spotify_session
                    .as_ref()
                    .map(|s| s.playback_target_label())
                    .unwrap_or("none")
            );
            next_heartbeat = Instant::now() + heartbeat;
        }
    }
}

#[derive(Deserialize, Default)]
struct ApResolveData {
    #[serde(default)]
    accesspoint: Vec<String>,
    #[serde(default)]
    dealer: Vec<String>,
    #[serde(default)]
    spclient: Vec<String>,
}

#[derive(Clone)]
struct SpotifyEndpoint {
    host: String,
    port: u16,
}

struct SpotifyRuntimeSession {
    ap: crate::r::spotify_ap::ApSession,
    dealer_endpoint: Option<SpotifyEndpoint>,
    spclient_endpoint: Option<SpotifyEndpoint>,
    dealer: DealerState,
    connect_state: ConnectStateProbeState,
    playback_target: Option<SpotifyPlaybackTarget>,
    playback_metadata: PlaybackMetadataProbeState,
    playback_file: Option<SpotifyAudioFileCandidate>,
    playback_storage: PlaybackStorageProbeState,
    playback_storage_result: Option<SpotifyStorageResolveProbe>,
    playback_audio_key: PlaybackAudioKeyProbeState,
    playback_audio_key_bytes: Option<[u8; 16]>,
    playback_cdn: PlaybackCdnProbeState,
    playback_pcm_sink: PlaybackPcmSinkProbeState,
    playback_decoder: PlaybackDecoderProbeState,
    playback_stream: PlaybackStreamProbeState,
    playback_cdn_url: Option<String>,
    playback_cdn_offset: usize,
    playback_cdn_warmup: Option<Vec<u8>>,
    playback_vorbis_decoder: Option<crate::r::spotify_vorbis::VorbisPacketDecoder>,
    playback_ogg_stream: SpotifyOggStreamParser,
    playback_pending_pcm: Vec<i16>,
    playback_vorbis_ident: Option<VorbisIdentProbe>,
    playback_vorbis_packets: Option<VorbisPacketCapture>,
    playback_vorbis_input: Option<crate::r::spotify_vorbis::PreparedVorbisDecoderInput>,
    playback_ogg_audio_packets: usize,
    playback_ogg_serial: Option<u32>,
    pcm_sink: Option<crate::hda::PcmStreamHandle>,
    connection_id: Option<String>,
    session_id: String,
    keymaster_scope: KeymasterScope,
}

enum DealerState {
    WaitingToken,
    Connected(crate::r::net::srv::wss::WssConnection),
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeymasterScope {
    Primary,
    StreamingOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConnectStateProbeState {
    WaitingDealer,
    WaitingConnectionId,
    Announced,
    Active,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybackMetadataProbeState {
    WaitingTarget,
    WaitingRequest,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybackStorageProbeState {
    WaitingFile,
    WaitingRequest,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybackAudioKeyProbeState {
    WaitingInputs,
    WaitingRequest,
    WaitingResponse,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybackCdnProbeState {
    WaitingInputs,
    WaitingRequest,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybackPcmSinkProbeState {
    WaitingCdn,
    Opening,
    Ready,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybackDecoderProbeState {
    WaitingPcm,
    NeedsVorbisBackend,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybackStreamProbeState {
    WaitingDecoder,
    WaitingRequest,
    Done,
    Failed,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum DealerIncoming {
    Message {
        #[serde(default)]
        headers: BTreeMap<String, String>,
        uri: String,
        method: Option<String>,
    },
    Request {
        #[serde(default)]
        headers: BTreeMap<String, String>,
        message_ident: String,
        key: String,
        #[serde(default)]
        payload: DealerRequestPayload,
    },
}

#[derive(Deserialize, Default)]
struct DealerRequestPayload {
    #[serde(default)]
    compressed: String,
}

enum DealerAction {
    ConnectionId(String),
    Reply {
        key: String,
        ident_len: usize,
        endpoint: String,
        message_id: u32,
        sent_by_device_id: String,
        playback_target: Option<SpotifyPlaybackTarget>,
        success: bool,
    },
}

impl KeymasterScope {
    fn scopes(self) -> &'static str {
        match self {
            Self::Primary => KEYMASTER_PRIMARY_SCOPES,
            Self::StreamingOnly => KEYMASTER_FALLBACK_SCOPES,
        }
    }

    fn fallback(self) -> Option<Self> {
        match self {
            Self::Primary => Some(Self::StreamingOnly),
            Self::StreamingOnly => None,
        }
    }
}

impl SpotifyRuntimeSession {
    fn dealer_label(&self) -> &'static str {
        match self.dealer {
            DealerState::WaitingToken => "waiting-token",
            DealerState::Connected(_) => "connected",
            DealerState::Failed => "failed",
        }
    }

    fn connect_state_label(&self) -> &'static str {
        match self.connect_state {
            ConnectStateProbeState::WaitingDealer => "waiting-dealer",
            ConnectStateProbeState::WaitingConnectionId => "waiting-connection-id",
            ConnectStateProbeState::Announced => "announced",
            ConnectStateProbeState::Active => "active",
            ConnectStateProbeState::Failed => "failed",
        }
    }

    fn playback_target_label(&self) -> &'static str {
        if self.playback_stream == PlaybackStreamProbeState::WaitingRequest {
            "streaming"
        } else if self.playback_stream == PlaybackStreamProbeState::Done {
            "stream-done"
        } else if self.playback_stream == PlaybackStreamProbeState::Failed {
            "stream-failed"
        } else if self.playback_decoder == PlaybackDecoderProbeState::NeedsVorbisBackend {
            if self.playback_vorbis_input.is_some() {
                "decoder-input-ready"
            } else {
                "decoder-missing"
            }
        } else if self.playback_decoder == PlaybackDecoderProbeState::Done {
            "decoder-ready"
        } else if self.playback_decoder == PlaybackDecoderProbeState::Failed {
            "decoder-failed"
        } else if self.playback_pcm_sink == PlaybackPcmSinkProbeState::Ready {
            "pcm-ready"
        } else if self.playback_pcm_sink == PlaybackPcmSinkProbeState::Failed {
            "pcm-failed"
        } else if self.playback_cdn == PlaybackCdnProbeState::Done {
            "cdn"
        } else if self.playback_audio_key == PlaybackAudioKeyProbeState::Done {
            "audio-key"
        } else if self.playback_storage == PlaybackStorageProbeState::Done {
            "storage"
        } else if self.playback_file.is_some() {
            "metadata-file"
        } else if self.playback_target.is_some() {
            "transfer-target"
        } else {
            "none"
        }
    }

    async fn tick_dealer(&mut self) {
        let mut active_publish = None;
        match &mut self.dealer {
            DealerState::WaitingToken => {
                let Some(endpoint) = self.dealer_endpoint.clone() else {
                    self.dealer = DealerState::Failed;
                    crate::log!("spotify-dealer: no dealer endpoint from apresolve\n");
                    return;
                };
                let Some(token) = self.ap.keymaster_token() else {
                    self.maybe_retry_keymaster();
                    return;
                };
                let url = format!(
                    "wss://{}:{}/?access_token={}",
                    endpoint.host.as_str(),
                    endpoint.port,
                    token.access_token.as_str()
                );
                crate::log!(
                    "spotify-dealer: connect begin host={} port={} token_type={} token_len={}\n",
                    endpoint.host.as_str(),
                    endpoint.port,
                    token.token_type.as_str(),
                    token.access_token.len()
                );
                match crate::r::net::srv::wss::WssConnection::connect_with_profile(
                    url.as_str(),
                    NetProfile::default(),
                )
                .await
                {
                    Ok(conn) => {
                        crate::log!(
                            "spotify-dealer: websocket connected host={} port={} next=wait-hello\n",
                            endpoint.host.as_str(),
                            endpoint.port
                        );
                        self.connect_state = ConnectStateProbeState::WaitingConnectionId;
                        self.dealer = DealerState::Connected(conn);
                    }
                    Err(err) => {
                        crate::log!(
                            "spotify-dealer: websocket failed host={} port={} err={:?}\n",
                            endpoint.host.as_str(),
                            endpoint.port,
                            err
                        );
                        self.dealer = DealerState::Failed;
                    }
                }
            }
            DealerState::Connected(conn) => {
                if let Some(message) = conn.recv() {
                    match Self::inspect_dealer_message(message.as_str()) {
                        Some(DealerAction::ConnectionId(connection_id)) => {
                            if self.connection_id.is_none() {
                                crate::log!(
                                    "spotify-dealer: connection id received len={} next=put-connect-state\n",
                                    connection_id.len()
                                );
                                self.connection_id = Some(connection_id);
                            }
                        }
                        Some(DealerAction::Reply {
                            key,
                            ident_len,
                            endpoint,
                            message_id,
                            sent_by_device_id,
                            playback_target,
                            success,
                        }) => {
                            let reply = dealer_reply_json(key.as_str(), success);
                            match conn.send(reply.as_str()) {
                                Ok(()) => {
                                    crate::log!(
                                        "spotify-dealer: websocket request replied ident_len={} key_len={} endpoint={} success={}\n",
                                        ident_len,
                                        key.len(),
                                        endpoint.as_str(),
                                        success as u8
                                    );
                                    if success {
                                        if let Some(target) = playback_target {
                                            crate::log!(
                                                "spotify-playback: target endpoint={} source={} track_uri={} gid={} uid_len={} context_uri={} position_ms={} position_as_of={} playback_ts={} paused={}\n",
                                                endpoint.as_str(),
                                                target.source,
                                                target.track_uri.as_deref().unwrap_or(""),
                                                target.track_gid_hex.as_deref().unwrap_or(""),
                                                target
                                                    .track_uid
                                                    .as_ref()
                                                    .map_or(0, |uid| uid.len()),
                                                target.context_uri.as_deref().unwrap_or(""),
                                                target.position_ms.unwrap_or(0),
                                                target.position_as_of_timestamp_ms.unwrap_or(0),
                                                target.playback_timestamp_ms.unwrap_or(0),
                                                target.paused.map_or(-1, |paused| paused as i32)
                                            );
                                            self.playback_target = Some(target);
                                            self.playback_metadata =
                                                PlaybackMetadataProbeState::WaitingRequest;
                                            self.playback_file = None;
                                            self.playback_storage =
                                                PlaybackStorageProbeState::WaitingFile;
                                            self.playback_storage_result = None;
                                            self.playback_audio_key =
                                                PlaybackAudioKeyProbeState::WaitingInputs;
                                            self.playback_audio_key_bytes = None;
                                            self.playback_cdn =
                                                PlaybackCdnProbeState::WaitingInputs;
                                            self.playback_pcm_sink =
                                                PlaybackPcmSinkProbeState::WaitingCdn;
                                            self.playback_decoder =
                                                PlaybackDecoderProbeState::WaitingPcm;
                                            self.playback_stream =
                                                PlaybackStreamProbeState::WaitingDecoder;
                                            self.playback_cdn_url = None;
                                            self.playback_cdn_offset = 0;
                                            self.playback_cdn_warmup = None;
                                            self.playback_vorbis_decoder = None;
                                            self.playback_ogg_stream =
                                                SpotifyOggStreamParser::new();
                                            self.playback_pending_pcm.clear();
                                            self.playback_vorbis_ident = None;
                                            self.playback_vorbis_packets = None;
                                            self.playback_vorbis_input = None;
                                            self.playback_ogg_audio_packets = 0;
                                            self.playback_ogg_serial = None;
                                            self.pcm_sink = None;
                                            self.ap.clear_audio_key_state();
                                        } else if endpoint == "transfer" {
                                            crate::log!(
                                                "spotify-playback: transfer target absent message_id={} sent_by_len={} action=ack-only\n",
                                                message_id,
                                                sent_by_device_id.len()
                                            );
                                        }
                                        if endpoint == "transfer" {
                                            active_publish = Some(DealerTransferAck {
                                                message_id,
                                                sent_by_device_id,
                                            });
                                        }
                                    }
                                }
                                Err(err) => crate::log!(
                                    "spotify-dealer: websocket request reply failed ident_len={} key_len={} endpoint={} err={:?}\n",
                                    ident_len,
                                    key.len(),
                                    endpoint.as_str(),
                                    err
                                ),
                            }
                        }
                        None => {}
                    }
                }
            }
            DealerState::Failed => {}
        }
        self.tick_connect_state().await;
        if let Some(ack) = active_publish {
            self.publish_active_connect_state(ack).await;
        }
        self.tick_playback_metadata_probe().await;
        self.tick_playback_storage_probe().await;
        self.tick_playback_audio_key_probe().await;
        self.tick_playback_cdn_probe().await;
        self.tick_playback_pcm_sink_probe();
        self.tick_playback_decoder_probe();
        self.tick_playback_stream_probe().await;
    }

    fn maybe_retry_keymaster(&mut self) {
        let Some(failure) = self.ap.take_keymaster_failure() else {
            return;
        };
        crate::log!(
            "spotify-session: keymaster token failed seq={:?} scope={} status={:?} payload_len={} reason={}\n",
            failure.seq,
            self.keymaster_scope.scopes(),
            failure.status_code,
            failure.payload_len,
            failure.reason.as_str()
        );
        let Some(next_scope) = self.keymaster_scope.fallback() else {
            self.dealer = DealerState::Failed;
            self.connect_state = ConnectStateProbeState::Failed;
            crate::log!("spotify-session: keymaster token exhausted fallbacks\n");
            return;
        };
        self.keymaster_scope = next_scope;
        match self
            .ap
            .request_keymaster_token(next_scope.scopes(), crate::r::spotify_discovery::DEVICE_ID)
        {
            Ok(seq) => crate::log!(
                "spotify-session: mercury keymaster token retry seq={} scopes={}\n",
                seq,
                next_scope.scopes()
            ),
            Err(err) => {
                self.dealer = DealerState::Failed;
                self.connect_state = ConnectStateProbeState::Failed;
                crate::log!("spotify-session: mercury keymaster token retry failed err={}\n", err);
            }
        }
    }

    fn inspect_dealer_message(message: &str) -> Option<DealerAction> {
        match serde_json::from_str::<DealerIncoming>(message) {
            Ok(DealerIncoming::Message {
                headers,
                uri,
                method,
            }) => {
                crate::log!(
                    "spotify-dealer: websocket message uri_len={} method={}\n",
                    uri.len(),
                    method.as_deref().unwrap_or("")
                );
                headers
                    .get("Spotify-Connection-Id")
                    .cloned()
                    .map(DealerAction::ConnectionId)
            }
            Ok(DealerIncoming::Request {
                headers,
                message_ident,
                key,
                payload,
            }) => {
                let probe = inspect_dealer_request(&headers, &payload);
                crate::log!(
                    "spotify-dealer: websocket request ident_len={} key_len={} endpoint={} message_id={} sent_by_len={} payload_bytes={} action=reply-{}\n",
                    message_ident.len(),
                    key.len(),
                    probe.endpoint.as_str(),
                    probe.message_id,
                    probe.sent_by_len,
                    probe.payload_len,
                    if probe.success { "success" } else { "failure" }
                );
                Some(DealerAction::Reply {
                    key,
                    ident_len: message_ident.len(),
                    endpoint: probe.endpoint,
                    message_id: probe.message_id,
                    sent_by_device_id: probe.sent_by_device_id,
                    playback_target: probe.playback_target,
                    success: probe.success,
                })
            }
            Err(err) => {
                crate::log!(
                    "spotify-dealer: websocket text message bytes={} parse_err={}\n",
                    message.len(),
                    err
                );
                None
            }
        }
    }

    async fn publish_active_connect_state(&mut self, ack: DealerTransferAck) {
        let Some(connection_id) = self.connection_id.clone() else {
            crate::log!(
                "spotify-connect: put active skipped reason=no-connection-id message_id={}\n",
                ack.message_id
            );
            return;
        };
        let Some(endpoint) = self.spclient_endpoint.clone() else {
            crate::log!(
                "spotify-connect: put active skipped reason=no-spclient message_id={}\n",
                ack.message_id
            );
            self.connect_state = ConnectStateProbeState::Failed;
            return;
        };
        let Some(token) = self.ap.keymaster_token().cloned() else {
            crate::log!(
                "spotify-connect: put active skipped reason=no-token message_id={}\n",
                ack.message_id
            );
            return;
        };

        let body = build_put_active_connect_state_request(
            crate::r::spotify_discovery::DEVICE_ID,
            self.session_id.as_str(),
            self.playback_target.as_ref(),
            &ack,
        );
        let url = format!(
            "https://{}:{}/connect-state/v1/devices/{}",
            endpoint.host.as_str(),
            endpoint.port,
            crate::r::spotify_discovery::DEVICE_ID
        );
        crate::log!(
            "spotify-connect: put active begin host={} port={} body_bytes={} connection_id_len={} message_id={} sent_by_len={} token_len={}\n",
            endpoint.host.as_str(),
            endpoint.port,
            body.len(),
            connection_id.len(),
            ack.message_id,
            ack.sent_by_device_id.len(),
            token.access_token.len()
        );
        match crate::r::net::https::put_protobuf_shared(
            url.as_str(),
            body.as_slice(),
            Some(token.access_token.as_str()),
            Some(connection_id.as_str()),
            15_000,
            256 * 1024,
        )
        .await
        {
            Ok(response) => {
                crate::log!(
                    "spotify-connect: put active ok response_bytes={} state=active\n",
                    response.len()
                );
                self.connect_state = ConnectStateProbeState::Active;
            }
            Err(err) => {
                crate::log!("spotify-connect: put active failed err={}\n", err);
                self.connect_state = ConnectStateProbeState::Failed;
            }
        }
    }

    async fn tick_connect_state(&mut self) {
        if self.connect_state != ConnectStateProbeState::WaitingConnectionId {
            return;
        }
        let Some(connection_id) = self.connection_id.clone() else {
            return;
        };
        let Some(endpoint) = self.spclient_endpoint.clone() else {
            crate::log!("spotify-connect: no spclient endpoint from apresolve\n");
            self.connect_state = ConnectStateProbeState::Failed;
            return;
        };
        let Some(token) = self.ap.keymaster_token().cloned() else {
            return;
        };

        let body = build_put_connect_state_request(
            crate::r::spotify_discovery::DEVICE_ID,
            self.session_id.as_str(),
        );
        let url = format!(
            "https://{}:{}/connect-state/v1/devices/{}",
            endpoint.host.as_str(),
            endpoint.port,
            crate::r::spotify_discovery::DEVICE_ID
        );
        crate::log!(
            "spotify-connect: put state begin host={} port={} body_bytes={} connection_id_len={} token_len={} client_token=absent\n",
            endpoint.host.as_str(),
            endpoint.port,
            body.len(),
            connection_id.len(),
            token.access_token.len()
        );
        match crate::r::net::https::put_protobuf_shared(
            url.as_str(),
            body.as_slice(),
            Some(token.access_token.as_str()),
            Some(connection_id.as_str()),
            15_000,
            256 * 1024,
        )
        .await
        {
            Ok(response) => {
                crate::log!(
                    "spotify-connect: put state ok response_bytes={} next=dealer-control\n",
                    response.len()
                );
                self.connect_state = ConnectStateProbeState::Announced;
            }
            Err(err) => {
                crate::log!("spotify-connect: put state failed err={}\n", err);
                self.connect_state = ConnectStateProbeState::Failed;
            }
        }
    }

    async fn tick_playback_metadata_probe(&mut self) {
        if self.playback_metadata != PlaybackMetadataProbeState::WaitingRequest {
            return;
        }
        let Some(target) = self.playback_target.clone() else {
            self.playback_metadata = PlaybackMetadataProbeState::WaitingTarget;
            return;
        };
        let Some(track_uri) = target.track_uri.as_deref() else {
            crate::log!("spotify-playback: metadata skipped reason=no-track-uri\n");
            self.playback_metadata = PlaybackMetadataProbeState::Failed;
            return;
        };
        let Some(endpoint) = self.spclient_endpoint.clone() else {
            return;
        };
        let Some(token) = self.ap.keymaster_token().cloned() else {
            return;
        };

        let body = build_extended_metadata_track_request(track_uri);
        let url = format!(
            "https://{}:{}/extended-metadata/v0/extended-metadata",
            endpoint.host.as_str(),
            endpoint.port
        );
        crate::log!(
            "spotify-playback: metadata begin host={} port={} track_uri={} body_bytes={} token_len={}\n",
            endpoint.host.as_str(),
            endpoint.port,
            track_uri,
            body.len(),
            token.access_token.len()
        );
        match crate::r::net::https::post_protobuf_shared(
            url.as_str(),
            body.as_slice(),
            Some(token.access_token.as_str()),
            15_000,
            512 * 1024,
        )
        .await
        {
            Ok(response) => match parse_extended_metadata_track_response(response.as_slice()) {
                Some(track) => {
                    let selected = select_audio_file(track.files.as_slice());
                    crate::log!(
                        "spotify-playback: metadata ok response_bytes={} track_bytes={} name_len={} duration_ms={} files={} selected_format={} selected_file={}\n",
                        response.len(),
                        track.track_bytes,
                        track.name.as_ref().map_or(0, |name| name.len()),
                        track.duration_ms.unwrap_or(0),
                        track.files.len(),
                        selected
                            .as_ref()
                            .map_or("none", |file| audio_format_label(file.format)),
                        selected
                            .as_ref()
                            .and_then(|file| file.file_id_hex.as_deref())
                            .unwrap_or("")
                    );
                    if let Some(selected) = selected {
                        self.playback_file = Some(selected);
                        self.playback_storage = PlaybackStorageProbeState::WaitingRequest;
                        self.playback_audio_key = PlaybackAudioKeyProbeState::WaitingRequest;
                    }
                    self.playback_metadata = PlaybackMetadataProbeState::Done;
                }
                None => {
                    crate::log!(
                        "spotify-playback: metadata parse failed response_bytes={}\n",
                        response.len()
                    );
                    self.playback_metadata = PlaybackMetadataProbeState::Failed;
                }
            },
            Err(err) => {
                crate::log!("spotify-playback: metadata failed err={}\n", err);
                self.playback_metadata = PlaybackMetadataProbeState::Failed;
            }
        }
    }

    async fn tick_playback_storage_probe(&mut self) {
        if self.playback_storage != PlaybackStorageProbeState::WaitingRequest {
            return;
        }
        let Some(file) = self.playback_file.clone() else {
            self.playback_storage = PlaybackStorageProbeState::WaitingFile;
            return;
        };
        let Some(file_id_hex) = file.file_id_hex.as_deref() else {
            crate::log!("spotify-playback: storage skipped reason=no-file-id\n");
            self.playback_storage = PlaybackStorageProbeState::Failed;
            return;
        };
        let Some(endpoint) = self.spclient_endpoint.clone() else {
            return;
        };
        let Some(token) = self.ap.keymaster_token().cloned() else {
            return;
        };

        let url = format!(
            "https://{}:{}/storage-resolve/files/audio/interactive/{}",
            endpoint.host.as_str(),
            endpoint.port,
            file_id_hex
        );
        crate::log!(
            "spotify-playback: storage begin host={} port={} file={} format={} token_len={}\n",
            endpoint.host.as_str(),
            endpoint.port,
            file_id_hex,
            audio_format_label(file.format),
            token.access_token.len()
        );
        match crate::r::net::https::get_bytes_bearer_shared(
            url.as_str(),
            Some(token.access_token.as_str()),
            15_000,
            128 * 1024,
        )
        .await
        {
            Ok(response) => match parse_storage_resolve_response(response.as_slice()) {
                Some(storage) => {
                    crate::log!(
                        "spotify-playback: storage ok response_bytes={} result={} urls={} fileid={}\n",
                        response.len(),
                        storage.result,
                        storage.urls.len(),
                        storage.file_id_hex.as_deref().unwrap_or("")
                    );
                    if storage.result == 0 && !storage.urls.is_empty() {
                        self.playback_storage_result = Some(storage);
                        self.playback_storage = PlaybackStorageProbeState::Done;
                    } else {
                        self.playback_storage_result = Some(storage);
                        self.playback_storage = PlaybackStorageProbeState::Failed;
                    }
                }
                None => {
                    crate::log!(
                        "spotify-playback: storage parse failed response_bytes={}\n",
                        response.len()
                    );
                    self.playback_storage = PlaybackStorageProbeState::Failed;
                }
            },
            Err(err) => {
                crate::log!("spotify-playback: storage failed err={}\n", err);
                self.playback_storage = PlaybackStorageProbeState::Failed;
            }
        }
    }

    async fn tick_playback_audio_key_probe(&mut self) {
        match self.playback_audio_key {
            PlaybackAudioKeyProbeState::WaitingInputs => {}
            PlaybackAudioKeyProbeState::WaitingRequest => {
                let Some(target) = self.playback_target.as_ref() else {
                    self.playback_audio_key = PlaybackAudioKeyProbeState::WaitingInputs;
                    return;
                };
                let Some(track_gid) = target.track_gid_raw.as_deref() else {
                    crate::log!("spotify-playback: audio key skipped reason=no-track-gid\n");
                    self.playback_audio_key = PlaybackAudioKeyProbeState::Failed;
                    return;
                };
                let Some(file) = self.playback_file.as_ref() else {
                    self.playback_audio_key = PlaybackAudioKeyProbeState::WaitingInputs;
                    return;
                };
                let Some(file_id) = file.file_id_raw.as_deref() else {
                    crate::log!("spotify-playback: audio key skipped reason=no-file-id\n");
                    self.playback_audio_key = PlaybackAudioKeyProbeState::Failed;
                    return;
                };
                match self.ap.request_audio_key(track_gid, file_id) {
                    Ok(seq) => {
                        crate::log!(
                            "spotify-playback: audio key requested seq={} track_gid={} file={}\n",
                            seq,
                            target.track_gid_hex.as_deref().unwrap_or(""),
                            file.file_id_hex.as_deref().unwrap_or("")
                        );
                        self.playback_audio_key = PlaybackAudioKeyProbeState::WaitingResponse;
                    }
                    Err(err) => {
                        crate::log!("spotify-playback: audio key request failed err={}\n", err);
                        self.playback_audio_key = PlaybackAudioKeyProbeState::Failed;
                    }
                }
            }
            PlaybackAudioKeyProbeState::WaitingResponse => {
                let Some(result) = self.ap.take_audio_key_result() else {
                    return;
                };
                if result.key.is_some() {
                    self.playback_audio_key_bytes = result.key;
                    crate::log!(
                        "spotify-playback: audio key ok seq={} next=cdn-fetch\n",
                        result.seq
                    );
                    self.playback_audio_key = PlaybackAudioKeyProbeState::Done;
                } else {
                    let err = result.error_code.unwrap_or((0, 0));
                    crate::log!(
                        "spotify-playback: audio key failed seq={} err={:02x}:{:02x}\n",
                        result.seq,
                        err.0,
                        err.1
                    );
                    self.playback_audio_key = PlaybackAudioKeyProbeState::Failed;
                }
            }
            PlaybackAudioKeyProbeState::Done | PlaybackAudioKeyProbeState::Failed => {}
        }
    }

    async fn tick_playback_cdn_probe(&mut self) {
        if self.playback_cdn == PlaybackCdnProbeState::WaitingInputs
            && self.playback_storage == PlaybackStorageProbeState::Done
            && self.playback_audio_key == PlaybackAudioKeyProbeState::Done
            && self.playback_audio_key_bytes.is_some()
        {
            self.playback_cdn = PlaybackCdnProbeState::WaitingRequest;
        }
        if self.playback_cdn != PlaybackCdnProbeState::WaitingRequest {
            return;
        }

        let Some(storage) = self.playback_storage_result.as_ref() else {
            self.playback_cdn = PlaybackCdnProbeState::WaitingInputs;
            return;
        };
        let Some(url) = storage.urls.first().cloned() else {
            crate::log!("spotify-playback: cdn skipped reason=no-url\n");
            self.playback_cdn = PlaybackCdnProbeState::Failed;
            return;
        };
        self.playback_cdn_url = Some(url.clone());
        self.playback_cdn_offset = 0;
        let Some(key) = self.playback_audio_key_bytes else {
            self.playback_cdn = PlaybackCdnProbeState::WaitingInputs;
            return;
        };

        crate::log!(
            "spotify-playback: cdn probe begin url_len={} offset=0 bytes={}\n",
            url.len(),
            CDN_WARMUP_BYTES
        );
        match crate::r::net::https::get_range_bytes_shared(
            url.as_str(),
            0,
            CDN_WARMUP_BYTES,
            20_000,
            CDN_WARMUP_BYTES + 4096,
        )
        .await
        {
            Ok(encrypted) => {
                let Some(decrypted) = decrypt_spotify_audio_range(&key, 0, encrypted.as_slice())
                else {
                    crate::log!(
                        "spotify-playback: cdn decrypt failed encrypted_bytes={}\n",
                        encrypted.len()
                    );
                    self.playback_cdn = PlaybackCdnProbeState::Failed;
                    return;
                };
                let ogg_offset = find_ogg_page(decrypted.as_slice());
                let ogg_probe = inspect_ogg_vorbis(decrypted.as_slice());
                let ident = ogg_probe.ident;
                let packet_capture = ogg_probe.capture.clone();
                self.playback_vorbis_ident = ident;
                self.playback_vorbis_packets = packet_capture.clone();
                self.playback_vorbis_input =
                    prepare_vorbis_decoder_input(ident, packet_capture.as_ref());
                self.playback_ogg_audio_packets = ogg_probe.audio_packets;
                self.playback_ogg_serial = ogg_probe.serial;
                self.playback_cdn_warmup = Some(decrypted.clone());
                crate::log!(
                    "spotify-playback: cdn warmup ok encrypted_bytes={} decrypted_bytes={} ogg_offset={} decoder_offset={} decoder_ogg={} pages={} packets={} audio_packets={} captured_audio_packets={} captured_audio_bytes={} ident_packet={} comment_packet={} setup_packet={} decoder_input={} vorbis_ident={} vorbis_comment={} vorbis_setup={} ident_valid={} channels={} sample_rate={} bitrate_nominal={} bitrate_min={} bitrate_max={} block0={} block1={} framing={} resampler={} first_vorbis={} first_audio={} serial={} first={}\n",
                    encrypted.len(),
                    decrypted.len(),
                    ogg_offset
                        .map(|offset| format!("{}", offset))
                        .unwrap_or_else(|| String::from("none")),
                    SPOTIFY_OGG_HEADER_END,
                    ogg_probe.decoder_offset_page as u8,
                    ogg_probe.pages,
                    ogg_probe.packets,
                    ogg_probe.audio_packets,
                    packet_capture
                        .as_ref()
                        .map_or(0, |capture| capture.audio_packets.len()),
                    packet_capture
                        .as_ref()
                        .map_or(0, |capture| capture.audio_bytes),
                    packet_capture
                        .as_ref()
                        .map_or(0, |capture| capture.ident.len()),
                    packet_capture
                        .as_ref()
                        .map_or(0, |capture| capture.comment.len()),
                    packet_capture
                        .as_ref()
                        .map_or(0, |capture| capture.setup.len()),
                    self.playback_vorbis_input.is_some() as u8,
                    ogg_probe.vorbis_ident as u8,
                    ogg_probe.vorbis_comment as u8,
                    ogg_probe.vorbis_setup as u8,
                    ident.is_some_and(|ident| ident.valid) as u8,
                    ident.map_or(0, |ident| ident.channels),
                    ident.map_or(0, |ident| ident.sample_rate),
                    ident.map_or(0, |ident| ident.bitrate_nominal),
                    ident.map_or(0, |ident| ident.bitrate_minimum),
                    ident.map_or(0, |ident| ident.bitrate_maximum),
                    ident.map_or(0, |ident| 1usize << ident.blocksize_0_exp),
                    ident.map_or(0, |ident| 1usize << ident.blocksize_1_exp),
                    ident.map_or(0, |ident| ident.framing as u8),
                    vorbis_resampler_label(ident),
                    ogg_probe
                        .first_vorbis_offset
                        .map(|offset| format!("{}", offset))
                        .unwrap_or_else(|| String::from("none")),
                    ogg_probe
                        .first_audio_offset
                        .map(|offset| format!("{}", offset))
                        .unwrap_or_else(|| String::from("none")),
                    ogg_probe
                        .serial
                        .map(|serial| format!("{:08x}", serial))
                        .unwrap_or_else(|| String::from("none")),
                    hex_preview(decrypted.as_slice(), 16).as_str()
                );
                self.playback_cdn = if ogg_probe.decoder_offset_page
                    && ogg_probe.vorbis_ident
                    && ogg_probe.vorbis_setup
                    && ident.is_some_and(|ident| ident.valid)
                {
                    PlaybackCdnProbeState::Done
                } else {
                    PlaybackCdnProbeState::Failed
                };
            }
            Err(err) => {
                crate::log!("spotify-playback: cdn probe failed err={}\n", err);
                self.playback_cdn = PlaybackCdnProbeState::Failed;
            }
        }
    }

    fn tick_playback_pcm_sink_probe(&mut self) {
        if self.playback_pcm_sink == PlaybackPcmSinkProbeState::WaitingCdn {
            if self.playback_cdn != PlaybackCdnProbeState::Done {
                return;
            }
            self.playback_pcm_sink = PlaybackPcmSinkProbeState::Opening;
        }
        if self.playback_pcm_sink != PlaybackPcmSinkProbeState::Opening {
            return;
        }
        if self.pcm_sink.is_some() {
            self.playback_pcm_sink = PlaybackPcmSinkProbeState::Ready;
            return;
        }

        match crate::hda::open_pcm_stream() {
            Ok(stream) => {
                let info = stream.info();
                let writable_frames = stream
                    .writable_samples(crate::hda::PCM_CHANNELS)
                    .map(|samples| samples / crate::hda::PCM_CHANNELS)
                    .unwrap_or(0);
                let queued_frames = stream
                    .queued_samples()
                    .map(|samples| samples / crate::hda::PCM_CHANNELS)
                    .unwrap_or(0);
                crate::log!(
                    "spotify-playback: pcm sink ready sample_rate={} channels={} sample_bits={} buffer_frames={} writable_frames={} queued_frames={} started={} owner=spotify-service\n",
                    info.sample_rate_hz,
                    info.channels,
                    info.sample_bits,
                    info.buffer_frames,
                    writable_frames,
                    queued_frames,
                    stream.is_started() as u8
                );
                self.pcm_sink = Some(stream);
                self.playback_pcm_sink = PlaybackPcmSinkProbeState::Ready;
            }
            Err(err) => {
                crate::log!("spotify-playback: pcm sink failed err={}\n", err);
                self.playback_pcm_sink = PlaybackPcmSinkProbeState::Failed;
            }
        }
    }

    fn tick_playback_decoder_probe(&mut self) {
        if self.playback_decoder != PlaybackDecoderProbeState::WaitingPcm {
            return;
        }
        if self.playback_pcm_sink == PlaybackPcmSinkProbeState::Failed {
            self.playback_decoder = PlaybackDecoderProbeState::Failed;
            return;
        }
        if self.playback_pcm_sink != PlaybackPcmSinkProbeState::Ready {
            return;
        }

        let ident = self.playback_vorbis_ident;
        let capture = self.playback_vorbis_packets.as_ref();
        let Some(input) = self.playback_vorbis_input.as_ref() else {
            crate::log!(
                "spotify-playback: decoder skipped reason=no-input channels={} sample_rate={} captured_audio_packets={} captured_audio_bytes={} ident_packet={} comment_packet={} setup_packet={} serial={}\n",
                ident.map_or(0, |ident| ident.channels),
                ident.map_or(0, |ident| ident.sample_rate),
                capture.map_or(0, |capture| capture.audio_packets.len()),
                capture.map_or(0, |capture| capture.audio_bytes),
                capture.map_or(0, |capture| capture.ident.len()),
                capture.map_or(0, |capture| capture.comment.len()),
                capture.map_or(0, |capture| capture.setup.len()),
                self.playback_ogg_serial
                    .map(|serial| format!("{:08x}", serial))
                    .unwrap_or_else(|| String::from("none"))
            );
            self.playback_decoder = PlaybackDecoderProbeState::Failed;
            return;
        };

        let decoder = match crate::r::spotify_vorbis::VorbisPacketDecoder::new(input) {
            Ok(decoder) => decoder,
            Err(err) => {
                crate::log!(
                    "spotify-playback: decoder init failed err={:?} input_audio_packets={} input_audio_bytes={} ident_packet={} comment_packet={} setup_packet={} resampler={}\n",
                    err,
                    input.audio_packet_count(),
                    input.audio_bytes(),
                    input.ident_len(),
                    input.comment_len(),
                    input.setup_len(),
                    vorbis_resampler_label(ident)
                );
                self.playback_decoder = PlaybackDecoderProbeState::Failed;
                return;
            }
        };

        self.playback_vorbis_decoder = Some(decoder);
        self.playback_ogg_stream = SpotifyOggStreamParser::new();
        self.playback_pending_pcm.clear();
        self.playback_cdn_offset = 0;
        self.playback_decoder = PlaybackDecoderProbeState::Done;
        self.playback_stream = PlaybackStreamProbeState::WaitingRequest;
        crate::log!(
            "spotify-playback: decoder ready codec=vorbis input_audio_packets={} input_audio_bytes={} ident_packet={} comment_packet={} setup_packet={} channels={} sample_rate={} hda_channels={} hda_sample_rate={} resampler={} serial={} next=cdn-stream\n",
            input.audio_packet_count(),
            input.audio_bytes(),
            input.ident_len(),
            input.comment_len(),
            input.setup_len(),
            ident.map_or(0, |ident| ident.channels),
            ident.map_or(0, |ident| ident.sample_rate),
            crate::hda::PCM_CHANNELS,
            crate::hda::PCM_SAMPLE_RATE_HZ,
            vorbis_resampler_label(ident),
            self.playback_ogg_serial
                .map(|serial| format!("{:08x}", serial))
                .unwrap_or_else(|| String::from("none"))
        );
    }

    async fn tick_playback_stream_probe(&mut self) {
        if self.playback_stream != PlaybackStreamProbeState::WaitingRequest {
            return;
        }
        if self.playback_decoder != PlaybackDecoderProbeState::Done {
            return;
        }
        if self.playback_vorbis_decoder.is_none() {
            self.playback_stream = PlaybackStreamProbeState::Failed;
            crate::log!("spotify-playback: stream failed reason=no-decoder\n");
            return;
        }

        match self.drain_playback_pending_pcm() {
            Ok(pushed) => {
                if pushed.remaining_samples != 0 {
                    return;
                }
            }
            Err(err) => {
                self.playback_stream = PlaybackStreamProbeState::Failed;
                crate::log!("spotify-playback: stream failed reason=pcm-drain err={}\n", err);
                return;
            }
        }

        let Some(stream) = self.pcm_sink.as_ref() else {
            self.playback_stream = PlaybackStreamProbeState::Failed;
            crate::log!("spotify-playback: stream failed reason=no-pcm-sink\n");
            return;
        };
        let queued_samples = stream.queued_samples().unwrap_or(0);
        let queued_target = (crate::hda::PCM_SAMPLE_RATE_HZ as usize)
            .saturating_mul(crate::hda::PCM_CHANNELS)
            .saturating_mul(2);
        if queued_samples > queued_target {
            return;
        }

        let Some(url) = self.playback_cdn_url.clone() else {
            self.playback_stream = PlaybackStreamProbeState::Failed;
            crate::log!("spotify-playback: stream failed reason=no-cdn-url\n");
            return;
        };
        let Some(key) = self.playback_audio_key_bytes else {
            self.playback_stream = PlaybackStreamProbeState::Failed;
            crate::log!("spotify-playback: stream failed reason=no-audio-key\n");
            return;
        };

        if self.playback_cdn_offset == 0
            && let Some(decrypted) = self.playback_cdn_warmup.take()
        {
            let offset = 0usize;
            let encrypted_len = decrypted.len();
            if encrypted_len == 0 {
                self.playback_stream = PlaybackStreamProbeState::Done;
                crate::log!("spotify-playback: stream eof offset=0 source=warmup\n");
                return;
            }
            self.playback_cdn_offset = encrypted_len;
            self.process_playback_stream_decrypted_chunk(
                offset,
                encrypted_len,
                decrypted.as_slice(),
                encrypted_len < CDN_WARMUP_BYTES,
                "warmup",
            );
            return;
        }

        let offset = self.playback_cdn_offset;
        match crate::r::net::https::get_range_bytes_shared(
            url.as_str(),
            offset,
            CDN_STREAM_CHUNK_BYTES,
            20_000,
            CDN_STREAM_CHUNK_BYTES + 4096,
        )
        .await
        {
            Ok(encrypted) => {
                if encrypted.is_empty() {
                    self.playback_stream = PlaybackStreamProbeState::Done;
                    crate::log!(
                        "spotify-playback: stream eof offset={} pending_pcm={} pages={} audio_packets={}\n",
                        offset,
                        self.playback_pending_pcm.len(),
                        self.playback_ogg_stream.pages,
                        self.playback_ogg_stream.audio_packets
                    );
                    return;
                }

                let encrypted_len = encrypted.len();
                let Some(decrypted) =
                    decrypt_spotify_audio_range(&key, offset, encrypted.as_slice())
                else {
                    self.playback_stream = PlaybackStreamProbeState::Failed;
                    crate::log!(
                        "spotify-playback: stream decrypt failed offset={} encrypted_bytes={}\n",
                        offset,
                        encrypted_len
                    );
                    return;
                };
                self.playback_cdn_offset = self.playback_cdn_offset.saturating_add(encrypted_len);

                let eof = encrypted_len < CDN_STREAM_CHUNK_BYTES;
                self.process_playback_stream_decrypted_chunk(
                    offset,
                    encrypted_len,
                    decrypted.as_slice(),
                    eof,
                    "range",
                );
            }
            Err(err) => {
                self.playback_stream = PlaybackStreamProbeState::Failed;
                crate::log!(
                    "spotify-playback: stream fetch failed offset={} bytes={} err={}\n",
                    offset,
                    CDN_STREAM_CHUNK_BYTES,
                    err
                );
            }
        }
    }

    fn process_playback_stream_decrypted_chunk(
        &mut self,
        offset: usize,
        encrypted_len: usize,
        decrypted: &[u8],
        eof: bool,
        source: &str,
    ) {
        let mut packets = Vec::new();
        let ogg_stats = self.playback_ogg_stream.feed(decrypted, &mut packets);
        let mut decode_stats = crate::r::spotify_vorbis::VorbisDecodeStats::default();
        let Some(decoder) = self.playback_vorbis_decoder.as_mut() else {
            self.playback_stream = PlaybackStreamProbeState::Failed;
            crate::log!("spotify-playback: stream failed reason=decoder-lost\n");
            return;
        };
        for packet in &packets {
            match decoder.decode_packet_to_i16(packet.as_slice(), &mut self.playback_pending_pcm) {
                Ok(stats) => decode_stats.add(stats),
                Err(err) => {
                    self.playback_stream = PlaybackStreamProbeState::Failed;
                    crate::log!(
                        "spotify-playback: stream decode failed offset={} source={} err={:?} packet_len={} packets_ready={}\n",
                        offset,
                        source,
                        err,
                        packet.len(),
                        packets.len()
                    );
                    return;
                }
            }
        }

        let drain = match self.drain_playback_pending_pcm() {
            Ok(drain) => drain,
            Err(err) => {
                self.playback_stream = PlaybackStreamProbeState::Failed;
                crate::log!("spotify-playback: stream pcm push failed err={}\n", err);
                return;
            }
        };
        if eof && self.playback_pending_pcm.is_empty() {
            self.playback_stream = PlaybackStreamProbeState::Done;
        }
        crate::log!(
            "spotify-playback: stream chunk ok source={} offset={} encrypted_bytes={} decrypted_bytes={} eof={} ogg_pages={} ogg_packets={} ogg_audio_packets={} parser_buffer={} packets_ready={} decoded_packets={} empty_packets={} source_frames={} sink_frames={} pcm_samples={} pushed_samples={} remaining_pcm={} queued_before={} queued_after={} next_offset={} state={}\n",
            source,
            offset,
            encrypted_len,
            decrypted.len(),
            eof as u8,
            ogg_stats.pages,
            ogg_stats.packets,
            ogg_stats.audio_packets,
            self.playback_ogg_stream.buffered_bytes(),
            packets.len(),
            decode_stats.packets_decoded,
            decode_stats.empty_packets,
            decode_stats.source_frames,
            decode_stats.sink_frames,
            decode_stats.pcm_samples,
            drain.pushed_samples,
            drain.remaining_samples,
            drain.queued_before,
            drain.queued_after,
            self.playback_cdn_offset,
            self.playback_target_label()
        );
    }

    fn drain_playback_pending_pcm(&mut self) -> Result<PcmDrainProbe, &'static str> {
        let mut probe = PcmDrainProbe::default();
        let Some(stream) = self.pcm_sink.as_mut() else {
            return Err("no pcm sink");
        };
        probe.queued_before = stream.queued_samples().unwrap_or(0);
        probe.writable_before = stream
            .writable_samples(crate::hda::PCM_CHANNELS)
            .unwrap_or(0);
        let push_samples = self.playback_pending_pcm.len().min(probe.writable_before)
            & !(crate::hda::PCM_CHANNELS - 1);
        if push_samples != 0 {
            let chunk = self.playback_pending_pcm[..push_samples].to_vec();
            stream.push_samples(chunk.as_slice())?;
            self.playback_pending_pcm.drain(..push_samples);
            probe.pushed_samples = push_samples;
        }
        probe.remaining_samples = self.playback_pending_pcm.len();
        probe.queued_after = stream.queued_samples().unwrap_or(0);
        Ok(probe)
    }
}

fn dealer_reply_json(key: &str, success: bool) -> String {
    let reply = serde_json::json!({
        "type": "reply",
        "key": key,
        "payload": {
            "success": success,
        },
    });
    serde_json::to_string(&reply).unwrap_or_else(|_| {
        format!("{{\"type\":\"reply\",\"key\":\"{}\",\"payload\":{{\"success\":false}}}}", key)
    })
}

struct DealerRequestProbe {
    endpoint: String,
    message_id: u32,
    sent_by_device_id: String,
    sent_by_len: usize,
    payload_len: usize,
    playback_target: Option<SpotifyPlaybackTarget>,
    success: bool,
}

#[derive(Clone, Debug)]
struct SpotifyPlaybackTarget {
    source: &'static str,
    context_uri: Option<String>,
    track_uri: Option<String>,
    track_uid: Option<String>,
    track_gid_raw: Option<Vec<u8>>,
    track_gid_hex: Option<String>,
    position_ms: Option<u32>,
    position_as_of_timestamp_ms: Option<u32>,
    paused: Option<bool>,
    playback_timestamp_ms: Option<u64>,
}

struct SpotifyTrackMetadataProbe {
    track_bytes: usize,
    name: Option<String>,
    duration_ms: Option<u32>,
    files: Vec<SpotifyAudioFileCandidate>,
}

#[derive(Clone, Debug)]
struct SpotifyAudioFileCandidate {
    format: u64,
    file_id_raw: Option<Vec<u8>>,
    file_id_hex: Option<String>,
}

struct SpotifyStorageResolveProbe {
    result: u64,
    urls: Vec<String>,
    file_id_hex: Option<String>,
}

#[derive(Default)]
struct SpotifyOggProbe {
    pages: usize,
    packets: usize,
    audio_packets: usize,
    first_page_offset: Option<usize>,
    decoder_offset_page: bool,
    vorbis_ident: bool,
    vorbis_comment: bool,
    vorbis_setup: bool,
    first_vorbis_offset: Option<usize>,
    first_audio_offset: Option<usize>,
    ident: Option<VorbisIdentProbe>,
    serial: Option<u32>,
    capture: Option<VorbisPacketCapture>,
}

#[derive(Clone, Debug, Default)]
struct VorbisPacketCapture {
    ident: Vec<u8>,
    comment: Vec<u8>,
    setup: Vec<u8>,
    audio_packets: Vec<Vec<u8>>,
    audio_bytes: usize,
}

#[derive(Default)]
struct PcmDrainProbe {
    writable_before: usize,
    queued_before: usize,
    queued_after: usize,
    pushed_samples: usize,
    remaining_samples: usize,
}

#[derive(Default)]
struct SpotifyOggStreamStats {
    pages: usize,
    packets: usize,
    audio_packets: usize,
    header_packets: usize,
    skipped_bytes: usize,
}

#[derive(Default)]
struct SpotifyOggStreamParser {
    buffer: Vec<u8>,
    packet: Vec<u8>,
    pages: usize,
    packets: usize,
    audio_packets: usize,
    header_packets: usize,
    serial: Option<u32>,
}

impl SpotifyOggStreamParser {
    fn new() -> Self {
        Self::default()
    }

    fn buffered_bytes(&self) -> usize {
        self.buffer.len().saturating_add(self.packet.len())
    }

    fn feed(&mut self, data: &[u8], out: &mut Vec<Vec<u8>>) -> SpotifyOggStreamStats {
        self.buffer.extend_from_slice(data);
        let mut stats = SpotifyOggStreamStats::default();

        loop {
            if self.buffer.len() < 27 {
                break;
            }
            if self.buffer.get(0..4) != Some(b"OggS") {
                if let Some(next) = self.buffer.windows(4).position(|window| window == b"OggS") {
                    if next != 0 {
                        self.buffer.drain(..next);
                        stats.skipped_bytes = stats.skipped_bytes.saturating_add(next);
                    }
                    continue;
                }

                let keep = self.buffer.len().min(3);
                let drop_len = self.buffer.len().saturating_sub(keep);
                if drop_len != 0 {
                    self.buffer.drain(..drop_len);
                    stats.skipped_bytes = stats.skipped_bytes.saturating_add(drop_len);
                }
                break;
            }

            let page_segments = self.buffer[26] as usize;
            let lacing_start = 27usize;
            let payload_start = lacing_start.saturating_add(page_segments);
            if self.buffer.len() < payload_start {
                break;
            }
            let payload_len: usize = self.buffer[lacing_start..payload_start]
                .iter()
                .map(|value| *value as usize)
                .sum();
            let page_len = payload_start.saturating_add(payload_len);
            if self.buffer.len() < page_len {
                break;
            }

            if self.serial.is_none() {
                self.serial = Some(u32::from_le_bytes([
                    self.buffer[14],
                    self.buffer[15],
                    self.buffer[16],
                    self.buffer[17],
                ]));
            }
            self.pages = self.pages.saturating_add(1);
            stats.pages = stats.pages.saturating_add(1);

            let mut payload_cursor = payload_start;
            let lacing = self.buffer[lacing_start..payload_start].to_vec();
            for segment in lacing {
                let segment_len = segment as usize;
                let segment_end = payload_cursor.saturating_add(segment_len);
                if segment_end > page_len {
                    break;
                }
                self.packet
                    .extend_from_slice(&self.buffer[payload_cursor..segment_end]);
                payload_cursor = segment_end;

                if segment < 255 {
                    self.packets = self.packets.saturating_add(1);
                    stats.packets = stats.packets.saturating_add(1);
                    if is_vorbis_header_packet(self.packet.as_slice()) {
                        self.header_packets = self.header_packets.saturating_add(1);
                        stats.header_packets = stats.header_packets.saturating_add(1);
                        self.packet.clear();
                    } else if !self.packet.is_empty() {
                        self.audio_packets = self.audio_packets.saturating_add(1);
                        stats.audio_packets = stats.audio_packets.saturating_add(1);
                        out.push(core::mem::take(&mut self.packet));
                    } else {
                        self.packet.clear();
                    }
                }
            }

            self.buffer.drain(..page_len);
        }

        stats
    }
}

#[derive(Clone, Copy, Debug)]
struct VorbisIdentProbe {
    channels: u8,
    sample_rate: u32,
    bitrate_maximum: i32,
    bitrate_nominal: i32,
    bitrate_minimum: i32,
    blocksize_0_exp: u8,
    blocksize_1_exp: u8,
    framing: bool,
    valid: bool,
}

#[derive(Default)]
struct TransferStateProbeSummary {
    top_mask: u64,
    playback_mask: u64,
    session_mask: u64,
    queue_mask: u64,
    context_pages: usize,
    context_tracks: usize,
    queue_tracks: usize,
    queue_is_playing: bool,
}

#[derive(Default)]
struct TransferTrackProbe {
    uri: Option<String>,
    uid: Option<String>,
    gid: Option<Vec<u8>>,
}

struct DealerTransferAck {
    message_id: u32,
    sent_by_device_id: String,
}

fn inspect_dealer_request(
    headers: &BTreeMap<String, String>,
    payload: &DealerRequestPayload,
) -> DealerRequestProbe {
    let Ok(text) = decode_dealer_request_payload(headers, payload.compressed.as_str()) else {
        return DealerRequestProbe {
            endpoint: String::from("decode-failed"),
            message_id: 0,
            sent_by_device_id: String::new(),
            sent_by_len: 0,
            payload_len: 0,
            playback_target: None,
            success: false,
        };
    };
    let payload_len = text.len();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.as_str()) else {
        return DealerRequestProbe {
            endpoint: String::from("json-failed"),
            message_id: 0,
            sent_by_device_id: String::new(),
            sent_by_len: 0,
            payload_len,
            playback_target: None,
            success: false,
        };
    };
    let message_id = value
        .get("message_id")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let sent_by_device_id = value
        .get("sent_by_device_id")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
        .unwrap_or_default();
    let sent_by_len = sent_by_device_id.len();
    let endpoint = value
        .get("command")
        .and_then(|command| command.get("endpoint"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let success = is_connect_control_endpoint(endpoint);
    let playback_target = match endpoint {
        "transfer" => inspect_transfer_target(&value),
        "play" => inspect_play_target(&value),
        _ => None,
    };

    DealerRequestProbe {
        endpoint: String::from(endpoint),
        message_id,
        sent_by_device_id,
        sent_by_len,
        payload_len,
        playback_target,
        success,
    }
}

fn parse_transfer_target_base64(encoded: &str) -> Option<SpotifyPlaybackTarget> {
    let decoded = decode_base64(encoded)?;
    parse_transfer_target(decoded.as_slice())
}

fn inspect_transfer_target(value: &serde_json::Value) -> Option<SpotifyPlaybackTarget> {
    let data = value.get("command").and_then(|command| command.get("data"));
    let data_kind = json_value_kind(data);
    let data_len = data.and_then(serde_json::Value::as_str).map_or(0, str::len);
    let decoded = data
        .and_then(serde_json::Value::as_str)
        .and_then(decode_base64);
    let target = decoded
        .as_deref()
        .and_then(parse_transfer_target)
        .or_else(|| {
            data.and_then(serde_json::Value::as_str)
                .and_then(parse_transfer_target_base64)
        });
    if target.is_none() {
        let summary = decoded
            .as_deref()
            .map(summarize_transfer_state)
            .unwrap_or_default();
        crate::log!(
            "spotify-playback: transfer target missing data_kind={} data_len={} decoded_bytes={} top=0x{:x} playback=0x{:x} session=0x{:x} queue=0x{:x} context_pages={} context_tracks={} queue_tracks={} queue_playing={}\n",
            data_kind,
            data_len,
            decoded.as_ref().map_or(0, Vec::len),
            summary.top_mask,
            summary.playback_mask,
            summary.session_mask,
            summary.queue_mask,
            summary.context_pages,
            summary.context_tracks,
            summary.queue_tracks,
            summary.queue_is_playing as u8
        );
    }
    target
}

fn inspect_play_target(value: &serde_json::Value) -> Option<SpotifyPlaybackTarget> {
    let command = value.get("command")?;
    let context = command.get("context");
    let options = command.get("options");
    let context_uri = context.and_then(json_context_uri);
    let target = command
        .get("track")
        .and_then(json_track_target)
        .or_else(|| json_skip_to_target(options, context))
        .or_else(|| context.and_then(json_first_context_track_target));

    let Some(mut target) = target else {
        crate::log!(
            "spotify-playback: play target missing context_kind={} context_uri={} options_kind={}\n",
            json_value_kind(context),
            context_uri.as_deref().unwrap_or(""),
            json_value_kind(options)
        );
        return None;
    };
    if target.context_uri.is_none() {
        target.context_uri = context_uri;
    }
    if target.position_ms.is_none() {
        target.position_ms = options.and_then(json_play_options_position_ms);
    }
    if target.paused.is_none() {
        target.paused = options.and_then(json_play_options_initially_paused);
    }
    target.source = "play";
    crate::log!(
        "spotify-playback: play target parsed track_uri={} gid={} uid_len={} context_uri={} position_ms={} paused={}\n",
        target.track_uri.as_deref().unwrap_or(""),
        target.track_gid_hex.as_deref().unwrap_or(""),
        target.track_uid.as_ref().map_or(0, |uid| uid.len()),
        target.context_uri.as_deref().unwrap_or(""),
        target.position_ms.unwrap_or(0),
        target.paused.map_or(-1, |paused| paused as i32)
    );
    Some(target)
}

fn parse_transfer_target(transfer: &[u8]) -> Option<SpotifyPlaybackTarget> {
    let playback = proto_len(transfer, 2);
    let session = proto_len(transfer, 3);
    let queue = proto_len(transfer, 4);

    let context = session.and_then(|session| proto_len(session, 2));
    let context_uri = context.and_then(|context| proto_string(context, 1));
    let session_current_uid = session.and_then(|session| proto_string(session, 3));
    let position_as_of_timestamp_ms = playback
        .and_then(|playback| proto_varint(playback, 2))
        .and_then(|position| u32::try_from(position).ok());
    let paused = playback
        .and_then(|playback| proto_varint(playback, 4))
        .map(|value| value != 0);
    let playback_timestamp_ms = playback.and_then(|playback| proto_varint(playback, 1));
    let position_ms = normalized_transfer_position_ms(
        position_as_of_timestamp_ms,
        playback_timestamp_ms,
        paused.unwrap_or(false),
    );
    let queue_is_playing = queue
        .and_then(|queue| proto_varint(queue, 2))
        .is_some_and(|value| value != 0);

    let mut source = "none";
    let mut track = if queue_is_playing {
        let track = queue.and_then(first_queue_track);
        if track.is_some() {
            source = "queue.playing";
        }
        track
    } else {
        None
    };
    if track.is_none() {
        track = playback
            .and_then(|playback| proto_len(playback, 5))
            .and_then(parse_context_track);
        if track.is_some() {
            source = "playback.current_track";
        }
    }
    if track.is_none() {
        track = playback
            .and_then(|playback| proto_len(playback, 6))
            .and_then(parse_context_track);
        if track.is_some() {
            source = "playback.associated_current_track";
        }
    }
    if track.is_none() {
        track = session_current_uid
            .as_deref()
            .and_then(|uid| context.and_then(|context| context_track_by_uid(context, uid)));
        if track.is_some() {
            source = "session.context.current_uid";
        }
    }
    if track.is_none() {
        track = context.and_then(first_context_track);
        if track.is_some() {
            source = "session.context";
        }
    }
    if track.is_none() {
        track = queue.and_then(first_queue_track);
        if track.is_some() {
            source = "queue";
        }
    }
    let track = track?;

    let track_uri = track
        .uri
        .or_else(|| track.gid.as_deref().and_then(track_uri_from_gid));
    let track_gid_raw = track
        .gid
        .clone()
        .or_else(|| track_uri.as_deref().and_then(track_gid_from_uri));
    let track_gid_hex = track_gid_raw.as_deref().map(hex_string);
    let track_uid = track.uid.or(session_current_uid);

    Some(SpotifyPlaybackTarget {
        source,
        context_uri,
        track_uri,
        track_uid,
        track_gid_raw,
        track_gid_hex,
        position_ms,
        position_as_of_timestamp_ms,
        paused,
        playback_timestamp_ms,
    })
}

fn normalized_transfer_position_ms(
    position_as_of_timestamp_ms: Option<u32>,
    playback_timestamp_ms: Option<u64>,
    paused: bool,
) -> Option<u32> {
    let position_ms = position_as_of_timestamp_ms?;
    if paused {
        return Some(position_ms);
    }

    let Some(playback_timestamp_ms) = playback_timestamp_ms else {
        return Some(position_ms);
    };
    let Some(now_ms) = crate::time::unix_time_seconds().map(|now| now.saturating_mul(1000)) else {
        return Some(position_ms);
    };
    let elapsed_ms = now_ms.saturating_sub(playback_timestamp_ms);
    let adjusted = u64::from(position_ms).saturating_add(elapsed_ms);
    Some(adjusted.min(u64::from(u32::MAX)) as u32)
}

fn json_value_kind(value: Option<&serde_json::Value>) -> &'static str {
    match value {
        Some(serde_json::Value::Null) => "null",
        Some(serde_json::Value::Bool(_)) => "bool",
        Some(serde_json::Value::Number(_)) => "number",
        Some(serde_json::Value::String(_)) => "string",
        Some(serde_json::Value::Array(_)) => "array",
        Some(serde_json::Value::Object(_)) => "object",
        None => "missing",
    }
}

fn decode_base64(encoded: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(encoded.as_bytes())
        .ok()
        .or_else(|| {
            base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(encoded.as_bytes())
                .ok()
        })
        .or_else(|| {
            base64::engine::general_purpose::URL_SAFE
                .decode(encoded.as_bytes())
                .ok()
        })
        .or_else(|| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(encoded.as_bytes())
                .ok()
        })
        .or_else(|| decode_base64_with_padding(encoded))
}

fn decode_base64_with_padding(encoded: &str) -> Option<Vec<u8>> {
    let rem = encoded.len() % 4;
    if rem == 0 {
        return None;
    }
    let mut padded = String::from(encoded);
    for _ in 0..(4 - rem) {
        padded.push('=');
    }
    base64::engine::general_purpose::STANDARD
        .decode(padded.as_bytes())
        .ok()
        .or_else(|| {
            base64::engine::general_purpose::URL_SAFE
                .decode(padded.as_bytes())
                .ok()
        })
}

fn json_context_uri(value: &serde_json::Value) -> Option<String> {
    json_string_field(value, "uri")
        .or_else(|| json_string_field(value, "context_uri"))
        .or_else(|| json_string_field(value, "contextUri"))
}

fn json_skip_to_target(
    options: Option<&serde_json::Value>,
    context: Option<&serde_json::Value>,
) -> Option<SpotifyPlaybackTarget> {
    let skip_to = options?.get("skip_to").or_else(|| options?.get("skipTo"))?;
    json_track_target(skip_to).or_else(|| {
        if let Some(uid) = json_string_field(skip_to, "track_uid")
            .or_else(|| json_string_field(skip_to, "trackUid"))
            && let Some(target) =
                context.and_then(|context| json_context_track_by_uid(context, uid.as_str()))
        {
            return Some(target);
        }
        let index = skip_to
            .get("track_index")
            .or_else(|| skip_to.get("trackIndex"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())?;
        context.and_then(|context| json_context_track_by_index(context, index))
    })
}

fn json_play_options_position_ms(options: &serde_json::Value) -> Option<u32> {
    options
        .get("seek_to")
        .or_else(|| options.get("seekTo"))
        .or_else(|| options.get("position_ms"))
        .or_else(|| options.get("positionMs"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn json_play_options_initially_paused(options: &serde_json::Value) -> Option<bool> {
    options
        .get("initially_paused")
        .or_else(|| options.get("initiallyPaused"))
        .and_then(serde_json::Value::as_bool)
}

fn json_first_context_track_target(context: &serde_json::Value) -> Option<SpotifyPlaybackTarget> {
    json_context_track_by_index(context, 0)
}

fn json_context_track_by_uid(
    context: &serde_json::Value,
    wanted_uid: &str,
) -> Option<SpotifyPlaybackTarget> {
    let pages = context.get("pages").and_then(serde_json::Value::as_array)?;
    for page in pages {
        let Some(tracks) = page.get("tracks").and_then(serde_json::Value::as_array) else {
            continue;
        };
        for track in tracks {
            let uid = json_string_field(track, "uid")
                .or_else(|| json_string_field(track, "track_uid"))
                .or_else(|| json_string_field(track, "trackUid"));
            if uid.as_deref() == Some(wanted_uid) {
                return json_track_target(track);
            }
        }
    }
    None
}

fn json_context_track_by_index(
    context: &serde_json::Value,
    wanted: usize,
) -> Option<SpotifyPlaybackTarget> {
    let mut seen = 0usize;
    let pages = context.get("pages").and_then(serde_json::Value::as_array)?;
    for page in pages {
        let Some(tracks) = page.get("tracks").and_then(serde_json::Value::as_array) else {
            continue;
        };
        for track in tracks {
            if seen == wanted {
                return json_track_target(track);
            }
            seen = seen.saturating_add(1);
        }
    }
    None
}

fn json_track_target(track: &serde_json::Value) -> Option<SpotifyPlaybackTarget> {
    let track_uri = json_string_field(track, "uri")
        .or_else(|| json_string_field(track, "track_uri"))
        .or_else(|| json_string_field(track, "trackUri"));
    let track_uid = json_string_field(track, "uid")
        .or_else(|| json_string_field(track, "track_uid"))
        .or_else(|| json_string_field(track, "trackUid"));
    let track_gid_raw = json_string_field(track, "gid")
        .or_else(|| json_string_field(track, "track_gid"))
        .or_else(|| json_string_field(track, "trackGid"))
        .and_then(|gid| decode_base64(gid.as_str()).or_else(|| parse_hex_bytes(gid.as_str())));

    let metadata = track.get("metadata");
    let track_uri = track_uri
        .or_else(|| metadata.and_then(|metadata| json_string_field(metadata, "uri")))
        .or_else(|| track_gid_raw.as_deref().and_then(track_uri_from_gid));
    let track_gid_raw = track_gid_raw.or_else(|| track_uri.as_deref().and_then(track_gid_from_uri));
    let track_gid_hex = track_gid_raw.as_deref().map(hex_string);

    (track_uri.is_some() || track_gid_raw.is_some()).then_some(SpotifyPlaybackTarget {
        source: "json-track",
        context_uri: None,
        track_uri,
        track_uid,
        track_gid_raw,
        track_gid_hex,
        position_ms: None,
        position_as_of_timestamp_ms: None,
        paused: None,
        playback_timestamp_ms: None,
    })
}

fn json_string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(String::from)
}

fn parse_context_track(data: &[u8]) -> Option<TransferTrackProbe> {
    let uri = proto_string(data, 1);
    let uid = proto_string(data, 2);
    let gid = proto_len(data, 3).map(|gid| gid.to_vec());
    (uri.is_some() || uid.is_some() || gid.is_some()).then_some(TransferTrackProbe {
        uri,
        uid,
        gid,
    })
}

fn context_track_by_uid(context: &[u8], wanted_uid: &str) -> Option<TransferTrackProbe> {
    let mut index = 0usize;
    while let Some((field, wire, value, _varint)) = proto_next_field(context, &mut index) {
        if field == 5 && wire == 2 {
            if let Some(track) = context_page_track_by_uid(value, wanted_uid) {
                return Some(track);
            }
        }
    }
    None
}

fn context_page_track_by_uid(page: &[u8], wanted_uid: &str) -> Option<TransferTrackProbe> {
    let mut index = 0usize;
    while let Some((field, wire, value, _varint)) = proto_next_field(page, &mut index) {
        if field == 4 && wire == 2 {
            let track = parse_context_track(value)?;
            if track.uid.as_deref() == Some(wanted_uid) {
                return Some(track);
            }
        }
    }
    None
}

fn first_context_track(context: &[u8]) -> Option<TransferTrackProbe> {
    let mut index = 0usize;
    while let Some((field, wire, value, _varint)) = proto_next_field(context, &mut index) {
        if field == 5 && wire == 2 {
            if let Some(track) = first_context_page_track(value) {
                return Some(track);
            }
        }
    }
    None
}

fn first_context_page_track(page: &[u8]) -> Option<TransferTrackProbe> {
    let mut index = 0usize;
    while let Some((field, wire, value, _varint)) = proto_next_field(page, &mut index) {
        if field == 4 && wire == 2 {
            if let Some(track) = parse_context_track(value) {
                return Some(track);
            }
        }
    }
    None
}

fn first_queue_track(queue: &[u8]) -> Option<TransferTrackProbe> {
    let mut index = 0usize;
    while let Some((field, wire, value, _varint)) = proto_next_field(queue, &mut index) {
        if field == 1 && wire == 2 {
            if let Some(track) = parse_context_track(value) {
                return Some(track);
            }
        }
    }
    None
}

fn summarize_transfer_state(transfer: &[u8]) -> TransferStateProbeSummary {
    let playback = proto_len(transfer, 2);
    let session = proto_len(transfer, 3);
    let queue = proto_len(transfer, 4);
    let context = session.and_then(|session| proto_len(session, 2));

    TransferStateProbeSummary {
        top_mask: proto_field_mask(transfer),
        playback_mask: playback.map(proto_field_mask).unwrap_or(0),
        session_mask: session.map(proto_field_mask).unwrap_or(0),
        queue_mask: queue.map(proto_field_mask).unwrap_or(0),
        context_pages: context.map(count_context_pages).unwrap_or(0),
        context_tracks: context.map(count_context_tracks).unwrap_or(0),
        queue_tracks: queue.map(count_queue_tracks).unwrap_or(0),
        queue_is_playing: queue
            .and_then(|queue| proto_varint(queue, 2))
            .is_some_and(|value| value != 0),
    }
}

fn proto_field_mask(data: &[u8]) -> u64 {
    let mut mask = 0u64;
    let mut index = 0usize;
    while let Some((field, _wire, _value, _varint)) = proto_next_field(data, &mut index) {
        if (1..=63).contains(&field) {
            mask |= 1u64 << field;
        }
    }
    mask
}

fn count_context_pages(context: &[u8]) -> usize {
    let mut count = 0usize;
    let mut index = 0usize;
    while let Some((field, wire, _value, _varint)) = proto_next_field(context, &mut index) {
        if field == 5 && wire == 2 {
            count = count.saturating_add(1);
        }
    }
    count
}

fn count_context_tracks(context: &[u8]) -> usize {
    let mut count = 0usize;
    let mut index = 0usize;
    while let Some((field, wire, value, _varint)) = proto_next_field(context, &mut index) {
        if field == 5 && wire == 2 {
            count = count.saturating_add(count_context_page_tracks(value));
        }
    }
    count
}

fn count_context_page_tracks(page: &[u8]) -> usize {
    let mut count = 0usize;
    let mut index = 0usize;
    while let Some((field, wire, _value, _varint)) = proto_next_field(page, &mut index) {
        if field == 4 && wire == 2 {
            count = count.saturating_add(1);
        }
    }
    count
}

fn count_queue_tracks(queue: &[u8]) -> usize {
    let mut count = 0usize;
    let mut index = 0usize;
    while let Some((field, wire, _value, _varint)) = proto_next_field(queue, &mut index) {
        if field == 1 && wire == 2 {
            count = count.saturating_add(1);
        }
    }
    count
}

fn decode_dealer_request_payload(
    headers: &BTreeMap<String, String>,
    compressed: &str,
) -> Result<String, String> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(compressed.as_bytes())
        .map_err(|err| format!("base64 {}", err))?;
    let bytes = if header_value_contains_token(headers, "Transfer-Encoding", "gzip") {
        decode_gzip(decoded.as_slice(), DEALER_REQUEST_MAX_BYTES)
            .ok_or_else(|| String::from("gzip decode"))?
    } else {
        decoded
    };
    if bytes.len() > DEALER_REQUEST_MAX_BYTES {
        return Err(format!("payload too large {}", bytes.len()));
    }
    String::from_utf8(bytes).map_err(|err| format!("utf8 {}", err))
}

fn is_connect_control_endpoint(endpoint: &str) -> bool {
    matches!(
        endpoint,
        "transfer"
            | "play"
            | "resume"
            | "pause"
            | "seek_to"
            | "skip_next"
            | "skip_prev"
            | "set_queue"
            | "set_options"
            | "set_repeating_context"
            | "set_repeating_track"
            | "set_shuffling_context"
            | "add_to_queue"
            | "update_context"
    )
}

fn header_value_contains_token(
    headers: &BTreeMap<String, String>,
    name: &str,
    token: &str,
) -> bool {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| ascii_contains_token(value.as_bytes(), token.as_bytes()))
        .unwrap_or(false)
}

fn ascii_lower(byte: u8) -> u8 {
    if byte.is_ascii_uppercase() {
        byte + 32
    } else {
        byte
    }
}

fn ascii_contains_token(value: &[u8], token: &[u8]) -> bool {
    if token.is_empty() || value.len() < token.len() {
        return false;
    }

    'outer: for start in 0..=value.len().saturating_sub(token.len()) {
        for off in 0..token.len() {
            if ascii_lower(value[start + off]) != ascii_lower(token[off]) {
                continue 'outer;
            }
        }
        return true;
    }
    false
}

fn decode_gzip(body: &[u8], max_out: usize) -> Option<Vec<u8>> {
    if body.len() < 18 {
        return None;
    }
    if body[0] != 0x1f || body[1] != 0x8b || body[2] != 8 {
        return None;
    }

    let flags = body[3];
    let mut pos: usize = 10;
    let len = body.len();

    if (flags & 0x04) != 0 {
        if pos + 2 > len {
            return None;
        }
        let xlen = u16::from_le_bytes([body[pos], body[pos + 1]]) as usize;
        pos += 2;
        if pos + xlen > len {
            return None;
        }
        pos += xlen;
    }

    if (flags & 0x08) != 0 {
        while pos < len && body[pos] != 0 {
            pos += 1;
        }
        pos = pos.saturating_add(1);
        if pos > len {
            return None;
        }
    }

    if (flags & 0x10) != 0 {
        while pos < len && body[pos] != 0 {
            pos += 1;
        }
        pos = pos.saturating_add(1);
        if pos > len {
            return None;
        }
    }

    if (flags & 0x02) != 0 {
        pos = pos.saturating_add(2);
        if pos > len {
            return None;
        }
    }

    if pos + 8 > len {
        return None;
    }
    let deflate_end = len.saturating_sub(8);
    if deflate_end < pos {
        return None;
    }

    miniz_oxide::inflate::decompress_to_vec_with_limit(&body[pos..deflate_end], max_out).ok()
}

fn proto_varint(data: &[u8], wanted_field: u32) -> Option<u64> {
    let mut index = 0usize;
    while let Some((field, wire, _value, varint)) = proto_next_field(data, &mut index) {
        if field == wanted_field && wire == 0 {
            return Some(varint);
        }
    }
    None
}

fn proto_len(data: &[u8], wanted_field: u32) -> Option<&[u8]> {
    let mut index = 0usize;
    while let Some((field, wire, value, _varint)) = proto_next_field(data, &mut index) {
        if field == wanted_field && wire == 2 {
            return Some(value);
        }
    }
    None
}

fn proto_string(data: &[u8], wanted_field: u32) -> Option<String> {
    let raw = proto_len(data, wanted_field)?;
    if raw.is_empty() {
        return None;
    }
    core::str::from_utf8(raw).ok().map(String::from)
}

fn proto_next_field<'a>(data: &'a [u8], index: &mut usize) -> Option<(u32, u8, &'a [u8], u64)> {
    let key = read_proto_varint(data, index)?;
    let field = u32::try_from(key >> 3).ok()?;
    let wire = (key & 0x07) as u8;
    match wire {
        0 => {
            let value = read_proto_varint(data, index)?;
            Some((field, wire, &[], value))
        }
        1 => {
            let end = index.checked_add(8)?;
            if end > data.len() {
                return None;
            }
            let value = &data[*index..end];
            *index = end;
            Some((field, wire, value, 0))
        }
        2 => {
            let len = usize::try_from(read_proto_varint(data, index)?).ok()?;
            let end = index.checked_add(len)?;
            if end > data.len() {
                return None;
            }
            let value = &data[*index..end];
            *index = end;
            Some((field, wire, value, 0))
        }
        5 => {
            let end = index.checked_add(4)?;
            if end > data.len() {
                return None;
            }
            let value = &data[*index..end];
            *index = end;
            Some((field, wire, value, 0))
        }
        _ => None,
    }
}

fn read_proto_varint(data: &[u8], index: &mut usize) -> Option<u64> {
    let mut shift = 0u32;
    let mut value = 0u64;
    while *index < data.len() && shift <= 63 {
        let byte = data[*index];
        *index += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if (byte & 0x80) == 0 {
            return Some(value);
        }
        shift += 7;
    }
    None
}

fn hex_string(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(data.len().saturating_mul(2));
    for byte in data.iter().copied() {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn parse_hex_bytes(raw: &str) -> Option<Vec<u8>> {
    let bytes = raw.as_bytes();
    if bytes.is_empty() || bytes.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut idx = 0usize;
    while idx < bytes.len() {
        let hi = hex_nibble(bytes[idx])?;
        let lo = hex_nibble(bytes[idx + 1])?;
        out.push((hi << 4) | lo);
        idx += 2;
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn track_uri_from_gid(gid: &[u8]) -> Option<String> {
    let base62 = spotify_id_base62_from_raw(gid)?;
    Some(format!("spotify:track:{}", base62))
}

fn spotify_id_base62_from_raw(raw: &[u8]) -> Option<String> {
    const BASE62_DIGITS: &[u8; 62] =
        b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    if raw.len() != 16 {
        return None;
    }
    let mut id = [0u8; 16];
    id.copy_from_slice(raw);
    let n = u128::from_be_bytes(id);
    let mut dst = [0u8; 22];
    let mut used = 0usize;

    for shift in [96u32, 64, 32, 0] {
        let mut carry = ((n >> shift) as u32) as u64;
        for slot in &mut dst[..used] {
            carry += u64::from(*slot) << 32;
            *slot = (carry % 62) as u8;
            carry /= 62;
        }
        while carry > 0 && used < dst.len() {
            dst[used] = (carry % 62) as u8;
            carry /= 62;
            used += 1;
        }
    }

    let mut out = String::with_capacity(dst.len());
    for digit in dst.iter().rev().copied() {
        out.push(BASE62_DIGITS[digit as usize] as char);
    }
    Some(out)
}

fn track_gid_from_uri(uri: &str) -> Option<Vec<u8>> {
    let encoded = uri.strip_prefix("spotify:track:")?;
    spotify_id_raw_from_base62(encoded)
}

fn spotify_id_raw_from_base62(encoded: &str) -> Option<Vec<u8>> {
    if encoded.len() != 22 {
        return None;
    }
    let mut value = 0u128;
    for byte in encoded.bytes() {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'z' => byte - b'a' + 10,
            b'A'..=b'Z' => byte - b'A' + 36,
            _ => return None,
        };
        value = value.checked_mul(62)?.checked_add(u128::from(digit))?;
    }
    Some(value.to_be_bytes().to_vec())
}

async fn run_session_probe(credential: SpotifyCredential) -> Option<SpotifyRuntimeSession> {
    crate::log!(
        "spotify-session: probe start user_len={} auth_type={} auth_data_len={}\n",
        credential.username.len(),
        credential.auth_type,
        credential.auth_data.len()
    );

    let resolved = match resolve_spotify_access_points().await {
        Ok(resolved) => resolved,
        Err(err) => {
            crate::log!("spotify-session: apresolve failed err={}\n", err);
            return None;
        }
    };

    let mut accesspoints = endpoints_from_items(resolved.accesspoint.as_slice());
    if accesspoints.is_empty() {
        accesspoints.push(SpotifyEndpoint {
            host: String::from("ap.spotify.com"),
            port: 443,
        });
    }
    let dealer_endpoint = first_endpoint(resolved.dealer.as_slice());
    let spclient_endpoint = first_endpoint(resolved.spclient.as_slice());

    for (idx, accesspoint) in accesspoints.iter().enumerate() {
        crate::log!(
            "spotify-session: selected accesspoint idx={} count={} {}:{} dealer_count={} spclient_count={}\n",
            idx + 1,
            accesspoints.len(),
            accesspoint.host.as_str(),
            accesspoint.port,
            resolved.dealer.len(),
            resolved.spclient.len()
        );
        let (vnet, handle) = match tcp_connect_socket(accesspoint.host.as_str(), accesspoint.port)
            .await
        {
            Ok((vnet, handle)) => (vnet, handle),
            Err(err) => {
                crate::log!(
                    "spotify-session: accesspoint tcp failed idx={} count={} host={} port={} err={}\n",
                    idx + 1,
                    accesspoints.len(),
                    accesspoint.host.as_str(),
                    accesspoint.port,
                    err
                );
                continue;
            }
        };

        crate::log!(
            "spotify-session: accesspoint tcp ok idx={} count={} host={} port={} next=ap-handshake-auth handle={}\n",
            idx + 1,
            accesspoints.len(),
            accesspoint.host.as_str(),
            accesspoint.port,
            handle.0
        );
        match crate::r::spotify_ap::authenticate_session(
            vnet,
            handle,
            &credential,
            crate::r::spotify_discovery::DEVICE_ID,
        )
        .await
        {
            Ok(mut session) => {
                let welcome = session.welcome();
                crate::log!(
                    "spotify-session: ap auth ok handle={} canonical_user_len={} reusable_auth_type={:?} reusable_auth_data_len={} keepalive=armed mercury=ready dealer=needs-spclient-token\n",
                    session.handle_id(),
                    welcome.canonical_username_len,
                    welcome.reusable_auth_type,
                    welcome.reusable_auth_data_len
                );
                let keymaster_scope = KeymasterScope::Primary;
                match session.request_keymaster_token(
                    keymaster_scope.scopes(),
                    crate::r::spotify_discovery::DEVICE_ID,
                ) {
                    Ok(seq) => crate::log!(
                        "spotify-session: mercury keymaster token requested seq={} scopes={}\n",
                        seq,
                        keymaster_scope.scopes()
                    ),
                    Err(err) => crate::log!(
                        "spotify-session: mercury keymaster token request failed err={}\n",
                        err
                    ),
                }
                return Some(SpotifyRuntimeSession {
                    ap: session,
                    dealer_endpoint,
                    spclient_endpoint,
                    dealer: DealerState::WaitingToken,
                    connect_state: ConnectStateProbeState::WaitingDealer,
                    playback_target: None,
                    playback_metadata: PlaybackMetadataProbeState::WaitingTarget,
                    playback_file: None,
                    playback_storage: PlaybackStorageProbeState::WaitingFile,
                    playback_storage_result: None,
                    playback_audio_key: PlaybackAudioKeyProbeState::WaitingInputs,
                    playback_audio_key_bytes: None,
                    playback_cdn: PlaybackCdnProbeState::WaitingInputs,
                    playback_pcm_sink: PlaybackPcmSinkProbeState::WaitingCdn,
                    playback_decoder: PlaybackDecoderProbeState::WaitingPcm,
                    playback_stream: PlaybackStreamProbeState::WaitingDecoder,
                    playback_cdn_url: None,
                    playback_cdn_offset: 0,
                    playback_cdn_warmup: None,
                    playback_vorbis_decoder: None,
                    playback_ogg_stream: SpotifyOggStreamParser::new(),
                    playback_pending_pcm: Vec::new(),
                    playback_vorbis_ident: None,
                    playback_vorbis_packets: None,
                    playback_vorbis_input: None,
                    playback_ogg_audio_packets: 0,
                    playback_ogg_serial: None,
                    pcm_sink: None,
                    connection_id: None,
                    session_id: random_hex_string(16),
                    keymaster_scope,
                });
            }
            Err(err) => {
                crate::log!(
                    "spotify-session: ap auth failed idx={} count={} host={} port={} err={}\n",
                    idx + 1,
                    accesspoints.len(),
                    accesspoint.host.as_str(),
                    accesspoint.port,
                    err
                );
            }
        }
    }

    crate::log!("spotify-session: all accesspoints failed count={}\n", accesspoints.len());
    None
}

fn build_put_connect_state_request(device_id: &str, session_id: &str) -> Vec<u8> {
    build_put_connect_state_request_inner(device_id, session_id, 3, false, None, None)
}

fn build_put_active_connect_state_request(
    device_id: &str,
    session_id: &str,
    target: Option<&SpotifyPlaybackTarget>,
    ack: &DealerTransferAck,
) -> Vec<u8> {
    build_put_connect_state_request_inner(device_id, session_id, 4, true, target, Some(ack))
}

fn build_put_connect_state_request_inner(
    device_id: &str,
    session_id: &str,
    reason: u64,
    active: bool,
    target: Option<&SpotifyPlaybackTarget>,
    ack: Option<&DealerTransferAck>,
) -> Vec<u8> {
    let device_info = build_connect_device_info(device_id);
    let private_device_info = build_private_device_info();
    let player_state = build_connect_player_state(session_id, active, target);

    let mut device = Vec::new();
    pb_message(&mut device, 1, device_info.as_slice());
    pb_message(&mut device, 2, player_state.as_slice());
    pb_message(&mut device, 3, private_device_info.as_slice());

    let mut out = Vec::new();
    pb_message(&mut out, 2, device.as_slice());
    pb_varint(&mut out, 3, 2); // MemberType::CONNECT_STATE
    if active {
        pb_varint(&mut out, 4, 1);
    }
    pb_varint(&mut out, 5, reason);
    pb_varint(&mut out, 6, 1);
    if let Some(ack) = ack {
        pb_string(&mut out, 7, ack.sent_by_device_id.as_str());
        pb_varint(&mut out, 8, ack.message_id as u64);
    }
    if let Some(now) = crate::time::unix_time_seconds() {
        let now_ms = now.saturating_mul(1000);
        if active {
            let position_ms = target
                .and_then(|target| target.position_ms)
                .map(u64::from)
                .unwrap_or(0);
            pb_varint(&mut out, 9, now_ms.saturating_sub(position_ms));
            pb_varint(&mut out, 11, position_ms);
        }
        pb_varint(&mut out, 12, now_ms);
    }
    out
}

fn build_connect_device_info(device_id: &str) -> Vec<u8> {
    let capabilities = build_connect_capabilities();
    let mut out = Vec::new();
    pb_varint(&mut out, 1, 1);
    pb_varint(&mut out, 2, u16::MAX as u64);
    pb_string(&mut out, 3, "TRUEOS Spotify");
    pb_message(&mut out, 4, capabilities.as_slice());
    pb_string(&mut out, 6, "0.8.0-trueos");
    pb_varint(&mut out, 7, 4); // devices.DeviceType::SPEAKER
    pb_string(&mut out, 9, "3.2.6");
    pb_string(&mut out, 10, device_id);
    pb_string(&mut out, 13, SPOTIFY_CLIENT_ID);
    pb_string(&mut out, 14, "TRUEOS");
    pb_string(&mut out, 15, "kernel-service");
    pb_string(&mut out, 17, "0");
    pb_string(&mut out, 18, device_id);
    out
}

fn build_private_device_info() -> Vec<u8> {
    let mut out = Vec::new();
    pb_string(&mut out, 1, "TRUEOS kernel-service");
    out
}

fn build_connect_capabilities() -> Vec<u8> {
    let mut out = Vec::new();
    pb_varint(&mut out, 2, 1);
    pb_varint(&mut out, 5, 1);
    pb_varint(&mut out, 7, 1);
    pb_varint(&mut out, 8, 64);
    pb_string(&mut out, 9, "audio/episode");
    pb_string(&mut out, 9, "audio/track");
    pb_string(&mut out, 9, "audio/local");
    pb_varint(&mut out, 10, 1);
    pb_varint(&mut out, 15, 1);
    pb_varint(&mut out, 16, 1);
    pb_varint(&mut out, 19, 1);
    pb_varint(&mut out, 20, 1);
    pb_varint(&mut out, 22, 1);
    pb_varint(&mut out, 23, 1);
    pb_varint(&mut out, 25, 1);
    pb_varint(&mut out, 30, 4); // common.media.AudioQuality::VERY_HIGH
    out
}

fn build_connect_player_state(
    session_id: &str,
    active: bool,
    target: Option<&SpotifyPlaybackTarget>,
) -> Vec<u8> {
    let mut out = Vec::new();
    let target_context_uri = target.and_then(|target| {
        target
            .context_uri
            .as_deref()
            .or(target.track_uri.as_deref())
    });
    let target_paused = target.and_then(|target| target.paused).unwrap_or(false);
    let target_position_ms = target.and_then(|target| target.position_ms).unwrap_or(0);
    let is_playing = active && !target_paused;
    let is_paused = active && target_paused;
    let now_ms = crate::time::unix_time_seconds().map(|now| now.saturating_mul(1000));

    if let Some(now_ms) = now_ms {
        pb_varint(&mut out, 1, now_ms);
    }
    if let Some(context_uri) = target_context_uri {
        pb_string(&mut out, 2, context_uri);
        pb_string(&mut out, 3, format!("context://{}", context_uri).as_str());
    }
    pb_message(&mut out, 5, &[]);
    if let Some(target) = target {
        if let Some(track_uri) = target.track_uri.as_deref() {
            let track = build_connect_provided_track(track_uri, target);
            pb_message(&mut out, 7, track.as_slice());
        }
    }
    pb_f64(&mut out, 9, if is_playing { 1.0 } else { 0.0 });
    if active {
        if let Some(now_ms) = now_ms {
            pb_varint(&mut out, 10, now_ms);
        }
        pb_varint(&mut out, 11, 0);
    }
    if is_playing {
        pb_varint(&mut out, 12, 1);
    }
    if is_paused {
        pb_varint(&mut out, 13, 1);
    }
    pb_varint(&mut out, 15, 1);
    pb_message(&mut out, 16, &[]);
    pb_message(&mut out, 18, &[]);
    if active {
        pb_varint(&mut out, 25, u64::from(target_position_ms));
    }
    pb_string(&mut out, 23, session_id);
    out
}

fn build_connect_provided_track(track_uri: &str, target: &SpotifyPlaybackTarget) -> Vec<u8> {
    let mut out = Vec::new();
    pb_string(&mut out, 1, track_uri);
    if let Some(uid) = target.track_uid.as_deref() {
        pb_string(&mut out, 2, uid);
    }
    out
}

fn build_extended_metadata_track_request(track_uri: &str) -> Vec<u8> {
    let mut query = Vec::new();
    pb_varint(&mut query, 1, 10); // extendedmetadata.ExtensionKind::TRACK_V4

    let mut entity = Vec::new();
    pb_string(&mut entity, 1, track_uri);
    pb_message(&mut entity, 2, query.as_slice());

    let mut out = Vec::new();
    pb_message(&mut out, 2, entity.as_slice());
    out
}

fn parse_extended_metadata_track_response(data: &[u8]) -> Option<SpotifyTrackMetadataProbe> {
    let mut index = 0usize;
    while let Some((field, wire, value, _varint)) = proto_next_field(data, &mut index) {
        if field == 2 && wire == 2 {
            if let Some(track) = parse_extended_metadata_array(value) {
                return Some(track);
            }
        }
    }
    None
}

fn parse_extended_metadata_array(data: &[u8]) -> Option<SpotifyTrackMetadataProbe> {
    let mut index = 0usize;
    while let Some((field, wire, value, _varint)) = proto_next_field(data, &mut index) {
        if field == 3 && wire == 2 {
            if let Some(track) = parse_extended_metadata_entity(value) {
                return Some(track);
            }
        }
    }
    None
}

fn parse_extended_metadata_entity(data: &[u8]) -> Option<SpotifyTrackMetadataProbe> {
    let any = proto_len(data, 3)?;
    let track = proto_len(any, 2)?;
    parse_track_metadata(track)
}

fn parse_track_metadata(track: &[u8]) -> Option<SpotifyTrackMetadataProbe> {
    let mut files = Vec::new();
    let mut index = 0usize;
    while let Some((field, wire, value, _varint)) = proto_next_field(track, &mut index) {
        if field == 12
            && wire == 2
            && let Some(file) = parse_track_audio_file(value)
        {
            files.push(file);
        }
    }
    Some(SpotifyTrackMetadataProbe {
        track_bytes: track.len(),
        name: proto_string(track, 2),
        duration_ms: proto_varint(track, 7)
            .map(decode_sint32)
            .and_then(|value| u32::try_from(value.max(0)).ok()),
        files,
    })
}

fn parse_track_audio_file(data: &[u8]) -> Option<SpotifyAudioFileCandidate> {
    let file_id_raw = proto_len(data, 1).map(|file_id| file_id.to_vec());
    let file_id_hex = file_id_raw.as_deref().map(hex_string);
    let format = proto_varint(data, 2)?;
    Some(SpotifyAudioFileCandidate {
        format,
        file_id_raw,
        file_id_hex,
    })
}

fn select_audio_file(files: &[SpotifyAudioFileCandidate]) -> Option<SpotifyAudioFileCandidate> {
    const PREFERRED: &[u64] = &[1, 2, 0, 5, 6, 3, 4];
    for wanted in PREFERRED {
        if let Some(file) = files.iter().find(|file| file.format == *wanted) {
            return Some(file.clone());
        }
    }
    files.first().cloned()
}

fn audio_format_label(format: u64) -> &'static str {
    match format {
        0 => "ogg-vorbis-96",
        1 => "ogg-vorbis-160",
        2 => "ogg-vorbis-320",
        3 => "mp3-256",
        4 => "mp3-320",
        5 => "mp3-160",
        6 => "mp3-96",
        7 => "mp3-160-enc",
        8 => "aac-24",
        9 => "aac-48",
        16 => "flac",
        18 => "xhe-aac-24",
        19 => "xhe-aac-16",
        20 => "xhe-aac-12",
        22 => "flac-24",
        _ => "unknown",
    }
}

fn parse_storage_resolve_response(data: &[u8]) -> Option<SpotifyStorageResolveProbe> {
    let mut result = None;
    let mut urls = Vec::new();
    let mut file_id_hex = None;
    let mut index = 0usize;
    while let Some((field, wire, value, varint)) = proto_next_field(data, &mut index) {
        match (field, wire) {
            (1, 0) => result = Some(varint),
            (2, 2) => {
                if let Ok(url) = core::str::from_utf8(value) {
                    urls.push(String::from(url));
                }
            }
            (4, 2) => file_id_hex = Some(hex_string(value)),
            _ => {}
        }
    }
    Some(SpotifyStorageResolveProbe {
        result: result?,
        urls,
        file_id_hex,
    })
}

fn decrypt_spotify_audio_range(key: &[u8; 16], offset: usize, encrypted: &[u8]) -> Option<Vec<u8>> {
    let mut cipher = SpotifyAudioCtr::new_from_slices(key, &AUDIO_AESIV).ok()?;
    cipher.seek(offset as u64);
    let mut out = encrypted.to_vec();
    cipher.apply_keystream(out.as_mut_slice());
    Some(out)
}

fn inspect_ogg_vorbis(data: &[u8]) -> SpotifyOggProbe {
    let mut probe = SpotifyOggProbe {
        decoder_offset_page: data
            .get(SPOTIFY_OGG_HEADER_END..SPOTIFY_OGG_HEADER_END.saturating_add(4))
            == Some(b"OggS"),
        ..SpotifyOggProbe::default()
    };
    let mut pos = find_ogg_page(data).unwrap_or(data.len());
    let mut packet = Vec::new();
    let mut packet_start = None;

    while pos.saturating_add(27) <= data.len() {
        if data.get(pos..pos.saturating_add(4)) != Some(b"OggS") {
            let Some(next) = data[pos.saturating_add(1)..]
                .windows(4)
                .position(|window| window == b"OggS")
            else {
                break;
            };
            pos = pos.saturating_add(1).saturating_add(next);
            continue;
        }

        let page_segments = data[pos + 26] as usize;
        let lacing_start = pos.saturating_add(27);
        let payload_start = lacing_start.saturating_add(page_segments);
        if payload_start > data.len() {
            break;
        }

        let payload_len: usize = data[lacing_start..payload_start]
            .iter()
            .map(|value| *value as usize)
            .sum();
        let payload_end = payload_start.saturating_add(payload_len);
        if payload_end > data.len() {
            break;
        }

        if probe.first_page_offset.is_none() {
            probe.first_page_offset = Some(pos);
            probe.serial = Some(u32::from_le_bytes([
                data[pos + 14],
                data[pos + 15],
                data[pos + 16],
                data[pos + 17],
            ]));
        }
        probe.pages = probe.pages.saturating_add(1);

        let mut payload_cursor = payload_start;
        for segment in &data[lacing_start..payload_start] {
            let segment_len = *segment as usize;
            let segment_end = payload_cursor.saturating_add(segment_len);
            if segment_end > payload_end {
                return probe;
            }
            if packet.is_empty() {
                packet_start = Some(payload_cursor);
            }
            packet.extend_from_slice(&data[payload_cursor..segment_end]);
            payload_cursor = segment_end;

            if *segment < 255 {
                probe.packets = probe.packets.saturating_add(1);
                inspect_vorbis_packet(&mut probe, packet.as_slice(), packet_start);
                packet.clear();
                packet_start = None;
            }
        }

        pos = payload_end;
    }

    probe
}

fn inspect_vorbis_packet(probe: &mut SpotifyOggProbe, packet: &[u8], packet_start: Option<usize>) {
    if packet.first() == Some(&0) {
        probe.audio_packets = probe.audio_packets.saturating_add(1);
        if probe.first_audio_offset.is_none() {
            probe.first_audio_offset = packet_start;
        }
        capture_vorbis_audio_packet(probe, packet);
        return;
    }
    if packet.len() < 7 || packet.get(1..7) != Some(b"vorbis") {
        return;
    }
    if probe.first_vorbis_offset.is_none() {
        probe.first_vorbis_offset = packet_start;
    }
    match packet[0] {
        1 => {
            probe.vorbis_ident = true;
            probe.ident = parse_vorbis_ident_packet(packet);
            capture_vorbis_header_packet(probe, 1, packet);
        }
        3 => {
            probe.vorbis_comment = true;
            capture_vorbis_header_packet(probe, 3, packet);
        }
        5 => {
            probe.vorbis_setup = true;
            capture_vorbis_header_packet(probe, 5, packet);
        }
        _ => {}
    }
}

fn is_vorbis_header_packet(packet: &[u8]) -> bool {
    packet.len() >= 7
        && packet.get(1..7) == Some(b"vorbis")
        && matches!(packet.first(), Some(1 | 3 | 5))
}

fn vorbis_packet_capture(probe: &mut SpotifyOggProbe) -> &mut VorbisPacketCapture {
    probe
        .capture
        .get_or_insert_with(VorbisPacketCapture::default)
}

fn capture_vorbis_header_packet(probe: &mut SpotifyOggProbe, packet_type: u8, packet: &[u8]) {
    let capture = vorbis_packet_capture(probe);
    match packet_type {
        1 if capture.ident.is_empty() => capture.ident.extend_from_slice(packet),
        3 if capture.comment.is_empty() => capture.comment.extend_from_slice(packet),
        5 if capture.setup.is_empty() => capture.setup.extend_from_slice(packet),
        _ => {}
    }
}

fn capture_vorbis_audio_packet(probe: &mut SpotifyOggProbe, packet: &[u8]) {
    let capture = vorbis_packet_capture(probe);
    capture.audio_bytes = capture.audio_bytes.saturating_add(packet.len());
    if capture.audio_packets.len() < CDN_WARMUP_AUDIO_PACKETS {
        capture.audio_packets.push(packet.to_vec());
    }
}

fn prepare_vorbis_decoder_input(
    ident: Option<VorbisIdentProbe>,
    capture: Option<&VorbisPacketCapture>,
) -> Option<crate::r::spotify_vorbis::PreparedVorbisDecoderInput> {
    let ident = ident.filter(|ident| ident.valid)?;
    let capture = capture?;
    crate::r::spotify_vorbis::PreparedVorbisDecoderInput::new(
        capture.ident.as_slice(),
        capture.comment.as_slice(),
        capture.setup.as_slice(),
        capture.audio_packets.as_slice(),
        usize::from(ident.channels),
        ident.sample_rate,
        crate::hda::PCM_CHANNELS,
        crate::hda::PCM_SAMPLE_RATE_HZ,
    )
    .ok()
}

fn parse_vorbis_ident_packet(packet: &[u8]) -> Option<VorbisIdentProbe> {
    if packet.len() < 30 || packet.first() != Some(&1) || packet.get(1..7) != Some(b"vorbis") {
        return None;
    }
    let version = u32::from_le_bytes([packet[7], packet[8], packet[9], packet[10]]);
    let channels = packet[11];
    let sample_rate = u32::from_le_bytes([packet[12], packet[13], packet[14], packet[15]]);
    let bitrate_maximum = i32::from_le_bytes([packet[16], packet[17], packet[18], packet[19]]);
    let bitrate_nominal = i32::from_le_bytes([packet[20], packet[21], packet[22], packet[23]]);
    let bitrate_minimum = i32::from_le_bytes([packet[24], packet[25], packet[26], packet[27]]);
    let blocksize_0_exp = packet[28] & 0x0f;
    let blocksize_1_exp = packet[28] >> 4;
    let framing = packet[29] & 1 != 0;
    let valid = version == 0
        && channels != 0
        && sample_rate != 0
        && (6..=13).contains(&blocksize_0_exp)
        && (6..=13).contains(&blocksize_1_exp)
        && blocksize_0_exp <= blocksize_1_exp
        && framing;

    Some(VorbisIdentProbe {
        channels,
        sample_rate,
        bitrate_maximum,
        bitrate_nominal,
        bitrate_minimum,
        blocksize_0_exp,
        blocksize_1_exp,
        framing,
        valid,
    })
}

fn vorbis_resampler_label(ident: Option<VorbisIdentProbe>) -> &'static str {
    let Some(ident) = ident else {
        return "unknown";
    };
    if usize::from(ident.channels) != crate::hda::PCM_CHANNELS {
        "channel-map-needed"
    } else if ident.sample_rate != crate::hda::PCM_SAMPLE_RATE_HZ {
        "needed"
    } else {
        "not-needed"
    }
}

fn find_ogg_page(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|window| window == b"OggS")
}

fn hex_preview(data: &[u8], max: usize) -> String {
    hex_string(&data[..data.len().min(max)])
}

fn decode_sint32(value: u64) -> i32 {
    ((value >> 1) as i32) ^ (-((value & 1) as i32))
}

fn random_hex_string(bytes_len: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = Vec::new();
    bytes.resize(bytes_len, 0);
    if !crate::tyche::fill_bytes(bytes.as_mut_slice()) {
        for (idx, byte) in bytes.iter_mut().enumerate() {
            *byte = (crate::time::uptime_seconds() as u8).wrapping_add(idx as u8);
        }
    }
    let mut out = String::new();
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
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

fn pb_fixed64(out: &mut Vec<u8>, field: u32, value: u64) {
    write_varint(out, ((field as u64) << 3) | 1);
    out.extend_from_slice(&value.to_le_bytes());
}

fn pb_f64(out: &mut Vec<u8>, field: u32, value: f64) {
    pb_fixed64(out, field, value.to_bits());
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

async fn resolve_spotify_access_points() -> Result<ApResolveData, String> {
    const URL: &str = "https://apresolve.spotify.com/?type=accesspoint&type=dealer&type=spclient";
    crate::log!("spotify-session: apresolve begin url={}\n", URL);
    let body = crate::r::net::https::get_bytes_shared(URL, 15_000, 16 * 1024).await?;
    let resolved: ApResolveData =
        serde_json::from_slice(body.as_slice()).map_err(|err| alloc::format!("json {}", err))?;
    crate::log!(
        "spotify-session: apresolve ok accesspoint_count={} dealer_count={} spclient_count={}\n",
        resolved.accesspoint.len(),
        resolved.dealer.len(),
        resolved.spclient.len()
    );
    Ok(resolved)
}

fn first_endpoint(items: &[String]) -> Option<SpotifyEndpoint> {
    for item in items {
        if let Some(endpoint) = parse_endpoint(item.as_str()) {
            return Some(endpoint);
        }
    }
    None
}

fn endpoints_from_items(items: &[String]) -> Vec<SpotifyEndpoint> {
    let mut out = Vec::new();
    for item in items {
        if let Some(endpoint) = parse_endpoint(item.as_str()) {
            out.push(endpoint);
        }
    }
    out
}

fn parse_endpoint(item: &str) -> Option<SpotifyEndpoint> {
    let (host, port) = item.rsplit_once(':')?;
    let port = port.parse::<u16>().ok()?;
    if host.is_empty() || port == 0 {
        return None;
    }
    Some(SpotifyEndpoint {
        host: String::from(host),
        port,
    })
}

async fn tcp_connect_socket(host: &str, port: u16) -> Result<(VNet, api::NetHandle), String> {
    let device_index = NetProfile::default()
        .resolve_device_index()
        .ok_or_else(|| String::from("no nic"))?;
    let ip = crate::r::net::dns::resolve_ipv4_for_device(
        device_index,
        host,
        crate::r::net::dns::DnsConfig::default().with_timeout_ms(10_000),
    )
    .await
    .map_err(|err| alloc::format!("dns {:?}", err))?;

    crate::log!(
        "spotify-session: accesspoint dns host={} ip={}.{}.{}.{} port={}\n",
        host,
        ip[0],
        ip[1],
        ip[2],
        ip[3],
        port
    );

    let vnet = VNet::open(device_index).ok_or_else(|| String::from("vnet open"))?;
    vnet.submit(api::Command::OpenTcpConnect {
        remote: api::EndpointV4 { addr: ip, port },
    })
    .map_err(|_| String::from("tcp submit"))?;

    let deadline = Instant::now() + EmbassyDuration::from_millis(10_000);
    let mut tcp_handle = None;
    loop {
        while let Some(ev) = vnet.pop_event() {
            match ev {
                api::Event::Opened {
                    handle,
                    kind: api::SocketKind::Tcp,
                } => tcp_handle = Some(handle),
                api::Event::TcpEstablished { handle, .. } if tcp_handle == Some(handle) => {
                    return Ok((vnet, handle));
                }
                api::Event::Error { msg } => return Err(String::from(msg)),
                api::Event::Closed { handle } if tcp_handle == Some(handle) => {
                    return Err(String::from("closed"));
                }
                _ => {}
            }
        }

        if Instant::now() >= deadline {
            if let Some(handle) = tcp_handle {
                let _ = vnet.submit(api::Command::Close { handle });
            }
            return Err(String::from("timeout"));
        }

        embassy_time::Timer::after(EmbassyDuration::from_millis(10)).await;
    }
}
