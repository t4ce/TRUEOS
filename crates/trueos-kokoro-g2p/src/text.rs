use alloc::{borrow::Cow, string::String, vec::Vec};
use core::{ops::Range, str};

use crate::{DecodeError, IpaError, Model, PronunciationLookup, canonicalize_ipa, kokoro_token_id};

pub const KOKORO_BOUNDARY_TOKEN: u8 = 0;
pub const CHUNK_TARGET_MIN: usize = 175;
pub const CHUNK_TARGET_MAX: usize = 250;
pub const CHUNK_FALLBACK_MAX: usize = 450;
pub const KOKORO_MODEL_MAX: usize = 510;

const MAX_TEXT_BYTES: usize = 8 * 1024;
const MAX_FRONTEND_TOKENS: usize = 32 * 1024;
const MAX_ACRONYM_LETTERS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnglishTokenKind {
    Word,
    Number,
    Whitespace,
    Punctuation,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnglishToken<'a> {
    pub range: Range<usize>,
    pub kind: EnglishTokenKind,
    text: &'a str,
}

impl<'a> EnglishToken<'a> {
    pub const fn text(&self) -> &'a str {
        self.text
    }
}

/// Contiguous, zero-allocation English source tokenizer.
pub struct EnglishTokens<'a> {
    text: &'a str,
    offset: usize,
}

impl<'a> EnglishTokens<'a> {
    pub const fn new(text: &'a str) -> Self {
        Self { text, offset: 0 }
    }
}

impl<'a> Iterator for EnglishTokens<'a> {
    type Item = EnglishToken<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset == self.text.len() {
            return None;
        }
        let start = self.offset;
        let character = self.text.get(start..)?.chars().next()?;
        let (kind, end) = if character.is_whitespace() {
            (EnglishTokenKind::Whitespace, scan_while(self.text, start, char::is_whitespace))
        } else if character.is_ascii_digit() {
            (EnglishTokenKind::Number, scan_number(self.text, start))
        } else if character.is_alphabetic() {
            (EnglishTokenKind::Word, scan_word(self.text, start))
        } else if is_punctuation(character) {
            let end = if self.text.get(start..)?.starts_with("...") {
                start + 3
            } else {
                start + character.len_utf8()
            };
            (EnglishTokenKind::Punctuation, end)
        } else {
            (EnglishTokenKind::Other, start + character.len_utf8())
        };
        self.offset = end;
        Some(EnglishToken {
            range: start..end,
            kind,
            text: self.text.get(start..end)?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendOutput {
    pub phonemes: String,
    pub token_ids: Vec<u8>,
    pub chunks: Vec<Range<usize>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendError {
    TextTooLong,
    TooManyTokens,
    UnsupportedText(char),
    NoPronunciation,
    Decode(DecodeError),
    Ipa(IpaError),
    Allocation,
}

impl From<DecodeError> for FrontendError {
    fn from(error: DecodeError) -> Self {
        Self::Decode(error)
    }
}

impl From<IpaError> for FrontendError {
    fn from(error: IpaError) -> Self {
        Self::Ipa(error)
    }
}

pub fn prepare_english(model: &Model<'_>, text: &str) -> Result<FrontendOutput, FrontendError> {
    prepare_english_with(model, text, None)
}

pub fn prepare_english_with(
    model: &Model<'_>,
    text: &str,
    primary: Option<&dyn PronunciationLookup>,
) -> Result<FrontendOutput, FrontendError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(FrontendError::TextTooLong);
    }
    let mut output = FrontendOutput {
        phonemes: String::new(),
        token_ids: Vec::new(),
        chunks: Vec::new(),
    };
    output
        .phonemes
        .try_reserve(text.len())
        .map_err(|_| FrontendError::Allocation)?;
    output
        .token_ids
        .try_reserve(text.len())
        .map_err(|_| FrontendError::Allocation)?;

    let mut previous_lexical_end = None;
    for token in EnglishTokens::new(text) {
        let is_lexical = matches!(token.kind, EnglishTokenKind::Word | EnglishTokenKind::Number);
        if is_lexical && previous_lexical_end == Some(token.range.start) {
            append_character(&mut output, ' ')?;
        }
        match token.kind {
            EnglishTokenKind::Word => append_word(&mut output, model, token.text(), primary)?,
            EnglishTokenKind::Number => append_number(&mut output, model, token.text(), primary)?,
            EnglishTokenKind::Whitespace => append_character(&mut output, ' ')?,
            EnglishTokenKind::Punctuation => {
                append_character(&mut output, normalized_punctuation(token.text())?)?;
            }
            EnglishTokenKind::Other => append_other(&mut output, model, token.text(), primary)?,
        }
        previous_lexical_end = is_lexical.then_some(token.range.end);
    }
    output.chunks = chunk_ranges(&output.token_ids)?;
    Ok(output)
}

fn append_word(
    output: &mut FrontendOutput,
    model: &Model<'_>,
    word: &str,
    primary: Option<&dyn PronunciationLookup>,
) -> Result<(), FrontendError> {
    let word = normalize_apostrophes(word)?;
    if is_unknown_acronym(model, &word, primary) {
        let mut first = true;
        for letter in word
            .chars()
            .filter(|character| character.is_ascii_alphabetic())
        {
            if !first {
                append_character(output, ' ')?;
            }
            append_ipa(output, acronym_ipa(letter))?;
            first = false;
        }
        return Ok(());
    }
    let pronunciation = model.phonemize_with(&word, primary)?;
    if pronunciation.is_empty() {
        return Err(FrontendError::NoPronunciation);
    }
    append_ipa(output, &pronunciation)
}

fn normalize_apostrophes(word: &str) -> Result<Cow<'_, str>, FrontendError> {
    if !word.contains(['‘', '’']) {
        return Ok(Cow::Borrowed(word));
    }
    let mut normalized = String::new();
    normalized
        .try_reserve_exact(word.len())
        .map_err(|_| FrontendError::Allocation)?;
    for character in word.chars() {
        normalized.push(if matches!(character, '‘' | '’') {
            '\''
        } else {
            character
        });
    }
    Ok(Cow::Owned(normalized))
}

fn is_unknown_acronym(
    model: &Model<'_>,
    word: &str,
    primary: Option<&dyn PronunciationLookup>,
) -> bool {
    let letters = word.chars().count();
    if !(2..=MAX_ACRONYM_LETTERS).contains(&letters)
        || !word.chars().all(|character| character.is_ascii_uppercase())
    {
        return false;
    }
    if primary.and_then(|source| source.lookup(word)).is_some() || model.lexicon_get(word).is_some()
    {
        return false;
    }
    let mut lower = [0u8; MAX_ACRONYM_LETTERS];
    for (destination, source) in lower.iter_mut().zip(word.bytes()) {
        *destination = source.to_ascii_lowercase();
    }
    let lowercase = match str::from_utf8(&lower[..letters]) {
        Ok(value) => value,
        Err(_) => return true,
    };
    primary
        .and_then(|source| source.lookup(lowercase))
        .is_none()
        && model.lexicon_get(lowercase).is_none()
}

const fn acronym_ipa(letter: char) -> &'static str {
    match letter {
        'A' => "eɪ",
        'B' => "biː",
        'C' => "siː",
        'D' => "diː",
        'E' => "iː",
        'F' => "ɛf",
        'G' => "ʤiː",
        'H' => "eɪʧ",
        'I' => "aɪ",
        'J' => "ʤeɪ",
        'K' => "keɪ",
        'L' => "ɛl",
        'M' => "ɛm",
        'N' => "ɛn",
        'O' => "oʊ",
        'P' => "piː",
        'Q' => "kjuː",
        'R' => "ɑɹ",
        'S' => "ɛs",
        'T' => "tiː",
        'U' => "juː",
        'V' => "viː",
        'W' => "dʌbəljuː",
        'X' => "ɛks",
        'Y' => "waɪ",
        'Z' => "ziː",
        _ => "",
    }
}

