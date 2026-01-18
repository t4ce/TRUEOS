use super::{Trb, TrbRing, XhciContext};
use super::xhci::trb_type;
use crate::pci::dma;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;
use trueos_math::{NodeId, Tree};

pub const USB_CLASS_HUB: u8 = 0x09;
pub const USB_SUBCLASS_HUB: u8 = 0x00;

pub const LOG_PORTS_MAX: usize = 32;
pub const MAX_DEVICES: usize = 8;
const USB_TREE_CAPACITY: usize = LOG_PORTS_MAX + MAX_DEVICES + 1;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum UsbNodeKind {
    Root,
    Port,
    Device,
}

#[derive(Copy, Clone, Debug)]
struct UsbNode {
    kind: UsbNodeKind,
    port: u8,
    slot_id: u32,
    vid: u16,
    pid: u16,
    class: u8,
    subclass: u8,
    protocol: u8,
}

static USB_TREE: Mutex<Option<Tree<UsbNode, USB_TREE_CAPACITY>>> = Mutex::new(None);
static ROOT_PORT_NODE_IDS: Mutex<Vec<NodeId, LOG_PORTS_MAX>> = Mutex::new(Vec::new());
static SLOT_NODE_IDS: Mutex<Vec<(u32, NodeId), MAX_DEVICES>> = Mutex::new(Vec::new());
static HUB_PORT_NODE_IDS: Mutex<Vec<(u32, u8, NodeId), 64>> = Mutex::new(Vec::new());

pub fn init_topology(port_count: u8) {
    let mut tree_guard = USB_TREE.lock();
    let mut tree = Tree::new();
    let root = match tree.add_root(UsbNode {
        kind: UsbNodeKind::Root,
        port: 0,
        slot_id: 0,
        vid: 0,
        pid: 0,
        class: 0,
        subclass: 0,
        protocol: 0,
    }) {
        Some(id) => id,
        None => {
            *tree_guard = Some(tree);
            return;
        }
    };

    let mut ports = ROOT_PORT_NODE_IDS.lock();
    ports.clear();
    SLOT_NODE_IDS.lock().clear();
    HUB_PORT_NODE_IDS.lock().clear();

    let limit = core::cmp::min(port_count as usize, LOG_PORTS_MAX);
    for port in 1..=limit {
        let Some(node) = tree.add_child(
            root,
            UsbNode {
                kind: UsbNodeKind::Port,
                port: port as u8,
                slot_id: 0,
                vid: 0,
                pid: 0,
                class: 0,
                subclass: 0,
                protocol: 0,
            },
        ) else {
            break;
        };
        let _ = ports.push(node);
    }

    *tree_guard = Some(tree);
}

pub fn record_root_device(
    target_port: u8,
    slot_id: u32,
    dev_vid: u16,
    dev_pid: u16,
    dev_cls: u8,
    dev_sub: u8,
    dev_prot: u8,
) {
    let mut tree_guard = USB_TREE.lock();
    let Some(tree) = tree_guard.as_mut() else {
        return;
    };

    let ports = ROOT_PORT_NODE_IDS.lock();
    let idx = (target_port as usize).saturating_sub(1);
    let Some(port_node) = ports.get(idx).copied() else {
        return;
    };

    let Some(node) = tree.add_child(
        port_node,
        UsbNode {
            kind: UsbNodeKind::Device,
            port: target_port,
            slot_id,
            vid: dev_vid,
            pid: dev_pid,
            class: dev_cls,
            subclass: dev_sub,
            protocol: dev_prot,
        },
    ) else {
        return;
    };

    let _ = SLOT_NODE_IDS.lock().push((slot_id, node));
}

pub fn record_hub_ports(hub_slot_id: u32, port_count: u8) {
    let mut tree_guard = USB_TREE.lock();
    let Some(tree) = tree_guard.as_mut() else {
        return;
    };

    let hub_node = {
        let slots = SLOT_NODE_IDS.lock();
        slots.iter().find(|(slot, _)| *slot == hub_slot_id).map(|(_, id)| *id)
    };

    let Some(hub_node) = hub_node else {
        return;
    };

    let mut hub_ports = HUB_PORT_NODE_IDS.lock();
    for port in 1..=port_count {
        if hub_ports.iter().any(|(slot, p, _)| *slot == hub_slot_id && *p == port) {
            continue;
        }
        if let Some(node) = tree.add_child(
            hub_node,
            UsbNode {
                kind: UsbNodeKind::Port,
                port,
                slot_id: 0,
                vid: 0,
                pid: 0,
                class: 0,
                subclass: 0,
                protocol: 0,
            },
        ) {
            let _ = hub_ports.push((hub_slot_id, port, node));
        }
    }
}

