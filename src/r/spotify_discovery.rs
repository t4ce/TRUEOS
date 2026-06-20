extern crate alloc;

use alloc::{format, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Timer};
use v::{vhttp_srv, vnet as api};

use crate::r::net::VNet;
use crate::r::spotify_zeroconf::SpotifyZeroconf;

const MDNS_PORT: u16 = crate::allports::well_known::MDNS;
const HTTP_PORT: u16 = crate::allports::services::SPOTIFY_CONNECT_HTTP_TCP_PORT;
const MAX_REQUEST_BYTES: usize = 16 * 1024;
const POLL_IDLE_MS: u64 = 10;
const DEVICE_NAME: &str = "TRUEOS Spotify";
pub(crate) const DEVICE_ID: &str = "5d78e64b5dc8dd4a7a0e7a9fdac5e93b2d615c79";
const CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const MDNS_MCAST_V4: [u8; 4] = [224, 0, 0, 251];
const DNS_T_PTR: u16 = 12;
const DNS_T_TXT: u16 = 16;
const DNS_T_A: u16 = 1;
const DNS_T_SRV: u16 = 33;
const DNS_CLASS_IN: u16 = 1;
const DNS_CLASS_IN_FLUSH: u16 = 0x8001;
const DNS_TTL_SECONDS: u32 = 120;
const QNAME_SERVICES: &[u8] = b"\x09_services\x07_dns-sd\x04_udp\x05local\x00";
const QNAME_SPOTIFY_CONNECT: &[u8] = b"\x10_spotify-connect\x04_tcp\x05local\x00";
const QNAME_INSTANCE: &[u8] = b"\x0eTRUEOS Spotify\x10_spotify-connect\x04_tcp\x05local\x00";
const QNAME_HOST: &[u8] = b"\x0etrueos-spotify\x05local\x00";

struct SpotifyDiscoveryEndpoint {
    vnet: VNet,
    server: vhttp_srv::HttpServer,
    dev_idx: usize,
    ipv4: Option<[u8; 4]>,
    tcp_ready: bool,
    udp_handle: Option<api::NetHandle>,
    udp_packets: u64,
    zeroconf: SpotifyZeroconf,
}

pub struct SpotifyDiscoveryService {
    endpoints: Vec<SpotifyDiscoveryEndpoint>,
    discovery_ticks: u32,
}

struct HttpResponsePlan {
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

impl SpotifyDiscoveryService {
    pub fn new() -> Self {
        Self {
            endpoints: Vec::new(),
            discovery_ticks: 0,
        }
    }

    pub fn endpoint_count(&self) -> usize {
        self.endpoints.len()
    }

    pub fn add_endpoints(&mut self) -> usize {
        let mut added = 0usize;
        for dev_idx in 0..crate::net::device_count() {
            if self
                .endpoints
                .iter()
                .any(|endpoint| endpoint.dev_idx == dev_idx)
            {
                continue;
            }
            if let Some(endpoint) = open_endpoint(dev_idx) {
                self.endpoints.push(endpoint);
                added = added.saturating_add(1);
            }
        }
        added
    }

