pub(crate) trait SupportedEncodings: Copy {
    fn gzip(&self) -> bool;
    fn deflate(&self) -> bool;
    fn br(&self) -> bool;
    fn zstd(&self) -> bool;
}

// This enum's variants are ordered from least to most preferred.
#[derive(Copy, Clone, Debug, Ord, PartialOrd, PartialEq, Eq)]
pub(crate) enum Encoding {
    #[allow(dead_code)]
    Identity,
    #[cfg(any(feature = "fs", feature = "compression-deflate"))]
    Deflate,
    #[cfg(any(feature = "fs", feature = "compression-gzip"))]
    Gzip,
    #[cfg(any(feature = "fs", feature = "compression-br"))]
    Brotli,
    #[cfg(any(feature = "fs", feature = "compression-zstd"))]
    Zstd,
}

impl Encoding {
    #[allow(dead_code)]
    fn to_str(self) -> &'static str {
        match self {
            #[cfg(any(feature = "fs", feature = "compression-gzip"))]
            Encoding::Gzip => "gzip",
            #[cfg(any(feature = "fs", feature = "compression-deflate"))]
            Encoding::Deflate => "deflate",
            #[cfg(any(feature = "fs", feature = "compression-br"))]
            Encoding::Brotli => "br",
            #[cfg(any(feature = "fs", feature = "compression-zstd"))]
            Encoding::Zstd => "zstd",
            Encoding::Identity => "identity",
        }
    }

    #[cfg(all(feature = "fs", any(target_os = "trueos", target_os = "zkvm")))]
    pub(crate) fn to_file_extension(self) -> Option<&'static tokio::ffi::OsStr> {
        match self {
            Encoding::Gzip => Some(".gz"),
            Encoding::Deflate => Some(".zz"),
            Encoding::Brotli => Some(".br"),
            Encoding::Zstd => Some(".zst"),
            Encoding::Identity => None,
        }
    }

    #[cfg(all(feature = "fs", not(any(target_os = "trueos", target_os = "zkvm"))))]
    pub(crate) fn to_file_extension(self) -> Option<&'static core::ffi::OsStr> {
        match self {
            Encoding::Gzip => Some(core::ffi::OsStr::new(".gz")),
            Encoding::Deflate => Some(core::ffi::OsStr::new(".zz")),
            Encoding::Brotli => Some(core::ffi::OsStr::new(".br")),
            Encoding::Zstd => Some(core::ffi::OsStr::new(".zst")),
            Encoding::Identity => None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn into_header_value(self) -> http::HeaderValue {
        http::HeaderValue::from_static(self.to_str())
    }

    #[cfg(any(
        feature = "compression-gzip",
        feature = "compression-br",
        feature = "compression-deflate",
        feature = "compression-zstd",
        feature = "fs",
    ))]
    fn parse(s: &str, _supported_encoding: impl SupportedEncodings) -> Option<Encoding> {
        #[cfg(any(feature = "fs", feature = "compression-gzip"))]
        if (s.eq_ignore_ascii_case("gzip") || s.eq_ignore_ascii_case("x-gzip"))
            && _supported_encoding.gzip()
        {
            return Some(Encoding::Gzip);
        }

        #[cfg(any(feature = "fs", feature = "compression-deflate"))]
        if s.eq_ignore_ascii_case("deflate") && _supported_encoding.deflate() {
            return Some(Encoding::Deflate);
        }

        #[cfg(any(feature = "fs", feature = "compression-br"))]
        if s.eq_ignore_ascii_case("br") && _supported_encoding.br() {
            return Some(Encoding::Brotli);
        }

        #[cfg(any(feature = "fs", feature = "compression-zstd"))]
        if s.eq_ignore_ascii_case("zstd") && _supported_encoding.zstd() {
            return Some(Encoding::Zstd);
        }

        if s.eq_ignore_ascii_case("identity") {
            return Some(Encoding::Identity);
        }

        None
    }

    #[cfg(any(
        feature = "compression-gzip",
        feature = "compression-br",
        feature = "compression-zstd",
        feature = "compression-deflate",
    ))]
    // based on https://github.com/http-rs/accept-encoding
    pub(crate) fn from_headers(
        headers: &http::HeaderMap,
        supported_encoding: impl SupportedEncodings,
    ) -> Self {
        Encoding::preferred_encoding(encodings(headers, supported_encoding))
            .unwrap_or(Encoding::Identity)
    }

    #[cfg(any(
        feature = "compression-gzip",
        feature = "compression-br",
        feature = "compression-zstd",
        feature = "compression-deflate",
        feature = "fs",
    ))]
    pub(crate) fn preferred_encoding(
        accepted_encodings: impl Iterator<Item = (Encoding, QValue)>,
    ) -> Option<Self> {
        accepted_encodings
            .filter(|(_, qvalue)| qvalue.0 > 0)
            .max_by_key(|&(encoding, qvalue)| (qvalue, encoding))
            .map(|(encoding, _)| encoding)
    }
}

