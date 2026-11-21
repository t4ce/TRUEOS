//! Convert a COLR v0 font to a microemoji bundle and export its glyphs as SVG.

use std::collections::BTreeSet;
use std::error::Error;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use microemoji::{Bundle, svg};
use ttf_parser::colr::{ClipBox, CompositeMode, Paint, Painter};
use ttf_parser::{Face, GlyphId, OutlineBuilder, RgbaColor, Tag, Transform};

const DEFAULT_GLYPH_COUNT: usize = 256;
const HEADER_LEN: usize = 16;
const INDEX_ENTRY_LEN: usize = 16;
const USAGE: &str = "Usage\n  export <font.ttf> <output-dir> [--selection <file>]\n  export <font.ttf> --alpha-bundle <output.bin> [--selection <file>]\n  export <font.ttf> --list-default\nThe output directory must be new or empty. The alpha bundle must be a new file.";

enum Output<'a> {
    Svg(&'a Path),
    AlphaBundle(&'a Path),
    List,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("microemoji export failed. {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let (font, output, selection) = match args.as_slice() {
        [help] if help == "--help" || help == "-h" => {
            println!("{USAGE}");
            return Ok(());
        }
        [font, list] if list == "--list-default" => (font, Output::List, None),
        [font, option, output] if option == "--alpha-bundle" => {
            (font, Output::AlphaBundle(Path::new(output)), None)
        }
        [font, option, output, flag, selection]
            if option == "--alpha-bundle" && flag == "--selection" =>
        {
            (
                font,
                Output::AlphaBundle(Path::new(output)),
                Some(selection),
            )
        }
        [font, output] if output != "--alpha-bundle" => {
            (font, Output::Svg(Path::new(output)), None)
        }
        [font, output, flag, selection] if output != "--alpha-bundle" && flag == "--selection" => {
            (font, Output::Svg(Path::new(output)), Some(selection))
        }
        _ => return Err(USAGE.into()),
    };

    let font = fs::read(font)?;
    let face = Face::parse(&font, 0)?;
    validate_font(&face)?;
    let selected = match selection {
        Some(path) => parse_selection(&fs::read_to_string(path)?)?,
        None => default_selection(&face)?,
    };

    match output {
        Output::Svg(path) => write_outputs(path, &pack(&face, &selected, false)?),
        Output::AlphaBundle(path) => write_alpha_bundle(path, &pack(&face, &selected, true)?),
        Output::List => {
            let mut out = io::stdout().lock();
            for codepoint in selected {
                writeln!(out, "U+{:04X}", u32::from(codepoint))?;
            }
            Ok(())
        }
    }
}

fn validate_font(face: &Face<'_>) -> Result<(), Box<dyn Error>> {
    let Some(colr) = face.raw_face().table(Tag::from_bytes(b"COLR")) else {
        return Err("The font has no COLR table.".into());
    };
    if !colr.starts_with(&[0, 0]) {
        return Err("The exporter requires a COLR version 0 font.".into());
    }
    if face.color_palettes().is_none() {
        return Err("The font has invalid COLR or CPAL tables.".into());
    }
    Ok(())
}

fn parse_selection(text: &str) -> Result<Vec<char>, Box<dyn Error>> {
    let mut selected = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        let entry = line.split('#').next().unwrap_or_default().trim();
        if entry.is_empty() {
            continue;
        }
        let codepoint = entry
            .strip_prefix("U+")
            .filter(|digits| digits.bytes().all(|digit| digit.is_ascii_hexdigit()))
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .and_then(char::from_u32)
            .ok_or_else(|| format!("Expected a Unicode scalar as U+HEX on line {}.", index + 1))?;
        if !selected.insert(codepoint) {
            return Err(format!("Duplicate selection {entry} on line {}.", index + 1).into());
        }
    }
    if selected.is_empty() {
        return Err("The selection file contains no Unicode scalars.".into());
    }
    Ok(selected.into_iter().collect())
}

fn default_selection(face: &Face<'_>) -> Result<Vec<char>, Box<dyn Error>> {
    let cmap = face.tables().cmap.ok_or("The font has no cmap table.")?;
    let mut candidates = BTreeSet::new();
    for table in cmap
        .subtables
        .into_iter()
        .filter(|table| table.is_unicode())
    {
        table.codepoints(|scalar| {
            if let Some(codepoint) = char::from_u32(scalar) {
                candidates.insert(codepoint);
            }
        });
    }

    let selected: Vec<_> = candidates
        .into_iter()
        .filter(|&codepoint| color_glyph(face, codepoint).is_some())
        .take(DEFAULT_GLYPH_COUNT)
        .collect();
    if selected.is_empty() {
        return Err("The font has no Unicode scalars with COLR glyphs.".into());
    }
    Ok(selected)
}

fn color_glyph(face: &Face<'_>, codepoint: char) -> Option<GlyphId> {
    face.glyph_index(codepoint)
        .filter(|&glyph| face.is_color_glyph(glyph))
}

