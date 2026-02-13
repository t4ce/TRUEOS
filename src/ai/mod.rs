use crate::shell::cmd::registry::{ShellCommandCtx, ParsedArgs};
use crate::shell::CommandAction;
use crate::ecma48;
use alloc::format;

pub fn cmd_ai(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> CommandAction {
    ctx.io.write_fmt(format_args!(
        "{}\r\n", 
        ecma48::color("AI Interface Online", (0, 255, 255))
    ));

    if let Some(args) = args {
        if let Some(msg) = args.get_str(0) {
            ctx.io.write_fmt(format_args!("Received: {}\r\n", msg));
            // In the future, we can add logic here to interact with internal kernel state 
            // or expose diagnostics specifically requested by the AI.
        } else {
             ctx.io.write_str("No message provided.\r\n");
        }
    } else {
        ctx.io.write_str("Usage: ai <message>\r\n");
    }

    CommandAction::None
}
