pub const DEFAULT_TIMEZONE_NAME: &str = "UTC";

#[inline]
pub fn current_language_code() -> &'static str {
    trueos_locale::DEFAULT_LANGUAGE_CODE
}

#[inline]
pub fn current_intl_locale_code() -> &'static str {
    current_intl_profile().code
}

#[inline]
pub fn current_intl_profile() -> &'static trueos_locale::IntlLocaleProfile {
    trueos_locale::intl_locale_profile(trueos_locale::DEFAULT_INTL_LOCALE)
}

#[inline]
pub fn current_timezone_name() -> &'static str {
    DEFAULT_TIMEZONE_NAME
}

#[inline]
pub fn env_var(key: &str) -> Option<&'static str> {
    match key {
        "LANG" | "LANGUAGE" | "TRUEOS_LANGUAGE" => Some(current_language_code()),
        "LC_ALL" | "LC_COLLATE" | "LC_CTYPE" | "LC_MESSAGES" | "LC_MONETARY" | "LC_NUMERIC"
        | "LC_TIME" | "TRUEOS_LOCALE" => Some(current_intl_locale_code()),
        "TZ" | "TRUEOS_TIMEZONE" => Some(current_timezone_name()),
        _ => None,
    }
}
