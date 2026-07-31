use std::{
    env, eprintln, fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    string::String,
    vec,
    vec::Vec,
};

use super::*;

type Gram<'a> = (&'a [u32], i16);

fn put_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_string(output: &mut Vec<u8>, value: &str) {
    put_u16(output, value.len() as u16);
    output.extend_from_slice(value.as_bytes());
}

fn put_varint(output: &mut Vec<u8>, mut value: u32) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        output.push(if value == 0 { byte } else { byte | 0x80 });
        if value == 0 {
            break;
        }
    }
}

fn put_grams(output: &mut Vec<u8>, grams: &[Gram<'_>]) {
    put_u32(output, grams.len() as u32);
    for &(ids, score) in grams {
        output.push(ids.len() as u8);
        for &id in ids {
            put_varint(output, id);
        }
        output.extend_from_slice(&score.to_le_bytes());
    }
}

fn model_blob(
    tokens: &[(&str, &str)],
    order: u8,
    logo: bool,
    ngrams: &[Gram<'_>],
    backoffs: &[Gram<'_>],
    unknown: f32,
    lexicon: &[(&str, &str)],
) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(b"G2P2");
    output.push(order);
    output.push(u8::from(logo));
    output.extend_from_slice(&[0, 0]);
    put_u32(&mut output, tokens.len() as u32);
    for &(graph, phoneme) in tokens {
        put_string(&mut output, graph);
        put_string(&mut output, phoneme);
    }
    put_grams(&mut output, ngrams);
    put_grams(&mut output, backoffs);
    output.extend_from_slice(&unknown.to_le_bytes());
    put_u32(&mut output, lexicon.len() as u32);
    for &(word, phoneme) in lexicon {
        put_string(&mut output, word);
        put_string(&mut output, phoneme);
    }
    output
}

fn toy_blob() -> Vec<u8> {
    model_blob(
        &[("", ""), ("a", "ɑ"), ("b", "b"), ("ab", "AB")],
        2,
        false,
        &[(&[0], -500), (&[1], -500), (&[2], -500), (&[3], -400)],
        &[],
        -5.0,
        &[("cat", "kat")],
    )
}

#[test]
fn pinned_asset_contract_is_stable() {
    assert_eq!(PINNED_ENGLISH_PATH, "models/kokoro/en.g2p");
    assert_eq!(PINNED_ENGLISH_BYTES, 6_691_149);
    assert_eq!(
        PINNED_ENGLISH_SHA256,
        [
            0x09, 0x13, 0x47, 0xd3, 0x75, 0xe4, 0x94, 0xb5, 0x54, 0x22, 0x02, 0x20, 0x1a, 0x24,
            0xa0, 0xf7, 0x24, 0x73, 0x8a, 0x3b, 0x18, 0xc3, 0x8d, 0x56, 0xa8, 0x70, 0x22, 0x97,
            0x0c, 0x70, 0xaa, 0x9c,
        ]
    );
}

#[test]
fn pinned_parser_authenticates_before_interpreting_the_image() {
    let mut bytes = vec![0; PINNED_ENGLISH_BYTES];
    bytes[..4].copy_from_slice(b"G2P2");
    // Although the fake image has the right byte count and magic, its digest
    // is rejected before its deliberately nonsensical body reaches parsing.
    assert!(matches!(Model::parse_pinned_english(&bytes), Err(ModelError::EnglishDigestMismatch)));
}

#[test]
fn tiny_model_parses_and_matches_upstream_beam_tiers() {
    let bytes = toy_blob();
    let model = Model::parse(&bytes).unwrap();
    assert_eq!(model.order(), 2);
    assert!(!model.is_logographic());
    assert_eq!(model.max_grapheme_chunk(), 2);
    assert_eq!(model.token_count(), 4);
    assert_eq!(model.ngram_count(), 4);
    assert_eq!(model.phonemize("ab").unwrap(), "AB");
    assert_eq!(model.phonemize("AB").unwrap(), "AB");
    assert_eq!(model.phonemize("CAT").unwrap(), "kat");
    assert_eq!(model.phonemize("z").unwrap(), "");
}

struct Primary;

impl PronunciationLookup for Primary {
    fn lookup(&self, word: &str) -> Option<&str> {
        (word == "hello").then_some("tɹuː")
    }
}

#[test]
fn primary_dictionary_precedes_model_and_frontend_normalizes_text() {
    let bytes = model_blob(
        &[("", "")],
        1,
        false,
        &[],
        &[],
        -5.0,
        &[("hello", "hɛloʊ"), ("forty", "fɔːti"), ("two", "tuː")],
    );
    let model = Model::parse(&bytes).unwrap();
    assert_eq!(model.phonemize_with("hello", Some(&Primary)).unwrap(), "tɹuː");
    let output = prepare_english_with(&model, "hello CPU 42!", Some(&Primary)).unwrap();
    assert_eq!(output.phonemes, "tɹuː siː piː juː fɔːti tuː!");
    assert_eq!(output.chunks, vec![0..output.token_ids.len()]);
    assert!(
        output
            .token_ids
            .iter()
            .all(|&id| id != KOKORO_BOUNDARY_TOKEN)
    );
}

#[test]
fn tokenizer_is_contiguous_and_classifies_english_surface_forms() {
    let text = "Can't CPU42, 1,024…🙂";
    let tokens: Vec<_> = EnglishTokens::new(text).collect();
    let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();
    assert_eq!(
        kinds,
        vec![
            EnglishTokenKind::Word,
            EnglishTokenKind::Whitespace,
            EnglishTokenKind::Word,
            EnglishTokenKind::Number,
            EnglishTokenKind::Punctuation,
            EnglishTokenKind::Whitespace,
            EnglishTokenKind::Number,
            EnglishTokenKind::Punctuation,
            EnglishTokenKind::Other,
        ]
    );
    let rebuilt: String = tokens.iter().map(EnglishToken::text).collect();
    assert_eq!(rebuilt, text);
    assert_eq!(tokens.first().unwrap().range.start, 0);
    assert_eq!(tokens.last().unwrap().range.end, text.len());
    assert!(
        tokens
            .windows(2)
            .all(|pair| pair[0].range.end == pair[1].range.start)
    );
}

#[test]
fn ipa_is_canonicalized_to_the_fixed_kokoro_vocabulary() {
    let encoded = canonicalize_ipa("d͡ʒ gɝxɬ l̩ aɪ̯ ɜ˞ o\u{200d}ʊ").unwrap();
    assert_eq!(encoded.phonemes, "ʤ ɡɚkl ᵊl aɪ ɚ oʊ");
    assert_eq!(encoded.token_ids[0], 82);
    assert_eq!(kokoro_token_id('ɡ'), Some(92));
    assert_eq!(kokoro_token_id('ᵊ'), Some(42));
    assert_eq!(canonicalize_ipa("🙂"), Err(IpaError::UnsupportedCharacter('🙂')));
}

fn assert_complete_partition(ranges: &[core::ops::Range<usize>], length: usize) {
    if length == 0 {
        assert!(ranges.is_empty());
        return;
    }
    assert_eq!(ranges.first().unwrap().start, 0);
    assert_eq!(ranges.last().unwrap().end, length);
    assert!(
        ranges.iter().all(|range| {
            range.start < range.end && range.end - range.start <= KOKORO_MODEL_MAX
        })
    );
    assert!(ranges.windows(2).all(|pair| pair[0].end == pair[1].start));
}

#[test]
fn chunker_prefers_model_sized_breaks_then_bounded_fallbacks() {
    assert_eq!(CHUNK_TARGET_MIN, 175);
    assert_eq!(CHUNK_TARGET_MAX, 250);
    assert_eq!(CHUNK_FALLBACK_MAX, 450);
    assert_eq!(KOKORO_MODEL_MAX, 510);

    let mut tokens = vec![43; 900];
    tokens[224] = 4;
    tokens[449] = 3;
    tokens[674] = 16;
    let ranges = chunk_ranges(&tokens).unwrap();
    assert_complete_partition(&ranges, tokens.len());
    assert_eq!(ranges[0], 0..225);
    assert!(
        ranges
            .iter()
            .take(ranges.len().saturating_sub(1))
            .all(|range| range.end - range.start >= CHUNK_TARGET_MIN)
    );

    let mut ordinary_fallback = vec![43; 700];
    ordinary_fallback[399] = 16;
    let ranges = chunk_ranges(&ordinary_fallback).unwrap();
    assert_complete_partition(&ranges, ordinary_fallback.len());
    assert_eq!(ranges[0], 0..400);
    assert!(ranges[0].len() <= CHUNK_FALLBACK_MAX);

    let pathological = vec![43; 1_200];
    let ranges = chunk_ranges(&pathological).unwrap();
    assert_eq!(ranges, vec![0..510, 510..1_020, 1_020..1_200]);
    assert_complete_partition(&ranges, pathological.len());
}

#[test]
fn fallback_growth_is_lexical_not_decoder_admission() {
    // With no natural break, the frontend may offer all 450 tokens as one
    // lexical chunk. It has neither speed nor duration logits, so this range
    // is not proof that the decoder's F<=2560 contract holds. The runtime must
    // reject and split/retry after phase-zero duration resolution when needed.
    let ranges = chunk_ranges(&vec![43; CHUNK_FALLBACK_MAX]).unwrap();
    assert_eq!(ranges, vec![0..CHUNK_FALLBACK_MAX]);
}

#[test]
fn parser_rejects_structural_corruption_without_panicking() {
    let valid = toy_blob();
    for cut in 0..valid.len() {
        let parsed = catch_unwind(AssertUnwindSafe(|| Model::parse(&valid[..cut])));
        assert!(parsed.is_ok(), "parser panicked at truncation {cut}");
        assert!(parsed.unwrap().is_err(), "prefix {cut} parsed as complete");
    }

    let mut bad = valid.clone();
    bad[0] = b'X';
    assert!(matches!(Model::parse(&bad), Err(ModelError::BadMagic)));
    let mut bad = valid.clone();
    bad[4] = 0;
    assert!(matches!(Model::parse(&bad), Err(ModelError::UnsupportedOrder)));
    let mut bad = valid.clone();
    bad[5] = 2;
    assert!(matches!(Model::parse(&bad), Err(ModelError::InvalidLogo)));
    let mut bad = valid.clone();
    bad[6] = 1;
    assert!(matches!(Model::parse(&bad), Err(ModelError::NonZeroPadding)));
    let mut bad = valid.clone();
    bad[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(Model::parse(&bad), Err(ModelError::CountLimit)));
    let mut bad = valid.clone();
    bad[18] = 0xff;
    assert!(matches!(Model::parse(&bad), Err(ModelError::InvalidUtf8)));
    let mut bad = valid.clone();
    bad.push(0);
    assert!(matches!(Model::parse(&bad), Err(ModelError::TrailingBytes)));
}

#[test]
fn parser_rejects_invalid_relations_and_duplicate_keys() {
    let eos = model_blob(&[("x", "")], 1, false, &[], &[], -1.0, &[]);
    assert!(matches!(Model::parse(&eos), Err(ModelError::InvalidEos)));
    let empty_graph = model_blob(&[("", ""), ("", "x")], 1, false, &[], &[], -1.0, &[]);
    assert!(matches!(Model::parse(&empty_graph), Err(ModelError::EmptyTokenGraph)));
    let long_graph = model_blob(&[("", ""), ("abcdefghi", "x")], 1, false, &[], &[], -1.0, &[]);
    assert!(matches!(Model::parse(&long_graph), Err(ModelError::TokenChunkTooLong)));
    let bad_id = model_blob(&[("", ""), ("a", "a")], 1, false, &[(&[2], -1)], &[], -1.0, &[]);
    assert!(matches!(Model::parse(&bad_id), Err(ModelError::InvalidTokenId)));
    let bad_length = model_blob(&[("", "")], 1, false, &[(&[], -1)], &[], -1.0, &[]);
    assert!(matches!(Model::parse(&bad_length), Err(ModelError::InvalidGramLength)));
    let duplicate_ngram =
        model_blob(&[("", ""), ("a", "a")], 1, false, &[(&[1], -1), (&[1], -2)], &[], -1.0, &[]);
    assert!(matches!(Model::parse(&duplicate_ngram), Err(ModelError::DuplicateNgram)));
    let duplicate_backoff =
        model_blob(&[("", "")], 2, false, &[], &[(&[0], -1), (&[0], -2)], -1.0, &[]);
    assert!(matches!(Model::parse(&duplicate_backoff), Err(ModelError::DuplicateBackoff)));
    let duplicate_lexicon =
        model_blob(&[("", "")], 1, false, &[], &[], -1.0, &[("word", "wɜːd"), ("word", "wəːd")]);
    assert!(matches!(Model::parse(&duplicate_lexicon), Err(ModelError::DuplicateLexicon)));
    let nan = model_blob(&[("", "")], 1, false, &[], &[], f32::NAN, &[]);
    assert!(matches!(Model::parse(&nan), Err(ModelError::InvalidUnknownScore)));
}

fn pinned_fixture() -> Option<Vec<u8>> {
    let path = env::var_os("TRUEOS_G2P_MODEL").map(PathBuf::from)?;
    Some(fs::read(path).expect("TRUEOS_G2P_MODEL must point to a readable model"))
}

#[test]
fn released_english_model_matches_upstream_reference() {
    let Some(bytes) = pinned_fixture() else {
        eprintln!("skipping real-model parity; set TRUEOS_G2P_MODEL");
        return;
    };
    let model = Model::parse_pinned_english(&bytes).unwrap();
    assert_eq!(model.order(), 6);
    assert_eq!(model.token_count(), 3_600);
    assert_eq!(model.ngram_count(), 315_032);
    assert_eq!(model.backoff_count(), 211_749);
    assert_eq!(model.lexicon_count(), 92_406);
    let memory = model.memory_usage();
    assert_eq!(memory.borrowed_model_bytes, PINNED_ENGLISH_BYTES);
    assert_eq!(memory.token_index_bytes, 115_200);
    assert_eq!(memory.graph_index_bytes, 86_376);
    assert_eq!(memory.ngram_index_bytes, 5_040_512);
    assert_eq!(memory.backoff_index_bytes, 3_387_984);
    assert_eq!(memory.lexicon_index_bytes, 2_956_992);
    assert_eq!(memory.allocated_index_bytes, 11_587_064);
    assert_eq!(memory.contiguous_allocations, 5);

    for (word, expected) in [
        ("hello", "hɛloʊ"),
        ("world", "wɜːld"),
        ("cat", "kat"),
        ("dog", "dɒɡ"),
        ("through", "θɹuː"),
        ("queue", "kjuː"),
        ("speech", "spiːt͡ʃ"),
        ("kernel", "kɜːnəl"),
        ("processor", "pɹəʊsɛsəɹ"),
        ("asynchronous", "eɪsɪŋkɹənəs"),
        ("extraordinary", "ɪkstɹɔːrdɪnəɹi"),
        ("Worcestershire", "wʊstəʃə"),
        ("colonel", "kɜːnl̩"),
        ("yacht", "jɒt"),
        ("knight", "naɪ̯t"),
        ("read", "ɹiːd"),
        ("lead", "lɛd"),
        ("record", "ɹɛkɔːd"),
        ("resume", "ɹɪzjuːm"),
        ("TrueOS", "tɹuːəʊz"),
        ("kokoro", "koʊkəɹoʊ"),
        ("kokorization", "koʊkəɹaɪzeɪʃən"),
        ("hypervectorized", "haɪpɚvɛktəɹaɪzd"),
        ("flibbertigibbet", "flɪbətidʒɪbɪt"),
        ("xyzzy", "zɪzi"),
        ("NVIDIA", "nvɪdiə"),
        ("CPU", "siːpiːjuː"),
        ("NASA", "næsə"),
        ("AVX", "eɪvks"),
    ] {
        assert_eq!(model.phonemize(word).unwrap(), expected, "word={word}");
        let encoded = canonicalize_ipa(expected).unwrap_or_else(|error| {
            panic!("word={word} produced IPA outside Kokoro's vocabulary: {error:?}")
        });
        assert!(!encoded.token_ids.is_empty(), "word={word}");
    }

    let sentence = prepare_english(&model, "Hello world, Kokoro speaks through TrueOS 42!")
        .expect("representative English must reach Kokoro tokens");
    assert!(!sentence.token_ids.is_empty());
    assert_complete_partition(&sentence.chunks, sentence.token_ids.len());
    assert!(
        sentence
            .chunks
            .iter()
            .all(|range| range.len() <= KOKORO_MODEL_MAX)
    );
    assert!(
        sentence
            .token_ids
            .iter()
            .all(|&token| token != KOKORO_BOUNDARY_TOKEN)
    );
}
