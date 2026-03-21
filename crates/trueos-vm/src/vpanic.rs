use core::sync::atomic::{AtomicU32, Ordering};

use crate::v;

static STAGE: AtomicU32 = AtomicU32::new(0);

pub fn set_stage(stage: u32) {
    STAGE.store(stage, Ordering::Release);
}

pub fn stage() -> u32 {
    STAGE.load(Ordering::Acquire)
}

pub fn note(tag: &str) {
    write_raw(b"VMPANIC: ");
    write_raw(tag.as_bytes());
    write_raw(b"\n");
}

pub fn dump(tag: &str) {
    let (rip, rsp, rbp) = read_regs();
    let stage = stage();

    let mut line = [0u8; 128];
    let mut n = 0usize;
    n = push_bytes(&mut line, n, b"VMPANICD: ");
    n = push_bytes(&mut line, n, tag.as_bytes());
    n = push_bytes(&mut line, n, b" stage=0x");
    n = push_hex_u32(&mut line, n, stage);
    n = push_bytes(&mut line, n, b" rip=0x");
    n = push_hex_u64(&mut line, n, rip);
    n = push_bytes(&mut line, n, b" rsp=0x");
    n = push_hex_u64(&mut line, n, rsp);
    n = push_bytes(&mut line, n, b" rbp=0x");
    n = push_hex_u64(&mut line, n, rbp);
    n = push_bytes(&mut line, n, b"\n");
    write_raw(&line[..n]);
}

fn write_raw(bytes: &[u8]) {
    let _ = v::vshell::uart1_shell_write(bytes);
    v::vsys::write_stream(2, bytes);
}

fn read_regs() -> (u64, u64, u64) {
    let rip: u64;
    let rsp: u64;
    let rbp: u64;
    unsafe {
        core::arch::asm!(
            "lea {rip_out}, [rip + 0]",
            "mov {rsp_out}, rsp",
            "mov {rbp_out}, rbp",
            rip_out = out(reg) rip,
            rsp_out = out(reg) rsp,
            rbp_out = out(reg) rbp,
            options(nostack, nomem, preserves_flags),
        );
    }
    (rip, rsp, rbp)
}

fn push_bytes(dst: &mut [u8], at: usize, src: &[u8]) -> usize {
    let mut i = at;
    for &b in src {
        if i >= dst.len() {
            break;
        }
        dst[i] = b;
        i += 1;
    }
    i
}

fn push_hex_u32(dst: &mut [u8], at: usize, value: u32) -> usize {
    push_hex_u64(dst, at, value as u64)
}

fn push_hex_u64(dst: &mut [u8], at: usize, value: u64) -> usize {
    let mut i = at;
    for shift in (0..16).rev() {
        if i >= dst.len() {
            break;
        }
        let nibble = ((value >> (shift * 4)) & 0xF) as u8;
        dst[i] = match nibble {
            0..=9 => b'0' + nibble,
            _ => b'A' + (nibble - 10),
        };
        i += 1;
    }
    i
}