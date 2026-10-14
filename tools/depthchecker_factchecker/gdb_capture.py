#!/usr/bin/env python3
"""GDB first-hit capture for the prepared depthchecker_factchecker manifest.

Source this file inside GDB after attaching to a newly owned QEMU. Set
DEPTHCHECKER_CAPTURE_MANIFEST to a prepared JSON manifest from elfmap.py.
This file does not attach, resume, detach, or stop any existing VM by itself.
Invoke the registered depthchecker-run GDB command to drive the owned probe.
Dependency breakpoints arm at begin(index), disarm at end(index), and save only
the first entry hit. The complete(pass_mask) marker stops the fresh VM.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import time

try:
    import gdb
except ImportError:
    gdb = None


def x86_u64(value):
    """GDB's Rust register types can expose canonical pointers as signed i64."""
    return int(value) & 0xffff_ffff_ffff_ffff


def bounded_stack(read_memory, start, size, chunk_size=4096):
    if not 16 <= size <= 1024 * 1024:
        raise ValueError("Raw stack capture must be between 16 bytes and 1 MiB")
    data, ranges = bytearray(size), []
    offset = 0
    while offset < size:
        count = min(size - offset, chunk_size - ((start + offset) % chunk_size))
        row = dict(address=start + offset, file_offset=offset, requested_bytes=count)
        try:
            captured = bytes(read_memory(start + offset, count))
            if len(captured) != count:
                raise ValueError(f"Short memory read: {len(captured)}/{count}")
            data[offset:offset + count] = captured
            row.update(captured_bytes=count, readable=True)
        except Exception as error:
            row.update(captured_bytes=0, readable=False, error=str(error))
        ranges.append(row)
        offset += count
    return bytes(data), dict(start=start, requested_bytes=size, end=start + size, ranges=ranges,
                            unreadable_bytes=sum(r["requested_bytes"] for r in ranges if not r["readable"]),
                            unreadable_encoding="Zero-filled holes; zeros are not observations. Consult ranges.")


def entry_frame_walk(read_memory, pc, rsp, rbp, stack_end, max_frames=64):
    """Entry has not pushed RBP: the immediate caller is the word at RSP.

    The inherited RBP belongs to that caller and points to its caller's frame.
    No stack scanning or speculative return addresses are presented as frames.
    """
    if not 1 <= max_frames <= 128:
        raise ValueError("Frame limit must be between 1 and 128")
    frames = [dict(pc=pc, rsp=rsp, rbp=rbp, evidence="exact-breakpoint-function-entry")]
    if max_frames == 1:
        return dict(frames=frames, termination="frame-limit", complete=False)
    try:
        caller = struct.unpack("<Q", bytes(read_memory(rsp, 8)))[0]
    except Exception as error:
        return dict(frames=frames, termination="entry-return-address-unreadable", error=str(error), complete=False)
    if not caller:
        return dict(frames=frames, termination="zero-entry-return-address", complete=False)
    frames.append(dict(pc=caller, stack_word_address=rsp, rbp=rbp, evidence="entry-return-address-at-rsp"))
    seen = set()
    while len(frames) < max_frames:
        if not rbp:
            return dict(frames=frames, termination="zero-frame-pointer", complete=False)
        if rbp in seen:
            return dict(frames=frames, termination="frame-pointer-cycle", complete=False)
        if rbp % 8:
            return dict(frames=frames, termination="misaligned-frame-pointer", complete=False)
        if rbp < rsp or rbp + 16 > stack_end:
            return dict(frames=frames, termination="frame-pointer-outside-capture-range", complete=False)
        seen.add(rbp)
        try:
            next_rbp, return_pc = struct.unpack("<QQ", bytes(read_memory(rbp, 16)))
        except Exception as error:
            return dict(frames=frames, termination="frame-pointer-record-unreadable", error=str(error), complete=False)
        if not return_pc:
            return dict(frames=frames, termination="zero-return-address", complete=False,
                        terminal_record=dict(frame_pointer=rbp, next_frame_pointer=next_rbp, return_address=return_pc))
        frames.append(dict(pc=return_pc, stack_word_address=rbp + 8, rbp=next_rbp, evidence="validated-bounded-frame-pointer-record"))
        if next_rbp and next_rbp <= rbp:
            return dict(frames=frames, termination="non-increasing-frame-pointer", complete=False)
        rbp = next_rbp
    return dict(frames=frames, termination="frame-limit", complete=False)


