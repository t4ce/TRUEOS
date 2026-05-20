use core::future::Future;
use core::pin::Pin;

use crate::arg::Argument;
use crate::ReturnCodes;

pub type CommandResult = Result<(), ReturnCodes>;
pub type CommandFuture<'a> = Pin<&'a mut (dyn Future<Output = CommandResult> + 'a)>;
pub type CommandHandler = for<'a> fn(CommandCall<'a>) -> CommandFuture<'a>;
pub type CommandSyncHandler = for<'a> fn(CommandCall<'a>) -> CommandResult;

pub enum CommandOutcome<'a> {
    Future(CommandFuture<'a>),
    Ready(CommandResult),
}

#[derive(Clone, Copy)]
pub struct CommandCall<'a> {
    pub arguments: &'a [Argument<'a>],
    pub context: *mut (),
}

impl<'a> CommandCall<'a> {
    pub const fn new(arguments: &'a [Argument<'a>], context: *mut ()) -> Self {
        Self { arguments, context }
    }

    pub unsafe fn context_mut<T>(&self) -> Option<&mut T> {
        unsafe { self.context.cast::<T>().as_mut() }
    }
}

#[derive(Clone, Copy)]
pub struct CommandCallback {
    kind: CommandCallbackKind,
    context: *mut (),
}

#[derive(Clone, Copy)]
enum CommandCallbackKind {
    Future(CommandHandler),
    Sync(CommandSyncHandler),
}

impl CommandCallback {
    pub const fn asyn_call(handler: CommandHandler) -> Self {
        Self {
            kind: CommandCallbackKind::Future(handler),
            context: core::ptr::null_mut(),
        }
    }

    pub const fn asyn_call_with_context(handler: CommandHandler, context: *mut ()) -> Self {
        Self {
            kind: CommandCallbackKind::Future(handler),
            context,
        }
    }

    pub const fn syn_call(handler: CommandSyncHandler) -> Self {
        Self {
            kind: CommandCallbackKind::Sync(handler),
            context: core::ptr::null_mut(),
        }
    }

    pub const fn syn_call_with_context(handler: CommandSyncHandler, context: *mut ()) -> Self {
        Self {
            kind: CommandCallbackKind::Sync(handler),
            context,
        }
    }

    pub const fn new(handler: CommandHandler) -> Self {
        Self::asyn_call(handler)
    }

    pub const fn with_context(handler: CommandHandler, context: *mut ()) -> Self {
        Self::asyn_call_with_context(handler, context)
    }

    pub const fn sync(handler: CommandSyncHandler) -> Self {
        Self::syn_call(handler)
    }

    pub const fn sync_with_context(handler: CommandSyncHandler, context: *mut ()) -> Self {
        Self::syn_call_with_context(handler, context)
    }

    pub fn call<'a>(self, arguments: &'a [Argument<'a>]) -> CommandOutcome<'a> {
        let call = CommandCall::new(arguments, self.context);
        match self.kind {
            CommandCallbackKind::Future(handler) => CommandOutcome::Future(handler(call)),
            CommandCallbackKind::Sync(handler) => CommandOutcome::Ready(handler(call)),
        }
    }
}
