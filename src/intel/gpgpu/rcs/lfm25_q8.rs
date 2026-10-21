const LFM25_Q8_LEGACY_BASE_COMMAND_DWORDS: usize = 94;
const LFM25_Q8_COMMAND_DWORDS_PER_PROJECTION: usize = 21;
const LFM25_Q8_PHASE_PROBE_EXTRA_COMMAND_DWORDS: usize = 12;

const fn lfm25_q8_result_end_slot(phase_probe_sampled: bool) -> usize {
    if phase_probe_sampled {
        LFM25_Q8_GPU_POST_RELEASE_TIMESTAMP_SLOT + 2
    } else {
        LFM25_Q8_GPU_END_TIMESTAMP_SLOT + 2
    }
}

const fn lfm25_q8_encoded_command_dwords(
    projection_count: usize,
    phase_probe_sampled: bool,
) -> usize {
    LFM25_Q8_LEGACY_BASE_COMMAND_DWORDS
        + projection_count * LFM25_Q8_COMMAND_DWORDS_PER_PROJECTION
        + if phase_probe_sampled {
            LFM25_Q8_PHASE_PROBE_EXTRA_COMMAND_DWORDS
        } else {
            0
        }
}

const _: () = {
    // Keep the unsampled batch byte-for-byte on the legacy command-length and
    // result-flush extent. The sampled variant adds two six-DWord timestamps.
    assert!(lfm25_q8_result_end_slot(false) == LFM25_Q8_GPU_END_TIMESTAMP_SLOT + 2);
    assert!(
        (lfm25_q8_result_end_slot(false) - LFM25_Q8_POST_MARKER_SLOT) * core::mem::size_of::<u32>()
            == 24
    );
    assert!(
        (lfm25_q8_result_end_slot(true) - LFM25_Q8_POST_MARKER_SLOT) * core::mem::size_of::<u32>()
            == 40
    );
    assert!(lfm25_q8_encoded_command_dwords(LFM25_Q8_MAX_BATCH_PROJECTIONS, false) == 157);
    assert!(lfm25_q8_encoded_command_dwords(LFM25_Q8_MAX_BATCH_PROJECTIONS, true) == 169);
    assert!(
        lfm25_q8_encoded_command_dwords(LFM25_Q8_MAX_BATCH_PROJECTIONS, true)
            * core::mem::size_of::<u32>()
            <= LFM25_Q8_COMMAND_BYTES
    );
};