def recognize_physical_root(walk, known_roots):
    last = walk["frames"][-1]
    labels = list(last.get("module", {}).get("symbols", []))
    if last.get("kernel", {}).get("symbol"):
        labels.append(last["kernel"]["symbol"])
    root = next((label for label in labels if label in known_roots), None)
    terminal = walk["termination"] == "zero-frame-pointer" or (
        walk["termination"] == "zero-return-address" and walk.get("terminal_record", {}).get("next_frame_pointer") == 0)
    if root and terminal:
        walk.update(observed_termination=walk["termination"], termination="recognized-physical-root", complete=True, root_symbol=root)
    return walk


def decode_guest_snapshot(raw, expected_vm_id=None):
    if len(raw) != 1152:
        raise ValueError("VMX debug snapshot must contain 1152 bytes")
    names = ["magic", "schema_version", "snapshot_bytes", "sequence", "vm_id", "cpu_slot", "guest_rip", "guest_rsp",
             "guest_rflags", "guest_cr3", "guest_fs_base", "guest_gs_base", "guest_cr0", "guest_cr2", "guest_cr4", "guest_efer",
             "guest_cs", "guest_ss", "eptp", "hhdm_offset", "guest_stack_base", "stack_host_base", "stack_bytes"]
    snapshot = dict(zip(names, struct.unpack_from("<23Q", raw)))
    if snapshot["magic"] != 0x5041_4e53_4346_4344 or snapshot["schema_version"] != 1 or snapshot["snapshot_bytes"] != len(raw):
        raise ValueError("Unsupported VMX debug snapshot magic/schema/size")
    if expected_vm_id is not None and snapshot["vm_id"] != int(expected_vm_id):
        raise ValueError("VMX snapshot does not belong to the expected probe VM")
    gprs = ["rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15"]
    registers = dict(zip(gprs, struct.unpack_from("<15Q", raw, 184)))
    registers.update(rip=snapshot["guest_rip"], rsp=snapshot["guest_rsp"], eflags=snapshot["guest_rflags"],
                     cr0=snapshot["guest_cr0"], cr2=snapshot["guest_cr2"], cr3=snapshot["guest_cr3"], cr4=snapshot["guest_cr4"],
                     fs_base=snapshot["guest_fs_base"], gs_base=snapshot["guest_gs_base"], efer=snapshot["guest_efer"],
                     cs=snapshot["guest_cs"], ss=snapshot["guest_ss"])
    snapshot["guest_xsave_mask"], snapshot["guest_xsave_size"] = struct.unpack_from("<QQ", raw, 304)
    if snapshot["guest_xsave_size"] != 832:
        raise ValueError("Unsupported guest XSAVE snapshot length")
    snapshot.update(registers=registers, xsave=raw[320:1152], raw_snapshot=raw)
    return snapshot


def parse_addr2line_batch(output, addresses):
    """Bind inline source rows only to the exact requested -a delimiters."""
    records, active, rows = {}, None, []
    observed = []
    for line in output.splitlines():
        if not line.strip():
            # elfutils emits blank separators after the final address block.
            continue
        if re.fullmatch(r"0x[0-9a-fA-F]{8,16}", line):
            if active is not None:
                records[active] = "\n".join(rows) + "\n"
            active, rows = int(line, 16), []
            observed.append(active)
        elif active is None:
            raise ValueError("addr2line emitted a row before its address delimiter")
        else:
            rows.append(line)
    if active is not None:
        records[active] = "\n".join(rows) + "\n"
    if observed != list(addresses) or len(records) != len(addresses):
        raise ValueError("addr2line address delimiters do not match the unique requested addresses")
    if any(not value.strip() or len(value.splitlines()) % 2 for value in records.values()):
        raise ValueError("addr2line function/source rows are incomplete")
    return records


