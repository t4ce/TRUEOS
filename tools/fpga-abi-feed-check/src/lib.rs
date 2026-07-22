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
        while !validator.staging_complete() {
            validator
                .stage(validator.expected_stage().unwrap())
                .unwrap();
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
        assert_eq!(
            feed::FeedCapability::from_bar0_words(feed::REQUIRED_CAPABILITY.bar0_words()),
            feed::REQUIRED_CAPABILITY
        );
        assert_eq!(feed::BAR0_FEED_CAPABILITY_MAGIC_OFFSET, 0x280);
        assert_eq!(feed::BAR0_FEED_CAPABILITY_REQUIRED_BYTES, 0x294);
        assert_eq!(feed::BAR0_FEED_STATE_OFFSET, 0x294);
        assert_eq!(feed::BAR0_FEED_RETIRED_MODE_LAYER_OFFSET, 0x298);
        assert_eq!(feed::BAR0_FEED_RETIRED_SESSION_EPOCH_OFFSET, 0x29c);
        assert_eq!(feed::BAR0_FEED_RETIRED_SEQUENCE_OFFSET, 0x2a0);
        assert_eq!(feed::BAR0_FEED_RETIRED_ITEM_OFFSET, 0x2a4);
        assert_eq!(feed::BAR0_FEED_ERROR_CODE_OFFSET, 0x2a8);
        assert_eq!(feed::BAR0_FEED_COMPLETION_COUNT_OFFSET, 0x2ac);
        assert_eq!(feed::BAR0_FEED_CONTROL_OFFSET, 0x2b0);
        assert_eq!(feed::BAR0_FEED_REQUIRED_BYTES, 0x2b4);
        assert_eq!(
            feed::BAR0_FEED_SHARED_IRQ_ACK_OFFSET,
            trueos_fpga_abi::BAR0_CALL_IRQ_ACK_OFFSET
        );
        assert_eq!(
            feed::BAR0_FEED_SHARED_IRQ_STATE_OFFSET,
            trueos_fpga_abi::BAR0_CALL_IRQ_STATE_OFFSET
        );
        assert_eq!(feed::FEED_SHARED_IRQ_PENDING_BIT, 1);
        assert_eq!(feed::FEED_ERROR_NONE, 0);
        assert_eq!(feed::FEED_ERROR_FRONTEND_POISON, 0xbad4_0001);
        assert!(feed::BAR0_FEED_REQUIRED_BYTES <= feed::TGA_BAR0_APERTURE_BYTES);
        let mut widened = feed::REQUIRED_CAPABILITY;
        widened.capability_bits |= 1 << 31;
        assert!(!feed::capability_is_exact(widened));

        assert_eq!(feed::ALL_FEED_MODES.len(), 16);
        assert_eq!(feed::FeedMode::EmbeddingQ8Row.shape().commits(), 1);
        assert_eq!(feed::FeedMode::OperatorRmsNormWeights.shape().commits(), 1);
        assert_eq!(feed::FeedMode::AttentionFirstTokenCore.shape().commits(), 1);
        assert_eq!(feed::FeedMode::FfnGateUpRows.shape().items, 4_608);
        assert_eq!(feed::FeedMode::FfnGateUpRows.shape().commits(), 4_608);
        assert_eq!(feed::FeedMode::FfnGateUpRows.shape().lanes, 2);
        assert_eq!(feed::FeedMode::FfnDownRows.shape().stages_per_item, 144);
        assert_eq!(feed::FeedMode::FfnDownRows.shape().commits(), 1_024);
        assert_eq!(feed::FeedMode::TiedLmHeadRows.shape().items, 65_536);
        assert_eq!(feed::FeedMode::TiedLmHeadRows.shape().stages_per_item, 32);
        assert_eq!(feed::FeedMode::TiedLmHeadRows.shape().commits(), 65_536);
    }

    #[test]
    fn completion_plane_decodes_and_matches_exact_commit_identity() {
        let mut validator = feed::FeedSequenceValidator::begin(
            feed::REQUIRED_CAPABILITY,
            request(feed::FeedMode::FfnGateUpRows, Some(0)),
        )
        .unwrap();
        while !validator.staging_complete() {
            let staged = validator.expected_stage().unwrap();
            validator.stage(staged).unwrap();
        }
        let record = validator.expected_commit().unwrap();
        let words = [
            feed::FeedState::Complete as u32,
            feed::retired_mode_layer_word(feed::FeedMode::FfnGateUpRows, Some(0)),
            record.session_epoch,
            record.sequence,
            record.item,
            feed::FEED_ERROR_NONE,
            10,
        ];
        let status = feed::FeedRetirementStatus::from_bar0_words(words).unwrap();
        assert!(status.identity_matches(record));
        assert!(status.completion_matches(record, 9));

        let mut bad = words;
        bad[4] += 1;
        assert!(!feed::FeedRetirementStatus::from_bar0_words(bad)
            .unwrap()
            .identity_matches(record));
        bad = words;
        bad[0] = 7;
        assert_eq!(
            feed::FeedRetirementStatus::from_bar0_words(bad),
            Err(feed::FeedError::InvalidRetirement)
        );
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
        while !validator.staging_complete() {
            validator
                .stage(validator.expected_stage().unwrap())
                .unwrap();
        }
        let record = validator.expected_commit().unwrap();
        assert_eq!(record.stages_per_lane, 32);
        assert_eq!(record.last_stage_slot, 31);
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
        let mut stale = validator.expected_stage().unwrap();
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
        assert_eq!(record.last_stage_slot, feed::FEED_NO_STAGE_SLOT);
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
        assert_eq!(head.committed_units(), lfm25::MODEL_VOCABULARY_SIZE);
        assert_eq!(head.expected_commit(), Err(feed::FeedError::Complete));
    }
}
