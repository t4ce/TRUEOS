use alloc::string::String;

const ALLOWED_SUFFIXES: [&str; 8] = [".de", ".eu", ".com", ".fr", ".co.uk", ".io", ".net", ".it"];

pub(crate) fn try_parse(line: &str) -> Option<String> {
    let candidate = line.trim();
    if candidate.is_empty() || candidate.split_whitespace().nth(1).is_some() {
        return None;
    }

    if !is_domain_chars_only(candidate) {
        return None;
    }

    let lowered = candidate.to_ascii_lowercase();
    if !ALLOWED_SUFFIXES.iter().any(|suffix| lowered.ends_with(suffix)) {
        return None;
    }

    Some(prepare_url(candidate))
}

pub(crate) fn prepare_call_with_url(_url: &str) {
    // URL handoff is intentionally scoped here for now.
}

fn prepare_url(host: &str) -> String {
    let mut url = String::from("https://");
    url.push_str(host);
    url
}

fn is_domain_chars_only(s: &str) -> bool {
    let mut saw_dot = false;
    let mut prev_dot = false;

    for ch in s.chars() {
        let ok = ch.is_ascii_alphanumeric() || ch == '-' || ch == '.';
        if !ok {
            return false;
        }

        if ch == '.' {
            if prev_dot {
                return false;
            }
            saw_dot = true;
            prev_dot = true;
        } else {
            prev_dot = false;
        }
    }

    saw_dot && !s.starts_with('.') && !s.ends_with('.')
}
