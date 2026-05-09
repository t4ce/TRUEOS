#![allow(dead_code)]

// These registers keep continuous count of statistics regarding the graphics
// pipeline. They are saved and restored with context, but software should not
// modify them except to reset them to 0 at context creation time. Writes to
// the counters in this section must be issued via MI_LOAD_REGISTER_IMM,
// MI_LOAD_REGISTER_MEM, or MI_LOAD_REGISTER_REG from the ring buffer or a
// batch buffer. The registers may be read at any time, but a pipeline flush
// immediately before reading is required to synchronize the counts with the
// primitive stream and produce meaningful results.

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum RenderStat {
    IaVerticesCount,
    IaPrimitivesCount,
    VsInvocationCount,
    HsInvocationCount,
    DsInvocationCount,
    GsInvocationCount,
    GsPrimitivesCount,
    ClInvocationCount,
    ClPrimitivesCount,
    PsInvocationCount,
    PsInvocationCountSlice0,
    PsInvocationCountSlice1,
    PsInvocationCountSlice2,
    PsInvocationCountSlice4,
    PsInvocationCountSlice5,
    CpsInvocationCount,
    PsDepthCount,
    PsDepthCountSlice0,
    PsDepthCountSlice1,
    PsDepthCountSlice2,
    PsDepthCountSlice3,
    PsDepthCountSlice4,
    PsDepthCountSlice5,
    Timestamp,
    StreamOutput0WriteOffset,
    StreamOutput1WriteOffset,
    StreamOutput2WriteOffset,
    StreamOutput3WriteOffset,
    WindowHardwareGeneratedClearValue,
    CsCtxTimestamp,
}

pub(crate) const RENDER_STATS: &[RenderStat] = &[
    RenderStat::IaVerticesCount,
    RenderStat::IaPrimitivesCount,
    RenderStat::VsInvocationCount,
    RenderStat::HsInvocationCount,
    RenderStat::DsInvocationCount,
    RenderStat::GsInvocationCount,
    RenderStat::GsPrimitivesCount,
    RenderStat::ClInvocationCount,
    RenderStat::ClPrimitivesCount,
    RenderStat::PsInvocationCount,
    RenderStat::PsInvocationCountSlice0,
    RenderStat::PsInvocationCountSlice1,
    RenderStat::PsInvocationCountSlice2,
    RenderStat::PsInvocationCountSlice4,
    RenderStat::PsInvocationCountSlice5,
    RenderStat::CpsInvocationCount,
    RenderStat::PsDepthCount,
    RenderStat::PsDepthCountSlice0,
    RenderStat::PsDepthCountSlice1,
    RenderStat::PsDepthCountSlice2,
    RenderStat::PsDepthCountSlice3,
    RenderStat::PsDepthCountSlice4,
    RenderStat::PsDepthCountSlice5,
    RenderStat::Timestamp,
    RenderStat::StreamOutput0WriteOffset,
    RenderStat::StreamOutput1WriteOffset,
    RenderStat::StreamOutput2WriteOffset,
    RenderStat::StreamOutput3WriteOffset,
    RenderStat::WindowHardwareGeneratedClearValue,
    RenderStat::CsCtxTimestamp,
];

impl RenderStat {
    pub(crate) const fn mmio_offset(self) -> Option<usize> {
        match self {
            Self::IaVerticesCount => Some(0x2310),
            Self::IaPrimitivesCount => Some(0x2318),
            Self::VsInvocationCount => Some(0x2320),
            Self::HsInvocationCount => Some(0x2300),
            Self::DsInvocationCount => Some(0x2308),
            Self::GsInvocationCount => Some(0x2328),
            Self::GsPrimitivesCount => Some(0x2330),
            Self::ClInvocationCount => Some(0x2338),
            Self::ClPrimitivesCount => Some(0x2340),
            Self::PsInvocationCount => Some(0x2348),
            Self::CpsInvocationCount => Some(0x2478),
            Self::PsDepthCount => Some(0x2350),
            Self::PsInvocationCountSlice0 => Some(0x22C8),
            Self::PsDepthCountSlice0 => Some(0x22D8),
            _ => None,
        }
    }

