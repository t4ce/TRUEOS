extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

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
    loop {
        discovery.tick().await;
        if let Some(credential) = take_pending_credential() {
            run_session_probe(credential).await;
        }
        if Instant::now() >= next_heartbeat {
            crate::log!(
                "spotify-service: idle epoch={} slot={} ready=0x{:08X} discovery_endpoints={}\n",
                epoch,
                slot,
                crate::r::readiness::mask(),
                discovery.endpoint_count()
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

async fn run_session_probe(credential: SpotifyCredential) {
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
            return;
        }
    };

    let Some(accesspoint) = first_endpoint(resolved.accesspoint.as_slice()).or_else(|| {
        Some(SpotifyEndpoint {
            host: String::from("ap.spotify.com"),
            port: 443,
        })
    }) else {
        crate::log!("spotify-session: no accesspoint endpoint\n");
        return;
    };

    crate::log!(
        "spotify-session: selected accesspoint {}:{} dealer_count={} spclient_count={}\n",
        accesspoint.host.as_str(),
        accesspoint.port,
        resolved.dealer.len(),
        resolved.spclient.len()
    );

    match tcp_connect_probe(accesspoint.host.as_str(), accesspoint.port).await {
        Ok(()) => crate::log!(
            "spotify-session: accesspoint tcp ok host={} port={} next=ap-handshake-auth\n",
            accesspoint.host.as_str(),
            accesspoint.port
        ),
        Err(err) => crate::log!(
            "spotify-session: accesspoint tcp failed host={} port={} err={}\n",
            accesspoint.host.as_str(),
            accesspoint.port,
            err
        ),
    }
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

async fn tcp_connect_probe(host: &str, port: u16) -> Result<(), String> {
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
                    let _ = vnet.submit(api::Command::Close { handle });
                    return Ok(());
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
