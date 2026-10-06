#!/usr/bin/env python3
"""Exercise the real ELF loader and KVM exits with tiny synthetic guests."""
import os
from pathlib import Path
import signal
import struct
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "tools/kvm/run.sh"
BASE = 0xFFFFFFFF80000000
HHDM = 0xFFFF800000000000
PHYS = 0x100000
COMMON = (0xC7B1DD30DF4C8B88, 0x0A82E883A194F07B)
IDS = (
    (0x48DCF1CB8AD2B852, 0x63984E959A98244B),
    (0x67CF3D9D378A806F, 0xE304ACDFC50C3C62),
    (0x71BA76863CC55F63, 0xB2644A48C516A487),
    (0x502746E184C088AA, 0xFBC5EC83E6327893),
    (0x4B161536E598651E, 0xB390AD4A2F1F303A),
)


def elf(code):
    """Fixed-address ELF containing code plus the kernel's five boot requests."""
    data = bytearray(8192)
    ident = b"\x7fELF\x02\x01\x01" + bytes(9)
    data[:64] = struct.pack("<16sHHIQQQIHHHHHH", ident, 2, 62, 1,
                           BASE + PHYS + 0x100, 64, 0x1800, 0, 64, 56, 1, 64, 3, 1)
    data[64:120] = struct.pack("<IIQQQQQQ", 1, 7, 0, BASE + PHYS, PHYS,
                              len(data), len(data), 4096)
    data[0x100:0x100 + len(code)] = code
    for i, request in enumerate(IDS):
        data[0x1000 + 48 * i:0x1000 + 48 * (i + 1)] = struct.pack(
            "<6Q", *COMMON, *request, 0, 0)
    names = b"\0.shstrtab\0.limine_requests\0"
    data[0x1700:0x1700 + len(names)] = names
    data[0x1840:0x1880] = struct.pack("<IIQQQQIIQQ", 1, 3, 0, 0,
                                    0x1700, len(names), 0, 0, 1, 0)
    data[0x1880:0x18C0] = struct.pack("<IIQQQQIIQQ", 11, 1, 3,
                                    BASE + PHYS + 0x1000, 0x1000, 240, 0, 0, 8, 0)
    return data


def output(byte=b"T"):
    return b"\x66\xba\xe9\x00\xb0" + byte + b"\xee"


class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.kernel = Path(self.temp.name) / "test.elf"

    def run_guest(self, image, *args):
        self.kernel.write_bytes(image)
        return subprocess.run([str(RUNNER), "--timeout", "1", *args, str(self.kernel)],
                              capture_output=True, timeout=5)

    def require_kvm(self):
        if not os.access("/dev/kvm", os.R_OK | os.W_OK):
            self.skipTest("requires accessible /dev/kvm")

    def test_truncated_elf(self):
        result = self.run_guest(b"\x7fELF")
        self.assertEqual(result.returncode, 1)
        self.assertIn(b"truncated ELF header", result.stderr)

    def test_segment_outside_ram(self):
        image = elf(b"\xf4")
        struct.pack_into("<Q", image, 64 + 24, 1 << 40)
        result = self.run_guest(image)
        self.assertEqual(result.returncode, 1)
        self.assertIn(b"ELF segment is outside RAM", result.stderr)

    def test_invalid_request_section(self):
        image = elf(b"\xf4")
        struct.pack_into("<Q", image, 0x1880 + 16, BASE + PHYS + 0x1001)
        result = self.run_guest(image)
        self.assertEqual(result.returncode, 1)
        self.assertIn(b"missing or invalid .limine_requests", result.stderr)

    def test_missing_request(self):
        image = elf(b"\xf4")
        image[0x1000:0x1008] = bytes(8)
        result = self.run_guest(image)
        self.assertEqual(result.returncode, 1)
        self.assertIn(b"five supported boot requests", result.stderr)

    def test_hhdm_handoff_and_unsupported_io(self):
        self.require_kvm()
        # Follow the patched request pointer in guest virtual memory and verify
        # the HHDM response. A failed comparison skips output and triple faults.
        code = b"\x48\xb8" + struct.pack("<Q", BASE + PHYS + 0x1000)
        code += b"\x48\x8b\x40\x28\x48\xbb" + struct.pack("<Q", HHDM)
        code += b"\x48\x39\x58\x08\x74\x02\x0f\x0b"
        code += output() + b"\x66\xba\x34\x12\xee"
        result = self.run_guest(elf(code))
        self.assertEqual(result.stdout, b"T")
        self.assertEqual(result.returncode, 1)
        self.assertIn(b"unsupported IO write port=0x1234", result.stderr)

    def test_unsupported_mmio(self):
        self.require_kvm()
        code = b"\x48\xb8" + struct.pack("<Q", 0xFED00000) + b"\x48\x8b\x00"
        result = self.run_guest(elf(code))
        self.assertEqual(result.returncode, 1)
        self.assertIn(b"unsupported MMIO read address=0xfed00000", result.stderr)

    def test_timeout(self):
        self.require_kvm()
        result = self.run_guest(elf(b"\xeb\xfe"))
        self.assertEqual(result.returncode, 124)
        self.assertIn(b"time limit reached", result.stderr)

    def test_interrupt(self):
        self.require_kvm()
        self.kernel.write_bytes(elf(output() + b"\xeb\xfe"))
        process = subprocess.Popen([str(RUNNER), "--timeout", "2", str(self.kernel)],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            self.assertEqual(process.stdout.read(1), b"T")
            process.send_signal(signal.SIGINT)
            _, error = process.communicate(timeout=3)
            self.assertEqual(process.returncode, 130)
            self.assertIn(b"interrupted", error)
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate()

    def test_long_mode_self_test(self):
        self.require_kvm()
        result = subprocess.run([str(RUNNER), "--self-test"], capture_output=True, timeout=5)
        self.assertEqual(result.returncode, 0)
        self.assertIn(b"self-test PASS", result.stderr)


if __name__ == "__main__":
    unittest.main()
