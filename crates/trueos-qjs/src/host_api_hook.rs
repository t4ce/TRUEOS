#![cfg(feature = "trueos")]

use spin::Once;

use crate::JSContext;

pub type ContextInitHook = unsafe fn(*mut JSContext);

static HOOK: Once<ContextInitHook> = Once::new();

pub fn set_context_init_hook(hook: ContextInitHook) {
    HOOK.call_once(|| hook);
}

pub(crate) unsafe fn call_context_init_hook(ctx: *mut JSContext) {
    if let Some(&hook) = HOOK.get() {
        hook(ctx);
    }
}
