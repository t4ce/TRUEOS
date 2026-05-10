extern crate alloc;
extern crate std;

use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::sync::atomic::{AtomicU16, Ordering};
use std::{io, net::SocketAddr};

use axum::{
    Router,
    body::Body,
    http::{
        StatusCode,
        header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::Response,
    routing::get,
    serve::ListenerExt,
};
use embassy_time::{Duration as EmbassyDuration, Timer};
use serde::Serialize;

use crate::allports::services::WEBDEVICES_HTTP_TCP_PORT;

const WEBDEVICES_HTTP_BLOCKING_LANE_RETRY_MS: u64 = 1000;
const WEBDEVICES_INDEX_HTML: &str = include_str!("index.html");

static WEBDEVICES_HTTP_PORT: AtomicU16 = AtomicU16::new(0);

pub fn current_port() -> Option<u16> {
    match WEBDEVICES_HTTP_PORT.load(Ordering::Acquire) {
        0 => None,
        port => Some(port),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HardwareSnapshot {
    schema: &'static str,
    generated_at_s: u64,
    uptime_s: u64,
    service: ServiceSnapshot,
    pci: PciSnapshot,
    usb: UsbSnapshot,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceSnapshot {
    name: &'static str,
    port: Option<u16>,
    primary_ipv4: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PciSnapshot {
    count: usize,
    classes: Vec<ClassCount>,
    devices: Vec<PciDeviceSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClassCount {
    class: String,
    count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PciDeviceSummary {
    id: String,
    bdf: String,
    vendor_id: String,
    device_id: String,
    class_code: String,
    subclass: String,
    prog_if: String,
    class_name: &'static str,
    role: &'static str,
    name: String,
    command: String,
    status: String,
    bars: Vec<PciBarSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PciBarSummary {
    index: u8,
    kind: &'static str,
    width: &'static str,
    prefetchable: bool,
    base: String,
    raw: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsbSnapshot {
    controller_count: usize,
    device_count: usize,
    topology_count: usize,
    probe_device_count: Option<u32>,
    probe_error: Option<&'static str>,
    controllers: Vec<UsbControllerSummary>,
    devices: Vec<UsbDeviceSummary>,
    topology: Vec<UsbTopologySummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsbControllerSummary {
    id: usize,
    bdf: String,
    vendor_id: String,
    device_id: String,
    phase: &'static str,
    lifecycle: &'static str,
    event_ready: bool,
    root_port_change_seen: bool,
    empty_probe_streak: u32,
    ports: Vec<UsbPortSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsbPortSummary {
    id: u8,
    connected: bool,
    enabled: bool,
    powered: bool,
    reset: bool,
    speed: String,
    link_state: &'static str,
    portsc: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsbDeviceSummary {
    id: String,
    controller_id: usize,
    slot_id: u32,
    root_port_id: u8,
    port_id: u8,
    route: String,
    speed: &'static str,
    vendor_id: String,
    product_id: String,
    class_code: String,
    class_name: &'static str,
    manufacturer: Option<String>,
    product: Option<String>,
    serial: Option<String>,
    interface_count: usize,
    endpoint_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsbTopologySummary {
    controller_id: usize,
    kind: &'static str,
    root_port_id: u8,
    port_id: u8,
    depth: u8,
    slot_id: Option<u32>,
    parent_slot_id: Option<u32>,
    speed: &'static str,
    vid_pid: String,
    class_code: String,
}

fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

fn response(status: u16, content_type: &'static str, body: Vec<u8>, no_store: bool) -> Response {
    let mut builder = Response::builder()
        .status(status_code(status))
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, body.len().to_string());
    if no_store {
        builder = builder.header(CACHE_CONTROL, "no-store");
    } else {
        builder = builder.header(CACHE_CONTROL, "no-cache");
    }
    builder
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn text_response(status: u16, content_type: &'static str, body: &str) -> Response {
    response(status, content_type, body.as_bytes().to_vec(), false)
}

fn json_response<T: Serialize>(status: u16, value: &T) -> Response {
    match serde_json::to_vec(value) {
        Ok(body) => response(status, "application/json; charset=utf-8", body, true),
        Err(_) => text_response(500, "text/plain; charset=utf-8", "json serialization failed\n"),
    }
}

async fn handle_index() -> Response {
    crate::log_trace!("webdevices-http: GET /\n");
    text_response(200, "text/html; charset=utf-8", WEBDEVICES_INDEX_HTML)
}

async fn handle_healthz() -> Response {
    json_response(
        200,
        &serde_json::json!({
            "ok": true,
            "service": "webdevices-http",
            "port": current_port(),
        }),
    )
}

async fn handle_snapshot() -> Response {
    crate::log_trace!("webdevices-http: api snapshot\n");
    json_response(200, &hardware_snapshot())
}

fn now_seconds() -> u64 {
    crate::r::net::ntp::current_unix_seconds()
        .or_else(crate::time::unix_time_seconds)
        .unwrap_or_else(crate::time::uptime_seconds)
}

fn primary_ipv4_string() -> Option<String> {
    let dev_idx = crate::net::primary_device_index();
    let ip = crate::net::adapter::ipv4_at(dev_idx)?;
    Some(format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]))
}

fn primary_ipv4_addr(port: u16) -> Option<SocketAddr> {
    let dev_idx = crate::net::primary_device_index();
    let ip = crate::net::adapter::ipv4_at(dev_idx)?;
    Some(SocketAddr::from((ip, port)))
}

fn hex2(value: u8) -> String {
    format!("{:02X}", value)
}

fn hex4(value: u16) -> String {
    format!("{:04X}", value)
}

fn hex8(value: u32) -> String {
    format!("0x{:08X}", value)
}

fn hex16(value: u64) -> String {
    format!("0x{:016X}", value)
}

fn ensure_pci_devices_enumerated() {
    let mut count = 0usize;
    crate::pci::with_devices(|devices| count = devices.len());
    if count == 0 {
        crate::pci::enumerate_impl();
    }
}

fn pci_class_name(class: u8, subclass: u8, prog_if: u8) -> &'static str {
    match (class, subclass, prog_if) {
        (0x01, 0x06, _) => "SATA storage",
        (0x01, 0x08, _) => "NVMe storage",
        (0x02, _, _) => "Network",
        (0x03, 0x00, _) => "VGA display",
        (0x03, _, _) => "Display",
        (0x04, 0x01, _) => "Audio",
        (0x04, _, _) => "Multimedia",
        (0x06, 0x00, _) => "Host bridge",
        (0x06, 0x01, _) => "ISA bridge",
        (0x06, 0x04, _) => "PCI bridge",
        (0x0C, 0x03, 0x30) => "USB xHCI",
        (0x0C, 0x03, _) => "USB controller",
        (0x0C, _, _) => "Serial bus",
        (0x08, _, _) => "System peripheral",
        (0x0B, _, _) => "Processor",
        _ => "Other",
    }
}

fn pci_role_name(class: u8, subclass: u8, prog_if: u8) -> &'static str {
    match (class, subclass, prog_if) {
        (0x01, 0x08, _) => "storage",
        (0x01, _, _) => "storage",
        (0x02, _, _) => "network",
        (0x03, _, _) => "graphics",
        (0x04, 0x01, _) => "audio",
        (0x06, _, _) => "fabric",
        (0x0C, 0x03, _) => "usb",
        _ => "device",
    }
}

fn usb_class_name(class: u8, subclass: u8, protocol: u8) -> &'static str {
    match (class, subclass, protocol) {
        (0x00, _, _) => "Composite",
        (0x01, _, _) => "Audio",
        (0x03, 0x01, 0x01) => "Keyboard",
        (0x03, 0x01, 0x02) => "Mouse",
        (0x03, _, _) => "HID",
        (0x08, 0x06, 0x50) => "Mass storage",
        (0x08, _, _) => "Storage",
        (0x09, _, _) => "Hub",
        (0x0E, _, _) => "Video",
        (0xE0, _, _) => "Wireless",
        (0xEF, _, _) => "Misc",
        _ => "USB device",
    }
}

fn pci_db() -> Option<Vec<u8>> {
    if !crate::r::readiness::is_set(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED) {
        return None;
    }
    crate::pci::pciids::load_sanitized_from_root_blocking()
        .ok()
        .flatten()
}

fn pci_name(dev: &crate::pci::PciDevice, db: Option<&[u8]>) -> String {
    if let Some(db) = db {
        if let Some((vendor, device)) =
            crate::pci::pciids::lookup_vendor_device_from_db(db, dev.vendor, dev.device)
        {
            let vendor_s = String::from_utf8_lossy(vendor).trim().to_string();
            let device_s = String::from_utf8_lossy(device).trim().to_string();
            return format!("{} {}", vendor_s, device_s);
        }
    }
    format!(
        "{} {}:{}",
        pci_class_name(dev.class, dev.subclass, dev.prog_if),
        hex4(dev.vendor),
        hex4(dev.device)
    )
}

fn pci_bars(dev: &crate::pci::PciDevice) -> Vec<PciBarSummary> {
    let mut bars = Vec::new();
    let mut index = 0u8;
    while index < 6 {
        let (lo, hi) = crate::pci::read_bar_raw(dev.bus, dev.slot, dev.function, index);
        if lo == 0 || lo == 0xFFFF_FFFF {
            index += 1;
            continue;
        }

        if (lo & 0x1) != 0 {
            bars.push(PciBarSummary {
                index,
                kind: "io",
                width: "32",
                prefetchable: false,
                base: hex16((lo & !0x3) as u64),
                raw: hex8(lo),
            });
            index += 1;
            continue;
        }

        let is_64 = ((lo >> 1) & 0x3) == 0x2;
        let base = if is_64 {
            (((hi.unwrap_or(0) as u64) << 32) | (lo as u64)) & !0xFu64
        } else {
            (lo as u64) & !0xFu64
        };
        bars.push(PciBarSummary {
            index,
            kind: "mmio",
            width: if is_64 { "64" } else { "32" },
            prefetchable: (lo & 0x8) != 0,
            base: hex16(base),
            raw: match hi {
                Some(hi) => format!("{}:{}", hex8(hi), hex8(lo)),
                None => hex8(lo),
            },
        });
        index += if is_64 { 2 } else { 1 };
    }
    bars
}

fn pci_snapshot() -> PciSnapshot {
    ensure_pci_devices_enumerated();
    let db = pci_db();
    let mut devices = Vec::new();
    crate::pci::with_devices(|list| {
        for dev in list.iter() {
            let bdf = format!("{:02X}:{:02X}.{}", dev.bus, dev.slot, dev.function);
            let command = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);
            let status = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x06);
            devices.push(PciDeviceSummary {
                id: format!("pci-{}", bdf),
                bdf,
                vendor_id: hex4(dev.vendor),
                device_id: hex4(dev.device),
                class_code: hex2(dev.class),
                subclass: hex2(dev.subclass),
                prog_if: hex2(dev.prog_if),
                class_name: pci_class_name(dev.class, dev.subclass, dev.prog_if),
                role: pci_role_name(dev.class, dev.subclass, dev.prog_if),
                name: pci_name(dev, db.as_deref()),
                command: hex4(command),
                status: hex4(status),
                bars: pci_bars(dev),
            });
        }
    });

    let mut classes = Vec::<ClassCount>::new();
    for dev in devices.iter() {
        if let Some(item) = classes
            .iter_mut()
            .find(|item| item.class.as_str() == dev.class_name)
        {
            item.count += 1;
        } else {
            classes.push(ClassCount {
                class: dev.class_name.to_string(),
                count: 1,
            });
        }
    }

    PciSnapshot {
        count: devices.len(),
        classes,
        devices,
    }
}

fn usb_port_speed_text(portsc: u32) -> String {
    match (portsc >> 10) & 0xF {
        0 => String::from("-"),
        1 => String::from("full"),
        2 => String::from("low"),
        3 => String::from("high"),
        4 => String::from("super"),
        5 => String::from("super+"),
        n => format!("sp{}", n),
    }
}

fn usb_port_link_state(portsc: u32) -> &'static str {
    match (portsc >> 5) & 0xF {
        0 => "U0",
        1 => "U1",
        2 => "U2",
        3 => "U3",
        4 => "disabled",
        5 => "rxdetect",
        6 => "inactive",
        7 => "polling",
        8 => "recovery",
        9 => "hot-reset",
        10 => "compliance",
        11 => "test",
        15 => "resume",
        _ => "reserved",
    }
}

fn usb_snapshot() -> UsbSnapshot {
    let snapshot = crate::usb2::tlb_usb_snapshot();
    let mut controllers = Vec::new();
    for ctrl in snapshot.controllers.iter() {
        let mut ports = Vec::new();
        if let Some(diag) = crate::usb2::controller_mmio_diag(ctrl.index) {
            for port in diag.ports {
                ports.push(UsbPortSummary {
                    id: port.port_id,
                    connected: (port.portsc & 0x1) != 0,
                    enabled: (port.portsc & 0x2) != 0,
                    powered: (port.portsc & (1 << 9)) != 0,
                    reset: (port.portsc & (1 << 4)) != 0,
                    speed: usb_port_speed_text(port.portsc),
                    link_state: usb_port_link_state(port.portsc),
                    portsc: hex8(port.portsc),
                });
            }
        }

        controllers.push(UsbControllerSummary {
            id: ctrl.index,
            bdf: format!("{:02X}:{:02X}.{}", ctrl.bus, ctrl.slot, ctrl.function),
            vendor_id: hex4(ctrl.vendor_id),
            device_id: hex4(ctrl.device_id),
            phase: ctrl.controller_phase,
            lifecycle: ctrl.root_hub_lifecycle,
            event_ready: ctrl.event_ready,
            root_port_change_seen: ctrl.root_port_change_seen,
            empty_probe_streak: ctrl.empty_probe_streak,
            ports,
        });
    }

    let mut devices = Vec::new();
    for dev in snapshot.devices.iter() {
        let mut interface_count = 0usize;
        let mut endpoint_count = 0usize;
        for cfg in dev.configurations.iter() {
            interface_count += cfg.interfaces.len();
            for iface in cfg.interfaces.iter() {
                endpoint_count += iface.endpoints.len();
            }
        }
        devices.push(UsbDeviceSummary {
            id: format!("usb-{}-{}", dev.controller_index, dev.stable_id),
            controller_id: dev.controller_index,
            slot_id: dev.slot_id,
            root_port_id: dev.root_port_id,
            port_id: dev.port_id,
            route: hex8(dev.route_string),
            speed: dev.speed,
            vendor_id: hex4(dev.vendor_id),
            product_id: hex4(dev.product_id),
            class_code: format!("{:02X}/{:02X}/{:02X}", dev.class, dev.subclass, dev.protocol),
            class_name: usb_class_name(dev.class, dev.subclass, dev.protocol),
            manufacturer: dev.manufacturer.clone(),
            product: dev.product.clone(),
            serial: dev.serial.clone(),
            interface_count,
            endpoint_count,
        });
    }

    let mut topology = Vec::new();
    for node in snapshot.topology.iter() {
        let kind = match node.kind {
            crate::usb2::TlbUsbTopologyNodeKind::RootPort => "root",
            crate::usb2::TlbUsbTopologyNodeKind::Hub => "hub",
            crate::usb2::TlbUsbTopologyNodeKind::Device => "device",
        };
        topology.push(UsbTopologySummary {
            controller_id: node.controller_index,
            kind,
            root_port_id: node.root_port_id,
            port_id: node.port_id,
            depth: node.depth,
            slot_id: node.slot_id,
            parent_slot_id: node.parent_slot_id,
            speed: node.speed,
            vid_pid: match (node.vendor_id, node.product_id) {
                (Some(vid), Some(pid)) => format!("{}:{}", hex4(vid), hex4(pid)),
                _ => String::from("-"),
            },
            class_code: match (node.class, node.subclass, node.protocol) {
                (Some(class), Some(subclass), Some(protocol)) => {
                    format!("{:02X}/{:02X}/{:02X}", class, subclass, protocol)
                }
                _ => String::from("-"),
            },
        });
    }

    UsbSnapshot {
        controller_count: controllers.len(),
        device_count: devices.len(),
        topology_count: topology.len(),
        probe_device_count: snapshot.probe_device_count,
        probe_error: snapshot.probe_error,
        controllers,
        devices,
        topology,
    }
}

fn hardware_snapshot() -> HardwareSnapshot {
    HardwareSnapshot {
        schema: "trueos.webdevices.v1",
        generated_at_s: now_seconds(),
        uptime_s: crate::time::uptime_seconds(),
        service: ServiceSnapshot {
            name: "webdevices-http",
            port: current_port(),
            primary_ipv4: primary_ipv4_string(),
        },
        pci: pci_snapshot(),
        usb: usb_snapshot(),
    }
}

pub fn router() -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/index.html", get(handle_index))
        .route("/healthz", get(handle_healthz))
        .route("/api/healthz", get(handle_healthz))
        .route("/api/webdevices/snapshot", get(handle_snapshot))
        .route("/api/devices/snapshot", get(handle_snapshot))
}

async fn webdevices_http_runtime() -> Result<(), io::Error> {
    tokio::task::spawn_local(crate::t::shared_tokio_job_pump());

    let app = router();
    loop {
        let Some(addr) = primary_ipv4_addr(WEBDEVICES_HTTP_TCP_PORT) else {
            WEBDEVICES_HTTP_PORT.store(0, Ordering::Release);
            crate::log_trace!("webdevices-http: waiting for primary ipv4\n");
            tokio::time::sleep(core::time::Duration::from_millis(100)).await;
            continue;
        };

        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                WEBDEVICES_HTTP_PORT.store(0, Ordering::Release);
                crate::log_trace!(
                    "webdevices-http: bind {} failed kind={:?} err={}\n",
                    addr,
                    err.kind(),
                    err
                );
                tokio::time::sleep(core::time::Duration::from_millis(1000)).await;
                continue;
            }
        };

        WEBDEVICES_HTTP_PORT.store(addr.port(), Ordering::Release);
        crate::log_trace!("webdevices-http: axum listening on http://{}/\n", addr);
        let listener = listener.tap_io(|_| crate::log_trace!("webdevices-http: tcp accepted\n"));
        let result = axum::serve(listener, app).await;
        if result.is_err() {
            WEBDEVICES_HTTP_PORT.store(0, Ordering::Release);
        }
        return result;
    }
}

fn run_webdevices_http_runtime() -> Result<(), io::Error> {
    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_io();
    builder.enable_time();
    let runtime = builder.build()?;
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, webdevices_http_runtime())
}

#[embassy_executor::task]
pub async fn webdevices_http_service_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_V4_CONFIGURED).await;
    crate::log_trace!("webdevices-http: launching Tokio runtime after NET_V4_CONFIGURED\n");

    loop {
        let rc = crate::trueos_tokio_worker::spawn_blocking_job_with_purpose(
            Box::new(|| {
                if let Err(err) = run_webdevices_http_runtime() {
                    crate::log_trace!("webdevices-http: runtime failed {:?}\n", err);
                }
            }),
            "webdevices-http-runtime",
        );
        if rc == 0 {
            crate::log_trace!("webdevices-http: submitted Tokio runtime to blocking lane\n");
            core::future::pending::<()>().await;
        }
        crate::log_trace!(
            "webdevices-http: blocking lane unavailable rc={} retry={}ms\n",
            rc,
            WEBDEVICES_HTTP_BLOCKING_LANE_RETRY_MS
        );
        Timer::after(EmbassyDuration::from_millis(WEBDEVICES_HTTP_BLOCKING_LANE_RETRY_MS)).await;
    }
}
