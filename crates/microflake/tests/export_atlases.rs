use std::fs;
use std::path::Path;

use microflake::{Motif, Palette, Settings, Steps, render_atlas};

#[test]
#[ignore = "writes six PNG previews to tgt/microflake-atlases"]
fn export_atlases() {
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tgt/microflake-atlases");
    fs::create_dir_all(&output).unwrap();

    for (name, motif, palette) in [
        ("flower", Motif::Flower, Palette::Cobalt),
        ("snowflake", Motif::Snowflake, Palette::Lagoon),
    ] {
        for steps in [Steps::Sixteen, Steps::ThirtyTwo, Steps::FortyEight] {
            let settings = Settings {
                motif,
                steps,
                palette,
                seed: "tiny-wonder-8lx4",
                ..Settings::default()
            };
            let mut buffer = vec![0; steps.byte_len()];
            let pixels = render_atlas(&settings, &mut buffer).unwrap();

            let path = output.join(format!("{name}-{}.png", steps.count()));
            fs::write(&path, encode_png(steps, pixels)).unwrap();
            println!("{}", path.display());
        }
    }
}

fn encode_png(steps: Steps, rgba: &[u8]) -> Vec<u8> {
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&(steps.width() as u32).to_be_bytes());
    header.extend_from_slice(&(steps.height() as u32).to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]); // RGBA8, no interlace.

    let mut scanlines = Vec::with_capacity(rgba.len() + steps.height());
    for row in rgba.chunks_exact(steps.width() * 4) {
        scanlines.push(0); // No PNG row filter.
        scanlines.extend_from_slice(row);
    }
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&scanlines, 6);

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    append_chunk(&mut png, b"IHDR", &header);
    append_chunk(&mut png, b"IDAT", &compressed);
    append_chunk(&mut png, b"IEND", &[]);
    png
}

fn append_chunk(png: &mut Vec<u8>, kind: &[u8; 4], bytes: &[u8]) {
    png.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    let crc_start = png.len();
    png.extend_from_slice(kind);
    png.extend_from_slice(bytes);
    png.extend_from_slice(&crc32fast::hash(&png[crc_start..]).to_be_bytes());
}
