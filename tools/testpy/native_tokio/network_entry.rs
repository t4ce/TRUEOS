#![no_std]
#![no_main]
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    tokio_native_network_probe::exercise_network();
    trueos::entry::finish()
}
