// Included by probe_vid_fs.py after the production host-compatible helpers.
fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("asset path");
    let mut metadata = std::fs::File::create(args.next().expect("metadata path")).unwrap();
    let (data, timing, source, container) =
        h264_prepare_trueosfs_asset(std::fs::read(path).unwrap()).expect("demux asset");
    println!("container={container} annexb_bytes={} timed_samples={}", data.len(), timing.len());
    let mut reader = H264MemoryNalReader::new(data, source);
    let mut last_sps = None;
    let mut last_pps = None;
    let mut pending_au: Option<H264AccessUnitBuilder> = None;
    let mut access_units = Vec::new();
    let mut skipped_missing_headers = 0usize;
    let mut stopped_at = 0u64;
    let mut nal_count = 0usize;
    let mut vcl_nals_seen = 0usize;
    // PRODUCTION_ACCESS_UNIT_COLLECTOR
    println!("nals={nal_count} vcl_nals={vcl_nals_seen} scanned_bytes={stopped_at}");
    assert_eq!(skipped_missing_headers, 0, "missing parameter sets");
    assert!(!access_units.is_empty(), "no pictures collected");
    if !timing.is_empty() {
        assert_eq!(timing.len(), access_units.len(), "sample/AU count mismatch");
    }
    let mut counts = [0usize; 3];
    let mut max_slices = 0;
    for (index, unit) in access_units.iter().enumerate() {
        let frame = [
            unit.sps.as_slice(),
            unit.pps.as_slice(),
            unit.data.as_slice(),
        ]
        .concat();
        let mut plan = h264_cmd::parse_annexb_single_picture_plan(&frame)
            .unwrap_or_else(|err| panic!("frame {index}: parse {err:?}"));
        if index == 0 {
            println!(
                "coded={}x{} visible={}x{} entropy={} poc_type={}",
                plan.picture.coded_width(),
                plan.picture.coded_height(),
                plan.picture.visible_width,
                plan.picture.visible_height,
                if plan.picture.entropy_coding_mode {
                    "cabac"
                } else {
                    "cavlc"
                },
                plan.picture.pic_order_cnt_type,
            );
        }
        // Synthetic, non-overlapping GPU windows; no allocation or submission
        // on a GPU. Commit each successfully built frame to exercise the real
        // reference history, including IDR resets and ref-list modifications.
        let surface_bytes = plan.resources.dest_surface.byte_len;
        let base = 0x1_0000_0000u64;
        let scratch = base + 0x2000_0000;
        assert!(surface_bytes * 16 <= 0x1000_0000, "probe surface window exhausted");
        let layout = avc_dpb_probe_layout(base, surface_bytes * 16, surface_bytes).unwrap();
        let (slot, refs, surfaces, _) =
            avc_prepare_reference_state(&mut plan, layout, base, scratch)
                .unwrap_or_else(|err| panic!("frame {index}: references {err}"));
        let bindings = avc_scratch_bindings(
            plan,
            slot,
            16,
            base + (slot * layout.slot_bytes) as u64,
            surface_bytes,
            base + 0x1000_0000,
            surface_bytes,
            surfaces,
            base + 0x3000_0000,
            8 * 1024 * 1024,
            scratch,
            64 * 1024 * 1024,
        );
        let commands =
            h264_cmd::build_long_format_single_picture_command_stream(plan, bindings, refs)
                .unwrap_or_else(|err| panic!("frame {index}: command build {err:?}"));
        assert!(h264_cmd::validate_long_format_single_picture_command_stream_shape(&commands));
        avc_commit_decoded_reference(plan, slot);
        counts[match plan.slice.class {
            h264_cmd::AvcSliceClass::I => 0,
            h264_cmd::AvcSliceClass::P => 1,
            h264_cmd::AvcSliceClass::B => 2,
            other => panic!("unexpected class {other:?}"),
        }] += 1;
        max_slices = max_slices.max(plan.slice_count);
        let stamp = timing.get(index).copied().unwrap_or(H264SampleTiming {
            dts: 0,
            pts: 0,
            duration: 0,
            timescale: 0,
        });
        writeln!(
            metadata,
            "{} {} {} {} {} {}",
            plan.picture.visible_width,
            plan.picture.visible_height,
            stamp.dts,
            stamp.pts,
            stamp.duration,
            stamp.timescale
        )
        .unwrap();
    }
    println!("commands_validated={} I/P/B={counts:?} max_slices={max_slices}", access_units.len());
}
