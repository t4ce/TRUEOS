use core::future::pending;
use core::pin::Pin;

use shvessel::callback::{CommandCall, CommandCallback, CommandFuture, CommandResult};
use shvessel::cmd;
use shvessel::arg::Argument;
use shvessel::path::{Path, TextPath};
use shvessel::vessel::Vessel;
use shvessel::Command;

const MAX_CMDS: usize = 8;
const MAX_JOBS: usize = 4;

const FOOBAR: Command = Command::new("foobar", "fb", "foobar example command");
const MYCOMMAND: Command = Command::new("mycommand", "my", "mycommand example command");

fn main() {
    let kernel_callback = CommandCallback::new(kernel_noop_callback);
    let foobar_callback = CommandCallback::new(foobar_noop_callback);
    let mycommand_callback = CommandCallback::new(mycommand_noop_callback);

    let mut vessel = Vessel::<MAX_CMDS, MAX_JOBS>::new();

    let _ = vessel.register(cmd::MOVE, kernel_callback);
    let _ = vessel.register(cmd::REMOVE, kernel_callback);
    let _ = vessel.register(cmd::LIST, kernel_callback);
    let _ = vessel.register(FOOBAR, foobar_callback);
    let _ = vessel.register(MYCOMMAND, mycommand_callback);

    let args = [
        Argument::Path(Path::from_bytes(b"/raw/source")),
        Argument::TextPath(TextPath::new("/text/dest")),
    ];
    let _move_job = vessel.execute("move", &args);
}

fn kernel_noop_callback<'a>(_call: CommandCall<'a>) -> CommandFuture<'a> {
    noop_future()
}

fn foobar_noop_callback<'a>(_call: CommandCall<'a>) -> CommandFuture<'a> {
    noop_future()
}

fn mycommand_noop_callback<'a>(_call: CommandCall<'a>) -> CommandFuture<'a> {
    noop_future()
}

fn noop_future<'a>() -> CommandFuture<'a> {
    let future = Box::leak(Box::new(pending::<CommandResult>()));
    unsafe { Pin::new_unchecked(future) }
}
