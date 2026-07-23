use alloc::string::String;
use core::fmt::Write;

use embassy_executor::Spawner;

use crate::shell2::shell2_cmd::ParseOutcome;
use crate::shell2::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "lumen: usage `lumen hello` or `lumen decode <token>`");
}

enum Command {
    Hello,
    Decode(u32),
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    let command = match (args.next(), args.next(), args.next()) {
        (Some("hello"), None, None) => Command::Hello,
        (Some("decode"), Some(token), None) => {
            let Ok(token) = token.parse::<u32>() else {
                usage(io);
                return ParseOutcome::Handled;
            };
            if token >= trueos_fpga_abi::lfm25::MODEL_VOCABULARY_SIZE {
                print_shell_line(io, "lumen: token outside the sealed 65536-row vocabulary");
                return ParseOutcome::Handled;
            }
            Command::Decode(token)
        }
        (Some("help" | "-h" | "--help"), None, None) => {
            usage(io);
            return ParseOutcome::Handled;
        }
        _ => {
            usage(io);
            return ParseOutcome::Handled;
        }
    };

    if !crate::tga::is_online() {
        print_shell_line(io, "lumen: truega backend unavailable; admitted firmware is offline");
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    set_matrix_target_active(&target, true);
    let spawned = match command {
        Command::Hello => match lumen_hello_task(target.clone()) {
            Ok(task) => {
                spawner.spawn(task);
                true
            }
            Err(_) => false,
        },
        Command::Decode(token) => {
            let complete_firmware =
                crate::r::fpga_offload::lfm25_decode_transport_available()
                    && crate::r::fpga_offload::lfm25_feed_transport_available();
            let hybrid_cpu = crate::r::fpga_offload::lfm25_row_stream_available();
            if !complete_firmware && !hybrid_cpu {
                set_matrix_target_active(&target, false);
                print_shell_line(
                    io,
                    "lumen: decode unavailable; neither TGD1+TGF2 nor the BAR2/MSI FFN fallback is admitted",
                );
                return ParseOutcome::Handled;
            }
            match lumen_decode_task(target.clone(), token) {
                Ok(task) => {
                    spawner.spawn(task);
                    true
                }
                Err(_) => false,
            }
        }
    };
    if !spawned {
        set_matrix_target_active(&target, false);
        print_shell_line(io, "lumen: async module task unavailable");
    }
    ParseOutcome::Handled
}

#[embassy_executor::task]
async fn lumen_decode_task(target: MatrixTarget, token: u32) {
    let complete_firmware =
        crate::r::fpga_offload::lfm25_decode_transport_available()
            && crate::r::fpga_offload::lfm25_feed_transport_available();
    let backend_name = if complete_firmware {
        "truega"
    } else {
        "hybrid-cpu+truega-ffn"
    };
    print_matrix_target_line(
        &target,
        alloc::format!(
            "lumen: decode=start token={} position=0 plan_ops={} backend={} model=sealed-native-image",
            token,
            trueos_fpga_abi::lfm25_decode::OPS_PER_TOKEN,
            backend_name,
        )
        .as_str(),
    );
    let start_tick = embassy_time_driver::now();
    let before = crate::r::fpga_offload::stats();
    let result = if complete_firmware {
        match crate::lumen::decode::open_truega().await {
            Ok(module) => ::lumen::async_module::forward(
                &module,
                crate::lumen::decode::Lfm25DecodeInput::new(token),
            )
            .await
            .map_err(|error| alloc::format!("{error:?}")),
            Err(error) => Err(alloc::format!("model-open={error:?}")),
        }
    } else {
        match crate::lumen::decode::open_hybrid_cpu().await {
            Ok(module) => ::lumen::async_module::forward(
                &module,
                crate::lumen::decode::Lfm25DecodeInput::new(token),
            )
            .await
            .map_err(|error| alloc::format!("{error:?}")),
            Err(error) => Err(alloc::format!("model-open={error:?}")),
        }
    };
    let after = crate::r::fpga_offload::stats();

    match result {
        Ok(output) => {
            print_matrix_target_line(
                &target,
                alloc::format!(
                    "lumen: decode=pass input_token={} output_token={} score_q30={} position={} callbacks={} irq_delta={} timeout_recovery_delta={} elapsed_ms={}",
                    token,
                    output.token,
                    output.score_q30,
                    output.input_position,
                    output.callback_sequence,
                    after.interrupts.saturating_sub(before.interrupts),
                    after
                        .timeout_recoveries
                        .saturating_sub(before.timeout_recoveries),
                    elapsed_ms_since(start_tick),
                )
                .as_str(),
            );
            print_matrix_target_line(
                &target,
                if complete_firmware {
                    "lumen: decode path=async-module->fixed-99-op-plan->sealed-range-feed->bar2->msi-worker-callback"
                } else {
                    "lumen: decode path=async-module->fixed-99-op-plan->cpu-mixers+truega-ffn->msi-worker-callback"
                },
            );
        }
        Err(error) => print_matrix_target_line(
            &target,
            alloc::format!("lumen: decode=fail error={error}").as_str(),
        ),
    }
    set_matrix_target_active(&target, false);
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
                    digest_hex(&report.output_sha256),
                    report.fpga_calls,
                    report.interrupt_delta,
                    report.timeout_recovery_delta,
                )
                .as_str(),
            );
            print_matrix_target_line(
                &target,
                "lumen: hello path=async-module->generated-aot-op->truega-backend->bar2-row-stream completion=msi",
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
