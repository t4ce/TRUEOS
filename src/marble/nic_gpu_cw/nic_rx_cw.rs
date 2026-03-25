use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use super::{
    MarbleGadget, MeaningfulUdpPayloadMarble, NicGpuUniverseId, NicRxControlMarble,
    PciFunctionAddress,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NicRxHole {
    pub index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NicRxNodeKind {
    WhiteHole,
    FrameParser,
    UdpSelector,
    UdpExtractor,
    BlackHole,
}

impl NicRxNodeKind {
    pub const fn name(self) -> &'static str {
        match self {
            NicRxNodeKind::WhiteHole => "white-hole",
            NicRxNodeKind::FrameParser => "frame-parser-widget",
            NicRxNodeKind::UdpSelector => "udp-selector-widget",
            NicRxNodeKind::UdpExtractor => "udp-extractor-widget",
            NicRxNodeKind::BlackHole => "black-hole",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NicRxLaneKind {
    FrameBytes,
    Control1,
    MeaningfulPayload,
}

impl NicRxLaneKind {
    pub const fn name(self) -> &'static str {
        match self {
            NicRxLaneKind::FrameBytes => "frame-bytes",
            NicRxLaneKind::Control1 => "control[1]",
            NicRxLaneKind::MeaningfulPayload => "meaningful-payload",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NicRxTopologyNode {
    pub id: usize,
    pub kind: NicRxNodeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NicRxTopologyEdge {
    pub id: usize,
    pub from: usize,
    pub to: usize,
    pub lane: NicRxLaneKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NicRxTopology {
    pub nodes: Vec<NicRxTopologyNode>,
    pub edges: Vec<NicRxTopologyEdge>,
}

impl NicRxTopology {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "topology-nodes");
        for node in &self.nodes {
            let _ = writeln!(out, "node{}={}", node.id, node.kind.name());
        }
        let _ = writeln!(out, "topology-edges");
        for edge in &self.edges {
            let from = self
                .nodes
                .get(edge.from)
                .map(|node| node.kind.name())
                .unwrap_or("?");
            let to = self
                .nodes
                .get(edge.to)
                .map(|node| node.kind.name())
                .unwrap_or("?");
            let _ = writeln!(
                out,
                "edge{}={} -> {} via {}",
                edge.id,
                from,
                to,
                edge.lane.name()
            );
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NicRxOutcomeKind {
    EmitMeaningfulPayload,
    DropNonIpv4,
    DropNonUdp,
    DropMalformed,
}

impl NicRxOutcomeKind {
    pub const fn name(self) -> &'static str {
        match self {
            NicRxOutcomeKind::EmitMeaningfulPayload => "emit-meaningful-payload",
            NicRxOutcomeKind::DropNonIpv4 => "drop-non-ipv4",
            NicRxOutcomeKind::DropNonUdp => "drop-non-udp",
            NicRxOutcomeKind::DropMalformed => "drop-malformed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NicRxEdgeLoad {
    pub edge_id: usize,
    pub marble_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NicRxRunReport {
    pub outcome: NicRxOutcomeKind,
    pub emitted: Option<MeaningfulUdpPayloadMarble>,
    pub edge_loads: Vec<NicRxEdgeLoad>,
}

impl NicRxRunReport {
    pub fn render(&self, topology: &NicRxTopology) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "outcome={}", self.outcome.name());
        let _ = writeln!(out, "edge-loads");
        for load in &self.edge_loads {
            let edge = topology.edges.get(load.edge_id);
            let from = edge
                .and_then(|edge| topology.nodes.get(edge.from))
                .map(|node| node.kind.name())
                .unwrap_or("?");
            let to = edge
                .and_then(|edge| topology.nodes.get(edge.to))
                .map(|node| node.kind.name())
                .unwrap_or("?");
            let lane = edge.map(|edge| edge.lane.name()).unwrap_or("?");
            let _ = writeln!(
                out,
                "edge{}={} -> {} via {} marbles={}",
                load.edge_id, from, to, lane, load.marble_count
            );
        }
        if let Some(emitted) = &self.emitted {
            let _ = writeln!(out, "black-hole={}", emitted.render_summary());
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NicFrameParserWidget;

impl MarbleGadget for NicFrameParserWidget {
    fn name(&self) -> &'static str {
        "nic-frame-parser-widget"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UdpSelectorWidget;

impl MarbleGadget for UdpSelectorWidget {
    fn name(&self) -> &'static str {
        "udp-selector-widget"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UdpExtractorWidget;

impl MarbleGadget for UdpExtractorWidget {
    fn name(&self) -> &'static str {
        "udp-extractor-widget"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NicRxCollapsedWorld {
    pub universe: NicGpuUniverseId,
    pub nic: PciFunctionAddress,
    pub white_hole: NicRxHole,
    pub control_lane: usize,
    pub black_hole: NicRxHole,
    pub parser: NicFrameParserWidget,
    pub selector: UdpSelectorWidget,
    pub extractor: UdpExtractorWidget,
}

impl NicRxCollapsedWorld {
    pub const NAME: &'static str = "nic-rx-cw";
    pub const WHITE_TO_PARSER_FRAME_EDGE: usize = 0;
    pub const WHITE_TO_PARSER_CONTROL_EDGE: usize = 1;
    pub const PARSER_TO_SELECTOR_EDGE: usize = 2;
    pub const SELECTOR_TO_EXTRACTOR_EDGE: usize = 3;
    pub const EXTRACTOR_TO_BLACK_EDGE: usize = 4;

    pub const fn new(universe: NicGpuUniverseId, nic: PciFunctionAddress) -> Self {
        Self {
            universe,
            nic,
            white_hole: NicRxHole { index: 0 },
            control_lane: 1,
            black_hole: NicRxHole { index: 1 },
            parser: NicFrameParserWidget,
            selector: UdpSelectorWidget,
            extractor: UdpExtractorWidget,
        }
    }

    pub fn topology(&self) -> NicRxTopology {
        NicRxTopology {
            nodes: vec![
                NicRxTopologyNode {
                    id: 0,
                    kind: NicRxNodeKind::WhiteHole,
                },
                NicRxTopologyNode {
                    id: 1,
                    kind: NicRxNodeKind::FrameParser,
                },
                NicRxTopologyNode {
                    id: 2,
                    kind: NicRxNodeKind::UdpSelector,
                },
                NicRxTopologyNode {
                    id: 3,
                    kind: NicRxNodeKind::UdpExtractor,
                },
                NicRxTopologyNode {
                    id: 4,
                    kind: NicRxNodeKind::BlackHole,
                },
            ],
            edges: vec![
                NicRxTopologyEdge {
                    id: Self::WHITE_TO_PARSER_FRAME_EDGE,
                    from: 0,
                    to: 1,
                    lane: NicRxLaneKind::FrameBytes,
                },
                NicRxTopologyEdge {
                    id: Self::WHITE_TO_PARSER_CONTROL_EDGE,
                    from: 0,
                    to: 1,
                    lane: NicRxLaneKind::Control1,
                },
                NicRxTopologyEdge {
                    id: Self::PARSER_TO_SELECTOR_EDGE,
                    from: 1,
                    to: 2,
                    lane: NicRxLaneKind::FrameBytes,
                },
                NicRxTopologyEdge {
                    id: Self::SELECTOR_TO_EXTRACTOR_EDGE,
                    from: 2,
                    to: 3,
                    lane: NicRxLaneKind::FrameBytes,
                },
                NicRxTopologyEdge {
                    id: Self::EXTRACTOR_TO_BLACK_EDGE,
                    from: 3,
                    to: 4,
                    lane: NicRxLaneKind::MeaningfulPayload,
                },
            ],
        }
    }

    pub fn execute(&mut self, frame: &[u8], control: NicRxControlMarble) -> NicRxRunReport {
        let mut edge_loads = vec![
            NicRxEdgeLoad {
                edge_id: Self::WHITE_TO_PARSER_FRAME_EDGE,
                marble_count: frame.len(),
            },
            NicRxEdgeLoad {
                edge_id: Self::WHITE_TO_PARSER_CONTROL_EDGE,
                marble_count: 1,
            },
        ];

        let Some(parsed) = parse_ipv4_udp(frame) else {
            return NicRxRunReport {
                outcome: if frame.len() >= 14
                    && u16::from_be_bytes([frame[12], frame[13]]) != 0x0800
                {
                    NicRxOutcomeKind::DropNonIpv4
                } else {
                    NicRxOutcomeKind::DropMalformed
                },
                emitted: None,
                edge_loads,
            };
        };

        edge_loads.push(NicRxEdgeLoad {
            edge_id: Self::PARSER_TO_SELECTOR_EDGE,
            marble_count: frame.len(),
        });

        if !parsed.is_udp {
            return NicRxRunReport {
                outcome: NicRxOutcomeKind::DropNonUdp,
                emitted: None,
                edge_loads,
            };
        }

        edge_loads.push(NicRxEdgeLoad {
            edge_id: Self::SELECTOR_TO_EXTRACTOR_EDGE,
            marble_count: frame.len(),
        });

        let emitted = MeaningfulUdpPayloadMarble {
            universe: self.universe,
            nic: self.nic,
            src_ip: parsed.src_ip,
            dst_ip: parsed.dst_ip,
            src_port: parsed.src_port,
            dst_port: parsed.dst_port,
            payload: parsed.payload,
            prefer_gpu_upload: control.prefer_gpu_upload,
            target_tex_id_hint: control.target_tex_id_hint,
        };

        edge_loads.push(NicRxEdgeLoad {
            edge_id: Self::EXTRACTOR_TO_BLACK_EDGE,
            marble_count: emitted.payload.len(),
        });

        NicRxRunReport {
            outcome: NicRxOutcomeKind::EmitMeaningfulPayload,
            emitted: Some(emitted),
            edge_loads,
        }
    }
}

pub fn nic_rx_world_example_visual() -> String {
    let mut world = NicRxCollapsedWorld::new(
        NicGpuUniverseId(1),
        PciFunctionAddress {
            bus: 0,
            slot: 1,
            function: 0,
        },
    );
    let control = NicRxControlMarble {
        rx_queue: 0,
        checksum_ok: true,
        prefer_gpu_upload: true,
        target_tex_id_hint: 7,
    };
    let frame = sample_udp_frame(b"hello-gpu");
    let topology = world.topology();
    let report = world.execute(frame.as_slice(), control);

    let mut out = String::new();
    let _ = writeln!(out, "{}", NicRxCollapsedWorld::NAME);
    let _ = writeln!(out, "universe={}", world.universe.0);
    let _ = writeln!(out, "nic={}", world.nic.render());
    let _ = writeln!(out, "white-hole={}", world.white_hole.index);
    let _ = writeln!(out, "control-lane={} via white-hole", world.control_lane);
    let _ = writeln!(out, "black-hole={}", world.black_hole.index);
    let _ = writeln!(out, "parser={}", world.parser.name());
    let _ = writeln!(out, "selector={}", world.selector.name());
    let _ = writeln!(out, "extractor={}", world.extractor.name());
    let _ = writeln!(out);
    let _ = write!(out, "{}", topology.render());
    let _ = write!(out, "{}", report.render(&topology));
    out
}

#[derive(Debug)]
struct ParsedUdpFrame {
    is_udp: bool,
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    payload: Vec<u8>,
}

fn parse_ipv4_udp(frame: &[u8]) -> Option<ParsedUdpFrame> {
    if frame.len() < 14 + 20 {
        return None;
    }
    let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    if ethertype != 0x0800 {
        return None;
    }
    let ip_start = 14;
    let version = frame[ip_start] >> 4;
    let ihl = (frame[ip_start] & 0x0F) as usize * 4;
    if version != 4 || ihl < 20 || frame.len() < ip_start + ihl + 8 {
        return None;
    }
    let protocol = frame[ip_start + 9];
    let src_ip = [
        frame[ip_start + 12],
        frame[ip_start + 13],
        frame[ip_start + 14],
        frame[ip_start + 15],
    ];
    let dst_ip = [
        frame[ip_start + 16],
        frame[ip_start + 17],
        frame[ip_start + 18],
        frame[ip_start + 19],
    ];
    if protocol != 17 {
        return Some(ParsedUdpFrame {
            is_udp: false,
            src_ip,
            dst_ip,
            src_port: 0,
            dst_port: 0,
            payload: Vec::new(),
        });
    }
    let udp_start = ip_start + ihl;
    let src_port = u16::from_be_bytes([frame[udp_start], frame[udp_start + 1]]);
    let dst_port = u16::from_be_bytes([frame[udp_start + 2], frame[udp_start + 3]]);
    let udp_len = u16::from_be_bytes([frame[udp_start + 4], frame[udp_start + 5]]) as usize;
    if udp_len < 8 || frame.len() < udp_start + udp_len {
        return None;
    }
    let payload = frame[(udp_start + 8)..(udp_start + udp_len)].to_vec();
    Some(ParsedUdpFrame {
        is_udp: true,
        src_ip,
        dst_ip,
        src_port,
        dst_port,
        payload,
    })
}

fn sample_udp_frame(payload: &[u8]) -> Vec<u8> {
    let udp_len = 8 + payload.len();
    let total_len = 20 + udp_len;
    let mut frame = vec![0u8; 14 + total_len];
    frame[12] = 0x08;
    frame[13] = 0x00;
    frame[14] = 0x45;
    frame[16..18].copy_from_slice(&(total_len as u16).to_be_bytes());
    frame[23] = 17;
    frame[26..30].copy_from_slice(&[10, 0, 0, 1]);
    frame[30..34].copy_from_slice(&[10, 0, 0, 2]);
    let udp_start = 14 + 20;
    frame[udp_start..udp_start + 2].copy_from_slice(&4000u16.to_be_bytes());
    frame[udp_start + 2..udp_start + 4].copy_from_slice(&4001u16.to_be_bytes());
    frame[udp_start + 4..udp_start + 6].copy_from_slice(&(udp_len as u16).to_be_bytes());
    frame[(udp_start + 8)..(udp_start + 8 + payload.len())].copy_from_slice(payload);
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nic_rx_world_extracts_meaningful_udp_payload() {
        let mut world = NicRxCollapsedWorld::new(
            NicGpuUniverseId(9),
            PciFunctionAddress {
                bus: 2,
                slot: 0,
                function: 0,
            },
        );
        let frame = sample_udp_frame(b"frame-payload");
        let report = world.execute(
            frame.as_slice(),
            NicRxControlMarble {
                rx_queue: 3,
                checksum_ok: true,
                prefer_gpu_upload: true,
                target_tex_id_hint: 42,
            },
        );

        assert_eq!(report.outcome, NicRxOutcomeKind::EmitMeaningfulPayload);
        let emitted = report.emitted.as_ref().unwrap();
        assert_eq!(emitted.src_port, 4000);
        assert_eq!(emitted.dst_port, 4001);
        assert_eq!(emitted.payload, b"frame-payload");
        assert!(emitted.prefer_gpu_upload);
        assert_eq!(emitted.target_tex_id_hint, 42);
    }

    #[test]
    fn nic_rx_world_keeps_topology_stable() {
        let world = NicRxCollapsedWorld::new(
            NicGpuUniverseId(1),
            PciFunctionAddress {
                bus: 0,
                slot: 1,
                function: 0,
            },
        );
        let topology = world.topology();

        assert_eq!(topology.nodes.len(), 5);
        assert_eq!(topology.edges.len(), 5);
        assert_eq!(topology.edges[0].lane, NicRxLaneKind::FrameBytes);
        assert_eq!(topology.edges[1].lane, NicRxLaneKind::Control1);
        assert_eq!(topology.edges[4].lane, NicRxLaneKind::MeaningfulPayload);
    }

    #[test]
    fn nic_rx_visual_mentions_black_hole_payload() {
        let visual = nic_rx_world_example_visual();
        assert!(visual.contains("nic-rx-cw"));
        assert!(visual.contains("topology-edges"));
        assert!(visual.contains("black-hole=nic="));
    }
}