class OfflineAddr2line:
    """Bounded source metadata lookup; allocated symbol labels are separate."""
    def __init__(self, runner=subprocess.run, timeout=10, tool=None):
        self.runner, self.timeout, self.cache = runner, timeout, {}
        # GNU addr2line rescans this very large Rust sidecar for each PC.
        # elfutils accepts the same section/address flags and batches quickly.
        self.tool = tool or shutil.which("eu-addr2line") or "addr2line"

    def lookup(self, requests):
        # Requests are (authenticated ELF path, optional ET_REL section, PC or
        # section offset). This prevents identical offsets in separate sections
        # from being confused and reuses metadata across captured stacks.
        groups = {}
        for path, section, address in dict.fromkeys(requests):
            key = (path, section, address)
            if key not in self.cache:
                groups.setdefault((path, section), []).append(address)
        for (path, section), addresses in groups.items():
            command = [self.tool, "-a", "-C", "-f", "-i", "-e", path]
            if section is not None:
                command.extend(["-j", section])
            command.extend(hex(address) for address in addresses)
            started = time.monotonic()
            try:
                process = self.runner(command, capture_output=True, text=True, timeout=self.timeout)
                common = dict(exit_code=process.returncode, stderr=process.stderr,
                              batch_addresses=len(addresses), elapsed_seconds=time.monotonic()-started,
                              tool=self.tool, method="addr2line -a with validated exact address delimiters")
                if process.returncode:
                    common.update(error="addr2line process returned a nonzero status", error_kind="process-error")
                    records = {}
                else:
                    try:
                        records = parse_addr2line_batch(process.stdout, addresses)
                    except ValueError as error:
                        common.update(error=str(error), error_kind="address-delimiter-validation")
                        records = {}
            except Exception as error:
                common = dict(error=str(error), error_kind="process-timeout" if isinstance(error, subprocess.TimeoutExpired) else "process-exception",
                              batch_addresses=len(addresses), elapsed_seconds=time.monotonic()-started,
                              tool=self.tool, method="addr2line -a with validated exact address delimiters")
                records = {}
            for address in addresses:
                self.cache[(path, section, address)] = dict(common, **(dict(output=records[address]) if address in records else {}))
        return {key: dict(self.cache[key]) for key in requests}


