use core::future::Future;
use core::pin::Pin;
use core::ptr::addr_of_mut;
use core::task::{Context, Poll};

use shvessel::arg::Argument;
use shvessel::callback::{CommandCall, CommandCallback, CommandFuture, CommandResult};
use shvessel::job::JobTimeout;
use shvessel::path::{Path, TextPath};
use shvessel::vessel::Vessel;
use shvessel::Command;

// Step 1: Define your commands
const ECHO: Command = Command::new("echo", "ec", "write arguments to output");
const FOOBAR: Command = Command::new("foobar", "fb", "foobar example command");
const MYCOMMAND: Command = Command::new("mycommand", "my", "mycommand example command");

// and decide on how many you would have in total and run cap for parallel commands
const MAX_CMDS: usize = 8;
const MAX_JOBS: usize = 4;

fn main() {
    // Step 2: Get your vessel ready, it gives generic commands and jobs and good structure to build on
    let mut vessel = Vessel::<MAX_CMDS, MAX_JOBS>::new();

    // Step 3: The cmd.rs has some most common commands. 
    // Here you would just pass in the callbacks, to the commands you can or would like to expose.
    // let _ = vessel.register(cmd::MOVE, kernel_move_callback);
    // let _ = vessel.register(cmd::REMOVE, kernel_remove_callback);
    // let _ = vessel.register(cmd::LIST, kernel_list_callback);

    // This is the same step, but for commands that are not in the minimal list aka custom ones
    let _ = vessel.register(ECHO, CommandCallback::syn_call(echo_sync));
    let _ = vessel.register(FOOBAR, CommandCallback::syn_call(foobar_sync));
    let _ = vessel.register(MYCOMMAND, CommandCallback::asyn_call(mycommand_async));

    // Step 3.5: Arguments are supported, i believe my template is generic enough so this can stay, but feel free to adjust
    let args = [
        Argument::Path(Path::from_bytes(b"/raw/source")),
        Argument::TextPath(TextPath::new("/text/dest")),
    ];
    // Use a Timeout if you like to clear the jobs automatically after they did not return of a time
    let _move_job = vessel.execute_with_timeout("move", &args, Some(JobTimeout::new(30)));
    let echo_job = vessel.execute_with_timeout("echo", &args, None);
    let foobar_job = vessel.execute_with_timeout("foobar", &args, None);
    let mycommand_job = vessel.execute_with_timeout("mycommand", &args, None);

    // Step 4: Already you get to execute the commands! 
    demo_commands_runtime();

    // Jobs can be forgotton alltogether, or individually 
    vessel.clean_all();
}

// Demo to ease integration

fn demo_commands_runtime() {
    let waker = std::task::Waker::from(std::sync::Arc::new(StdWake));
    let mut context = Context::from_waker(&waker);

    if let Ok(job) = echo_job {
        let _ = vessel.poll_job(job, &mut context);
    }

    if let Ok(job) = foobar_job {
        let _ = vessel.poll_job(job, &mut context);
    }

    if let Ok(job) = mycommand_job {
        let _ = vessel.poll_job(job, &mut context);
    }
}

// ECHO usually means to output to the system that send in input
fn echo_sync(call: CommandCall<'_>) -> CommandResult {
    for argument in call.arguments {
        // TODO REPLACE WITH YOUR AWESOME CUSTOM OS BUFFER 
        // In case you just merely log it, its not really echo, in my opinion.
        std::println!("{}", argument);
    }
    Ok(()) // OR Err(ReturnCodes::Failed)
}

fn foobar_sync(call: CommandCall<'_>) -> CommandResult {
    // count to 10
    let mut number = 1;
    while number <= 10 {
        std::println!("{}", number);
        number += 1;
    }
    // reverse as string
    for argument in call.arguments {
        if let Argument::TextPath(path) = argument {
            for byte in path.text.as_bytes().iter().rev() {
                std::print!("{}", *byte as char);
            }
            std::println!();
            break;
        }
    }
    // foobar is just a all time classic
    Ok(())
}

fn mycommand_async<'a>(_call: CommandCall<'a>) -> CommandFuture<'a> {
    unsafe { Pin::new_unchecked(&mut *addr_of_mut!(MYCOMMAND_FUTURE)) }
}

struct PendingCommand;

impl Future for PendingCommand {
    type Output = CommandResult;

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

static mut MYCOMMAND_FUTURE: PendingCommand = PendingCommand;

struct StdWake;

impl std::task::Wake for StdWake {
    fn wake(self: std::sync::Arc<Self>) {}
}
