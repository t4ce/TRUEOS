//! Bounded, structured evidence for native HTTPS GETs. This never changes a
//! request or retry decision, and never includes cookies or authorization.
extern crate alloc;

use alloc::{format, string::String};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn clipped(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

pub(super) fn public_url(value: &str) -> String {
    let value = value.split(['?', '#']).next().unwrap_or("");
    let clean = match value.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
            let host = authority.rsplit('@').next().unwrap_or("");
            format!("{scheme}://{host}/{path}")
        }
        None => String::from(value),
    };
    clipped(&clean, 512)
}

fn header_value<'a>(raw: &'a [u8], name: &str) -> Option<&'a str> {
    let headers = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|end| &raw[..end])
        .unwrap_or(raw);
    let text = core::str::from_utf8(headers).ok()?;
    text.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then_some(value.trim())
    })
}

fn selected_header(raw: &[u8], name: &str) -> Option<String> {
    header_value(raw, name).map(|value| clipped(value, 256))
}

pub(super) fn response(
    url: &str,
    method: &str,
    user_agent: Option<&str>,
    peer: &str,
    raw: &[u8],
    status: u16,
    body: &[u8],
    elapsed_ms: u64,
    handshake_ms: Option<u64>,
    first_data_ms: Option<u64>,
) -> Value {
    let hash = Sha256::digest(body);
    let mut body_sha256 = String::with_capacity(64);
    for byte in hash {
        body_sha256.push_str(&format!("{byte:02x}"));
    }
    // Error text explains denials and rate limits. Successful assets remain
    // binary fingerprints; redirects expose only the sanitized destination.
    let preview = if status >= 400 {
        Some(String::from_utf8_lossy(&body[..body.len().min(1024)]).into_owned())
    } else {
        None
    };
    json!({
        "schema":"download_http_v1", "event":"response", "transport":"native-rustls",
        "url":public_url(url), "method":method, "user_agent":user_agent,
        "peer":peer, "status":status, "body_bytes":body.len(),
        "body_sha256":body_sha256, "body_preview":preview,
        "elapsed_ms":elapsed_ms, "handshake_ms":handshake_ms, "first_data_ms":first_data_ms,
        "headers": {
            "server":selected_header(raw,"server"), "via":selected_header(raw,"via"),
            "content_type":selected_header(raw,"content-type"),
            "content_encoding":selected_header(raw,"content-encoding"),
            "location":header_value(raw,"location").map(public_url).map(|s|clipped(&s,256)),
            "retry_after":selected_header(raw,"retry-after"),
            "x_cache":selected_header(raw,"x-cache"),
            "x_cache_status":selected_header(raw,"x-cache-status"),
            "x_request_id":selected_header(raw,"x-request-id"),
            "x_client_ip":selected_header(raw,"x-client-ip"),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_omit_credentials_queries_and_fragments() {
        assert_eq!(
            public_url("https://user:secret@example.test/a.png?token=secret#x"),
            "https://example.test/a.png"
        );
        assert_eq!(public_url("/a.png?secret"), "/a.png");
        assert_eq!(public_url("https://example.test"), "https://example.test/");
    }

    #[test]
    fn denial_retains_evidence_without_sensitive_headers() {
        let raw = b"HTTP/1.1 403 Forbidden\r\nServer: envoy\r\nRetry-After: 60\r\nX-Request-Id: rid\r\nLocation: https://secret:pw@next.test/a?token=bad\r\nSet-Cookie: private\r\nAuthorization: secret\r\n\r\n";
        let r = response(
            "https://example.test/a?token=bad",
            "GET",
            Some("TRUEOS"),
            "1.2.3.4",
            raw,
            403,
            b"robot policy\n",
            500,
            Some(200),
            Some(400),
        );
        assert_eq!(r["body_preview"], "robot policy\n");
        assert_eq!(r["headers"]["retry_after"], "60");
        assert_eq!(r["headers"]["location"], "https://next.test/a");
        assert_eq!(r["handshake_ms"], 200);
        assert_eq!(r["body_sha256"].as_str().unwrap().len(), 64);
        let serialized = r.to_string();
        for private in ["private", "Authorization", "pw@", "token=bad"] {
            assert!(!serialized.contains(private));
        }
    }

    #[test]
    fn redirect_credentials_are_removed_before_clipping_and_urls_remain_bounded() {
        let credentials = "supersecret".repeat(30);
        let raw = format!(
            "HTTP/1.1 302 Found\r\nLocation: https://{credentials}@next.test/image.png?token=hidden\r\n\r\n"
        );
        let record = response(
            "https://example.test/", "GET", None, "1.2.3.4", raw.as_bytes(),
            302, &[], 0, None, None,
        );
        assert_eq!(record["headers"]["location"], "https://next.test/image.png");
        assert!(!record.to_string().contains("supersecret"));

        let path = "segment/".repeat(100);
        let raw = format!(
            "HTTP/1.1 302 Found\r\nLocation: https://{credentials}@next.test/{path}?token=hidden#fragment\r\n\r\n"
        );
        let record = response(
            "https://example.test/", "GET", None, "1.2.3.4", raw.as_bytes(),
            302, &[], 0, None, None,
        );
        let location = record["headers"]["location"].as_str().unwrap();
        assert_eq!(location.chars().count(), 256);
        assert!(location.starts_with("https://next.test/segment/"));
        for secret in ["supersecret", "token=hidden", "fragment"] {
            assert!(!record.to_string().contains(secret));
        }
    }

    #[test]
    fn previews_and_headers_are_bounded_and_success_has_no_body_preview() {
        let body = [b'x'; 4096];
        let r = response(
            "https://example.test/",
            "GET",
            None,
            "preconnected",
            b"HTTP/1.1 429 Too Many Requests\r\n\r\n",
            429,
            &body,
            4,
            None,
            None,
        );
        assert_eq!(r["body_bytes"], 4096);
        assert_eq!(r["body_preview"].as_str().unwrap().len(), 1024);
        let success = response(
            "https://example.test/",
            "GET",
            None,
            "preconnected",
            b"HTTP/1.1 200 OK\r\n\r\n",
            200,
            &body,
            4,
            None,
            None,
        );
        assert!(success["body_preview"].is_null());
        assert_eq!(success["body_sha256"], r["body_sha256"]);
    }
}
