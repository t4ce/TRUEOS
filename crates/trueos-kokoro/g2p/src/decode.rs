use alloc::{string::String, vec::Vec};

use crate::model::{MAX_NGRAM_ORDER, Model, is_combining};

const BEAM_WIDTH: usize = 8;
const SKIP_PENALTY: f32 = -20.0;
const MAX_WORD_BYTES: usize = 256;
const MAX_WORD_GRAPHEMES: usize = 128;
const MAX_PRONUNCIATION_BYTES: usize = 8 * 1024;
const LOWERCASE_RESERVE: usize = MAX_WORD_BYTES * 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    WordTooLong,
    TooManyGraphemes,
    PronunciationTooLong,
    ModelInvariant,
    Allocation,
}

/// Optional primary dictionary queried before the model's own lexicon/beam.
///
/// A future sorted zero-copy Misaki dictionary can implement this trait without
/// changing the text pipeline or the resident G2P2 fallback model.
pub trait PronunciationLookup {
    fn lookup(&self, word: &str) -> Option<&str>;
}

impl<'a> Model<'a> {
    /// Exact upstream-compatible word pronunciation without an override layer.
    pub fn phonemize(&self, word: &str) -> Result<String, DecodeError> {
        self.phonemize_with(word, None)
    }

    /// Query `primary` first, then the embedded lexicon, then the n-gram beam.
    pub fn phonemize_with(
        &self,
        word: &str,
        primary: Option<&dyn PronunciationLookup>,
    ) -> Result<String, DecodeError> {
        if word.len() > MAX_WORD_BYTES {
            return Err(DecodeError::WordTooLong);
        }
        if let Some(pronunciation) = primary.and_then(|source| source.lookup(word)) {
            return copy_pronunciation(pronunciation);
        }
        if let Some(pronunciation) = self.lexicon_get(word) {
            return copy_pronunciation(pronunciation);
        }

        let lowercase = lowercase(word)?;
        if lowercase != word {
            if let Some(pronunciation) = primary.and_then(|source| source.lookup(&lowercase)) {
                return copy_pronunciation(pronunciation);
            }
            if let Some(pronunciation) = self.lexicon_get(&lowercase) {
                return copy_pronunciation(pronunciation);
            }
        }

        if self.is_logographic() {
            let fallback = character_fallback(self, word)?;
            if !fallback.is_empty() {
                return Ok(fallback);
            }
            if self.has_ngrams() {
                return beam_decode(self, &lowercase);
            }
            return Ok(String::new());
        }

        if self.has_ngrams() {
            let pronunciation = beam_decode(self, &lowercase)?;
            if !pronunciation.is_empty() {
                return Ok(pronunciation);
            }
        }
        character_fallback(self, word)
    }
}

fn copy_pronunciation(pronunciation: &str) -> Result<String, DecodeError> {
    if pronunciation.len() > MAX_PRONUNCIATION_BYTES {
        return Err(DecodeError::PronunciationTooLong);
    }
    let mut output = String::new();
    output
        .try_reserve_exact(pronunciation.len())
        .map_err(|_| DecodeError::Allocation)?;
    output.push_str(pronunciation);
    Ok(output)
}

fn lowercase(word: &str) -> Result<String, DecodeError> {
    let mut output = String::new();
    output
        .try_reserve_exact(LOWERCASE_RESERVE)
        .map_err(|_| DecodeError::Allocation)?;
    for character in word.chars() {
        for lower in character.to_lowercase() {
            output.push(lower);
        }
    }
    if output.len() > LOWERCASE_RESERVE {
        return Err(DecodeError::WordTooLong);
    }
    Ok(output)
}

fn character_fallback(model: &Model<'_>, word: &str) -> Result<String, DecodeError> {
    let boundaries = grapheme_boundaries(word)?;
    let mut output = String::new();
    let mut any = false;
    for pair in boundaries.windows(2) {
        let cluster = word
            .get(pair[0]..pair[1])
            .ok_or(DecodeError::ModelInvariant)?;
        if let Some(pronunciation) = model.lexicon_get(cluster) {
            let new_len = output
                .len()
                .checked_add(pronunciation.len())
                .ok_or(DecodeError::PronunciationTooLong)?;
            if new_len > MAX_PRONUNCIATION_BYTES {
                return Err(DecodeError::PronunciationTooLong);
            }
            output
                .try_reserve(pronunciation.len())
                .map_err(|_| DecodeError::Allocation)?;
            output.push_str(pronunciation);
            any = true;
        }
    }
    if any { Ok(output) } else { Ok(String::new()) }
}

struct Hypothesis {
    position: usize,
    history: [u16; MAX_NGRAM_ORDER - 1],
    history_len: u8,
    output: Vec<u16>,
    score: f32,
}

impl Hypothesis {
    fn duplicate(&self) -> Result<Self, DecodeError> {
        let mut output = Vec::new();
        output
            .try_reserve_exact(self.output.len())
            .map_err(|_| DecodeError::Allocation)?;
        output.extend_from_slice(&self.output);
        Ok(Self {
            position: self.position,
            history: self.history,
            history_len: self.history_len,
            output,
            score: self.score,
        })
    }

