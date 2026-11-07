#![no_std]
#![no_main]
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    if let Ok(runtime) = trueos::runtime::current_thread().build() {
        let _ = runtime.block_on(trueos::net::resolve_host("example.test", 443));
    }
    trueos::entry::finish()
}
