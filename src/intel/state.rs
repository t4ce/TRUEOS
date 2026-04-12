#![allow(dead_code)]

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PipelineStage13 {
    CommandStream,
    VertexFetch,
    VertexShader,
    HullShader,
    TessellationEngine,
    DomainShader,
    GeometryShader,
    StreamOutputLogic,
    Clipper,
    StripFan,
    WindowerMasker,
    CoarsePixelShading,
    PsDispatch,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct StageChecklist {
    pub(crate) stage: PipelineStage13,
    pub(crate) short_name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) relevant_packets: &'static [&'static str],
    pub(crate) current_probe_signal: &'static str,
}

pub(crate) const PIPELINE_STAGE_CHECKLIST: &[StageChecklist] = &[
    StageChecklist {
        stage: PipelineStage13::CommandStream,
        short_name: "CS",
        description: "Manages the 3D pipeline and feeds commands and constant data into later stages.",
        relevant_packets: &[
            "STATE_BASE_ADDRESS",
            "PIPE_CONTROL",
            "3DSTATE_BINDING_TABLE_POOL_ALLOC",
            "3DPRIMITIVE",
        ],
        current_probe_signal: "Draw path launches and pre-3d marker writes land.",
    },
    StageChecklist {
        stage: PipelineStage13::VertexFetch,
        short_name: "VF",
        description: "Fetches vertex data from memory, reformats it, and emits VUE references downstream.",
        relevant_packets: &[
            "3DSTATE_VERTEX_BUFFERS",
            "3DSTATE_VERTEX_ELEMENTS",
            "3DSTATE_VF_STATISTICS",
            "3DSTATE_VF",
            "3DSTATE_VF_TOPOLOGY",
            "3DPRIMITIVE",
        ],
        current_probe_signal: "post_vf marker and IA/VF statistics counters.",
    },
    StageChecklist {
        stage: PipelineStage13::VertexShader,
        short_name: "VS",
        description: "Dispatches vertex shader threads for incoming vertices.",
        relevant_packets: &[
            "3DSTATE_VS",
            "3DSTATE_BINDING_TABLE_POINTERS_VS",
            "3DSTATE_SAMPLER_STATE_POINTERS_VS",
            "3DSTATE_URB_ALLOC_VS",
        ],
        current_probe_signal: "post_vs marker and VS_INVOCATION_COUNT.",
    },
    StageChecklist {
        stage: PipelineStage13::HullShader,
        short_name: "HS",
        description: "Processes patch primitives for tessellation factor generation.",
        relevant_packets: &[
            "3DSTATE_HS",
            "3DSTATE_BINDING_TABLE_POINTERS_HS",
            "3DSTATE_SAMPLER_STATE_POINTERS_HS",
            "3DSTATE_URB_ALLOC_HS",
        ],
        current_probe_signal: "Currently disabled/zeroed in our minimal draw path.",
    },
    StageChecklist {
        stage: PipelineStage13::TessellationEngine,
        short_name: "TE",
        description: "Tessellates parametric domains using hull-shader-produced tessellation factors.",
        relevant_packets: &["3DSTATE_TE"],
        current_probe_signal: "Currently disabled/zeroed in our minimal draw path.",
    },
    StageChecklist {
        stage: PipelineStage13::DomainShader,
        short_name: "DS",
        description: "Shades tessellated domain points into output vertices.",
        relevant_packets: &[
            "3DSTATE_DS",
            "3DSTATE_BINDING_TABLE_POINTERS_DS",
            "3DSTATE_SAMPLER_STATE_POINTERS_DS",
            "3DSTATE_URB_ALLOC_DS",
        ],
        current_probe_signal: "Currently disabled/zeroed in our minimal draw path.",
    },
    StageChecklist {
        stage: PipelineStage13::GeometryShader,
        short_name: "GS",
        description: "Processes complete input objects in geometry-shader threads.",
        relevant_packets: &[
            "3DSTATE_GS",
            "3DSTATE_BINDING_TABLE_POINTERS_GS",
            "3DSTATE_SAMPLER_STATE_POINTERS_GS",
            "3DSTATE_URB_ALLOC_GS",
        ],
        current_probe_signal: "Currently disabled/zeroed in our minimal draw path.",
    },
    StageChecklist {
        stage: PipelineStage13::StreamOutputLogic,
        short_name: "SOL",
        description: "Writes object vertices to stream output buffers in memory.",
        relevant_packets: &["3DSTATE_STREAMOUT"],
        current_probe_signal: "Currently disabled/zeroed in our minimal draw path.",
    },
    StageChecklist {
        stage: PipelineStage13::Clipper,
        short_name: "CLIP",
        description: "Performs fixed-function clip tests and clipping on incoming objects.",
        relevant_packets: &["3DSTATE_CLIP"],
        current_probe_signal: "post_clip marker.",
    },
    StageChecklist {
        stage: PipelineStage13::StripFan,
        short_name: "SF",
        description: "Performs fixed-function primitive setup for strips, fans, and raster handoff.",
        relevant_packets: &[
            "3DSTATE_SF",
            "3DSTATE_RASTER",
            "3DSTATE_SBE",
            "3DSTATE_SBE_SWIZ",
            "3DSTATE_DRAWING_RECTANGLE",
            "3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP",
        ],
        current_probe_signal: "post_raster marker after SF/raster setup.",
    },
    StageChecklist {
        stage: PipelineStage13::WindowerMasker,
        short_name: "WM",
        description: "Rasterizes primitives into pixel coverage and visibility.",
        relevant_packets: &[
            "3DSTATE_WM",
            "3DSTATE_WM_DEPTH_STENCIL",
            "3DSTATE_MULTISAMPLE",
            "3DSTATE_SAMPLE_MASK",
            "3DSTATE_DEPTH_BOUNDS",
            "3DSTATE_VIEWPORT_STATE_POINTERS_CC",
        ],
        current_probe_signal: "No dedicated marker yet; failure after raster may implicate WM/PS handoff.",
    },
    StageChecklist {
        stage: PipelineStage13::CoarsePixelShading,
        short_name: "CPS",
        description: "Gathers coarse pixels for coarse pixel shading dispatch.",
        relevant_packets: &["3DSTATE_CPS_POINTER"],
        current_probe_signal: "Not actively used in our current path.",
    },
    StageChecklist {
        stage: PipelineStage13::PsDispatch,
        short_name: "PSD",
        description: "Assembles and dispatches pixel shader threads for pixel, sample, or coarse-pixel rates.",
        relevant_packets: &[
            "3DSTATE_PS",
            "3DSTATE_PS_EXTRA",
            "3DSTATE_PS_BLEND",
            "3DSTATE_BLEND_STATE_POINTERS",
            "3DSTATE_CC_STATE_POINTERS",
            "3DSTATE_BINDING_TABLE_POINTERS_PS",
            "3DSTATE_SAMPLER_STATE_POINTERS_PS",
        ],
        current_probe_signal: "post_ps_state marker, PS statistics counters, and eventual post-3d completion.",
    },
];

impl PipelineStage13 {
    pub(crate) const fn ordinal(self) -> u8 {
        match self {
            Self::CommandStream => 1,
            Self::VertexFetch => 2,
            Self::VertexShader => 3,
            Self::HullShader => 4,
            Self::TessellationEngine => 5,
            Self::DomainShader => 6,
            Self::GeometryShader => 7,
            Self::StreamOutputLogic => 8,
            Self::Clipper => 9,
            Self::StripFan => 10,
            Self::WindowerMasker => 11,
            Self::CoarsePixelShading => 12,
            Self::PsDispatch => 13,
        }
    }

    pub(crate) const fn short_name(self) -> &'static str {
        match self {
            Self::CommandStream => "CS",
            Self::VertexFetch => "VF",
            Self::VertexShader => "VS",
            Self::HullShader => "HS",
            Self::TessellationEngine => "TE",
            Self::DomainShader => "DS",
            Self::GeometryShader => "GS",
            Self::StreamOutputLogic => "SOL",
            Self::Clipper => "CLIP",
            Self::StripFan => "SF",
            Self::WindowerMasker => "WM",
            Self::CoarsePixelShading => "CPS",
            Self::PsDispatch => "PSD",
        }
    }
}