    pub(crate) const fn symbol(self) -> &'static str {
        match self {
            Self::IaVerticesCount => "IA_VERTICES_COUNT",
            Self::IaPrimitivesCount => "IA_PRIMITIVES_COUNT",
            Self::VsInvocationCount => "VS_INVOCATION_COUNT",
            Self::HsInvocationCount => "HS_INVOCATION_COUNT",
            Self::DsInvocationCount => "DS_INVOCATION_COUNT",
            Self::GsInvocationCount => "GS_INVOCATION_COUNT",
            Self::GsPrimitivesCount => "GS_PRIMITIVES_COUNT",
            Self::ClInvocationCount => "CL_INVOCATION_COUNT",
            Self::ClPrimitivesCount => "CL_PRIMITIVES_COUNT",
            Self::PsInvocationCount => "PS_INVOCATION_COUNT",
            Self::PsInvocationCountSlice0 => "PS_INVOCATION_COUNT_SLICE0",
            Self::PsInvocationCountSlice1 => "PS_INVOCATION_COUNT_SLICE1",
            Self::PsInvocationCountSlice2 => "PS_INVOCATION_COUNT_SLICE2",
            Self::PsInvocationCountSlice4 => "PS_INVOCATION_COUNT_SLICE4",
            Self::PsInvocationCountSlice5 => "PS_INVOCATION_COUNT_SLICE5",
            Self::CpsInvocationCount => "CPS_INVOCATION_COUNT",
            Self::PsDepthCount => "PS_DEPTH_COUNT",
            Self::PsDepthCountSlice0 => "PS_DEPTH_COUNT_SLICE0",
            Self::PsDepthCountSlice1 => "PS_DEPTH_COUNT_SLICE1",
            Self::PsDepthCountSlice2 => "PS_DEPTH_COUNT_SLICE2",
            Self::PsDepthCountSlice3 => "PS_DEPTH_COUNT_SLICE3",
            Self::PsDepthCountSlice4 => "PS_DEPTH_COUNT_SLICE4",
            Self::PsDepthCountSlice5 => "PS_DEPTH_COUNT_SLICE5",
            Self::Timestamp => "TIMESTAMP",
            Self::StreamOutput0WriteOffset => "STREAM_OUTPUT_0_WRITE_OFFSET",
            Self::StreamOutput1WriteOffset => "STREAM_OUTPUT_1_WRITE_OFFSET",
            Self::StreamOutput2WriteOffset => "STREAM_OUTPUT_2_WRITE_OFFSET",
            Self::StreamOutput3WriteOffset => "STREAM_OUTPUT_3_WRITE_OFFSET",
            Self::WindowHardwareGeneratedClearValue => "WINDOW_HARDWARE_GENERATED_CLEAR_VALUE",
            Self::CsCtxTimestamp => "CS_CTX_TIMESTAMP",
        }
    }

    pub(crate) const fn description(self) -> Option<&'static str> {
        match self {
            Self::IaVerticesCount => Some("IA Vertices Count"),
            Self::IaPrimitivesCount => Some("Primitives Generated By VF"),
            Self::VsInvocationCount => Some("VS Invocation Counter"),
            Self::HsInvocationCount => Some("HS Invocation Counter"),
            Self::DsInvocationCount => Some("DS Invocation Counter"),
            Self::GsInvocationCount => Some("GS Invocation Counter"),
            Self::GsPrimitivesCount => Some("GS Primitives Counter"),
            Self::ClInvocationCount => Some("Clipper Invocation Counter"),
            Self::ClPrimitivesCount => Some("Clipper Primitives Counter"),
            Self::PsInvocationCount => Some("PS Invocation Count"),
            Self::PsInvocationCountSlice0 => Some("PS Invocation Count for Slice0"),
            Self::PsInvocationCountSlice1 => Some("PS Invocation Count for Slice1"),
            Self::PsInvocationCountSlice2 => Some("PS Invocation Count for Slice2"),
            Self::PsInvocationCountSlice4 => Some("PS Invocation Count for Slice4"),
            Self::PsInvocationCountSlice5 => Some("PS Invocation Count for Slice5"),
            Self::CpsInvocationCount => Some("CPS Invocation Counter"),
            Self::PsDepthCount => None,
            Self::PsDepthCountSlice0 => Some("PS Depth Count for Slice0"),
            Self::PsDepthCountSlice1 => Some("PS Depth Count for Slice1"),
            Self::PsDepthCountSlice2 => Some("PS Depth Count for Slice2"),
            Self::PsDepthCountSlice3 => Some("PS Depth Count for Slice3"),
            Self::PsDepthCountSlice4 => Some("PS Depth Count for Slice4"),
            Self::PsDepthCountSlice5 => Some("PS Depth Count for Slice5"),
            Self::Timestamp => Some("Reported Timestamp Count"),
            Self::StreamOutput0WriteOffset => Some("Stream Output 0 Write Offset"),
            Self::StreamOutput1WriteOffset => Some("Stream Output 1 Write Offset"),
            Self::StreamOutput2WriteOffset => Some("Stream Output 2 Write Offset"),
            Self::StreamOutput3WriteOffset => Some("Stream Output 3 Write Offset"),
            Self::WindowHardwareGeneratedClearValue => {
                Some("Window Hardware Generated Clear Value")
            }
            Self::CsCtxTimestamp => Some("CS Context Timestamp Count"),
        }
    }
}
