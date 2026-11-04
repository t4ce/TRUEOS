#!/usr/bin/env python3
"""Run the production Conway reference tests; optionally time its CPU fallback.

No graphics hardware or Cargo dependencies are needed. Timing is a local host
measurement, not a native Intel GPU or baremetal performance result.
"""

import argparse
import json
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def compile_fixture(directory: Path, body: str, name: str, test: bool = False) -> Path:
    source = directory / (name + ".rs")
    source.write_text("extern crate alloc;\n"
        + "mod r { pub mod services { pub mod shell3_font_service {\n#[path="
        + json.dumps(str(ROOT / "src/r/services/shell3_hex_font.rs")) + "] pub mod hex;\n}}}\n#[path="
        + json.dumps(str(ROOT / "src/ui4/edit_life_model.rs"))
        + "] mod model;\n" + body)
    binary = directory / name
    command = ["rustc", "--edition=2024", "--target=x86_64-unknown-linux-gnu", "-O",
        "-Adead_code", "-C", "linker=" + str(ROOT / "tools/rust-host-ld"), str(source), "-o", str(binary)]
    if test:
        command.append("--test")
    subprocess.run(command, cwd=ROOT, check=True)
    return binary


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--benchmark", action="store_true")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="trueos-edit-life-model-") as temporary:
        directory = Path(temporary)
        tests = compile_fixture(directory, "", "tests", test=True)
        subprocess.run([str(tests)], check=True)
        if args.benchmark:
            benchmark = compile_fixture(directory, r'''
fn main() {
    let (width, height) = (1280u32, 720u32);
    let (cell_width, cell_height) = (12u32, 22u32);
    let columns = width.div_ceil(cell_width);
    let mut life = model::Life::new(width, height).unwrap();
    let cells: Vec<[u32; 4]> = (0..columns * height.div_ceil(cell_height)).map(|i| {
        let (glyph, fg) = r::services::shell3_font_service::hex::resolve(
            if i % 17 < 3 { 'A' } else { ' ' }, [200, 220, 240, 255]);
        [glyph, u32::from_le_bytes(fg), 0xff000000, 0]
    }).collect();
    let start = std::time::Instant::now();
    assert!(life.snapshot_hex(&cells, columns, cell_width, cell_height, false, true));
    println!("host_cpu_hex_snapshot_us={} input_bytes={}", start.elapsed().as_micros(), cells.len() * 16);
    let start = std::time::Instant::now();
    for _ in 0..10 { life.step(); }
    println!("host_cpu_step_us={}", start.elapsed().as_micros() / 10);
    let start = std::time::Instant::now();
    let mut checksum = 0u64;
    for y in 0..height {
        for x in 0..width {
            let ink = model::hex_ink(&cells, columns, cell_width, cell_height, x, y);
            checksum += model::composite_background(ink, life.pixel(x, y))[0] as u64;
        }
    }
    println!("host_cpu_display_us={} checksum={checksum}", start.elapsed().as_micros());
}
''', "benchmark")
            subprocess.run([str(benchmark)], check=True)


if __name__ == "__main__":
    main()
