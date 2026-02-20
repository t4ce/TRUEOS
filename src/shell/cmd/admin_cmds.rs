use crate::shell::CommandAction;
use crate::shell::cmd::registry::{ParsedArgs, ShellCommandCtx};

pub(crate) fn cmd_update(
    ctx: &mut ShellCommandCtx<'_>,
    _: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    *ctx.mode = crate::shell::ShellMode::Wizard(crate::shell::InstallWizardStage::UpdateSelectDisk);
    CommandAction::ShowUpdateDiskTable
}

pub(crate) fn cmd_install(
    ctx: &mut ShellCommandCtx<'_>,
    _: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    *ctx.mode = crate::shell::ShellMode::Wizard(crate::shell::InstallWizardStage::SelectDisk);
    CommandAction::ShowInstallDiskTable
}

pub(crate) fn cmd_format(
    ctx: &mut ShellCommandCtx<'_>,
    _: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    *ctx.mode = crate::shell::ShellMode::Wizard(crate::shell::InstallWizardStage::FormatSelectDisk);
    CommandAction::ShowFormatDiskTable
}

pub(crate) fn cmd_file(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    *ctx.mode = crate::shell::ShellMode::Wizard(crate::shell::InstallWizardStage::FileSelectMount);
    CommandAction::ShowFileMountTable
}

pub(crate) fn cmd_bench(
    ctx: &mut ShellCommandCtx<'_>,
    _: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    *ctx.mode = crate::shell::ShellMode::Wizard(crate::shell::InstallWizardStage::BenchSelectDisk);
    CommandAction::ShowBenchDiskTable
}

pub(crate) fn cmd_netbench(
    ctx: &mut ShellCommandCtx<'_>,
    _: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    *ctx.mode =
        crate::shell::ShellMode::Wizard(crate::shell::InstallWizardStage::NetbenchSelectNic);
    CommandAction::ShowNetbenchNicTable
}
