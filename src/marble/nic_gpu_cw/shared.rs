use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use super::Marble;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NicGpuUniverseId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciFunctionAddress {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
}

impl PciFunctionAddress {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = write!(out, "{:02x}:{:02x}.{}", self.bus, self.slot, self.function);
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NicRxControlMarble {
    pub rx_queue: u16,
    pub checksum_ok: bool,
    pub prefer_gpu_upload: bool,
    pub target_tex_id_hint: u32,
}

impl Marble for NicRxControlMarble {
    fn kind(&self) -> &'static str {
        "nic-rx-control-marble"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeaningfulUdpPayloadMarble {
    pub universe: NicGpuUniverseId,
    pub nic: PciFunctionAddress,
    pub src_ip: [u8; 4],
    pub dst_ip: [u8; 4],
    pub src_port: u16,
    pub dst_port: u16,
    pub payload: Vec<u8>,
    pub prefer_gpu_upload: bool,
    pub target_tex_id_hint: u32,
}

impl MeaningfulUdpPayloadMarble {
    pub fn render_summary(&self) -> String {
        let mut out = String::new();
        let _ = write!(
            out,
            "nic={} {}.{}.{}.{}:{} -> {}.{}.{}.{}:{} payload-bytes={} gpu-hint={} tex-hint={}",
            self.nic.render(),
            self.src_ip[0],
            self.src_ip[1],
            self.src_ip[2],
            self.src_ip[3],
            self.src_port,
            self.dst_ip[0],
            self.dst_ip[1],
            self.dst_ip[2],
            self.dst_ip[3],
            self.dst_port,
            self.payload.len(),
            self.prefer_gpu_upload,
            self.target_tex_id_hint
        );
        out
    }
}

impl Marble for MeaningfulUdpPayloadMarble {
    fn kind(&self) -> &'static str {
        "meaningful-udp-payload-marble"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuSubmitControlMarble {
    pub gpu: PciFunctionAddress,
    pub allow_submit: bool,
    pub queue_index: u16,
    pub present_after_upload: bool,
}

impl Marble for GpuSubmitControlMarble {
    fn kind(&self) -> &'static str {
        "gpu-submit-control-marble"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuSubmissionMarble {
    pub universe: NicGpuUniverseId,
    pub gpu: PciFunctionAddress,
    pub queue_index: u16,
    pub target_tex_id: u32,
    pub bytes: Vec<u8>,
    pub present_after_upload: bool,
}

impl GpuSubmissionMarble {
    pub fn render_summary(&self) -> String {
        let mut out = String::new();
        let _ = write!(
            out,
            "gpu={} queue={} tex={} bytes={} present={}",
            self.gpu.render(),
            self.queue_index,
            self.target_tex_id,
            self.bytes.len(),
            self.present_after_upload
        );
        out
    }
}

impl Marble for GpuSubmissionMarble {
    fn kind(&self) -> &'static str {
        "gpu-submission-marble"
    }
}
