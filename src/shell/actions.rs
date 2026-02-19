use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use alloc::boxed::Box;

use super::cube::{CubeState, WireShape};
use super::{CommandAction, PendingAction, ShellBackend, ShellMode};

pub(super) async fn handle_command_action(
    action: CommandAction,
    mode: &mut ShellMode,
    cube_mode: &mut bool,
    cube: &mut CubeState,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
    spawner: &Spawner,
    history: &mut alloc::vec::Vec<alloc::string::String>,
) {
    match action {
        CommandAction::Pending(pending) => handle_pending(mode, pending),
        CommandAction::ShowInstallDiskTable => {
            let rev = super::output::ReverseOutput::new(io, *term_cols, *term_rows, history);
            super::print_install_disk_table(&rev).await;
        }
        CommandAction::ShowFormatDiskTable => {
            let rev = super::output::ReverseOutput::new(io, *term_cols, *term_rows, history);
            super::print_format_disk_table(&rev).await;
        }
        CommandAction::ShowUpdateDiskTable => {
            let rev = super::output::ReverseOutput::new(io, *term_cols, *term_rows, history);
            super::print_update_disk_table(&rev).await;
        }
        CommandAction::ShowFileMountTable => {
            super::print_trueosfs_mount_table(io).await;
            io.write_str("file: enter mount index or disk id (blank/q cancels)\r\n");
        }
        CommandAction::ShowBenchDiskTable => {
            let rev = super::output::ReverseOutput::new(io, *term_cols, *term_rows, history);
            super::print_bench_disk_table(&rev).await;
            io.write_str("bench: enter TRUEOSFS disk id (blank/q cancels)\r\n");
        }
        CommandAction::ShowNetbenchNicTable => {
            super::print_netbench_nic_table(io).await;
            io.write_str("netbench: enter nic id (blank/q cancels)\r\n");
        }
        CommandAction::Qjs { src } => {
            handle_qjs(io, term_cols, term_rows, spawner, history, src).await
        }
        CommandAction::EnterCube => handle_enter_cube(cube_mode, cube, io, term_cols, term_rows),
        CommandAction::EnterIco => handle_enter_ico(cube_mode, cube, io, term_cols, term_rows),
        CommandAction::EnterGo => handle_enter_go(io).await,
        CommandAction::EnterGoTwo => handle_enter_go_two(io).await,
        CommandAction::EnterRain => handle_enter_rain(cube_mode, io, term_cols, term_rows).await,
        CommandAction::EnterTxtEdt { filename, slot_id } => {
            handle_enter_txt(cube_mode, io, term_cols, term_rows, filename, slot_id).await;
        }
        CommandAction::EnterTetris => {
            handle_enter_tetris(cube_mode, io, term_cols, term_rows).await;
        }
        CommandAction::DoFormat { disc_id } => {
            handle_do_format(mode, io, disc_id).await;
        }
        CommandAction::DoInstall { disc_id } => {
            handle_do_install(mode, io, term_cols, spawner, disc_id).await;
        }
        CommandAction::DoUpdate { disc_id } => {
            handle_do_update(mode, io, term_cols, spawner, disc_id).await;
        }
        CommandAction::RunNetbench { nic_index } => {
            super::bench::run_netbench(io, nic_index, *term_cols, *term_rows, history).await;
            clear_statusbar(io, *term_cols, *term_rows);
            *mode = ShellMode::Idle;
        }
        CommandAction::RunBenchFs { disk_id } => {
            let target = crate::disc::block::device_handles()
                .into_iter()
                .find(|h| h.parent().is_none() && h.id().raw() == disk_id);
            if let Some(handle) = target {
                super::bench::run_bench_fs(io, handle, *term_cols, *term_rows, history).await;
            } else {
                io.write_str("\r\nbench: disk disappeared\r\n");
            }
            clear_statusbar(io, *term_cols, *term_rows);
            *mode = ShellMode::Idle;
        }
        CommandAction::OpenAiChat { first } => {
            Box::pin(crate::shell::cmd::ai::run_ai_wizard(
                io,
                *term_cols,
                *term_rows,
                spawner,
                history,
                first.as_str(),
            ))
            .await;
        }
        CommandAction::None => {}
    }
}

fn handle_pending(mode: &mut ShellMode, pending: PendingAction) {
    match pending {
        PendingAction::AcpiReset | PendingAction::AcpiState(_) => {
            *mode = ShellMode::Wait {
                action: pending,
                deadline: Instant::now() + EmbassyDuration::from_secs(5),
            };
        }
        _ => {
            *mode = ShellMode::Confirm(pending);
        }
    }
}

