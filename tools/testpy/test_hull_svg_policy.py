#!/usr/bin/env python3
"""Exercise production SVG recognition and conversion URLs without network/boot."""
from pathlib import Path
import json
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def production_function(source: str, name: str) -> str:
    match = re.search(rf"^(?:pub(?:\([^)]*\))? )?fn {name}\b", source, re.MULTILINE)
    if match is None:
        raise ValueError(f"missing production function {name}")
    end = re.search(r"^}\s*\n", source[match.end():], re.MULTILINE)
    if end is None:
        raise ValueError(f"missing closing brace for {name}")
    return source[match.start():match.end() + end.end()]


TESTS = r'''
#[cfg(test)]mod tests {
    use super::hull_svg_policy::{svg_url,svg_document,conversion_url};
    use super::hull_image_policy as image;
    #[test]fn svg_extension_checks_only_the_path_case_insensitively() {
        for url in ["https://example.test/logo.svg","https://example.test/logo.SVG?version=2#part","https://example.test/logo.SvGz?format=png","//example.test/a.svg","/icons/a.svg","a.svg#view"] {
            assert!(svg_url(url),"not recognized: {url}");
        }
        for url in ["https://example.svg","https://example.svg?file=a.svg","//example.svg","https://example.test/a.png?file=logo.svg","https://example.test/a.png#logo.svg","https://example.test/a.svg/more","https://example.test/a.svg.png","https://example.test/a.sv","svg"] {
            assert!(!svg_url(url),"wrongly recognized: {url}");
        }
        assert!(!svg_url(&format!("https://example.test/{}.svg","x".repeat(image::MAX_URL_BYTES))));
    }
    #[test]fn svg_document_accepts_common_xml_prologs_and_actual_svg_root() {
        for bytes in [
            &b"<svg/>"[..],&b" \t\r\n<svg width='1' height='1'></svg>"[..],
            &b"\xef\xbb\xbf<?xml version='1.0' encoding='UTF-8'?>\n<!-- icon -->\n<svg xmlns='http://www.w3.org/2000/svg'/>"[..],
            &b"<?xml version='1.0'?><!DOCTYPE svg PUBLIC '-//W3C//DTD SVG 1.1//EN' 'https://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd'><svg/>"[..],
            &b"<!DOCTYPE svg [<!ENTITY marker 'quoted > and [ text'><!ENTITY other \"x>y\">]><!-- next --><svg/>"[..],
            &b"<!DOCTYPE svg [<!-- [ and > ignored --><!ENTITY x 'value'>]><svg/>"[..],
        ] {assert!(svg_document(bytes),"not recognized: {}",String::from_utf8_lossy(bytes));}
    }
    #[test]fn svg_document_does_not_search_through_html_or_an_unfinished_prolog() {
        for bytes in [
            &b""[..],&b"<html><body><svg/></body></html>"[..],&b"<!DOCTYPE html><html><svg/></html>"[..],
            &b"<script>const text='<svg/>'</script>"[..],&b"garbage<svg/>"[..],&b"<svgscript/>"[..],
            &b"<SVG/>"[..],&b"<!-- <svg/>"[..],&b"<?xml version='1.0'<svg/>"[..],
            &b"<!DOCTYPE svg [<!ENTITY x 'value'><svg/>"[..],&b"<!DOCTYPE svg ]><svg/>"[..],
            &b"<?xml-stylesheet href='theme.css'?><svg/>"[..],&b"\x89PNG\r\n\x1a\n<svg/>"[..],
        ] {assert!(!svg_document(bytes),"wrongly recognized: {}",String::from_utf8_lossy(bytes));}
    }
    #[test]fn svg_document_prefix_work_is_bounded_even_for_unclosed_comments() {
        let mut bytes=b"<!--".to_vec();bytes.extend_from_slice(&vec![b'x';32*1024]);bytes.extend_from_slice(b"--><svg/>");
        assert!(!svg_document(&bytes));
        // The source body after the root marker is outside recognition's job.
        let mut bytes=b"<svg>".to_vec();bytes.extend_from_slice(&vec![b'x';64*1024]);
        assert!(svg_document(&bytes));
    }
    #[test]fn conversion_url_encodes_source_query_and_unicode_without_parameter_injection() {
        let url=conversion_url("https://EXAMPLE.test:443/assets/\u{00fc}ber.svg?x=1&output=webp#ignored",300,150).unwrap();
        assert_eq!(url,"https://wsrv.nl/?url=https%3A%2F%2Fexample.test%2Fassets%2F%C3%BCber.svg%3Fx%3D1%26output%3Dwebp&output=png&w=300&h=150&fit=inside&n=1");
        assert!(!url.contains("ignored"));
        assert_ne!(url,conversion_url("https://example.test/assets/\u{00fc}ber.svg?x=2&output=webp",300,150).unwrap());
        assert_ne!(url,conversion_url("https://example.test/assets/\u{00fc}ber.svg?x=1&output=webp",301,150).unwrap());
    }
    #[test]fn conversion_url_uses_existing_target_limits_and_preserves_reserved_query_bytes() {
        let (width,height,_)=image::target_extent(8192,4096).unwrap();
        let url=conversion_url("https://example.test/a.svg?sig=a%2Fb+~_.-&n=3",8192,4096).unwrap();
        assert_eq!(url,format!("https://wsrv.nl/?url=https%3A%2F%2Fexample.test%2Fa.svg%3Fsig%3Da%252Fb%2B~_.-%26n%3D3&output=png&w={width}&h={height}&fit=inside&n=1"));
        assert_eq!(conversion_url("https://example.test/a.svg",0,10),Err(image::ERR_INVALID));
        assert_eq!(conversion_url("https://example.test/a.svg",10,0),Err(image::ERR_INVALID));
    }
    #[test]fn conversion_url_rejects_local_non_https_credentials_controls_and_oversize() {
        for source in ["http://127.0.0.1:50000/a.svg","http://example.test/a.svg","https://user@example.test/a.svg","file:///a.svg","https://example.test/a\r\nBad:yes.svg"] {
            assert!(conversion_url(source,32,32).is_err(),"accepted {source}");
        }
        // The input itself fits, but escaping it would exceed the GET URL cap.
        let source=format!("https://example.test/a.svg?value={}","%".repeat(2000));
        assert!(source.len()<image::MAX_URL_BYTES);
        assert_eq!(conversion_url(&source,32,32),Err(image::ERR_TOO_LARGE));
    }
}
'''