class CaptureController:
    def __init__(self, manifest, backend):
        self.gdb = backend
        self.manifest = manifest
        self.output = Path(manifest["output_dir"]).resolve()
        self.output.mkdir(parents=True, exist_ok=True)
        self.active = None
        self.scenarios = {s["index"]: s for s in manifest["scenarios"]}
        self.records = {i: dict(id=s["id"], index=i, begins=0, ends=0, hits={}, errors=[])
                        for i, s in self.scenarios.items()}
        self.events, self.breakpoints = [], []
        self.complete_seen = False
        self.aborted = False
        self.pass_mask = None
        self.pending = None
        self.errors = []
        self.module_map = None
        self.kernel_map = None
        self.source_metadata = OfflineAddr2line()
        self.guest_snapshot = None
        self.guest_memory = None
        self.install()

    def current_integer(self, register):
        if self.guest_snapshot is not None:
            key = "eflags" if register == "rflags" else register
            return x86_u64(self.guest_snapshot["registers"][key])
        return x86_u64(self.gdb.parse_and_eval("$" + register))

    def event(self, kind, **fields):
        self.events.append(dict(kind=kind, wall_time_unix=time.time(), **fields))

    def summary(self):
        scenarios = []
        for index, scenario in self.scenarios.items():
            record = dict(self.records[index])
            record["missing_targets"] = [t["symbol"] for t in scenario["targets"] if t["symbol"] not in record["hits"]]
            record["capture_ok"] = record["begins"] == 1 and record["ends"] == 1 and not record["missing_targets"] and not record["errors"]
            scenarios.append(record)
        return dict(version=1, complete_marker_seen=self.complete_seen, probe_pass_mask=self.pass_mask,
                    aborted=self.aborted,
                    capture_ok=self.complete_seen and not self.errors and all(s["capture_ok"] for s in scenarios), scenarios=scenarios,
                    errors=self.errors,
                    events=self.events, physical_unwind_method="entry return word at RSP, then bounded frame-pointer records",
                    unwind_scope="Physical stack only. Async logical task chains are not inferred; every termination remains explicit.")

    def write_summary(self):
        (self.output / "capture-summary.json").write_text(json.dumps(self.summary(), indent=2) + "\n")

    def install(self):
        # Keep GDB's host Rust printer/unwinder exposure bounded. We deliberately
        # do not load module DWARF into GDB or run bt; offline addr2line uses it.
        for command in ["set pagination off", "set print frame-arguments none", "set auto-load python-scripts off", "disable pretty-printer"]:
            self.gdb.execute(command, to_string=True)
        controller = self
        class MarkerBreakpoint(self.gdb.Breakpoint):
            def __init__(self, kind, address):
                super().__init__("*0x%x" % address, internal=True)
                self.kind, self.address = kind, address
                self.enabled = kind != "end"
            def stop(self):
                controller.pending = ("marker", self.kind)
                return True
        class LeafBreakpoint(self.gdb.Breakpoint):
            def __init__(self, scenario_index, target):
                super().__init__("*0x%x" % target["resolved"]["address"], internal=True)
                self.scenario_index, self.target = scenario_index, target
                self.address = target["resolved"]["address"]
                self.enabled = False
            def stop(self):
                if controller.active != self.scenario_index:
                    return False
                record = controller.records[self.scenario_index]
                if not controller.scenarios[self.scenario_index].get("allow_cross_thread", False) and controller.gdb.selected_thread().global_num != record.get("begin_thread"):
                    return False
                controller.pending = ("leaf", self)
                return True
        class RunCommand(self.gdb.Command):
            def __init__(self):
                super().__init__("depthchecker-run", controller.gdb.COMMAND_USER)
            def invoke(self, argument, from_tty):
                controller.run()
        self.run_command = RunCommand()
        class GuestHookBreakpoint(self.gdb.Breakpoint):
            def __init__(self, address):
                super().__init__("*0x%x" % address, internal=True)
            def stop(self):
                controller.pending = ("guest", None)
                return True
        self.marker_breakpoints = {}
        for kind in ["begin", "end", "complete"]:
            breakpoint = MarkerBreakpoint(kind, self.manifest["markers"][kind]["address"])
            self.breakpoints.append(breakpoint)
            self.marker_breakpoints[kind] = breakpoint
        self.leaves = []
        for index, scenario in self.scenarios.items():
            for target in scenario["targets"]:
                breakpoint = LeafBreakpoint(index, target)
                self.breakpoints.append(breakpoint)
                self.leaves.append(breakpoint)
        if self.manifest.get("vmx_handoff"):
            self.guest_hook = GuestHookBreakpoint(self.manifest["vmx_handoff"]["hook"]["address"])
            self.sync_guest_whitelist()
            self.gdb.selected_inferior().write_memory(self.manifest["vmx_handoff"]["enable"]["address"], struct.pack("<Q", 1))
        self.event("installed", marker_addresses={k: v["address"] for k, v in self.manifest["markers"].items()})
        # Runner can omit gate activation and open the gate itself. Only the
        # explicitly named exported probe AtomicU64 is writable here.
        gate = self.manifest["markers"].get("gate")
        if gate and self.manifest.get("release_gate", True):
            if gate["size"] != 8:
                raise ValueError("Debugger gate must be an exported 8-byte AtomicU64")
            self.gdb.selected_inferior().write_memory(gate["address"], struct.pack("<Q", 1))
            self.event("gate-released", address=gate["address"])
        self.write_summary()

    def run(self):
        # Breakpoint.stop only queues an observation. Arming/disarming other
        # breakpoints happens here, outside GDB's stop callback restrictions.
        while not self.complete_seen and not self.aborted:
            self.pending = None
            try:
                self.gdb.execute("continue", to_string=True)
            except Exception as error:
                self.errors.append("continue failed: " + str(error))
                break
            if self.pending is None:
                self.errors.append("Inferior stopped outside the expected probe markers/targets")
                break
            kind, value = self.pending
            if kind == "marker":
                self.marker(value)
            elif kind == "guest":
                self.handle_guest_hook()
            else:
                self.handle_leaf(value)
            self.sync_guest_whitelist()
        self.write_summary()

    def handle_leaf(self, breakpoint):
        record = self.records[breakpoint.scenario_index]
        try:
            captured = self.capture(breakpoint.scenario_index, breakpoint.target)
            if captured is not None:
                breakpoint.enabled = False
                record["hits"][breakpoint.target["symbol"]] = dict(first_hit_count=1, capture=captured)
        except Exception as error:
            breakpoint.enabled = False
            record["errors"].append("capture failed: " + str(error))
        self.write_summary()

    def sync_guest_whitelist(self):
        handoff = self.manifest.get("vmx_handoff")
        if not handoff:
            return
        addresses = sorted({bp.address for bp in self.breakpoints if bp.enabled})
        if len(addresses) > 64:
            raise ValueError("VMX capture whitelist exceeds 64 addresses")
        inferior = self.gdb.selected_inferior()
        # Publish count last. The inferior is stopped for the whole update.
        inferior.write_memory(handoff["address_count"]["address"], struct.pack("<Q", 0))
        inferior.write_memory(handoff["addresses"]["address"], struct.pack("<64Q", *(addresses + [0] * (64-len(addresses)))))
        inferior.write_memory(handoff["address_count"]["address"], struct.pack("<Q", len(addresses)))

    def handle_guest_hook(self):
        # Fixed repr(C), versioned kernel handoff: 23 u64 headers, 15 GPRs,
        # XSAVE mask/size and 832 raw bytes. This reads only the L1 snapshot.
        pointer = x86_u64(self.gdb.parse_and_eval("$rdi"))
        raw = bytes(self.gdb.selected_inferior().read_memory(pointer, 1152))
        snapshot = decode_guest_snapshot(raw, self.manifest.get("expected_vm_id"))
        registers = snapshot["registers"]
        from vmx_memory import GuestMemory
        memory = GuestMemory(self.gdb.selected_inferior().read_memory, snapshot["guest_cr3"], snapshot["eptp"],
                             snapshot["hhdm_offset"], snapshot["guest_cr4"])
        self.guest_snapshot, self.guest_memory = snapshot, memory
        try:
            candidates = [bp for bp in self.breakpoints if bp.enabled and bp.address == registers["rip"]]
            if len(candidates) != 1:
                raise ValueError("Guest snapshot RIP does not identify exactly one armed breakpoint")
            breakpoint = candidates[0]
            self.event("vmx-guest-handoff", snapshot_pointer=pointer, sequence=snapshot["sequence"],
                       vm_id=snapshot["vm_id"], cpu_slot=snapshot["cpu_slot"], guest_rip=registers["rip"])
            if hasattr(breakpoint, "kind"):
                self.marker(breakpoint.kind)
            elif self.active == breakpoint.scenario_index:
                self.handle_leaf(breakpoint)
                if breakpoint.target["symbol"] not in self.records[breakpoint.scenario_index]["hits"]:
                    # A same-RIP L2 resume must see the restored original
                    # opcode. An ignored hit cannot leave INT3 armed without
                    # a guest single-step mechanism; fail coverage explicitly.
                    breakpoint.enabled = False
                    self.records[breakpoint.scenario_index]["errors"].append(
                        "Guest dependency entry was not accepted; breakpoint disabled before same-RIP resume")
            else:
                raise ValueError("Guest leaf belongs to an inactive scenario")
        except Exception as error:
            self.errors.append("VMX guest handoff failed: " + str(error))
            self.aborted = True
        finally:
            self.guest_snapshot = self.guest_memory = None
        self.write_summary()

    def marker(self, kind):
        argument = self.current_integer("rdi")
        self.event(kind, argument=argument)
        if kind == "complete":
            self.complete_seen, self.pass_mask = True, argument
            for leaf in self.leaves:
                leaf.enabled = False
            for marker in self.marker_breakpoints.values():
                marker.enabled = False
            self.write_summary()
            return True
        if argument not in self.scenarios:
            self.event("unknown-scenario-index", argument=argument)
            self.errors.append("Unknown scenario marker index: " + str(argument))
            return False
        record = self.records[argument]
        if kind == "begin":
            self.marker_breakpoints["begin"].enabled = False
            self.marker_breakpoints["end"].enabled = True
            if self.active is not None:
                self.records[self.active]["errors"].append("begin marker arrived before the active scenario ended")
            record["begins"] += 1
            record["begin_thread"] = self.gdb.selected_thread().global_num
            self.active = argument
            for leaf in self.leaves:
                leaf.enabled = leaf.scenario_index == argument and leaf.target["symbol"] not in record["hits"]
        else:
            self.marker_breakpoints["end"].enabled = False
            self.marker_breakpoints["begin"].enabled = True
            record["ends"] += 1
            if self.active != argument:
                record["errors"].append("end marker does not match the active scenario")
            self.active = None
            for leaf in self.leaves:
                leaf.enabled = False
        self.write_summary()
        return False

    def label_frame(self, frame):
        pc = frame["pc"]
        # Return addresses refer just after the call. PC-1 is appropriate for
        # caller labels, while the dependency entry uses its exact PC.
        lookup_pc = pc if frame["evidence"] == "exact-breakpoint-function-entry" else pc - 1
        label = self.module_map.at(lookup_pc)
        if label:
            frame["module"] = label
            if label["executable"]:
                return (self.manifest["module"]["debug_elf"], label["section_name"], label["section_offset"])
        elif self.kernel_map and (kernel_label := self.kernel_map.at(lookup_pc)):
            frame["kernel"] = kernel_label
            return (self.manifest["kernel_map"]["kernel_elf"], None, lookup_pc)
        else:
            try:
                frame["kernel_or_external_symbol"] = self.gdb.execute("info symbol 0x%x" % lookup_pc, to_string=True).strip()
            except Exception as error:
                frame["symbol_error"] = str(error)

    def label_frames(self, frames):
        requests = [(frame, self.label_frame(frame)) for frame in frames]
        metadata = self.source_metadata.lookup([key for _, key in requests if key is not None])
        for frame, key in requests:
            if key is not None:
                frame["offline_addr2line"] = metadata[key]

    def scoped_identity(self, rsp):
        """Read current kernel identity; never execute a guest/helper function.

        CURRENT is meaningful only for a host-carried native continuation.
        Its owned stack must contain the observed RSP before identity is
        attributed. A Hull's private/current globals must not be mistaken for
        a host thread merely because it uses the same AP.
        """
        if self.guest_snapshot is not None:
            snapshot = self.guest_snapshot
            return dict(method="versioned VMX guest snapshot at scoped L1 handoff", context="L2-Hull-guest",
                        native_thread_observed=False, vm_id=snapshot["vm_id"], cpu_slot=snapshot["cpu_slot"],
                        snapshot_sequence=snapshot["sequence"], cr3=snapshot["guest_cr3"],
                        guest_stack=dict(start=snapshot["guest_stack_base"], end=snapshot["guest_stack_base"]+snapshot["stack_bytes"],
                                         host_alias_start=snapshot["stack_host_base"], bytes=snapshot["stack_bytes"]),
                        note="Host CURRENT thread table is not consulted for a Hull guest.", errors=[])
        result = dict(method="GS-backed PerCpu self-pointer, then DWARF Thread fields with active-stack containment",
                      native_thread_observed=False, errors=[])
        try:
            result["cr3"] = self.current_integer("cr3")
        except Exception as error:
            result["errors"].append("CR3 unavailable: " + str(error))
        try:
            gs = self.current_integer("gs_base")
            result["gs_base"] = gs
            if not gs:
                raise ValueError("GS base is zero")
            value = self.gdb.Value(gs).cast(self.gdb.lookup_type("TRUEOS::percpu::PerCpu").pointer()).dereference()
            if x86_u64(value["self_ptr"]) != gs:
                raise ValueError("PerCpu self-pointer does not match GS base")
            slot = int(value["cpu_index"])
            result.update(cpu_slot=slot, lapic_id=int(value["lapic_id"]))
            def symbol_value(name):
                symbol = self.gdb.lookup_global_symbol(name)
                if symbol is None:
                    symbol = self.gdb.lookup_static_symbol(name)
                if symbol is None:
                    raise ValueError("Kernel DWARF symbol unavailable: " + name)
                return symbol.value()
            current = symbol_value("TRUEOS::r::threads::CURRENT")
            if not 0 <= slot < current.type.sizeof // 8:
                raise ValueError("CPU slot is outside CURRENT table")
            inferior = self.gdb.selected_inferior()
            pointer = struct.unpack("<Q", bytes(inferior.read_memory(x86_u64(current.address) + slot * 8, 8)))[0]
            result["current_thread_pointer"] = pointer
            try:
                tags = symbol_value("TRUEOS::hv::CURRENT_VM_ID_BY_CPU")
                tag = bytes(inferior.read_memory(x86_u64(tags.address) + slot, 1))[0]
                result["vmx_vm_id_tag"] = tag
                result["vmx_vm_id"] = tag - 1 if tag else None
            except Exception as error:
                result["errors"].append("VMX slot tag unavailable: " + str(error))
            if not pointer:
                result["context"] = "no-host-native-continuation"
                return result
            thread = self.gdb.Value(pointer).cast(self.gdb.lookup_type("TRUEOS::r::threads::Thread").pointer()).dereference()
            backing = thread["stack"]["backing"]
            stack_start = x86_u64(backing["ptr"]["pointer"])
            stack_len = int(thread["stack"]["len"])
            if not 0 < stack_len <= 64 * 1024 * 1024:
                raise ValueError("Invalid native stack length")
            stack_end = stack_start + stack_len
            result["candidate_stack"] = dict(start=stack_start, end=stack_end, bytes=stack_len)
            if not stack_start <= rsp < stack_end:
                result["context"] = "CURRENT candidate does not own the captured physical stack; Hull/host identity unassigned"
                return result
            vm = thread["vm_id"]
            result.update(native_thread_observed=True, context="host-carried-native-continuation",
                          logical_thread_id=int(thread["id"]), carrier_slot=int(thread["carrier"]),
                          vm_id_field=dict(type=str(vm.type), display=str(vm),
                                           raw_hex=bytes(inferior.read_memory(x86_u64(vm.address), vm.type.sizeof)).hex()),
                          child_saved_rsp=x86_u64(thread["child"]), parent_saved_rsp=x86_u64(thread["parent"]),
                          active_stack=dict(start=stack_start, end=stack_end, bytes=stack_len))
        except Exception as error:
            result["errors"].append("Scoped identity unavailable: " + str(error))
        return result

    def read_physical_stack_memory(self, address, size):
        if self.guest_snapshot is None:
            return self.gdb.selected_inferior().read_memory(address, size)
        snapshot = self.guest_snapshot
        base, end = snapshot["guest_stack_base"], snapshot["guest_stack_base"] + snapshot["stack_bytes"]
        if snapshot["stack_host_base"] and base <= address and address + size <= end:
            return self.gdb.selected_inferior().read_memory(snapshot["stack_host_base"] + address - base, size)
        return self.guest_memory.read_memory(address, size)

    def capture(self, scenario_index, target):
        if self.module_map is None:
            # The manifest already authenticated bytes; verify its pinned hashes
            # again before resolving the captured addresses.
            from elfmap import ModuleMap
            module = self.manifest["module"]
            self.module_map = ModuleMap(module["debug_elf"], module["packed_elf"], module["load_base"])
            evidence = self.module_map.evidence()
            if evidence["debug_sha256"] != module["debug_sha256"] or evidence["packed_sha256"] != module["packed_sha256"]:
                raise ValueError("Module sidecar changed since manifest preparation")
            if self.manifest.get("kernel_map"):
                from elfmap import KernelMap
                self.kernel_map = KernelMap(self.manifest["kernel_map"]["kernel_elf"])
                if self.kernel_map.evidence()["sha256"] != self.manifest["kernel_map"]["sha256"]:
                    raise ValueError("Kernel ELF changed since manifest preparation")
        pc, rsp, rbp = [self.current_integer(r) for r in ["rip", "rsp", "rbp"]]
        if pc != target["resolved"]["address"]:
            raise ValueError("Stop is not at the exact dependency function entry")
        ident = self.scenarios[scenario_index]["id"]
        inferior = self.gdb.selected_inferior()
        identity = self.scoped_identity(rsp)
        requested_limit = int(self.manifest.get("stack_bytes", 65536))
        size = requested_limit
        if identity.get("native_thread_observed"):
            # Do not include the known unmapped upper guard as fabricated zeros.
            size = min(size, identity["active_stack"]["end"] - rsp)
        if identity.get("guest_stack"):
            if not identity["guest_stack"]["start"] <= rsp < identity["guest_stack"]["end"]:
                raise ValueError("Captured guest RSP is outside the snapshot's owned guest stack")
            size = min(size, identity["guest_stack"]["end"] - rsp)
        reader = self.read_physical_stack_memory
        walk = entry_frame_walk(reader, pc, rsp, rbp, rsp + size, int(self.manifest.get("max_frames", 64)))
        if target.get("scope") == "kernel" and target.get("require_module_caller", True) and not any(
                (label := self.module_map.at(frame["pc"] - 1)) and label["executable"] for frame in walk["frames"][1:]):
            self.event("ignored-kernel-hit-without-probe-caller", scenario_index=scenario_index, symbol=target["symbol"], pc=pc)
            return None
        capture_dir = self.output / (ident + "-" + hashlib.sha256(target["symbol"].encode()).hexdigest()[:12])
        capture_dir.mkdir(exist_ok=False)
        values, errors = {}, {}
        if self.guest_snapshot is not None:
            snapshot = self.guest_snapshot
            enumeration = "VMX guest architectural snapshot; host debugger registers are separate"
            values = {name: dict(integer=value, display=hex(value)) for name, value in snapshot["registers"].items()}
            xsave = snapshot["xsave"]
            xstate_bv = struct.unpack_from("<Q", xsave, 512)[0]
            legacy_fxsave = snapshot["guest_xsave_mask"] == 0
            values["mxcsr"] = dict(integer=struct.unpack_from("<I", xsave, 24)[0], xstate_present=legacy_fxsave or bool(xstate_bv & 2))
            for name, offset, fmt in [("fctrl", 0, "<H"), ("fstat", 2, "<H"), ("ftag", 4, "<B"),
                                      ("fop", 6, "<H"), ("fip", 8, "<Q"), ("fdp", 16, "<Q")]:
                values[name] = dict(integer=struct.unpack_from(fmt, xsave, offset)[0])
            for index in range(16):
                values["xmm%d" % index] = dict(raw_hex=xsave[160+index*16:176+index*16].hex(), xstate_present=legacy_fxsave or bool(xstate_bv & 2))
                values["ymm%d_high128" % index] = dict(raw_hex=xsave[576+index*16:592+index*16].hex(), xstate_present=bool(xstate_bv & 4))
            for index in range(8):
                values["st%d" % index] = dict(raw80_hex=xsave[32+index*16:42+index*16].hex(), xstate_present=legacy_fxsave or bool(xstate_bv & 1))
            values["xsave_state"] = dict(requested_mask=snapshot["guest_xsave_mask"], xstate_bv=xstate_bv,
                                         raw_file="guest-xsave.bin", raw_bytes=len(xsave),
                                         format="FXSAVE legacy image plus reserved tail" if legacy_fxsave else "standard XSAVE image",
                                         note="Inactive XSAVE components use architectural defaults; their raw bytes may be stale and are not asserted as live registers.")
            (capture_dir / "guest-xsave.bin").write_bytes(xsave)
            (capture_dir / "guest-snapshot.bin").write_bytes(snapshot["raw_snapshot"])
            (capture_dir / "all-registers.txt").write_text("VMX GUEST REGISTERS (L1 debugger registers are separate)\n" + json.dumps(values, indent=2) + "\n")
            (capture_dir / "host-debugger-registers.txt").write_text(self.gdb.execute("info all-registers", to_string=True))
            names = []
        else:
            (capture_dir / "all-registers.txt").write_text(self.gdb.execute("info all-registers", to_string=True))
            enumeration = "gdb architecture register descriptors"
            try:
                names = [r.name for r in self.gdb.selected_frame().architecture().registers()]
            except Exception:
                enumeration = "fallback names; all-registers.txt retains the remote's complete supported register display"
                names = ["rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp", "rip", "eflags", "cs", "ss", "ds", "es", "fs", "gs", "fs_base", "gs_base", "cr0", "cr2", "cr3", "cr4", "cr8", "mxcsr", "fctrl", "fstat", "ftag"] + ["xmm%d" % i for i in range(32)] + ["st%d" % i for i in range(8)]
        for name in names:
            try:
                value = self.gdb.selected_frame().read_register(name)
                row = dict(display=str(value))
                try:
                    row["integer"] = x86_u64(value)
                except Exception:
                    pass
                values[name] = row
            except Exception as error:
                errors[name] = str(error)
        stack, stack_info = bounded_stack(reader, rsp, size)
        (capture_dir / "stack.bin").write_bytes(stack)
        stack_info["sha256"] = hashlib.sha256(stack).hexdigest()
        stack_info["configured_limit_bytes"] = requested_limit
        if self.guest_snapshot is not None:
            snapshot = self.guest_snapshot
            stack_info["address_space"] = "Guest virtual addresses; reads use the kernel-exported host HHDM stack alias"
            for item in stack_info["ranges"]:
                item["host_read_address"] = snapshot["stack_host_base"] + item["address"] - snapshot["guest_stack_base"]
        self.label_frames(walk["frames"])
        known_roots = self.manifest.get("known_physical_roots", ["trueos_thread_context_start", "trueos_vmx_guest_entry", "_start"])
        recognize_physical_root(walk, known_roots)
        thread = self.gdb.selected_thread()
        result = dict(scenario_id=ident, scenario_index=scenario_index, target=target, first_hit_count=1,
                      timestamp_unix=time.time(), thread=dict(global_num=thread.global_num, ptid=thread.ptid),
                      scoped_identity=identity,
                      register_origin="vmx-guest-snapshot" if self.guest_snapshot is not None else "direct-qemu-gdb-current-core",
                      registers=dict(enumeration=enumeration, values=values, errors=errors, complete_text="all-registers.txt"),
                      raw_stack=stack_info, physical_stack=walk,
                      boundary="Function entry before the prologue; immediate caller read at RSP, not RBP.",
                      limits=("Guest GPRs/RIP/RSP/RFLAGS, listed VMCS/CR2 fields and supported x87/SSE/AVX state come from the versioned VMX snapshot. Other MSRs/debug registers are not captured. Host GDB registers are separate. "
                              if self.guest_snapshot is not None else "Registers are those exposed by the direct QEMU GDB stop. ") +
                             "Unreadable raw-stack ranges and physical unwind termination are explicit; no async logical backtrace is inferred.")
        (capture_dir / "capture.json").write_text(json.dumps(result, indent=2) + "\n")
        return str(capture_dir / "capture.json")


def install_from_environment():
    manifest_path = Path(os.environ["DEPTHCHECKER_CAPTURE_MANIFEST"]).resolve()
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    manifest = json.loads(manifest_path.read_text())
    if manifest.get("version") != 1 or not manifest.get("module", {}).get("allocated_sidecars_match"):
        raise ValueError("Capture requires a version-1 prepared, authenticated module manifest")
    return CaptureController(manifest, gdb)


if gdb is not None and os.environ.get("DEPTHCHECKER_CAPTURE_MANIFEST"):
    DEPTHCHECKER_CONTROLLER = install_from_environment()
