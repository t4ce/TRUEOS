use std::{env, fs, path::Path};

use naga::{
    back::spv::{Options, PipelineOptions},
    valid::{Capabilities, ValidationFlags, Validator},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let entry = args.next().ok_or("usage: trueos-wgsl-spv ENTRY INPUT OUTPUT")?;
    let input = args.next().ok_or("usage: trueos-wgsl-spv ENTRY INPUT OUTPUT")?;
    let output = args.next().ok_or("usage: trueos-wgsl-spv ENTRY INPUT OUTPUT")?;
    if args.next().is_some() {
        return Err("usage: trueos-wgsl-spv ENTRY INPUT OUTPUT".into());
    }
    let entry = entry.into_string().map_err(|_| "entry point is not UTF-8")?;
    let source = fs::read_to_string(&input)?;
    let module = naga::front::wgsl::parse_str(&source).map_err(|error| {
        error.emit_to_string_with_path(&source, Path::new(&input))
    })?;
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .map_err(|error| format!("WGSL validation failed: {error}"))?;
    let shader_stage = module
        .entry_points
        .iter()
        .find(|point| point.name == entry)
        .ok_or("entry point not found")?
        .stage;
    let words = naga::back::spv::write_vec(
        &module,
        &info,
        &Options::default(),
        Some(&PipelineOptions { shader_stage, entry_point: entry }),
    )?;
    let bytes = words
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect::<Vec<_>>();
    fs::write(output, bytes)?;
    Ok(())
}