// Allowed q-values are numbers between 0 and 1 with at most 3 digits in the fractional part. They
// are presented here as an unsigned integer between 0 and 1000.
#[cfg(any(
    feature = "compression-gzip",
    feature = "compression-br",
    feature = "compression-zstd",
    feature = "compression-deflate",
    feature = "fs",
))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct QValue(u16);

#[cfg(any(
    feature = "compression-gzip",
    feature = "compression-br",
    feature = "compression-zstd",
    feature = "compression-deflate",
    feature = "fs",
))]
impl QValue {
    #[inline]
    pub(crate) fn one() -> Self {
        Self(1000)
    }

    // Parse a q-value as specified in RFC 7231 section 5.3.1.
    fn parse(s: &str) -> Option<Self> {
        let mut c = s.chars();
        // Parse "q=" (case-insensitively).
        match c.next() {
            Some('q' | 'Q') => (),
            _ => return None,
        };
        match c.next() {
            Some('=') => (),
            _ => return None,
        };

        // Parse leading digit. Since valid q-values are between 0.000 and 1.000, only "0" and "1"
        // are allowed.
        let mut value = match c.next() {
            Some('0') => 0,
            Some('1') => 1000,
            _ => return None,
        };

        // Parse optional decimal point.
        match c.next() {
            Some('.') => (),
            None => return Some(Self(value)),
            _ => return None,
        };

        // Parse optional fractional digits. The value of each digit is multiplied by `factor`.
        // Since the q-value is represented as an integer between 0 and 1000, `factor` is `100` for
        // the first digit, `10` for the next, and `1` for the digit after that.
        let mut factor = 100;
        loop {
            match c.next() {
                Some(n @ '0'..='9') => {
                    // If `factor` is less than `1`, three digits have already been parsed. A
                    // q-value having more than 3 fractional digits is invalid.
                    if factor < 1 {
                        return None;
                    }
                    // Add the digit's value multiplied by `factor` to `value`.
                    value += factor * (n as u16 - '0' as u16);
                }
                None => {
                    // No more characters to parse. Check that the value representing the q-value is
                    // in the valid range.
                    return if value <= 1000 {
                        Some(Self(value))
                    } else {
                        None
                    };
                }
                _ => return None,
            };
            factor /= 10;
        }
    }
}

#[cfg(any(
    feature = "compression-gzip",
    feature = "compression-br",
    feature = "compression-zstd",
    feature = "compression-deflate",
    feature = "fs",
))]
// based on https://github.com/http-rs/accept-encoding
pub(crate) fn encodings<'a>(
    headers: &'a http::HeaderMap,
    supported_encoding: impl SupportedEncodings + 'a,
) -> impl Iterator<Item = (Encoding, QValue)> + 'a {
    headers
        .get_all(http::header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|hval| hval.to_str().ok())
        .flat_map(|s| s.split(','))
        .filter_map(move |v| {
            let mut v = v.splitn(2, ';');

            let encoding = match Encoding::parse(v.next().unwrap().trim(), supported_encoding) {
                Some(encoding) => encoding,
                None => return None, // ignore unknown encodings
            };

            let qval = if let Some(qval) = v.next() {
                QValue::parse(qval.trim())?
            } else {
                QValue::one()
            };

            Some((encoding, qval))
        })
}
