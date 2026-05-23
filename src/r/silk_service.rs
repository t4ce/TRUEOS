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

const PLACE_SOURCE: &str = r#"
# Tiny boot-time placement plan.
place demo.art in main align 16
place arena.art in main align 16
place path.art in main align 8
place buf.art in main align 16
place ring.art in main align 16
"#;

enum SilkServiceError {
    Parse(trueos_silk::ParseError),
    PlaceParse(trueos_silk::PlacementError),
    MissingArtifact,
    MissingPlacementArtifact,
    UnknownPlacementArena,
    RingArtifact(trueos_silk::SilkStatus),
    RingPlace(trueos_silk::SilkStatus),
    RingBind(trueos_silk::SilkStatus),
    RingOp(trueos_silk::SilkStatus),
}

impl core::fmt::Display for SilkServiceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Parse(err) => write!(f, "parse: {:?}", err),
            Self::PlaceParse(err) => write!(f, "place parse: {:?}", err),
            Self::MissingArtifact => f.write_str("missing in-memory artifact"),
            Self::MissingPlacementArtifact => f.write_str("missing placement artifact"),
            Self::UnknownPlacementArena => f.write_str("unknown placement arena"),
            Self::RingArtifact(status) => write!(f, "ring artifact: {:?}", status),
            Self::RingPlace(status) => write!(f, "ring place: {:?}", status),
            Self::RingBind(status) => write!(f, "ring bind: {:?}", status),
            Self::RingOp(status) => write!(f, "ring op: {:?}", status),
        }
    }
}

struct Artifact<'a> {
    name: &'static str,
    bytes: &'a [u8],
}

fn artifact_bytes<'a>(artifacts: &'a [Artifact<'a>], name: &str) -> Option<&'a [u8]> {
    artifacts
        .iter()
        .find(|artifact| artifact.name == name)
        .map(|artifact| artifact.bytes)
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

fn ring_artifact() -> Result<String, SilkServiceError> {
    let result = trueos_silk::RingArtifact::u8("ring.art", 8);
    if result.status != trueos_silk::SilkStatus::Ok {
        return Err(SilkServiceError::RingArtifact(result.status));
    }

    let layout = result.artifact.layout;
    Ok(format!(
        "artifact {} kind=ring.u8 header={} data_offset={} capacity={} total_len={} align={}\nops=bind,push,pop,validate\n",
        result.artifact.name,
        layout.header_len,
        layout.data_offset,
        layout.capacity,
        layout.total_len,
        layout.align
    ))
}

fn ring_runtime_demo(plan: &trueos_silk::Plan) -> Result<String, SilkServiceError> {
    let result = trueos_silk::RingArtifact::u8("ring.art", 8);
    if result.status != trueos_silk::SilkStatus::Ok {
        return Err(SilkServiceError::RingArtifact(result.status));
    }

    let artifact = result.artifact;
    let mut arena = trueos_silk::Arena::new(0x2000, plan.arena.size);
    let placed = arena.alloc_aligned(artifact.layout.total_len, artifact.layout.align);
    if placed.status != trueos_silk::SilkStatus::Ok {
        return Err(SilkServiceError::RingPlace(placed.status));
    }

    let mut data = [0u8; 8];
    let mut ring = trueos_silk::RingBinding::bind(artifact, placed.span, &mut data)
        .map_err(SilkServiceError::RingBind)?;
    let start = ring.validate().map_err(SilkServiceError::RingOp)?;
    ring.push(b'A').map_err(SilkServiceError::RingOp)?;
    ring.push(b'B').map_err(SilkServiceError::RingOp)?;
    let popped = ring.pop().map_err(SilkServiceError::RingOp)?;
    let end = ring.validate().map_err(SilkServiceError::RingOp)?;

    Ok(format!(
        "ring.art runtime span=0x{:x}+{} start={:?} push=[A,B] pop={} end={:?} remaining={}\n",
        placed.span.addr,
        placed.span.len,
        start,
        popped as char,
        end,
        arena.remaining()
    ))
}

fn place_artifacts(
    plan: &trueos_silk::Plan,
    artifacts: &[Artifact<'_>],
) -> Result<String, SilkServiceError> {
    let placement =
        trueos_silk::parse_placement_program(PLACE_SOURCE).map_err(SilkServiceError::PlaceParse)?;
    let mut arena = trueos_silk::Arena::new(0x1000, plan.arena.size);
    let mut report =
        format!("placement arena={} base=0x{:x} len={}\n", plan.arena.name, arena.base, arena.len);

    for step in &placement.steps {
        if step.arena != plan.arena.name {
            return Err(SilkServiceError::UnknownPlacementArena);
        }
        let bytes = artifact_bytes(artifacts, step.artifact.as_str())
            .ok_or(SilkServiceError::MissingPlacementArtifact)?;
        let placed = arena.alloc_aligned(bytes.len() as u64, step.align);
        report.push_str(
            format!(
                "place {} status={:?} addr=0x{:x} len={} align={} remaining={}\n",
                step.artifact,
                placed.status,
                placed.span.addr,
                placed.span.len,
                step.align,
                arena.remaining()
            )
            .as_str(),
        );
    }

    Ok(report)
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
    let ring = ring_artifact()?;
    let artifacts = [
        Artifact {
            name: "demo.art",
            bytes: demo.as_bytes(),
        },
        Artifact {
            name: "arena.art",
            bytes: arena.as_bytes(),
        },
        Artifact {
            name: "path.art",
            bytes: path.as_bytes(),
        },
        Artifact {
            name: "buf.art",
            bytes: buf.as_bytes(),
        },
        Artifact {
            name: "ring.art",
            bytes: ring.as_bytes(),
        },
    ];
    let placement = place_artifacts(&plan, &artifacts)?;
    let ring_runtime = ring_runtime_demo(&plan)?;

    crate::log!(
        "silk-service: built in-memory artifacts demo={} arena={} path={} buf={} ring={} placement={} ring_runtime={}\n",
        demo.len(),
        arena.len(),
        path.len(),
        buf.len(),
        ring.len(),
        placement.len(),
        ring_runtime.len()
    );
    crate::log!("silk-service: placement begin\n{}silk-service: placement end\n", placement);
    crate::log!("silk-service: ring begin\n{}silk-service: ring end\n", ring_runtime);
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
