use alloc::string::String;
use core::fmt::Write;

use embassy_executor::Spawner;

use crate::shell2::shell2_cmd::ParseOutcome;
use crate::shell2::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "lumen: usage `lumen hello`");
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    match (args.next(), args.next()) {
        (Some("hello"), None) => {}
        (Some("help" | "-h" | "--help"), None) => {
            usage(io);
            return ParseOutcome::Handled;
        }
        _ => {
            usage(io);
            return ParseOutcome::Handled;
        }
    }

    if !crate::tga::is_online() {
        print_shell_line(io, "lumen: truega backend unavailable; admitted firmware is offline");
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    set_matrix_target_active(&target, true);
    match lumen_hello_task(target.clone()) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_shell_line(io, "lumen: async module task unavailable");
        }
    }
    ParseOutcome::Handled
}

#[embassy_executor::task]
async fn lumen_hello_task(target: MatrixTarget) {
    use crate::r::lfm25_ffn;

    print_matrix_target_line(
        &target,
        alloc::format!(
            "lumen: hello=start module=lfm25.layer0.ffn input=sealed-vector0 backend=truega expected_calls={}",
            lfm25_ffn::expected_fpga_calls(),
        )
        .as_str(),
    );
    let start_tick = embassy_time_driver::now();

    match crate::lumen::hello().await {
        Ok(output) => {
            let tensor = output.tensor();
            let report = output.report();
            let values = tensor.as_slice();
            let first = values.first().copied().unwrap_or_default();
            let last = values.last().copied().unwrap_or_default();
            print_matrix_target_line(
                &target,
                alloc::format!(
                    "lumen: hello=pass module=lfm25.layer0.ffn output_shape=[{}] q30_first={} q30_last={} elapsed_ms={}",
                    tensor.shape()[0],
                    first,
                    last,
                    elapsed_ms_since(start_tick),
                )
                .as_str(),
            );
            print_matrix_target_line(
                &target,
                alloc::format!(
                    "lumen: hello output_q30_sha256={} calls={} irq_delta={} timeout_recovery_delta={}",
                    digest_hex(&report.down_sha256),
                    report.fpga_calls,
                    report.interrupt_delta,
                    report.timeout_recovery_delta,
                )
                .as_str(),
            );
            print_matrix_target_line(
                &target,
                "lumen: hello path=async-module->truega-backend->bar2-row-stream completion=msi",
            );
        }
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("lumen: hello=fail error={error:?}").as_str(),
            );
        }
    }

    set_matrix_target_active(&target, false);
}

fn digest_hex(digest: &[u8; 32]) -> String {
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

fn elapsed_ms_since(start_tick: u64) -> u64 {
    embassy_time_driver::now()
        .saturating_sub(start_tick)
        .saturating_mul(1_000)
        / embassy_time_driver::TICK_HZ
}
