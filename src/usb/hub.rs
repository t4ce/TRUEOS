use super::{Trb, TrbRing, XhciContext};
use super::xhci::{
    endpoint_target, ep_avg_trb_len_bits, ep_cerr_bits, ep_interval_bits,
    ep_max_esit_payload_lo_bits, ep_max_packet_bits, ep_state_bits, ep_type_bits, trb_type,
    TrbRingState, EP_STATE_DISABLED, EP_TYPE_INT_IN,
};
use super::xhci::MAX_XHCI_CONTROLLERS;
use core::ptr::{read_volatile, write_volatile};
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
    vid: u16,
    pid: u16,
    class: u8,
    subclass: u8,
    protocol: u8,
}

struct HubTopology {
    tree: Option<Tree<UsbNode, USB_TREE_CAPACITY>>,
    root_port_node_ids: Vec<NodeId, LOG_PORTS_MAX>,
    slot_node_ids: Vec<(u32, NodeId), MAX_DEVICES>,
    hub_port_node_ids: Vec<(u32, u8, NodeId), 64>,
    hub_ep0_rings: Vec<(u32, TrbRingState), MAX_DEVICES>,
}

impl HubTopology {
    const fn new() -> Self {
        Self {
            tree: None,
            root_port_node_ids: Vec::new(),
            slot_node_ids: Vec::new(),
            hub_port_node_ids: Vec::new(),
            hub_ep0_rings: Vec::new(),
        }
    }
}

static HUB_TOPOLOGIES: [Mutex<HubTopology>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(HubTopology::new()) }; MAX_XHCI_CONTROLLERS];

pub fn init_topology(ctx: &XhciContext) {
    let controller_id = ctx.controller_id;
    let mut topo = HUB_TOPOLOGIES[controller_id].lock();
    let mut tree = Tree::new();
    let root = match tree.add_root(UsbNode {
        kind: UsbNodeKind::Root,
        vid: 0,
        pid: 0,
        class: 0,
        subclass: 0,
        protocol: 0,
    }) {
        Some(id) => id,
        None => {
            topo.tree = Some(tree);
            return;
        }
    };

    topo.root_port_node_ids.clear();
    topo.slot_node_ids.clear();
    topo.hub_port_node_ids.clear();
    topo.hub_ep0_rings.clear();

    let limit = core::cmp::min(ctx.port_count as usize, LOG_PORTS_MAX);
    for port in 1..=limit {
        let Some(node) = tree.add_child(
            root,
            UsbNode {
                kind: UsbNodeKind::Port,
                vid: 0,
                pid: 0,
                class: 0,
                subclass: 0,
                protocol: 0,
            },
        ) else {
            break;
        };
        let _ = topo.root_port_node_ids.push(node);
    }

    topo.tree = Some(tree);
}

pub fn record_root_device(
    ctx: &XhciContext,
    target_port: u8,
    slot_id: u32,
    dev_vid: u16,
    dev_pid: u16,
    dev_cls: u8,
    dev_sub: u8,
    dev_prot: u8,
) {
    let controller_id = ctx.controller_id;
    let mut topo = HUB_TOPOLOGIES[controller_id].lock();
    let idx = (target_port as usize).saturating_sub(1);
    let Some(port_node) = topo.root_port_node_ids.get(idx).copied() else {
        return;
    };

    let node = {
        let Some(tree) = topo.tree.as_mut() else {
            return;
        };

        let Some(node) = tree.add_child(
            port_node,
            UsbNode {
                kind: UsbNodeKind::Device,
                vid: dev_vid,
                pid: dev_pid,
                class: dev_cls,
                subclass: dev_sub,
                protocol: dev_prot,
            },
        ) else {
            return;
        };
        node
    };

    let _ = topo.slot_node_ids.push((slot_id, node));
}

