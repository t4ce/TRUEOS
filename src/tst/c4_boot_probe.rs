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
                        "c4-boot-probe: vm failed path={} err={:?}\n",
                        C4_BOOT_PROBE_VM_PATH,
                        err
                    );
                    return;
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
                "c4-boot-probe: parse failed path={} err={}\n",
                C4_BOOT_PROBE_RUST_PATH,
                err.message
            );
            return;
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
                "c4-boot-probe: write failed path={} err={:?}\n",
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
                "c4-boot-probe: write failed path={} err={:?}\n",
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
                "c4-boot-probe: accepted=1 source_path={} source_ok={} rust_path={} rust_bytes={} vm_path={} vm_ok={} vm_bytes={} vm_code={} vm_symbols={} vm_stack={}\n",
                C4_BOOT_PROBE_SOURCE_PATH,
                if source_ok { 1 } else { 0 },
                C4_BOOT_PROBE_RUST_PATH,
                rust.len(),
                C4_BOOT_PROBE_VM_PATH,
                if vm_ok { 1 } else { 0 },
                vm.bytes.len(),
                vm.code_len,
                vm.symbol_count,
                vm.stack_bytes
            );
        }
        Ok(false) => {
            crate::log!(
                "c4-boot-probe: accepted=0 path={} reason=write-returned-false\n",
                C4_BOOT_PROBE_RUST_PATH
            );
        }
        Err(err) => {
            crate::log!(
                "c4-boot-probe: accepted=0 path={} err={:?}\n",
                C4_BOOT_PROBE_RUST_PATH,
                err
            );
        }
    }
}
