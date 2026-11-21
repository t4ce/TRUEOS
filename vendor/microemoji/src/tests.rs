use super::*;
use std::vec::Vec;

fn point(bytes: &mut Vec<u8>, x: i32, y: i32) {
    bytes.extend(x.to_le_bytes());
    bytes.extend(y.to_le_bytes());
}

fn fixture() -> Vec<u8> {
    let mut path = Vec::new();
    path.push(0);
    point(&mut path, -64, 64);
    path.push(1);
    point(&mut path, 6400, 64);
    path.push(2);
    point(&mut path, 9600, 3200);
    point(&mut path, 6400, 6400);
    path.push(3);
    point(&mut path, 3200, 9600);
    point(&mut path, 0, 6400);
    point(&mut path, -64, 3200);
    path.push(4);

    let mut bytes = Vec::from(*b"MEOJ");
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(512u16.to_le_bytes());
    bytes.extend(477i16.to_le_bytes());
    bytes.extend((-91i16).to_le_bytes());
    bytes.extend(2u32.to_le_bytes());

    let payload_size = (LAYER_HEADER_SIZE + path.len()) as u32;
    let first_payload = (HEADER_SIZE + 2 * ENTRY_SIZE) as u32;
    for (scalar, offset) in [('©', first_payload), ('😀', first_payload + payload_size)] {
        bytes.extend((scalar as u32).to_le_bytes());
        bytes.extend(offset.to_le_bytes());
        bytes.extend(payload_size.to_le_bytes());
        bytes.extend(512u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
    }

    for color in [[0x12, 0x34, 0x56, 0xff], [0x78, 0x9a, 0xbc, 0x80]] {
        bytes.extend(color);
        bytes.extend((path.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&path);
    }
    bytes
}

#[test]
fn lookup_reads_metrics_and_closed_curve_records() {
    let bytes = fixture();
    let bundle = Bundle::parse(&bytes).unwrap();
    assert_eq!(
        bundle.glyphs().map(Glyph::scalar).collect::<Vec<_>>(),
        ['©', '😀']
    );
    assert!(bundle.glyph('a').is_none());
    assert_eq!(bundle.metrics().units_per_em, 512);

    let glyph = bundle.glyph('😀').unwrap();
    assert_eq!(glyph.advance(), 512);
    let [layer] = glyph.layers().collect::<Vec<_>>()[..] else {
        panic!("expected one layer");
    };
    assert_eq!(layer.color(), [0x78, 0x9a, 0xbc, 0x80]);
    let commands = layer.commands().collect::<Vec<_>>();
    assert_eq!(commands[0], Command::Move(Point { x: -64, y: 64 }));
    assert!(matches!(commands[1], Command::Line(_)));
    assert!(matches!(commands[2], Command::Quad(..)));
    assert!(matches!(commands[3], Command::Cubic(..)));
    assert_eq!(commands[4], Command::Close);
}

#[test]
fn parse_rejects_invalid_index_and_unclosed_contour() {
    let mut bytes = fixture();
    assert!((0..bytes.len()).all(|len| Bundle::parse(&bytes[..len]).is_err()));
    let last = bytes.len() - 1;
    bytes[last] = 0;
    assert_eq!(Bundle::parse(&bytes).unwrap_err(), Error::Path);
    bytes[last] = 4;
    bytes[HEADER_SIZE + ENTRY_SIZE..HEADER_SIZE + ENTRY_SIZE + 4]
        .copy_from_slice(&('©' as u32).to_le_bytes());
    assert_eq!(Bundle::parse(&bytes).unwrap_err(), Error::Index);
}
