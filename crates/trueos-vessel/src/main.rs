#![no_std]
#![no_main]

use core::panic::PanicInfo;

const STDOUT: u32 = 1;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    write_stdout(b"hello from trueos-vessel\n");
    halt()
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    write_stdout(b"trueos-vessel panic\n");
    halt()
}

fn write_stdout(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }

    unsafe {
        trueos_cabi_write(STDOUT, bytes.as_ptr(), bytes.len());
    }
}

fn halt() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
