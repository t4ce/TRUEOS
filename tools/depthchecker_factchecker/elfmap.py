#!/usr/bin/env python3
"""Authenticate Blueprint ET_REL sidecars and resolve loaded function entries.

Offsets follow hv/blueprint/blueprint.rs: allocated sections, in ELF order,
each aligned before placement. Debug stripping can change section indices;
the ordered allocated contents must still agree byte for byte.
"""
import argparse
from bisect import bisect_right
import hashlib
import json
import mmap
from pathlib import Path
import struct


class ELF:
    def __init__(self, path, accepted_types=(1,)):
        self.path = Path(path).resolve()
        self.stream = self.path.open("rb")
        try:
            self.data = mmap.mmap(self.stream.fileno(), 0, access=mmap.ACCESS_READ)
            header = struct.unpack_from("<16sHHIQQQIHHHHHH", self.data)
            if header[0][:6] != b"\x7fELF\x02\x01" or header[1] not in accepted_types or header[2] != 62:
                raise ValueError(f"Expected little-endian x86_64 ELF type {accepted_types}: {self.path}")
            table, width, count, strings = header[6], header[11], header[12], header[13]
            if width != 64:
                raise ValueError("Unsupported ELF section-header width")
            first = struct.unpack_from("<IIQQQQIIQQ", self.data, table)
            count = count or first[5]
            strings = first[6] if strings == 0xffff else strings
            if table + count * width > len(self.data) or strings >= count:
                raise ValueError("ELF section table out of bounds")
            raw = [struct.unpack_from("<IIQQQQIIQQ", self.data, table + i * width) for i in range(count)]
            names = self.payload(raw[strings][4], raw[strings][5])
            self.sections = []
            for i, row in enumerate(raw):
                name, kind, flags, _, offset, size, link, info, align, entsize = row
                end = names.find(b"\0", name)
                if name >= len(names) or end < 0:
                    raise ValueError("ELF section name out of bounds")
                self.sections.append(dict(index=i, name=names[name:end].decode(), type=kind, flags=flags, address=row[3],
                                          file_offset=offset, size=size, link=link, info=info, align=align or 1, entsize=entsize))
        except Exception:
            self.close()
            raise

    def close(self):
        if getattr(self, "data", None) is not None:
            self.data.close()
            self.data = None
        self.stream.close()

    def payload(self, offset, size):
        if offset + size > len(self.data):
            raise ValueError("ELF payload out of bounds")
        return self.data[offset:offset + size]

    def section_digest(self, section):
        if section["type"] == 8:
            return "NOBITS"
        return hashlib.sha256(self.payload(section["file_offset"], section["size"])).hexdigest()

    def symbols(self):
        for table in self.sections:
            if table["type"] != 2:
                continue
            if table["entsize"] != 24 or table["size"] % 24 or table["link"] >= len(self.sections):
                raise ValueError("Invalid ELF symbol table")
            names_section = self.sections[table["link"]]
            names = self.payload(names_section["file_offset"], names_section["size"])
            extended = next((s for s in self.sections if s["type"] == 18 and s["link"] == table["index"]), None)
            for number in range(table["size"] // 24):
                name, info, other, section, value, size = struct.unpack_from("<IBBHQQ", self.data, table["file_offset"] + number * 24)
                if section == 0xffff:
                    if not extended or number * 4 + 4 > extended["size"]:
                        raise ValueError("Missing extended symbol section index")
                    section = struct.unpack_from("<I", self.data, extended["file_offset"] + number * 4)[0]
                if not name:
                    continue
                end = names.find(b"\0", name)
                if name >= len(names) or end < 0:
                    raise ValueError("ELF symbol name out of bounds")
                yield dict(name=names[name:end].decode(), symbol_index=number, section_index=section,
                           value=value, size=size, type=info & 15, binding=info >> 4, visibility=other & 3)


class ModuleMap:
    def __init__(self, debug_elf, packed_elf, load_base=0):
        self.debug = ELF(debug_elf)
        try:
            self.packed = ELF(packed_elf)
            self.load_base = int(load_base, 0) if isinstance(load_base, str) else int(load_base)
            self.sections, self.debug_sections = [], {}
            debug = [s for s in self.debug.sections if s["flags"] & 2]
            packed = [s for s in self.packed.sections if s["flags"] & 2]
            if len(debug) != len(packed):
                raise ValueError("Debug/packed allocated section counts differ")
            cursor = 0
            for d, p in zip(debug, packed):
                fields = ("name", "type", "flags", "size", "align")
                if any(d[k] != p[k] for k in fields) or self.debug.section_digest(d) != self.packed.section_digest(p):
                    raise ValueError(f"Debug/packed allocated section mismatch: {d['name']} / {p['name']}")
                align = p["align"]
                if align & (align - 1):
                    raise ValueError("Allocated section alignment is not a power of two")
                cursor = (cursor + align - 1) & -align
                row = {k: p[k] for k in fields}
                row.update(packed_index=p["index"], debug_index=d["index"], image_offset=cursor,
                           address=self.load_base + cursor, content_sha256=self.packed.section_digest(p))
                self.sections.append(row)
                self.debug_sections[d["index"]] = row
                cursor += p["size"]
            self.allocated_span = cursor
            self.function_rows = []
            self.named_symbols = {}
            for symbol in self.debug.symbols():
                section = self.debug_sections.get(symbol["section_index"])
                if not section:
                    continue
                if symbol["value"] + symbol["size"] > section["size"]:
                    raise ValueError(f"Symbol outside allocated section: {symbol['name']}")
                row = dict(symbol, address=section["address"] + symbol["value"],
                           image_offset=section["image_offset"] + symbol["value"],
                           packed_section_index=section["packed_index"], section_name=section["name"], executable=bool(section["flags"] & 4))
                self.named_symbols.setdefault(symbol["name"], []).append(row)
                if symbol["type"] == 2 and symbol["size"] and row["executable"]:
                    self.function_rows.append(row)
            self.function_rows.sort(key=lambda row: row["address"])
            self.starts = [row["address"] for row in self.function_rows]
            self.maximum_ends = []
            maximum = 0
            for row in self.function_rows:
                maximum = max(maximum, row["address"] + row["size"])
                self.maximum_ends.append(maximum)
        except Exception:
            self.close()
            raise

    def close(self):
        self.debug.close()
        if getattr(self, "packed", None):
            self.packed.close()

    def resolve(self, name, function=True):
        matches = self.named_symbols.get(name, [])
        if function:
            matches = [s for s in matches if s["type"] == 2 and s["size"] and s["executable"]]
        if len(matches) != 1:
            raise ValueError(f"Symbol {name!r} resolves to {len(matches)} valid {'function entries' if function else 'allocated symbols'}")
        return dict(matches[0])

    def at(self, address):
        address = int(address)
        section = next((s for s in self.sections if s["address"] <= address < s["address"] + s["size"]), None)
        if not section:
            return None
        # Aliases/overlapping symbols can be ambiguous: keep every covering
        # candidate instead of inventing a unique function identity.
        candidates = []
        i = bisect_right(self.starts, address) - 1
        while i >= 0 and self.maximum_ends[i] > address:
            row = self.function_rows[i]
            if row["address"] <= address < row["address"] + row["size"]:
                candidates.append(row["name"])
            i -= 1
        return dict(section_name=section["name"], section_offset=address - section["address"],
                    image_offset=address - self.load_base, executable=bool(section["flags"] & 4), symbols=candidates)

    def evidence(self):
        def sha(path):
            with path.open("rb") as stream:
                return hashlib.file_digest(stream, "sha256").hexdigest()
        return dict(debug_elf=str(self.debug.path), packed_elf=str(self.packed.path),
                    debug_sha256=sha(self.debug.path), packed_sha256=sha(self.packed.path),
                    load_base=self.load_base, allocated_span=self.allocated_span,
                    allocated_sections=self.sections, allocated_sidecars_match=True)


class KernelMap:
    """The TRUEOS kernel is ET_EXEC with no address slide in the current boot path."""
    def __init__(self, kernel_elf):
        self.elf = ELF(kernel_elf, accepted_types=(2,))
        self.functions = []
        self.by_name = {}
        self.allocated_symbols = {}
        for symbol in self.elf.symbols():
            if not symbol["size"] or symbol["section_index"] >= len(self.elf.sections):
                continue
            section = self.elf.sections[symbol["section_index"]]
            if not section["flags"] & 2:
                continue
            row = dict(symbol, address=symbol["value"], section_name=section["name"], executable=bool(section["flags"] & 4))
            self.allocated_symbols.setdefault(row["name"], []).append(row)
            if symbol["type"] != 2 or not row["executable"]:
                continue
            self.functions.append(row)
            self.by_name.setdefault(row["name"], []).append(row)
        self.functions.sort(key=lambda row: row["address"])
        self.starts = [row["address"] for row in self.functions]

    def close(self):
        self.elf.close()

    def resolve(self, name, function=True):
        matches = (self.by_name if function else self.allocated_symbols).get(name, [])
        if len(matches) != 1:
            raise ValueError(f"Kernel symbol {name!r} resolves to {len(matches)} real function entries")
        return dict(matches[0])

    def at(self, address):
        index = bisect_right(self.starts, int(address)) - 1
        if index >= 0:
            row = self.functions[index]
            if row["address"] <= address < row["address"] + row["size"]:
                return dict(symbol=row["name"], symbol_offset=address-row["address"], executable=True)
        return None

    def evidence(self):
        with self.elf.path.open("rb") as stream:
            sha = hashlib.file_digest(stream, "sha256").hexdigest()
        return dict(kernel_elf=str(self.elf.path), sha256=sha, elf_type="ET_EXEC", address_slide=0,
                    note="Runtime boot-kernel identity must also be authenticated by the runner.")


def prepare_manifest(spec):
    module = spec["module"]
    mapped = ModuleMap(module["debug_elf"], module.get("packed_elf", module.get("loaded_elf")), module["load_base"])
    kernel = None
    try:
        result = dict(spec, version=1, module=mapped.evidence())
        if spec.get("kernel_elf"):
            kernel = KernelMap(spec["kernel_elf"])
            result["kernel_map"] = kernel.evidence()
            try:
                handoff = {"hook": kernel.resolve("trueos_depthchecker_debug_stop")}
                for key, name in [("enable", "DEPTHCHECKER_DEBUG_ENABLE"), ("address_count", "DEPTHCHECKER_DEBUG_ADDRESS_COUNT"),
                                  ("addresses", "DEPTHCHECKER_DEBUG_ADDRESSES")]:
                    handoff[key] = kernel.resolve(name, function=False)
                result["vmx_handoff"] = handoff
            except ValueError:
                if spec.get("vmx_handoff_required"):
                    raise
        markers = dict(begin="depthchecker_begin", end="depthchecker_end", complete="depthchecker_complete")
        markers.update(spec.get("markers", {}))
        result["markers"] = {key: mapped.resolve(value, function=key != "gate") for key, value in markers.items()}
        ids, indexes, scenarios = set(), set(), []
        for scenario in spec["scenarios"]:
            ident, index = scenario["id"], int(scenario["index"])
            if ident in ids or index in indexes:
                raise ValueError("Scenario IDs and marker indexes must be unique")
            ids.add(ident); indexes.add(index)
            targets, addresses = [], set()
            for target in scenario["targets"]:
                target = dict(symbol=target) if isinstance(target, str) else dict(target)
                scope = target.get("scope", "module")
                if scope not in ("module", "kernel"):
                    raise ValueError("Target scope must be module or kernel")
                if scope == "kernel" and kernel is None:
                    raise ValueError("Kernel dependency target requires kernel_elf")
                resolved = (kernel if scope == "kernel" else mapped).resolve(target["symbol"])
                if resolved["address"] in addresses or any(resolved["address"] == value["address"] for value in result["markers"].values()):
                    raise ValueError("Targets must have distinct real dependency entries, separate from probe markers")
                addresses.add(resolved["address"])
                targets.append(dict(target, scope=scope, resolved=resolved, boundary="entry-before-prologue"))
            if not targets:
                raise ValueError(f"Scenario {ident} has no dependency entry targets")
            scenarios.append(dict(scenario, index=index, targets=targets))
        result["scenarios"] = scenarios
        return result
    finally:
        mapped.close()
        if kernel:
            kernel.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--spec", type=Path, help="Prepare a capture manifest")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--debug-elf", type=Path)
    parser.add_argument("--packed-elf", type=Path)
    parser.add_argument("--load-base", default="0")
    parser.add_argument("--contains", help="List exact function names containing this substring")
    args = parser.parse_args()
    if args.spec:
        result = prepare_manifest(json.loads(args.spec.read_text()))
    else:
        if not args.debug_elf or not args.packed_elf:
            parser.error("--spec or both --debug-elf and --packed-elf are required")
        mapped = ModuleMap(args.debug_elf, args.packed_elf, args.load_base)
        try:
            result = dict(module=mapped.evidence(), functions=[row for row in mapped.function_rows if not args.contains or args.contains in row["name"]])
        finally:
            mapped.close()
    encoded = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.write_text(encoded)
    else:
        print(encoded, end="")


if __name__ == "__main__":
    main()
