//! Optional local Wi-Fi provisioning for private builds.
//!
//! CI never reads a credential source. The generated binary is empty unless a
//! local two-line SSID/password file passes validation; diagnostics omit values.

use std::{
    env,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

const DEFAULT_SOURCE: &str = "/home/t4ce/Downloads/pw.txt";
const OUTPUT: &str = "wifi-boot.bin";

pub fn generate(out_dir: &Path) -> io::Result<()> {
    generate_for_environment(out_dir, |name| env::var_os(name))
}

fn generate_for_environment(
    out_dir: &Path,
    environment: impl Fn(&str) -> Option<OsString>,
) -> io::Result<()> {
    println!("cargo:rerun-if-changed=tools/build/wifi_credentials.rs");
    for variable in ["TRUEOS_WIFI_CREDENTIALS", "CI", "GITHUB_ACTIONS"] {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    // Clear an earlier private output before any parse/read error or early
    // return. A public build must never inherit a previous local payload.
    write_output(out_dir, &[])?;
    if environment("CI").is_some() || environment("GITHUB_ACTIONS").is_some() {
        return Ok(());
    }
    let source = match environment("TRUEOS_WIFI_CREDENTIALS") {
        Some(path) if path.is_empty() => return Ok(()),
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(DEFAULT_SOURCE),
    };
    println!("cargo:rerun-if-changed={}", source.display());
    let input = match fs::read(&source) {
        Ok(input) => input,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                "cannot read local Wi-Fi credential file",
            ));
        }
    };
    let payload = encode(&input)?;
    write_output(out_dir, &payload)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn encode(input: &[u8]) -> io::Result<Vec<u8>> {
    let text =
        std::str::from_utf8(input).map_err(|_| invalid("Wi-Fi credential file must be UTF-8"))?;
    // Only remove line endings: spaces and tabs are part of both values.
    let body = if let Some(body) = text.strip_suffix('\n') {
        body.strip_suffix('\r').unwrap_or(body)
    } else {
        text
    };
    let (ssid, password) = body
        .split_once('\n')
        .ok_or_else(|| invalid("Wi-Fi credential file must contain exactly two lines"))?;
    let ssid = ssid.strip_suffix('\r').unwrap_or(ssid);
    if ssid
        .bytes()
        .chain(password.bytes())
        .any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
    {
        return Err(invalid(
            "Wi-Fi credential file must contain exactly two text lines",
        ));
    }
    if ssid.is_empty() || ssid.len() > 32 {
        return Err(invalid("Wi-Fi SSID must contain 1-32 UTF-8 bytes"));
    }
    if !(8..=63).contains(&password.len())
        && !(password.len() == 64 && password.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(invalid(
            "Wi-Fi password must contain 8-63 bytes or 64 hexadecimal digits",
        ));
    }
    let mut payload = Vec::with_capacity(6 + ssid.len() + password.len());
    payload.extend_from_slice(b"TW01");
    payload.extend_from_slice(&[ssid.len() as u8, password.len() as u8]);
    payload.extend_from_slice(ssid.as_bytes());
    payload.extend_from_slice(password.as_bytes());
    Ok(payload)
}

