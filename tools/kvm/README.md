# Direct KVM launcher

This is a small Linux/x86-64 virtual-machine monitor for TRUEOS kernel bring-up.
It opens `/dev/kvm` itself. It does not invoke QEMU, boot firmware, build an ISO,
or change the physical rig. The native executable is built from `launch.c` with
the host C compiler and Linux KVM headers into `bld/kvm/trueos-kvm`.

From the repository root, build and run the exact kernel Cargo produces:

```sh
cargo kvm
cargo kvm -- --timeout 30 --memory 1024
```

`cargo run` retains its host-execution guard. `cargo kvm` explicitly overrides
that runner for the TRUEOS target. Kernel build prerequisites are the same as
for `cargo build`; use `make kernel` when the embedded artifacts need rebuilding.

To run an already built kernel without Cargo, or check KVM independently:

```sh
tools/kvm/run.sh --timeout 10 tgt/x86_64-unknown-trueos/debug/TRUEOS
tools/kvm/run.sh --self-test
```

The launcher defaults to 1024 MiB of anonymous guest RAM, one vCPU and a
10-second time limit. Memory can be 256–2048 MiB; time limits can be 1–3600
seconds. Ctrl-C stops this VM. A timeout exits 124, an interrupt exits 128 plus
the signal number, and unsupported guest operations exit 1 with registers and
the failing IO port or MMIO address. The self-test exits 0 only after its small
guest executes in 64-bit higher-half addresses and sends its completion marker
through a KVM IO exit. It does not certify a complete TRUEOS boot.

## Boot handoff

This first version loads the fixed-address TRUEOS ELF's `PT_LOAD` segments,
zeros their BSS, builds identity, HHDM and kernel mappings with 2 MiB pages, and
starts the BSP at the ELF entry in long mode. It answers five requests in the
ELF's `.limine_requests` section: HHDM, memory map, executable address, date at
boot and command line. The boot data and page tables are reserved in low RAM;
the kernel image is excluded from the usable-memory map.

**It does not run Limine.** This is a limited handoff using the request layouts
already consumed by TRUEOS, not a conformant Limine protocol implementation.
The base revision remains unsupported and other response pointers stay null.
Running stock Limine would require a guest BIOS/UEFI environment, which this
launcher deliberately does not implement. Kernels enforcing full protocol
compliance will need a fuller loader.

## Guest platform and current boundary

KVM supplies CPU execution and in-kernel PIC/IOAPIC/local-APIC support. The
userspace loop supplies polling COM1 output, debug port `0xe9`, POST port `0x80`
and an empty PCI configuration bus. Unknown IO and MMIO accesses stop the VM
instead of pretending a device worked. UART output goes to stdout; launcher
diagnostics go to stderr. TRUEOS enables its emulator UART sink after PCI
enumeration, so earlier boot records are not printed by this launcher.

There is no framebuffer, GPU, storage, NIC, USB, ACPI/HPET table set, SMP boot
handoff, firmware service environment or nested VMX. This is suitable for CPU
and kernel bring-up, not desktop or Blueprint hypervisor validation. Guest
timing currently relies on whatever TRUEOS can derive without firmware tables;
service-start logs alone do not prove timer-driven services work.

On the i9-13900K development host, the current debug kernel reached service
startup (`net-shell-listener`, `tga-rpc`) and remained running until the time
limit without an unsupported device exit. There is no NIC, so the listener is
not reachable from the host. The launcher also passed its higher-half execution
self-test. Runtime success should be judged against the subsystem under test,
not against the mere absence of a crash.

Requirements: Linux x86-64, working KVM with device access for the current user,
a C compiler and Linux KVM headers. No root escalation or software-emulation
fallback is attempted. KVM API reference:
<https://docs.kernel.org/virt/kvm/api.html>.