async fn handle_qjs(
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
    spawner: &Spawner,
    history: &mut alloc::vec::Vec<alloc::string::String>,
    src: heapless::String<192>,
) {
    if trueos_qjs::async_fs::ensure_service_started(spawner) {
        if src.trim().is_empty() {
            super::shellqjs::repl_shell(io, *term_cols, *term_rows, history).await;
        } else {
            super::shellqjs::run(io, src.as_str()).await;
        }
    } else {
        io.write_str("qjs: async fs service unavailable\r\n");
    }
}

fn handle_enter_cube(
    cube_mode: &mut bool,
    cube: &mut CubeState,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
) {
    *cube_mode = true;
    cube.set_shape(WireShape::Cube);
    cube.reset();
    super::enter_cube_mode(io, term_cols, term_rows);
}

fn handle_enter_ico(
    cube_mode: &mut bool,
    cube: &mut CubeState,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
) {
    *cube_mode = true;
    cube.set_shape(WireShape::Icosidodecahedron);
    cube.reset();
    super::enter_cube_mode(io, term_cols, term_rows);
}

async fn handle_enter_go(io: &'static dyn ShellBackend) {
    const GO_CHARS: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
    run_go_animation(io, &GO_CHARS).await;
}

async fn handle_enter_go_two(io: &'static dyn ShellBackend) {
    const GO_TWO_CHARS: [char; 9] = ['⢈', '⡈', '⡐', '⡠', '⣀', '⢄', '⢂', '⢁', '⡁'];
    run_go_animation(io, &GO_TWO_CHARS).await;
}

async fn run_go_animation(io: &'static dyn ShellBackend, chars: &[char]) {
    if chars.is_empty() {
        return;
    }
    let mut go_idx = 0;
    io.write_str(crate::ecma48::HIDE_CURSOR);
    loop {
        if io.read_byte().is_some() {
            break;
        }
        let ch = chars[go_idx];
        go_idx = (go_idx + 1) % chars.len();
        io.write_str("\r");
        super::write_prompt(io);
        io.write_char(ch);
        Timer::after(EmbassyDuration::from_millis(160)).await;
    }
    io.write_str(crate::ecma48::SHOW_CURSOR);
    io.write_str("\r\n");
}

async fn handle_enter_rain(
    cube_mode: &mut bool,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
) {
    *cube_mode = false;
    let cols = *term_cols;
    let rows = *term_rows;
    super::cmd::rain::run(io, cols, rows).await;
    reset_shell_display(io, *term_cols, *term_rows);
}

async fn handle_enter_txt(
    cube_mode: &mut bool,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
    filename: heapless::String<48>,
    slot_id: u8,
) {
    *cube_mode = false;
    let cols = *term_cols;
    let rows = *term_rows;

    if let Some(buf) = crate::matrix::take_blob(slot_id) {
        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Running);
        let out_buf = super::txt::run(io, cols, rows, filename.as_str(), buf).await;
        let _ = crate::matrix::set_blob_owned_with_preview(slot_id, out_buf);
        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
        io.write_fmt(format_args!("\r\ntxt: updated §{}\r\n", slot_id + 1));
        super::refresh_title_bar(io, cols);
    } else {
        io.write_str("\r\ntxt: invalid slot\r\n");
    }

    reset_shell_display(io, *term_cols, *term_rows);
}

async fn handle_enter_tetris(
    cube_mode: &mut bool,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
) {
    *cube_mode = false;
    let cols = *term_cols;
    let rows = *term_rows;
    super::shelltetris::run(io, cols, rows).await;
    reset_shell_display(io, *term_cols, *term_rows);
}

