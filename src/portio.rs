// ARMTODO: `portio` is x86 I/O-port machinery, not a portable device access API.
// The current non-x86 stub is only here to keep the ARM build moving.
// Known caller impact if this stays stubbed on ARM:
// - src/globalog.rs and src/exceptions.rs use port 0xE9 style debug output.
//   That is cheap to lose.
// - src/shell2/backends/uart1_com1.rs is legacy COM1 serial. Also fine to
//   lose on ARM.
// - src/pci/vrng.rs and src/net/vio.rs are bigger: they use legacy virtio over
//   PCI I/O ports. Losing those means losing those device paths on ARM until
//   replaced.
// - src/efi/acpi/mod.rs and src/efi/acpi/sleep.rs also touch port I/O.

#[cfg(not(target_arch = "x86_64"))]
#[cold]
fn unsupported_portio() -> ! {
    panic!("portio is x86-only; non-x86 needs MMIO/platform-specific device backends")
}

#[inline(always)]
#[cfg(target_arch = "x86_64")]
pub(crate) unsafe fn inb(port: u16) -> u8 {
    let mut value: u8;
    core::arch::asm!(
        "in al, dx",
        out("al") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    value
}

#[inline(always)]
#[cfg(not(target_arch = "x86_64"))]
pub(crate) unsafe fn inb(port: u16) -> u8 {
    let _ = port;
    unsupported_portio()
}

#[inline(always)]
#[cfg(target_arch = "x86_64")]
pub(crate) unsafe fn inw(port: u16) -> u16 {
    let mut value: u16;
    core::arch::asm!(
        "in ax, dx",
        out("ax") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    value
}

#[inline(always)]
#[cfg(not(target_arch = "x86_64"))]
pub(crate) unsafe fn inw(port: u16) -> u16 {
    let _ = port;
    unsupported_portio()
}

#[inline(always)]
#[cfg(target_arch = "x86_64")]
pub(crate) unsafe fn inl(port: u16) -> u32 {
    let mut value: u32;
    core::arch::asm!(
        "in eax, dx",
        out("eax") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    value
}

#[inline(always)]
#[cfg(not(target_arch = "x86_64"))]
pub(crate) unsafe fn inl(port: u16) -> u32 {
    let _ = port;
    unsupported_portio()
}

#[inline(always)]
#[cfg(target_arch = "x86_64")]
pub(crate) unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") val,
        options(nomem, nostack, preserves_flags)
    );
}

#[inline(always)]
#[cfg(not(target_arch = "x86_64"))]
pub(crate) unsafe fn outb(port: u16, val: u8) {
    let _ = (port, val);
    unsupported_portio()
}

#[inline(always)]
#[cfg(target_arch = "x86_64")]
pub(crate) unsafe fn outw(port: u16, val: u16) {
    core::arch::asm!(
        "out dx, ax",
        in("dx") port,
        in("ax") val,
        options(nomem, nostack, preserves_flags)
    );
}

#[inline(always)]
#[cfg(not(target_arch = "x86_64"))]
pub(crate) unsafe fn outw(port: u16, val: u16) {
    let _ = (port, val);
    unsupported_portio()
}

#[inline(always)]
#[cfg(target_arch = "x86_64")]
pub(crate) unsafe fn outl(port: u16, val: u32) {
    core::arch::asm!(
        "out dx, eax",
        in("dx") port,
        in("eax") val,
        options(nomem, nostack, preserves_flags)
    );
}

#[inline(always)]
#[cfg(not(target_arch = "x86_64"))]
pub(crate) unsafe fn outl(port: u16, val: u32) {
    let _ = (port, val);
    unsupported_portio()
}
