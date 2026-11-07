#![no_std]
#![no_main]
#[unsafe(no_mangle)]
pub extern "C" fn _start()->! {
    assert_eq!(tokio_native_probe::exercise_runtime(),42);
    assert_eq!(tokio_native_probe::exercise_multi_runtime(),28);
    trueos::entry::finish()
}