async fn handle_do_format(mode: &mut ShellMode, io: &'static dyn ShellBackend, disc_id: u32) {
    let target = crate::disc::block::device_handles()
        .into_iter()
        .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
    if let Some(handle) = target {
        io.write_str("\r\nformat: creating 1 partition + TRUEOSFS...\r\n");
        let parts = [crate::disc::install::gpt::GptPartitionSpec {
            type_guid: crate::v::disc::partition::GPT_TYPE_LINUX_FILESYSTEM_BYTES,
            name: "TRUEOS",
            size: crate::disc::install::gpt::PartitionSize::Remaining,
            attributes: 0,
        }];
        let mut log = |msg: &str| {
            io.write_str(msg);
            io.write_str("\r\n");
        };

        match crate::disc::install::gpt::write_gpt_layout_with_log(handle, &parts, &mut log).await {
            Ok(_) => {
                if let Ok(reg) = crate::v::disc::partition::register_gpt_partitions(handle).await {
                    if let Some(first) = reg.first() {
                        if let Some(part_handle) = crate::disc::block::device_handle(first.id) {
                            match crate::v::fs::trueosfs::format_blank_partition_async(part_handle)
                                .await
                            {
                                Ok(()) => {
                                    let (status, err) =
                                        crate::v::disc::detect::detect_physical_disk_detail(handle)
                                            .await;
                                    io.write_fmt(format_args!(
                                        "format: ok (status now: {}{})\r\n",
                                        status.short(),
                                        match (&status, err) {
                                            (
                                                crate::v::disc::detect::DiscStatus::Unknown,
                                                Some(e),
                                            ) => alloc::format!("; err={:?}", e),
                                            _ => alloc::string::String::new(),
                                        }
                                    ));
                                }
                                Err(e) => io.write_fmt(format_args!(
                                    "format: TRUEOSFS failed ({:?})\r\n",
                                    e
                                )),
                            }
                        }
                    }
                }
            }
            Err(e) => io.write_fmt(format_args!("format: GPT write failed ({:?})\r\n", e)),
        }
    } else {
        io.write_str("\r\nformat: no such disk\r\n");
    }
    *mode = ShellMode::Idle;
}

async fn handle_do_install(
    mode: &mut ShellMode,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    spawner: &Spawner,
    disc_id: u32,
) {
    let target = crate::disc::block::device_handles()
        .into_iter()
        .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
    if let Some(handle) = target {
        if let (Some(kernel), Some(bootx64)) = (
            crate::limine::install_kernel_bytes(),
            crate::limine::install_bootx64_bytes(),
        ) {
            io.write_str("\r\ninstall: starting...\r\n");
            match crate::matrix::alloc_slot(alloc::format!("install disc{:03}", disc_id).as_str()) {
                Some(slot) => {
                    let _ = spawner.spawn(crate::matrix::install_matrix_job(
                        slot, handle, bootx64, kernel,
                    ));
                    io.write_fmt(format_args!(
                        "install: started §{} (dump logs with §{})\r\n",
                        slot + 1,
                        slot + 1
                    ));
                    super::refresh_title_bar(io, *term_cols);
                }
                None => io.write_str("install: matrix full\r\n"),
            }
        } else {
            io.write_str("\r\ninstall: kernel or BOOTX64.EFI missing\r\n");
        }
    } else {
        io.write_str("\r\ninstall: no such disk\r\n");
    }
    *mode = ShellMode::Idle;
}

async fn handle_do_update(
    mode: &mut ShellMode,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    spawner: &Spawner,
    disc_id: u32,
) {
    let target = crate::disc::block::device_handles()
        .into_iter()
        .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
    if let Some(handle) = target {
        io.write_str("\r\nupdate: starting...\r\n");
        match crate::matrix::alloc_slot(alloc::format!("update disc{:03}", disc_id).as_str()) {
            Some(slot) => {
                let _ = spawner.spawn(crate::matrix::update_matrix_job(slot, handle));
                io.write_fmt(format_args!(
                    "update: started §{} (dump logs with §{})\r\n",
                    slot + 1,
                    slot + 1
                ));
                super::refresh_title_bar(io, *term_cols);
            }
            None => io.write_str("update: matrix full\r\n"),
        }
    } else {
        io.write_str("\r\nupdate: no such disk\r\n");
    }
    *mode = ShellMode::Idle;
}

fn clear_statusbar(io: &dyn super::ShellIo, cols: usize, rows: usize) {
    let _ = super::statusbar::set_left_active("");
    let _ = super::statusbar::set_right_active("");
    for i in 0..super::statusbar::INDICATOR_COUNT {
        let _ = super::statusbar::set_indicator_active(i, 0);
    }
    super::statusbar::refresh(io, cols, rows);
}

fn reset_shell_display(io: &'static dyn ShellBackend, term_cols: usize, term_rows: usize) {
    io.write_str(crate::ecma48::CLEAR_SCREEN);
    io.write_str(crate::ecma48::HOME);
    super::write_banner(io, term_cols);
    super::apply_shell_scroll_region(io, term_rows);
}
