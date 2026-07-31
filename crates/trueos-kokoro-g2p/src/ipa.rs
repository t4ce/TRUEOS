use alloc::{string::String, vec::Vec};

const MAX_IPA_INPUT_BYTES: usize = 8 * 1024;
const MAX_IPA_OUTPUT_TOKENS: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpaError {
    InputTooLong,
    OutputTooLong,
    UnsupportedCharacter(char),
    Allocation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedPhonemes {
    pub phonemes: String,
    pub token_ids: Vec<u8>,
}

/// Canonicalize general IPA to Kokoro v1.0 symbols and encode fixed token IDs.
pub fn canonicalize_ipa(input: &str) -> Result<EncodedPhonemes, IpaError> {
    if input.len() > MAX_IPA_INPUT_BYTES {
        return Err(IpaError::InputTooLong);
    }
    let string_capacity = input.len().checked_mul(2).ok_or(IpaError::OutputTooLong)?;
    let mut output = EncodedPhonemes {
        phonemes: String::new(),
        token_ids: Vec::new(),
    };
    output
        .phonemes
        .try_reserve_exact(string_capacity)
        .map_err(|_| IpaError::Allocation)?;
    output
        .token_ids
        .try_reserve_exact(input.len())
        .map_err(|_| IpaError::Allocation)?;

    let mut characters = input.chars().peekable();
    while let Some(character) = characters.next() {
        let mut lookahead = characters.clone();
        if matches!(lookahead.next(), Some('\u{035c}' | '\u{0361}')) {
            if let Some(right) = lookahead.next()
                && let Some(affricate) = affricate(character, right)
            {
                characters.next();
                characters.next();
                push(&mut output, affricate)?;
                continue;
            }
        }
        if character == 'ɜ' && characters.peek() == Some(&'˞') {
            characters.next();
            push(&mut output, 'ɚ')?;
            continue;
        }
        if characters.peek() == Some(&'\u{0329}') {
            characters.next();
            push(&mut output, 'ᵊ')?;
            push_mapped(&mut output, character)?;
            continue;
        }
        push_mapped(&mut output, character)?;
    }
    Ok(output)
}

fn affricate(left: char, right: char) -> Option<char> {
    match (left, right) {
        ('d', 'z') => Some('ʣ'),
        ('d', 'ʒ') => Some('ʤ'),
        ('d', 'ɕ') => Some('ʥ'),
        ('t', 's') => Some('ʦ'),
        ('t', 'ʃ') => Some('ʧ'),
        ('t', 'ɕ') => Some('ʨ'),
        _ => None,
    }
}

fn push_mapped(output: &mut EncodedPhonemes, character: char) -> Result<(), IpaError> {
    match character {
        'g' => push(output, 'ɡ'),
        'ɝ' => push(output, 'ɚ'),
        'x' | 'ç' => push(output, 'k'),
        'ɬ' | 'ɫ' | 'ɭ' => push(output, 'l'),
        'ɱ' | 'ᵐ' => push(output, 'm'),
        'ɦ' => push(output, 'h'),
        'ɘ' | 'ɵ' => push(output, 'ə'),
        'ʉ' | 'ᵿ' => push(output, 'u'),
        'ʏ' => push(output, 'i'),
        'ʍ' | 'ʷ' => push(output, 'w'),
        'ʙ' => push(output, 'b'),
        'ʱ' => push(output, 'ʰ'),
        'ʴ' => push(output, 'r'),
        'ˀ' => push(output, 'ʔ'),
        'ˑ' => push(output, 'ː'),
        'ã' => {
            push(output, 'a')?;
            push(output, '\u{0303}')
        }
        'ĩ' => {
            push(output, 'i')?;
            push(output, '\u{0303}')
        }
        'á' | 'ä' | 'ā' | 'ă' => push(output, 'a'),
        'ē' => push(output, 'e'),
        'ī' | 'ĭ' | 'ǐ' => push(output, 'i'),
        'ô' | 'ō' | 'ŏ' => push(output, 'o'),
        'ü' => push(output, 'u'),
        'ǁ' => push(output, 'k'),
        '\u{035c}' | '\u{0361}' | '\u{0301}' | '\u{0306}' | '\u{0308}' | '\u{030a}'
        | '\u{030d}' | '\u{0319}' | '\u{031a}' | '\u{031d}' | '\u{031e}' | '\u{031f}'
        | '\u{0320}' | '\u{0325}' | '\u{0329}' | '\u{032a}' | '\u{032c}' | '\u{032f}'
        | '\u{0330}' | '\u{0346}' | '˞' | 'ˤ' | '˥' | '˦' | '˧' | '˨' | '˩' | '˭' | '-' | '‿'
        | '⁽' | '⁾' => Ok(()),
        value if value.is_whitespace() => push(output, ' '),
        value => push(output, value),
    }
}

fn push(output: &mut EncodedPhonemes, character: char) -> Result<(), IpaError> {
    if output.token_ids.len() >= MAX_IPA_OUTPUT_TOKENS {
        return Err(IpaError::OutputTooLong);
    }
    let token = kokoro_token_id(character).ok_or(IpaError::UnsupportedCharacter(character))?;
    output.phonemes.push(character);
    output.token_ids.push(token);
    Ok(())
}

/// Exact Kokoro v1.0 vocabulary mapping from the released model configuration.
pub const fn kokoro_token_id(character: char) -> Option<u8> {
    match character {
        ';' => Some(1),
        ':' => Some(2),
        ',' => Some(3),
        '.' => Some(4),
        '!' => Some(5),
        '?' => Some(6),
        '—' => Some(9),
        '…' => Some(10),
        '"' => Some(11),
        '(' => Some(12),
        ')' => Some(13),
        '“' => Some(14),
        '”' => Some(15),
        ' ' => Some(16),
        '\u{0303}' => Some(17),
        'ʣ' => Some(18),
        'ʥ' => Some(19),
        'ʦ' => Some(20),
        'ʨ' => Some(21),
        'ᵝ' => Some(22),
        '\u{ab67}' => Some(23),
        'A' => Some(24),
        'I' => Some(25),
        'O' => Some(31),
        'Q' => Some(33),
        'S' => Some(35),
        'T' => Some(36),
        'W' => Some(39),
        'Y' => Some(41),
        'ᵊ' => Some(42),
        'a' => Some(43),
        'b' => Some(44),
        'c' => Some(45),
        'd' => Some(46),
        'e' => Some(47),
        'f' => Some(48),
        'h' => Some(50),
        'i' => Some(51),
        'j' => Some(52),
        'k' => Some(53),
        'l' => Some(54),
        'm' => Some(55),
        'n' => Some(56),
        'o' => Some(57),
        'p' => Some(58),
        'q' => Some(59),
        'r' => Some(60),
        's' => Some(61),
        't' => Some(62),
        'u' => Some(63),
        'v' => Some(64),
        'w' => Some(65),
        'x' => Some(66),
        'y' => Some(67),
        'z' => Some(68),
        'ɑ' => Some(69),
        'ɐ' => Some(70),
        'ɒ' => Some(71),
        'æ' => Some(72),
        'β' => Some(75),
        'ɔ' => Some(76),
        'ɕ' => Some(77),
        'ç' => Some(78),
        'ɖ' => Some(80),
        'ð' => Some(81),
        'ʤ' => Some(82),
        'ə' => Some(83),
        'ɚ' => Some(85),
        'ɛ' => Some(86),
        'ɜ' => Some(87),
        'ɟ' => Some(90),
        'ɡ' => Some(92),
        'ɥ' => Some(99),
        'ɨ' => Some(101),
        'ɪ' => Some(102),
        'ʝ' => Some(103),
        'ɯ' => Some(110),
        'ɰ' => Some(111),
        'ŋ' => Some(112),
        'ɳ' => Some(113),
        'ɲ' => Some(114),
        'ɴ' => Some(115),
        'ø' => Some(116),
        'ɸ' => Some(118),
        'θ' => Some(119),
        'œ' => Some(120),
        'ɹ' => Some(123),
        'ɾ' => Some(125),
        'ɻ' => Some(126),
        'ʁ' => Some(128),
        'ɽ' => Some(129),
        'ʂ' => Some(130),
        'ʃ' => Some(131),
        'ʈ' => Some(132),
        'ʧ' => Some(133),
        'ʊ' => Some(135),
        'ʋ' => Some(136),
        'ʌ' => Some(138),
        'ɣ' => Some(139),
        'ɤ' => Some(140),
        'χ' => Some(142),
        'ʎ' => Some(143),
        'ʒ' => Some(147),
        'ʔ' => Some(148),
        'ˈ' => Some(156),
        'ˌ' => Some(157),
        'ː' => Some(158),
        'ʰ' => Some(162),
        'ʲ' => Some(164),
        '↓' => Some(169),
        '→' => Some(171),
        '↗' => Some(172),
        '↘' => Some(173),
        'ᵻ' => Some(177),
        _ => None,
    }
}
