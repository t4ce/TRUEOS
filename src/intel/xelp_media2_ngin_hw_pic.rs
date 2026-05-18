// Hardware-picture mission slice of the broader media NGIN backend.
//
// This module keeps the hw_pic-facing API narrow so the boot-logo JPEG path
// can evolve without dragging MP4/H.264/demo-first-frame surfaces into view.
// The heavy shared media backend remains in xelp_media2_ngin for now; this
// module is the focused entry surface for the logo decode mission.

use super::xelp_media2_ngin::{
    self as media, MediaBitstreamBacking, MediaEngineDescriptor, MediaGpuWindowLayout,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct JpegQuantTables {
    pub(super) tables: [[u8; 64]; 4],
    pub(super) present_mask: u8,
    pub(super) component_qtable: [u8; 3],
    pub(super) component_count: u8,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct JpegHuffTables {
    pub(super) dc_bits: [[u8; 12]; 4],
    pub(super) dc_values: [[u8; 12]; 4],
    pub(super) ac_bits: [[u8; 16]; 4],
    pub(super) ac_values: [[u8; 162]; 4],
    pub(super) dc_present_mask: u8,
    pub(super) ac_present_mask: u8,
    pub(super) y_dc_selector: u8,
    pub(super) y_ac_selector: u8,
    pub(super) chroma_dc_selector: u8,
    pub(super) chroma_ac_selector: u8,
    pub(super) has_chroma_selector: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct JpegScanInfo {
    pub(super) input_format: u8,
    pub(super) scan_data_offset: u32,
    pub(super) scan_data_length: u32,
    pub(super) scan_component_count: u8,
    pub(super) scan_component_mask: u8,
    pub(super) interleaved: bool,
    pub(super) restart_interval: u16,
    pub(super) mcu_count: u32,
}

#[derive(Copy, Clone, Debug)]
pub(super) struct MediaEncodedStreamProof {
    pub engine_name: &'static str,
    pub bitstream_gpu_addr: u64,
    pub bitstream_phys: u64,
    pub bitstream_virt: usize,
    pub bytes_written: usize,
    pub capacity: usize,
    pub signature: u32,
    pub forcewake_engine_ack_reg: usize,
    pub forcewake_engine_ack: u32,
    pub forcewake_engine_awake: bool,
    pub forcewake_global_ack: u32,
    pub forcewake_awake_count: usize,
}

#[derive(Copy, Clone, Debug)]
pub(super) struct MediaJpegSmokeSubmitProof {
    pub engine_name: &'static str,
    pub batch_gpu_addr: u64,
    pub result_gpu_addr: u64,
    pub bitstream_gpu_addr: u64,
    pub output_surface_gpu_addr: u64,
    pub bitstream_bytes: usize,
    pub coded_width: u32,
    pub coded_height: u32,
    pub jpeg_input_format: u8,
    pub jpeg_output_format: u8,
    pub jpeg_scan_component_count: u8,
    pub jpeg_interleaved: bool,
    pub jpeg_restart_interval: u16,
    pub jpeg_mcu_count: u32,
    pub jpeg_scan_data_offset: u32,
    pub jpeg_scan_data_length: u32,
    pub jpeg_bsd_dw4: u32,
    pub output_surface_pitch: usize,
    pub output_surface_bytes: usize,
    pub surface_dw2: u32,
    pub surface_dw3: u32,
    pub surface_dw4: u32,
    pub surface_dw5: u32,
    pub pipe_mode_dw1: u32,
    pub jpeg_pic_dw1: u32,
    pub jpeg_pic_dw2: u32,
    pub output_surface_detail: bool,
    pub batch_tail_bytes: usize,
    pub ring_tail_bytes: usize,
    pub kickoff_marker: u32,
    pub presubmit_marker: u32,
    pub postsubmit_marker: u32,
    pub complete_marker: u32,
    pub kickoff_value: u32,
    pub presubmit_value: u32,
    pub postsubmit_value: u32,
    pub complete_value: u32,
    pub retired: bool,
    pub poll_iters: usize,
    pub execlist_status_lo: u32,
    pub execlist_status_hi: u32,
    pub ring_start: u32,
    pub ring_ctl: u32,
    pub ring_hws_pga: u32,
    pub ring_head: u32,
    pub ring_tail: u32,
    pub ring_acthd: u32,
    pub ring_acthd_hi: u32,
    pub acthd_region: &'static str,
    pub acthd_offset_bytes: u32,
    pub acthd_dword: u32,
    pub bbaddr_lo: u32,
    pub bbaddr_hi: u32,
    pub dma_fadd_lo: u32,
    pub dma_fadd_hi: u32,
    pub bbstate: u32,
    pub esr: u32,
    pub instps: u32,
    pub psmi_ctl: u32,
    pub nopid: u32,
    pub ipeir: u32,
    pub ipehr: u32,
    pub fault_gen8: u32,
    pub fault_gen12: u32,
    pub fault_tlb_data0_gen8: u32,
    pub fault_tlb_data1_gen8: u32,
    pub fault_tlb_data0_gen12: u32,
    pub fault_tlb_data1_gen12: u32,
    pub stage_flags_value: u32,
    pub bitstream_dword0: u32,
}

const JPEG_QM_SCAN_8X8: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

#[inline]
fn ceil_div_u32(value: u32, divisor: u32) -> u32 {
    if divisor == 0 {
        0
    } else {
        value.saturating_add(divisor.saturating_sub(1)) / divisor
    }
}

pub(super) fn parse_jpeg_frame_dims(encoded: &[u8]) -> Option<(u32, u32)> {
    if encoded.len() < 4 || encoded[0] != 0xFF || encoded[1] != 0xD8 {
        return None;
    }

    let mut idx = 2usize;
    while idx + 3 < encoded.len() {
        if encoded[idx] != 0xFF {
            idx += 1;
            continue;
        }
        while idx < encoded.len() && encoded[idx] == 0xFF {
            idx += 1;
        }
        if idx >= encoded.len() {
            break;
        }
        let marker = encoded[idx];
        idx += 1;

        if marker == 0xD9 || marker == 0xDA {
            break;
        }
        if matches!(marker, 0x01 | 0xD0..=0xD7) {
            continue;
        }
        if idx + 1 >= encoded.len() {
            break;
        }
        let segment_len = u16::from_be_bytes([encoded[idx], encoded[idx + 1]]) as usize;
        idx += 2;
        if segment_len < 2 || idx + segment_len - 2 > encoded.len() {
            break;
        }

        if matches!(marker, 0xC0..=0xC2 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF)
            && segment_len >= 7
        {
            let height = u16::from_be_bytes([encoded[idx + 1], encoded[idx + 2]]) as u32;
            let width = u16::from_be_bytes([encoded[idx + 3], encoded[idx + 4]]) as u32;
            if width != 0 && height != 0 {
                return Some((width, height));
            }
        }

        idx += segment_len - 2;
    }

    None
}

fn jpeg_input_format_from_sampling(
    component_count: u8,
    h_sampling: &[u8; 3],
    v_sampling: &[u8; 3],
) -> Option<u8> {
    if component_count == 1 {
        return Some(0);
    }
    if component_count < 3 {
        return None;
    }

    let y = (h_sampling[0], v_sampling[0]);
    let cb = (h_sampling[1], v_sampling[1]);
    let cr = (h_sampling[2], v_sampling[2]);
    if cb != cr {
        return None;
    }

    match (y, cb) {
        ((2, 2), (1, 1)) => Some(1),
        ((2, 1), (1, 1)) => Some(2),
        ((1, 1), (1, 1)) => Some(3),
        ((4, 1), (1, 1)) => Some(4),
        ((1, 2), (1, 1)) => Some(5),
        ((2, 2), (1, 2)) => Some(6),
        ((2, 2), (2, 1)) => Some(7),
        _ => None,
    }
}

#[inline]
pub(super) fn jpeg_output_format_from_input(input_format: u8) -> u8 {
    match input_format {
        1 => 1,
        _ => 0,
    }
}

pub(super) fn parse_jpeg_quant_tables(encoded: &[u8]) -> Option<JpegQuantTables> {
    if encoded.len() < 4 || encoded[0] != 0xFF || encoded[1] != 0xD8 {
        return None;
    }

    let mut parsed = JpegQuantTables {
        tables: [[0; 64]; 4],
        present_mask: 0,
        component_qtable: [0, 1, 1],
        component_count: 1,
    };
    let mut idx = 2usize;

    while idx + 3 < encoded.len() {
        if encoded[idx] != 0xFF {
            idx += 1;
            continue;
        }
        while idx < encoded.len() && encoded[idx] == 0xFF {
            idx += 1;
        }
        if idx >= encoded.len() {
            break;
        }
        let marker = encoded[idx];
        idx += 1;

        if marker == 0xD9 || marker == 0xDA {
            break;
        }
        if matches!(marker, 0x01 | 0xD0..=0xD7) {
            continue;
        }
        if idx + 1 >= encoded.len() {
            break;
        }
        let segment_len = u16::from_be_bytes([encoded[idx], encoded[idx + 1]]) as usize;
        idx += 2;
        if segment_len < 2 || idx + segment_len - 2 > encoded.len() {
            break;
        }

        match marker {
            0xDB => {
                let mut seg_idx = idx;
                let seg_end = idx + segment_len - 2;
                while seg_idx < seg_end {
                    let pq_tq = encoded[seg_idx];
                    seg_idx += 1;
                    let precision = pq_tq >> 4;
                    let table_id = (pq_tq & 0x0F) as usize;
                    if precision != 0 || table_id >= parsed.tables.len() || seg_idx + 64 > seg_end {
                        return None;
                    }
                    for (zigzag_idx, &raster_idx) in JPEG_QM_SCAN_8X8.iter().enumerate() {
                        parsed.tables[table_id][raster_idx] = encoded[seg_idx + zigzag_idx];
                    }
                    parsed.present_mask |= 1 << table_id;
                    seg_idx += 64;
                }
            }
            0xC0..=0xC2 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                if segment_len >= 8 {
                    let component_count = encoded[idx + 5].min(3);
                    parsed.component_count = component_count.max(1);
                    for component_idx in 0..usize::from(component_count) {
                        parsed.component_qtable[component_idx] =
                            encoded[idx + 8 + component_idx * 3] & 0x0F;
                    }
                }
            }
            _ => {}
        }

        idx += segment_len - 2;
    }

    Some(parsed)
}

pub(super) fn parse_jpeg_huff_tables(encoded: &[u8]) -> Option<JpegHuffTables> {
    if encoded.len() < 4 || encoded[0] != 0xFF || encoded[1] != 0xD8 {
        return None;
    }

    let mut parsed = JpegHuffTables {
        dc_bits: [[0; 12]; 4],
        dc_values: [[0; 12]; 4],
        ac_bits: [[0; 16]; 4],
        ac_values: [[0; 162]; 4],
        dc_present_mask: 0,
        ac_present_mask: 0,
        y_dc_selector: 0,
        y_ac_selector: 0,
        chroma_dc_selector: 1,
        chroma_ac_selector: 1,
        has_chroma_selector: false,
    };
    let mut sof_component_ids = [0u8, 1, 2];
    let mut sof_component_count = 1u8;
    let mut saw_scan = false;
    let mut idx = 2usize;

    while idx + 3 < encoded.len() {
        if encoded[idx] != 0xFF {
            idx += 1;
            continue;
        }
        while idx < encoded.len() && encoded[idx] == 0xFF {
            idx += 1;
        }
        if idx >= encoded.len() {
            break;
        }
        let marker = encoded[idx];
        idx += 1;

        if marker == 0xD9 {
            break;
        }
        if matches!(marker, 0x01 | 0xD0..=0xD7) {
            continue;
        }
        if idx + 1 >= encoded.len() {
            break;
        }
        let segment_len = u16::from_be_bytes([encoded[idx], encoded[idx + 1]]) as usize;
        idx += 2;
        if segment_len < 2 || idx + segment_len - 2 > encoded.len() {
            break;
        }

        match marker {
            0xC4 => {
                let mut seg_idx = idx;
                let seg_end = idx + segment_len - 2;
                while seg_idx < seg_end {
                    let tc_th = encoded[seg_idx];
                    seg_idx += 1;
                    let table_class = tc_th >> 4;
                    let table_id = (tc_th & 0x0F) as usize;
                    if table_id >= 4 || seg_idx + 16 > seg_end {
                        return None;
                    }

                    let mut counts = [0u8; 16];
                    counts.copy_from_slice(&encoded[seg_idx..seg_idx + 16]);
                    seg_idx += 16;
                    let symbol_count = counts
                        .iter()
                        .map(|&count| usize::from(count))
                        .sum::<usize>();
                    if seg_idx + symbol_count > seg_end {
                        return None;
                    }

                    match table_class {
                        0 => {
                            if counts[12..].iter().any(|&count| count != 0) || symbol_count > 12 {
                                return None;
                            }
                            parsed.dc_bits[table_id].copy_from_slice(&counts[..12]);
                            parsed.dc_values[table_id][..symbol_count]
                                .copy_from_slice(&encoded[seg_idx..seg_idx + symbol_count]);
                            parsed.dc_present_mask |= 1 << table_id;
                        }
                        1 => {
                            if symbol_count > 162 {
                                return None;
                            }
                            parsed.ac_bits[table_id].copy_from_slice(&counts);
                            parsed.ac_values[table_id][..symbol_count]
                                .copy_from_slice(&encoded[seg_idx..seg_idx + symbol_count]);
                            parsed.ac_present_mask |= 1 << table_id;
                        }
                        _ => return None,
                    }

                    seg_idx += symbol_count;
                }
            }
            0xC0..=0xC2 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                if segment_len >= 8 {
                    let component_count = encoded[idx + 5].min(3);
                    sof_component_count = component_count.max(1);
                    for component_idx in 0..usize::from(component_count) {
                        sof_component_ids[component_idx] = encoded[idx + 6 + component_idx * 3];
                    }
                }
            }
            0xDA => {
                if segment_len < 6 {
                    return None;
                }
                let scan_component_count = encoded[idx].min(3);
                let required_len = 2 + 1 + usize::from(scan_component_count) * 2 + 3;
                if segment_len < required_len {
                    return None;
                }

                let mut saw_y_selector = false;
                for component_idx in 0..usize::from(scan_component_count) {
                    let base = idx + 1 + component_idx * 2;
                    let component_id = encoded[base];
                    let selectors = encoded[base + 1];
                    let dc_selector = selectors >> 4;
                    let ac_selector = selectors & 0x0F;
                    let is_luma = component_id == sof_component_ids[0]
                        || (!saw_y_selector && component_idx == 0)
                        || sof_component_count == 1;

                    if is_luma && !saw_y_selector {
                        parsed.y_dc_selector = dc_selector;
                        parsed.y_ac_selector = ac_selector;
                        saw_y_selector = true;
                    } else if !parsed.has_chroma_selector {
                        parsed.chroma_dc_selector = dc_selector;
                        parsed.chroma_ac_selector = ac_selector;
                        parsed.has_chroma_selector = true;
                    }
                }

                if !saw_y_selector {
                    return None;
                }
                saw_scan = true;
                break;
            }
            _ => {}
        }

        idx += segment_len - 2;
    }

    if !saw_scan {
        return None;
    }

    if (parsed.dc_present_mask & (1 << parsed.y_dc_selector)) == 0
        || (parsed.ac_present_mask & (1 << parsed.y_ac_selector)) == 0
    {
        return None;
    }
    if parsed.has_chroma_selector
        && ((parsed.dc_present_mask & (1 << parsed.chroma_dc_selector)) == 0
            || (parsed.ac_present_mask & (1 << parsed.chroma_ac_selector)) == 0)
    {
        return None;
    }

    Some(parsed)
}

pub(super) fn parse_jpeg_scan_info(encoded: &[u8]) -> Option<JpegScanInfo> {
    if encoded.len() < 4 || encoded[0] != 0xFF || encoded[1] != 0xD8 {
        return None;
    }

    let mut width = 0u32;
    let mut height = 0u32;
    let mut component_ids = [0u8, 1, 2];
    let mut h_sampling = [1u8; 3];
    let mut v_sampling = [1u8; 3];
    let mut component_count = 0u8;
    let mut max_h_sampling = 1u8;
    let mut max_v_sampling = 1u8;
    let mut restart_interval = 0u16;
    let mut saw_sof = false;
    let mut idx = 2usize;

    while idx + 3 < encoded.len() {
        if encoded[idx] != 0xFF {
            idx += 1;
            continue;
        }
        while idx < encoded.len() && encoded[idx] == 0xFF {
            idx += 1;
        }
        if idx >= encoded.len() {
            break;
        }

        let marker = encoded[idx];
        idx += 1;

        if marker == 0xD9 {
            break;
        }
        if matches!(marker, 0x01 | 0xD0..=0xD7) {
            continue;
        }
        if idx + 1 >= encoded.len() {
            break;
        }

        let segment_len = u16::from_be_bytes([encoded[idx], encoded[idx + 1]]) as usize;
        idx += 2;
        if segment_len < 2 || idx + segment_len - 2 > encoded.len() {
            break;
        }
        let payload_len = segment_len - 2;

        match marker {
            0xC0..=0xC2 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                if payload_len < 6 {
                    return None;
                }
                let sof_component_count = encoded[idx + 5].min(3);
                let required_len = 6usize + usize::from(sof_component_count) * 3;
                if sof_component_count == 0 || payload_len < required_len {
                    return None;
                }

                width = u16::from_be_bytes([encoded[idx + 3], encoded[idx + 4]]) as u32;
                height = u16::from_be_bytes([encoded[idx + 1], encoded[idx + 2]]) as u32;
                component_count = sof_component_count;
                max_h_sampling = 1;
                max_v_sampling = 1;
                for component_idx in 0..usize::from(sof_component_count) {
                    let base = idx + 6 + component_idx * 3;
                    component_ids[component_idx] = encoded[base];
                    let sampling = encoded[base + 1];
                    h_sampling[component_idx] = (sampling >> 4).max(1);
                    v_sampling[component_idx] = (sampling & 0x0F).max(1);
                    max_h_sampling = max_h_sampling.max(h_sampling[component_idx]);
                    max_v_sampling = max_v_sampling.max(v_sampling[component_idx]);
                }
                saw_sof = true;
            }
            0xDD => {
                if payload_len != 2 {
                    return None;
                }
                restart_interval = u16::from_be_bytes([encoded[idx], encoded[idx + 1]]);
            }
            0xDA => {
                if !saw_sof || payload_len < 4 {
                    return None;
                }
                let scan_component_count = encoded[idx].min(3);
                let required_len = 1usize + usize::from(scan_component_count) * 2 + 3;
                if scan_component_count == 0 || payload_len < required_len {
                    return None;
                }

                let input_format =
                    jpeg_input_format_from_sampling(component_count, &h_sampling, &v_sampling)?;

                let mut scan_component_mask = 0u8;
                for component_idx in 0..usize::from(scan_component_count) {
                    let scan_component_id = encoded[idx + 1 + component_idx * 2];
                    if let Some(component_pos) = component_ids[..usize::from(component_count)]
                        .iter()
                        .position(|&component_id| component_id == scan_component_id)
                    {
                        scan_component_mask |= 1 << component_pos;
                    }
                }

                let scan_data_offset = idx + required_len;
                let mut scan_end = scan_data_offset;
                while scan_end < encoded.len() {
                    if encoded[scan_end] != 0xFF {
                        scan_end += 1;
                        continue;
                    }
                    if scan_end + 1 >= encoded.len() {
                        scan_end = encoded.len();
                        break;
                    }
                    match encoded[scan_end + 1] {
                        0x00 | 0xD0..=0xD7 => scan_end += 2,
                        0xFF => scan_end += 1,
                        _ => break,
                    }
                }
                if scan_end <= scan_data_offset {
                    return None;
                }

                let interleaved = scan_component_count > 1;
                let mcu_count = if interleaved {
                    let mcu_width = 8u32.saturating_mul(u32::from(max_h_sampling));
                    let mcu_height = 8u32.saturating_mul(u32::from(max_v_sampling));
                    ceil_div_u32(width, mcu_width).saturating_mul(ceil_div_u32(height, mcu_height))
                } else {
                    let scan_component_id = encoded[idx + 1];
                    let component_idx = component_ids[..usize::from(component_count)]
                        .iter()
                        .position(|&component_id| component_id == scan_component_id)
                        .unwrap_or(0);
                    let component_h = u32::from(h_sampling[component_idx]);
                    let component_v = u32::from(v_sampling[component_idx]);
                    let blocks_wide = ceil_div_u32(
                        width.saturating_mul(component_h),
                        8u32.saturating_mul(u32::from(max_h_sampling)),
                    );
                    let blocks_high = ceil_div_u32(
                        height.saturating_mul(component_v),
                        8u32.saturating_mul(u32::from(max_v_sampling)),
                    );
                    blocks_wide.saturating_mul(blocks_high)
                };

                return Some(JpegScanInfo {
                    input_format,
                    scan_data_offset: scan_data_offset as u32,
                    scan_data_length: (scan_end - scan_data_offset) as u32,
                    scan_component_count,
                    scan_component_mask,
                    interleaved,
                    restart_interval,
                    mcu_count,
                });
            }
            _ => {}
        }

        idx += payload_len;
    }

    None
}

