#[derive(Copy, Clone, Debug)]
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
}

#[derive(Copy, Clone, Debug)]
pub struct IntlLocaleProfile {
    pub code: &'static str,
    pub language_name: &'static str,
    pub decimal_separator: char,
    pub grouping_separator: char,
    pub minus_sign: char,
    pub percent_sign: char,
    /// CLDR-like date skeleton for simple formatter shims.
    pub date_pattern: &'static str,
    /// CLDR-like time skeleton for simple formatter shims.
    pub time_pattern: &'static str,
    /// First day of week using ISO: 1=Mon ... 7=Sun.
    pub first_day_of_week: u8,
}

pub const LANGUAGES: &[Language] = &[
    Language {
        code: "sq",
        name: "Albanian",
    },
    Language {
        code: "af",
        name: "Afrikaans",
    },
    Language {
        code: "ar",
        name: "Arabic",
    },
    Language {
        code: "az",
        name: "Azerbaijani",
    },
    Language {
        code: "eu",
        name: "Basque",
    },
    Language {
        code: "be",
        name: "Belarusian",
    },
    Language {
        code: "bg",
        name: "Bulgarian",
    },
    Language {
        code: "ca",
        name: "Catalan",
    },
    Language {
        code: "zh_cn",
        name: "Chinese Simplified",
    },
    Language {
        code: "zh_tw",
        name: "Chinese Traditional",
    },
    Language {
        code: "hr",
        name: "Croatian",
    },
    Language {
        code: "cz",
        name: "Czech",
    },
    Language {
        code: "da",
        name: "Danish",
    },
    Language {
        code: "nl",
        name: "Dutch",
    },
    Language {
        code: "en",
        name: "English",
    },
    Language {
        code: "fi",
        name: "Finnish",
    },
    Language {
        code: "fr",
        name: "French",
    },
    Language {
        code: "gl",
        name: "Galician",
    },
    Language {
        code: "de",
        name: "German",
    },
    Language {
        code: "el",
        name: "Greek",
    },
    Language {
        code: "he",
        name: "Hebrew",
    },
    Language {
        code: "hi",
        name: "Hindi",
    },
    Language {
        code: "hu",
        name: "Hungarian",
    },
    Language {
        code: "is",
        name: "Icelandic",
    },
    Language {
        code: "id",
        name: "Indonesian",
    },
    Language {
        code: "it",
        name: "Italian",
    },
    Language {
        code: "ja",
        name: "Japanese",
    },
    Language {
        code: "kr",
        name: "Korean",
    },
    Language {
        code: "ku",
        name: "Kurmanji (Kurdish)",
    },
    Language {
        code: "la",
        name: "Latvian",
    },
    Language {
        code: "lt",
        name: "Lithuanian",
    },
    Language {
        code: "mk",
        name: "Macedonian",
    },
    Language {
        code: "no",
        name: "Norwegian",
    },
    Language {
        code: "fa",
        name: "Persian (Farsi)",
    },
    Language {
        code: "pl",
        name: "Polish",
    },
    Language {
        code: "pt",
        name: "Portuguese",
    },
    Language {
        code: "pt_br",
        name: "Português Brasil",
    },
    Language {
        code: "ro",
        name: "Romanian",
    },
    Language {
        code: "ru",
        name: "Russian",
    },
    Language {
        code: "sr",
        name: "Serbian",
    },
    Language {
        code: "sk",
        name: "Slovak",
    },
    Language {
        code: "sl",
        name: "Slovenian",
    },
    Language {
        code: "es",
        name: "Spanish",
    },
    Language {
        code: "sv",
        name: "Swedish",
    },
    Language {
        code: "th",
        name: "Thai",
    },
    Language {
        code: "tr",
        name: "Turkish",
    },
    Language {
        code: "uk",
        name: "Ukrainian",
    },
    Language {
        code: "vi",
        name: "Vietnamese",
    },
    Language {
        code: "zu",
        name: "Zulu",
    },
];

pub const DEFAULT_LANGUAGE_CODE: &str = "en";
pub const DEFAULT_GERMAN_LANGUAGE_CODE: &str = "de";
pub const DEFAULT_INTL_LOCALE: &str = "en";

pub const INTL_LOCALES: &[IntlLocaleProfile] = &[
    IntlLocaleProfile {
        code: "en",
        language_name: "English",
        decimal_separator: '.',
        grouping_separator: ',',
        minus_sign: '-',
        percent_sign: '%',
        date_pattern: "MM/dd/yyyy",
        time_pattern: "HH:mm:ss",
        first_day_of_week: 7,
    },
    IntlLocaleProfile {
        code: "de",
        language_name: "German",
        decimal_separator: ',',
        grouping_separator: '.',
        minus_sign: '-',
        percent_sign: '%',
        date_pattern: "dd.MM.yyyy",
        time_pattern: "HH:mm:ss",
        first_day_of_week: 1,
    },
    IntlLocaleProfile {
        code: "fr",
        language_name: "French",
        decimal_separator: ',',
        grouping_separator: ' ',
        minus_sign: '-',
        percent_sign: '%',
        date_pattern: "dd/MM/yyyy",
        time_pattern: "HH:mm:ss",
        first_day_of_week: 1,
    },
    IntlLocaleProfile {
        code: "pt",
        language_name: "Portuguese",
        decimal_separator: ',',
        grouping_separator: '.',
        minus_sign: '-',
        percent_sign: '%',
        date_pattern: "dd/MM/yyyy",
        time_pattern: "HH:mm:ss",
        first_day_of_week: 1,
    },
    IntlLocaleProfile {
        code: "sv",
        language_name: "Swedish",
        decimal_separator: ',',
        grouping_separator: ' ',
        minus_sign: '-',
        percent_sign: '%',
        date_pattern: "yyyy-MM-dd",
        time_pattern: "HH:mm:ss",
        first_day_of_week: 1,
    },
];

#[inline]
pub fn is_supported_language(code: &str) -> bool {
    LANGUAGES.iter().any(|l| l.code.eq_ignore_ascii_case(code))
}

#[inline]
pub fn normalized_language_code(code: &str) -> &str {
    if is_supported_language(code) {
        code
    } else {
        DEFAULT_LANGUAGE_CODE
    }
}

#[inline]
fn base_language(code: &str) -> &str {
    code.split(['-', '_']).next().unwrap_or(DEFAULT_LANGUAGE_CODE)
}

#[inline]
pub fn is_supported_intl_locale(code: &str) -> bool {
    let base = base_language(code);
    INTL_LOCALES
        .iter()
        .any(|l| l.code.eq_ignore_ascii_case(base))
}

#[inline]
pub fn normalized_intl_locale(code: &str) -> &str {
    let base = base_language(code);
    if is_supported_intl_locale(base) {
        base
    } else {
        DEFAULT_INTL_LOCALE
    }
}

#[inline]
pub fn intl_locale_profile(code: &str) -> &'static IntlLocaleProfile {
    let normalized = normalized_intl_locale(code);
    INTL_LOCALES
        .iter()
        .find(|l| l.code.eq_ignore_ascii_case(normalized))
        .unwrap_or(&INTL_LOCALES[0])
}
