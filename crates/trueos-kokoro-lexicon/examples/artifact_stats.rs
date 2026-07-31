use std::{env, fs, process, time::Instant};

use trueos_kokoro_lexicon::{Lexicon, PINNED_US_PATH};

fn main() {
    let mut arguments = env::args().skip(1);
    let path = arguments
        .next()
        .unwrap_or_else(|| PINNED_US_PATH.to_owned());
    let bytes = fs::read(&path).unwrap_or_else(|error| {
        eprintln!("failed to read {path}: {error}");
        process::exit(1);
    });
    let started = Instant::now();
    let lexicon = Lexicon::parse_pinned_us(&bytes).unwrap_or_else(|error| {
        eprintln!("failed to parse {path}: {error:?}");
        process::exit(1);
    });
    println!(
        "load_ms={:.3} entries={} variants={} resident_bytes={}",
        started.elapsed().as_secs_f64() * 1_000.0,
        lexicon.entry_count(),
        lexicon.variant_count(),
        lexicon.resident_bytes(),
    );
    for word in arguments {
        println!("{word}\t{}", lexicon.get(&word).unwrap_or("<missing>"));
    }
}
