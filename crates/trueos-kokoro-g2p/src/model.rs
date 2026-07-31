use alloc::vec::Vec;
use core::{cmp::Ordering, mem::size_of, str};

use sha2::{Digest, Sha256};

const MAGIC: &[u8; 4] = b"G2P2";
const QSCALE: f32 = 1000.0;

pub const MAX_NGRAM_ORDER: usize = 6;
const MAX_MODEL_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOKENS: usize = 65_536;
const MAX_NGRAMS: usize = 1_000_000;
const MAX_BACKOFFS: usize = 1_000_000;
const MAX_LEXICON_ENTRIES: usize = 1_000_000;
const MAX_TOKEN_GRAPH_BYTES: usize = 32;
const MAX_TOKEN_PHONEME_BYTES: usize = 256;
const MAX_LEXICON_WORD_BYTES: usize = 256;
const MAX_LEXICON_PHONEME_BYTES: usize = 512;
const MAX_GRAPH_CHUNK: usize = 8;
const MAX_CANDIDATES_PER_GRAPH: usize = 64;

pub const PINNED_ENGLISH_BYTES: usize = 6_691_149;
/// Runtime path used by the TRUEOS model-residency service.
pub const PINNED_ENGLISH_PATH: &str = "models/kokoro/en.g2p";
pub const PINNED_ENGLISH_SHA256: [u8; 32] = [
    0x09, 0x13, 0x47, 0xd3, 0x75, 0xe4, 0x94, 0xb5, 0x54, 0x22, 0x02, 0x20, 0x1a, 0x24, 0xa0, 0xf7,
    0x24, 0x73, 0x8a, 0x3b, 0x18, 0xc3, 0x8d, 0x56, 0xa8, 0x70, 0x22, 0x97, 0x0c, 0x70, 0xaa, 0x9c,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelError {
    ModelTooLarge,
    BadMagic,
    UnsupportedOrder,
    InvalidLogo,
    NonZeroPadding,
    CountLimit,
    Truncated,
    StringTooLong,
    InvalidUtf8,
    InvalidEos,
    EmptyTokenGraph,
    TokenChunkTooLong,
    DuplicateToken,
    InvalidGramLength,
    InvalidTokenId,
    MalformedVarint,
    NonCanonicalVarint,
    DuplicateNgram,
    DuplicateBackoff,
    InvalidUnknownScore,
    EmptyLexiconEntry,
    DuplicateLexicon,
    TooManyCandidates,
    TrailingBytes,
    Allocation,
    EnglishSizeMismatch,
    EnglishProfileMismatch,
    EnglishDigestMismatch,
}

#[derive(Clone, Copy, Debug)]
pub struct JointToken<'a> {
    grapheme: &'a str,
    phoneme: &'a str,
}

impl<'a> JointToken<'a> {
    pub const fn grapheme(self) -> &'a str {
        self.grapheme
    }

    pub const fn phoneme(self) -> &'a str {
        self.phoneme
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryUsage {
    pub borrowed_model_bytes: usize,
    pub token_index_bytes: usize,
    pub graph_index_bytes: usize,
    pub ngram_index_bytes: usize,
    pub backoff_index_bytes: usize,
    pub lexicon_index_bytes: usize,
    pub allocated_index_bytes: usize,
    pub contiguous_allocations: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct GraphCandidate<'a> {
    pub(crate) graph: &'a str,
    pub(crate) token_id: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GramEntry {
    // The parser caps the token table at 65,536 entries, so every valid ID
    // fits losslessly in 16 bits. This keeps a six-gram record at 16 bytes
    // instead of 28 bytes with upstream's u32 IDs.
    ids: [u16; MAX_NGRAM_ORDER],
    score_q: i16,
    len: u8,
}

impl GramEntry {
    fn compare_key(&self, ids: &[u16; MAX_NGRAM_ORDER], len: u8) -> Ordering {
        self.ids.cmp(ids).then_with(|| self.len.cmp(&len))
    }

    fn same_key(&self, other: &Self) -> bool {
        self.len == other.len && self.ids == other.ids
    }

    fn score(self) -> f32 {
        f32::from(self.score_q) / QSCALE
    }
}

#[derive(Clone, Copy)]
struct LexiconEntry<'a> {
    word: &'a str,
    phoneme: &'a str,
}

