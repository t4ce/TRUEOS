#![no_std]

#[cfg(test)]
mod tests {
    use trueos_fpga_abi::lfm25;
    use trueos_fpga_abi::lfm25_decode_feed as feed;

    fn request(mode: feed::FeedMode, layer: Option<u8>) -> feed::FeedRequest {
        feed::FeedRequest {
            mode,
            layer,
            position: 0,
            token: if mode == feed::FeedMode::EmbeddingQ8Row {
                Some(1)
            } else {
                None
            },
            session_epoch: 17,
        }
    }

    fn retire_one(validator: &mut feed::FeedSequenceValidator) {
        for lane in 0..validator.request().mode.shape().lanes {
            validator.stage(validator.expected_stage(lane).unwrap()).unwrap();
        }
        let record = validator.expected_commit().unwrap();
        let bytes = record.encode_le();
        validator
            .commit(feed::FeedCommitRecord::decode_le(&bytes).unwrap())
            .unwrap();
    }

    #[test]
    fn exact_capability_and_fixed_shapes() {
        assert!(feed::capability_is_exact(feed::REQUIRED_CAPABILITY));
        let mut widened = feed::REQUIRED_CAPABILITY;
        widened.capability_bits |= 1 << 31;
        assert!(!feed::capability_is_exact(widened));

        assert_eq!(feed::ALL_FEED_MODES.len(), 16);
        assert_eq!(feed::FeedMode::EmbeddingQ8Row.shape().commits(), 32);
        assert_eq!(feed::FeedMode::FfnGateUpRows.shape().items, 4_608);
        assert_eq!(feed::FeedMode::FfnGateUpRows.shape().lanes, 2);
        assert_eq!(feed::FeedMode::FfnDownRows.shape().blocks_per_item, 144);
        assert_eq!(feed::FeedMode::TiedLmHeadRows.shape().items, 65_536);
        assert_eq!(feed::FeedMode::TiedLmHeadRows.shape().commits(), 2_097_152);
    }

    #[test]
    fn v1_offsets_remain_compatible_and_commit_is_published_last() {
        assert_eq!(trueos_fpga_abi::LFM25_STREAM_CAPABILITY_MAGIC, 0x3252_4754);
        assert_eq!(feed::StageBank::Bank0.bar2_offset(), 0x0000);
        assert_eq!(feed::StageBank::Bank1.bar2_offset(), 0x4000);
        assert_eq!(feed::StageBank::Bank2.bar2_offset(), 0x8000);

        let mut validator = feed::FeedSequenceValidator::begin(
            feed::REQUIRED_CAPABILITY,
            request(feed::FeedMode::FfnGateUpRows, Some(0)),
        )
        .unwrap();
        assert_eq!(validator.expected_commit(), Err(feed::FeedError::MissingStage));
        for lane in 0..2 {
            validator.stage(validator.expected_stage(lane).unwrap()).unwrap();
        }
        let record = validator.expected_commit().unwrap();
        let bytes = record.encode_le();
        assert_eq!(&bytes[60..], &feed::FEED_COMMIT_MAGIC.to_le_bytes());
        assert_eq!(feed::FeedCommitRecord::decode_le(&bytes), Ok(record));
    }

    #[test]
    fn layer_position_and_sequence_errors_fail_closed_and_poison() {
        assert_eq!(
            request(feed::FeedMode::AttentionQueryRows, Some(0)).validate(),
            Err(feed::FeedError::InvalidLayer)
        );
        let mut later = request(feed::FeedMode::AttentionFirstTokenCore, Some(2));
        later.position = 1;
        assert_eq!(later.validate(), Err(feed::FeedError::InvalidPosition));

        let mut validator = feed::FeedSequenceValidator::begin(
            feed::REQUIRED_CAPABILITY,
            request(feed::FeedMode::EmbeddingQ8Row, None),
        )
        .unwrap();
        let mut stale = validator.expected_stage(0).unwrap();
        stale.generation += 1;
        assert_eq!(validator.stage(stale), Err(feed::FeedError::UnexpectedStage));
        assert!(validator.is_poisoned());
        assert_eq!(validator.expected_commit(), Err(feed::FeedError::Poisoned));
    }

    #[test]
    fn control_only_attention_and_full_tied_head_reach_exact_completion() {
        let mut core = feed::FeedSequenceValidator::begin(
            feed::REQUIRED_CAPABILITY,
            request(feed::FeedMode::AttentionFirstTokenCore, Some(2)),
        )
        .unwrap();
        let record = core.expected_commit().unwrap();
        assert_eq!(record.lane_mask, 0);
        assert_eq!(record.stage_slot, feed::FEED_NO_STAGE_SLOT);
        core.commit(record).unwrap();
        assert!(core.is_complete());

        let mut head = feed::FeedSequenceValidator::begin(
            feed::REQUIRED_CAPABILITY,
            request(feed::FeedMode::TiedLmHeadRows, None),
        )
        .unwrap();
        while !head.is_complete() {
            retire_one(&mut head);
        }
        assert_eq!(head.committed_units(), lfm25::MODEL_VOCABULARY_SIZE * 32);
        assert_eq!(head.expected_commit(), Err(feed::FeedError::Complete));
    }
}
