#!/usr/bin/env python3
"""Meaningful host checks for sidecar identity, entry frames and first-hit gating."""
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
from types import SimpleNamespace

from elfmap import KernelMap, ModuleMap, prepare_manifest
from gdb_capture import CaptureController, OfflineAddr2line, bounded_stack, decode_guest_snapshot, entry_frame_walk, parse_addr2line_batch, recognize_physical_root, x86_u64
from vmx_memory import GuestMemory


class MemoryEvidenceTests(unittest.TestCase):
    def test_batched_source_rows_preserve_inline_functions_and_exact_address_binding(self):
        output = "0x0000000000000010\ninline_leaf\nleaf.rs:8\ncaller\ncaller.rs:20\n0x0000000000000030\nother\nother.rs:50\n"
        mapped = parse_addr2line_batch(output, [0x10, 0x30])
        self.assertEqual(mapped[0x10], "inline_leaf\nleaf.rs:8\ncaller\ncaller.rs:20\n")
        for incorrect in [output.replace("0x0000000000000030", "0x0000000000000010"),
                          output.replace("0x0000000000000030", "0x0000000000000040"),
                          "unexpected preamble\n" + output, output.rstrip().rsplit("\n", 1)[0] + "\n"]:
            with self.assertRaises(ValueError):
                parse_addr2line_batch(incorrect, [0x10, 0x30])

    def test_source_lookup_batches_and_caches_without_cross_section_aliasing(self):
        commands = []
        def runner(command, **kwargs):
            commands.append(command)
            addresses = [int(value, 16) for value in command if value.startswith("0x")]
            tag = command[command.index("-j") + 1] if "-j" in command else "kernel"
            return SimpleNamespace(returncode=0, stderr="", stdout="".join(
                f"0x{address:016x}\n{tag}-fn\nfile.rs:{address}\n" for address in addresses))
        source = OfflineAddr2line(runner=runner)
        first = ("module.o", ".text.a", 0x10)
        second = ("module.o", ".text.a", 0x30)
        separate = ("module.o", ".text.b", 0x10)
        kernel = ("kernel.elf", None, 0x10)
        result = source.lookup([first, second, first, separate, kernel])
        self.assertEqual(len(commands), 3)
        self.assertEqual(result[first]["batch_addresses"], 2)
        self.assertIn(".text.a-fn", result[first]["output"])
        self.assertIn(".text.b-fn", result[separate]["output"])
        self.assertIn("kernel-fn", result[kernel]["output"])
        source.lookup([kernel, separate, second])
        self.assertEqual(len(commands), 3)

    def test_metadata_failures_are_cached_errors_not_guessed_source(self):
        cases = [(lambda command, **kwargs: SimpleNamespace(returncode=0, stderr="", stdout="0x0000000000000020\nwrong\nwrong.rs:1\n"), "address-delimiter-validation"),
                 (lambda command, **kwargs: SimpleNamespace(returncode=1, stderr="bad elf", stdout=""), "process-error")]
        def timeout(command, **kwargs):
            raise subprocess.TimeoutExpired(command, 10)
        cases.append((timeout, "process-timeout"))
        for runner, kind in cases:
            source = OfflineAddr2line(runner=runner)
            key = ("module.o", ".text", 0x10)
            result = source.lookup([key])[key]
            self.assertEqual(result["error_kind"], kind)
            self.assertNotIn("output", result)
            self.assertEqual(source.lookup([key])[key], result)

    def test_guest_dispatch_failure_cannot_claim_a_complete_marker(self):
        pc = 0xabcdef
        header = [0x50414e5343464344, 1, 1152, 5, 0, 19, pc, 0x400100, 2, 0x5000] + [0]*13
        header[18] = 0x1000 | (3 << 3)
        header[19] = 0xffff800000000000
        raw = struct.pack("<23Q15Q2Q", *header, *range(100, 115), 7, 832)+bytes(832)
        controller = CaptureController.__new__(CaptureController)
        controller.gdb = SimpleNamespace(parse_and_eval=lambda expression: 0x10000,
                                        selected_inferior=lambda: SimpleNamespace(read_memory=lambda address, size: raw))
        controller.breakpoints, controller.records, controller.scenarios = [], {}, {}
        controller.manifest, controller.events, controller.errors = dict(expected_vm_id=0), [], []
        controller.complete_seen, controller.aborted, controller.pass_mask = False, False, None
        controller.write_summary = lambda: None
        controller.handle_guest_hook()
        summary = controller.summary()
        self.assertTrue(summary["aborted"])
        self.assertFalse(summary["complete_marker_seen"])
        self.assertFalse(summary["capture_ok"])

    def test_rust_signed_registers_preserve_unsigned_x86_address_bits(self):
        address = 0xffff800212345678
        signed = address - (1 << 64)
        self.assertEqual(x86_u64(signed), address)
        controller = CaptureController.__new__(CaptureController)
        controller.guest_snapshot = None
        controller.gdb = SimpleNamespace(parse_and_eval=lambda register: signed)
        self.assertEqual(controller.current_integer("rdi"), address)
        self.assertEqual(controller.current_integer("rsp"), address)

    def test_ignored_l2_entry_restores_opcode_and_fails_coverage_instead_of_retrapping(self):
        pc = 0xabcdef
        header = [0x50414e5343464344, 1, 1152, 5, 0, 19, pc, 0x400100, 2, 0x5000] + [0]*13
        header[18] = 0x1000 | (3 << 3)
        header[19] = 0xffff800000000000
        raw = struct.pack("<23Q15Q2Q", *header, *range(100, 115), 7, 832)+bytes(832)
        controller = CaptureController.__new__(CaptureController)
        pointer = 0xffff800212345678
        reads = []
        def read(address, size):
            reads.append(address)
            return raw
        controller.gdb = SimpleNamespace(parse_and_eval=lambda expression: pointer-(1 << 64),
                                        selected_inferior=lambda: SimpleNamespace(read_memory=read))
        target = SimpleNamespace(enabled=True, address=pc, scenario_index=1, target=dict(symbol="kernel_leaf", scope="kernel"))
        controller.breakpoints = [target]
        controller.active = 1
        controller.manifest = dict(expected_vm_id=0)
        controller.records = {1: dict(hits={}, errors=[])}
        controller.events = []
        controller.errors = []
        controller.complete_seen = False
        controller.capture = lambda index, leaf: None
        controller.write_summary = lambda: None
        controller.handle_guest_hook()
        self.assertFalse(target.enabled)
        self.assertEqual(controller.records[1]["hits"], {})
        self.assertIn("disabled before same-RIP resume", controller.records[1]["errors"][0])
        self.assertIsNone(controller.guest_snapshot)
        self.assertEqual(reads, [pointer])

    def test_guest_snapshot_is_versioned_scoped_and_keeps_guest_registers(self):
        header = [0x50414e5343464344, 1, 1152, 5, 2, 19, 0xabcdef, 0x400100, 2, 0x5000] + [0] * 13
        raw = struct.pack("<23Q15Q2Q", *header, *range(100, 115), 7, 832) + bytes(832)
        decoded = decode_guest_snapshot(raw, expected_vm_id=2)
        self.assertEqual(decoded["registers"]["rip"], 0xabcdef)
        self.assertEqual(decoded["registers"]["rdi"], 105)
        self.assertEqual(decoded["registers"]["rbp"], 106)
        self.assertEqual(len(decoded["xsave"]), 832)
        with self.assertRaisesRegex(ValueError, "expected probe VM"):
            decode_guest_snapshot(raw, expected_vm_id=3)
        with self.assertRaisesRegex(ValueError, "magic/schema/size"):
            decode_guest_snapshot(bytes(1152))

    def test_guest_cr3_and_ept_read_never_substitute_host_low_virtual_memory(self):
        hhdm = 0xffff800000000000
        words = {0x1000: 0x2000 | 7, 0x2000: 0x87,  # EPT 1 GiB identity mapping
                 0x5000: 0x6000 | 3, 0x6000: 0x7000 | 3, 0x7000: 0x8000 | 3,
                 0x8000 + 4*8: 0x9000 | 3, 0x8000 + 5*8: 0xa000 | 3, 0x8000 + 6*8: 0}
        observed = []
        def host_read(address, size):
            observed.append(address)
            physical = address-hhdm
            if size == 8 and physical in words:
                return struct.pack("<Q", words[physical])
            if 0x9000 <= physical < 0xa000:
                return b"a" * size
            if 0xa000 <= physical < 0xb000:
                return b"b" * size
            raise OSError("unmapped host physical fixture")
        memory = GuestMemory(host_read, 0x5000, 0x1000 | (3 << 3) | 6, hhdm)
        self.assertEqual(memory.read_memory(0x4ff8, 16), b"a"*8+b"b"*8)
        self.assertTrue(all(address >= hhdm for address in observed))
        with self.assertRaisesRegex(OSError, "Unmapped guest virtual"):
            memory.read_memory(0x6000, 8)
        with self.assertRaisesRegex(ValueError, "Noncanonical"):
            memory.read_memory(0x0000800000000000, 8)

    def test_guest_large_page_translation_and_unsupported_paging_are_explicit(self):
        hhdm = 0xffff800000000000
        words = {0x1000: 0x2000 | 7, 0x2000: 0x87,
                 0x5000: 0x6000 | 3, 0x6000: 0x7000 | 3, 0x7000: 0x200000 | 0x83}
        memory = GuestMemory(lambda address, size: struct.pack("<Q", words[address-hhdm]),
                             0x5000, 0x1000 | (3 << 3), hhdm)
        self.assertEqual(memory.translate(0x1234), 0x201234)
        with self.assertRaisesRegex(ValueError, "Five-level"):
            GuestMemory(None, 0x5000, 0x1000 | (3 << 3), hhdm, 1 << 12)
        with self.assertRaisesRegex(ValueError, "four-level EPT"):
            GuestMemory(None, 0x5000, 0x1000 | (4 << 3), hhdm)

    def test_entry_caller_is_rsp_not_inherited_rbp(self):
        values = {0x1000: struct.pack("<Q", 0xaaaa), 0x1040: struct.pack("<QQ", 0x1080, 0xbbbb),
                  0x1080: struct.pack("<QQ", 0, 0xcccc)}
        walk = entry_frame_walk(lambda address, size: values[address][:size], 0xdddd, 0x1000, 0x1040, 0x2000)
        self.assertEqual([f["pc"] for f in walk["frames"]], [0xdddd, 0xaaaa, 0xbbbb, 0xcccc])
        self.assertEqual(walk["frames"][1]["stack_word_address"], 0x1000)
        self.assertEqual(walk["termination"], "zero-frame-pointer")
        self.assertFalse(walk["complete"])

    def test_frame_pointer_record_never_reads_outside_bounded_capture(self):
        reads = []
        def read(address, size):
            reads.append((address, size)); return struct.pack("<Q", 0xaaaa)
        walk = entry_frame_walk(read, 0xdddd, 0x1000, 0x3000, 0x2000)
        self.assertEqual(reads, [(0x1000, 8)])
        self.assertEqual(walk["termination"], "frame-pointer-outside-capture-range")

    def test_non_increasing_chain_is_reported(self):
        values = {0x1000: struct.pack("<Q", 0xaaaa), 0x1040: struct.pack("<QQ", 0x1040, 0xbbbb)}
        walk = entry_frame_walk(lambda address, size: values[address][:size], 0xdddd, 0x1000, 0x1040, 0x2000)
        self.assertEqual(walk["termination"], "non-increasing-frame-pointer")

    def test_raw_stack_holes_are_explicit_not_fabricated_memory(self):
        def read(address, size):
            if address == 0x2000:
                raise OSError("unmapped guard page")
            return b"x" * size
        raw, info = bounded_stack(read, 0x1ff0, 64)
        self.assertEqual(raw[:16], b"x" * 16)
        self.assertEqual(raw[16:], bytes(48))
        self.assertEqual(info["unreadable_bytes"], 48)
        self.assertEqual(info["ranges"][1]["error"], "unmapped guard page")
        self.assertFalse(info["ranges"][1]["readable"])

    def test_short_read_is_an_error_range(self):
        _, info = bounded_stack(lambda address, size: b"x", 0x1000, 64)
        self.assertEqual(info["unreadable_bytes"], 64)
        self.assertIn("Short memory read", info["ranges"][0]["error"])

    def test_known_root_does_not_hide_frame_limit(self):
        walk = dict(frames=[dict(module=dict(symbols=["_start"]))], termination="frame-limit", complete=False)
        self.assertFalse(recognize_physical_root(walk, ["_start"])["complete"])

    def test_known_continuation_root_with_zero_rbp_is_complete(self):
        walk = dict(frames=[dict(kernel=dict(symbol="trueos_thread_context_start"))], termination="zero-frame-pointer", complete=False)
        self.assertTrue(recognize_physical_root(walk, ["trueos_thread_context_start"])["complete"])
        self.assertEqual(walk["observed_termination"], "zero-frame-pointer")

    def test_known_module_root_requires_zero_ancestry_at_zero_return(self):
        for next_rbp, expected in [(0, True), (0x1200, False)]:
            walk = dict(frames=[dict(module=dict(symbols=["_start"]))], termination="zero-return-address", complete=False,
                        terminal_record=dict(next_frame_pointer=next_rbp, return_address=0))
            self.assertEqual(recognize_physical_root(walk, ["_start"])["complete"], expected)