pub fn record_hub_ports(ctx: &XhciContext, hub_slot_id: u32, port_count: u8) {
    let controller_id = ctx.controller_id;
    let mut topo = HUB_TOPOLOGIES[controller_id].lock();
    let hub_node = topo
        .slot_node_ids
        .iter()
        .find(|(slot, _)| *slot == hub_slot_id)
        .map(|(_, id)| *id);

    let Some(hub_node) = hub_node else {
        return;
    };

    let mut hub_ports = core::mem::take(&mut topo.hub_port_node_ids);
    {
        let Some(tree) = topo.tree.as_mut() else {
            topo.hub_port_node_ids = hub_ports;
            return;
        };

        for port in 1..=port_count {
            if hub_ports.iter().any(|(slot, p, _)| *slot == hub_slot_id && *p == port) {
                continue;
            }
            if let Some(node) = tree.add_child(
                hub_node,
                UsbNode {
                    kind: UsbNodeKind::Port,
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
    topo.hub_port_node_ids = hub_ports;
}

pub fn record_hub_child(
    ctx: &XhciContext,
    hub_slot_id: u32,
    hub_port: u8,
    slot_id: u32,
    dev_vid: u16,
    dev_pid: u16,
    dev_cls: u8,
    dev_sub: u8,
    dev_prot: u8,
) {
    let controller_id = ctx.controller_id;
    let mut topo = HUB_TOPOLOGIES[controller_id].lock();
    let port_node = topo
        .hub_port_node_ids
        .iter()
        .find(|(slot, port, _)| *slot == hub_slot_id && *port == hub_port)
        .map(|(_, _, id)| *id);

    let Some(port_node) = port_node else {
        return;
    };

    let node = {
        let Some(tree) = topo.tree.as_mut() else {
            return;
        };

        let Some(node) = tree.add_child(
            port_node,
            UsbNode {
                kind: UsbNodeKind::Device,
                vid: dev_vid,
                pid: dev_pid,
                class: dev_cls,
                subclass: dev_sub,
                protocol: dev_prot,
            },
        ) else {
            return;
        };
        node
    };

    let _ = topo.slot_node_ids.push((slot_id, node));
}

pub fn register_ep0_ring(ctx: &XhciContext, slot_id: u32, ep0_ring: &TrbRing) {
    let controller_id = ctx.controller_id;
    let state = ep0_ring.snapshot();
    let mut topo = HUB_TOPOLOGIES[controller_id].lock();
    let rings = &mut topo.hub_ep0_rings;
    if let Some(entry) = rings.iter_mut().find(|(slot, _)| *slot == slot_id) {
        entry.1 = state;
        return;
    }
    let _ = rings.push((slot_id, state));
}

fn take_ep0_state(ctx: &XhciContext, slot_id: u32) -> Option<TrbRingState> {
    let controller_id = ctx.controller_id;
    let topo = HUB_TOPOLOGIES[controller_id].lock();
    topo.hub_ep0_rings
        .iter()
        .find(|(slot, _)| *slot == slot_id)
        .map(|(_, s)| *s)
}

pub async fn force_hub_port_reset_via_saved_ep0(
    ctx: &XhciContext,
    hub_slot_id: u32,
    hub_port: u8,
    power_on_good_ms: u16,
    hub_speed_code: u32,
) -> Option<HubPortState> {
    let Some(state) = take_ep0_state(ctx, hub_slot_id) else {
        crate::log!(
            "usb: hub slot {} missing ep0 ring; cannot port-reset port={}\n",
            hub_slot_id,
            hub_port
        );
        return None;
    };
    let mut ring = unsafe { TrbRing::from_state(state) };
    let out = force_port_reset(
        ctx,
        &mut ring,
        hub_slot_id,
        hub_port,
        power_on_good_ms,
        hub_speed_code,
    )
    .await;
    store_ep0_state(ctx, hub_slot_id, ring.snapshot());
    out
}

pub async fn read_hub_port_state_via_saved_ep0(
    ctx: &XhciContext,
    hub_slot_id: u32,
    hub_port: u8,
    hub_speed_code: u32,
) -> Option<HubPortState> {
    let Some(state) = take_ep0_state(ctx, hub_slot_id) else {
        crate::log!(
            "usb: hub slot {} missing ep0 ring; cannot read port status port={}\n",
            hub_slot_id,
            hub_port
        );
        return None;
    };

    let mut ring = unsafe { TrbRing::from_state(state) };
    let (buf_phys, buf_virt) = dma::alloc(8, 8)?;
    let out = read_port_state(
        ctx,
        &mut ring,
        hub_slot_id,
        hub_port,
        buf_phys,
        buf_virt,
        hub_speed_code,
    )
    .await;
    dma::dealloc(buf_virt, 8);
    store_ep0_state(ctx, hub_slot_id, ring.snapshot());
    out
}

#[derive(Copy, Clone, Debug)]
pub struct HubIdentity {
    pub slot_id: u32,
    pub vid: u16,
    pub pid: u16,
    pub protocol: u8,
}

#[derive(Copy, Clone, Debug)]
pub struct DeviceIdentity {
    pub vid: u16,
    pub pid: u16,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
}

pub fn identity_for_slot(controller_id: usize, slot_id: u32) -> Option<DeviceIdentity> {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return None;
    }
    let topo = HUB_TOPOLOGIES[controller_id].lock();
    let tree = topo.tree.as_ref()?;
    let node_id = topo
        .slot_node_ids
        .iter()
        .find(|(slot, _)| *slot == slot_id)
        .map(|(_, id)| *id)?;
    let node = tree.get(node_id)?;
    Some(DeviceIdentity {
        vid: node.vid,
        pid: node.pid,
        class: node.class,
        subclass: node.subclass,
        protocol: node.protocol,
    })
}

pub fn list_hubs_with_saved_ep0(ctx: &XhciContext) -> Vec<HubIdentity, MAX_DEVICES> {
    let controller_id = ctx.controller_id;
    let topo = HUB_TOPOLOGIES[controller_id].lock();
    let Some(tree) = topo.tree.as_ref() else {
        return Vec::new();
    };

    let mut out: Vec<HubIdentity, MAX_DEVICES> = Vec::new();

    for (slot_id, _state) in topo.hub_ep0_rings.iter() {
        let node_id = topo
            .slot_node_ids
            .iter()
            .find(|(slot, _)| *slot == *slot_id)
            .map(|(_, id)| *id);
        let Some(node_id) = node_id else {
            continue;
        };
        let Some(node) = tree.get(node_id) else {
            continue;
        };
        if node.kind != UsbNodeKind::Device {
            continue;
        }
        if node.class != USB_CLASS_HUB {
            continue;
        }
        let _ = out.push(HubIdentity {
            slot_id: *slot_id,
            vid: node.vid,
            pid: node.pid,
            protocol: node.protocol,
        });
    }

    out
}

fn store_ep0_state(ctx: &XhciContext, slot_id: u32, state: TrbRingState) {
    let controller_id = ctx.controller_id;
    let mut topo = HUB_TOPOLOGIES[controller_id].lock();
    let rings = &mut topo.hub_ep0_rings;
    if let Some(entry) = rings.iter_mut().find(|(slot, _)| *slot == slot_id) {
        entry.1 = state;
        return;
    }
    let _ = rings.push((slot_id, state));
}

pub struct AttachParams<'a> {
    pub ctx: &'a XhciContext,
    pub ep0_ring: &'a mut TrbRing,
    pub slot_id: u32,
    pub cfg: &'a [u8],
    pub target_port: u8,
    pub dev_prot: u8,
}

#[derive(Copy, Clone, Debug)]
pub struct HubWork {
    pub hub_slot_id: u32,
    pub root_port: u8,
    pub route_string: u32,
    pub depth: u8,
    pub hub_speed_code: u32,
    pub multi_tt: bool,
    pub port_count: u8,
    pub power_on_good_ms: u16,
    pub tt_think_time: u8,
}

#[derive(Copy, Clone, Debug)]
pub struct HubChild {
    pub port: u8,
    pub route: u32,
    pub depth: u8,
    pub speed_code: u32,
    pub tt_info: Option<(u32, u8)>,
    pub tt_think_time: u8,
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
        cfg,
        target_port,
        dev_prot,
    } = params;

    let config_value = parse_first_config_value(cfg).unwrap_or(1);
    if super::control_out(
        ctx,
        ep0_ring,
        slot_id,
        setup_set_configuration(config_value),
        None,
        0,
        "hub-set-configuration",
        800,
    )
    .await
    .is_err()
    {
        crate::log!(
            "usb: hub claimed slot={} port={} (set configuration failed)\n",
            slot_id,
            target_port
        );
        return Err(());
    }

    const HUB_CONFIG_SETTLE_MS: u64 = 10;
    if HUB_CONFIG_SETTLE_MS > 0 {
        Timer::after(EmbassyDuration::from_millis(HUB_CONFIG_SETTLE_MS)).await;
    }

    // For USB3 hubs (bDeviceProtocol=3), ask for the SuperSpeed Hub Descriptor first.
    // Many SS hubs will STALL if we probe the USB2 hub descriptor type (0x29) first.
    let prefer_ss = dev_prot == 3;
    let Some(desc) = read_hub_descriptor(ctx, ep0_ring, slot_id, prefer_ss).await else {
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

    Ok(desc)
}

pub struct HubConfigParams<'a> {
    pub ctx: &'a XhciContext,
    pub cmd_ring: &'a mut TrbRing,
    pub slot_id: u32,
    pub dev_ctx_virt: *mut u8,
    pub ctx_stride_bytes: usize,
    pub ctx_stride_words: usize,
    pub target_port: u8,
    pub port_count: u8,
    pub tt_think_time: u8,
    pub multi_tt: bool,
}

unsafe fn copy_slot_ep0_contexts(
    dev_ctx_virt: *mut u8,
    input_cfg_virt: *mut u8,
    ctx_stride_bytes: usize,
    ctx_stride_words: usize,
) {
    let slot_ctx = input_cfg_virt.add(ctx_stride_bytes) as *mut u32;
    let dev_slot_ctx = dev_ctx_virt as *const u32;
    for i in 0..ctx_stride_words {
        write_volatile(slot_ctx.add(i), read_volatile(dev_slot_ctx.add(i)));
    }

    let ep0_src = dev_ctx_virt.add(ctx_stride_bytes) as *const u32;
    let ep0_dst = input_cfg_virt.add(ctx_stride_bytes * 2) as *mut u32;
    for i in 0..ctx_stride_words {
        write_volatile(ep0_dst.add(i), read_volatile(ep0_src.add(i)));
    }
}

pub async fn configure_hub_context(params: HubConfigParams<'_>) -> Result<(), ()> {
    let HubConfigParams {
        ctx,
        cmd_ring,
        slot_id,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        target_port,
        port_count,
        tt_think_time,
        multi_tt,
    } = params;

    let (input_cfg_phys, input_cfg_virt) = dma::alloc(4096, 64).ok_or(())?;
    unsafe { core::ptr::write_bytes(input_cfg_virt, 0, 4096) };

    let mut in_add_flags: u32 = 0;
    let mut in_dw0: u32 = 0;
    let mut in_dw1: u32 = 0;
    let mut in_dw2: u32 = 0;

    unsafe {
        let add_flags_ptr = input_cfg_virt as *mut u32;
        // For Evaluate Context, include Slot + EP0. Some controllers fail to
        // latch hub fields unless EP0 is present in the input context.
        write_volatile(add_flags_ptr.add(1), 0x3);
        in_add_flags = read_volatile(add_flags_ptr.add(1));

        copy_slot_ep0_contexts(dev_ctx_virt, input_cfg_virt, ctx_stride_bytes, ctx_stride_words);
        let slot_ctx = input_cfg_virt.add(ctx_stride_bytes) as *mut u32;

        let mut dw0 = read_volatile(slot_ctx.add(0));
        dw0 |= 1 << 26; // Hub bit
        if multi_tt {
            dw0 |= 1 << 25; // MTT bit
        }
        write_volatile(slot_ctx.add(0), dw0);

        let mut dw1 = read_volatile(slot_ctx.add(1));
        dw1 = (dw1 & !(0xFF << 16)) | ((target_port as u32) << 16);
        dw1 = (dw1 & !(0xFF << 24)) | ((port_count as u32) << 24);
        write_volatile(slot_ctx.add(1), dw1);

        if tt_think_time != 0 {
            let mut dw2 = read_volatile(slot_ctx.add(2));
            dw2 = (dw2 & !(0x3 << 16)) | (((tt_think_time as u32) & 0x3) << 16);
            write_volatile(slot_ctx.add(2), dw2);
        }

        in_dw0 = read_volatile(slot_ctx.add(0));
        in_dw1 = read_volatile(slot_ctx.add(1));
        in_dw2 = read_volatile(slot_ctx.add(2));
    }

    if super::USB_LOG_VERBOSE {
        crate::log!(
            "usb: hub eval-ctx input slot={} add=0x{:08X} slot_dw0=0x{:08X} slot_dw1=0x{:08X} slot_dw2=0x{:08X} (hub_bit={} mtt={} ports={} tt_think={})\n",
            slot_id,
            in_add_flags,
            in_dw0,
            in_dw1,
            in_dw2,
            (in_dw0 >> 26) & 1,
            (in_dw0 >> 25) & 1,
            (in_dw1 >> 24) & 0xFF,
            (in_dw2 >> 16) & 0x3,
        );
    }

    // Evaluate Context is the intended mechanism for updating slot-context fields
    // (Hub bit, MTT, number of ports, TT think time) after a device is addressed.
    let eval_ctx_cmd = Trb {
        d0: super::xhci::lo(input_cfg_phys),
        d1: super::xhci::hi(input_cfg_phys),
        d2: 0,
        d3: trb_type(13) | (slot_id << 24),
    };
    let _ = super::xhci::submit_cmd_and_wait(
        ctx,
        cmd_ring,
        eval_ctx_cmd,
        Some(slot_id),
        "hub-eval-ctx",
        600,
        EmbassyDuration::from_millis(5),
    )
    .await?;

    // If the controller doesn't reflect the hub fields in the output Slot Context
    // after a successful Evaluate Context, attempt to "latch" the slot context via
    // Address Device with BSR=1 (no SET_ADDRESS on the bus). Some emulations/controllers
    // appear picky about when hub slot fields become effective.
    unsafe {
        let out_slot = dev_ctx_virt as *const u32;
        let out_dw0 = read_volatile(out_slot.add(0));
        let out_dw1 = read_volatile(out_slot.add(1));
        let out_hub_bit = (out_dw0 >> 26) & 1;
        let out_ports = (out_dw1 >> 24) & 0xFF;
        if out_hub_bit == 0 || out_ports == 0 {
            crate::log!(
                "usb: hub slot {} eval-ctx did not reflect hub fields (hub_bit={} ports={}); trying addr-dev BSR=1\n",
                slot_id,
                out_hub_bit,
                out_ports
            );

            // Reuse the same DMA buffer: rebuild it as a slot+EP0 input context.
            write_volatile((input_cfg_virt as *mut u32).add(1), 0x3);

            // Slot + EP0 contexts already prepared above.

            let addr_dev_cmd = Trb {
                d0: super::xhci::lo(input_cfg_phys),
                d1: super::xhci::hi(input_cfg_phys),
                d2: 0,
                d3: trb_type(11) | (1 << 9) | (slot_id << 24),
            };

            let _ = super::xhci::submit_cmd_and_wait(
                ctx,
                cmd_ring,
                addr_dev_cmd,
                Some(slot_id),
                "hub-addrdev-bsr",
                800,
                EmbassyDuration::from_millis(5),
            )
            .await?;
        }
    }

    dma::dealloc(input_cfg_virt, 4096);
    Ok(())
}

fn find_hub_interrupt_ep(cfg: &[u8]) -> Option<(u8, u16, u8)> {
    let mut idx = 0usize;
    let mut in_hub_if = false;
    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        let ty = cfg[idx + 1];
        match ty {
            4 if len >= 9 => {
                let if_cls = cfg[idx + 5];
                let if_sub = cfg[idx + 6];
                in_hub_if = if_cls == USB_CLASS_HUB && if_sub == USB_SUBCLASS_HUB;
            }
            5 if in_hub_if && len >= 7 => {
                let ep_addr = cfg[idx + 2];
                let attrs = cfg[idx + 3];
                let max_packet = u16::from_le_bytes([cfg[idx + 4], cfg[idx + 5]]);
                let interval = cfg[idx + 6];
                let ep_type = attrs & 0x3;
                let ep_in = (ep_addr & 0x80) != 0;
                if ep_type == 3 && ep_in {
                    return Some((ep_addr, max_packet, interval));
                }
            }
            _ => {}
        }
        idx += len;
    }
    None
}

