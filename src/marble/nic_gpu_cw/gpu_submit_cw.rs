use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use super::{
    GpuSubmissionMarble, GpuSubmitControlMarble, MarbleGadget, MeaningfulUdpPayloadMarble,
    NicGpuUniverseId, PciFunctionAddress,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuSubmitHole {
    pub index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuSubmitNodeKind {
    WhiteHole,
    PayloadSelector,
    UploadBuilder,
    BlackHole,
}

impl GpuSubmitNodeKind {
    pub const fn name(self) -> &'static str {
        match self {
            GpuSubmitNodeKind::WhiteHole => "white-hole",
            GpuSubmitNodeKind::PayloadSelector => "payload-selector-widget",
            GpuSubmitNodeKind::UploadBuilder => "upload-builder-widget",
            GpuSubmitNodeKind::BlackHole => "black-hole",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuSubmitLaneKind {
    MeaningfulPayload,
    Control1,
    GpuSubmission,
}

impl GpuSubmitLaneKind {
    pub const fn name(self) -> &'static str {
        match self {
            GpuSubmitLaneKind::MeaningfulPayload => "meaningful-payload",
            GpuSubmitLaneKind::Control1 => "control[1]",
            GpuSubmitLaneKind::GpuSubmission => "gpu-submission",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuSubmitTopologyNode {
    pub id: usize,
    pub kind: GpuSubmitNodeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuSubmitTopologyEdge {
    pub id: usize,
    pub from: usize,
    pub to: usize,
    pub lane: GpuSubmitLaneKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuSubmitTopology {
    pub nodes: Vec<GpuSubmitTopologyNode>,
    pub edges: Vec<GpuSubmitTopologyEdge>,
}

impl GpuSubmitTopology {
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
pub enum GpuSubmitOutcomeKind {
    EmitSubmission,
    DropNotGpuBound,
    DropSubmitDisabled,
}

impl GpuSubmitOutcomeKind {
    pub const fn name(self) -> &'static str {
        match self {
            GpuSubmitOutcomeKind::EmitSubmission => "emit-submission",
            GpuSubmitOutcomeKind::DropNotGpuBound => "drop-not-gpu-bound",
            GpuSubmitOutcomeKind::DropSubmitDisabled => "drop-submit-disabled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuSubmitEdgeLoad {
    pub edge_id: usize,
    pub marble_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuSubmitRunReport {
    pub outcome: GpuSubmitOutcomeKind,
    pub emitted: Option<GpuSubmissionMarble>,
    pub edge_loads: Vec<GpuSubmitEdgeLoad>,
}

impl GpuSubmitRunReport {
    pub fn render(&self, topology: &GpuSubmitTopology) -> String {
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
pub struct PayloadSelectorWidget;

impl MarbleGadget for PayloadSelectorWidget {
    fn name(&self) -> &'static str {
        "payload-selector-widget"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UploadBuilderWidget;

impl MarbleGadget for UploadBuilderWidget {
    fn name(&self) -> &'static str {
        "upload-builder-widget"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuSubmitCollapsedWorld {
    pub universe: NicGpuUniverseId,
    pub gpu: PciFunctionAddress,
    pub white_hole: GpuSubmitHole,
    pub control_lane: usize,
    pub black_hole: GpuSubmitHole,
    pub selector: PayloadSelectorWidget,
    pub builder: UploadBuilderWidget,
}

impl GpuSubmitCollapsedWorld {
    pub const NAME: &'static str = "gpu-submit-cw";
    pub const WHITE_TO_SELECTOR_PAYLOAD_EDGE: usize = 0;
    pub const WHITE_TO_SELECTOR_CONTROL_EDGE: usize = 1;
    pub const SELECTOR_TO_BUILDER_EDGE: usize = 2;
    pub const BUILDER_TO_BLACK_EDGE: usize = 3;

    pub const fn new(universe: NicGpuUniverseId, gpu: PciFunctionAddress) -> Self {
        Self {
            universe,
            gpu,
            white_hole: GpuSubmitHole { index: 0 },
            control_lane: 1,
            black_hole: GpuSubmitHole { index: 1 },
            selector: PayloadSelectorWidget,
            builder: UploadBuilderWidget,
        }
    }

    pub fn topology(&self) -> GpuSubmitTopology {
        GpuSubmitTopology {
            nodes: vec![
                GpuSubmitTopologyNode {
                    id: 0,
                    kind: GpuSubmitNodeKind::WhiteHole,
                },
                GpuSubmitTopologyNode {
                    id: 1,
                    kind: GpuSubmitNodeKind::PayloadSelector,
                },
                GpuSubmitTopologyNode {
                    id: 2,
                    kind: GpuSubmitNodeKind::UploadBuilder,
                },
                GpuSubmitTopologyNode {
                    id: 3,
                    kind: GpuSubmitNodeKind::BlackHole,
                },
            ],
            edges: vec![
                GpuSubmitTopologyEdge {
                    id: Self::WHITE_TO_SELECTOR_PAYLOAD_EDGE,
                    from: 0,
                    to: 1,
                    lane: GpuSubmitLaneKind::MeaningfulPayload,
                },
                GpuSubmitTopologyEdge {
                    id: Self::WHITE_TO_SELECTOR_CONTROL_EDGE,
                    from: 0,
                    to: 1,
                    lane: GpuSubmitLaneKind::Control1,
                },
                GpuSubmitTopologyEdge {
                    id: Self::SELECTOR_TO_BUILDER_EDGE,
                    from: 1,
                    to: 2,
                    lane: GpuSubmitLaneKind::MeaningfulPayload,
                },
                GpuSubmitTopologyEdge {
                    id: Self::BUILDER_TO_BLACK_EDGE,
                    from: 2,
                    to: 3,
                    lane: GpuSubmitLaneKind::GpuSubmission,
                },
            ],
        }
    }

    pub fn execute(
        &mut self,
        payload: MeaningfulUdpPayloadMarble,
        control: GpuSubmitControlMarble,
    ) -> GpuSubmitRunReport {
        let mut edge_loads = vec![
            GpuSubmitEdgeLoad {
                edge_id: Self::WHITE_TO_SELECTOR_PAYLOAD_EDGE,
                marble_count: payload.payload.len(),
            },
            GpuSubmitEdgeLoad {
                edge_id: Self::WHITE_TO_SELECTOR_CONTROL_EDGE,
                marble_count: 1,
            },
        ];

        if !payload.prefer_gpu_upload {
            return GpuSubmitRunReport {
                outcome: GpuSubmitOutcomeKind::DropNotGpuBound,
                emitted: None,
                edge_loads,
            };
        }
        if !control.allow_submit {
            return GpuSubmitRunReport {
                outcome: GpuSubmitOutcomeKind::DropSubmitDisabled,
                emitted: None,
                edge_loads,
            };
        }

        edge_loads.push(GpuSubmitEdgeLoad {
            edge_id: Self::SELECTOR_TO_BUILDER_EDGE,
            marble_count: payload.payload.len(),
        });

        let emitted = GpuSubmissionMarble {
            universe: self.universe,
            gpu: control.gpu,
            queue_index: control.queue_index,
            target_tex_id: payload.target_tex_id_hint,
            bytes: payload.payload,
            present_after_upload: control.present_after_upload,
        };

        edge_loads.push(GpuSubmitEdgeLoad {
            edge_id: Self::BUILDER_TO_BLACK_EDGE,
            marble_count: emitted.bytes.len(),
        });

        GpuSubmitRunReport {
            outcome: GpuSubmitOutcomeKind::EmitSubmission,
            emitted: Some(emitted),
            edge_loads,
        }
    }
}

pub fn gpu_submit_world_example_visual() -> String {
    let mut world = GpuSubmitCollapsedWorld::new(
        NicGpuUniverseId(1),
        PciFunctionAddress {
            bus: 0,
            slot: 2,
            function: 0,
        },
    );
    let payload = MeaningfulUdpPayloadMarble {
        universe: NicGpuUniverseId(1),
        nic: PciFunctionAddress {
            bus: 0,
            slot: 1,
            function: 0,
        },
        src_ip: [10, 0, 0, 1],
        dst_ip: [10, 0, 0, 2],
        src_port: 4000,
        dst_port: 4001,
        payload: b"hello-gpu".to_vec(),
        prefer_gpu_upload: true,
        target_tex_id_hint: 7,
    };
    let control = GpuSubmitControlMarble {
        gpu: world.gpu,
        allow_submit: true,
        queue_index: 0,
        present_after_upload: true,
    };
    let topology = world.topology();
    let report = world.execute(payload, control);

    let mut out = String::new();
    let _ = writeln!(out, "{}", GpuSubmitCollapsedWorld::NAME);
    let _ = writeln!(out, "universe={}", world.universe.0);
    let _ = writeln!(out, "gpu={}", world.gpu.render());
    let _ = writeln!(out, "white-hole={}", world.white_hole.index);
    let _ = writeln!(out, "control-lane={} via white-hole", world.control_lane);
    let _ = writeln!(out, "black-hole={}", world.black_hole.index);
    let _ = writeln!(out, "selector={}", world.selector.name());
    let _ = writeln!(out, "builder={}", world.builder.name());
    let _ = writeln!(out);
    let _ = write!(out, "{}", topology.render());
    let _ = write!(out, "{}", report.render(&topology));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_submit_world_emits_submission_from_meaningful_payload() {
        let mut world = GpuSubmitCollapsedWorld::new(
            NicGpuUniverseId(9),
            PciFunctionAddress {
                bus: 3,
                slot: 0,
                function: 0,
            },
        );
        let payload = MeaningfulUdpPayloadMarble {
            universe: NicGpuUniverseId(9),
            nic: PciFunctionAddress {
                bus: 2,
                slot: 0,
                function: 0,
            },
            src_ip: [10, 0, 0, 1],
            dst_ip: [10, 0, 0, 2],
            src_port: 4000,
            dst_port: 4001,
            payload: b"frame-payload".to_vec(),
            prefer_gpu_upload: true,
            target_tex_id_hint: 99,
        };
        let report = world.execute(
            payload,
            GpuSubmitControlMarble {
                gpu: world.gpu,
                allow_submit: true,
                queue_index: 2,
                present_after_upload: true,
            },
        );

        assert_eq!(report.outcome, GpuSubmitOutcomeKind::EmitSubmission);
        let emitted = report.emitted.as_ref().unwrap();
        assert_eq!(emitted.target_tex_id, 99);
        assert_eq!(emitted.bytes, b"frame-payload");
        assert_eq!(emitted.queue_index, 2);
        assert!(emitted.present_after_upload);
    }

    #[test]
    fn gpu_submit_world_keeps_topology_stable() {
        let world = GpuSubmitCollapsedWorld::new(
            NicGpuUniverseId(1),
            PciFunctionAddress {
                bus: 0,
                slot: 2,
                function: 0,
            },
        );
        let topology = world.topology();

        assert_eq!(topology.nodes.len(), 4);
        assert_eq!(topology.edges.len(), 4);
        assert_eq!(topology.edges[0].lane, GpuSubmitLaneKind::MeaningfulPayload);
        assert_eq!(topology.edges[1].lane, GpuSubmitLaneKind::Control1);
        assert_eq!(topology.edges[3].lane, GpuSubmitLaneKind::GpuSubmission);
    }

    #[test]
    fn gpu_submit_visual_mentions_black_hole_submission() {
        let visual = gpu_submit_world_example_visual();
        assert!(visual.contains("gpu-submit-cw"));
        assert!(visual.contains("topology-edges"));
        assert!(visual.contains("black-hole=gpu="));
    }
}