fn append_number(
    output: &mut FrontendOutput,
    model: &Model<'_>,
    number: &str,
    primary: Option<&dyn PronunciationLookup>,
) -> Result<(), FrontendError> {
    let words = cardinal_words(number)?;
    for (index, word) in words.split_ascii_whitespace().enumerate() {
        if index != 0 {
            append_character(output, ' ')?;
        }
        append_word(output, model, word, primary)?;
    }
    Ok(())
}

fn cardinal_words(number: &str) -> Result<String, FrontendError> {
    let mut digits = String::new();
    digits
        .try_reserve_exact(number.len())
        .map_err(|_| FrontendError::Allocation)?;
    for character in number.chars() {
        if character.is_ascii_digit() {
            digits.push(character);
        }
    }
    let spell_digits = digits.len() > 12 || (digits.len() > 1 && digits.starts_with('0'));
    if spell_digits {
        let mut output = String::new();
        output
            .try_reserve_exact(digits.len().saturating_mul(6))
            .map_err(|_| FrontendError::Allocation)?;
        for (index, digit) in digits.bytes().enumerate() {
            if index != 0 {
                output.push(' ');
            }
            output.push_str(digit_word(digit).ok_or(FrontendError::NoPronunciation)?);
        }
        return Ok(output);
    }
    let mut value = 0u64;
    for digit in digits.bytes() {
        value = value
            .checked_mul(10)
            .and_then(|current| current.checked_add(u64::from(digit - b'0')))
            .ok_or(FrontendError::NoPronunciation)?;
    }
    let mut output = String::new();
    output
        .try_reserve_exact(256)
        .map_err(|_| FrontendError::Allocation)?;
    write_cardinal(value, &mut output)?;
    Ok(output)
}

