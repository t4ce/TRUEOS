use core::future::Future;
use core::pin::Pin;

use crate::arg::Argument;
use crate::ReturnCodes;

pub type CommandResult = Result<(), ReturnCodes>;
pub type CommandFuture<'a> = Pin<&'a mut (dyn Future<Output = CommandResult> + 'a)>;
pub type CommandHandler = for<'a> fn(CommandCall<'a>) -> CommandFuture<'a>;

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
    handler: CommandHandler,
    context: *mut (),
}

impl CommandCallback {
    pub const fn new(handler: CommandHandler) -> Self {
        Self {
            handler,
            context: core::ptr::null_mut(),
        }
    }

    pub const fn with_context(handler: CommandHandler, context: *mut ()) -> Self {
        Self { handler, context }
    }

    pub fn call<'a>(self, arguments: &'a [Argument<'a>]) -> CommandFuture<'a> {
        (self.handler)(CommandCall::new(arguments, self.context))
    }
}