    pub async fn tick(&mut self) {
        if self.discovery_ticks == 0 {
            self.add_endpoints();
        }
        self.discovery_ticks = (self.discovery_ticks + 1) % 100;

        for endpoint in self.endpoints.iter_mut() {
            poll_endpoint(endpoint);
        }

        Timer::after(EmbassyDuration::from_millis(POLL_IDLE_MS)).await;
    }
}

fn open_endpoint(dev_idx: usize) -> Option<SpotifyDiscoveryEndpoint> {
    let usable = crate::net::adapter::ipv4_at(dev_idx).is_some()
        || crate::net::link_state_at(dev_idx)
            .map(|state| state.up)
            .unwrap_or(false);
    if !usable {
        return None;
    }

    let vnet = VNet::open(dev_idx)?;
    if vnet
        .submit(api::Command::OpenTcpListen { port: HTTP_PORT })
        .is_err()
    {
        crate::log!(
            "spotify-discovery: tcp listen submit failed dev={} owner={}\n",
            dev_idx,
            vnet.owner()
        );
        return None;
    }

    if vnet
        .submit(api::Command::OpenUdp { port: MDNS_PORT })
        .is_err()
    {
        crate::log!(
            "spotify-discovery: mdns udp submit failed dev={} owner={}\n",
            dev_idx,
            vnet.owner()
        );
    }

    let ip = crate::net::adapter::ipv4_at(dev_idx);
    let name = crate::net::device_name_at(dev_idx).unwrap_or("?");
    match ip {
        Some([a, b, c, d]) => crate::log!(
            "spotify-discovery: submitted http={} mdns_udp={} owner={} dev={} {} ip={}.{}.{}.{}\n",
            HTTP_PORT,
            MDNS_PORT,
            vnet.owner(),
            dev_idx,
            name,
            a,
            b,
            c,
            d
        ),
        None => crate::log!(
            "spotify-discovery: submitted http={} mdns_udp={} owner={} dev={} {} ip=none\n",
            HTTP_PORT,
            MDNS_PORT,
            vnet.owner(),
            dev_idx,
            name
        ),
    }

    Some(SpotifyDiscoveryEndpoint {
        vnet,
        server: vhttp_srv::HttpServer::new(HTTP_PORT, MAX_REQUEST_BYTES),
        dev_idx,
        ipv4: ip,
        tcp_ready: false,
        udp_handle: None,
        udp_packets: 0,
        zeroconf: SpotifyZeroconf::new(),
    })
}

fn poll_endpoint(endpoint: &mut SpotifyDiscoveryEndpoint) {
    while let Some(ev) = endpoint.vnet.pop_event() {
        match ev {
            api::Event::Opened {
                handle,
                kind: api::SocketKind::Udp,
            } => {
                endpoint.udp_handle = Some(handle);
                crate::log!(
                    "spotify-discovery: mdns udp opened dev={} port={} handle={} multicast=224.0.0.251\n",
                    endpoint.dev_idx,
                    MDNS_PORT,
                    handle.0
                );
            }
            api::Event::UdpPacket { handle, from, data } if endpoint.udp_handle == Some(handle) => {
                endpoint.udp_packets = endpoint.udp_packets.saturating_add(1);
                let query = mdns_query_kind(data.as_slice());
                if endpoint.udp_packets == 1 || endpoint.udp_packets % 64 == 0 {
                    crate::log!(
                        "spotify-discovery: mdns udp packet count={} dev={} from={:?} bytes={} query={}\n",
                        endpoint.udp_packets,
                        endpoint.dev_idx,
                        from,
                        data.as_slice().len(),
                        query.label()
                    );
                }
                if let Some(reply) = build_mdns_response(data.as_slice(), query, endpoint.ipv4) {
                    let target =
                        if from.addr == [0, 0, 0, 0] || from.port == 0 || from.port == MDNS_PORT {
                            api::EndpointV4::new(MDNS_MCAST_V4, MDNS_PORT)
                        } else {
                            from
                        };
                    let _ = endpoint.vnet.submit(api::Command::SendUdp {
                        handle,
                        remote: target,
                        data: api::ByteBuf::from_slice_trunc(reply.as_slice()),
                    });
                    crate::log!(
                        "spotify-discovery: mdns response sent dev={} query={} to={:?} bytes={}\n",
                        endpoint.dev_idx,
                        query.label(),
                        target,
                        reply.len()
                    );
                }
            }
            other => handle_http_event(endpoint, other),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MdnsQueryKind {
    None,
    Services,
    SpotifyConnect,
    Host,
}

impl MdnsQueryKind {
    const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Services => "services",
            Self::SpotifyConnect => "spotify-connect",
            Self::Host => "host",
        }
    }
}

fn mdns_query_kind(packet: &[u8]) -> MdnsQueryKind {
    if packet.len() < 12 {
        return MdnsQueryKind::None;
    }

    if packet
        .windows(QNAME_SERVICES.len())
        .any(|w| w == QNAME_SERVICES)
    {
        return MdnsQueryKind::Services;
    }
    if packet
        .windows(QNAME_SPOTIFY_CONNECT.len())
        .any(|w| w == QNAME_SPOTIFY_CONNECT)
    {
        return MdnsQueryKind::SpotifyConnect;
    }
    if packet.windows(QNAME_HOST.len()).any(|w| w == QNAME_HOST) {
        return MdnsQueryKind::Host;
    }

    MdnsQueryKind::None
}

fn build_mdns_response(
    query: &[u8],
    kind: MdnsQueryKind,
    ipv4: Option<[u8; 4]>,
) -> Option<Vec<u8>> {
    if kind == MdnsQueryKind::None {
        return None;
    }

    let mut out = Vec::new();
    out.extend_from_slice(query.get(0..2).unwrap_or(&[0, 0]));
    out.extend_from_slice(&0x8400u16.to_be_bytes()); // response + authoritative answer
    out.extend_from_slice(&0u16.to_be_bytes()); // questions
    let ancount_pos = out.len();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // authority
    out.extend_from_slice(&0u16.to_be_bytes()); // additional

    let mut answers = 0u16;
    match kind {
        MdnsQueryKind::Services => {
            push_ptr_record(&mut out, QNAME_SERVICES, QNAME_SPOTIFY_CONNECT, DNS_TTL_SECONDS)?;
            answers = answers.saturating_add(1);
        }
        MdnsQueryKind::SpotifyConnect => {
            push_ptr_record(&mut out, QNAME_SPOTIFY_CONNECT, QNAME_INSTANCE, DNS_TTL_SECONDS)?;
            answers = answers.saturating_add(1);
            push_srv_record(&mut out, QNAME_INSTANCE, HTTP_PORT, QNAME_HOST, DNS_TTL_SECONDS)?;
            answers = answers.saturating_add(1);
            push_txt_record(
                &mut out,
                QNAME_INSTANCE,
                &[b"VERSION=1.0", b"CPath=/"],
                DNS_TTL_SECONDS,
            )?;
            answers = answers.saturating_add(1);
            if let Some(ipv4) = ipv4 {
                push_a_record(&mut out, QNAME_HOST, ipv4, DNS_TTL_SECONDS)?;
                answers = answers.saturating_add(1);
            }
        }
        MdnsQueryKind::Host => {
            let ipv4 = ipv4?;
            push_a_record(&mut out, QNAME_HOST, ipv4, DNS_TTL_SECONDS)?;
            answers = answers.saturating_add(1);
        }
        MdnsQueryKind::None => return None,
    }

    out[ancount_pos..ancount_pos + 2].copy_from_slice(&answers.to_be_bytes());
    Some(out)
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_dns_record_head(
    out: &mut Vec<u8>,
    name: &[u8],
    record_type: u16,
    class: u16,
    ttl: u32,
    rdlen: u16,
) -> Option<()> {
    out.extend_from_slice(name);
    push_u16(out, record_type);
    push_u16(out, class);
    push_u32(out, ttl);
    push_u16(out, rdlen);
    Some(())
}

fn push_ptr_record(out: &mut Vec<u8>, name: &[u8], target: &[u8], ttl: u32) -> Option<()> {
    push_dns_record_head(out, name, DNS_T_PTR, DNS_CLASS_IN, ttl, target.len() as u16)?;
    out.extend_from_slice(target);
    Some(())
}

fn push_srv_record(
    out: &mut Vec<u8>,
    name: &[u8],
    port: u16,
    target: &[u8],
    ttl: u32,
) -> Option<()> {
    let rdlen = 6usize.saturating_add(target.len());
    push_dns_record_head(out, name, DNS_T_SRV, DNS_CLASS_IN_FLUSH, ttl, rdlen as u16)?;
    push_u16(out, 0); // priority
    push_u16(out, 0); // weight
    push_u16(out, port);
    out.extend_from_slice(target);
    Some(())
}

fn push_txt_record(out: &mut Vec<u8>, name: &[u8], txt: &[&[u8]], ttl: u32) -> Option<()> {
    let mut rdlen = 0usize;
    for item in txt {
        if item.len() > u8::MAX as usize {
            return None;
        }
        rdlen = rdlen.saturating_add(1).saturating_add(item.len());
    }
    push_dns_record_head(out, name, DNS_T_TXT, DNS_CLASS_IN_FLUSH, ttl, rdlen as u16)?;
    for item in txt {
        out.push(item.len() as u8);
        out.extend_from_slice(item);
    }
    Some(())
}

fn push_a_record(out: &mut Vec<u8>, name: &[u8], addr: [u8; 4], ttl: u32) -> Option<()> {
    push_dns_record_head(out, name, DNS_T_A, DNS_CLASS_IN_FLUSH, ttl, 4)?;
    out.extend_from_slice(&addr);
    Some(())
}

fn handle_http_event(endpoint: &mut SpotifyDiscoveryEndpoint, ev: api::Event) {
    if !endpoint.tcp_ready
        && let api::Event::Opened {
            kind: api::SocketKind::Tcp,
            ..
        } = ev
    {
        endpoint.tcp_ready = true;
        crate::log!(
            "spotify-discovery: http tcp listen opened dev={} port={}\n",
            endpoint.dev_idx,
            HTTP_PORT
        );
    }

    match endpoint.server.on_event(ev) {
        vhttp_srv::HttpServerEvent::None => {}
        vhttp_srv::HttpServerEvent::Submit(cmd) => {
            let _ = endpoint.vnet.submit(cmd);
        }
        vhttp_srv::HttpServerEvent::Error(msg) => {
            crate::log!("spotify-discovery: http error {}\n", msg);
        }
        vhttp_srv::HttpServerEvent::RequestTooLarge { handle } => {
            submit_response(
                endpoint,
                handle,
                false,
                HttpResponsePlan {
                    status: "HTTP/1.1 413 Payload Too Large\r\n",
                    content_type: "application/json",
                    body: br#"{"status":102,"spotifyError":1,"statusString":"ERROR-REQUEST-TOO-LARGE"}"#.to_vec(),
                },
            );
        }
        vhttp_srv::HttpServerEvent::BadRequest { handle } => {
            submit_response(
                endpoint,
                handle,
                false,
                HttpResponsePlan {
                    status: "HTTP/1.1 400 Bad Request\r\n",
                    content_type: "application/json",
                    body: br#"{"status":102,"spotifyError":1,"statusString":"ERROR-BAD-REQUEST"}"#
                        .to_vec(),
                },
            );
        }
        vhttp_srv::HttpServerEvent::RequestReady { handle, request } => {
            let action = request_action(request.target(), request.body_bytes()).unwrap_or("-");
            crate::log!(
                "spotify-discovery: http request method={} target={} action={} body_bytes={}\n",
                request.method(),
                request.target(),
                action,
                request.body_bytes().len()
            );
            if request.method() == "POST" {
                log_post_probe(request.body_bytes());
            }
            let keep_alive = request.keep_alive();
            let response = route_request(
                &mut endpoint.zeroconf,
                request.method(),
                request.target(),
                request.body_bytes(),
            );
            submit_response(endpoint, handle, keep_alive, response);
        }
    }
}

fn submit_response(
    endpoint: &mut SpotifyDiscoveryEndpoint,
    handle: api::NetHandle,
    keep_alive: bool,
    response: HttpResponsePlan,
) {
    let mut cmds = Vec::new();
    let pending = vhttp_srv::queue_response_head(
        &mut cmds,
        handle,
        response.status,
        response.content_type,
        "Cache-Control: no-store\r\n",
        response.body.len() as u64,
        keep_alive,
    );
    endpoint.server.mark_response(handle, pending, keep_alive);
    vhttp_srv::queue_send_bytes(&mut cmds, handle, response.body.as_slice());
    for cmd in cmds {
        let _ = endpoint.vnet.submit(cmd);
    }
}

fn route_request(
    zeroconf: &mut SpotifyZeroconf,
    method: &str,
    target: &str,
    body: &[u8],
) -> HttpResponsePlan {
    match (method, request_action(target, body)) {
        ("GET", Some("getInfo")) => get_info_response(zeroconf),
        ("POST", Some("addUser")) => {
            let Some(form) = core::str::from_utf8(body).ok() else {
                return json_response(
                    "HTTP/1.1 400 Bad Request\r\n",
                    br#"{"status":102,"spotifyError":1,"statusString":"ERROR-BAD-REQUEST"}"#
                        .to_vec(),
                );
            };
            match zeroconf.add_user(form) {
                Ok(result) => {
                    crate::r::spotify_service::submit_zeroconf_credential(result.credential);
                    crate::log!(
                        "spotify-discovery: addUser ok user_len={} encrypted_blob_len={} decrypted_blob_len={} state=credentials-ready\n",
                        result.username_len,
                        result.encrypted_len,
                        result.decrypted_len
                    );
                    add_user_ok_response()
                }
                Err(err) => {
                    crate::log!("spotify-discovery: addUser failed err={}\n", err);
                    add_user_error_response(err)
                }
            }
        }
        ("GET", _) if vhttp_srv::path_only(target) == "/healthz" => json_response(
            "HTTP/1.1 200 OK\r\n",
            br#"{"ok":true,"service":"spotify-discovery","librespot":"not-linked"}"#.to_vec(),
        ),
        _ => json_response(
            "HTTP/1.1 404 Not Found\r\n",
            br#"{"status":102,"spotifyError":1,"statusString":"ERROR-NOT-FOUND"}"#.to_vec(),
        ),
    }
}

fn log_post_probe(body: &[u8]) {
    let Some(form) = core::str::from_utf8(body).ok() else {
        crate::log!("spotify-discovery: post probe form=utf8-error bytes={}\n", body.len());
        return;
    };

    let mut field_count = 0usize;
    let mut action = "-";
    let mut username_len = 0usize;
    let mut blob_len = 0usize;
    let mut client_key_len = 0usize;
    let mut has_username = false;
    let mut has_blob = false;
    let mut has_client_key = false;

    for part in form.split('&') {
        if part.is_empty() {
            continue;
        }
        field_count = field_count.saturating_add(1);
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "action" => action = value,
            "userName" => {
                has_username = true;
                username_len = value.len();
            }
            "blob" => {
                has_blob = true;
                blob_len = value.len();
            }
            "clientKey" => {
                has_client_key = true;
                client_key_len = value.len();
            }
            _ => {}
        }
    }

    crate::log!(
        "spotify-discovery: post probe action={} fields={} user={} user_len={} blob={} blob_len={} client_key={} client_key_len={}\n",
        action,
        field_count,
        has_username as u8,
        username_len,
        has_blob as u8,
        blob_len,
        has_client_key as u8,
        client_key_len
    );
}

fn request_action<'a>(target: &'a str, body: &'a [u8]) -> Option<&'a str> {
    if let Some(action) = vhttp_srv::query_param(target, "action") {
        return Some(action);
    }

    let body = core::str::from_utf8(body).ok()?;
    for part in body.split('&') {
        if let Some((key, value)) = part.split_once('=')
            && key == "action"
        {
            return Some(value);
        }
    }
    None
}

fn json_response(status: &'static str, body: Vec<u8>) -> HttpResponsePlan {
    HttpResponsePlan {
        status,
        content_type: "application/json",
        body,
    }
}

fn get_info_response(zeroconf: &SpotifyZeroconf) -> HttpResponsePlan {
    let body = format!(
        "{{\"status\":101,\"statusString\":\"OK\",\"spotifyError\":0,\"version\":\"2.9.0\",\"deviceID\":\"{}\",\"deviceType\":\"speaker\",\"remoteName\":\"{}\",\"publicKey\":\"{}\",\"brandDisplayName\":\"TRUEOS\",\"modelDisplayName\":\"kernel-service\",\"libraryVersion\":\"kernel-probe\",\"resolverVersion\":\"1\",\"groupStatus\":\"NONE\",\"tokenType\":\"default\",\"clientID\":\"{}\",\"productID\":0,\"scope\":\"streaming\",\"availability\":\"\",\"supported_drm_media_formats\":[],\"supported_capabilities\":1,\"accountReq\":\"PREMIUM\",\"activeUser\":\"{}\",\"aliases\":[]}}",
        DEVICE_ID,
        DEVICE_NAME,
        zeroconf.public_key_b64(),
        CLIENT_ID,
        zeroconf.active_user()
    );
    json_response("HTTP/1.1 200 OK\r\n", body.into_bytes())
}

fn add_user_ok_response() -> HttpResponsePlan {
    json_response(
        "HTTP/1.1 200 OK\r\n",
        br#"{"status":101,"spotifyError":0,"statusString":"OK"}"#.to_vec(),
    )
}

fn add_user_error_response(err: &str) -> HttpResponsePlan {
    let body = format!(
        "{{\"status\":102,\"spotifyError\":1,\"statusString\":\"ERROR-ZEROCONF-AUTH\",\"error\":\"{}\"}}",
        err
    );
    json_response("HTTP/1.1 400 Bad Request\r\n", body.into_bytes())
}
