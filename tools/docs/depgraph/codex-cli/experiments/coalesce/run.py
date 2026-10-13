#!/usr/bin/env python3
"""Relink the existing Codex Blueprint closure; never rebuild or publish it."""
import argparse
import json
import mmap
from pathlib import Path
import struct
import subprocess
import tempfile
import time

HERE = Path(__file__).resolve().parent
WORK = Path('/home/t4ce/Repos/codex/codex-rs/cli/target/trueos-blueprint/work/codex')
TOOLS = Path('/home/t4ce/Repos/TRUEOS-Rust-Toolchain-nightly-2026-07-10/lib/rustlib/x86_64-unknown-linux-gnu/bin')


def elf_summary(path):
    with path.open('rb') as stream, mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as data:
        assert data[:6] == b'\x7fELF\x02\x01', 'expected little-endian ELF64'
        offset = struct.unpack_from('<Q', data, 40)[0]
        width, count, names = struct.unpack_from('<HHH', data, 58)
        if count == 0:
            count = struct.unpack_from('<Q', data, offset + 32)[0]
        if names == 65535:
            names = struct.unpack_from('<I', data, offset + 40)[0]
        sections = [struct.unpack_from('<IIQQQQIIQQ', data, offset + i * width) for i in range(count)]
        strings = sections[names]
        string_start = strings[4]
        result = dict(path=str(path), file_bytes=len(data), section_count=count,
                      section_header_bytes=width * count, allocated_file_bytes=0,
                      bss_bytes=0, relocation_bytes=0, tables={}, entry=None,
                      loader_image_bytes_before_got_thunks=0, relocation_types={},
                      relocation_writes_in_bounds=True)
        for section in sections:
            name_start = string_start + section[0]
            name = data[name_start:data.find(b'\0', name_start)].decode('utf-8', errors='replace')
            kind, flags, size = section[1], section[2], section[5]
            if flags & 2:
                alignment = max(section[8], 1)
                assert alignment & (alignment - 1) == 0
                mapped = result['loader_image_bytes_before_got_thunks']
                result['loader_image_bytes_before_got_thunks'] = (mapped + alignment - 1) // alignment * alignment + size
                if kind == 8:
                    result['bss_bytes'] += size
                else:
                    result['allocated_file_bytes'] += size
            if kind in (4, 9):
                result['relocation_bytes'] += size
                target = sections[section[7]]
                if target[2] & 2:
                    assert kind == 4 and section[9] == 24, 'loader needs ELF64 RELA'
                    for pos in range(section[4], section[4] + size, 24):
                        place, info, addend = struct.unpack_from('<QQq', data, pos)
                        relocation_type = info & 0xffffffff
                        key = str(relocation_type)
                        result['relocation_types'][key] = result['relocation_types'].get(key, 0) + 1
                        write_size = {0:0,1:8,2:4,4:4,9:4,11:4,41:4,42:4}.get(relocation_type)
                        assert write_size is not None, f'unsupported loader relocation {relocation_type}'
                        if place + write_size > target[5]:
                            result['relocation_writes_in_bounds'] = False
            if name in ('.strtab', '.shstrtab', '.symtab', '.symtab_shndx'):
                result['tables'][name] = size
        undefined = set()
        for index, section in enumerate(sections):
            if section[1] != 2:
                continue
            symbol_strings = sections[section[6]][4]
            extended = next((s for s in sections if s[1] == 18 and s[6] == index), None)
            for i, pos in enumerate(range(section[4], section[4] + section[5], section[9])):
                name_index, info, other, shndx, value, size = struct.unpack_from('<IBBHQQ', data, pos)
                if not name_index:
                    continue
                if shndx == 65535 and extended:
                    shndx = struct.unpack_from('<I', data, extended[4] + i * 4)[0]
                if shndx != 0 and not (info & 15 == 2 and name_index):
                    continue
                start = symbol_strings + name_index
                end = data.find(b'\0', start)
                if shndx == 0:
                    undefined.add(data[start:end].decode('utf-8', errors='replace'))
                elif data[start:end] == b'_start':
                    result['entry'] = dict(symbol='_start', section=shndx, value=value,
                                           executable_section=bool(sections[shndx][2] & 4))
        result['undefined_symbols'] = sorted(undefined)
        result['undefined_symbol_count'] = len(undefined)
        return result


