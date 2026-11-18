# Nikon Z7 UHD capture on TRUEGA

The target is live Nikon Z7 HDMI capture at 3840 × 2160, recorded by TRUEOS through the original Tang Mega 138K Pro FPGA. Nikon's 30p mode runs at 29.97 fps. The camera supports 8-bit or 10-bit HDMI output with a compatible sink. Record the negotiated color format and depth when testing. [Nikon specifications](https://onlinemanual.nikonimglib.com/z7_z6/en/14_technical_notes_06.html) · [Nikon HDMI settings](https://onlinemanual.nikonimglib.com/z7_z6/en-nsg/10_connections_04.html)

## Input hardware

The Pro Dock 4071f schematic routes the onboard DVI RX TMDS pairs to general-purpose differential I/O. Gowin lists 1.6 Gb/s for GW5AST-138 LVDS receive. Standard UHD30 at a nominal 297 MHz TMDS clock requires 2.97 Gb/s per data lane. These published figures rule out relying on the existing differential input path for that mode. The old CAM1 frozen-tile code is a separate bring-up path. [Sipeed dock schematic](https://dl.sipeed.com/fileList/TANG/Mega_138K_Pro/02_Schematic/TANG_MEGA-138K_Pro-Dock-4071f_Schematics.pdf) · [Gowin Arora V specifications](https://www.gowinsemi.com/en/product/arora-v-fpga/) · [Analog Devices 297 MHz UHD receiver](https://www.analog.com/en/products/adv7619.html)

An external HDMI front end could feed the same FPGA through a 6G-SDI receiver. A candidate chain is Nikon mini-HDMI → 2160p29.97 HDMI-to-6G-SDI converter → 75 Ω coax → MSA-pinout SDI receiver SFP+ → Tang Mega 138K Pro. The schematic routes SFP0 RX to Q1 lane 0 and SFP1 RX to Q1 lane 1, while PCIe uses Q0. This is a candidate for an external HDMI input. Confirm module pinout, power and thermal limits, fractional-rate receive clocking, and Gowin SDI PHY/decoder support for this exact FPGA and tool release before sourcing hardware. [Converter formats](https://www.blackmagicdesign.com/products/miniconverters/techspecs/W-CONM-29) · [Example SDI module family](https://www.embrionix.com/en/product/EB12BN1R-MxR) · [Gowin SDI PHY guide](https://www.gowinsemi.com/upload/database_doc/3174/document/6887a4bb38923.pdf)

## Host bandwidth

Packed 10-bit 4:2:2 averages 20 bits per active pixel. At this resolution a frame is 20,736,000 bytes and 29.97 fps requires 621.46 MB/s. The archived PCIe configuration is x1 with 8 GT/s enabled. If the link negotiates Gen3 x1, its encoding-adjusted ceiling is 984.62 MB/s before protocol overhead. A 32-bit-per-pixel host format already needs 994.33 MB/s before overhead. Use packed active video and bus-master DMA into a host ring, and verify negotiated link speed and sustained payload throughput. Git revision `d809def15^` contains the historical PCIe configuration at `crates/trueos-fpga-abi/truega/src/serdes/pcie_controller/pcie_controller.ipc`.

The standalone RTL uses v210, which stores six pixels in 16 bytes. At 3840 pixels each row is exactly 10,240 bytes with no row padding. The resulting frame is 22,118,400 bytes and requires **662.89 MB/s** at 30000/1001 fps. Use that payload requirement for DMA testing with this format. The packer takes active sample pairs from an elastic buffer and emits 128-bit blocks. Upstream logic must detect FIFO overflow and reset the packing phase before restarting on a complete row. [v210 packing and alignment](https://developer.apple.com/documentation/corevideo/pixel-format-description-keys?language=objc)

The archived PCIe RTL exposes a 256-bit transmit interface at a configured 100 MHz. The former endpoint emitted BAR read completions. DMA still needs a requester that emits payload writes, observes controller credits and the negotiated maximum payload, splits writes at 4 KiB boundaries, and publishes completions only after the frame data.

## Local packing test

Run from the repository root with Icarus Verilog and FFmpeg:

```sh
stage=$(mktemp -d)
iverilog -g2012 -Wall -s v210_packer_tb -o "$stage/packer" tools/truega/v210_packer.sv tools/truega/v210_packer_tb.sv
(cd "$stage" && vvp packer)
python3 tools/truega/check_v210_packer.py "$stage/simulated.v210"
```

The simulation emits one UHD frame while the consumer intermittently stalls. The checker decodes it using FFmpeg and compares every luma and chroma sample against the input pattern.

## Hardware acceptance

1. Confirm a UHD source lock and the decoded active raster, frame rate, and sample format on the original FPGA.
2. Verify the negotiated PCIe link and sustained DMA at the required payload rate using numbered full-size frames. Check sequence gaps, overruns, and host-side frame integrity.
3. Capture actual Z7 video through the FPGA into TRUEOS. Verify that a saved recording decodes at 3840 × 2160 and 29.97 fps, displays the camera image, and accounts for dropped frames.

The host currently sees the Sipeed USB debugger. The existing account lacks write access to the USB device, the Gowin programmer executable has not been located, and PCI endpoint `22c2:1100` does not enumerate. No UHD hardware result has been obtained.

## USB bring-up on Linux

The first hardware stage uses the Pro Dock's SOFT_USB port and JTAG SRAM
programming. Sipeed's USB bulk-loopback project at revision
`d3cebf1703f1bcd06dca8a53bf2b660c86a5ec16` provides the board USB PHY,
controller, constraints and endpoint FIFO. The vendor image echoed a complete
1920 × 1080 RGBA8 test frame in 0.96 s, with all 8,294,400 bytes matching.
[Vendor project](https://github.com/sipeed/TangMega-138KPro-example/tree/d3cebf1703f1bcd06dca8a53bf2b660c86a5ec16/usb2_soft/usb_cy_bulkloop)

To build the FPGA-generated pattern for the same port:

```sh
python3 tools/truega/prepare_usb_rgba8.py path/to/usb_cy_bulkloop /path/to/staging
cp tools/truega/build_usb_rgba8.tcl /path/to/staging/
cd /path/to/staging
gw_sh build_usb_rgba8.tcl
openFPGALoader -m -c ft2232 --ftdi-serial 2025030317 impl/pnr/truega_usb_rgba8.fs
```

The host test uses PyUSB and FFmpeg. Run it from the repository root once
the USB device re-enumerates and is writable by the Linux account:

```sh
python3 tools/truega/usb_rgba8_loopback.py --from-fpga frame.png
```

In this mode each 512-byte USB request starts with a little-endian pixel
coordinate. The FPGA replies with the same coordinate and 127 generated
RGBA8 pixels. The host verifies the coordinate and pixel bytes, retries
failed packets, and writes the PNG from the received bytes. Omit
`--from-fpga` when using the vendor loopback image.

The indexed firmware passed a hardware readback of all 8,294,400 bytes at
1920 × 1080 RGBA8. This run took 136.73 s and retried 489 packets. The
decoded image SHA-256 was
`e967855d9343c86e47ad77e2e223fffbd5db448c0dec3963e95ea57efc03e65e`.

The optional `--edid` build also advertises 1080p29.97/30 to the camera through
the board's HDMI DDC mux. Its I²C responder passed a repeated-start transfer
of all 256 EDID bytes and a separate extension-block read in simulation.

Use `--dvi-ip-data /path/to/Gowin/IDE/ipcore/DVI_RX/data` when staging to
include the receiver probe. This variant enables EDID and reports the PLL
lock, phase lock, and raster flags over the same USB endpoint:

```sh
python3 tools/truega/usb_rgba8_loopback.py --camera-status
```

The probe build synthesized, placed, routed, loaded in SRAM, and replied over
USB. Its first hardware poll reported no PLL lock and no detected raster.
The RGBA capture block passed simulated pixel-order and malformed-frame
checks. The probe checks active dimensions.

The DDR probe combines the USB firmware with the BL8 controller and pin
constraints from Sipeed's `pro_ddr_test` example. Stage a fresh USB project
without receiver options, then run:

```sh
python3 tools/truega/prepare_ddr_probe.py path/to/pro_ddr_test /path/to/staging
```

Build and load the staged project using the same commands as the USB pattern
test. The status command reports DDR calibration and the full-frame memory
test result. In simulation, the frame store wrote and read all 259,200 bursts,
rejected incomplete frames, and held a pending read through backpressure.
This probe exercises the frame-store logic and memory, with internally
generated data.

On the original board, DDR3 calibrated and the hardware probe passed the
write/read comparison for all 259,200 bursts of a Full HD frame. The test
bitstream was built before the vendor DDR clock-domain groups were restored
to the merged USB timing constraints.

Stage with `--dvi-ip-data` and then apply `prepare_ddr_probe.py --capture` to
build the combined receiver and DDR variant. It captures one complete
1920 × 1080 raster into DDR, holds it, and reports the frozen-frame flag over
USB. The simulation verified pixel order and every DDR write address across
the dual-clock FIFO, including recovery after a FIFO overflow.

Once the status reports a frozen raster, read its pixels and save the PNG:

```sh
python3 tools/truega/usb_rgba8_loopback.py --from-camera z7-fullhd.png
```

Use `--next-frame z7-next.png` to release the indicated frozen frame,
wait for a different capture sequence, and download that frame. The FPGA
accepts release requests only for the current sequence, so retrying a USB
command cannot release a newer frame. Simulation verified two successive
Full HD rasters through the same frame slot.

The `--capture --raster-test` staging mode drives the capture pipeline
from an internal Full HD raster generator with a per-frame pixel marker.
It identifies itself separately over USB and camera mode rejects it. The
hardware test firmware enumerated, but did not report a frozen raster.
The live receiver firmware was restored afterward.

The FPGA returns 15 DDR bursts per request with a packet index and a CRC32
over the 480 pixel bytes. The simulator verified the final packet's addresses,
byte order and checksum against Python's CRC32. A live-camera hardware pass
is still pending.

For a DDR-to-USB hardware check, stage with `--capture --readout-test` and
use `--verify-ddr-test` to compare returned pixels with the known test pattern.
Test firmware reports its source separately and the host rejects it in camera
mode. Hardware spot checks verified CRCs and pixel bytes at six frame offsets
including the final packet. The live receiver firmware remains the camera
entry point.

The corrected dock DDC selector drives P26 high and N19/P19 low. This
selects SCL on IC3 Y4 and SDA on IC4 Y1 because IC4 swaps S0 and S2.

With the live firmware, `--reprobe-hdmi` cycles the FPGA's HPD output for
roughly 336 ms and reads the new link status over USB. Hardware telemetry
verified the FPGA's asserted/deasserted/asserted sequence. The DDC and TMDS
measurements still need a responding source before a camera frame can be
validated.

An early DDR-to-USB test verified the first 13,488 packets (6,474,240 bytes)
before Linux lost the USB device while restoring configuration after a
reset. The buffered test image failed USB enumeration. A later run with
the original DDR test bitstream and host reconnect recovery verified all
8,294,400 bytes in 71.52 seconds. It retried 174 packets and recovered one
USB disconnect. The resulting PNG is byte-identical to the earlier direct
FPGA test pattern.

USB readout now reclaims the interface after reset and can rediscover the
FPGA by its USB bus and physical port if Linux assigns a new device address.
The deterministic DDR test resumes at the first unverified packet after
reconnection. Camera readout starts the frame over so a reconnect cannot
join pixels from two captures.

A raw RGBA8 Full HD stream at 29.97 fps would need 248.58 MB/s. SOFT_USB's
480 Mb/s line rate permits individual full-resolution snapshots and a
lower-rate raw stream. Real-time Full HD at the camera's frame rate will need
compression on the card or a faster device-mode transport.