/// Borrowed strings plus five contiguous indexes for a validated G2P2 model.
pub struct Model<'a> {
    bytes: &'a [u8],
    tokens: Vec<JointToken<'a>>,
    graphs: Vec<GraphCandidate<'a>>,
    ngrams: Vec<GramEntry>,
    backoffs: Vec<GramEntry>,
    lexicon: Vec<LexiconEntry<'a>>,
    order: u8,
    logo: bool,
    max_chunk: usize,
    unk: f32,
}

impl<'a> Model<'a> {
    /// Parse any bounded model in the released G2P2 v0.2 binary format.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ModelError> {
        if bytes.len() > MAX_MODEL_BYTES {
            return Err(ModelError::ModelTooLarge);
        }
        let mut cursor = Cursor::new(bytes);
        if cursor.take(MAGIC.len())? != MAGIC {
            return Err(ModelError::BadMagic);
        }
        let order = cursor.u8()?;
        if order == 0 || usize::from(order) > MAX_NGRAM_ORDER {
            return Err(ModelError::UnsupportedOrder);
        }
        let logo = match cursor.u8()? {
            0 => false,
            1 => true,
            _ => return Err(ModelError::InvalidLogo),
        };
        if cursor.take(2)? != [0, 0] {
            return Err(ModelError::NonZeroPadding);
        }

        let token_count = cursor.count(MAX_TOKENS)?;
        if token_count == 0 {
            return Err(ModelError::InvalidEos);
        }
        cursor.require_records(token_count, 4)?;
        let mut tokens = reserved_vec(token_count)?;
        let mut max_chunk = 1usize;
        for token_id in 0..token_count {
            let graph = cursor.string(MAX_TOKEN_GRAPH_BYTES)?;
            let phoneme = cursor.string(MAX_TOKEN_PHONEME_BYTES)?;
            if token_id == 0 {
                if !graph.is_empty() || !phoneme.is_empty() {
                    return Err(ModelError::InvalidEos);
                }
            } else if graph.is_empty() {
                return Err(ModelError::EmptyTokenGraph);
            } else {
                let chunks = grapheme_count(graph);
                if chunks == 0 || chunks > MAX_GRAPH_CHUNK {
                    return Err(ModelError::TokenChunkTooLong);
                }
                max_chunk = max_chunk.max(chunks);
            }
            tokens.push(JointToken {
                grapheme: graph,
                phoneme,
            });
        }

        let mut ngrams = parse_grams(&mut cursor, token_count, order, GramKind::Ngram)?;
        let mut backoffs = parse_grams(&mut cursor, token_count, order, GramKind::Backoff)?;
        let unk = cursor.f32()?;
        if !unk.is_finite() {
            return Err(ModelError::InvalidUnknownScore);
        }

        let lexicon_count = cursor.count(MAX_LEXICON_ENTRIES)?;
        cursor.require_records(lexicon_count, 4)?;
        let mut lexicon = reserved_vec(lexicon_count)?;
        for _ in 0..lexicon_count {
            let word = cursor.string(MAX_LEXICON_WORD_BYTES)?;
            let phoneme = cursor.string(MAX_LEXICON_PHONEME_BYTES)?;
            if word.is_empty() || phoneme.is_empty() {
                return Err(ModelError::EmptyLexiconEntry);
            }
            lexicon.push(LexiconEntry { word, phoneme });
        }
        if !cursor.is_finished() {
            return Err(ModelError::TrailingBytes);
        }