pub fn record_hub_child(
    hub_slot_id: u32,
    hub_port: u8,
    slot_id: u32,
    dev_vid: u16,
    dev_pid: u16,
    dev_cls: u8,
    dev_sub: u8,
    dev_prot: u8,
) {
    let mut tree_guard = USB_TREE.lock();
    let Some(tree) = tree_guard.as_mut() else {
        return;
    };

    let port_node = {
        let ports = HUB_PORT_NODE_IDS.lock();
        ports
            .iter()
            .find(|(slot, port, _)| *slot == hub_slot_id && *port == hub_port)
            .map(|(_, _, id)| *id)
    };

    let Some(port_node) = port_node else {
        return;
    };

    let Some(node) = tree.add_child(
        port_node,
        UsbNode {
            kind: UsbNodeKind::Device,
            port: hub_port,
            slot_id,
            vid: dev_vid,
            pid: dev_pid,
            class: dev_cls,
            subclass: dev_sub,
            protocol: dev_prot,
        },
    ) else {
        return;
    };

    let _ = SLOT_NODE_IDS.lock().push((slot_id, node));
}

pub struct AttachParams<'a> {
    pub ctx: &'a XhciContext,
    pub ep0_ring: &'a mut TrbRing,
    pub slot_id: u32,
    pub cfg: &'a [u8],
    pub target_port: u8,
}

#[derive(Copy, Clone, Debug)]
pub struct HubWork {
    pub hub_slot_id: u32,
    pub root_port: u8,
    pub route_string: u32,
    pub depth: u8,
    pub hub_speed_code: u32,
    pub port_count: u8,
}

#[derive(Copy, Clone, Debug)]
pub struct HubChild {
    pub port: u8,
    pub route: u32,
    pub depth: u8,
    pub speed_code: u32,
    pub tt_info: Option<(u32, u8)>,
}

pub fn is_hub_device(dev_cls: u8, dev_sub: u8, _dev_prot: u8, cfg: &[u8]) -> bool {
    if dev_cls == USB_CLASS_HUB {
        return true;
    }

    // Composite device: look for a hub interface.
    let mut idx = 0usize;
    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        let ty = cfg[idx + 1];
        if ty == 4 && len >= 9 {
            let if_cls = cfg[idx + 5];
            let if_sub = cfg[idx + 6];
            if if_cls == USB_CLASS_HUB && if_sub == USB_SUBCLASS_HUB {
                return true;
            }
        }
        idx += len;
    }

    false
}

pub async fn attach_device(params: AttachParams<'_>) -> Result<HubDescriptorInfo, ()> {
    let AttachParams {
        ctx,
        ep0_ring,
        slot_id,
        cfg: _,
        target_port,
    } = params;

    let Some(desc) = read_hub_descriptor(ctx, ep0_ring, slot_id).await else {
        crate::log!(
            "usb: hub claimed slot={} port={} (hub descriptor read failed)\n",
            slot_id,
            target_port
        );
        return Err(());
    };

    crate::log!(
        "usb: hub claimed slot={} port={} desc=0x{:02X} ports={}\n",
        slot_id,
        target_port,
        desc.desc_type,
        desc.port_count
    );

    log_hub_ports(ctx, ep0_ring, slot_id, desc.port_count).await;

    Ok(desc)
}

#[derive(Copy, Clone, Debug)]
pub struct HubDescriptorInfo {
    pub desc_type: u8,
    pub port_count: u8,
}

#[derive(Copy, Clone, Debug)]
pub struct HubPortState {
    pub port: u8,
    pub status: u16,
    pub change: u16,
    pub connected: bool,
    pub enabled: bool,
    pub speed_code: u32,
}

