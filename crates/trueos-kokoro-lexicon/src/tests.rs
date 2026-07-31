use std::{env, fs, panic::catch_unwind, path::PathBuf, vec, vec::Vec};

use super::*;
use trueos_kokoro_g2p::{Model, canonicalize_ipa, prepare_english_with};

fn put_u16(output: &mut [u8], offset: usize, value: u16) {
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn seal(output: &mut [u8]) {
    output[DIGEST_OFFSET..DIGEST_END].fill(0);
    let digest: [u8; 32] = Sha256::digest(&*output).into();
    output[DIGEST_OFFSET..DIGEST_END].copy_from_slice(&digest);
}

fn artifact(entries: &[(&str, &str)], variants: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut pool = Vec::new();
    let mut entry_records = vec![0u8; entries.len() * ENTRY_RECORD_BYTES];
    for (index, &(word, pronunciation)) in entries.iter().enumerate() {
        let record = index * ENTRY_RECORD_BYTES;
        let word_offset = pool.len();
        pool.extend_from_slice(word.as_bytes());
        let pronunciation_offset = pool.len();
        pool.extend_from_slice(pronunciation.as_bytes());
        put_u32(&mut entry_records, record, word_offset as u32);
        put_u16(&mut entry_records, record + 4, word.len() as u16);
        put_u16(&mut entry_records, record + 6, pronunciation.len() as u16);
        put_u32(&mut entry_records, record + 8, pronunciation_offset as u32);
    }

    let mut variant_records = vec![0u8; variants.len() * VARIANT_RECORD_BYTES];
    for (index, &(word, tag, pronunciation)) in variants.iter().enumerate() {
        let entry_index = entries
            .iter()
            .position(|entry| entry.0 == word)
            .expect("variant word must exist");
        let record = index * VARIANT_RECORD_BYTES;
        let tag_offset = pool.len();
        pool.extend_from_slice(tag.as_bytes());
        let pronunciation_offset = pool.len();
        pool.extend_from_slice(pronunciation.as_bytes());
        put_u32(&mut variant_records, record, entry_index as u32);
        put_u32(&mut variant_records, record + 4, tag_offset as u32);
        put_u32(&mut variant_records, record + 8, pronunciation_offset as u32);
        put_u16(&mut variant_records, record + 12, tag.len() as u16);
        put_u16(&mut variant_records, record + 14, pronunciation.len() as u16);
    }

    let variants_offset = HEADER_BYTES + entry_records.len();
    let strings_offset = variants_offset + variant_records.len();
    let file_bytes = strings_offset + pool.len();
    let mut output = vec![0u8; HEADER_BYTES];
    output[..MAGIC.len()].copy_from_slice(MAGIC);
    put_u16(&mut output, 8, VERSION);
    put_u16(&mut output, 10, HEADER_BYTES as u16);
    put_u32(
        &mut output,
        12,
        if variants.is_empty() {
            0
        } else {
            FLAG_POS_VARIANTS
        },
    );
    put_u32(&mut output, 16, entries.len() as u32);
    put_u32(&mut output, 20, variants.len() as u32);
    put_u16(&mut output, 24, ENTRY_RECORD_BYTES as u16);
    put_u16(&mut output, 26, VARIANT_RECORD_BYTES as u16);
    put_u64(&mut output, 32, HEADER_BYTES as u64);
    put_u64(&mut output, 40, variants_offset as u64);
    put_u64(&mut output, 48, strings_offset as u64);
    put_u64(&mut output, 56, file_bytes as u64);
    put_u64(&mut output, 64, pool.len() as u64);
    output[SILVER_DIGEST_OFFSET..SILVER_DIGEST_OFFSET + 32]
        .copy_from_slice(&MISAKI_US_SILVER_SHA256);
    output[GOLD_DIGEST_OFFSET..GOLD_DIGEST_OFFSET + 32].copy_from_slice(&MISAKI_US_GOLD_SHA256);
    output[LICENSE_DIGEST_OFFSET..LICENSE_DIGEST_OFFSET + 32]
        .copy_from_slice(&MISAKI_LICENSE_SHA256);
    output[SOURCE_COMMIT_OFFSET..SOURCE_COMMIT_OFFSET + 20].copy_from_slice(&MISAKI_SOURCE_COMMIT);
    output.extend_from_slice(&entry_records);
    output.extend_from_slice(&variant_records);
    output.extend_from_slice(&pool);
    seal(&mut output);
    output
}

#[test]
fn zero_copy_lookup_and_g2p_override_hook_are_exact() {
    let bytes = artifact(
        &[
            ("alpha", "ˈælfə"),
            ("hello", "həlˈo\u{200d}ʊ"),
            ("world", "wˈɜːld"),
        ],
        &[("hello", "NOUN", "hˈɛlo\u{200d}ʊ")],
    );
    let lexicon = Lexicon::parse(&bytes).unwrap();
    assert_eq!(lexicon.entry_count(), 3);
    assert_eq!(lexicon.variant_count(), 1);
    assert_eq!(lexicon.resident_bytes(), bytes.len());
    assert_eq!(lexicon.get("hello"), Some("həlˈo\u{200d}ʊ"));
    assert_eq!(lexicon.get("missing"), None);
    assert_eq!(lexicon.get_variant("hello", "NOUN"), Some("hˈɛlo\u{200d}ʊ"));
    assert_eq!(lexicon.get_variant("hello", "VERB"), None);
    assert_eq!(lexicon.entry_at(usize::MAX), None);
    assert_eq!(lexicon.variant_at(usize::MAX), None);
    let source: &dyn PronunciationLookup = &lexicon;
    assert_eq!(source.lookup("world"), Some("wˈɜːld"));
    assert_eq!(lexicon.provenance().source_commit, MISAKI_SOURCE_COMMIT);
}

#[test]
fn parser_rejects_header_size_hash_and_canonical_layout_corruption() {
    let valid = artifact(&[("alpha", "a"), ("beta", "b")], &[]);

    let mut bad = valid.clone();
    bad[0] = b'X';
    assert!(matches!(Lexicon::parse(&bad), Err(LexiconError::BadMagic)));

    let mut bad = valid.clone();
    bad[HEADER_RESERVED_OFFSET] = 1;
    assert!(matches!(Lexicon::parse(&bad), Err(LexiconError::NonZeroReserved)));

    let mut bad = valid.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert!(matches!(Lexicon::parse(&bad), Err(LexiconError::ArtifactDigestMismatch)));

    let mut bad = valid.clone();
    bad.push(0);
    assert!(matches!(Lexicon::parse(&bad), Err(LexiconError::SizeMismatch)));

    let mut bad = valid.clone();
    put_u32(&mut bad, HEADER_BYTES, 1);
    seal(&mut bad);
    assert!(matches!(Lexicon::parse(&bad), Err(LexiconError::NonCanonicalStringPool)));
}

#[test]
fn parser_rejects_unsorted_words_variants_and_invalid_utf8() {
    let unsorted = artifact(&[("beta", "b"), ("alpha", "a")], &[]);
    assert!(matches!(Lexicon::parse(&unsorted), Err(LexiconError::UnsortedOrDuplicateWord)));

    let variants = artifact(&[("alpha", "a")], &[("alpha", "VERB", "v"), ("alpha", "NOUN", "n")]);
    assert!(matches!(Lexicon::parse(&variants), Err(LexiconError::UnsortedOrDuplicateVariant)));

    let mut invalid_utf8 = artifact(&[("alpha", "a")], &[]);
    let strings_offset = read_offset(&invalid_utf8, 48).unwrap();
    invalid_utf8[strings_offset] = 0xff;
    seal(&mut invalid_utf8);
    assert!(matches!(Lexicon::parse(&invalid_utf8), Err(LexiconError::InvalidUtf8)));
}

#[test]
fn every_truncated_prefix_is_rejected_without_panicking() {
    let valid = artifact(&[("alpha", "a"), ("beta", "b")], &[("beta", "NOUN", "b")]);
    for cut in 0..valid.len() {
        let result = catch_unwind(|| Lexicon::parse(&valid[..cut]));
        assert!(result.is_ok(), "parser panicked at prefix {cut}");
        assert!(result.unwrap().is_err(), "prefix {cut} parsed as complete");
    }
}

#[test]
fn pinned_contract_constants_are_explicit() {
    assert_eq!(PINNED_US_PATH, "models/kokoro/misaki-us.klex");
    assert_eq!(PINNED_US_ENTRIES, 389_904);
    assert_eq!(PINNED_US_VARIANTS, 41);
    assert_eq!(PINNED_US_BYTES, 15_844_468);
    assert_eq!(
        PINNED_US_SHA256,
        [
            0xdf, 0x5e, 0x2a, 0x52, 0x11, 0x0c, 0x70, 0xc3, 0xb0, 0x4a, 0x72, 0x2b, 0xb2, 0x4f,
            0xc4, 0xfa, 0x59, 0xf2, 0x45, 0x7d, 0xcb, 0x7b, 0x4b, 0x3a, 0x5c, 0x11, 0x0f, 0xf6,
            0x0a, 0x4c, 0xa0, 0x3b,
        ]
    );
}

#[test]
fn released_misaki_artifact_is_sealed_and_lookup_complete() {
    let Some(path) = env::var_os("TRUEOS_KLEX_MODEL").map(PathBuf::from) else {
        std::eprintln!("skipping real KLEX test; set TRUEOS_KLEX_MODEL");
        return;
    };
    let bytes = fs::read(path).expect("TRUEOS_KLEX_MODEL must point to a readable artifact");
    let lexicon = Lexicon::parse_pinned_us(&bytes).unwrap();
    assert_eq!(lexicon.entry_count(), PINNED_US_ENTRIES);
    assert_eq!(lexicon.variant_count(), PINNED_US_VARIANTS);
    assert_eq!(lexicon.resident_bytes(), PINNED_US_BYTES);
    assert_eq!(lexicon.get("aalii"), Some("ˈɑːlɪˌa\u{200d}ɪ"));
    assert_eq!(lexicon.get("hello"), Some("həlˈo\u{200d}ʊ"));
    assert_eq!(lexicon.get("world"), Some("wˈɜːld"));
    assert_eq!(lexicon.get("compiler"), Some("kəmpˈa\u{200d}ɪlɚ"));
    assert_eq!(lexicon.get("prefetch"), Some("pɹifˈɛʧ"));
    assert_eq!(lexicon.get_variant("prefetch", "NOUN"), Some("pɹˈifˌɛʧ"));
    assert_eq!(lexicon.get("definitely-not-a-misaki-word"), None);
    for index in 0..lexicon.entry_count() {
        let (word, pronunciation) = lexicon.entry_at(index).unwrap();
        canonicalize_ipa(pronunciation).unwrap_or_else(|error| {
            panic!("unsupported default pronunciation for {word:?}: {error:?}")
        });
    }
    for index in 0..lexicon.variant_count() {
        let (word, tag, pronunciation) = lexicon.variant_at(index).unwrap();
        canonicalize_ipa(pronunciation).unwrap_or_else(|error| {
            panic!("unsupported variant pronunciation for {word:?}/{tag}: {error:?}")
        });
    }
}

#[test]
fn released_overlay_drives_the_complete_kokoro_frontend() {
    let (Some(lexicon_path), Some(g2p_path)) = (
        env::var_os("TRUEOS_KLEX_MODEL").map(PathBuf::from),
        env::var_os("TRUEOS_G2P_MODEL").map(PathBuf::from),
    ) else {
        std::eprintln!(
            "skipping real frontend integration; set TRUEOS_KLEX_MODEL and TRUEOS_G2P_MODEL"
        );
        return;
    };
    let lexicon_bytes = fs::read(lexicon_path).unwrap();
    let g2p_bytes = fs::read(g2p_path).unwrap();
    let lexicon = Lexicon::parse_pinned_us(&lexicon_bytes).unwrap();
    let model = Model::parse_pinned_english(&g2p_bytes).unwrap();
    assert_eq!(model.phonemize("hello").unwrap(), "hɛloʊ");
    let output = prepare_english_with(
        &model,
        "Hello world, compiler!",
        Some(&lexicon as &dyn PronunciationLookup),
    )
    .unwrap();
    assert_eq!(output.phonemes, "həlˈoʊ wˈɜːld, kəmpˈaɪlɚ!");
    assert_eq!(output.chunks.len(), 1);
    assert_eq!(output.chunks[0], 0..output.token_ids.len());
    assert!(output.token_ids.iter().all(|&token| token != 0));
}