fn write_output(out_dir: &Path, bytes: &[u8]) -> io::Result<()> {
    fs::create_dir_all(out_dir)?;
    let output = out_dir.join(OUTPUT);
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&output)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // OpenOptions::mode only applies when creating a new file.
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
            let directory = env::temp_dir().join(format!(
                "trueos-wifi-build-test-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir_all(&directory).unwrap();
            Self(directory)
        }
        fn output(&self) -> Vec<u8> {
            fs::read(self.0.join(OUTPUT)).unwrap()
        }
        fn source(&self) -> PathBuf {
            self.0.join("synthetic-credentials.txt")
        }
        fn run(&self, variables: &[(&str, OsString)]) -> io::Result<()> {
            generate_for_environment(&self.0, |name| {
                variables
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| value.clone())
            })
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn wire_payload_preserves_whitespace_and_accepts_lf_crlf_or_no_final_newline() {
        let expected = b"TW01\x05\x0b ssidpassword \t ";
        for input in [
            &b" ssid\npassword \t "[..],
            &b" ssid\npassword \t \n"[..],
            &b" ssid\r\npassword \t \r\n"[..],
        ] {
            assert_eq!(encode(input).unwrap(), expected);
        }
    }

    #[test]
    fn utf8_byte_bounds_and_raw_hex_psk_match_wire_limits() {
        let ssid = "\u{00e9}".repeat(16);
        let password = "\u{00e4}".repeat(4);
        let payload = encode(format!("{ssid}\n{password}").as_bytes()).unwrap();
        assert_eq!(&payload[..6], b"TW01\x20\x08");
        let hex = "aB01".repeat(16);
        assert_eq!(encode(format!("ssid\n{hex}\n").as_bytes()).unwrap()[5], 64);
        assert!(encode(format!("ssid\n{}", "z".repeat(64)).as_bytes()).is_err());
        assert!(encode(format!("{}x\npassword", ssid).as_bytes()).is_err());
    }

    #[test]
    fn malformed_credentials_return_value_free_errors() {
        for input in [
            &b""[..],
            &b"only-one-line"[..],
            &b"\npassword"[..],
            &b"synthetic-ssid\nshort"[..],
            &b"ssid\npassword\nthird"[..],
            &b"ssid\npassword\n\n"[..],
            &b"ssid\npassword\0"[..],
            &b"ssid\npassword\r"[..],
            &b"ssid\npassword\xff"[..],
        ] {
            let error = encode(input).unwrap_err().to_string();
            assert!(!error.contains("synthetic-ssid"));
            assert!(!error.contains("only-one-line"));
            assert!(!error.contains("third"));
        }
    }

    #[test]
    fn missing_or_disabled_source_overwrites_stale_private_output() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join(OUTPUT), b"stale-private-output").unwrap();
        fixture
            .run(&[("TRUEOS_WIFI_CREDENTIALS", fixture.source().into_os_string())])
            .unwrap();
        assert!(fixture.output().is_empty());
        fs::write(fixture.0.join(OUTPUT), b"stale-private-output").unwrap();
        fixture
            .run(&[("TRUEOS_WIFI_CREDENTIALS", OsString::new())])
            .unwrap();
        assert!(fixture.output().is_empty());
    }

    #[test]
    fn ci_and_github_actions_disable_before_resolving_any_source() {
        for variable in ["CI", "GITHUB_ACTIONS"] {
            let fixture = Fixture::new();
            fs::write(fixture.0.join(OUTPUT), b"stale-private-output").unwrap();
            generate_for_environment(&fixture.0, |name| match name {
                "CI" | "GITHUB_ACTIONS" => (name == variable).then(OsString::new),
                "TRUEOS_WIFI_CREDENTIALS" => panic!("CI must not inspect the credential source"),
                _ => None,
            })
            .unwrap();
            assert!(fixture.output().is_empty());
        }
    }

    #[test]
    fn valid_local_source_generates_payload_and_parse_failure_clears_it() {
        let fixture = Fixture::new();
        let source = fixture.source();
        fs::write(&source, b"synthetic-ssid\nsynthetic-password\n").unwrap();
        let variables = [("TRUEOS_WIFI_CREDENTIALS", source.clone().into_os_string())];
        fixture.run(&variables).unwrap();
        assert_eq!(
            fixture.output(),
            encode(b"synthetic-ssid\nsynthetic-password\n").unwrap()
        );
        fs::write(&source, b"malformed").unwrap();
        assert!(fixture.run(&variables).is_err());
        assert!(fixture.output().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn generated_file_permissions_are_owner_only_even_for_existing_output() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = Fixture::new();
        let output = fixture.0.join(OUTPUT);
        fs::write(&output, b"old").unwrap();
        fs::set_permissions(&output, fs::Permissions::from_mode(0o666)).unwrap();
        write_output(&fixture.0, b"synthetic-payload").unwrap();
        assert_eq!(
            fs::metadata(output).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