async fn read_hub_descriptor(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
) -> Option<HubDescriptorInfo> {
    let (buf_phys, buf_virt) = dma::alloc(64, 64)?;
    unsafe { core::ptr::write_bytes(buf_virt, 0, 64) };

    let mut desc_type = 0x29u8;
    let mut res = super::control_in(
        ctx,
        ep0_ring,
        slot_id,
        setup_get_hub_descriptor(desc_type, 9),
        buf_phys,
        9,
        "hub-desc",
        800,
    )
    .await;

    if res.is_err() {
        desc_type = 0x2Au8;
        res = super::control_in(
            ctx,
            ep0_ring,
            slot_id,
            setup_get_hub_descriptor(desc_type, 9),
            buf_phys,
            9,
            "hub-desc",
            800,
        )
        .await;
    }

    let info = if res.is_ok() {
        unsafe {
            let b = core::slice::from_raw_parts(buf_virt, 9);
            if b.len() >= 3 {
                Some(HubDescriptorInfo {
                    desc_type,
                    port_count: b[2],
                })
            } else {
                None
            }
        }
    } else {
        None
    };

    dma::dealloc(buf_virt, 64);
    info
}

pub async fn scan_ports(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    port_count: u8,
) -> Vec<HubPortState, 32> {
    let mut out: Vec<HubPortState, 32> = Vec::new();
    let (buf_phys, buf_virt) = match dma::alloc(8, 8) {
        Some(pair) => pair,
        None => return out,
    };

    for port in 1..=port_count {
        if let Some(state) = read_port_state(ctx, ep0_ring, slot_id, port, buf_phys, buf_virt).await {
            let _ = out.push(state);
        }
    }

    dma::dealloc(buf_virt, 8);
    out
}

pub async fn ensure_port_enabled(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    port: u8,
) -> Option<HubPortState> {
    let (buf_phys, buf_virt) = dma::alloc(8, 8)?;

    let _ = set_port_feature(ctx, ep0_ring, slot_id, port, HUB_FEATURE_PORT_POWER).await;
    let _ = set_port_feature(ctx, ep0_ring, slot_id, port, HUB_FEATURE_PORT_RESET).await;

    let mut last = None;
    for _ in 0..50u32 {
        if let Some(state) = read_port_state(ctx, ep0_ring, slot_id, port, buf_phys, buf_virt).await {
            last = Some(state);
            if state.connected && state.enabled {
                break;
            }
        }
        Timer::after(EmbassyDuration::from_millis(10)).await;
    }

    dma::dealloc(buf_virt, 8);
    last
}

pub async fn collect_children(
    ctx: &XhciContext,
    hub_slot_id: u32,
    parent_route: u32,
    depth: u8,
    hub_speed_code: u32,
    port_count: u8,
) -> Vec<HubChild, 32> {
    let mut out: Vec<HubChild, 32> = Vec::new();
    if depth >= 5 {
        return out;
    }

    let (ep0_phys, ep0_virt_raw, mut ep0_ring) = match dma::alloc(32 * core::mem::size_of::<Trb>(), 64) {
        Some((phys, virt)) => (phys, virt, unsafe { TrbRing::new(phys, virt as *mut Trb, 32) }),
        None => return out,
    };

    let ports = scan_ports(ctx, &mut ep0_ring, hub_slot_id, port_count).await;
    for port_state in ports.iter() {
        if !port_state.connected {
            continue;
        }

        let Some(enabled) = ensure_port_enabled(ctx, &mut ep0_ring, hub_slot_id, port_state.port).await else {
            continue;
        };

        if !enabled.connected {
            continue;
        }

        let route = parent_route | ((port_state.port as u32) << (depth * 4));
        let speed_code = enabled.speed_code;
        let tt_info = if speed_code <= 2 && hub_speed_code == 3 {
            Some((hub_slot_id, port_state.port))
        } else {
            None
        };

        let _ = out.push(HubChild {
            port: port_state.port,
            route,
            depth: depth.saturating_add(1),
            speed_code,
            tt_info,
        });
    }

    dma::dealloc(ep0_virt_raw, 32 * core::mem::size_of::<Trb>());
    out
}

const HUB_FEATURE_PORT_RESET: u16 = 4;
const HUB_FEATURE_PORT_POWER: u16 = 8;

async fn set_port_feature(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    port: u8,
    feature: u16,
) -> Result<(), ()> {
    super::control_out(
        ctx,
        ep0_ring,
        slot_id,
        setup_set_port_feature(port, feature),
        None,
        0,
        "hub-set-port-feature",
        200,
    )
    .await
    .map(|_| ())
}