fn write_cardinal(value: u64, output: &mut String) -> Result<(), FrontendError> {
    for (scale, name) in [
        (1_000_000_000_000u64, "trillion"),
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1_000, "thousand"),
    ] {
        if value >= scale {
            write_cardinal(value / scale, output)?;
            push_word(output, name)?;
            let remainder = value % scale;
            if remainder != 0 {
                write_cardinal(remainder, output)?;
            }
            return Ok(());
        }
    }
    if value >= 100 {
        push_word(output, small_number_word((value / 100) as u8))?;
        push_word(output, "hundred")?;
        if value % 100 != 0 {
            write_cardinal(value % 100, output)?;
        }
    } else if value >= 20 {
        push_word(output, tens_word((value / 10) as u8))?;
        if value % 10 != 0 {
            push_word(output, small_number_word((value % 10) as u8))?;
        }
    } else {
        push_word(output, small_number_word(value as u8))?;
    }
    Ok(())
}

fn push_word(output: &mut String, word: &str) -> Result<(), FrontendError> {
    output
        .try_reserve(word.len() + usize::from(!output.is_empty()))
        .map_err(|_| FrontendError::Allocation)?;
    if !output.is_empty() {
        output.push(' ');
    }
    output.push_str(word);
    Ok(())
}

const fn small_number_word(value: u8) -> &'static str {
    match value {
        0 => "zero",
        1 => "one",
        2 => "two",
        3 => "three",
        4 => "four",
        5 => "five",
        6 => "six",
        7 => "seven",
        8 => "eight",
        9 => "nine",
        10 => "ten",
        11 => "eleven",
        12 => "twelve",
        13 => "thirteen",
        14 => "fourteen",
        15 => "fifteen",
        16 => "sixteen",
        17 => "seventeen",
        18 => "eighteen",
        19 => "nineteen",
        _ => "",
    }
}

const fn tens_word(value: u8) -> &'static str {
    match value {
        2 => "twenty",
        3 => "thirty",
        4 => "forty",
        5 => "fifty",
        6 => "sixty",
        7 => "seventy",
        8 => "eighty",
        9 => "ninety",
        _ => "",
    }
}

const fn digit_word(digit: u8) -> Option<&'static str> {
    match digit {
        b'0' => Some("zero"),
        b'1' => Some("one"),
        b'2' => Some("two"),
        b'3' => Some("three"),
        b'4' => Some("four"),
        b'5' => Some("five"),
        b'6' => Some("six"),
        b'7' => Some("seven"),
        b'8' => Some("eight"),
        b'9' => Some("nine"),
        _ => None,
    }
}

fn append_other(
    output: &mut FrontendOutput,
    model: &Model<'_>,
    text: &str,
    primary: Option<&dyn PronunciationLookup>,
) -> Result<(), FrontendError> {
    let character = text.chars().next().ok_or(FrontendError::NoPronunciation)?;
    let words = match character {
        '%' => "percent",
        '&' => "and",
        '+' => "plus",
        '@' => "at",
        '/' => "slash",
        '=' => "equals",
        '#' => "number",
        '_' => {
            append_character(output, ' ')?;
            return Ok(());
        }
        unsupported => return Err(FrontendError::UnsupportedText(unsupported)),
    };
    append_word(output, model, words, primary)
}

fn append_ipa(output: &mut FrontendOutput, ipa: &str) -> Result<(), FrontendError> {
    let encoded = canonicalize_ipa(ipa)?;
    let new_count = output
        .token_ids
        .len()
        .checked_add(encoded.token_ids.len())
        .ok_or(FrontendError::TooManyTokens)?;
    if new_count > MAX_FRONTEND_TOKENS {
        return Err(FrontendError::TooManyTokens);
    }
    output
        .phonemes
        .try_reserve(encoded.phonemes.len())
        .map_err(|_| FrontendError::Allocation)?;
    output
        .token_ids
        .try_reserve(encoded.token_ids.len())
        .map_err(|_| FrontendError::Allocation)?;
    output.phonemes.push_str(&encoded.phonemes);
    output.token_ids.extend_from_slice(&encoded.token_ids);
    Ok(())
}

fn append_character(output: &mut FrontendOutput, character: char) -> Result<(), FrontendError> {
    let token = kokoro_token_id(character).ok_or(FrontendError::UnsupportedText(character))?;
    if output.token_ids.len() >= MAX_FRONTEND_TOKENS {
        return Err(FrontendError::TooManyTokens);
    }
    output
        .phonemes
        .try_reserve(character.len_utf8())
        .map_err(|_| FrontendError::Allocation)?;
    output
        .token_ids
        .try_reserve(1)
        .map_err(|_| FrontendError::Allocation)?;
    output.phonemes.push(character);
    output.token_ids.push(token);
    Ok(())
}

