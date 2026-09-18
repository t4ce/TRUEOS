#!/usr/bin/env python3
"""Validate the private WC3 launcher Gate-0 source and probe bytecode."""

from __future__ import annotations

from pathlib import Path
import re
import struct


ROOT = Path(__file__).resolve().parent.parent
EXPECTED_SHA256 = "5a8cca727c719ae054adf8d15523a8e3099745225e2f4f9885f88774aa6f36d9"
STEP_GUEST = 0xC3573311
STEP_HOST_ACK = 0xA55AC33C
STEP_RESUME = 0x52E50A32
STACK_MARKER = 0x51ACC032
HOST_MEMORY_MARKER = 0xF5BA5E32


def require(source: str, needle: str, label: str) -> None:
    if needle not in source:
        raise ValueError(f"missing {label}: {needle}")


def probe_bytes(source: str) -> bytes:
    match = re.search(r"pub\(super\) const PROBE_CODE: &\[u8\] = &\[(.*?)\n\];", source, re.S)
    if not match:
        raise ValueError("probe byte array not found")
    body = re.sub(r"//[^\n]*", "", match.group(1))
    values = [int(token, 16) for token in re.findall(r"0x([0-9A-Fa-f]{2})", body)]
    return bytes(values)


def emulate_probe(code: bytes) -> None:
    registers = {"eax": 0, "ebx": 0, "ecx": 0}
    fs: dict[int, int] = {}
    stack: list[int] = []
    ip = 0
    zero = False
    vmcalls = 0
    while ip < len(code):
        opcode = code[ip]
        if opcode in (0xB8, 0xBB):
            value = struct.unpack_from("<I", code, ip + 1)[0]
            registers["eax" if opcode == 0xB8 else "ebx"] = value
            ip += 5
        elif opcode == 0x53:
            stack.append(registers["ebx"])
            ip += 1
        elif opcode == 0x59:
            registers["ecx"] = stack.pop()
            ip += 1
        elif code[ip : ip + 3] == b"\x64\x89\x0d":
            offset = struct.unpack_from("<I", code, ip + 3)[0]
            fs[offset] = registers["ecx"]
            ip += 7
        elif code[ip : ip + 3] == b"\x64\x8b\x0d":
            offset = struct.unpack_from("<I", code, ip + 3)[0]
            registers["ecx"] = fs.get(offset, 0)
            ip += 7
        elif code[ip : ip + 3] == b"\x0f\x01\xc1":
            vmcalls += 1
            if vmcalls == 1:
                if registers != {"eax": STEP_GUEST, "ebx": STACK_MARKER, "ecx": STACK_MARKER}:
                    raise ValueError(f"first VMCALL registers differ: {registers}")
                if fs.get(0x100) != STACK_MARKER:
                    raise ValueError("first VMCALL lacks the FS-relative guest write")
                registers["eax"] = STEP_HOST_ACK
                fs[0x104] = HOST_MEMORY_MARKER
                ip += 3
            elif vmcalls == 2:
                if registers["eax"] != STEP_RESUME or registers["ecx"] != HOST_MEMORY_MARKER:
                    raise ValueError(f"resume VMCALL state differs: {registers}")
                return
            else:
                raise ValueError("probe made an unexpected VMCALL")
        elif opcode == 0x3D:
            zero = registers["eax"] == struct.unpack_from("<I", code, ip + 1)[0]
            ip += 5
        elif opcode == 0x75:
            displacement = struct.unpack_from("<b", code, ip + 1)[0]
            ip = ip + 2 if zero else ip + 2 + displacement
        elif code[ip : ip + 2] == b"\x0f\x0b":
            raise ValueError("probe reached UD2 before its second VMCALL")
        else:
            raise ValueError(f"unknown probe opcode at {ip:#x}: {code[ip:ip + 8].hex()}")
    raise ValueError("probe ended before its second VMCALL")


