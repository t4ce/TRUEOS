#![allow(dead_code)]
extern crate alloc;

#[path = "@AVC_PATH@"]
mod avc;

#[path = "@SFC_PATH@"]
mod sfc_cmd;

mod backend {
    mod media {
        @MEDIA@
    }
    @BACKEND@

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::avc::*;

        #[derive(Default)]
        struct Bits(Vec<bool>);
        impl Bits {
            fn bit(&mut self, value: bool) { self.0.push(value); }
            fn fixed(&mut self, value: u32, count: u32) {
                for bit in (0..count).rev() { self.bit(value & (1 << bit) != 0); }
            }
            fn ue(&mut self, value: u32) {
                let code = value + 1;
                let bits = 32 - code.leading_zeros();
                for _ in 1..bits { self.bit(false); }
                self.fixed(code, bits);
            }
            fn se(&mut self, value: i32) {
                self.ue(if value > 0 { value as u32 * 2 - 1 } else { value.unsigned_abs() * 2 });
            }
            fn bytes(mut self) -> Vec<u8> {
                self.bit(true);
                while !self.0.len().is_multiple_of(8) { self.bit(false); }
                self.0.chunks(8).map(|bits| bits.iter().fold(0, |n, b| n * 2 + *b as u8)).collect()
            }
        }

        fn nal(out: &mut Vec<u8>, header: u8, payload: Vec<u8>) {
            out.extend_from_slice(&[0, 0, 1, header]);
            // Annex-B must escape RBSP zeros independently of the parser.
            let mut zeros = 0;
            for byte in payload {
                if zeros == 2 && byte <= 3 { out.push(3); zeros = 0; }
                out.push(byte);
                zeros = if byte == 0 { zeros + 1 } else { 0 };
            }
        }

        fn access_unit(class: AvcSliceClass, entropy: bool, cabac: &[u32]) -> Vec<u8> {
            let mut out = Vec::new();
            let mut sps = Bits::default();
            sps.fixed(77, 8); sps.fixed(0, 8); sps.fixed(40, 8);
            sps.ue(0); sps.ue(0); sps.ue(0); sps.ue(0); // SPS, frame_num, POC type/width
            sps.ue(1); sps.bit(false); // one reference, no gaps
            sps.ue(1); sps.ue(1); // 2x2 macroblocks
            sps.bit(true); sps.bit(true); // progressive, direct 8x8 inference
            sps.bit(false); sps.bit(false); // no cropping/VUI
            nal(&mut out, 0x67, sps.bytes());
            let mut pps = Bits::default();
            pps.ue(0); pps.ue(0); pps.bit(entropy); pps.bit(false);
            pps.ue(0); pps.ue(0); pps.ue(0); // no groups, default reference indices
            pps.bit(false); pps.fixed(0, 2); // no weighted prediction
            pps.se(0); pps.se(0); pps.se(0);
            pps.bit(true); pps.bit(false); pps.bit(false); // deblock control
            nal(&mut out, 0x68, pps.bytes());
            for (index, &idc) in cabac.iter().enumerate() {
                let mut slice = Bits::default();
                slice.ue(index as u32 * 2);
                slice.ue(match class { AvcSliceClass::P => 0, AvcSliceClass::B => 1, _ => 2 });
                slice.ue(0); slice.fixed(1, 4); slice.fixed(2, 4);
                if class == AvcSliceClass::B { slice.bit(true); }
                if matches!(class, AvcSliceClass::P | AvcSliceClass::B) {
                    slice.bit(false); // no reference-count override
                    slice.bit(false); // no list-0 modification
                    if class == AvcSliceClass::B { slice.bit(false); }
                }
                slice.bit(false); // adaptive reference marking disabled
                if entropy && matches!(class, AvcSliceClass::P | AvcSliceClass::B) { slice.ue(idc); }
                slice.se(-2); slice.ue(1); // QP delta, deblocking disabled
                nal(&mut out, 0x41, slice.bytes());
            }
            out
        }

        #[test]
        fn cabac_slice_header_reaches_the_hardware_state_for_all_legal_tables() {
            for class in [AvcSliceClass::P, AvcSliceClass::B] {
                for idc in 0..=2 {
                    let plan = parse_annexb_single_picture_plan(&access_unit(class, true, &[idc])).unwrap();
                    assert_eq!(plan.slice.cabac_init_idc, idc as u8);
                    assert_eq!(plan.slice.slice_qp_delta, -2);
                    let command = encode_mfx_avc_slice_state(
                        mfx_avc_slice_state_params_for_single_idr(plan)
                    );
                    assert_eq!((command.dwords[3] >> 24) & 3, idc);
                    assert_eq!((command.dwords[3] >> 16) & 0x3F, 24);
                }
            }
        }

        #[test]
        fn independent_slices_keep_their_own_cabac_initialization() {
            let plan = parse_annexb_single_picture_plan(&access_unit(AvcSliceClass::P, true, &[1, 2])).unwrap();
            assert_eq!(plan.slice_count, 2);
            for index in 0..2 {
                let command = encode_mfx_avc_slice_state(mfx_avc_slice_state_params_for_slice(
                    plan, plan.slices[index], index, index == 1,
                ));
                assert_eq!((command.dwords[3] >> 24) & 3, index as u32 + 1);
            }
        }