fn normalized_punctuation(text: &str) -> Result<char, FrontendError> {
    match text {
        "..." | "…" => Ok('…'),
        "-" | "–" | "—" => Ok('—'),
        "'" | "‘" | "’" | "\"" => Ok('"'),
        "[" | "{" => Ok('('),
        "]" | "}" => Ok(')'),
        value => value.chars().next().ok_or(FrontendError::NoPronunciation),
    }
}

pub fn chunk_ranges(token_ids: &[u8]) -> Result<Vec<Range<usize>>, FrontendError> {
    if token_ids.len() > MAX_FRONTEND_TOKENS {
        return Err(FrontendError::TooManyTokens);
    }
    let chunk_capacity = token_ids.len().div_ceil(CHUNK_TARGET_MIN);
    let mut ranges = Vec::new();
    ranges
        .try_reserve_exact(chunk_capacity)
        .map_err(|_| FrontendError::Allocation)?;
    let mut start = 0usize;
    while start < token_ids.len() {
        let remaining = token_ids.len() - start;
        if remaining <= CHUNK_TARGET_MAX {
            ranges.push(start..token_ids.len());
            break;
        }
        let target_high = start + CHUNK_TARGET_MAX;
        if let Some(end) = preferred_break(token_ids, start + CHUNK_TARGET_MIN, target_high) {
            ranges.push(start..end);
            start = end;
            continue;
        }
        if remaining <= CHUNK_FALLBACK_MAX {
            ranges.push(start..token_ids.len());
            break;
        }
        let fallback_high = start + CHUNK_FALLBACK_MAX;
        if let Some(end) = preferred_break(token_ids, target_high + 1, fallback_high) {
            ranges.push(start..end);
            start = end;
            continue;
        }
        if remaining <= KOKORO_MODEL_MAX {
            ranges.push(start..token_ids.len());
            break;
        }
        let hard_high = start + KOKORO_MODEL_MAX;
        let end = preferred_break(token_ids, fallback_high + 1, hard_high).unwrap_or(hard_high);
        ranges.push(start..end);
        start = end;
    }
    Ok(ranges)
}

fn preferred_break(token_ids: &[u8], low: usize, high: usize) -> Option<usize> {
    let high = high.min(token_ids.len());
    if low > high {
        return None;
    }
    for class in [BreakClass::Sentence, BreakClass::Clause, BreakClass::Space] {
        if let Some(boundary) = (low..=high).rev().find(|&boundary| {
            token_ids
                .get(boundary.saturating_sub(1))
                .is_some_and(|&token| class.matches(token))
        }) {
            return Some(boundary);
        }
    }
    None
}

#[derive(Clone, Copy)]
enum BreakClass {
    Sentence,
    Clause,
    Space,
}

impl BreakClass {
    const fn matches(self, token: u8) -> bool {
        match self {
            Self::Sentence => matches!(token, 4 | 5 | 6 | 10),
            Self::Clause => matches!(token, 1 | 2 | 3 | 9),
            Self::Space => token == 16,
        }
    }
}

fn scan_while(text: &str, start: usize, predicate: impl Fn(char) -> bool) -> usize {
    let mut end = start;
    for character in text[start..].chars() {
        if !predicate(character) {
            break;
        }
        end += character.len_utf8();
    }
    end
}

fn scan_number(text: &str, start: usize) -> usize {
    let mut end = start;
    let mut characters = text[start..].char_indices().peekable();
    while let Some((offset, character)) = characters.next() {
        if character.is_ascii_digit() {
            end = start + offset + 1;
        } else if character == ','
            && characters
                .peek()
                .is_some_and(|(_, next)| next.is_ascii_digit())
        {
            end = start + offset + 1;
        } else {
            break;
        }
    }
    end
}

fn scan_word(text: &str, start: usize) -> usize {
    let mut end = start;
    let mut characters = text[start..].char_indices().peekable();
    while let Some((offset, character)) = characters.next() {
        if character.is_alphabetic() {
            end = start + offset + character.len_utf8();
        } else if matches!(character, '\'' | '‘' | '’' | '-')
            && characters
                .peek()
                .is_some_and(|(_, next)| next.is_alphabetic())
        {
            end = start + offset + character.len_utf8();
        } else {
            break;
        }
    }
    end
}

const fn is_punctuation(character: char) -> bool {
    matches!(
        character,
        ';' | ':'
            | ','
            | '.'
            | '!'
            | '?'
            | '-'
            | '–'
            | '—'
            | '…'
            | '"'
            | '\''
            | '‘'
            | '’'
            | '“'
            | '”'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
    )
}