def compress(path):
    archive = path.with_suffix('.tar.lz4')
    tar = path.with_suffix('.tar.tmp')
    size = path.stat().st_size
    header = bytearray(512)
    header[:10] = b'module.elf'
    for start, end, value in ((100,108,0o644),(108,116,0),(116,124,0),(124,136,size),(136,148,0)):
        field = f'{value:0{end-start-1}o}'.encode()
        assert len(field) == end-start-1
        header[start:end-1] = field
    header[148:156] = b' ' * 8
    header[156] = ord('0')
    header[257:263] = b'ustar\0'
    header[263:265] = b'00'
    header[148:154] = f'{sum(header):06o}'.encode()
    header[154] = 0
    with tar.open('wb') as target, path.open('rb') as source:
        target.write(header)
        while chunk := source.read(1024 * 1024):
            target.write(chunk)
        target.write(bytes((-size) % 512 + 1024))
    subprocess.run(['lz4', '-12', '-BI', '--content-size', '-f', str(tar), str(archive)], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    tar.unlink()
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    output = args.output or Path(tempfile.mkdtemp(prefix='codex-bp-coalesce-'))
    output.mkdir(parents=True, exist_ok=True)
    selected = json.loads((HERE / 'selected-rlibs.json').read_text())
    rlibs = [row['path'] for row in selected['rlibs']]
    baseline = elf_summary(WORK / 'module.stripped.o')
    baseline['compressed_bytes'] = (WORK / 'module.stripped.tar.lz4').stat().st_size
    results = dict(artifact_directory=str(output), inputs=selected,
                   baseline=baseline, variants={}, command_logs={})
    print('baseline', json.dumps({k:v for k,v in baseline.items() if k!='undefined_symbols'}), flush=True)
    for name, extra in [('coalesce', []), ('coalesce-discard-locals', ['--discard-all'])]:
        linked = output / f'{name}.o'
        stripped = output / f'{name}.stripped.o'
        command = [str(TOOLS / 'gcc-ld/ld.lld'), '-r', '--gc-sections', '-T', str(HERE / 'rel-coalesce.ld'),
                   *extra, '-o', str(linked), '--undefined', '_start', str(WORK / 'app.rooted.o'),
                   '--start-group', *rlibs, '--end-group']
        results['command_logs'][name] = command
        start = time.monotonic()
        subprocess.run(command, check=True)
        subprocess.run([str(TOOLS / 'llvm-objcopy'), '--strip-debug', str(linked), str(stripped)], check=True)
        result = elf_summary(stripped)
        result['link_seconds'] = round(time.monotonic() - start, 2)
        archive = compress(stripped)
        result['compressed_bytes'] = archive.stat().st_size
        result['same_undefined_symbols'] = result['undefined_symbols'] == baseline['undefined_symbols']
        result['same_relocation_types'] = result['relocation_types'] == baseline['relocation_types']
        # Copy public capability flags, regenerate the entry hint for the new ELF.
        original_header = Path('/home/t4ce/Repos/TRUEOS-Blueprints/dist/codex.bp').open('rb').read(24)
        magic, version, flags, _, _, _ = struct.unpack('<4sHHQII', original_header)
        entry = result['entry']
        entry_hint = entry['section'] << 32 | entry['value']
        blueprint = output / f'{name}.bp'
        with blueprint.open('wb') as target, archive.open('rb') as source:
            target.write(struct.pack('<4sHHQII', magic, version, flags, entry_hint,
                                     result['compressed_bytes'], result['file_bytes']))
            while chunk := source.read(1024 * 1024):
                target.write(chunk)
        result['experimental_blueprint'] = str(blueprint)
        results['variants'][name] = result
        (HERE / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
        print(name, json.dumps({k:v for k,v in result.items() if k!='undefined_symbols'}), flush=True)
    print('results', HERE / 'results.json', flush=True)


if __name__ == '__main__':
    main()
