//! BSP-resident IPP Everywhere printer discovery and registry.

extern crate alloc;

use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::fmt::Write as _;

use embassy_time::{Duration, Instant, Timer};
use spin::Mutex;

use super::VNet;

const MDNS_PORT: u16 = 5_353;
const MDNS_MULTICAST: v::vnet::EndpointV4 = v::vnet::EndpointV4::new([224, 0, 0, 251], MDNS_PORT);
const DISCOVERY_INTERVAL_MS: u64 = 15_000;
const PRINTER_STALE_AFTER_MS: u64 = DISCOVERY_INTERVAL_MS * 3;
const SERVICES: [&str; 3] = [
    "_ipp._tcp.local.",
    "_print._sub._ipp._tcp.local.",
    "_ipps._tcp.local.",
];

#[derive(Clone, Debug, Default)]
struct PartialPrinter {
    instance: String,
    service: String,
    target: String,
    port: u16,
    txt: BTreeMap<String, String>,
    last_seen_ms: u64,
}

#[derive(Default)]
struct DiscoveryState {
    printers: BTreeMap<String, PartialPrinter>,
    ipv4: BTreeMap<String, [u8; 4]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrinterSnapshot {
    pub name: String,
    pub uri: String,
    pub secure: bool,
    pub make_and_model: Option<String>,
    pub formats: Vec<String>,
    pub last_seen_ms: u64,
}

static PRINTERS: Mutex<Vec<PrinterSnapshot>> = Mutex::new(Vec::new());

fn monotonic_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1_000) / hz
}

pub fn snapshot() -> Vec<PrinterSnapshot> {
    PRINTERS.lock().clone()
}

pub fn snapshot_text() -> String {
    let printers = PRINTERS.lock();
    let mut out = String::new();
    let _ = writeln!(out, "trueos printer snapshot v1");
    let _ = writeln!(out, "generated_at_ms={}", monotonic_ms());
    let _ = writeln!(out, "printer_count={}", printers.len());
    let _ = writeln!(out, "printer\tname\turi\tsecure\tmake_and_model\tformats\tlast_seen_ms");
    for printer in printers.iter() {
        let model = printer.make_and_model.as_deref().unwrap_or("");
        let formats = printer.formats.join(",");
        let _ = writeln!(
            out,
            "printer\t{}\t{}\t{}\t{}\t{}\t{}",
            sanitize_field(&printer.name),
            sanitize_field(&printer.uri),
            printer.secure as u8,
            sanitize_field(model),
            sanitize_field(&formats),
            printer.last_seen_ms,
        );
    }
    out
}

fn sanitize_field(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\t' | '\r' | '\n' => ' ',
            _ => ch,
        })
        .collect()
}

fn publish(printers: Vec<PrinterSnapshot>) {
    let newly_discovered = {
        let mut registry = PRINTERS.lock();
        let new = printers
            .iter()
            .filter(|candidate| !registry.iter().any(|known| known.uri == candidate.uri))
            .cloned()
            .collect::<Vec<_>>();
        *registry = printers;
        new
    };

    for printer in newly_discovered {
        crate::log_os::printer_discovered(&printer.name, &printer.uri);
    }
}

impl DiscoveryState {
    fn prune(&mut self, now_ms: u64) {
        self.printers.retain(|_, printer| {
            now_ms.saturating_sub(printer.last_seen_ms) <= PRINTER_STALE_AFTER_MS
        });
    }

