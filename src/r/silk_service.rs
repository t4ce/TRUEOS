extern crate alloc;

use alloc::format;
use alloc::string::String;

use embassy_time::{Duration as EmbassyDuration, Timer};

const DEMO_SOURCE: &str = r#"
# Tiny boot-time Silk plan.
arena main 64k
path = const "silk/demo.art"
buf = fs.read path using main
log.write buf
"#;

enum SilkServiceError {
    Parse(trueos_silk::ParseError),
    MissingArtifact,
}

impl core::fmt::Display for SilkServiceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Parse(err) => write!(f, "parse: {:?}", err),
            Self::MissingArtifact => f.write_str("missing in-memory artifact"),
        }
    }
}

fn demo_artifact(plan: &trueos_silk::Plan) -> String {
    format!(
        "TRUEOS Silk demo\narena={}:{} path={} read={} log={}\n\n{}",
        plan.arena.name,
        plan.arena.size,
        plan.path.value,
        plan.read.name,
        plan.log.source,
        DEMO_SOURCE.trim_start()
    )
}

fn arena_artifact(plan: &trueos_silk::Plan, read_len: usize) -> String {
    let mut arena = trueos_silk::Arena::new(0x1000, plan.arena.size);
    let header = arena.alloc_aligned(64, 16);
    let body = arena.alloc(read_len as u64);
    format!(
        "arena name={} base=0x{:x} len={} header={:?}@0x{:x}+{} body={:?}@0x{:x}+{} remaining={}\n",
        plan.arena.name,
        arena.base,
        arena.len,
        header.status,
        header.span.addr,
        header.span.len,
        body.status,
        body.span.addr,
        body.span.len,
        arena.remaining()
    )
}

fn path_artifact(plan: &trueos_silk::Plan) -> String {
    format!(
        "const {} = {:?}\nfs.read {} using arena {}\n",
        plan.path.name, plan.path.value, plan.read.path, plan.read.arena
    )
}

fn buf_artifact(plan: &trueos_silk::Plan, bytes: &[u8]) -> String {
    let span = trueos_silk::Span::checked(0, bytes.len() as u64, plan.arena.size);
    let text = core::str::from_utf8(bytes).unwrap_or("<non-utf8>");
    format!(
        "buffer {} status={:?} addr={} len={} bound={}\n\n{}",
        plan.read.name, span.status, span.span.addr, span.span.len, plan.arena.size, text
    )
}

async fn build_and_load_artifacts() -> Result<(), SilkServiceError> {
    let plan = trueos_silk::parse_plan(DEMO_SOURCE).map_err(SilkServiceError::Parse)?;
    let demo = demo_artifact(&plan);

    Timer::after(EmbassyDuration::from_millis(1)).await;

    let read_bytes = if plan.path.value == "silk/demo.art" {
        demo.as_bytes()
    } else {
        return Err(SilkServiceError::MissingArtifact);
    };

    let arena = arena_artifact(&plan, read_bytes.len());
    let path = path_artifact(&plan);
    let buf = buf_artifact(&plan, read_bytes);

    crate::log!(
        "silk-service: built in-memory artifacts demo={} arena={} path={} buf={}\n",
        demo.len(),
        arena.len(),
        path.len(),
        buf.len()
    );
    crate::log!(
        "silk-service: log.write {} bytes from {}\n",
        read_bytes.len(),
        plan.path.value.as_str()
    );
    match core::str::from_utf8(read_bytes) {
        Ok(text) => {
            crate::log!("silk-service: log.write begin\n{}\nsilk-service: log.write end\n", text)
        }
        Err(_) => crate::log!("silk-service: log.write non-utf8 len={}\n", read_bytes.len()),
    }

    Timer::after(EmbassyDuration::from_millis(1)).await;
    Ok(())
}

#[embassy_executor::task]
pub async fn silk_service_task() {
    match build_and_load_artifacts().await {
        Ok(()) => crate::log!("silk-service: in-memory demo loaded\n"),
        Err(err) => crate::log!("silk-service: in-memory demo failed: {}\n", err),
    }
}
