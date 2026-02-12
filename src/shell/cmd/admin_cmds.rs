
use crate::shell::cmd::registry::{ParsedArgs, ShellCommandCtx};
use crate::shell::CommandAction;

pub(crate) fn cmd_update(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> CommandAction {
    *ctx.mode = crate::shell::ShellMode::Wizard(crate::shell::InstallWizardStage::UpdateSelectDisk);
    CommandAction::ShowUpdateDiskTable
}

pub(crate) fn cmd_install(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> CommandAction {
    *ctx.mode = crate::shell::ShellMode::Wizard(crate::shell::InstallWizardStage::SelectDisk);
    CommandAction::ShowInstallDiskTable
}

pub(crate) fn cmd_format(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> CommandAction {
    *ctx.mode = crate::shell::ShellMode::Wizard(crate::shell::InstallWizardStage::FormatSelectDisk);
    CommandAction::ShowFormatDiskTable
}

pub(crate) fn cmd_file(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> CommandAction {
    *ctx.mode = crate::shell::ShellMode::Wizard(crate::shell::InstallWizardStage::FileSelectMount);
    CommandAction::ShowFileMountTable
}

pub(crate) fn cmd_bench(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> CommandAction {
    *ctx.mode = crate::shell::ShellMode::Wizard(crate::shell::InstallWizardStage::BenchSelectDisk);
    CommandAction::ShowBenchDiskTable
}

pub(crate) fn cmd_netbench(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> CommandAction {
    *ctx.mode = crate::shell::ShellMode::Wizard(crate::shell::InstallWizardStage::NetbenchSelectNic);
    CommandAction::ShowNetbenchNicTable
}

pub(crate) fn cmd_mv(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> CommandAction {
    let Some(args) = args else {
        ctx.io.write_str("mv: usage mv <src> <dst>\r\n");
        return CommandAction::None;
    };
    let src_str = args.get_str(0).unwrap_or("");
    let dst_str = args.get_str(1).unwrap_or("");

    if src_str.is_empty() || dst_str.is_empty() {
        ctx.io.write_str("mv: usage mv <src> <dst>\r\n");
        return CommandAction::None;
    }

    let mut src: heapless::String<160> = heapless::String::new();
    let _ = src.push_str(src_str);
    let mut dst: heapless::String<160> = heapless::String::new();
    let _ = dst.push_str(dst_str);

    CommandAction::Mv { src, dst }
}