@unittest.skipUnless(shutil.which("cc") and shutil.which("objcopy"), "host C/ELF tools unavailable")
class ELFAndDebuggerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="depthchecker-capture-tests-")
        cls.directory = Path(cls.temporary.name)
        source = cls.directory / "fixture.c"
        source.write_text(r'''
#include <stdint.h>
uint64_t DEPTHCHECKER_DEBUG_GATE = 0;
__attribute__((noinline)) void depthchecker_begin(uint64_t index) { __asm__ volatile("" : : "r"(index) : "memory"); }
__attribute__((noinline)) void depthchecker_end(uint64_t index) { __asm__ volatile("" : : "r"(index) : "memory"); }
__attribute__((noinline)) void depthchecker_complete(uint64_t mask) { __asm__ volatile("" : : "r"(mask) : "memory"); }
__attribute__((noinline)) int fixture_leaf(int value) { volatile int local = value + 1; return local; }
int main(void) {
    depthchecker_begin(1);
    int first = fixture_leaf(5);
    int second = fixture_leaf(6);
    depthchecker_end(1);
    depthchecker_complete(first == 6 && second == 7 ? 1 : 0);
    return 0;
}
''')
        cls.debug, cls.packed, cls.executable = [cls.directory / name for name in ["fixture.debug.o", "fixture.packed.o", "fixture.elf"]]
        subprocess.run(["cc", "-g", "-O0", "-fno-pie", "-fno-omit-frame-pointer", "-c", str(source), "-o", str(cls.debug)], check=True, capture_output=True)
        subprocess.run(["objcopy", "--strip-debug", str(cls.debug), str(cls.packed)], check=True, capture_output=True)
        subprocess.run(["cc", "-no-pie", str(cls.debug), "-o", str(cls.executable)], check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def test_strip_changed_indices_still_authenticate_ordered_allocated_bytes(self):
        mapped = ModuleMap(self.debug, self.packed, 0x100000)
        try:
            self.assertTrue(mapped.evidence()["allocated_sidecars_match"])
            self.assertTrue(any(s["debug_index"] != s["packed_index"] for s in mapped.sections))
            leaf = mapped.resolve("fixture_leaf")
            self.assertEqual(mapped.at(leaf["address"])["symbols"], ["fixture_leaf"])
            self.assertEqual(leaf["address"], 0x100000 + leaf["image_offset"])
        finally:
            mapped.close()

    def test_changed_code_in_sidecar_is_rejected(self):
        changed = self.directory / "changed.packed.o"
        original = ModuleMap(self.debug, self.packed)
        try:
            section = next(s for s in original.packed.sections if s["flags"] & 4 and s["size"])
            data = bytearray(self.packed.read_bytes()); data[section["file_offset"]] ^= 1
            changed.write_bytes(data)
        finally:
            original.close()
        with self.assertRaisesRegex(ValueError, "allocated section mismatch"):
            ModuleMap(self.debug, changed)

    def spec(self, output):
        return dict(module=dict(debug_elf=str(self.debug), packed_elf=str(self.packed), load_base=0x100000),
                    output_dir=str(output), scenarios=[dict(id="leaf", index=1, targets=[dict(symbol="fixture_leaf")])])

    def test_missing_target_cannot_be_a_pass(self):
        spec = self.spec(self.directory / "missing")
        spec["scenarios"][0]["targets"][0]["symbol"] = "not_in_the_object"
        with self.assertRaisesRegex(ValueError, "resolves to 0"):
            prepare_manifest(spec)

    def test_probe_marker_cannot_substitute_for_dependency_entry(self):
        spec = self.spec(self.directory / "marker")
        spec["scenarios"][0]["targets"][0]["symbol"] = "depthchecker_begin"
        with self.assertRaisesRegex(ValueError, "separate from probe markers"):
            prepare_manifest(spec)

    def test_kernel_entry_is_authenticated_absolute_symbol(self):
        spec = self.spec(self.directory / "kernel")
        spec["kernel_elf"] = str(self.executable)
        spec["scenarios"][0]["targets"][0]["scope"] = "kernel"
        prepared = prepare_manifest(spec)
        mapped = KernelMap(self.executable)
        try:
            address = mapped.resolve("fixture_leaf")["address"]
            self.assertEqual(prepared["scenarios"][0]["targets"][0]["resolved"]["address"], address)
            self.assertEqual(mapped.at(address)["symbol"], "fixture_leaf")
            self.assertEqual(prepared["kernel_map"]["elf_type"], "ET_EXEC")
        finally:
            mapped.close()

    @unittest.skipUnless(shutil.which("addr2line"), "host addr2line unavailable")
    def test_real_batched_addr2line_matches_section_offsets(self):
        mapped = ModuleMap(self.debug, self.packed)
        try:
            symbols = [mapped.resolve(name) for name in ["fixture_leaf", "depthchecker_begin"]]
            requests = [(str(self.debug), symbol["section_name"], symbol["value"]) for symbol in symbols]
            result = OfflineAddr2line().lookup(requests)
            for request, expected in zip(requests, ["fixture_leaf", "depthchecker_begin"]):
                self.assertEqual(result[request]["exit_code"], 0)
                self.assertIn(expected, result[request]["output"])
                self.assertIn("fixture.c", result[request]["output"])
                self.assertNotIn("error", result[request])
        finally:
            mapped.close()

    @unittest.skipUnless(shutil.which("gdb"), "host GDB unavailable")
    def test_real_gdb_entry_first_hit_and_missing_hit_fail_closed(self):
        # This owns only a tiny newly launched host fixture. No QEMU/physical
        # target is connected. Linked .text preserves this object's ordering.
        offsets, executable = ModuleMap(self.debug, self.packed), KernelMap(self.executable)
        try:
            base = executable.resolve("fixture_leaf")["address"] - offsets.resolve("fixture_leaf")["image_offset"]
            for marker in ["depthchecker_begin", "depthchecker_end", "depthchecker_complete"]:
                self.assertEqual(base + offsets.resolve(marker)["image_offset"], executable.resolve(marker)["address"])
        finally:
            offsets.close(); executable.close()
        for suffix, target, expected in [("hit", "fixture_leaf", True), ("unreached", "main", False)]:
            output = self.directory / ("gdb-" + suffix)
            spec = self.spec(output)
            spec["module"]["load_base"] = base
            spec["stack_bytes"] = 4096
            spec["scenarios"][0]["targets"][0]["symbol"] = target
            manifest = self.directory / (suffix + ".json")
            manifest.write_text(json.dumps(prepare_manifest(spec)))
            result = subprocess.run(["gdb", "-nx", "-nh", "-batch", "-ex", "file " + str(self.executable),
                                     "-ex", "set disable-randomization off", "-ex", "break main", "-ex", "run",
                                     "-ex", "source " + str(Path(__file__).with_name("gdb_capture.py").resolve()),
                                     "-ex", "depthchecker-run"], env=dict(os.environ, DEPTHCHECKER_CAPTURE_MANIFEST=str(manifest)),
                                    capture_output=True, text=True, timeout=30)
            if "ptrace: Operation not permitted" in result.stderr:
                self.skipTest("host ptrace denied; real GDB smoke cannot run")
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            summary = json.loads((output / "capture-summary.json").read_text())
            self.assertEqual(summary["capture_ok"], expected)
            if expected:
                hit = summary["scenarios"][0]["hits"][target]
                self.assertEqual(hit["first_hit_count"], 1)
                captured = json.loads(Path(hit["capture"]).read_text())
                self.assertEqual(captured["physical_stack"]["frames"][1]["evidence"], "entry-return-address-at-rsp")
                self.assertTrue(Path(hit["capture"]).with_name("stack.bin").is_file())
                self.assertTrue(Path(hit["capture"]).with_name("all-registers.txt").is_file())
            else:
                self.assertEqual(summary["scenarios"][0]["missing_targets"], ["main"])


if __name__ == "__main__":
    unittest.main()
