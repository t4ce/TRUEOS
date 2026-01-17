use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=src/audio/demo.mp3");

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let mp3_path = manifest_dir.join("src").join("audio").join("demo.mp3");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    if !mp3_path.exists() {
        println!(
            "cargo:warning=demo.mp3 missing at {} (embedding empty audio)",
            mp3_path.display()
        );
        write_generated(&out_dir, None).expect("write generated");
        return;
    }

    let mp3_bytes = fs::read(&mp3_path).expect("read demo.mp3");
    let mut decoder = minimp3::Decoder::new(std::io::Cursor::new(mp3_bytes));

    let mut sample_rate_hz: Option<u32> = None;
    let mut channels: Option<u16> = None;
    let mut pcm_i16: Vec<i16> = Vec::new();

    loop {
        match decoder.next_frame() {
            Ok(frame) => {
                if sample_rate_hz.is_none() {
                    sample_rate_hz = Some(frame.sample_rate as u32);
                }
                if channels.is_none() {
                    channels = Some(frame.channels as u16);
                }
                pcm_i16.extend_from_slice(&frame.data);
            }
            Err(minimp3::Error::Eof) => break,
            Err(e) => {
                println!("cargo:warning=demo.mp3 decode error: {e:?} (embedding decoded prefix)");
                break;
            }
        }
    }

    let sample_rate_hz = sample_rate_hz.unwrap_or(44_100);
    let channels = channels.unwrap_or(2);

    let pcm_path = out_dir.join("demo.pcm_s16le");
    write_pcm_s16le(&pcm_path, &pcm_i16).expect("write pcm");

    write_generated(
        &out_dir,
        Some(GeneratedMeta {
            sample_rate_hz,
            channels,
            pcm_path,
        }),
    )
    .expect("write generated");
}

struct GeneratedMeta {
    sample_rate_hz: u32,
    channels: u16,
    pcm_path: PathBuf,
}

fn write_pcm_s16le(path: &Path, samples: &[i16]) -> std::io::Result<()> {
    let mut f = fs::File::create(path)?;
    for &s in samples {
        f.write_all(&s.to_le_bytes())?;
    }
    Ok(())
}

fn write_generated(out_dir: &Path, meta: Option<GeneratedMeta>) -> std::io::Result<()> {
    let rs_path = out_dir.join("demo_mp3.rs");
    let mut f = fs::File::create(&rs_path)?;

    match meta {
        None => {
            writeln!(
                f,
                "pub const DEMO: super::DemoPcm = super::DemoPcm {{ sample_rate_hz: 44_100, channels: 2, samples_interleaved_i16: &[] }};"
            )?;
        }
        Some(meta) => {
            let pcm_path = meta.pcm_path;
            writeln!(f, "#[repr(align(2))]")?;
            writeln!(f, "pub struct AlignedBytes<const N: usize>(pub [u8; N]);")?;
            writeln!(
                f,
                "pub static PCM_BYTES: AlignedBytes<{{ include_bytes!(\"{}\").len() }}> = AlignedBytes(*include_bytes!(\"{}\"));",
                pcm_path.display(),
                pcm_path.display()
            )?;
            writeln!(f, "const fn samples_from_bytes(bytes: &'static [u8]) -> &'static [i16] {{")?;
            writeln!(f, "    let len = bytes.len() / 2;")?;
            writeln!(f, "    unsafe {{ core::slice::from_raw_parts(bytes.as_ptr() as *const i16, len) }}")?;
            writeln!(f, "}}")?;
            writeln!(
                f,
                "pub const DEMO: super::DemoPcm = super::DemoPcm {{ sample_rate_hz: {}, channels: {}, samples_interleaved_i16: samples_from_bytes(&PCM_BYTES.0) }};",
                meta.sample_rate_hz, meta.channels
            )?;
        }
    }

    Ok(())
}