def check() -> None:
    cargo = (ROOT / "Cargo.toml").read_text()
    hv = (ROOT / "src/hv/mod.rs").read_text()
    memory = (ROOT / "src/hv/memory.rs").read_text()
    guest32 = (ROOT / "src/hv/wc3/guest32.rs").read_text()
    launcher_source = (ROOT / "src/hv/wc3/launcher.rs").read_text()
    pe32 = (ROOT / "src/hv/wc3/pe32.rs").read_text()
    imports = (ROOT / "src/hv/wc3/imports.rs").read_text()
    thunks = (ROOT / "src/hv/wc3/thunk32.rs").read_text()
    shell = (ROOT / "src/shell2/cmds/wc3.rs").read_text()
    preflight = (ROOT / "tools/wc3-pe-preflight.txt").read_text(errors="replace")

    require(cargo, "wc3 = []", "private Cargo feature")
    require(hv, '#[cfg(feature = "wc3")]\nmod wc3;', "feature-gated module")
    require(hv, "pub fn start_wc3_launcher_test", "launcher test entry point")
    require(hv, "VmBootMode::Wc3Probe", "WC3 boot mode")
    require(hv, "wc3::handle_vmcall(vm_id)", "private VMCALL dispatch")
    require(hv, "if wc3_guest { 0xC09B } else { 0xA09B }", "32-bit CS access rights")
    require(memory, '"wc3-gate0-code"', "private executable EPT backing")
    require(memory, '"wc3-gate0-teb"', "private data EPT backing")
    require(memory, "| PT_ENTRY_NO_EXECUTE", "non-executable TEB mapping")
    require(guest32, "PROBE_TEB_VA", "controlled FS base")

    launcher = preflight.split("================================================================\nWar3.exe", 1)[0]
    require(launcher, EXPECTED_SHA256 + "  Warcraft III.exe", "fixed launcher digest")
    require(launcher, "AddressOfEntryPoint: 0x2144", "fixed launcher entry RVA")
    require(launcher, "ImageBase: 0x400000", "fixed launcher image base")
    require(launcher, "BaseReloc [\n]", "absent launcher relocations")
    require(launcher, "TLSDirectory {\n}", "absent launcher PE TLS")
    require(hv, "VmBootMode::Wc3Launcher", "separate launcher boot mode")
    require(hv, "wc3::handle_launcher_vmcall(vm_id)", "private launcher VMCALL dispatch")
    require(hv, "wc3: launcher entered", "launcher entry trace")
    require(memory, '"wc3-gate1a-image"', "private launcher image EPT backing")
    require(memory, '"wc3-gate1a-thunks"', "private launcher thunk EPT backing")
    require(memory, '"wc3-gate1a-teb"', "private launcher TEB EPT backing")
    require(launcher_source, "Sha256::digest(bytes)", "launcher hash validation")
    require(launcher_source, "gate-1a complete", "first-import completion trace")
    require(pe32, "IMAGE_BASE: u32 = 0x0040_0000", "fixed launcher image base")
    require(pe32, "ENTRY_RVA: u32 = 0x2144", "fixed launcher entry RVA")
    require(pe32, "pe ordinal import unsupported", "fail-closed ordinal import handling")
    require(imports, "find_get_version", "GetVersion import assertion")
    require(thunks, "0x0F,\n        0x01,\n        0xC1", "32-bit VMCALL thunk opcode")
    require(shell, "wc3_launcher: usage", "private launcher Shell2 command")

    emulate_probe(probe_bytes(guest32))
    print(
        "wc3-launcher-gate0: target=fixed feature=private code=32bit "
        "stack=roundtrip fs=read-write vmcall=two-stage resume=verified"
    )


if __name__ == "__main__":
    try:
        check()
    except (OSError, ValueError, IndexError, struct.error) as error:
        raise SystemExit(f"wc3-launcher-gate0: {error}") from error