def main() -> None:
    source = (ROOT / "src/r/services/hull_image_policy.rs").read_text()
    constants = "\n".join(line for line in source.splitlines() if any(
        line.startswith(f"pub(super) const {name}:") for name in (
            "MAX_URL_BYTES", "MAX_TARGET_DIMENSION", "MAX_TARGET_BYTES",
            "ERR_INVALID", "ERR_TOO_LARGE", "ERR_UNSUPPORTED",
        )
    ))
    policy = "\n".join(production_function(source, name)
                       for name in ("canonical_url", "target_extent"))
    module = json.dumps(str(ROOT / "src/r/services/hull_svg_policy.rs"))
    harness = ("extern crate alloc;\nmod services {\nmod hull_image_policy {\n"
               "use alloc::{format,string::String};\n" + constants + "\n" + policy
               + "\n}\n#[path=" + module + "] mod hull_svg_policy;\n" + TESTS + "\n}\n")
    with tempfile.TemporaryDirectory(prefix="hull-svg-policy-") as directory:
        path = Path(directory)
        (path / "test.rs").write_text(harness)
        subprocess.run(["rustc", "--edition=2024", "--test", str(path / "test.rs"),
                        "-o", str(path / "test")], check=True)
        subprocess.run([str(path / "test")], check=True)


if __name__ == "__main__":
    main()
