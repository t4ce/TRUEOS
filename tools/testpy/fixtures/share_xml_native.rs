use trueos_io::{BufRead, Cursor, Error, ErrorKind, Read, Write};
use xml::{EventReader, ParserConfig};
use xml::reader::{ErrorKind as XmlErrorKind, XmlEvent};

struct Chunks<'a> { bytes: &'a [u8], interrupted: bool }
impl Read for Chunks<'_> {
    fn read(&mut self, out: &mut [u8]) -> trueos_io::Result<usize> {
        if !self.interrupted { self.interrupted = true; return Err(ErrorKind::Interrupted.into()); }
        let n = out.len().min(self.bytes.len()).min(1);
        out[..n].copy_from_slice(&self.bytes[..n]); self.bytes = &self.bytes[n..]; Ok(n)
    }
}
impl BufRead for Chunks<'_> {
    fn fill_buf(&mut self) -> trueos_io::Result<&[u8]> { Ok(&self.bytes[..self.bytes.len().min(1)]) }
    fn consume(&mut self, n: usize) { self.bytes = &self.bytes[n..]; }
}
#[derive(Default)]
struct ShortWriter { bytes: Vec<u8>, interrupted: bool }
impl Write for ShortWriter {
    fn write(&mut self, data: &[u8]) -> trueos_io::Result<usize> {
        if !self.interrupted { self.interrupted = true; return Err(ErrorKind::Interrupted.into()); }
        let n = data.len().min(1); self.bytes.extend_from_slice(&data[..n]); Ok(n)
    }
    fn flush(&mut self) -> trueos_io::Result<()> { Ok(()) }
}
fn events(bytes: &[u8], config: ParserConfig) -> Vec<XmlEvent> {
    config.create_reader(Chunks { bytes, interrupted: false }).into_iter().collect::<Result<Vec<_>,_>>().unwrap()
}
fn text(events: &[XmlEvent]) -> String {
    events.iter().filter_map(|e| if let XmlEvent::Characters(s) = e {Some(s.as_str())} else {None}).collect()
}
#[test]
fn xml_utf8_one_byte_reads_retry_interrupted_and_expand_entities() {
    let doc = "<r>&amp;&custom;&#x1F680;ä</r>";
    let actual = events(doc.as_bytes(), ParserConfig::new().add_entity("custom", "Ω"));
    assert_eq!(text(&actual), "&Ω🚀ä");
}
#[test]
fn xml_utf16_both_endiannesses_and_surrogate_pairs() {
    for little in [false, true] {
        let mut bytes = if little { vec![0xff,0xfe] } else { vec![0xfe,0xff] };
        for unit in "<r>é🚀</r>".encode_utf16() {
            bytes.extend_from_slice(&if little {unit.to_le_bytes()} else {unit.to_be_bytes()});
        }
        assert_eq!(text(&events(&bytes, ParserConfig::new())), "é🚀");
    }
}
#[test]
fn xml_malformed_utf16_remains_invalid_data_and_utf8_rejected() {
    let mut bytes = vec![0xff,0xfe];
    for unit in [b'<' as u16,b'r' as u16,b'>' as u16,0xd800,b'<' as u16] {bytes.extend_from_slice(&unit.to_le_bytes());}
    let error = EventReader::new(bytes.as_slice()).into_iter().find_map(Result::err).unwrap();
    assert!(matches!(error.kind(), XmlErrorKind::Io(e) if e.kind()==ErrorKind::InvalidData));
    assert!(EventReader::new(&b"<r>\xff</r>"[..]).into_iter().any(|r|r.is_err()));
}
#[test]
fn xml_namespace_shadowing_restores_parent_binding() {
    let actual = events(br#"<r xmlns:p="urn:one"><p:a/><s xmlns:p="urn:two"><p:b/></s><p:c/></r>"#, ParserConfig::new());
    let names: Vec<_> = actual.iter().filter_map(|e|match e {XmlEvent::StartElement{name,..} if name.prefix.is_some()=>Some((name.local_name.as_str(),name.namespace.as_deref())),_=>None}).collect();
    assert_eq!(names,[("a",Some("urn:one")),("b",Some("urn:two")),("c",Some("urn:one"))]);
}
#[test]
fn xml_duplicate_expanded_attributes_rejected_on_large_set_path() {
    let doc = br#"<r xmlns:a="urn:x" xmlns:b="urn:x" n0="" n1="" n2="" n3="" n4="" n5="" n6="" a:z="1" b:z="2"/>"#;
    let error = EventReader::new(&doc[..]).into_iter().find_map(Result::err).unwrap();
    assert!(error.to_string().contains("redefined") || error.to_string().contains("Redefined"),"{error}");
}
#[test]
fn xml_entity_expansion_limit_and_configured_scalar_recovery_retained() {
    let config = ParserConfig::new().max_entity_expansion_length(3);
    let nested = br#"<!DOCTYPE r [<!ENTITY large "&leaf;&leaf;"><!ENTITY leaf "abcdefgh">]><r>&large;</r>"#;
    assert!(config.create_reader(&nested[..]).into_iter().any(|e|e.is_err()));
    assert!(EventReader::new(&b"<r>&#0;</r>"[..]).into_iter().any(|e|e.is_err()));
    assert_eq!(text(&events(b"<r>&#0;</r>",ParserConfig::new().replace_unknown_entity_references(true))),"\u{fffd}");
}
#[test]
fn xml_dtd_entities_and_immutable_entity_diagnostics_retained() {
    assert_eq!(text(&events(br#"<!DOCTYPE r [<!ENTITY local "kept">]><r>&local;</r>"#,ParserConfig::new())),"kept");
    let mut reader = EventReader::new(&b"<r/>"[..]);
    reader.next().unwrap(); reader.next().unwrap();
    let error: xml::reader::Error = reader.add_entities([("late","no")]).unwrap_err().into();
    assert_eq!(error.kind().clone(),*error.kind());
    assert!(error.to_string().contains("Element encountered"));
}
#[test]
fn xml_io_reader_and_writer_errors_clone_without_losing_static_detail() {
    struct Broken;
    impl Read for Broken {fn read(&mut self,_:&mut[u8])->trueos_io::Result<usize>{Err(Error::new(ErrorKind::PermissionDenied,"owned-read-detail"))}}
    impl Write for Broken {fn write(&mut self,_:&[u8])->trueos_io::Result<usize>{Err(Error::new(ErrorKind::BrokenPipe,"owned-write-detail"))} fn flush(&mut self)->trueos_io::Result<()>{Ok(())}}
    let error = EventReader::new(Broken).next().unwrap_err();
    assert_eq!(error.clone(),error); assert!(error.clone().to_string().contains("owned-read-detail"));
    let mut writer = xml::EventWriter::new(Broken);
    let error = writer.write(xml::writer::XmlEvent::start_element("r")).unwrap_err();
    assert!(error.clone().to_string().contains("owned-write-detail"));
    let other: xml::reader::Error = Error::new(ErrorKind::PermissionDenied,"other-detail").into();
    assert_ne!(error.to_string(),other.to_string());
}
#[test]
fn xmltree_roundtrip_preserves_namespaces_node_types_and_escaped_data() {
    let source = br#"<?audit kept?><r xmlns="urn:root" xmlns:p="urn:leaf" z="&quot;&amp;&lt;" a="first"><?audit kept?><!--note--><![CDATA[a<b]]><p:c>text&amp;</p:c></r>"#;
    let tree = xmltree::Element::parse(&source[..]).unwrap();
    assert!(tree.children.iter().any(|n|matches!(n,xmltree::XMLNode::Comment(s) if s=="note")));
    assert!(tree.children.iter().any(|n|matches!(n,xmltree::XMLNode::CData(s) if s=="a<b")));
    assert!(tree.children.iter().any(|n|matches!(n,xmltree::XMLNode::ProcessingInstruction(target,_) if target=="audit")));
    let mut out = ShortWriter::default(); tree.write_with_config(&mut out,xml::EmitterConfig::new().autopad_comments(false)).unwrap();
    let again = xmltree::Element::parse(out.bytes.as_slice()).unwrap();
    assert_eq!(tree,again); assert_eq!(tree.namespace.as_deref(),Some("urn:root"));
    assert_eq!(tree.get_child("c").unwrap().namespace.as_deref(),Some("urn:leaf"));
    assert_eq!(tree.attributes.keys().map(String::as_str).collect::<Vec<_>>(),["a","z"]);
}
#[test]
fn xml_legacy_facade_has_same_core_io_parser() {
    let reader = xml_legacy::EventReader::new(Cursor::new(b"<r/>"));
    assert_eq!(reader.into_iter().filter(|e|matches!(e,Ok(xml_legacy::reader::XmlEvent::StartElement{..}))).count(),1);
}
#[test]
fn html_full_entities_and_unicode_stream_decode_survive_short_reads() {
    let input = "&thetasym;&OElig;&zwnj;&euro;&spades;&#x1F680;é";
    let mut out = ShortWriter::default();
    htmlescape::decode_html_rw(Chunks{bytes:input.as_bytes(),interrupted:false},&mut out).unwrap();
    assert_eq!(String::from_utf8(out.bytes).unwrap(),"ϑŒ\u{200c}€♠🚀é");
}
#[test]
fn html_attribute_hex_prefix_and_escaping_honor_partial_writes() {
    let input = "a \"'&<é🚀";
    let mut out = ShortWriter::default(); htmlescape::encode_attribute_w(input,&mut out).unwrap();
    assert_eq!(String::from_utf8(out.bytes).unwrap(),htmlescape::encode_attribute(input));
    let mut out = ShortWriter::default(); htmlescape::encode_minimal_w(input,&mut out).unwrap();
    assert_eq!(String::from_utf8(out.bytes).unwrap(),htmlescape::encode_minimal(input));
}
#[test]
fn html_error_categories_and_positions_are_preserved() {
    for (source,position,kind) in [("ok &missing;",3,htmlescape::DecodeErrKind::UnknownEntity),("ok &#xzz;",3,htmlescape::DecodeErrKind::MalformedNumEscape),("ok &amp",3,htmlescape::DecodeErrKind::PrematureEnd)] {
        let error = htmlescape::decode_html(source).unwrap_err(); assert_eq!(error.position,position); assert_eq!(error.kind,kind);
    }
    let error = htmlescape::decode_html_rw(Cursor::new(b"x\xff"),&mut Vec::new()).unwrap_err();
    assert!(matches!(error.kind,htmlescape::DecodeErrKind::EncodingError));
}
#[test]
fn mime_case_parameters_suffix_and_hash_semantics_preserved() {
    use std::hash::{DefaultHasher,Hash,Hasher};
    let a: mime::Mime = "Text/Plain; Charset=UTF-8; note=\"Keep Case\"".parse().unwrap();
    let b: mime::Mime = "text/plain; charset=utf-8; note=\"Keep Case\"".parse().unwrap();
    assert_eq!(a,b); assert_eq!(a.get_param("note").unwrap(),"Keep Case");
    assert_eq!(a.get_param(mime::CHARSET).unwrap(),mime::UTF_8);
    let hash = |v:&mime::Mime| {let mut h=DefaultHasher::new();v.hash(&mut h);h.finish()}; assert_eq!(hash(&a),hash(&b));
    let json: mime::Mime = "application/problem+json".parse().unwrap(); assert_eq!(json.suffix(),Some(mime::JSON));
    for invalid in ["text", "te xt/plain", "text/plain; q=\"unclosed"] {assert!(invalid.parse::<mime::Mime>().is_err(),"{invalid}");}
}