    fn materialize(&self) -> Vec<PrinterSnapshot> {
        let mut result = Vec::new();
        for printer in self.printers.values() {
            if printer.target.is_empty() || printer.port == 0 {
                continue;
            }
            let secure =
                printer.service.contains("_ipps._tcp") || printer.instance.contains("._ipps._tcp");
            let scheme = if secure { "ipps" } else { "ipp" };
            let host = if secure {
                printer.target.trim_end_matches('.').to_string()
            } else {
                let Some(addr) = self.ipv4.get(&dns_key(&printer.target)) else {
                    continue;
                };
                format!("{}.{}.{}.{}", addr[0], addr[1], addr[2], addr[3])
            };
            let resource = printer
                .txt
                .get("rp")
                .map(|value| value.trim_start_matches('/'))
                .filter(|value| !value.is_empty())
                .unwrap_or("ipp/print");
            let uri = format!("{scheme}://{host}:{}/{resource}", printer.port);
            let name = printer
                .txt
                .get("ty")
                .filter(|value| !value.is_empty())
                .cloned()
                .unwrap_or_else(|| display_instance_name(&printer.instance));
            let make_and_model = printer
                .txt
                .get("product")
                .map(|value| value.trim_matches(['(', ')']).to_string())
                .filter(|value| !value.is_empty());
            let formats = printer
                .txt
                .get("pdl")
                .map(|value| {
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let candidate = PrinterSnapshot {
                name,
                uri,
                secure,
                make_and_model,
                formats,
                last_seen_ms: printer.last_seen_ms,
            };
            if !result
                .iter()
                .any(|known: &PrinterSnapshot| known.uri == candidate.uri)
            {
                result.push(candidate);
            }
        }
        result.sort_by(|left, right| {
            left.secure
                .cmp(&right.secure)
                .then_with(|| left.name.cmp(&right.name))
        });
        result
    }
}

#[embassy_executor::task]
pub async fn printer_discovery_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

    loop {
        let Some(vnet) = VNet::open_primary() else {
            Timer::after(Duration::from_millis(250)).await;
            continue;
        };
        if vnet
            .submit(v::vnet::Command::OpenUdp { port: MDNS_PORT })
            .is_err()
        {
            Timer::after(Duration::from_millis(250)).await;
            continue;
        }

        let query = build_mdns_query(&SERVICES);
        let mut udp_handle = None;
        let mut next_query = Instant::now();
        let mut state = DiscoveryState::default();

        'socket: loop {
            while let Some(event) = vnet.pop_event() {
                match event {
                    v::vnet::Event::Opened { handle, kind } if kind == v::vnet::SocketKind::Udp => {
                        udp_handle = Some(handle);
                        next_query = Instant::now();
                    }
                    v::vnet::Event::UdpPacket { handle, data, .. }
                        if udp_handle == Some(handle) =>
                    {
                        let now_ms = monotonic_ms();
                        if parse_mdns_packet(data.as_slice(), &mut state, now_ms).is_ok() {
                            state.prune(now_ms);
                            publish(state.materialize());
                        }
                    }
                    v::vnet::Event::Closed { handle } if udp_handle == Some(handle) => {
                        break 'socket;
                    }
                    v::vnet::Event::Error { msg } => {
                        crate::log_warn!(target: "net"; "printer: discovery error={}\n", msg);
                    }
                    _ => {}
                }
            }

            if let Some(handle) = udp_handle
                && Instant::now() >= next_query
            {
                let _ = vnet.submit(v::vnet::Command::SendUdp {
                    handle,
                    remote: MDNS_MULTICAST,
                    data: v::vnet::ByteBuf::from_slice_trunc(&query),
                });
                let now_ms = monotonic_ms();
                state.prune(now_ms);
                publish(state.materialize());
                next_query = Instant::now() + Duration::from_millis(DISCOVERY_INTERVAL_MS);
            }

            Timer::after(Duration::from_millis(10)).await;
        }

        Timer::after(Duration::from_millis(250)).await;
    }
}

fn build_mdns_query(services: &[&str]) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(services.len() as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    for service in services {
        encode_dns_name(service, &mut out);
        out.extend_from_slice(&12u16.to_be_bytes()); // PTR
        out.extend_from_slice(&1u16.to_be_bytes()); // IN, multicast response
    }
    out
}

fn encode_dns_name(name: &str, out: &mut Vec<u8>) {
    for label in name.trim_end_matches('.').split('.') {
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
}

fn parse_mdns_packet(
    packet: &[u8],
    state: &mut DiscoveryState,
    now_ms: u64,
) -> Result<(), &'static str> {
    if packet.len() < 12 {
        return Err("short DNS header");
    }
    let question_count = read_u16(packet, 4)? as usize;
    let answer_count = read_u16(packet, 6)? as usize;
    let authority_count = read_u16(packet, 8)? as usize;
    let additional_count = read_u16(packet, 10)? as usize;
    let mut offset = 12usize;

    for _ in 0..question_count {
        let _ = read_dns_name(packet, &mut offset)?;
        checked_advance(packet, &mut offset, 4)?;
    }

    let record_count = answer_count
        .checked_add(authority_count)
        .and_then(|count| count.checked_add(additional_count))
        .ok_or("DNS record count overflow")?;
    for _ in 0..record_count {
        let owner = read_dns_name(packet, &mut offset)?;
        let record_type = read_u16(packet, offset)?;
        checked_advance(packet, &mut offset, 2)?;
        checked_advance(packet, &mut offset, 2)?; // class
        checked_advance(packet, &mut offset, 4)?; // ttl
        let data_len = read_u16(packet, offset)? as usize;
        checked_advance(packet, &mut offset, 2)?;
        let data_start = offset;
        let data_end = data_start
            .checked_add(data_len)
            .filter(|end| *end <= packet.len())
            .ok_or("DNS record exceeds packet")?;

        match record_type {
            1 if data_len == 4 => {
                state.ipv4.insert(
                    dns_key(&owner),
                    [
                        packet[data_start],
                        packet[data_start + 1],
                        packet[data_start + 2],
                        packet[data_start + 3],
                    ],
                );
            }
            12 => {
                let mut cursor = data_start;
                let instance = read_dns_name(packet, &mut cursor)?;
                if cursor > data_end {
                    return Err("PTR target exceeds record");
                }
                let printer = state.printers.entry(dns_key(&instance)).or_default();
                printer.instance = instance;
                printer.last_seen_ms = now_ms;
                if owner.contains("_ipp._tcp") || owner.contains("_ipps._tcp") {
                    printer.service = owner;
                }
            }
            16 => {
                let values = parse_txt(&packet[data_start..data_end])?;
                let printer = state.printers.entry(dns_key(&owner)).or_default();
                printer.instance = owner;
                printer.last_seen_ms = now_ms;
                printer.txt.extend(values);
            }
            33 if data_len >= 6 => {
                let port = read_u16(packet, data_start + 4)?;
                let mut cursor = data_start + 6;
                let target = read_dns_name(packet, &mut cursor)?;
                if cursor > data_end {
                    return Err("SRV target exceeds record");
                }
                let printer = state.printers.entry(dns_key(&owner)).or_default();
                printer.instance = owner;
                printer.target = target;
                printer.port = port;
                printer.last_seen_ms = now_ms;
            }
            _ => {}
        }
        offset = data_end;
    }
    Ok(())
}