pub(super) fn default_decode_engine_and_window()
-> (super::xelp_media2_ngin::MediaEngineDescriptor, super::xelp_media2_ngin::MediaGpuWindowLayout) {
    super::xelp_media2_ngin::default_decode_engine_and_window()
}

pub(super) fn ensure_decode_backing(
    dev: crate::intel::Dev,
    windows: super::xelp_media2_ngin::MediaGpuWindowLayout,
) -> Option<super::xelp_media2_ngin::MediaBitstreamBacking> {
    super::xelp_media2_ngin::ensure_decode_backing(dev, windows)
}

pub(super) fn stream_encoded_to_bitstream(
    dev: crate::intel::Dev,
    engine: super::xelp_media2_ngin::MediaEngineDescriptor,
    windows: super::xelp_media2_ngin::MediaGpuWindowLayout,
    backing: super::xelp_media2_ngin::MediaBitstreamBacking,
    encoded: &[u8],
) -> Option<MediaEncodedStreamProof> {
    super::xelp_media2_ngin::stream_encoded_to_bitstream(dev, engine, windows, backing, encoded)
}

const MEDIA_CMD_OPCODE_MFX_JPEG: u32 = 7;
const MFX_JPEG_PIC_STATE: u32 = 0;
const MFD_JPEG_BSD_OBJECT: u32 = 8;
const MFX_CMD_LEN_JPEG_PIC_STATE: u32 = 1;
const MFX_CMD_LEN_JPEG_BSD_OBJECT: u32 = 4;
const MFX_JPEG_HUFF_TABLE_STATE: u32 = 2;
const MFX_CMD_LEN_JPEG_HUFF_TABLE_STATE: u32 = 51;
const MFX_PIPE_MODE_CODEC_JPEG: u32 = 3;
const MFX_PIPE_MODE_DECODE: u32 = 1 << 9;
const MEDIA_STAGE_FLAG_JPEG_SMOKE: u32 = 1 << 8;
const MEDIA_STAGE_FLAG_JPEG_PIC_STATE: u32 = 1 << 9;
const MEDIA_STAGE_FLAG_JPEG_QM_STATE: u32 = 1 << 10;
const MEDIA_STAGE_FLAG_JPEG_HUFF_STATE: u32 = 1 << 11;
const MEDIA_STAGE_FLAG_JPEG_BSD_OBJECT: u32 = 1 << 12;