        sort_and_check_grams(&mut ngrams, ModelError::DuplicateNgram)?;
        sort_and_check_grams(&mut backoffs, ModelError::DuplicateBackoff)?;
        lexicon.sort_unstable_by(|left, right| left.word.cmp(right.word));
        if lexicon.windows(2).any(|pair| pair[0].word == pair[1].word) {
            return Err(ModelError::DuplicateLexicon);
        }

        let mut graphs = reserved_vec(token_count.saturating_sub(1))?;
        for (token_id, token) in tokens.iter().enumerate().skip(1) {
            graphs.push(GraphCandidate {
                graph: token.grapheme,
                token_id: u16::try_from(token_id).map_err(|_| ModelError::InvalidTokenId)?,
            });
        }
        graphs.sort_unstable_by(|left, right| {
            left.graph
                .cmp(right.graph)
                .then_with(|| left.token_id.cmp(&right.token_id))
        });
        validate_graph_groups(&graphs, &tokens)?;

        Ok(Self {
            bytes,
            tokens,
            graphs,
            ngrams,
            backoffs,
            lexicon,
            order,
            logo,
            max_chunk,
            unk,
        })
    }

    /// Parse and provenance-seal the released 6,691,149-byte English model.
    pub fn parse_pinned_english(bytes: &'a [u8]) -> Result<Self, ModelError> {
        if bytes.len() != PINNED_ENGLISH_BYTES {
            return Err(ModelError::EnglishSizeMismatch);
        }
        // Authenticate the resident image before interpreting attacker-controlled
        // counts or allocating any indexes.  Apart from making the provenance
        // boundary explicit, this keeps a same-sized corrupt image from driving
        // the general parser's (bounded but deliberately generous) allocations.
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        if digest != PINNED_ENGLISH_SHA256 {
            return Err(ModelError::EnglishDigestMismatch);
        }
        let model = Self::parse(bytes)?;
        if model.order != 6 || model.logo {
            return Err(ModelError::EnglishProfileMismatch);
        }
        Ok(model)
    }

    pub const fn order(&self) -> u8 {
        self.order
    }

    pub const fn is_logographic(&self) -> bool {
        self.logo
    }

    pub const fn max_grapheme_chunk(&self) -> usize {
        self.max_chunk
    }

    pub fn tokens(&self) -> &[JointToken<'a>] {
        &self.tokens
    }

    pub fn token_count(&self) -> usize {
        self.tokens.len()
    }

    pub fn ngram_count(&self) -> usize {
        self.ngrams.len()
    }

    pub fn backoff_count(&self) -> usize {
        self.backoffs.len()
    }

    pub fn lexicon_count(&self) -> usize {
        self.lexicon.len()
    }

    pub fn lexicon_get(&self, word: &str) -> Option<&'a str> {
        self.lexicon
            .binary_search_by(|entry| entry.word.cmp(word))
            .ok()
            .map(|index| self.lexicon[index].phoneme)
    }

    pub fn memory_usage(&self) -> MemoryUsage {
        let token_index_bytes = vector_bytes(&self.tokens);
        let graph_index_bytes = vector_bytes(&self.graphs);
        let ngram_index_bytes = vector_bytes(&self.ngrams);
        let backoff_index_bytes = vector_bytes(&self.backoffs);
        let lexicon_index_bytes = vector_bytes(&self.lexicon);
        MemoryUsage {
            borrowed_model_bytes: self.bytes.len(),
            token_index_bytes,
            graph_index_bytes,
            ngram_index_bytes,
            backoff_index_bytes,
            lexicon_index_bytes,
            allocated_index_bytes: token_index_bytes
                + graph_index_bytes
                + ngram_index_bytes
                + backoff_index_bytes
                + lexicon_index_bytes,
            contiguous_allocations: usize::from(self.tokens.capacity() != 0)
                + usize::from(self.graphs.capacity() != 0)
                + usize::from(self.ngrams.capacity() != 0)
                + usize::from(self.backoffs.capacity() != 0)
                + usize::from(self.lexicon.capacity() != 0),
        }
    }

    pub(crate) fn has_ngrams(&self) -> bool {
        !self.ngrams.is_empty()
    }

    pub(crate) fn graph_candidates(&self, graph: &str) -> &[GraphCandidate<'a>] {
        let start = self
            .graphs
            .partition_point(|candidate| candidate.graph < graph);
        let end =
            self.graphs[start..].partition_point(|candidate| candidate.graph == graph) + start;
        &self.graphs[start..end]
    }

    pub(crate) fn token_phoneme(&self, token_id: u16) -> Option<&'a str> {
        self.tokens
            .get(usize::from(token_id))
            .map(|token| token.phoneme)
    }

    pub(crate) fn logp(&self, history: &[u16], token: u16) -> f32 {
        let mut ids = [0u16; MAX_NGRAM_ORDER];
        ids[..history.len()].copy_from_slice(history);
        ids[history.len()] = token;
        if let Some(score) = find_gram(&self.ngrams, &ids, (history.len() + 1) as u8) {
            return score;
        }
        if history.is_empty() {
            return self.unk;
        }
        let mut history_ids = [0u16; MAX_NGRAM_ORDER];
        history_ids[..history.len()].copy_from_slice(history);
        let backoff = find_gram(&self.backoffs, &history_ids, history.len() as u8).unwrap_or(0.0);
        backoff + self.logp(&history[1..], token)
    }
}