fn parse_txt(data: &[u8]) -> Result<BTreeMap<String, String>, &'static str> {
    let mut values = BTreeMap::new();
    let mut offset = 0usize;
    while offset < data.len() {
        let len = data[offset] as usize;
        offset += 1;
        let end = offset
            .checked_add(len)
            .filter(|end| *end <= data.len())
            .ok_or("truncated DNS TXT item")?;
        if let Ok(text) = core::str::from_utf8(&data[offset..end]) {
            let (key, value) = text.split_once('=').unwrap_or((text, ""));
            values.insert(key.to_ascii_lowercase(), value.to_string());
        }
        offset = end;
    }
    Ok(values)
}

fn read_dns_name(packet: &[u8], offset: &mut usize) -> Result<String, &'static str> {
    let mut labels = Vec::new();
    let mut cursor = *offset;
    let mut jumped = false;
    let mut jumps = 0usize;
    loop {
        let length = *packet.get(cursor).ok_or("truncated DNS name")?;
        if length & 0xc0 == 0xc0 {
            let next = *packet
                .get(cursor + 1)
                .ok_or("truncated DNS compression pointer")?;
            if !jumped {
                *offset = cursor + 2;
                jumped = true;
            }
            cursor = (((length & 0x3f) as usize) << 8) | next as usize;
            jumps += 1;
            if jumps > 32 || cursor >= packet.len() {
                return Err("invalid DNS compression pointer");
            }
            continue;
        }
        if length & 0xc0 != 0 {
            return Err("unsupported DNS label encoding");
        }
        cursor += 1;
        if length == 0 {
            if !jumped {
                *offset = cursor;
            }
            break;
        }
        let end = cursor
            .checked_add(length as usize)
            .filter(|end| *end <= packet.len())
            .ok_or("truncated DNS label")?;
        let label =
            core::str::from_utf8(&packet[cursor..end]).map_err(|_| "non-UTF-8 DNS label")?;
        labels.push(label.to_string());
        cursor = end;
    }
    if labels.is_empty() {
        Ok(String::from("."))
    } else {
        Ok(format!("{}.", labels.join(".")))
    }
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, &'static str> {
    let bytes = data.get(offset..offset + 2).ok_or("truncated u16")?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn checked_advance(data: &[u8], offset: &mut usize, count: usize) -> Result<(), &'static str> {
    *offset = offset
        .checked_add(count)
        .filter(|end| *end <= data.len())
        .ok_or("truncated DNS field")?;
    Ok(())
}

fn dns_key(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}

fn display_instance_name(instance: &str) -> String {
    let lowered = instance.to_ascii_lowercase();
    for marker in ["._ipp._tcp.", "._ipps._tcp."] {
        if let Some(index) = lowered.find(marker) {
            return instance[..index].to_string();
        }
    }
    instance.trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_contains_driverless_services() {
        let query = build_mdns_query(&SERVICES);
        assert_eq!(read_u16(&query, 4), Ok(3));
        assert!(query.windows(3).any(|window| window == b"ipp"));
        assert!(query.windows(4).any(|window| window == b"ipps"));
    }

    #[test]
    fn compression_pointer_decodes() {
        let packet = [
            3, b'f', b'o', b'o', 5, b'l', b'o', b'c', b'a', b'l', 0, 3, b'b', b'a', b'r', 0xc0,
            0x00,
        ];
        let mut offset = 11;
        assert_eq!(read_dns_name(&packet, &mut offset).as_deref(), Ok("bar.foo.local."));
        assert_eq!(offset, packet.len());
    }

    #[test]
    fn snapshot_fields_are_single_line() {
        assert_eq!(sanitize_field("office\tprinter\n"), "office printer ");
    }
}