pub struct HubInterruptParams<'a> {
    pub ctx: &'a XhciContext,
    pub cmd_ring: &'a mut TrbRing,
    pub slot_id: u32,
    pub dev_ctx_virt: *mut u8,
    pub ctx_stride_bytes: usize,
    pub ctx_stride_words: usize,
    pub target_port: u8,
    pub port_count: u8,
    pub tt_think_time: u8,
    pub multi_tt: bool,
    pub speed_code: u32,
    pub cfg: &'a [u8],
}

pub async fn configure_hub_interrupt(params: HubInterruptParams<'_>) -> Result<(), ()> {
    let HubInterruptParams {
        ctx,
        cmd_ring,
        slot_id,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        target_port,
        port_count,
        tt_think_time,
        multi_tt,
        speed_code,
        cfg,
    } = params;

    let Some((ep_addr, max_packet, interval)) = find_hub_interrupt_ep(cfg) else {
        return Err(());
    };

    const HUB_INT_TRBS: usize = 32;
    let (ep_ring_phys, ep_ring_virt) = dma::alloc(HUB_INT_TRBS * core::mem::size_of::<Trb>(), 64)
        .ok_or(())?;
    unsafe { core::ptr::write_bytes(ep_ring_virt, 0, HUB_INT_TRBS * core::mem::size_of::<Trb>()) };
    let ep_ring = unsafe { TrbRing::new(ep_ring_phys, ep_ring_virt as *mut Trb, HUB_INT_TRBS) };

    let (input_cfg_phys, input_cfg_virt) = dma::alloc(4096, 64).ok_or(())?;
    unsafe { core::ptr::write_bytes(input_cfg_virt, 0, 4096) };

    let dci = endpoint_target(ep_addr);
    // In the xHCI Input Context, index 0 is the Input Control Context,
    // index 1 is the Slot Context, and EP0 is DCI=1 at index 2.
    let ep_ctx_index = dci + 1;
    // Add Context flags are indexed by Device Context Index (DCI):
    // bit0=slot, bit1=EP0 (DCI=1), bit2=EP1 OUT (DCI=2), bit3=EP1 IN (DCI=3), ...
    let ep_add_bit = dci;

    unsafe {
        let add_flags_ptr = input_cfg_virt as *mut u32;
        // Match the proven HID Configure Endpoint path: add Slot + the target endpoint.
        write_volatile(add_flags_ptr.add(1), (1 << 0) | (1 << ep_add_bit));

        let slot_ctx = input_cfg_virt.add(ctx_stride_bytes) as *mut u32;
        // Slot context is at index 1; endpoint context for DCI=N is at index (1 + N).
        let ep_ctx_off: usize = ctx_stride_bytes * (ep_ctx_index as usize);
        let ep_ctx = input_cfg_virt.add(ep_ctx_off) as *mut u32;

        copy_slot_ep0_contexts(dev_ctx_virt, input_cfg_virt, ctx_stride_bytes, ctx_stride_words);

        let mut dw0 = read_volatile(slot_ctx.add(0));
        dw0 |= 1 << 26;
        if multi_tt {
            dw0 |= 1 << 25;
        }
        // Context Entries is the highest valid DCI.
        dw0 = (dw0 & !(0x1F << 27)) | ((dci as u32) << 27);
        write_volatile(slot_ctx.add(0), dw0);

        let mut dw1 = read_volatile(slot_ctx.add(1));
        dw1 = (dw1 & !(0xFF << 16)) | ((target_port as u32) << 16);
        dw1 = (dw1 & !(0xFF << 24)) | ((port_count as u32) << 24);
        write_volatile(slot_ctx.add(1), dw1);

        if tt_think_time != 0 {
            let mut dw2 = read_volatile(slot_ctx.add(2));
            dw2 = (dw2 & !(0x3 << 16)) | (((tt_think_time as u32) & 0x3) << 16);
            write_volatile(slot_ctx.add(2), dw2);
        }

        let interval_field = if speed_code == 3 {
            core::cmp::min(15u32, interval.saturating_sub(1) as u32)
        } else {
            interval as u32
        };

        write_volatile(
            ep_ctx.add(0),
            ep_state_bits(EP_STATE_DISABLED) | ep_interval_bits(interval_field),
        );
        let mut ep_cfg = ep_cerr_bits(3);
        ep_cfg |= ep_type_bits(EP_TYPE_INT_IN);
        ep_cfg |= ep_max_packet_bits((max_packet as u32) & 0x7FF);
        write_volatile(ep_ctx.add(1), ep_cfg);
        let dq = ep_ring.dequeue_ptr();
        write_volatile(ep_ctx.add(2), super::xhci::lo(dq));
        write_volatile(ep_ctx.add(3), super::xhci::hi(dq));

        let mps = (max_packet as u32) & 0x7FF;
        write_volatile(
            ep_ctx.add(4),
            ep_avg_trb_len_bits(mps) | ep_max_esit_payload_lo_bits(mps),
        );
    }

    let cfg_ep_cmd = Trb {
        d0: super::xhci::lo(input_cfg_phys),
        d1: super::xhci::hi(input_cfg_phys),
        d2: 0,
        d3: trb_type(12) | (slot_id << 24),
    };
    let _ = super::xhci::submit_cmd_and_wait(
        ctx,
        cmd_ring,
        cfg_ep_cmd,
        Some(slot_id),
        "hub-config-ep",
        600,
        EmbassyDuration::from_millis(5),
    )
    .await?;
    dma::dealloc(input_cfg_virt, 4096);
    // Keep the ring memory alive by not deallocating; no runtime use yet.
    let _ = dci;

    Ok(())
}