        #[test]
        fn absent_cabac_syntax_keeps_qp_and_deblock_fields_aligned() {
            for (class, entropy) in [(AvcSliceClass::I, true), (AvcSliceClass::P, false)] {
                let plan = parse_annexb_single_picture_plan(&access_unit(class, entropy, &[2])).unwrap();
                assert_eq!(plan.slice.cabac_init_idc, 0);
                assert_eq!(plan.slice.slice_qp_delta, -2);
                assert_eq!(plan.slice.disable_deblocking_filter_idc, 1);
            }
        }

        #[test]
        fn reserved_cabac_table_is_rejected_instead_of_truncated() {
            for idc in [3, 4, 255] {
                assert!(matches!(
                    parse_annexb_single_picture_plan(&access_unit(AvcSliceClass::P, true, &[idc])),
                    Err(AvcAnnexBPlanError::UnsupportedSlice),
                ));
            }
        }

        #[test]
        fn command_validation_rejects_reserved_cabac_values_from_constructed_plans() {
            let mut plan = parse_annexb_single_picture_plan(&access_unit(AvcSliceClass::I, true, &[0])).unwrap();
            plan.slices[0].cabac_init_idc = 3;
            let references = AvcReferenceState {
                refs: [None; 16], ref_count: 0, l0: [0; 16], l0_count: 0,
                l1: [0; 16], l1_count: 0,
            };
            assert!(matches!(validate_long_format_single_picture(plan, references),
                Err(AvcCommandStreamBlocker::Milestone(AvcMilestoneBlocker::InvalidSliceRange))));
        }

        #[test]
        fn platform_and_physical_engine_select_the_status_mmio_map() {
            for device in [0x4680, 0xA780] {
                assert_eq!(avc_decode_status_registers(device, 0x1C0000), Some((0x1C0800, 0x1C0868)));
                assert_eq!(avc_decode_status_registers(device, 0x1D0000), Some((0x1D0800, 0x1D0868)));
                assert_eq!(avc_decode_status_registers(device, 0x1C4000), None);
            }
            assert_eq!(avc_decode_status_registers(0xFFFF, 0x1D0000), None);
        }

        #[test]
        fn health_uses_intels_error_mask_and_preserves_concealment_as_a_count() {
            assert!(!AvcDecodeHealth::from_raw(true, 0, 0).known_bad());
            assert!(!AvcDecodeHealth::from_raw(true, 1 << 10, 0).known_bad());
            for bit in 0..16 {
                assert_eq!(AvcDecodeHealth::from_raw(true, 1 << bit, 0).known_bad(), bit != 10);
            }
            let health = AvcDecodeHealth::from_raw(true, 0, (123 << 16) | 7);
            assert_eq!(health.concealed_macroblocks(), 7);
            assert!(!health.known_bad()); // count alone does not widen Intel's failure policy
            assert!(!AvcDecodeHealth::from_raw(false, 1, 1).known_bad());
            assert!(!AvcDecodeHealth::from_raw(true, u32::MAX, u32::MAX).captured);
            assert!(!AvcDecodeHealth::from_raw(true, 1, u32::MAX).known_bad());
        }

        fn batch(registers: Option<(u32, u32)>) -> Vec<u32> {
            let mut words = vec![0; 256];
            let codec = [0x1122_3344, media::MI_FLUSH_DW | media::MI_FLUSH_DW_VIDEO_PIPELINE_CACHE_INVALIDATE, 0, 0, 0, 0];
            let bytes = build_avc_single_idr_batch_skeleton(
                words.as_mut_ptr().cast(), words.len() * 4,
                0x1_0010_0000, 0x2000, 0x3000, 4096, 100,
                32, 32, &codec, 0xA0, 0xA1, 0xA2, 0xA3, registers,
            ).unwrap();
            words.truncate(bytes / 4);
            words
        }

        #[test]
        fn register_stores_execute_after_codec_flush_before_completion_with_ppgtt_destinations() {
            let words = batch(avc_decode_status_registers(0x4680, 0x1D0000));
            let codec = words.iter().position(|&word| word == 0x1122_3344).unwrap();
            let status = codec + 6;
            assert_eq!(&words[status..status + 8], &[
                AVC_MI_STORE_REGISTER_MEM_PPGTT, 0x1D0800, 0x0010_0100, 1,
                AVC_MI_STORE_REGISTER_MEM_PPGTT, 0x1D0868, 0x0010_0108, 1,
            ]);
            let complete = words.iter().rposition(|&word| word == 0xA3).unwrap();
            assert!(complete > status + 8);
            assert_eq!(words[complete - 3], media::MI_FLUSH_DW
                | media::MI_FLUSH_DW_VIDEO_PIPELINE_CACHE_INVALIDATE
                | media::MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE);
        }

        #[test]
        fn unsupported_platform_emits_no_status_reads_and_short_buffers_cannot_submit() {
            let words = batch(None);
            assert!(!words.contains(&AVC_MI_STORE_REGISTER_MEM_PPGTT));
            let mut words = [0; 7];
            let mut index = 0;
            assert_eq!(emit_avc_decode_status(&mut words, &mut index, 0x1000, Some((0x1D0800, 0x1D0868))), None);
        }
    }
}