#[inline]
fn pack_u8x4(bytes: &[u8]) -> u32 {
    let mut value = 0u32;
    for (shift, byte) in bytes.iter().enumerate() {
        value |= u32::from(*byte) << (shift * 8);
    }
    value
}

fn emit_jpeg_huff_table_state(
    batch: &mut [u32],
    idx: &mut usize,
    huff_table_id: u32,
    dc_bits: &[u8; 12],
    dc_values: &[u8; 12],
    ac_bits: &[u8; 16],
    ac_values: &[u8; 162],
) -> Option<()> {
    let huff = media::begin_batch_packet(
        batch,
        idx,
        (MFX_CMD_LEN_JPEG_HUFF_TABLE_STATE + 2) as usize,
        media::media_cmd_header(
            MEDIA_CMD_OPCODE_MFX_JPEG,
            0,
            MFX_JPEG_HUFF_TABLE_STATE,
            MFX_CMD_LEN_JPEG_HUFF_TABLE_STATE,
        ),
    )?;

    batch[huff + 1] = huff_table_id;
    for dw in 0..3 {
        let base = dw * 4;
        batch[huff + 2 + dw] = pack_u8x4(&dc_bits[base..base + 4]);
        batch[huff + 5 + dw] = pack_u8x4(&dc_values[base..base + 4]);
    }
    for dw in 0..4 {
        let base = dw * 4;
        batch[huff + 8 + dw] = pack_u8x4(&ac_bits[base..base + 4]);
    }
    for dw in 0..40 {
        let base = dw * 4;
        batch[huff + 12 + dw] = pack_u8x4(&ac_values[base..base + 4]);
    }
    batch[huff + 52] = u32::from(ac_values[160]) | (u32::from(ac_values[161]) << 8);
    Some(())
}

