use lily_asset_pack::{DecodedPack, HEADER, MAGIC, PIXELS, crc32, decode, decode_with_video};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufReader, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::Instant,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const NONE: u32 = u32::MAX;

struct Input {
    names: Vec<String>,
    rects: Vec<[u16; 4]>,
    palette: [[u8; 4]; 32],
    indices: Vec<u8>,
    png_bytes: u64,
    stale: usize,
}
fn list_png(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            list_png(&path, out)?;
        } else if path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("png"))
        {
            out.push(path);
        }
    }
    Ok(())
}
fn load(root: &Path) -> Result<Input> {
    let alignment: Value = serde_json::from_slice(&fs::read(root.join("alignment.json"))?)?;
    if alignment["canvas"] != json!([256, 256])
        || alignment["rect_format"] != json!(["x", "y", "w", "h"])
        || alignment["origin"] != "top-left"
    {
        return Err("unexpected alignment convention".into());
    }
    let mut paths = Vec::new();
    list_png(root, &mut paths)?;
    paths.sort_by_key(|p| {
        (
            !p.strip_prefix(root).unwrap().starts_with("Static_up"),
            p.clone(),
        )
    });
    let mut input = Input {
        names: vec![],
        rects: vec![],
        palette: [[0; 4]; 32],
        indices: Vec::with_capacity(paths.len() * PIXELS),
        png_bytes: 0,
        stale: 0,
    };
    for path in paths {
        let name = path
            .strip_prefix(root)?
            .to_str()
            .ok_or("UTF8 path")?
            .replace('\\', "/");
        let r = alignment["frames"][&name]
            .as_array()
            .ok_or("missing crop metadata")?;
        let mut rect = [0u16; 4];
        for (v, n) in rect.iter_mut().zip(r) {
            *v = n.as_u64().ok_or("bad crop")?.try_into()?;
        }
        let [x, y, w, h] = rect.map(usize::from);
        if x + w > 256 || y + h > 256 || w == 0 || h == 0 {
            return Err("crop exceeds canvas".into());
        }
        let mut reader = png::Decoder::new(BufReader::new(fs::File::open(&path)?)).read_info()?;
        let info = reader.info();
        if info.color_type != png::ColorType::Indexed || info.bit_depth != png::BitDepth::Eight {
            return Err(format!("not 8-bit indexed: {name}").into());
        }
        let rgb = info.palette.as_ref().ok_or("missing palette")?;
        if rgb.len() != 96 || info.trns.as_deref() != Some(&[0][..]) {
            return Err("expected trimmed shared-32 palette".into());
        }
        let mut palette = [[0; 4]; 32];
        for i in 0..32 {
            palette[i][..3].copy_from_slice(&rgb[i * 3..i * 3 + 3]);
            palette[i][3] = if i == 0 { 0 } else { 255 };
        }
        if input.names.is_empty() {
            input.palette = palette;
        } else if palette != input.palette {
            return Err("palette differs".into());
        }
        let mut buf = vec![0; reader.output_buffer_size().ok_or("PNG buffer size")?];
        let frame = reader.next_frame(&mut buf)?;
        if frame.width as usize != w || frame.height as usize != h || frame.buffer_size() != w * h {
            return Err("PNG dimensions disagree with alignment".into());
        }
        if buf[..w * h].iter().any(|&v| v >= 32) {
            return Err("palette index >31".into());
        }
        let start = input.indices.len();
        input.indices.resize(start + PIXELS, 0);
        for row in 0..h {
            input.indices[start + (y + row) * 256 + x..start + (y + row) * 256 + x + w]
                .copy_from_slice(&buf[row * w..(row + 1) * w]);
        }
        input.png_bytes += fs::metadata(&path)?.len();
        input.names.push(name);
        input.rects.push(rect);
    }
    input.stale = alignment["frames"]
        .as_object()
        .ok_or("manifest frames")?
        .len()
        - input.names.len();
    println!(
        "Loaded {} PNGs, {} bytes; ignoring {} stale manifest entries",
        input.names.len(),
        input.png_bytes,
        input.stale
    );
    Ok(input)
}
fn refs(input: &Input, nearest: bool) -> Vec<u32> {
    let mut references = vec![NONE; input.names.len()];
    for (i, reference) in references.iter_mut().enumerate().skip(1) {
        if !nearest {
            if Path::new(&input.names[i]).parent() == Path::new(&input.names[i - 1]).parent() {
                *reference = (i - 1) as u32;
            }
            continue;
        }
        let current = &input.indices[i * PIXELS..(i + 1) * PIXELS];
        // Sample every 8th pixel/row to shortlist predecessors; evaluate full
        // Hamming distance on the best 12, immediate predecessor and statics.
        let mut candidates: Vec<(usize, usize)> = (0..i)
            .map(|r| {
                let other = &input.indices[r * PIXELS..(r + 1) * PIXELS];
                let mut distance = 0;
                for y in (3..256).step_by(8) {
                    for x in (3..256).step_by(8) {
                        distance += usize::from(current[y * 256 + x] != other[y * 256 + x]);
                    }
                }
                (distance, r)
            })
            .collect();
        candidates.sort_unstable();
        let mut ids: HashSet<usize> = candidates.iter().take(12).map(|p| p.1).collect();
        ids.insert(i - 1);
        for r in 0..3.min(i) {
            ids.insert(r);
        }
        let best = ids
            .into_iter()
            .map(|r| {
                let distance = current
                    .iter()
                    .zip(&input.indices[r * PIXELS..(r + 1) * PIXELS])
                    .filter(|(a, b)| a != b)
                    .count();
                (distance, r)
            })
            .min()
            .unwrap();
        *reference = best.1 as u32;
    }
    references
}
fn metadata(input: &Input, references: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in &input.palette {
        out.extend_from_slice(c);
    }
    for (i, name) in input.names.iter().enumerate() {
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        for n in input.rects[i] {
            out.extend_from_slice(&n.to_le_bytes());
        }
        out.extend_from_slice(&references[i].to_le_bytes());
        out.extend_from_slice(&crc32(&input.indices[i * PIXELS..(i + 1) * PIXELS]).to_le_bytes());
    }
    out
}
fn residual(input: &Input, references: &[u32], subtract: bool) -> Vec<u8> {
    let mut out = input.indices.clone();
    for (i, &r) in references.iter().enumerate() {
        if r != NONE {
            for j in 0..PIXELS {
                let other = input.indices[r as usize * PIXELS + j];
                let v = &mut out[i * PIXELS + j];
                *v = if subtract {
                    v.wrapping_sub(other) & 31
                } else {
                    *v ^ other
                };
            }
        }
    }
    out
}
fn bitplanes(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0; data.len() * 5 / 8];
    for (f, frame) in data.as_chunks::<PIXELS>().0.iter().enumerate() {
        for bit in 0..5 {
            for (j, chunk) in frame.as_chunks::<8>().0.iter().enumerate() {
                let mut value = 0;
                for (k, &v) in chunk.iter().enumerate() {
                    value |= ((v >> bit) & 1) << k;
                }
                out[f * PIXELS * 5 / 8 + bit * PIXELS / 8 + j] = value;
            }
        }
    }
    out
}
fn packed5(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0; data.len() * 5 / 8];
    for (i, &v) in data.iter().enumerate() {
        let bit = i * 5;
        let shift = bit % 8;
        let pos = bit / 8;
        out[pos] |= v << shift;
        if shift > 3 {
            out[pos + 1] |= v >> (8 - shift);
        }
    }
    out
}
fn tiles(input: &Input, tile: usize) -> Vec<u8> {
    let mut dictionary: HashMap<Vec<u8>, u32> = HashMap::new();
    let mut values = Vec::new();
    let mut ids = Vec::new();
    for frame in input.indices.as_chunks::<PIXELS>().0 {
        for y in (0..256).step_by(tile) {
            for x in (0..256).step_by(tile) {
                let mut pixels = Vec::with_capacity(tile * tile);
                for row in y..y + tile {
                    pixels.extend_from_slice(&frame[row * 256 + x..row * 256 + x + tile]);
                }
                let next = dictionary.len() as u32;
                let id = *dictionary.entry(pixels.clone()).or_insert_with(|| {
                    values.extend_from_slice(&pixels);
                    next
                });
                ids.extend_from_slice(&id.to_le_bytes());
            }
        }
    }
    println!(
        "  {}x{} tiles: {} unique / {} total",
        tile,
        tile,
        dictionary.len(),
        ids.len() / 4
    );
    let mut out = (dictionary.len() as u32).to_le_bytes().to_vec();
    out.extend(values);
    out.extend(ids);
    out
}
fn header(codec: u8, transform: u8, tile: u16, count: usize, body: &[u8]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&[codec, transform]);
    out.extend_from_slice(&tile.to_le_bytes());
    out.extend_from_slice(&(count as u32).to_le_bytes());
    out.extend_from_slice(&(body.len() as u64).to_le_bytes());
    out.extend_from_slice(&crc32(body).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out
}
fn encode(codec: u8, transform: u8, tile: u16, count: usize, body: &[u8]) -> Result<Vec<u8>> {
    let mut out = header(codec, transform, tile, count, body);
    let payload = match codec {
        0 => miniz_oxide::deflate::compress_to_vec_zlib(body, 9),
        1 => zstd::stream::encode_all(body, 19)?,
        2 => {
            let mut encoder = xz2::write::XzEncoder::new(Vec::new(), 9);
            encoder.write_all(body)?;
            encoder.finish()?
        }
        _ => return Err("unknown encoder".into()),
    };
    out.extend(payload);
    Ok(out)
}
fn decode_host(pack: &[u8]) -> Result<DecodedPack> {
    if pack.get(8) == Some(&3) {
        let payload = &pack[HEADER..];
        let meta = u32::from_le_bytes(payload[..4].try_into()?) as usize;
        let video = &payload[4 + meta..];
        // A pipe writer thread prevents deadlock while ffmpeg writes decoded frames.
        let mut child = Command::new("ffmpeg")
            .args([
                "-v", "error", "-threads", "2", "-f", "h264", "-i", "pipe:0", "-f", "rawvideo",
                "-pix_fmt", "gray", "pipe:1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child.stdin.take().unwrap();
        let bytes = video.to_vec();
        let writer = std::thread::spawn(move || stdin.write_all(&bytes));
        let output = child.wait_with_output()?;
        writer.join().unwrap()?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
        }
        Ok(decode_with_video(pack, |_| Ok(output.stdout))?)
    } else {
        Ok(decode(pack)?)
    }
}
fn verify(decoded: &DecodedPack, input: &Input) -> Result<(f64, usize)> {
    if decoded.frames.len() != input.names.len()
        || decoded.palette != input.palette
        || decoded.indices != input.indices
    {
        return Err("decoded indices/palette/count mismatch".into());
    }
    let start = Instant::now();
    let mut rgba_cache = Vec::with_capacity(input.names.len() * PIXELS * 4);
    for (i, frame) in decoded.frames.iter().enumerate() {
        if frame.path != input.names[i] || frame.rect != input.rects[i] {
            return Err("metadata differs".into());
        }
        let rgba = decoded.frame_rgba8(i)?;
        for (p, &v) in input.indices[i * PIXELS..(i + 1) * PIXELS]
            .iter()
            .enumerate()
        {
            if rgba[p * 4..p * 4 + 4] != input.palette[v as usize] {
                return Err("RGBA8 pixel mismatch".into());
            }
        }
        rgba_cache.extend_from_slice(&rgba);
    }
    Ok((start.elapsed().as_secs_f64(), rgba_cache.len()))
}
#[allow(clippy::too_many_arguments)] // Host benchmark plumbing; not a runtime API.
fn run_variant(
    input: &Input,
    out: &Path,
    name: &str,
    codec: u8,
    transform: u8,
    tile: u16,
    body: &[u8],
    results: &mut Vec<Value>,
) -> Result<Vec<u8>> {
    let start = Instant::now();
    let pack = encode(codec, transform, tile, input.names.len(), body)?;
    let encode_s = start.elapsed().as_secs_f64();
    let path = out.join(format!("{name}.lpak"));
    fs::write(&path, &pack)?;
    let start = Instant::now();
    let decoded = decode_host(&pack)?;
    let decode_s = start.elapsed().as_secs_f64();
    let (rgba_s, rgba_bytes) = verify(&decoded, input)?;
    let size = pack.len();
    println!(
        "{name}: {size} bytes ({:.2} MiB), encode {encode_s:.2}s, pure-Rust decode {decode_s:.3}s, RGBA8 cache {rgba_s:.3}s -- EXACT",
        size as f64 / 1048576.
    );
    results.push(json!({"name":name,"file":path.file_name().unwrap().to_str(),"bytes":size,"saved_vs_png_percent":100.*(1.-size as f64/input.png_bytes as f64),"encode_seconds":encode_s,"decode_seconds":decode_s,"rgba8_materialize_and_verify_seconds":rgba_s,"rgba8_cache_bytes":rgba_bytes,"exact":true,"codec":codec,"transform":transform,"tile":tile}));
    fs::write(
        out.join("results.json"),
        serde_json::to_vec_pretty(
            &json!({"frames":input.names.len(),"source_png_bytes":input.png_bytes,"ignored_stale_alignment_entries":input.stale,"variants":results}),
        )?,
    )?;
    Ok(pack)
}
fn video(input: &Input, out: &Path, results: &mut Vec<Value>) -> Result<()> {
    let start = Instant::now();
    let raw = out.join(".indices.gray");
    fs::write(&raw, &input.indices)?;
    let video = out.join(".indices.h264");
    let result = Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "gray",
            "-video_size",
            "256x256",
            "-framerate",
            "10",
            "-i",
        ])
        .arg(&raw)
        .args([
            "-an",
            "-c:v",
            "libx264",
            "-preset",
            "veryslow",
            "-qp",
            "0",
            "-pix_fmt",
            "gray",
            "-threads",
            "4",
            "-x264-params",
            "keyint=120:scenecut=40",
            "-f",
            "h264",
        ])
        .arg(&video)
        .output()?;
    fs::remove_file(raw)?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).into_owned().into());
    }
    let encoded = fs::read(&video)?;
    fs::remove_file(video)?;
    let meta = metadata(input, &vec![NONE; input.names.len()]);
    let mut body = meta.clone();
    body.extend_from_slice(&input.indices);
    let mut pack = header(3, 0, 0, input.names.len(), &body);
    pack.extend_from_slice(&(meta.len() as u32).to_le_bytes());
    pack.extend(meta);
    pack.extend(encoded);
    let encode_s = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let decoded = decode_host(&pack)?;
    let decode_s = start.elapsed().as_secs_f64();
    let (rgba_s, rgba_bytes) = verify(&decoded, input)?;
    fs::write(out.join("video-h264-lossless.lpak"), &pack)?;
    println!(
        "video-h264-lossless: {} bytes ({:.2} MiB), encode {encode_s:.2}s, decode {decode_s:.3}s -- EXACT",
        pack.len(),
        pack.len() as f64 / 1048576.
    );
    results.push(json!({"name":"video-h264-lossless","file":"video-h264-lossless.lpak","bytes":pack.len(),"saved_vs_png_percent":100.*(1.-pack.len() as f64/input.png_bytes as f64),"encode_seconds":encode_s,"decode_seconds":decode_s,"rgba8_materialize_and_verify_seconds":rgba_s,"rgba8_cache_bytes":rgba_bytes,"exact":true,"codec":3,"transform":0,"tile":0}));
    Ok(())
}
fn bench(root: &Path, out: &Path) -> Result<()> {
    fs::create_dir_all(out)?;
    let input = load(root)?;
    let none = vec![NONE; input.names.len()];
    let adjacent = refs(&input, false);
    let nearest = refs(&input, true);
    let mut results = vec![];
    let mut candidates: Vec<(usize, String, u8, u16, Vec<u8>)> = vec![];
    let variants = [
        ("aligned", 0, false),
        ("packed5", 6, false),
        ("bitplanes", 2, false),
        ("adjacent-xor", 1, false),
        ("reference-xor", 1, true),
        ("reference-xor-bitplanes", 3, true),
        ("reference-subtract", 4, true),
    ];
    for (name, transform, use_nearest) in variants {
        let references = if matches!(transform, 1 | 3 | 4) {
            if use_nearest { &nearest } else { &adjacent }
        } else {
            &none
        };
        let data = match transform {
            0 => input.indices.clone(),
            6 => packed5(&input.indices),
            2 => bitplanes(&input.indices),
            1 => residual(&input, references, false),
            3 => bitplanes(&residual(&input, references, false)),
            4 => residual(&input, references, true),
            _ => unreachable!(),
        };
        let mut body = metadata(&input, references);
        body.extend(data);
        let pack = run_variant(
            &input,
            out,
            &format!("{name}-zstd"),
            1,
            transform,
            0,
            &body,
            &mut results,
        )?;
        candidates.push((pack.len(), name.into(), transform, 0, body));
    }
    for tile in [4, 8, 16] {
        let mut body = metadata(&input, &none);
        body.extend(tiles(&input, tile));
        let name = format!("tiles{tile}");
        let pack = run_variant(
            &input,
            out,
            &format!("{name}-zstd"),
            1,
            5,
            tile as u16,
            &body,
            &mut results,
        )?;
        candidates.push((pack.len(), name, 5, tile as u16, body));
    }
    candidates.sort_by_key(|c| c.0);
    for (_, name, transform, tile, body) in candidates.iter().take(3) {
        run_variant(
            &input,
            out,
            &format!("{name}-xz"),
            2,
            *transform,
            *tile,
            body,
            &mut results,
        )?;
    }
    let (_, name, transform, tile, body) = &candidates[0];
    run_variant(
        &input,
        out,
        &format!("{name}-deflate"),
        0,
        *transform,
        *tile,
        body,
        &mut results,
    )?;
    if let Err(err) = video(&input, out, &mut results) {
        eprintln!("VIDEO FAILED (not accepted): {err}");
        fs::write(out.join("video-error.txt"), err.to_string())?;
    }
    results.sort_by_key(|r| r["bytes"].as_u64().unwrap());
    fs::write(
        out.join("results.json"),
        serde_json::to_vec_pretty(
            &json!({"frames":input.names.len(),"source_png_bytes":input.png_bytes,"ignored_stale_alignment_entries":input.stale,"variants":results}),
        )?,
    )?;
    let mut report = format!(
        "# LilyDS lossless pack experiment\n\n{} current PNG frames; source {} bytes ({:.2} MiB). {} stale alignment entries ignored.\n\nAll accepted packs were decoded and all aligned RGBA8 pixels verified exactly.\n\n| Variant | Bytes | MiB | Saved vs PNGs | Decode seconds |\n| --- | ---: | ---: | ---: | ---: |\n",
        input.names.len(),
        input.png_bytes,
        input.png_bytes as f64 / 1048576.,
        input.stale
    );
    for r in &results {
        let size = r["bytes"].as_u64().unwrap();
        report.push_str(&format!(
            "| {} | {} | {:.2} | {:.1}% | {:.3} |\n",
            r["name"].as_str().unwrap(),
            size,
            size as f64 / 1048576.,
            r["saved_vs_png_percent"].as_f64().unwrap(),
            r["decode_seconds"].as_f64().unwrap()
        ));
    }
    report.push_str(&format!("\nBest: `{}`. Full RGBA8 cache: {} bytes ({:.2} MiB). Index cache: {} bytes ({:.2} MiB). Decoder scratch memory is additional. Timings are one local release-build run, not TRUEOS timings. Video decode includes ffmpeg process/pipe overhead.\n",results[0]["file"].as_str().unwrap(),input.indices.len()*4,input.indices.len() as f64*4./1048576.,input.indices.len(),input.indices.len() as f64/1048576.));
    fs::write(out.join("REPORT.md"), &report)?;
    println!("\n{report}");
    Ok(())
}
fn unpack(path: &Path, out: &Path, raw: bool) -> Result<()> {
    let decoded = decode_host(&fs::read(path)?)?;
    fs::create_dir_all(out)?;
    let mut metadata = serde_json::Map::new();
    for (i, frame) in decoded.frames.iter().enumerate() {
        let relative = Path::new(&frame.path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err("unsafe output path".into());
        }
        let mut path = out.join(relative);
        if raw {
            path.set_extension("rgba8");
        }
        fs::create_dir_all(path.parent().unwrap())?;
        if raw {
            fs::write(path, decoded.frame_rgba8(i)?)?;
        } else {
            let mut encoder = png::Encoder::new(
                fs::File::create(path)?,
                frame.rect[2] as u32,
                frame.rect[3] as u32,
            );
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()?
                .write_image_data(&decoded.cropped_rgba8(i)?)?;
        }
        metadata.insert(frame.path.clone(), json!(frame.rect));
    }
    fs::write(
        out.join("alignment.json"),
        serde_json::to_vec(
            &json!({"canvas":[256,256],"rect_format":["x","y","w","h"],"origin":"top-left","frames":metadata}),
        )?,
    )?;
    println!("Unpacked {} frames", decoded.frames.len());
    Ok(())
}
fn runtime_load(path: &Path) -> Result<()> {
    let pack = fs::read(path)?;
    let start = Instant::now();
    let decoded = decode_host(&pack)?;
    let decode_s = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let mut rgba = Vec::with_capacity(decoded.frames.len() * PIXELS * 4);
    for i in 0..decoded.frames.len() {
        rgba.extend(decoded.frame_rgba8(i)?);
    }
    let expand_s = start.elapsed().as_secs_f64();
    println!(
        "{} frames; decode {decode_s:.3}s; RGBA8 expansion {expand_s:.3}s; RGBA8 cache {} bytes; index cache {} bytes; RGBA8 CRC32 {:08x}",
        decoded.frames.len(),
        rgba.len(),
        decoded.indices.len(),
        crc32(&rgba)
    );
    std::hint::black_box((&decoded, &rgba));
    Ok(())
}

