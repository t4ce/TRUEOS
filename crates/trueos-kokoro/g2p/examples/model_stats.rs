use std::{env, fs, process, time::Instant};

use trueos_kokoro_g2p::{Model, PINNED_ENGLISH_PATH};

fn main() {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| PINNED_ENGLISH_PATH.to_owned());
    let bytes = fs::read(&path).unwrap_or_else(|error| {
        eprintln!("failed to read {path}: {error}");
        process::exit(1);
    });
    let started = Instant::now();
    let model = Model::parse_pinned_english(&bytes).unwrap_or_else(|error| {
        eprintln!("failed to parse {path}: {error:?}");
        process::exit(1);
    });
    let elapsed = started.elapsed();
    let memory = model.memory_usage();
    println!(
        "load_ms={:.3} tokens={} ngrams={} backoffs={} lexicon={} borrowed_bytes={} index_bytes={} allocations={}",
        elapsed.as_secs_f64() * 1_000.0,
        model.token_count(),
        model.ngram_count(),
        model.backoff_count(),
        model.lexicon_count(),
        memory.borrowed_model_bytes,
        memory.allocated_index_bytes,
        memory.contiguous_allocations,
    );
}