fn emit_jpeg_bsd_object(
    batch: &mut [u32],
    idx: &mut usize,
    scan_info: &JpegScanInfo,
) -> Option<()> {
    let bsd = media::begin_batch_packet(
        batch,
        idx,
        (MFX_CMD_LEN_JPEG_BSD_OBJECT + 2) as usize,
        media::media_cmd_header(
            MEDIA_CMD_OPCODE_MFX_JPEG,
            1,
            MFD_JPEG_BSD_OBJECT,
            MFX_CMD_LEN_JPEG_BSD_OBJECT,
        ),
    )?;

    batch[bsd + 1] = scan_info.scan_data_length;
    batch[bsd + 2] = scan_info.scan_data_offset;
    batch[bsd + 3] = 0;
    batch[bsd + 4] = jpeg_bsd_dw4(scan_info);
    batch[bsd + 5] = u32::from(scan_info.restart_interval);
    Some(())
}

#[inline]
fn jpeg_bsd_dw4(scan_info: &JpegScanInfo) -> u32 {
    (scan_info.mcu_count & 0x03ff_ffff)
        | (u32::from(scan_info.scan_component_mask) << 27)
        | ((scan_info.interleaved as u32) << 30)
}

fn imc3_tiled_surface_layout(coded_height: u32, output_pitch: usize) -> Option<(u32, u32, usize)> {
    const YTILE_H: usize = 32;

    if coded_height == 0 || output_pitch == 0 {
        return None;
    }
    let coded_height = coded_height as usize;
    let chroma_y_offset = media::align_up_u32(coded_height as u32, YTILE_H as u32) as usize;
    let chroma_plane_rows = coded_height.div_ceil(2);
    let chroma_plane_stride_rows = chroma_plane_rows.div_ceil(YTILE_H) * YTILE_H;
    let cr_y_offset = chroma_y_offset + chroma_plane_stride_rows;
    let total_height = cr_y_offset + chroma_plane_rows;
    let bytes = total_height.div_ceil(YTILE_H) * output_pitch * YTILE_H;
    Some((chroma_y_offset as u32, cr_y_offset as u32, bytes))
}

