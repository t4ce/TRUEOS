use std::{env, fmt::Write as _, fs, process};

use trueos_kokoro_aot::{Program, SECTION_COUNT};

fn hash_hex(hash: &[u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in hash {
        write!(&mut output, "{byte:02x}").expect("String writes cannot fail");
    }
    output
}

fn usage() -> ! {
    eprintln!("usage: inspect <artifact.kkaot> [frame_count]");
    process::exit(2)
}

fn main() {
    let mut args = env::args().skip(1);
    let path = args.next().unwrap_or_else(|| usage());
    let frame_count = args
        .next()
        .map(|value| value.parse::<u32>().unwrap_or_else(|_| usage()));
    if args.next().is_some() {
        usage();
    }
    let artifact = fs::read(&path).unwrap_or_else(|error| {
        eprintln!("inspect: cannot read {path}: {error}");
        process::exit(1);
    });
    let program = Program::parse(&artifact).unwrap_or_else(|error| {
        eprintln!("inspect: rejected {path}: {error:?}");
        process::exit(1);
    });

    println!("artifact_bytes={}", artifact.len());
    println!("payload_sha256={}", hash_hex(program.payload_sha256()));
    println!("model_sha256={}", hash_hex(program.model_sha256()));
    println!("voices_sha256={}", hash_hex(program.voices_sha256()));
    for index in 0..SECTION_COUNT {
        let section = program.sections()[index];
        println!(
            "section[{index}]={:?} offset={} count={} stride={} bytes={}",
            section.kind, section.offset, section.count, section.stride, section.bytes
        );
    }
    for phase in program.phases() {
        println!(
            "phase={:?} ops={}..{} arena={}..{} frames={}..{} runtime_sized={}",
            phase.phase,
            phase.op_start,
            phase.op_end,
            phase.arena_min_bytes,
            phase.arena_max_bytes,
            phase.frame_count_min,
            phase.frame_count_max,
            phase.runtime_sized
        );
    }

    if let Some(frame_count) = frame_count {
        let mut bases = vec![0u64; program.slot_count() as usize];
        let plan = program
            .resolve_phase_two(frame_count, &mut bases)
            .unwrap_or_else(|error| {
                eprintln!("inspect: frame_count={frame_count} rejected: {error:?}");
                process::exit(1);
            });
        println!("resolved_frame_count={}", plan.frame_count());
        println!("resolved_arena_bytes={}", plan.arena_bytes());
        for slot in 0..program.slot_count() {
            println!("slot[{slot}].base={:?}", plan.slot_base(slot));
        }
    }
}