fn pack(face: &Face<'_>, selected: &[char], alpha_only: bool) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut index = Vec::with_capacity(HEADER_LEN + INDEX_ENTRY_LEN * selected.len());
    index.extend_from_slice(b"MEOJ");
    index.extend_from_slice(&1u16.to_le_bytes());
    index.extend_from_slice(&face.units_per_em().to_le_bytes());
    index.extend_from_slice(&face.ascender().to_le_bytes());
    index.extend_from_slice(&face.descender().to_le_bytes());
    index.extend_from_slice(&u32::try_from(selected.len())?.to_le_bytes());

    let payload_start = HEADER_LEN + INDEX_ENTRY_LEN * selected.len();
    let mut payload = Vec::new();
    for &codepoint in selected {
        let scalar = u32::from(codepoint);
        let glyph = color_glyph(face, codepoint)
            .ok_or_else(|| format!("U+{scalar:04X} has no COLR glyph."))?;
        let advance = face
            .glyph_hor_advance(glyph)
            .ok_or_else(|| format!("U+{scalar:04X} has no horizontal advance."))?;
        let start = u32::try_from(payload_start + payload.len())?;
        let mut painter = LayerWriter {
            face,
            outline: Outline::default(),
            payload: &mut payload,
            layer_count: 0,
            alpha_only,
        };
        let foreground = RgbaColor::new(0, 0, 0, 255);
        face.paint_color_glyph(glyph, 0, foreground, &mut painter)
            .ok_or_else(|| format!("U+{scalar:04X} has invalid COLR layers."))?;
        let layer_count = painter.layer_count;
        if layer_count == 0 {
            return Err(format!("U+{scalar:04X} has no drawable COLR layers.").into());
        }
        let end = u32::try_from(payload_start + payload.len())?;

        index.extend_from_slice(&scalar.to_le_bytes());
        index.extend_from_slice(&start.to_le_bytes());
        index.extend_from_slice(&(end - start).to_le_bytes());
        index.extend_from_slice(&advance.to_le_bytes());
        index.extend_from_slice(&layer_count.to_le_bytes());
    }
    index.extend_from_slice(&payload);
    Ok(index)
}

struct LayerWriter<'a, 'font> {
    face: &'a Face<'font>,
    outline: Outline,
    payload: &'a mut Vec<u8>,
    layer_count: u16,
    alpha_only: bool,
}

impl<'font> Painter<'font> for LayerWriter<'_, 'font> {
    fn outline_glyph(&mut self, glyph_id: GlyphId) {
        self.outline.0.clear();
        if self
            .face
            .outline_glyph(glyph_id, &mut self.outline)
            .is_none()
        {
            self.outline.0.clear();
        }
    }

    fn paint(&mut self, paint: Paint<'font>) {
        if self.outline.0.is_empty() {
            return;
        }
        let Paint::Solid(color) = paint else {
            unreachable!("COLR version 0 uses solid colors");
        };
        let rgba = if self.alpha_only {
            [0, 0, 0, color.alpha]
        } else {
            [color.red, color.green, color.blue, color.alpha]
        };
        self.payload.extend_from_slice(&rgba);
        let len = u32::try_from(self.outline.0.len()).expect("OpenType outline exceeds 4 GiB");
        self.payload.extend_from_slice(&len.to_le_bytes());
        self.payload.extend_from_slice(&self.outline.0);
        self.layer_count += 1;
    }

    // Only the two callbacks above are used by COLR version 0.
    fn push_clip(&mut self) {}
    fn push_clip_box(&mut self, _: ClipBox) {}
    fn pop_clip(&mut self) {}
    fn push_layer(&mut self, _: CompositeMode) {}
    fn pop_layer(&mut self) {}
    fn push_transform(&mut self, _: Transform) {}
    fn pop_transform(&mut self) {}
}

#[derive(Default)]
struct Outline(Vec<u8>);

impl Outline {
    fn command(&mut self, opcode: u8, points: &[(f32, f32)]) {
        self.0.push(opcode);
        for &(x, y) in points {
            self.0
                .extend_from_slice(&((x * 64.0).round() as i32).to_le_bytes());
            self.0
                .extend_from_slice(&((y * 64.0).round() as i32).to_le_bytes());
        }
    }
}

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.command(0, &[(x, y)]);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.command(1, &[(x, y)]);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.command(2, &[(cx, cy), (x, y)]);
    }

    fn curve_to(&mut self, cx1: f32, cy1: f32, cx2: f32, cy2: f32, x: f32, y: f32) {
        self.command(3, &[(cx1, cy1), (cx2, cy2), (x, y)]);
    }

    fn close(&mut self) {
        self.command(4, &[]);
    }
}

fn write_alpha_bundle(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    Bundle::parse(bytes)?;
    let mut file = File::create_new(path)?;
    let result = file.write_all(bytes);
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result.map_err(Into::into)
}

fn write_outputs(output_dir: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let bundle =
        Bundle::parse(bytes).map_err(|error| format!("Invalid generated bundle ({error:?})."))?;
    fs::create_dir_all(output_dir)?;
    if fs::read_dir(output_dir)?.next().transpose()?.is_some() {
        return Err(format!(
            "The output directory {} must be new or empty.",
            output_dir.display()
        )
        .into());
    }
    // Reserve the bundle name before producing SVGs so parallel runs cannot mix selections.
    let bundle_path = output_dir.join("microemoji.bin");
    let mut bundle_file = File::create_new(&bundle_path)?;
    let mut created = Vec::new();
    let result = (|| -> Result<(), Box<dyn Error>> {
        let mut document = String::new();
        for glyph in bundle.glyphs() {
            let scalar = u32::from(glyph.scalar());
            let path = output_dir.join(format!("u{scalar:04X}.svg"));
            document.clear();
            svg::write_glyph(&mut document, glyph)?;
            let mut file = File::create_new(&path)?;
            created.push(path);
            file.write_all(document.as_bytes())?;
        }
        bundle_file.write_all(bytes)?;
        Ok(())
    })();
    drop(bundle_file);
    if result.is_err() {
        for path in created {
            let _ = fs::remove_file(path);
        }
        let _ = fs::remove_file(bundle_path);
    }
    result
}