fn clear_output_surface_to_imc3_black(
    output_surface_virt: *mut u8,
    output_surface_bytes: usize,
    coded_width: u32,
    coded_height: u32,
    output_pitch: usize,
) -> bool {
    const YTILE_W: usize = 128;
    const YTILE_H: usize = 32;

    #[inline(always)]
    fn ytile_offset(byte_x: usize, row_y: usize, tiles_per_row: usize) -> usize {
        let tile_col = byte_x / YTILE_W;
        let tile_row = row_y / YTILE_H;
        let in_x = byte_x % YTILE_W;
        let in_y = row_y % YTILE_H;
        let oword_col = in_x / 16;
        let byte_in_oword = in_x % 16;
        let within_tile = oword_col * 512 + in_y * 16 + byte_in_oword;
        (tile_row * tiles_per_row + tile_col) * 4096 + within_tile
    }

    if output_surface_virt.is_null()
        || coded_width == 0
        || coded_height == 0
        || output_pitch < coded_width as usize
        || !output_pitch.is_multiple_of(YTILE_W)
    {
        return false;
    }

    let coded_width = coded_width as usize;
    let coded_height = coded_height as usize;
    let tiles_per_row = output_pitch / YTILE_W;
    let Some((chroma_y_offset, cr_y_offset, needed)) =
        imc3_tiled_surface_layout(coded_height as u32, output_pitch)
    else {
        return false;
    };
    let chroma_y_offset = chroma_y_offset as usize;
    let cr_y_offset = cr_y_offset as usize;
    if output_surface_bytes < needed {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(output_surface_virt, 0, output_surface_bytes);
    }

    for chroma_row in 0..coded_height.div_ceil(2) {
        let cb_row = chroma_y_offset + chroma_row;
        let cr_row = cr_y_offset + chroma_row;
        for byte_x in 0..coded_width {
            let cb_offset = ytile_offset(byte_x, cb_row, tiles_per_row);
            let cr_offset = ytile_offset(byte_x, cr_row, tiles_per_row);
            unsafe {
                core::ptr::write_volatile(output_surface_virt.add(cb_offset), 0x80);
                core::ptr::write_volatile(output_surface_virt.add(cr_offset), 0x80);
            }
        }
    }

    super::dma_flush(output_surface_virt, output_surface_bytes);
    true
}