#[derive(Copy, Clone, Debug)]
pub struct HubDescriptorInfo {
    pub desc_type: u8,
    pub port_count: u8,
    pub power_on_good_ms: u16,
    pub tt_think_time: u8,
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

fn ss_link_state(status: u16) -> u8 {
    ((status >> 5) & 0xF) as u8
}

async fn read_hub_descriptor(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    prefer_ss: bool,
) -> Option<HubDescriptorInfo> {
    // TODO(usb): ~86% accuracy / robustness note
    // Strategy: for USB3 hubs (bDeviceProtocol==3) we probe the SS hub descriptor first
    // (type 0x2A, 12 bytes) and fall back to the USB2 hub descriptor (0x29, 9 bytes).
    // Reason: many SS hubs will STALL if we probe 0x29 first; without EP0 STALL recovery
    // (CLEAR_FEATURE(ENDPOINT_HALT) on EP0, then retry), a single wrong-first probe can
    // poison the subsequent control transfers and cause repeated re-enumeration.
    let (buf_phys, buf_virt) = dma::alloc(64, 64)?;
    unsafe { core::ptr::write_bytes(buf_virt, 0, 64) };

    // USB2 hub descriptor (HUB): 0x29, SS hub descriptor (SS_HUB): 0x2A.
    // SS hub descriptors have a larger fixed header (12 bytes) and some hubs are
    // picky about the requested length.
    let tries: &[(u8, u16)] = if prefer_ss {
        &[(0x2A, 12), (0x29, 9)]
    } else {
        &[(0x29, 9), (0x2A, 12)]
    };

    let mut used_type: u8 = tries[0].0;
    let mut transferred: u16 = 0;
    let mut ok = false;
    for (ty, len) in tries.iter().copied() {
        used_type = ty;
        transferred = 0;
        if let Ok((_cc, xfer)) = super::control_in(
            ctx,
            ep0_ring,
            slot_id,
            setup_get_hub_descriptor(ty, len),
            buf_phys,
            len,
            "hub-desc",
            800,
        )
        .await
        {
            transferred = xfer;
            ok = true;
            break;
        }

        // If the first probe STALLs on some devices, the safest fix is to avoid
        // probing the wrong descriptor first (handled by `prefer_ss`). If we still
        // fail here, we fall through and report failure.
    }

    let info = if ok {
        unsafe {
            let want = (transferred as usize).min(64);
            let b = core::slice::from_raw_parts(buf_virt, want);
            if b.len() >= 3 {
                // Prefer what the device actually returned.
                let desc_type = if b.len() >= 2 { b[1] } else { used_type };
                let w_hub_chars = if b.len() >= 5 {
                    u16::from_le_bytes([b[3], b[4]])
                } else {
                    0
                };
                let tt_think_time = if desc_type == 0x2A {
                    0
                } else {
                    ((w_hub_chars >> 5) & 0x3) as u8
                };
                let raw_delay = if b.len() >= 6 { b[5] } else { 0 };
                let mut power_on_good_ms = (raw_delay as u16) * 2;
                if power_on_good_ms == 0 {
                    power_on_good_ms = 20;
                }
                Some(HubDescriptorInfo {
                    desc_type,
                    port_count: b[2],
                    power_on_good_ms,
                    tt_think_time,
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
    power_on_good_ms: u16,
    hub_speed_code: u32,
) -> Vec<HubPortState, 32> {
    let mut out: Vec<HubPortState, 32> = Vec::new();
    let (buf_phys, buf_virt) = match dma::alloc(8, 8) {
        Some(pair) => pair,
        None => return out,
    };

    for port in 1..=port_count {
        let _ = set_port_feature(ctx, ep0_ring, slot_id, port, HUB_FEATURE_PORT_POWER).await;
    }
    let delay_ms = core::cmp::max(20, power_on_good_ms as u64);
    Timer::after(EmbassyDuration::from_millis(delay_ms)).await;

    let mut failed: Vec<u8, 32> = Vec::new();
    for port in 1..=port_count {
        let mut attempt = 0u8;
        let mut state_opt = None;
        while attempt < 3 {
            attempt += 1;
            if let Some(state) =
                read_port_state(ctx, ep0_ring, slot_id, port, buf_phys, buf_virt, hub_speed_code)
                    .await
            {
                state_opt = Some(state);
                break;
            }
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }

        if let Some(state) = state_opt {
            let _ = clear_port_change_bits(ctx, ep0_ring, slot_id, port, state.change).await;
            let _ = out.push(state);
        } else {
            crate::log!("usb: hub port {} status read failed\n", port);
            let _ = failed.push(port);
        }
        Timer::after(EmbassyDuration::from_millis(2)).await;
    }

    if !failed.is_empty() {
        for port in failed.iter().copied() {
            crate::log!("usb: hub port {} status read failed (skipped)\n", port);
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
    power_on_good_ms: u16,
    hub_speed_code: u32,
) -> Option<HubPortState> {
    let (buf_phys, buf_virt) = dma::alloc(8, 8)?;

    let _ = set_port_feature(ctx, ep0_ring, slot_id, port, HUB_FEATURE_PORT_POWER).await;
    let delay_ms = core::cmp::max(20, power_on_good_ms as u64);
    Timer::after(EmbassyDuration::from_millis(delay_ms)).await;

    let mut last = None;
    let mut attempts = 0u32;
    while attempts < 4 {
        attempts += 1;

        if let Some(state) =
            read_port_state(ctx, ep0_ring, slot_id, port, buf_phys, buf_virt, hub_speed_code).await
        {
            let saw_connect_change = (state.change & 0x0001) != 0;
            let need_reset = state.connected && (!state.enabled || saw_connect_change);
            if state.connected {
                if hub_speed_code >= 4 {
                    crate::log!(
                        "usb: hub port {} pre status=0x{:04X} change=0x{:04X} ped={} pls={}\n",
                        port,
                        state.status,
                        state.change,
                        state.enabled as u8,
                        ss_link_state(state.status),
                    );
                } else {
                    crate::log!(
                        "usb: hub port {} pre status=0x{:04X} change=0x{:04X} ped={}\n",
                        port,
                        state.status,
                        state.change,
                        state.enabled as u8,
                    );
                }
            }

            if (state.status & (1 << 8)) == 0 {
                let _ = set_port_feature(ctx, ep0_ring, slot_id, port, HUB_FEATURE_PORT_POWER).await;
                Timer::after(EmbassyDuration::from_millis(20)).await;
            }

            // Some hubs will report PED=1 immediately after a connect change.
            // Still force a reset on connect-change so the downstream device
            // reliably enters Default state before we issue Address Device.
            // Also, perform the reset before clearing change bits; some hubs
            // appear sensitive to the ordering here.
            if need_reset {
                let _ = set_port_feature(ctx, ep0_ring, slot_id, port, HUB_FEATURE_PORT_RESET).await;
                let mut wait = 0u32;
                while wait < 12 {
                    Timer::after(EmbassyDuration::from_millis(20)).await;
                    wait += 1;
                    if let Some(after_reset) = read_port_state(
                        ctx,
                        ep0_ring,
                        slot_id,
                        port,
                        buf_phys,
                        buf_virt,
                        hub_speed_code,
                    )
                    .await
                    {
                        let reset_active = (after_reset.status & (1 << 4)) != 0;
                        let _ = clear_port_change_bits(ctx, ep0_ring, slot_id, port, after_reset.change).await;
                        let ss_pls_ok = if hub_speed_code >= 4 {
                            // For SS hubs, wait until the link settles into a normal state.
                            // U0/U1/U2 are fine; transient states (Polling/Recovery/Hot Reset) are not.
                            matches!(ss_link_state(after_reset.status), 0 | 1 | 2)
                        } else {
                            true
                        };

                        if after_reset.connected && after_reset.enabled && !reset_active && ss_pls_ok {
                            crate::log!(
                                "usb: hub port {} reset ok status=0x{:04X} change=0x{:04X}\n",
                                port,
                                after_reset.status,
                                after_reset.change,
                            );
                            let settle_ms = match after_reset.speed_code {
                                1 | 2 => 50,
                                3 => 10,
                                _ => 80,
                            };
                            Timer::after(EmbassyDuration::from_millis(settle_ms)).await;
                            last = Some(after_reset);
                            break;
                        }
                    }
                }
                if let Some(last_state) = last {
                    if last_state.connected && last_state.enabled {
                        break;
                    }
                }

                let _ = set_port_feature(ctx, ep0_ring, slot_id, port, HUB_FEATURE_PORT_ENABLE).await;
                Timer::after(EmbassyDuration::from_millis(20)).await;
                if let Some(after_enable) = read_port_state(
                    ctx,
                    ep0_ring,
                    slot_id,
                    port,
                    buf_phys,
                    buf_virt,
                    hub_speed_code,
                )
                .await
                {
                    if hub_speed_code >= 4 {
                        crate::log!(
                            "usb: hub port {} enable status=0x{:04X} change=0x{:04X} ped={} pls={}\n",
                            port,
                            after_enable.status,
                            after_enable.change,
                            after_enable.enabled as u8,
                            ss_link_state(after_enable.status),
                        );
                    } else {
                        crate::log!(
                            "usb: hub port {} enable status=0x{:04X} change=0x{:04X} ped={}\n",
                            port,
                            after_enable.status,
                            after_enable.change,
                            after_enable.enabled as u8,
                        );
                    }
                    let _ = clear_port_change_bits(ctx, ep0_ring, slot_id, port, after_enable.change).await;
                    if after_enable.connected && after_enable.enabled {
                        last = Some(after_enable);
                        break;
                    }
                }
            }

            let _ = clear_port_change_bits(ctx, ep0_ring, slot_id, port, state.change).await;
        }

        if let Some(state) =
            read_port_state(ctx, ep0_ring, slot_id, port, buf_phys, buf_virt, hub_speed_code).await
        {
            last = Some(state);
            if state.connected && state.enabled {
                break;
            }
        } else {
            crate::log!("usb: hub port {} status read failed during enable\n", port);
        }

        Timer::after(EmbassyDuration::from_millis(20)).await;
    }

    if let Some(state) = last {
        if !(state.connected && state.enabled) {
            crate::log!(
                "usb: hub port {} enable give-up status=0x{:04X} change=0x{:04X} ccs={} ped={} speed_code={}\n",
                port,
                state.status,
                state.change,
                state.connected as u8,
                state.enabled as u8,
                state.speed_code,
            );
        }
    } else {
        crate::log!("usb: hub port {} enable give-up (no status)\n", port);
    }

    dma::dealloc(buf_virt, 8);
    last
}

async fn force_port_reset(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    port: u8,
    power_on_good_ms: u16,
    hub_speed_code: u32,
) -> Option<HubPortState> {
    let (buf_phys, buf_virt) = dma::alloc(8, 8)?;

    let _ = set_port_feature(ctx, ep0_ring, slot_id, port, HUB_FEATURE_PORT_POWER).await;
    let delay_ms = core::cmp::max(20, power_on_good_ms as u64);
    Timer::after(EmbassyDuration::from_millis(delay_ms)).await;

    let mut last = None;

    let Some(state) =
        read_port_state(ctx, ep0_ring, slot_id, port, buf_phys, buf_virt, hub_speed_code).await
    else {
        dma::dealloc(buf_virt, 8);
        return None;
    };
    last = Some(state);
    if state.connected {
        if hub_speed_code >= 4 {
            crate::log!(
                "usb: hub port {} pre status=0x{:04X} change=0x{:04X} ped={} pls={}\n",
                port,
                state.status,
                state.change,
                state.enabled as u8,
                ss_link_state(state.status),
            );
        } else {
            crate::log!(
                "usb: hub port {} pre status=0x{:04X} change=0x{:04X} ped={}\n",
                port,
                state.status,
                state.change,
                state.enabled as u8,
            );
        }
    }

    if !state.connected {
        dma::dealloc(buf_virt, 8);
        return last;
    }

    let _ = set_port_feature(ctx, ep0_ring, slot_id, port, HUB_FEATURE_PORT_RESET).await;
    let mut wait = 0u32;
    while wait < 12 {
        Timer::after(EmbassyDuration::from_millis(20)).await;
        wait += 1;
        if let Some(after_reset) =
            read_port_state(ctx, ep0_ring, slot_id, port, buf_phys, buf_virt, hub_speed_code).await
        {
            last = Some(after_reset);
            let reset_active = (after_reset.status & (1 << 4)) != 0;
            let _ = clear_port_change_bits(ctx, ep0_ring, slot_id, port, after_reset.change).await;
            let ss_pls_ok = if hub_speed_code >= 4 {
                matches!(ss_link_state(after_reset.status), 0 | 1 | 2)
            } else {
                true
            };

            if after_reset.connected && after_reset.enabled && !reset_active && ss_pls_ok {
                crate::log!(
                    "usb: hub port {} reset ok status=0x{:04X} change=0x{:04X}\n",
                    port,
                    after_reset.status,
                    after_reset.change,
                );
                let settle_ms = match after_reset.speed_code {
                    1 | 2 => 50,
                    3 => 10,
                    _ => 250,
                };
                Timer::after(EmbassyDuration::from_millis(settle_ms)).await;
                dma::dealloc(buf_virt, 8);
                return Some(after_reset);
            }
        }
    }

    // If reset didn't converge, try a final PORT_ENABLE poke.
    let _ = set_port_feature(ctx, ep0_ring, slot_id, port, HUB_FEATURE_PORT_ENABLE).await;
    Timer::after(EmbassyDuration::from_millis(20)).await;
    if let Some(after_enable) =
        read_port_state(ctx, ep0_ring, slot_id, port, buf_phys, buf_virt, hub_speed_code).await
    {
        last = Some(after_enable);
        let _ = clear_port_change_bits(ctx, ep0_ring, slot_id, port, after_enable.change).await;
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
    multi_tt: bool,
    port_count: u8,
    power_on_good_ms: u16,
    hub_tt_think_time: u8,
) -> Vec<HubChild, 32> {
    let mut out: Vec<HubChild, 32> = Vec::new();
    if depth >= 5 {
        return out;
    }

    let Some(ep0_state) = take_ep0_state(ctx, hub_slot_id) else {
        crate::log!("usb: hub slot {} missing ep0 ring; cannot scan ports\n", hub_slot_id);
        return out;
    };
    let mut ep0_ring = unsafe { TrbRing::from_state(ep0_state) };

    let ports =
        scan_ports(ctx, &mut ep0_ring, hub_slot_id, port_count, power_on_good_ms, hub_speed_code)
            .await;
    for port_state in ports.iter() {
        crate::log!(
            "usb: hub child scan slot={} port={} status=0x{:04X} change=0x{:04X} ccs={} ped={} speed_code={}\n",
            hub_slot_id,
            port_state.port,
            port_state.status,
            port_state.change,
            port_state.connected as u8,
            port_state.enabled as u8,
            port_state.speed_code,
        );
        if !port_state.connected {
            continue;
        }

        let Some(enabled) = ensure_port_enabled(
            ctx,
            &mut ep0_ring,
            hub_slot_id,
            port_state.port,
            power_on_good_ms,
            hub_speed_code,
        )
        .await
        else {
            crate::log!(
                "usb: hub child enable failed hub_slot={} port={}\n",
                hub_slot_id,
                port_state.port
            );
            continue;
        };

        crate::log!(
            "usb: hub child enabled hub_slot={} port={} status=0x{:04X} change=0x{:04X} ped={} speed_code={}\n",
            hub_slot_id,
            port_state.port,
            enabled.status,
            enabled.change,
            enabled.enabled as u8,
            enabled.speed_code,
        );

        if !enabled.connected {
            crate::log!(
                "usb: hub child not connected after enable hub_slot={} port={}\n",
                hub_slot_id,
                port_state.port
            );
            continue;
        }

        // xHCI Route String is a 20-bit value made of up to 5 4-bit hub port nibbles.
        // The first tier behind the root hub occupies bits 3:0, then 7:4, etc.
        let shift = (depth as u32) * 4;
        let route = (parent_route & !(0xFu32 << shift))
            | (((port_state.port as u32) & 0xF) << shift);
        let speed_code = enabled.speed_code;
        let tt_info = if speed_code <= 2 && hub_speed_code == 3 {
            Some((hub_slot_id, port_state.port))
        } else {
            None
        };
        let tt_think_time = if tt_info.is_some() { hub_tt_think_time } else { 0 };

        let _ = out.push(HubChild {
            port: port_state.port,
            route,
            depth: depth.saturating_add(1),
            speed_code,
            tt_info,
            tt_think_time,
        });
    }

    store_ep0_state(ctx, hub_slot_id, ep0_ring.snapshot());
    out
}

const HUB_FEATURE_PORT_ENABLE: u16 = 1;
const HUB_FEATURE_PORT_RESET: u16 = 4;
const HUB_FEATURE_PORT_POWER: u16 = 8;
const HUB_FEATURE_C_PORT_CONNECTION: u16 = 16;
const HUB_FEATURE_C_PORT_ENABLE: u16 = 17;
const HUB_FEATURE_C_PORT_OVER_CURRENT: u16 = 19;
const HUB_FEATURE_C_PORT_RESET: u16 = 20;

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

async fn clear_port_feature(
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
        setup_clear_port_feature(port, feature),
        None,
        0,
        "hub-clear-port-feature",
        200,
    )
    .await
    .map(|_| ())
}

async fn clear_port_change_bits(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    port: u8,
    change: u16,
) -> Result<(), ()> {
    if (change & (1 << 0)) != 0 {
        let _ = clear_port_feature(ctx, ep0_ring, slot_id, port, HUB_FEATURE_C_PORT_CONNECTION).await;
    }
    if (change & (1 << 1)) != 0 {
        let _ = clear_port_feature(ctx, ep0_ring, slot_id, port, HUB_FEATURE_C_PORT_ENABLE).await;
    }
    if (change & (1 << 3)) != 0 {
        let _ = clear_port_feature(ctx, ep0_ring, slot_id, port, HUB_FEATURE_C_PORT_OVER_CURRENT).await;
    }
    if (change & (1 << 4)) != 0 {
        let _ = clear_port_feature(ctx, ep0_ring, slot_id, port, HUB_FEATURE_C_PORT_RESET).await;
    }
    Ok(())
}

async fn read_port_state(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    port: u8,
    buf_phys: u64,
    buf_virt: *mut u8,
    hub_speed_code: u32,
) -> Option<HubPortState> {
    let mut attempt = 0u8;
    while attempt < 3 {
        attempt += 1;
        unsafe { core::ptr::write_bytes(buf_virt, 0, 8) };
        if super::control_in(
            ctx,
            ep0_ring,
            slot_id,
            setup_get_port_status(port, 4),
            buf_phys,
            4,
            "hub-port-status",
            200,
        )
        .await
        .is_ok()
        {
            break;
        }

        if attempt < 3 {
            Timer::after(EmbassyDuration::from_millis(5)).await;
        } else {
            return None;
        }
    }

    let (status, change) = unsafe {
        let b = core::slice::from_raw_parts(buf_virt, 4);
        let st = u16::from_le_bytes([b[0], b[1]]);
        let ch = u16::from_le_bytes([b[2], b[3]]);
        (st, ch)
    };

    let connected = (status & (1 << 0)) != 0;
    let enabled = (status & (1 << 1)) != 0;

    // USB2 hub port status encodes LS/HS in bits 9/10. USB3 hub port status uses
    // different semantics; those bits are not reliable there. If this hub is SS
    // (xHCI speed ID 4/5), treat downstream devices as SS.
    let speed_code = if hub_speed_code >= 4 {
        hub_speed_code
    } else {
        let low = (status & (1 << 9)) != 0;
        let high = (status & (1 << 10)) != 0;
        if high { 3 } else if low { 2 } else { 1 }
    };

    Some(HubPortState {
        port,
        status,
        change,
        connected,
        enabled,
        speed_code,
    })
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

fn setup_set_configuration(value: u8) -> Trb {
    // bmRequestType=0x00 (OUT|Standard|Device), bRequest=0x09 (SET_CONFIGURATION)
    Trb {
        d0: (0x00u32) | (0x09u32 << 8) | ((value as u32) << 16),
        d1: 0,
        d2: 8,
        d3: trb_type(2) | (1 << 6),
    }
}

fn parse_first_config_value(cfg: &[u8]) -> Option<u8> {
    let mut idx = 0usize;
    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        let ty = cfg[idx + 1];
        if ty == 2 && len >= 6 {
            return Some(cfg[idx + 5]);
        }
        idx += len;
    }
    None
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

fn setup_clear_port_feature(port: u8, feature: u16) -> Trb {
    // bmRequestType=0x23 (OUT|Class|Other), bRequest=0x01 (CLEAR_FEATURE)
    Trb {
        d0: (0x23u32) | (0x01u32 << 8) | ((feature as u32) << 16),
        d1: (port as u32),
        d2: 8,
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
