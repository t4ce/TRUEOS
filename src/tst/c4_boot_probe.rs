extern crate alloc;

use alloc::format;
use embassy_time::{Duration as EmbassyDuration, Timer};

const C4_BOOT_PROBE_SOURCE: &str = r#"
{
    int i, sum;
    bool ok;
    int[4] values;

    i = 0;
    sum = 0;
    ok = true;

    while (i < 4) {
        values[i] = i + 1;
        sum = sum + values[i];
        i = i + 1;
    }

    if (ok && (sum == 10)) {
        sum = sum + 32;
    } else {
        sum = 0;
    }
}
"#;

const C4_BOOT_PROBE_SOURCE_PATH: &str = "impossible.c4";
const C4_BOOT_PROBE_RUST_PATH: &str = "impossible.rs";
const C4_BOOT_PROBE_VM_PATH: &str = "impossible.vm";
const C4_BOOT_PROBE_STEP_LIMIT: usize = 100_000;

#[embassy_executor::task]
pub async fn task() {
    crate::r::readiness::wait_for(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED).await;

    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log!("c4-boot-probe: skipped reason=no-primary-root\n");
        return;
    };

    // Let the mount service finish its immediate index warmup before adding our
    // proof artifact.
    Timer::after(EmbassyDuration::from_millis(250)).await;

    let _ = run_disk_proof(disk, "post-root").await;
}

pub(crate) async fn run_disk_proof(
    disk: crate::disc::block::DeviceHandle,
    lane: &'static str,
) -> bool {
    let (rust, vm) = match trueos_c4::parse_program(C4_BOOT_PROBE_SOURCE) {
        Ok(program) => {
            let rust = trueos_c4::emit_rust(&program);
            let vm = match trueos_c4::emit_vm_object(&program) {
                Ok(object) => object,
                Err(err) => {
                    let report = format!("c4 boot probe vm emit failed: {:?}\n", err);
                    let _ = crate::r::fs::trueosfs::file_in_async(
                        disk,
                        C4_BOOT_PROBE_VM_PATH,
                        report.as_bytes(),
                    )
                    .await;
                    crate::log!(
                        "c4-boot-probe: lane={} vm failed path={} err={:?}\n",
                        lane,
                        C4_BOOT_PROBE_VM_PATH,
                        err
                    );
                    return false;
                }
            };
            (rust, vm)
        }
        Err(err) => {
            let report = format!(
                "c4 boot probe parse failed: {} at line {}, column {}\n",
                err.message, err.span.line, err.span.column
            );
            let _ = crate::r::fs::trueosfs::file_in_async(
                disk,
                C4_BOOT_PROBE_RUST_PATH,
                report.as_bytes(),
            )
                .await;
            crate::log!(
                "c4-boot-probe: lane={} parse failed path={} err={}\n",
                lane,
                C4_BOOT_PROBE_RUST_PATH,
                err.message
            );
            return false;
        }
    };

    let (vm_run_ok, vm_steps) = match trueos_c4::run_vm_object(vm.bytes.as_slice(), C4_BOOT_PROBE_STEP_LIMIT) {
        Ok(report) => (true, report.steps),
        Err(err) => {
            crate::log!(
                "c4-boot-probe: lane={} run failed vm_path={} err={:?}\n",
                lane,
                C4_BOOT_PROBE_VM_PATH,
                err
            );
            (false, 0)
        }
    };

    let source_ok = match crate::r::fs::trueosfs::file_in_async(
        disk,
        C4_BOOT_PROBE_SOURCE_PATH,
        C4_BOOT_PROBE_SOURCE.as_bytes(),
    )
    .await
    {
        Ok(ok) => ok,
        Err(err) => {
            crate::log!(
                "c4-boot-probe: lane={} write failed path={} err={:?}\n",
                lane,
                C4_BOOT_PROBE_SOURCE_PATH,
                err
            );
            false
        }
    };

    let vm_ok = match crate::r::fs::trueosfs::file_in_async(
        disk,
        C4_BOOT_PROBE_VM_PATH,
        vm.bytes.as_slice(),
    )
    .await
    {
        Ok(ok) => ok,
        Err(err) => {
            crate::log!(
                "c4-boot-probe: lane={} write failed path={} err={:?}\n",
                lane,
                C4_BOOT_PROBE_VM_PATH,
                err
            );
            false
        }
    };

    match crate::r::fs::trueosfs::file_in_async(disk, C4_BOOT_PROBE_RUST_PATH, rust.as_bytes())
        .await
    {
        Ok(true) => {
            crate::log!(
                "c4-boot-probe: lane={} accepted=1 source_path={} source_ok={} rust_path={} rust_bytes={} vm_path={} vm_ok={} vm_bytes={} vm_code={} vm_symbols={} vm_stack={} vm_run_ok={} vm_steps={}\n",
                lane,
                C4_BOOT_PROBE_SOURCE_PATH,
                if source_ok { 1 } else { 0 },
                C4_BOOT_PROBE_RUST_PATH,
                rust.len(),
                C4_BOOT_PROBE_VM_PATH,
                if vm_ok { 1 } else { 0 },
                vm.bytes.len(),
                vm.code_len,
                vm.symbol_count,
                vm.stack_bytes,
                if vm_run_ok { 1 } else { 0 },
                vm_steps
            );
            source_ok && vm_ok && vm_run_ok
        }
        Ok(false) => {
            crate::log!(
                "c4-boot-probe: lane={} accepted=0 path={} reason=write-returned-false\n",
                lane,
                C4_BOOT_PROBE_RUST_PATH
            );
            false
        }
        Err(err) => {
            crate::log!(
                "c4-boot-probe: lane={} accepted=0 path={} err={:?}\n",
                lane,
                C4_BOOT_PROBE_RUST_PATH,
                err
            );
            false
        }
    }
}