fn build_jpeg_smoke_batch_skeleton(
    batch_virt: *mut u8,
    batch_bytes: usize,
    result_gpu_addr: u64,
    bitstream_gpu_addr: u64,
    output_surface_gpu_addr: u64,
    output_surface_bytes: usize,
    bitstream_bytes: usize,
    coded_width: u32,
    coded_height: u32,
    jpeg_quant_tables: Option<&JpegQuantTables>,
    jpeg_huff_tables: Option<&JpegHuffTables>,
    jpeg_scan_info: Option<&JpegScanInfo>,
    kickoff_marker: u32,
    presubmit_marker: u32,
    postsubmit_marker: u32,
    complete_marker: u32,
) -> Option<usize> {
    let batch = unsafe {
        core::slice::from_raw_parts_mut(
            batch_virt as *mut u32,
            batch_bytes / core::mem::size_of::<u32>(),
        )
    };
    let mut idx = 0usize;
    let output_pitch = media::align_up_u32(coded_width.max(128), 128);
    let (chroma_y_offset, cr_y_offset, _) =
        imc3_tiled_surface_layout(coded_height, output_pitch as usize)?;
    let frame_width_blocks_minus1 =
        (media::align_up_u32(coded_width.max(8), 8) / 8).saturating_sub(1);
    let frame_height_blocks_minus1 =
        (media::align_up_u32(coded_height.max(8), 8) / 8).saturating_sub(1);
    let jpeg_output_format = jpeg_scan_info
        .map(|scan_info| jpeg_output_format_from_input(scan_info.input_format))
        .unwrap_or(0);
    let pipe_mode_dw1 = MFX_PIPE_MODE_CODEC_JPEG | MFX_PIPE_MODE_DECODE;
    let mut stage_flags = MEDIA_STAGE_FLAG_JPEG_SMOKE
        | MEDIA_STAGE_FLAG_JPEG_PIC_STATE
        | MEDIA_STAGE_FLAG_JPEG_QM_STATE;
    if jpeg_huff_tables.is_some() {
        stage_flags |= MEDIA_STAGE_FLAG_JPEG_HUFF_STATE;
    }
    if jpeg_scan_info.is_some() {
        stage_flags |= MEDIA_STAGE_FLAG_JPEG_BSD_OBJECT;
    }

    if !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        result_gpu_addr + media::MEDIA_RESULT_KICKOFF_SLOT,
        kickoff_marker,
    ) || !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        result_gpu_addr + media::MEDIA_RESULT_BITSTREAM_ADDR_LO_SLOT,
        bitstream_gpu_addr as u32,
    ) || !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        result_gpu_addr + media::MEDIA_RESULT_BITSTREAM_ADDR_HI_SLOT,
        (bitstream_gpu_addr >> 32) as u32,
    ) || !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        result_gpu_addr + media::MEDIA_RESULT_BITSTREAM_BYTES_SLOT,
        bitstream_bytes as u32,
    ) || !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        result_gpu_addr + media::MEDIA_RESULT_SAMPLE_NALS_SLOT,
        0,
    ) || !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        result_gpu_addr + media::MEDIA_RESULT_STAGE_FLAGS_SLOT,
        stage_flags,
    ) || !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        result_gpu_addr + media::MEDIA_RESULT_OUTPUT_SURFACE_ADDR_LO_SLOT,
        output_surface_gpu_addr as u32,
    ) || !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        result_gpu_addr + media::MEDIA_RESULT_OUTPUT_SURFACE_ADDR_HI_SLOT,
        (output_surface_gpu_addr >> 32) as u32,
    ) || !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        result_gpu_addr + media::MEDIA_RESULT_OUTPUT_SURFACE_BYTES_SLOT,
        output_surface_bytes as u32,
    ) || !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        result_gpu_addr + media::MEDIA_RESULT_FRAME_DIMS_SLOT,
        coded_width | (coded_height << 16),
    ) {
        return None;
    }

    let presubmit_flush = media::begin_batch_packet(
        batch,
        &mut idx,
        5,
        media::MI_FLUSH_DW
            | media::MI_FLUSH_DW_VIDEO_PIPELINE_CACHE_INVALIDATE
            | media::MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE,
    )?;
    batch[presubmit_flush + 1] = (result_gpu_addr + media::MEDIA_RESULT_PRESUBMIT_SLOT) as u32;
    batch[presubmit_flush + 2] =
        ((result_gpu_addr + media::MEDIA_RESULT_PRESUBMIT_SLOT) >> 32) as u32;
    batch[presubmit_flush + 3] = presubmit_marker;
    batch[presubmit_flush + 4] = 0;

    if idx.saturating_add(2) > batch.len() {
        return None;
    }
    batch[idx] = media::MI_FORCE_WAKEUP;
    batch[idx + 1] = media::MI_FORCE_WAKEUP_MFX_WELL;
    idx += 2;

    if !media::emit_mfx_wait(batch, &mut idx) {
        return None;
    }

    let pipe_mode = media::begin_batch_packet(
        batch,
        &mut idx,
        (media::MFX_CMD_LEN_PIPE_MODE_SELECT + 2) as usize,
        media::media_cmd_header(
            media::MEDIA_CMD_OPCODE_MFX_COMMON,
            0,
            media::MFX_PIPE_MODE_SELECT,
            media::MFX_CMD_LEN_PIPE_MODE_SELECT,
        ),
    )?;
    batch[pipe_mode + 1] = pipe_mode_dw1;

    if !media::emit_mfx_wait(batch, &mut idx) {
        return None;
    }

    let surface = media::begin_batch_packet(
        batch,
        &mut idx,
        (media::MFX_CMD_LEN_SURFACE_STATE + 2) as usize,
        media::media_cmd_header(
            media::MEDIA_CMD_OPCODE_MFX_COMMON,
            0,
            media::MFX_SURFACE_STATE,
            media::MFX_CMD_LEN_SURFACE_STATE,
        ),
    )?;
    batch[surface + 2] =
        ((coded_width.saturating_sub(1)) << 4) | ((coded_height.saturating_sub(1)) << 18);
    batch[surface + 3] = (1 << 1) | 1 | ((output_pitch.saturating_sub(1)) << 3) | (4 << 28);
    batch[surface + 4] = chroma_y_offset;
    batch[surface + 5] = cr_y_offset;

    let pipe_buf = media::begin_batch_packet(
        batch,
        &mut idx,
        (media::MFX_CMD_LEN_PIPE_BUF_ADDR_STATE + 2) as usize,
        media::media_cmd_header(
            media::MEDIA_CMD_OPCODE_MFX_COMMON,
            0,
            media::MFX_PIPE_BUF_ADDR_STATE,
            media::MFX_CMD_LEN_PIPE_BUF_ADDR_STATE,
        ),
    )?;
    media::packet_write_addr64(batch, pipe_buf, 1, output_surface_gpu_addr);
    batch[pipe_buf + 3] = media::MFX_MOCS_UC;
    media::packet_write_addr64(batch, pipe_buf, 4, output_surface_gpu_addr);
    batch[pipe_buf + 6] = media::MFX_MOCS_UC;
    batch[pipe_buf + 9] = media::MFX_MOCS_UC;
    batch[pipe_buf + 12] = media::MFX_MOCS_UC;

    let ind_obj = media::begin_batch_packet(
        batch,
        &mut idx,
        (media::MFX_CMD_LEN_IND_OBJ_BASE_ADDR_STATE + 2) as usize,
        media::media_cmd_header(
            media::MEDIA_CMD_OPCODE_MFX_COMMON,
            0,
            media::MFX_IND_OBJ_BASE_ADDR_STATE,
            media::MFX_CMD_LEN_IND_OBJ_BASE_ADDR_STATE,
        ),
    )?;
    media::packet_write_addr64(batch, ind_obj, 1, bitstream_gpu_addr);
    batch[ind_obj + 3] = media::MFX_MOCS_UC;
    media::packet_write_addr64(batch, ind_obj, 4, bitstream_gpu_addr + bitstream_bytes as u64);
    batch[ind_obj + 8] = media::MFX_MOCS_UC;
    batch[ind_obj + 13] = media::MFX_MOCS_UC;
    batch[ind_obj + 18] = media::MFX_MOCS_UC;
    batch[ind_obj + 23] = media::MFX_MOCS_UC;

    let jpeg_pic = media::begin_batch_packet(
        batch,
        &mut idx,
        (MFX_CMD_LEN_JPEG_PIC_STATE + 2) as usize,
        media::media_cmd_header(
            MEDIA_CMD_OPCODE_MFX_JPEG,
            0,
            MFX_JPEG_PIC_STATE,
            MFX_CMD_LEN_JPEG_PIC_STATE,
        ),
    )?;
    batch[jpeg_pic + 1] = jpeg_scan_info
        .map(|scan_info| u32::from(scan_info.input_format))
        .unwrap_or(0)
        | (u32::from(jpeg_output_format) << 8);
    batch[jpeg_pic + 2] = frame_width_blocks_minus1 | (frame_height_blocks_minus1 << 16);

    let fallback_qm = [16u8; 64];
    let component_count = jpeg_quant_tables
        .map(|tables| tables.component_count.max(1))
        .unwrap_or(1);
    for component_idx in 0..usize::from(component_count) {
        let mut qm_matrix = fallback_qm;
        if let Some(tables) = jpeg_quant_tables {
            let table_selector = usize::from(tables.component_qtable[component_idx] & 0x03);
            if (tables.present_mask & (1 << table_selector)) != 0 {
                qm_matrix = tables.tables[table_selector];
            }
        }

        let qm = media::begin_batch_packet(
            batch,
            &mut idx,
            (media::MFX_CMD_LEN_QM_STATE + 2) as usize,
            media::media_cmd_header(
                media::MEDIA_CMD_OPCODE_MFX_COMMON,
                0,
                media::MFX_QM_STATE,
                media::MFX_CMD_LEN_QM_STATE,
            ),
        )?;
        batch[qm + 1] = component_idx as u32;
        for dw in 0..16 {
            let base = dw * 4;
            batch[qm + 2 + dw] = (qm_matrix[base] as u32)
                | ((qm_matrix[base + 1] as u32) << 8)
                | ((qm_matrix[base + 2] as u32) << 16)
                | ((qm_matrix[base + 3] as u32) << 24);
        }
    }

    if let Some(huff_tables) = jpeg_huff_tables {
        emit_jpeg_huff_table_state(
            batch,
            &mut idx,
            0,
            &huff_tables.dc_bits[usize::from(huff_tables.y_dc_selector)],
            &huff_tables.dc_values[usize::from(huff_tables.y_dc_selector)],
            &huff_tables.ac_bits[usize::from(huff_tables.y_ac_selector)],
            &huff_tables.ac_values[usize::from(huff_tables.y_ac_selector)],
        )?;

        if huff_tables.has_chroma_selector {
            emit_jpeg_huff_table_state(
                batch,
                &mut idx,
                1,
                &huff_tables.dc_bits[usize::from(huff_tables.chroma_dc_selector)],
                &huff_tables.dc_values[usize::from(huff_tables.chroma_dc_selector)],
                &huff_tables.ac_bits[usize::from(huff_tables.chroma_ac_selector)],
                &huff_tables.ac_values[usize::from(huff_tables.chroma_ac_selector)],
            )?;
        }
    }

    if let Some(scan_info) = jpeg_scan_info {
        emit_jpeg_bsd_object(batch, &mut idx, scan_info)?;
    }

    if !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        result_gpu_addr + media::MEDIA_RESULT_POSTSUBMIT_SLOT,
        postsubmit_marker,
    ) {
        return None;
    }

    let done_flush = media::begin_batch_packet(
        batch,
        &mut idx,
        5,
        media::MI_FLUSH_DW
            | media::MI_FLUSH_DW_VIDEO_PIPELINE_CACHE_INVALIDATE
            | media::MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE,
    )?;
    batch[done_flush + 1] = (result_gpu_addr + media::MEDIA_RESULT_COMPLETE_SLOT) as u32;
    batch[done_flush + 2] = ((result_gpu_addr + media::MEDIA_RESULT_COMPLETE_SLOT) >> 32) as u32;
    batch[done_flush + 3] = complete_marker;
    batch[done_flush + 4] = 0;

    if idx.saturating_add(3) > batch.len() {
        return None;
    }
    batch[idx] = media::MI_ARB_CHECK;
    batch[idx + 1] = media::MI_BATCH_BUFFER_END;
    batch[idx + 2] = media::MI_NOOP;
    Some((idx + 3).saturating_mul(core::mem::size_of::<u32>()))
}

