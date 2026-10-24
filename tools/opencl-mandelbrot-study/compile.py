#!/usr/bin/env python3
"""Offline compile-only study. Never opens or submits to a GPU device."""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path(__file__).with_name('mandelbrot.cl')
TARGETS = [
    ('ubuntu-host', 'Raptor Lake-S UHD 770', '0xa780', 4, 'rpl-s'),
    ('trueos-rig', 'Alder Lake-S GT1', '0x4680', 12, 'adl-s'),
    ('laptop', 'Tiger Lake-LP (i7-1185G7)', '0x9a49', 1, 'tgllp'),
]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def elf_sections(data):
    if data[:6] != b'\x7fELF\x02\x01':
        raise ValueError('Expected little-endian ELF64 Zebin')
    start = struct.unpack_from('<Q', data, 40)[0]
    stride, count, names_index = struct.unpack_from('<HHH', data, 58)
    headers = [struct.unpack_from('<IIQQQQIIQQ', data, start + i * stride) for i in range(count)]
    names_header = headers[names_index]
    names = data[names_header[4]:names_header[4] + names_header[5]]
    return {
        names[h[0]:].split(b'\0', 1)[0].decode(): data[h[4]:h[4] + h[5]]
        for h in headers if h[1] != 8
    }


def run(command, directory, environment, log):
    result = subprocess.run(command, cwd=directory, env=environment, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
    (directory / log).write_text('$ ' + shlex.join(command) + '\n\n' + result.stdout)
    if result.returncode:
        raise RuntimeError(f'{log}: exit {result.returncode}\n{result.stdout}')
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, default=SOURCE)
    parser.add_argument('--define', action='append', default=[])
    parser.add_argument('--out', type=Path, default=ROOT / 'bld/opencl-mandelbrot-study')
    parser.add_argument('--library-dir', type=Path,
                        default=ROOT / 'bld/opencl-mandelbrot-toolchain/root/usr/lib/x86_64-linux-gnu')
    args = parser.parse_args()
    source = args.source.resolve()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env['LC_ALL'] = 'C'
    if args.library_dir.is_dir():
        env['LD_LIBRARY_PATH'] = str(args.library_dir.resolve()) + (
            ':' + env['LD_LIBRARY_PATH'] if env.get('LD_LIBRARY_PATH') else '')
    ocloc = shutil.which('ocloc')
    iga = shutil.which('iga64')
    if not ocloc or not iga:
        raise RuntimeError('ocloc and iga64 must be on PATH')
    shutil.copy2(source, out / source.name)
    records = []
    for key, model, device, revision, acronym in TARGETS:
        for variant in ['default', 'simd8']:
            directory = out / key / variant
            directory.mkdir(parents=True, exist_ok=True)
            options = '-cl-std=CL1.2' + ''.join(' -D' + define for define in args.define) + (' -DFORCE_SIMD8=1' if variant == 'simd8' else '')
            command = [ocloc, 'compile', '-file', str(source), '-device', device,
                       '-revision_id', str(revision), '-64', '-options', options,
                       '-output', 'mandelbrot', '-output_no_suffix', '-out_dir', str(directory)]
            build_log = run(command, directory, env, 'compile.log')
            validate_log = run([ocloc, 'validate', '-file', 'mandelbrot.bin'], directory, env, 'validate.log')
            if 'Binary is VALID' not in validate_log:
                raise RuntimeError('ocloc did not report a valid binary')
            sections = elf_sections((directory / 'mandelbrot.bin').read_bytes())
            native = sections['.text.mandelbrot']
            metadata = sections['.ze_info'].decode()
            (directory / 'kernel.bin').write_bytes(native)
            (directory / 'ze_info.yaml').write_text(metadata)
            run([iga, '-p=12p1', '-d', 'kernel.bin', '-Xprint-pc', '-o', 'section.asm'],
                directory, env, 'disassemble.log')
            # Device callees can follow EOT; include all executable FUNC symbols.
            functions = [entry for entry in struct.iter_unpack('<IBBHQQ', sections['.symtab'])
                         if entry[1] & 15 == 2 and entry[5] != 0]
            extent = max(entry[4] + entry[5] for entry in functions)
            if not 0 < extent <= len(native):
                raise RuntimeError('Invalid executable symbol extent')
            full_asm = (directory / 'section.asm').read_text()
            terminals = [line for line in full_asm.splitlines()
                         if '{EOT' in line or re.search(r'\bret\s+\(', line)]
            end = max(int(re.search(r'\[([0-9A-Fa-f]+)\]', line)[1], 16)
                      + (8 if 'Compacted' in line else 16) for line in terminals)
            if end > extent:
                raise RuntimeError('Terminal instruction exceeds FUNC symbol extent')
            executable = native[:end]
            (directory / 'executable.bin').write_bytes(executable)
            run([iga, '-p=12p1', '-d', 'executable.bin', '-Xprint-pc', '-o', 'kernel.asm'],
                directory, env, 'disassemble-executable.log')
            run([iga, '-p=12p1', '-a', 'kernel.asm', '-o', 'kernel.roundtrip.bin'],
                directory, env, 'assemble.log')
            if (directory / 'kernel.roundtrip.bin').read_bytes() != executable:
                raise RuntimeError('IGA native roundtrip differs')
            asm = (directory / 'kernel.asm').read_text()
            instructions = []
            for line in asm.splitlines():
                match = re.match(r'/\* \[([0-9A-Fa-f]+)\]\s*\*/\s*(?:\([^)]*\)\s*)?([a-z][\w.]*)\b', line)
                if match:
                    instructions.append((int(match[1], 16), match[2], line))
            histogram = Counter(x[1] for x in instructions)
            def meta(name):
                match = re.search(r'^\s+' + name + r':\s+(\d+)', metadata, re.M)
                return int(match[1]) if match else None
            if not instructions or not histogram['while'] or not any(op.startswith('send') for op in histogram):
                raise RuntimeError('Expected decoded loop and SEND instructions')
            record = dict(target=key, model=model, device=device, revision=revision,
                          variant=variant, compiler_mapping=build_log.splitlines()[0],
                          simd=meta('simd_size'), grf_mode=meta('grf_count'),
                          native_section_bytes=len(native), executable_bytes=len(executable), function_symbols=len(functions), binary_bytes=len((directory / 'mandelbrot.bin').read_bytes()),
                          instructions=len(instructions), compacted_instructions=asm.count('Compacted'),
                          last_instruction_pc=instructions[-1][0], op_counts=dict(histogram),
                          slm_size_reported=meta('slm_size'),
                          per_thread_memory_buffers_reported='per_thread_memory_buffers:' in metadata,
                          has_stack_calls='has_stack_calls: true' in metadata,
                          private_size_reported=meta('private_size'), spill_size_reported=meta('spill_size'),
                          native_sha256=sha(native), executable_sha256=sha(executable), source_sha256=sha(source.read_bytes()),
                          relative_dir=f'{key}/{variant}',
                          validation='valid with decoder warnings' if 'potential problems' in validate_log else 'valid',
                          iga_roundtrip='byte-identical', command=command)
            if variant == 'simd8' and record['simd'] != 8:
                raise RuntimeError('SIMD8 attribute was not honored')
            records.append(record)
            print(f'{key:12} {variant:7} SIMD{record["simd"]} {len(native):4} bytes '
                  f'{len(executable):4} executable bytes, {record["instructions"]:3} instructions, {record["compacted_instructions"]} compacted; VALID; IGA roundtrip OK')
    run([ocloc, 'query', 'OCL_DRIVER_VERSION'], out, env, 'ocloc-version.log')
    provenance = {'ocloc_driver_version': (out / 'OCL_DRIVER_VERSION').read_text().strip('\x00\r\n '),
                  'ocloc_path': os.path.realpath(ocloc), 'iga_path': os.path.realpath(iga),
                  'igc_libraries': {p.name: sha(p.read_bytes()) for p in args.library_dir.glob('*.so.*')
                                    if not p.is_symlink() and p.name.startswith(('libigc.', 'libigdfcl.', 'libopencl-clang.'))},
                  'target_basis': {'host': 'lspci: 8086:a780 revision 04',
                                   'rig': 'docs/experiments/odin-runtime-mandelbrot.md: ADL-S 4680:0c',
                                   'laptop': 'docs/experiments/tgl-9a49-adls-alias.md + current TGL admission: 9a49:01'},
                  'execution': 'Compilation, binary validation and disassembly only; no GPU execution.'}
    (out / 'results.json').write_text(json.dumps({'provenance': provenance, 'builds': records}, indent=2) + '\n')
    rows = ['| Machine | PCI ID / rev | Variant | SIMD | EU executable bytes | Native section bytes | Instructions | 8-byte compacted |',
            '|---|---|---|---:|---:|---:|---:|---:|']
    for r in records:
        rows.append(f'| {r["target"]} | {r["device"]}:{r["revision"]:02x} | {r["variant"]} | {r["simd"]} | {r["executable_bytes"]} | {r["native_section_bytes"]} | {r["instructions"]} | {r["compacted_instructions"]} |')
    equal = {variant: len({r['executable_sha256'] for r in records if r['variant'] == variant}) == 1
             for variant in ['default', 'simd8']}
    notes = '\n'.join(f'- {v}: EU executable bytes identical across all three targets: {ok}.' for v, ok in equal.items())
    links = '\n'.join(f'- {r["target"]} / {r["variant"]}: [ISA]({r["relative_dir"]}/kernel.asm), [metadata]({r["relative_dir"]}/ze_info.yaml), [compile log]({r["relative_dir"]}/compile.log), [validation]({r["relative_dir"]}/validate.log)' for r in records)
    (out / 'summary.md').write_text('# Offline OpenCL Mandelbrot compile study\n\n' + provenance['execution'] + '\n\n' + '\n'.join(rows) + '\n\n' + notes + '\n\n' + links + '\n\nNative section size includes compiler trailer and padding. EU image extends through the last EOT/ret within FUNC symbol bounds, including callees after EOT; IGA roundtrip covers that prefix. Inter-function padding can remain inside that span. Instruction count is static and excludes the trailer; it is not runtime iterations or cycles; decoded inter-function padding can also appear in this count. GRF metadata reports the allocation mode, not measured live-register pressure. No local-memory / scratch allocation is claimed unless present in metadata.\n')
    print('Results:', out / 'summary.md')


if __name__ == '__main__':
    main()
