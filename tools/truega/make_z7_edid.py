"""Generate the HDMI sink EDID for the Nikon Z7 Full HD bring-up."""

import argparse
from pathlib import Path


def checksum(block):
    if len(block) != 127:
        raise ValueError("EDID block must have 127 bytes before its checksum")
    return bytes(block) + bytes([-sum(block) & 0xff])


def description(tag, text):
    return bytes([0, 0, 0, tag, 0]) + text.ljust(13, b" ")


def full_hd_edid():
    base = bytearray.fromhex(
        "00 ff ff ff ff ff ff 00"  # EDID header
        " 52 e1 01 00 01 00 00 00"  # TWA, product 1, serial 1
        " 01 24 01 03"  # Manufacture week/year, EDID 1.3
        " 80 00 00 78 06"  # Digital RGB, sRGB, preferred timing
        " ee 91 a3 54 4c 99 26 0f 50 54"  # sRGB primaries
        " 20 00 00"  # Required VGA fallback
    )
    base += bytes([1, 1]) * 8  # No standard timings
    # 74.25 MHz, 2200x1125 total, 1920x1080 active, positive syncs.
    timing = bytes.fromhex("01 1d 80 18 71 38 2d 40 58 2c 45 00 00 00 00 00 00 1e")
    base += timing
    base += description(0xFC, b"TRUEGA 1080\n")
    base += bytes.fromhex("00 00 00 fd 00 1d 3d 1e 23 08 00 0a 20 20 20 20 20 20")
    base += bytes.fromhex("00 00 00 10 00") + bytes(13)
    base.append(1)

    # Advertise VIC 34 (1920x1080p29.97/30). The HDMI vendor
    # block supplies a physical address and does not advertise deep color.
    cta = bytearray.fromhex("02 03 10 81 42 22 01 65 03 0c 00 10 00 e2 00 4a")
    cta += timing
    cta.extend(bytes(127 - len(cta)))
    return checksum(base) + checksum(cta)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--hex", action="store_true", help="Write a Verilog ROM initialization file")
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    edid = full_hd_edid()
    if args.hex:
        args.output.write_text("".join(f"{byte:02x}\n" for byte in edid))
    else:
        args.output.write_bytes(edid)


if __name__ == "__main__":
    main()
