// Hardware-picture mission slice of the broader media NGIN backend.
//
// This module keeps the hw_pic-facing API narrow so the boot-logo JPEG path
// can evolve without dragging MP4/H.264/demo-first-frame surfaces into view.
// The heavy shared media backend remains in xelp_media2_ngin for now; this
// module is the focused entry surface for the logo decode mission.

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
    pub output_surface_pitch: usize,
    pub output_surface_bytes: usize,
    pub surface_dw2: u32,
    pub surface_dw3: u32,
    pub jpeg_pic_dw1: u32,
    pub jpeg_pic_dw2: u32,
    pub output_surface_signature: u32,
    pub output_surface_nonzero_samples: usize,
    pub output_surface_probe: super::xelp_media2_ngin::MediaSurfaceProbe,
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
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40,
    48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29,
    22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54,
    47, 55, 62, 63,
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
                    let symbol_count = counts.iter().map(|&count| usize::from(count)).sum::<usize>();
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

                let input_format = jpeg_input_format_from_sampling(
                    component_count,
                    &h_sampling,
                    &v_sampling,
                )?;

                let mut scan_component_ids = [0u8; 3];
                for component_idx in 0..usize::from(scan_component_count) {
                    scan_component_ids[component_idx] = encoded[idx + 1 + component_idx * 2];
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
                    ceil_div_u32(width, mcu_width)
                        .saturating_mul(ceil_div_u32(height, mcu_height))
                } else {
                    let scan_component_id = scan_component_ids[0];
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

pub(super) fn default_decode_engine_and_window(
) -> (
    super::xelp_media2_ngin::MediaEngineDescriptor,
    super::xelp_media2_ngin::MediaGpuWindowLayout,
) {
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

pub(super) fn submit_jpeg_smoke_batch(
    dev: crate::intel::Dev,
    engine: super::xelp_media2_ngin::MediaEngineDescriptor,
    windows: super::xelp_media2_ngin::MediaGpuWindowLayout,
    backing: super::xelp_media2_ngin::MediaBitstreamBacking,
    bitstream_bytes: usize,
    submit_token: u32,
) -> Option<MediaJpegSmokeSubmitProof> {
    super::xelp_media2_ngin::submit_jpeg_smoke_batch(
        dev,
        engine,
        windows,
        backing,
        bitstream_bytes,
        submit_token,
    )
}