/// Production path: one self-contained pack, no benchmark outputs.
fn pack_single(source: &Path, output: &Path) -> Result<()> {
    let input = load(source)?;
    let references = vec![NONE; input.names.len()];
    let mut body = metadata(&input, &references);
    body.extend(tiles(&input, 16));
    let pack = encode(2, 5, 16, input.names.len(), &body)?;
    let decoded = decode(&pack)?;
    verify(&decoded, &input)?;
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, &pack)?;
    println!(
        "Wrote {} frames, {} bytes, exact RGBA8 verified: {}",
        input.names.len(),
        pack.len(),
        output.display()
    );
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str){
        Some("pack") if args.len() == 4 => pack_single(Path::new(&args[2]), Path::new(&args[3])),
        Some("load") if args.len() == 3 => runtime_load(Path::new(&args[2])),
        Some("bench")=>bench(Path::new(args.get(2).map(String::as_str).unwrap_or("../LilyDS")),Path::new(args.get(3).map(String::as_str).unwrap_or("packs"))),
        Some("unpack")|Some("rgba8") if args.len()==4=>unpack(Path::new(&args[2]),Path::new(&args[3]),args[1]=="rgba8"),
        Some("verify") if args.len()==4=>{let input=load(Path::new(&args[3]))?;let decoded=decode_host(&fs::read(&args[2])?)?;verify(&decoded,&input)?;println!("All {} frames EXACT",input.names.len());Ok(())},
        _=>Err("usage: lily-pack pack SOURCE OUTPUT | bench [LilyDS] [packs] | load PACK | verify PACK SOURCE | unpack PACK DIRECTORY | rgba8 PACK DIRECTORY".into()),
    }
}