async fn read_port_state(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    port: u8,
    buf_phys: u64,
    buf_virt: *mut u8,
) -> Option<HubPortState> {
    unsafe { core::ptr::write_bytes(buf_virt, 0, 8) };
    if super::control_in(
        ctx,
        ep0_ring,
        slot_id,
        setup_get_port_status(port, 4),
        buf_phys,
        4,
        "hub-port-status",
        400,
    )
    .await
    .is_err()
    {
        return None;
    }

    let (status, change) = unsafe {
        let b = core::slice::from_raw_parts(buf_virt, 4);
        let st = u16::from_le_bytes([b[0], b[1]]);
        let ch = u16::from_le_bytes([b[2], b[3]]);
        (st, ch)
    };

    let connected = (status & (1 << 0)) != 0;
    let enabled = (status & (1 << 1)) != 0;
    let low = (status & (1 << 9)) != 0;
    let high = (status & (1 << 10)) != 0;
    let speed_code = if high { 3 } else if low { 2 } else { 1 };

    Some(HubPortState {
        port,
        status,
        change,
        connected,
        enabled,
        speed_code,
    })
}

async fn log_hub_ports(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    port_count: u8,
) {
    crate::log!(
        "usb: hub slot={} ports={} (status: ccs ped pp spd csc pec prc oc)\n",
        slot_id,
        port_count
    );
    crate::log!("usb: hub #  status  change ccs ped pp spd csc pec prc oc\n");

    let (buf_phys, buf_virt) = match dma::alloc(8, 8) {
        Some(pair) => pair,
        None => return,
    };

    for port in 1..=port_count {
        unsafe { core::ptr::write_bytes(buf_virt, 0, 8) };
        let res = super::control_in(
            ctx,
            ep0_ring,
            slot_id,
            setup_get_port_status(port, 4),
            buf_phys,
            4,
            "hub-port-status",
            400,
        )
        .await;

        if res.is_err() {
            crate::log!(
                "usb: hub {:>2} status=ERR\n",
                port
            );
            continue;
        }

        let (status, change) = unsafe {
            let b = core::slice::from_raw_parts(buf_virt, 4);
            let st = u16::from_le_bytes([b[0], b[1]]);
            let ch = u16::from_le_bytes([b[2], b[3]]);
            (st, ch)
        };

        let ccs = (status & (1 << 0)) != 0;
        let ped = (status & (1 << 1)) != 0;
        let pp = (status & (1 << 8)) != 0;
        let low = (status & (1 << 9)) != 0;
        let high = (status & (1 << 10)) != 0;
        let speed = if high { "high" } else if low { "low" } else { "full" };

        let csc = (change & (1 << 0)) != 0;
        let pec = (change & (1 << 1)) != 0;
        let prc = (change & (1 << 4)) != 0;
        let oc = (change & (1 << 3)) != 0;

        crate::log!(
            "usb: hub {:>2} 0x{:04X} 0x{:04X} {:>3} {:>3} {:>2} {:>4} {:>3} {:>3} {:>3} {:>2}\n",
            port,
            status,
            change,
            ccs as u8,
            ped as u8,
            pp as u8,
            speed,
            csc as u8,
            pec as u8,
            prc as u8,
            oc as u8,
        );
    }

    dma::dealloc(buf_virt, 8);
}

fn setup_get_hub_descriptor(desc_type: u8, length: u16) -> Trb {
    // bmRequestType=0xA0 (IN|Class|Device), bRequest=0x06 (GET_DESCRIPTOR)
    let w_value = ((desc_type as u16) << 8) | 0u16;
    Trb {
        d0: (0xA0u32) | (0x06u32 << 8) | ((w_value as u32) << 16),
        d1: (length as u32) << 16,
        d2: 8 | (2 << 16),
        d3: trb_type(2) | (1 << 6),
    }
}

fn setup_get_port_status(port: u8, length: u16) -> Trb {
    // bmRequestType=0xA3 (IN|Class|Other), bRequest=0x00 (GET_STATUS)
    Trb {
        d0: (0xA3u32) | (0x00u32 << 8),
        d1: (port as u32) | ((length as u32) << 16),
        d2: 8 | (2 << 16),
        d3: trb_type(2) | (1 << 6),
    }
}

fn setup_set_port_feature(port: u8, feature: u16) -> Trb {
    // bmRequestType=0x23 (OUT|Class|Other), bRequest=0x03 (SET_FEATURE)
    Trb {
        d0: (0x23u32) | (0x03u32 << 8) | ((feature as u32) << 16),
        d1: (port as u32),
        d2: 8,
        d3: trb_type(2) | (1 << 6),
    }
}