    fn advance(
        &self,
        position: usize,
        token: u16,
        order: usize,
        score: f32,
    ) -> Result<Self, DecodeError> {
        let mut next = self.duplicate()?;
        next.position = position;
        next.score += score;
        next.output
            .try_reserve(1)
            .map_err(|_| DecodeError::Allocation)?;
        next.output.push(token);
        let retained = order.saturating_sub(1);
        if retained == 0 {
            next.history_len = 0;
        } else if usize::from(next.history_len) < retained {
            next.history[usize::from(next.history_len)] = token;
            next.history_len += 1;
        } else {
            next.history.copy_within(1..retained, 0);
            next.history[retained - 1] = token;
            next.history_len = retained as u8;
        }
        Ok(next)
    }

    fn skipped(&self) -> Result<Self, DecodeError> {
        let mut next = self.duplicate()?;
        next.position += 1;
        next.score += SKIP_PENALTY;
        Ok(next)
    }

    fn history(&self) -> &[u16] {
        &self.history[..usize::from(self.history_len)]
    }
}

fn beam_decode(model: &Model<'_>, word: &str) -> Result<String, DecodeError> {
    let boundaries = grapheme_boundaries(word)?;
    let grapheme_count = boundaries.len().saturating_sub(1);
    if grapheme_count == 0 {
        return Ok(String::new());
    }
    let order = usize::from(model.order()).max(1);
    let mut beam = Vec::new();
    beam.try_reserve_exact(1)
        .map_err(|_| DecodeError::Allocation)?;
    beam.push(Hypothesis {
        position: 0,
        history: [0; MAX_NGRAM_ORDER - 1],
        history_len: 0,
        output: Vec::new(),
        score: 0.0,
    });

    while !beam
        .iter()
        .all(|hypothesis| hypothesis.position == grapheme_count)
    {
        let mut next = Vec::new();
        next.try_reserve(BEAM_WIDTH * model.max_grapheme_chunk())
            .map_err(|_| DecodeError::Allocation)?;
        for hypothesis in &beam {
            if hypothesis.position == grapheme_count {
                push_hypothesis(&mut next, hypothesis.duplicate()?)?;
                continue;
            }
            let maximum = model
                .max_grapheme_chunk()
                .min(grapheme_count - hypothesis.position);
            let mut matched = false;
            for chunk_len in 1..=maximum {
                let start = boundaries[hypothesis.position];
                let end = boundaries[hypothesis.position + chunk_len];
                let graph = word.get(start..end).ok_or(DecodeError::ModelInvariant)?;
                let candidates = model.graph_candidates(graph);
                if !candidates.is_empty() {
                    matched = true;
                }
                for candidate in candidates {
                    let score = model.logp(hypothesis.history(), candidate.token_id);
                    let expanded = hypothesis.advance(
                        hypothesis.position + chunk_len,
                        candidate.token_id,
                        order,
                        score,
                    )?;
                    push_hypothesis(&mut next, expanded)?;
                }
            }
            if !matched {
                push_hypothesis(&mut next, hypothesis.skipped()?)?;
            }
        }
        if next.is_empty() {
            break;
        }
        next.sort_by(|left, right| right.score.total_cmp(&left.score));
        next.truncate(BEAM_WIDTH);
        beam = next;
    }

    let best = beam
        .iter()
        .filter(|hypothesis| hypothesis.position == grapheme_count)
        .map(|hypothesis| (hypothesis.score + model.logp(hypothesis.history(), 0), hypothesis))
        .max_by(|left, right| left.0.total_cmp(&right.0));
    let Some((_, best)) = best else {
        return Ok(String::new());
    };

    let mut required = 0usize;
    for &token_id in &best.output {
        let phoneme = model
            .token_phoneme(token_id)
            .ok_or(DecodeError::ModelInvariant)?;
        required = required
            .checked_add(phoneme.len())
            .ok_or(DecodeError::PronunciationTooLong)?;
        if required > MAX_PRONUNCIATION_BYTES {
            return Err(DecodeError::PronunciationTooLong);
        }
    }
    let mut output = String::new();
    output
        .try_reserve_exact(required)
        .map_err(|_| DecodeError::Allocation)?;
    for &token_id in &best.output {
        output.push_str(
            model
                .token_phoneme(token_id)
                .ok_or(DecodeError::ModelInvariant)?,
        );
    }
    Ok(output)
}

fn push_hypothesis(
    hypotheses: &mut Vec<Hypothesis>,
    hypothesis: Hypothesis,
) -> Result<(), DecodeError> {
    hypotheses
        .try_reserve(1)
        .map_err(|_| DecodeError::Allocation)?;
    hypotheses.push(hypothesis);
    Ok(())
}

fn grapheme_boundaries(word: &str) -> Result<Vec<usize>, DecodeError> {
    let mut boundaries = Vec::new();
    boundaries
        .try_reserve_exact(MAX_WORD_GRAPHEMES + 1)
        .map_err(|_| DecodeError::Allocation)?;
    boundaries.push(0);
    for (offset, character) in word.char_indices() {
        if offset != 0 && !is_combining(character) {
            if boundaries.len() > MAX_WORD_GRAPHEMES {
                return Err(DecodeError::TooManyGraphemes);
            }
            boundaries.push(offset);
        }
    }
    if boundaries.len() > MAX_WORD_GRAPHEMES {
        return Err(DecodeError::TooManyGraphemes);
    }
    boundaries.push(word.len());
    Ok(boundaries)
}
