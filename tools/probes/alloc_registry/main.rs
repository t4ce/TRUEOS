//! Exercise C allocation tracking beyond its 65,536-record capacity.
use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::Instant;

const LIVE: usize = 70_000;
const BATCH: usize = 256;

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

struct Block {
    ptr: NonNull<u8>,
    size: usize,
    witness: u8,
}

impl Block {
    fn new(index: usize) -> Self {
        let size = 16 + index % 5 * 16;
        let ptr = NonNull::new(unsafe { malloc(size) }.cast::<u8>()).expect("malloc");
        let witness = (index % 251 + 1) as u8;
        unsafe { ptr.as_ptr().write_bytes(witness, size) };
        Self { ptr, size, witness }
    }

    fn check(&self) {
        assert!(unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.size) }
            .iter().all(|byte| *byte == self.witness), "allocation contents");
    }

    fn resize(&mut self, size: usize) {
        self.check();
        let ptr = NonNull::new(unsafe { realloc(self.ptr.as_ptr().cast(), size) }.cast::<u8>())
            .expect("realloc");
        let preserved = self.size.min(size);
        // The successful realloc transfers ownership before any assertion.
        self.ptr = ptr;
        self.size = size;
        assert!(unsafe { std::slice::from_raw_parts(ptr.as_ptr(), preserved) }
            .iter().all(|byte| *byte == self.witness), "realloc contents");
        unsafe { ptr.as_ptr().write_bytes(self.witness, size) };
    }
}

impl Drop for Block {
    fn drop(&mut self) {
        unsafe { free(self.ptr.as_ptr().cast()) };
    }
}

fn record(phase: &'static str, start: Instant, operations: usize, max_batch_us: u128) {
    let args = format_args!(
        "alloc_registry: {phase} live={LIVE} ops={operations} elapsed_us={} max_batch_us={max_batch_us} batch={BATCH}",
        start.elapsed().as_micros()
    );
    #[cfg(target_os = "trueos")]
    trueos::logl::log(trueos::logl::level::INFO, args);
    #[cfg(not(target_os = "trueos"))]
    println!("{args}");
}

fn main() {
    let total = Instant::now();
    record("BEGIN", total, 0, 0);
    let mut blocks = Vec::with_capacity(LIVE);
    let start = Instant::now();
    let mut batch = start;
    let mut max_batch = 0;
    for index in 0..LIVE {
        blocks.push(Some(Block::new(index)));
        if (index + 1) % BATCH == 0 {
            max_batch = max_batch.max(batch.elapsed().as_micros());
            batch = Instant::now();
        }
        if (index + 1) % 16_384 == 0 {
            record("FILL_PROGRESS", start, index + 1, max_batch);
        }
    }
    record("FILLED", start, LIVE, max_batch);

    let start = Instant::now();
    // Include late allocations beyond capacity, plus early and interior ones.
    let mut operations = 0;
    for index in (0..LIVE).step_by(257).chain((65_536..LIVE).step_by(17)) {
        let block = blocks[index].as_mut().unwrap();
        block.resize(113);
        block.resize(24);
        operations += 2;
    }
    record("REALLOC_AT_CAPACITY", start, operations, 0);

    let start = Instant::now();
    for index in (0..LIVE).step_by(2) {
        let block = blocks[index].take().unwrap();
        block.check();
        drop(block);
    }
    record("HALF_FREED", start, LIVE / 2, 0);
    let start = Instant::now();
    for index in (0..LIVE).step_by(2) {
        blocks[index] = Some(Block::new(index));
    }
    record("REFILLED", start, LIVE / 2, 0);

    let start = Instant::now();
    for index in (0..LIVE).step_by(113) {
        blocks[index].as_mut().unwrap().resize(97);
    }
    record("REALLOC_AFTER_CHURN", start, LIVE.div_ceil(113), 0);
    let start = Instant::now();
    while let Some(block) = blocks.pop() {
        let block = block.unwrap();
        block.check();
        drop(block);
    }
    record("FREED", start, LIVE, 0);
    record("ALL_PASS", total, LIVE * 3 + operations + LIVE.div_ceil(113), 0);
}