pub(super) fn submit_jpeg_smoke_batch(
    dev: crate::intel::Dev,
    engine: MediaEngineDescriptor,
    windows: MediaGpuWindowLayout,
    backing: MediaBitstreamBacking,
    bitstream_bytes: usize,
    submit_token: u32,
) -> Option<MediaJpegSmokeSubmitProof> {
    if bitstream_bytes == 0 || bitstream_bytes > backing.bitstream_bytes {
        return None;
    }

    let ring_virt = backing.ring_virt;
    let context_virt = backing.context_virt;
    let ring_gpu_addr = windows.ring_gpu_addr;
    let context_gpu_addr = windows.context_gpu_addr;
    let kickoff_marker = media::marker_base(engine)
        .wrapping_add(0x80)
        .wrapping_add((submit_token & 0x3F) << 2);
    let ring_prelaunch_marker = kickoff_marker.wrapping_sub(1);
    let presubmit_marker = kickoff_marker + 1;
    let postsubmit_marker = kickoff_marker + 2;
    let complete_marker = kickoff_marker + 3;

    media::reset_media_engine(dev, engine, context_virt);
    media::wake_media_engine_forcewake(dev, engine);

    unsafe {
        core::ptr::write_bytes(ring_virt, 0, backing.ring_bytes);
        core::ptr::write_bytes(context_virt, 0, backing.context_bytes);
        core::ptr::write_bytes(backing.batch_virt, 0, backing.batch_bytes);
        core::ptr::write_bytes(backing.result_virt, 0, backing.result_bytes);
    }

    let bitstream = unsafe {
        core::slice::from_raw_parts(backing.bitstream_virt as *const u8, bitstream_bytes)
    };
    let (coded_width, coded_height) = parse_jpeg_frame_dims(bitstream).unwrap_or((128, 128));
    let jpeg_quant_tables = parse_jpeg_quant_tables(bitstream);
    let jpeg_huff_tables = parse_jpeg_huff_tables(bitstream);
    let jpeg_scan_info = parse_jpeg_scan_info(bitstream);
    let output_surface_pitch = media::align_up_u32(coded_width.max(128), 128) as usize;
    let frame_width_blocks_minus1 =
        (media::align_up_u32(coded_width.max(8), 8) / 8).saturating_sub(1);
    let frame_height_blocks_minus1 =
        (media::align_up_u32(coded_height.max(8), 8) / 8).saturating_sub(1);
    let jpeg_input_format = jpeg_scan_info
        .as_ref()
        .map(|scan_info| scan_info.input_format)
        .unwrap_or(0);
    let jpeg_output_format = jpeg_output_format_from_input(jpeg_input_format);
    let (chroma_y_offset, cr_y_offset, output_surface_bytes) =
        imc3_tiled_surface_layout(coded_height, output_surface_pitch).unwrap_or((0, 0, 0));
    if output_surface_bytes == 0 || output_surface_bytes > backing.output_surface_bytes {
        return None;
    }
    if !clear_output_surface_to_imc3_black(
        backing.output_surface_virt,
        backing.output_surface_bytes,
        coded_width,
        coded_height,
        output_surface_pitch,
    ) {
        return None;
    }
    let surface_dw2 =
        ((coded_width.saturating_sub(1)) << 4) | ((coded_height.saturating_sub(1)) << 18);
    let surface_dw3 = (1 << 1)
        | 1
        | ((u32::try_from(output_surface_pitch)
            .unwrap_or(u32::MAX)
            .saturating_sub(1))
            << 3)
        | (4 << 28);
    let pipe_mode_dw1 = MFX_PIPE_MODE_CODEC_JPEG | MFX_PIPE_MODE_DECODE;
    let jpeg_pic_dw1 = u32::from(jpeg_input_format) | (u32::from(jpeg_output_format) << 8);
    let jpeg_pic_dw2 = frame_width_blocks_minus1 | (frame_height_blocks_minus1 << 16);

    let batch_tail_bytes = build_jpeg_smoke_batch_skeleton(
        backing.batch_virt,
        backing.batch_bytes,
        windows.result_gpu_addr,
        windows.bitstream_gpu_addr,
        windows.output_surface_gpu_addr,
        output_surface_bytes,
        bitstream_bytes,
        coded_width,
        coded_height,
        jpeg_quant_tables.as_ref(),
        jpeg_huff_tables.as_ref(),
        jpeg_scan_info.as_ref(),
        kickoff_marker,
        presubmit_marker,
        postsubmit_marker,
        complete_marker,
    )?;

    let ring_tail_bytes = media::build_ring_batch_start_words(
        ring_virt,
        backing.ring_bytes,
        0,
        windows.result_gpu_addr,
        ring_prelaunch_marker,
        windows.batch_gpu_addr,
    )?;
    let ring_ctl = media::ring_ctl_value_for_size(backing.ring_bytes)?;
    let ring_start = ring_gpu_addr as u32;
    let pphwsp_gpu = (context_gpu_addr & !0xFFF) as u32;
    let ctx_ctl_after = media::media_ctx_control_value(false);
    if !media::init_gen12_video_context_image(
        context_virt,
        backing.context_bytes,
        engine.ring_base,
        0,
        ring_start,
        ring_tail_bytes as u32,
        ring_ctl,
        pphwsp_gpu,
        backing.ppgtt_pml4_phys,
        false,
    ) {
        return None;
    }

    {
        let mode_bits = media::GFX_RUN_LIST_ENABLE | media::GEN11_GFX_DISABLE_LEGACY_MODE;
        super::mmio_write(
            dev,
            engine.ring_base + media::RING_MODE_GEN7,
            mode_bits | (mode_bits << 16),
        );
    }
    media::seed_media_ring_live_state(
        dev,
        engine.ring_base,
        pphwsp_gpu,
        ring_start,
        ring_ctl,
        ring_tail_bytes as u32,
    );
    media::init_csb_pointers(dev, engine.ring_base, context_virt);

    super::dma_flush(backing.batch_virt, batch_tail_bytes);
    super::dma_flush(ring_virt, ring_tail_bytes);
    super::dma_flush(context_virt, backing.context_bytes);
    super::dma_flush(backing.result_virt, backing.result_bytes);

    {
        super::mmio_write(dev, engine.ring_base + media::RING_CONTEXT_CONTROL, ctx_ctl_after);
        super::mmio_write(dev, engine.ring_base + media::RING_CONTEXT_CONTROL_REF, ctx_ctl_after);
        super::mmio_write(
            dev,
            engine.ring_base + media::RING_MI_MODE,
            media::masked_bit_disable(media::STOP_RING),
        );
        super::mmio_write(dev, engine.ring_base + media::RING_HWS_PGA, pphwsp_gpu);
    }

    let submit_counter = submit_token.wrapping_add(1) & 0x3F;
    let (ctx_desc_lo, ctx_desc_hi) = media::build_media_execlist_context_descriptor(
        context_gpu_addr,
        engine,
        submit_counter,
        true,
    );
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    media::execlist_submit_port_push(dev, engine.ring_base, ctx_desc_lo, ctx_desc_hi, 0, 0);
    super::mmio_write(dev, engine.ring_base + media::RING_EXECLIST_CONTROL, media::EL_CTRL_LOAD);

    let mut retired = false;
    let mut poll_iters = 0usize;
    let mut complete_value = 0u32;
    while poll_iters < media::MEDIA_SUBMIT_POLL_ITERS {
        super::dma_flush(
            unsafe {
                backing
                    .result_virt
                    .add(media::MEDIA_RESULT_COMPLETE_SLOT as usize)
            },
            8,
        );
        complete_value =
            media::read_result_dword(backing.result_virt, media::MEDIA_RESULT_COMPLETE_SLOT);
        if complete_value == complete_marker {
            retired = true;
            break;
        }
        core::hint::spin_loop();
        poll_iters += 1;
    }

    super::dma_flush(backing.output_surface_virt, output_surface_bytes);
    super::dma_flush(backing.result_virt, backing.result_bytes);
    let output_surface = unsafe {
        core::slice::from_raw_parts(backing.output_surface_virt as *const u8, output_surface_bytes)
    };
    let output_surface_probe = media::probe_output_surface(
        output_surface,
        u16::try_from(coded_width).unwrap_or(u16::MAX),
        u16::try_from(coded_height).unwrap_or(u16::MAX),
        0,
        0,
        u16::try_from(coded_width).unwrap_or(u16::MAX),
        u16::try_from(coded_height).unwrap_or(u16::MAX),
        output_surface_pitch,
    );
    media::log_output_surface_probe(engine.name, submit_token, retired, output_surface_probe);
    let output_surface_detail = media::output_surface_has_decoded_detail(&output_surface_probe);
    let (output_surface_signature, output_surface_nonzero_samples) =
        media::surface_signature(output_surface);
    if !output_surface_detail {
        crate::log!(
            "intel/media2: jpeg blank-surface engine={} sample={} retired={} detail_range=0 sig=0x{:08X} nonzero_samples={}\n",
            engine.name,
            submit_token,
            retired as u8,
            output_surface_signature,
            output_surface_nonzero_samples,
        );
    }

    let ring_acthd = super::mmio_read(dev, engine.ring_base + media::RING_ACTHD);
    let ring_acthd_hi = super::mmio_read(dev, engine.ring_base + media::RING_ACTHD_UDW);
    let bbaddr_lo = super::mmio_read(dev, engine.ring_base + media::RING_BBADDR);
    let bbaddr_hi = super::mmio_read(dev, engine.ring_base + media::RING_BBADDR_UDW);
    let dma_fadd_lo = super::mmio_read(dev, engine.ring_base + media::RING_DMA_FADD);
    let dma_fadd_hi = super::mmio_read(dev, engine.ring_base + media::RING_DMA_FADD_UDW);
    let fault_gen8 = super::mmio_read(dev, 0x4094);
    let fault_gen12 = super::mmio_read(dev, media::GEN12_RING_FAULT_REG);
    let (acthd_region, acthd_offset_bytes, acthd_dword) = media::classify_media_acthd(
        ring_acthd,
        windows,
        backing,
        batch_tail_bytes,
        ring_tail_bytes,
    );

    Some(MediaJpegSmokeSubmitProof {
        engine_name: engine.name,
        batch_gpu_addr: windows.batch_gpu_addr,
        result_gpu_addr: windows.result_gpu_addr,
        bitstream_gpu_addr: windows.bitstream_gpu_addr,
        output_surface_gpu_addr: windows.output_surface_gpu_addr,
        bitstream_bytes,
        coded_width,
        coded_height,
        jpeg_input_format,
        jpeg_output_format,
        jpeg_scan_component_count: jpeg_scan_info
            .as_ref()
            .map(|scan_info| scan_info.scan_component_count)
            .unwrap_or(0),
        jpeg_interleaved: jpeg_scan_info
            .as_ref()
            .map(|scan_info| scan_info.interleaved)
            .unwrap_or(false),
        jpeg_restart_interval: jpeg_scan_info
            .as_ref()
            .map(|scan_info| scan_info.restart_interval)
            .unwrap_or(0),
        jpeg_mcu_count: jpeg_scan_info
            .as_ref()
            .map(|scan_info| scan_info.mcu_count)
            .unwrap_or(0),
        jpeg_scan_data_offset: jpeg_scan_info
            .as_ref()
            .map(|scan_info| scan_info.scan_data_offset)
            .unwrap_or(0),
        jpeg_scan_data_length: jpeg_scan_info
            .as_ref()
            .map(|scan_info| scan_info.scan_data_length)
            .unwrap_or(0),
        jpeg_bsd_dw4: jpeg_scan_info.as_ref().map(jpeg_bsd_dw4).unwrap_or(0),
        output_surface_pitch,
        output_surface_bytes,
        surface_dw2,
        surface_dw3,
        surface_dw4: chroma_y_offset,
        surface_dw5: cr_y_offset,
        pipe_mode_dw1,
        jpeg_pic_dw1,
        jpeg_pic_dw2,
        output_surface_detail,
        batch_tail_bytes,
        ring_tail_bytes,
        kickoff_marker,
        presubmit_marker,
        postsubmit_marker,
        complete_marker,
        kickoff_value: media::read_result_dword(
            backing.result_virt,
            media::MEDIA_RESULT_KICKOFF_SLOT,
        ),
        presubmit_value: media::read_result_dword(
            backing.result_virt,
            media::MEDIA_RESULT_PRESUBMIT_SLOT,
        ),
        postsubmit_value: media::read_result_dword(
            backing.result_virt,
            media::MEDIA_RESULT_POSTSUBMIT_SLOT,
        ),
        complete_value,
        retired,
        poll_iters,
        execlist_status_lo: super::mmio_read(
            dev,
            engine.ring_base + media::RING_EXECLIST_STATUS_LO,
        ),
        execlist_status_hi: super::mmio_read(
            dev,
            engine.ring_base + media::RING_EXECLIST_STATUS_HI,
        ),
        ring_start: super::mmio_read(dev, engine.ring_base + media::RING_START),
        ring_ctl: super::mmio_read(dev, engine.ring_base + media::RING_CTL),
        ring_hws_pga: super::mmio_read(dev, engine.ring_base + media::RING_HWS_PGA),
        ring_head: super::mmio_read(dev, engine.ring_base + media::RING_HEAD),
        ring_tail: super::mmio_read(dev, engine.ring_base + media::RING_TAIL),
        ring_acthd,
        ring_acthd_hi,
        acthd_region,
        acthd_offset_bytes,
        acthd_dword,
        bbaddr_lo,
        bbaddr_hi,
        dma_fadd_lo,
        dma_fadd_hi,
        bbstate: super::mmio_read(dev, engine.ring_base + media::RING_BBSTATE),
        esr: super::mmio_read(dev, engine.ring_base + media::RING_ESR),
        instps: super::mmio_read(dev, engine.ring_base + media::RING_INSTPS),
        psmi_ctl: super::mmio_read(dev, engine.ring_base + media::RING_PSMI_CTL),
        nopid: super::mmio_read(dev, engine.ring_base + media::RING_NOPID),
        ipeir: super::mmio_read(dev, engine.ring_base + media::RING_IPEIR),
        ipehr: super::mmio_read(dev, engine.ring_base + media::RING_IPEHR),
        fault_gen8,
        fault_gen12,
        fault_tlb_data0_gen8: super::mmio_read(dev, 0x4B10),
        fault_tlb_data1_gen8: super::mmio_read(dev, 0x4B14),
        fault_tlb_data0_gen12: super::mmio_read(dev, 0xCEB8),
        fault_tlb_data1_gen12: super::mmio_read(dev, 0xCEBC),
        stage_flags_value: media::read_result_dword(
            backing.result_virt,
            media::MEDIA_RESULT_STAGE_FLAGS_SLOT,
        ),
        bitstream_dword0: media::sample_buffer_dword(
            backing.bitstream_virt,
            backing.bitstream_bytes,
            0,
        ),
    })
}
