#[derive(Copy, Clone, Debug)]
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
}

pub const LANGUAGES: &[Language] = &[
    Language { code: "sq", name: "Albanian" },
    Language { code: "af", name: "Afrikaans" },
    Language { code: "ar", name: "Arabic" },
    Language { code: "az", name: "Azerbaijani" },
    Language { code: "eu", name: "Basque" },
    Language { code: "be", name: "Belarusian" },
    Language { code: "bg", name: "Bulgarian" },
    Language { code: "ca", name: "Catalan" },
    Language { code: "zh_cn", name: "Chinese Simplified" },
    Language { code: "zh_tw", name: "Chinese Traditional" },
    Language { code: "hr", name: "Croatian" },
    Language { code: "cz", name: "Czech" },
    Language { code: "da", name: "Danish" },
    Language { code: "nl", name: "Dutch" },
    Language { code: "en", name: "English" },
    Language { code: "fi", name: "Finnish" },
    Language { code: "fr", name: "French" },
    Language { code: "gl", name: "Galician" },
    Language { code: "de", name: "German" },
    Language { code: "el", name: "Greek" },
    Language { code: "he", name: "Hebrew" },
    Language { code: "hi", name: "Hindi" },
    Language { code: "hu", name: "Hungarian" },
    Language { code: "is", name: "Icelandic" },
    Language { code: "id", name: "Indonesian" },
    Language { code: "it", name: "Italian" },
    Language { code: "ja", name: "Japanese" },
    Language { code: "kr", name: "Korean" },
    Language { code: "ku", name: "Kurmanji (Kurdish)" },
    Language { code: "la", name: "Latvian" },
    Language { code: "lt", name: "Lithuanian" },
    Language { code: "mk", name: "Macedonian" },
    Language { code: "no", name: "Norwegian" },
    Language { code: "fa", name: "Persian (Farsi)" },
    Language { code: "pl", name: "Polish" },
    Language { code: "pt", name: "Portuguese" },
    Language { code: "pt_br", name: "Português Brasil" },
    Language { code: "ro", name: "Romanian" },
    Language { code: "ru", name: "Russian" },
    Language { code: "sr", name: "Serbian" },
    Language { code: "sk", name: "Slovak" },
    Language { code: "sl", name: "Slovenian" },
    Language { code: "es", name: "Spanish" },
    Language { code: "sv", name: "Swedish" },
    Language { code: "th", name: "Thai" },
    Language { code: "tr", name: "Turkish" },
    Language { code: "uk", name: "Ukrainian" },
    Language { code: "vi", name: "Vietnamese" },
    Language { code: "zu", name: "Zulu" },
];

pub const DEFAULT_LANGUAGE_CODE: &str = "en";
pub const DEFAULT_GERMAN_LANGUAGE_CODE: &str = "de";

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