fn vector_bytes<T>(values: &Vec<T>) -> usize {
    values.capacity() * size_of::<T>()
}

fn find_gram(entries: &[GramEntry], ids: &[u16; MAX_NGRAM_ORDER], len: u8) -> Option<f32> {
    entries
        .binary_search_by(|entry| entry.compare_key(ids, len))
        .ok()
        .map(|index| entries[index].score())
}

fn sort_and_check_grams(
    entries: &mut [GramEntry],
    duplicate: ModelError,
) -> Result<(), ModelError> {
    entries.sort_unstable_by(|left, right| {
        left.ids
            .cmp(&right.ids)
            .then_with(|| left.len.cmp(&right.len))
    });
    if entries.windows(2).any(|pair| pair[0].same_key(&pair[1])) {
        return Err(duplicate);
    }
    Ok(())
}

fn validate_graph_groups(
    graphs: &[GraphCandidate<'_>],
    tokens: &[JointToken<'_>],
) -> Result<(), ModelError> {
    let mut start = 0usize;
    while start < graphs.len() {
        let graph = graphs[start].graph;
        let mut end = start + 1;
        while end < graphs.len() && graphs[end].graph == graph {
            end += 1;
        }
        if end - start > MAX_CANDIDATES_PER_GRAPH {
            return Err(ModelError::TooManyCandidates);
        }
        for left in start..end {
            let left_phoneme = tokens[usize::from(graphs[left].token_id)].phoneme;
            for right in left + 1..end {
                if left_phoneme == tokens[usize::from(graphs[right].token_id)].phoneme {
                    return Err(ModelError::DuplicateToken);
                }
            }
        }
        start = end;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum GramKind {
    Ngram,
    Backoff,
}

fn parse_grams(
    cursor: &mut Cursor<'_>,
    token_count: usize,
    order: u8,
    kind: GramKind,
) -> Result<Vec<GramEntry>, ModelError> {
    let (limit, minimum_bytes) = match kind {
        GramKind::Ngram => (MAX_NGRAMS, 4),
        GramKind::Backoff => (MAX_BACKOFFS, 3),
    };
    let count = cursor.count(limit)?;
    cursor.require_records(count, minimum_bytes)?;
    let mut entries = reserved_vec(count)?;
    for _ in 0..count {
        let len = cursor.u8()?;
        let valid_len = match kind {
            GramKind::Ngram => len != 0 && len <= order,
            GramKind::Backoff => len < order,
        };
        if !valid_len || usize::from(len) > MAX_NGRAM_ORDER {
            return Err(ModelError::InvalidGramLength);
        }
        let mut ids = [0u16; MAX_NGRAM_ORDER];
        for slot in &mut ids[..usize::from(len)] {
            let token_id = cursor.varint()?;
            let token_index = usize::try_from(token_id).map_err(|_| ModelError::InvalidTokenId)?;
            if token_index >= token_count {
                return Err(ModelError::InvalidTokenId);
            }
            *slot = u16::try_from(token_id).map_err(|_| ModelError::InvalidTokenId)?;
        }
        entries.push(GramEntry {
            ids,
            score_q: cursor.i16()?,
            len,
        });
    }
    Ok(entries)
}

fn reserved_vec<T>(capacity: usize) -> Result<Vec<T>, ModelError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| ModelError::Allocation)?;
    Ok(values)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ModelError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(ModelError::Truncated)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ModelError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, ModelError> {
        let value = *self.bytes.get(self.offset).ok_or(ModelError::Truncated)?;
        self.offset += 1;
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16, ModelError> {
        let encoded: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| ModelError::Truncated)?;
        Ok(u16::from_le_bytes(encoded))
    }

    fn i16(&mut self) -> Result<i16, ModelError> {
        let encoded: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| ModelError::Truncated)?;
        Ok(i16::from_le_bytes(encoded))
    }

    fn u32(&mut self) -> Result<u32, ModelError> {
        let encoded: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| ModelError::Truncated)?;
        Ok(u32::from_le_bytes(encoded))
    }

    fn f32(&mut self) -> Result<f32, ModelError> {
        let encoded: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| ModelError::Truncated)?;
        Ok(f32::from_le_bytes(encoded))
    }

    fn count(&mut self, maximum: usize) -> Result<usize, ModelError> {
        let count = self.u32()? as usize;
        if count > maximum {
            return Err(ModelError::CountLimit);
        }
        Ok(count)
    }

    fn string(&mut self, maximum: usize) -> Result<&'a str, ModelError> {
        let count = usize::from(self.u16()?);
        if count > maximum {
            return Err(ModelError::StringTooLong);
        }
        str::from_utf8(self.take(count)?).map_err(|_| ModelError::InvalidUtf8)
    }

    fn varint(&mut self) -> Result<u32, ModelError> {
        let mut value = 0u32;
        for index in 0..5u32 {
            let byte = self.u8()?;
            if index == 4 && byte & 0xf0 != 0 {
                return Err(ModelError::MalformedVarint);
            }
            value |= u32::from(byte & 0x7f) << (index * 7);
            if byte & 0x80 == 0 {
                if (index + 1) as usize != varint_len(value) {
                    return Err(ModelError::NonCanonicalVarint);
                }
                return Ok(value);
            }
        }
        Err(ModelError::MalformedVarint)
    }

    fn require_records(&self, count: usize, minimum_bytes: usize) -> Result<(), ModelError> {
        let required = count
            .checked_mul(minimum_bytes)
            .ok_or(ModelError::CountLimit)?;
        if self.bytes.len().saturating_sub(self.offset) < required {
            return Err(ModelError::Truncated);
        }
        Ok(())
    }

    fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

const fn varint_len(value: u32) -> usize {
    if value < (1 << 7) {
        1
    } else if value < (1 << 14) {
        2
    } else if value < (1 << 21) {
        3
    } else if value < (1 << 28) {
        4
    } else {
        5
    }
}

pub(crate) fn is_combining(character: char) -> bool {
    matches!(character as u32,
        0x0300..=0x036f |
        0x0483..=0x0489 |
        0x0591..=0x05bd | 0x05bf | 0x05c1..=0x05c2 | 0x05c4..=0x05c5 | 0x05c7 |
        0x0610..=0x061a | 0x064b..=0x065f | 0x0670 | 0x06d6..=0x06dc | 0x06df..=0x06e4 |
        0x0900..=0x0903 | 0x093a..=0x094f | 0x0951..=0x0957 |
        0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f)
}

fn grapheme_count(value: &str) -> usize {
    let mut count = 0usize;
    for character in value.chars() {
        if count == 0 || !is_combining(character) {
            count += 1;
        }
    }
    count
}
