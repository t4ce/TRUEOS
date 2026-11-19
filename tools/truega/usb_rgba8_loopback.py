"""Transfer a Full HD RGBA8 test image through Sipeed's SOFT_USB bulk loopback."""

import argparse
import errno
import hashlib
import pathlib
import struct
import subprocess
import sys
import time
import zlib

import usb.core
import usb.util

WIDTH = 1920
HEIGHT = 1080
RGBA_BYTES = WIDTH * HEIGHT * 4


def test_image(from_fpga=False):
    pixels = bytearray(RGBA_BYTES)
    for y in range(HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            if from_fpga:
                pixels[offset] = x >> 3
                pixels[offset + 1] = y >> 3
                pixels[offset + 2] = 224 if (x ^ y) & 64 else 32
            else:
                pixels[offset] = x * 255 // (WIDTH - 1)
                pixels[offset + 1] = y * 255 // (HEIGHT - 1)
                pixels[offset + 2] = 224 if (x // 80 + y // 80) % 2 else 32
            pixels[offset + 3] = 255
    return pixels


def read_indexed_frame(device, endpoint_out, endpoint_in, source):
    packet = 512
    pixels_per_reply = (packet - 4) // 4
    received = bytearray()
    retries = 0
    next_report = 1024 * 1024
    for pixel in range(0, WIDTH * HEIGHT, pixels_per_reply):
        y, x = divmod(pixel, WIDTH)
        header = struct.pack("<HH", x, y)
        request = header.ljust(packet, b"\0")
        expected = source[4 * pixel:4 * (pixel + pixels_per_reply)]
        for attempt in range(8):
            try:
                if attempt:
                    # A failed transfer can strand a reply in the device FIFO.
                    device.reset()
                    device.set_configuration(1)
                device.clear_halt(endpoint_out)
                device.clear_halt(endpoint_in)
                if endpoint_out.write(request, timeout=250) != packet:
                    continue
                reply = bytes(endpoint_in.read(packet, timeout=250))
            except usb.core.USBError:
                continue
            if len(reply) == packet and reply[:4] == header and reply[4:4 + len(expected)] == expected:
                received.extend(reply[4:4 + len(expected)])
                retries += attempt
                if len(received) >= next_report:
                    print(f"Verified {len(received):,} of {len(source):,} bytes", flush=True)
                    next_report += 1024 * 1024
                break
        else:
            raise SystemExit(f"Could not verify FPGA packet starting at x={x}, y={y}")
    print(f"Retried {retries} USB packets")
    return received


def read_camera_frame(device, endpoint_out, endpoint_in, expected=None, batch_size=1):
    received = bytearray()
    retries = 0
    resets = 0
    out_address, in_address = endpoint_out.bEndpointAddress, endpoint_in.bEndpointAddress
    location = device.bus, tuple(device.port_numbers)
    packet = 0
    reconnects = 0
    while packet < RGBA_BYTES // 480:
        count = min(batch_size, RGBA_BYTES // 480 - packet)
        request = struct.pack("<HHB", 2046, packet, count - 1).ljust(512, b"\0")
        last_error = "no reply"
        for attempt in range(8):
            try:
                # First retry at the endpoint level. Resetting the whole
                # device can make Linux lose the current USB address.
                if attempt >= 2:
                    device.reset()
                    device.set_configuration(1)
                    usb.util.claim_interface(device, 0)
                    resets += 1
                device.clear_halt(out_address)
                device.clear_halt(in_address)
                if device.write(out_address, request, timeout=500) != 512:
                    last_error = "short USB write"
                    continue
                # Let the FPGA fill the complete USB packet from DDR before
                # the first IN token can read a partially populated FIFO.
                time.sleep(0.0005 * count)
                reply = bytes(device.read(in_address, 512 * count, timeout=500))
            except usb.core.USBError as error:
                last_error = str(error)
                if error.errno in (errno.ENOENT, errno.ENODEV):
                    if reconnects == 5:
                        raise SystemExit("The FPGA USB device disconnected repeatedly") from error
                    reconnects += 1
                    usb.util.dispose_resources(device)
                    device = reconnect_capture_device(location, expected is not None)
                    if expected is None:
                        print("USB reconnected. Restarting the camera frame from its first packet.",
                              file=sys.stderr)
                        received.clear()
                        packet = 0
                    else:
                        print(f"USB reconnected. Resuming the verified DDR test at packet {packet}.",
                              file=sys.stderr)
                    break
                continue
            if len(reply) >= 5 and reply[:4] == request[:4] and reply[4] == 0:
                raise SystemExit("The FPGA has no frozen HDMI frame")
            if len(reply) != 512 * count:
                last_error = f"reply length {len(reply)}, header {reply[:4].hex()}"
                continue
            payloads = bytearray()
            for i in range(count):
                offset = 512 * i
                index = packet + i
                payload = reply[offset + 5:offset + 485]
                checksum, = struct.unpack_from("<I", reply, offset + 485)
                if (reply[offset:offset + 5] != struct.pack("<HHB", 2046, index, 1)
                        or zlib.crc32(payload) != checksum):
                    last_error = f"payload CRC or header mismatch in packet {index}"
                    break
                if expected is not None and payload != expected[480 * index:480 * (index + 1)]:
                    raise SystemExit(f"DDR test pixel mismatch in packet {index}")
                payloads.extend(payload)
            else:
                received.extend(payloads)
                retries += attempt
                if (packet + count) // 2160 != packet // 2160:
                    print(f"Verified {len(received):,} of {RGBA_BYTES:,} DDR bytes",
                          file=sys.stderr, flush=True)
                packet += count
                break
        else:
            raise SystemExit(f"Could not verify camera packet {packet}: {last_error}")
    usb.util.dispose_resources(device)
    print(f"Retried {retries} USB packets, reset {resets} times, "
          f"and recovered {reconnects} USB disconnects", file=sys.stderr)
    return received


def reconnect_capture_device(location, test_mode):
    deadline = time.monotonic() + 25
    expected_mode = 11 if test_mode else 10
    while time.monotonic() < deadline:
        devices = usb.core.find(find_all=True, idVendor=0x04B4, idProduct=0x1004)
        for device in devices:
            if (device.bus, tuple(device.port_numbers)) != location:
                continue
            try:
                flags = read_probe_status(device)[0]
                if flags >> 4 == expected_mode and flags & 8:
                    usb.util.claim_interface(device, 0)
                    return device
            except (usb.core.USBError, SystemExit):
                usb.util.dispose_resources(device)
        time.sleep(0.2)
    raise SystemExit("The FPGA did not return on the same USB port with a frozen frame")


def read_probe_status(device, reprobe=False, release_sequence=None):
    if release_sequence is None:
        header = struct.pack("<HH", 2045 if reprobe else 2047, 0)
    else:
        header = struct.pack("<HH", 2044, release_sequence)
    for attempt in range(4):
        try:
            if attempt >= 2:
                device.reset()
                device.set_configuration(1)
            else:
                try:
                    device.get_active_configuration()
                except usb.core.USBError as error:
                    if error.errno not in (None, errno.ENODATA):
                        raise
                    device.set_configuration(1)
            usb.util.claim_interface(device, 0)
            device.clear_halt(0x02)
            device.clear_halt(0x82)
            device.write(0x02, header.ljust(512, b"\0"), timeout=500)
            reply = bytes(device.read(0x82, 512, timeout=500))
        except usb.core.USBError:
            continue
        if len(reply) != 512 or reply[:4] != header:
            continue
        if reply[4] >> 4 in (7, 8, 9, 10, 11, 12):
            return reply[4:10]
    raise SystemExit("No valid FPGA probe status reply")


def camera_status(device):
    status = read_probe_status(device)
    mode = status[0] >> 4
    flags = status[0]
    if mode == 8:
        edges, = struct.unpack_from("<H", status, 1)
        print(f"HDMI input clock: {edges * 0.016:.3f} MHz")
        print(f"EDID address seen: {'yes' if flags & 1 else 'no'}")
        return
    if mode == 9:
        print(f"DDR calibrated: {'yes' if flags & 1 else 'no'}")
        print(f"Full-frame DDR test: {'failed' if flags & 4 else 'passed' if flags & 2 else 'pending'}")
        return
    if mode == 11:
        print("DDR test pattern: active")
    if mode == 12:
        print("Internal raster test: active")
    labels = ("TMDS PLL lock", "TMDS phase lock")
    labels += (("DDR calibrated", "Full HD raster frozen")
               if mode in (10, 11, 12) else ("Full HD raster seen", "Raster rejected"))
    for bit, label in enumerate(labels):
        print(f"{label}: {'yes' if flags & (1 << bit) else 'no'}")
    if mode in (10, 11, 12):
        sequence, = struct.unpack_from("<H", status, 3)
        if sequence:
            print(f"Frozen frame sequence: {sequence}")
        print(f"EDID address seen: {'yes' if status[1] & 1 else 'no'}")
        if status[2] in (0xdd, 0xde):
            for bit, label in enumerate(("DDC SCL high", "DDC SDA high", "DDC SCL activity",
                                         "DDC SDA activity", "HDMI HPD asserted"), start=1):
                print(f"{label}: {'yes' if status[1] & (1 << bit) else 'no'}")
        if status[2] == 0xde:
            print(f"HDMI active video seen: {'yes' if status[1] & 64 else 'no'}")
            print(f"HDMI DE with VSYNC high: {'yes' if status[1] & 128 else 'no'}")
        if status[5] & 128:
            labels = ("VSYNC", "capture arm", "FIFO backpressure", "DDR begin",
                      "DDR data", "DDR commit", "DDR abort")
            events = [label for bit, label in enumerate(labels) if status[5] & (1 << bit)]
            print("Capture events: " + (", ".join(events) if events else "none"))


def wait_for_next_frame(device):
    status = read_probe_status(device)
    if status[0] >> 4 != 10:
        raise SystemExit("The loaded firmware is not in camera capture mode")
    previous = None
    if status[0] & 8:
        previous, = struct.unpack_from("<H", status, 3)
        read_probe_status(device, release_sequence=previous)
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        status = read_probe_status(device)
        sequence, = struct.unpack_from("<H", status, 3)
        if status[0] >> 4 == 10 and status[0] & 8 and sequence != previous:
            return
        time.sleep(0.25)
    raise SystemExit("The FPGA did not freeze a new HDMI frame")


def transfer(device, source, from_fpga, from_camera=False, batch_size=1):
    if from_fpga or from_camera:
        device.reset()
        device.set_configuration(1)
    try:
        config = device.get_active_configuration()
    except usb.core.USBError as error:
        if error.errno not in (None, errno.ENODATA):
            raise
        device.set_configuration()
        config = device.get_active_configuration()
    interface = config[(0, 0)]
    endpoint_out = usb.util.find_descriptor(interface, bEndpointAddress=0x02)
    endpoint_in = usb.util.find_descriptor(interface, bEndpointAddress=0x82)
    if any(ep is None or usb.util.endpoint_type(ep.bmAttributes) != usb.util.ENDPOINT_TYPE_BULK
           for ep in (endpoint_out, endpoint_in)):
        raise SystemExit("Expected Sipeed bulk endpoints 02 and 82")
    packet = min(endpoint_out.wMaxPacketSize, endpoint_in.wMaxPacketSize)
    received = bytearray()
    try:
        usb.util.claim_interface(device, interface)
        if from_fpga or from_camera:
            if packet != 512:
                raise SystemExit("FPGA readout requires high-speed 512-byte endpoints")
            if from_camera:
                return read_camera_frame(device, endpoint_out, endpoint_in, source, batch_size)
            return read_indexed_frame(device, endpoint_out, endpoint_in, source)
        # Drain echo data left behind by an interrupted host transfer.
        for _ in range(8192 // packet):
            try:
                endpoint_in.read(packet, timeout=100)
            except usb.core.USBTimeoutError:
                break
        batch = 4 * packet
        for offset in range(0, len(source), batch):
            chunk = source[offset:offset + batch]
            if endpoint_out.write(chunk, timeout=2000) != len(chunk):
                raise SystemExit(f"Short USB write at byte {offset}")
            echoed = bytearray()
            try:
                while len(echoed) < len(chunk):
                    echoed.extend(endpoint_in.read(len(chunk) - len(echoed), timeout=1000))
            except usb.core.USBError as error:
                raise SystemExit(f"USB read failed at byte {offset + len(echoed)}: {error}") from error
            if echoed != chunk:
                raise SystemExit(f"USB data mismatch at packet offset {offset}")
            received.extend(echoed)
    finally:
        usb.util.dispose_resources(device)
    return received


def stream_camera(device, output, batch_size):
    location = device.bus, tuple(device.port_numbers)
    while True:
        wait_for_next_frame(device)
        frame = transfer(device, None, False, from_camera=True, batch_size=batch_size)
        output.write(frame)
        output.flush()
        # Recovery during transfer may have changed the device address.
        device = reconnect_capture_device(location, test_mode=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--from-fpga", action="store_true",
                       help="Request a frame from the custom test-pattern firmware")
    modes.add_argument("--camera-status", action="store_true",
                       help="Read HDMI link and raster flags from the receiver probe")
    modes.add_argument("--reprobe-hdmi", action="store_true",
                       help="Pulse HDMI hot-plug detect and read the new link status")
    modes.add_argument("--from-camera", action="store_true",
                       help="Download and verify the frozen HDMI RGBA8 frame")
    modes.add_argument("--next-frame", action="store_true",
                       help="Release any previously captured frame and download a fresh one")
    modes.add_argument("--stream-camera", action="store_true",
                       help="Write consecutive verified Full HD RGBA8 frames to stdout")
    modes.add_argument("--verify-ddr-test", action="store_true",
                       help="Download the DDR test frame and compare every pixel")
    parser.add_argument("png", type=pathlib.Path, nargs="?")
    parser.add_argument("--batch", type=int, choices=range(1, 5), default=1,
                        help="Indexed DDR reply packets per request on batch-capable firmware")
    args = parser.parse_args()
    if args.stream_camera and args.png is not None:
        parser.error("--stream-camera writes to stdout and takes no PNG path")
    if not (args.camera_status or args.reprobe_hdmi or args.stream_camera) and args.png is None:
        parser.error("a PNG output path is required")
    devices = list(usb.core.find(find_all=True, idVendor=0x04B4, idProduct=0x1004))
    if len(devices) != 1:
        raise SystemExit(f"Expected one Sipeed 04b4:1004 device, found {len(devices)}")
    if args.stream_camera:
        try:
            stream_camera(devices[0], sys.stdout.buffer, args.batch)
        except (BrokenPipeError, KeyboardInterrupt):
            return
        return
    if args.camera_status:
        camera_status(devices[0])
        return
    if args.reprobe_hdmi:
        read_probe_status(devices[0], reprobe=True)
        time.sleep(1)
        camera_status(devices[0])
        return
    if args.next_frame:
        wait_for_next_frame(devices[0])
        args.from_camera = True
    if args.from_camera or args.verify_ddr_test:
        flags = read_probe_status(devices[0])[0]
        expected_mode = 11 if args.verify_ddr_test else 10
        if flags >> 4 != expected_mode:
            raise SystemExit("The loaded firmware does not match the selected image source")
        if not flags & 8:
            raise SystemExit("The FPGA has no frozen frame")
    source = None if args.from_camera else test_image(args.from_fpga or args.verify_ddr_test)
    started = time.monotonic()
    received = transfer(devices[0], source, args.from_fpga,
                        args.from_camera or args.verify_ddr_test, args.batch)
    seconds = time.monotonic() - started
    subprocess.run([
        "ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba",
        "-video_size", f"{WIDTH}x{HEIGHT}", "-i", "pipe:0", "-frames:v", "1",
        str(args.png),
    ], input=received, check=True)
    print(f"Verified {len(received):,} bytes over USB in {seconds:.2f} s")
    print(f"RGBA8 SHA-256 {hashlib.sha256(received).hexdigest()}")
    print(args.png)


if __name__ == "__main__":
    main()
