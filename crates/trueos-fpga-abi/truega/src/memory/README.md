# TRUEGA board-memory staging

These files prepare the later full-model residency milestone without changing
the working PCIe/heartbeat image.  Nothing in this directory is included by
`min_pci_led.gprj`.

## Why the programmer is not the model-loading path

Gowin Programmer configures FPGA SRAM or configuration SPI flash; it does not
initialize the two volatile DDR3 devices.  Its CLI exposes SRAM, embedded-flash,
external-flash, user-flash, and EBR operations, but no DDR operation.  The SOM's
two 128-Mbit SPI NOR devices provide only 32 MiB in total, versus the sealed
native model's 376,701,952 bytes (359.25 MiB).  A programmer preload therefore
cannot replace a runtime DDR writer.  Even a hypothetical larger SPI device
would still need an FPGA flash-to-DDR loader after configuration.

For the first complete gate-row proof, streaming the 32 native blocks from
TRUEOSFS through the existing BAR is consequently the shortest path.  DDR is
needed when weights become resident for full projections/layers.

## Exact future DDR boundary

`ddr3_memory_interface.ipc` preserves the official Tang Mega 138K Pro DDR3
parameters but targets the project's exact `gw5ast138b-002` device.
`tang_mega_138k_pro_ddr3.cst` contains the FPG676 pin map, including A14; do not
use the PG484 constraint file from the current Sipeed example repository.

Regenerate the encrypted `DDR3_Memory_Interface_Top` with Gowin IP Core
Generator before integration.  Open the IPC file, confirm:

- part `GW5AST-LV138FPG676AES`, device version B;
- 400 MHz DDR memory clock and 1:4 application clock;
- 32-bit data / four DQS lanes and 256-bit native application data;
- 15 row-address pins, three bank pins, and 29-bit application address.

The currently installed Gowin 1.9.12.02 IPFlow Tcl command crashes while reading
this DDR IPC in headless mode, so the generated encrypted source is deliberately
not checked in from an unverified target.  The GUI generator is the safe path
until Gowin fixes that command.

`truega_ddr3_model_writer.v` is the reusable controller-side boundary.  It
accepts the fixed native image as 32-bit words in the DDR application clock
domain, packs eight words per 256-bit write, and stops at the synthesized byte
count.  Integration still requires:

1. a small asynchronous FIFO from the 100 MHz PCIe TLP domain to the DDR
   controller's `clk_out` domain;
2. BAR model-load address/data/status registers or a larger posted-write window;
3. calibration status and writer counters exposed read-only through BAR0;
4. a DDR read client shared with GEMV;
5. deterministic readback of the contract and sampled native blocks before the
   firmware marks the model resident.

The controller's application address counts 32-bit words, so every 256-bit beat
increments it by eight.  The native image contains 11,771,936 such beats and
fits below the 512 MiB boundary even before the final A14/full-capacity proof.
