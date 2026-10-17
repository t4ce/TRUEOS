#!/usr/bin/env python3
"""Read-only CABI inventory and packaged Blueprint import census (requires lz4/nm).

Source scanning is deliberately conservative: cfg and macros are not evaluated.
No absence in this report proves code safe to delete.
"""
import argparse
import collections
import hashlib
import io
import json
from pathlib import Path
import re
import struct
import subprocess
import tarfile
import tempfile

NAME = r'trueos_cabi_[A-Za-z0-9_]+'
DECL = re.compile(r'pub fn (' + NAME + r')\s*\(')
DEF = re.compile(r'\bfn (' + NAME + r')\s*\(')
ARM = re.compile(r'((?:"[^"\n]+"\s*\|\s*)*"[^"\n]+")\s*=>')


def declarations(path):
    source = path.read_text()
    result = {}
    for match in DECL.finditer(source):
        end = source.find(';', match.end())
        if end < 0:
            raise ValueError(f'unterminated declaration in {path}')
        result[match[1]] = re.sub(r'\s+', '', source[match.start():end])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--kernel', type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument('--blueprints', type=Path)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    kernel = args.kernel.resolve()
    bp = (args.blueprints or kernel.parent / 'TRUEOS-Blueprints').resolve()
    kernel_decl = declarations(kernel / 'crates/trueos-v/src/bp_abi.rs')
    sdk_decl = declarations(bp / 'crates/trueos-v/src/bp_abi.rs')
    definitions = collections.defaultdict(list)
    for path in sorted((kernel / 'src').rglob('*.rs')):
        text = path.read_text()
        for match in DEF.finditer(text):
            definitions[match[1]].append(f'{path.relative_to(kernel)}:{text.count(chr(10), 0, match.start()) + 1}')
    # Literal resolver candidates; includes gated arms and rejected Unix names.
    # These are coverage hints, never a claim of successful runtime resolution.
    literal_candidates = set()
    for rel in ['src/hv/blueprint/blueprint.rs', 'src/unix_compat.rs']:
        for match in ARM.finditer((kernel / rel).read_text()):
            literal_candidates.update(re.findall(r'"([^"\n]+)"', match[1]))
    native = {n for n in kernel_decl if n in definitions and not n.startswith('trueos_cabi_x86_')}
    users = collections.defaultdict(list)
    artifacts = []
    errors = []
    with tempfile.TemporaryDirectory(prefix='trueos-api-audit-') as tmp:
        elf_path = Path(tmp) / 'module.elf'
        for path in sorted((bp / 'dist').glob('*.bp')):
            try:
                blob = path.read_bytes()
                if len(blob) < 24 or blob[:4] != b'TRBP':
                    raise ValueError('invalid TRBP header')
                version, flags, entry, payload_len, raw_len = struct.unpack_from('<HHQII', blob, 4)
                if version != 1 or payload_len != len(blob) - 24:
                    raise ValueError('unsupported version or payload length mismatch')
                encoding = flags & 255
                if encoding == 3:
                    tar = subprocess.run(['lz4', '-dc'], input=blob[24:], capture_output=True, check=True).stdout
                    with tarfile.open(fileobj=io.BytesIO(tar)) as archive:
                        member = archive.extractfile('module.elf')
                        if member is None:
                            raise ValueError('missing module.elf')
                        elf = member.read()
                elif encoding == 1:
                    elf = blob[24:]
                else:
                    raise ValueError(f'unsupported encoding {encoding}')
                if len(elf) != raw_len or elf[:4] != b'\x7fELF':
                    raise ValueError('invalid ELF or raw length mismatch')
                elf_path.write_bytes(elf)
                output = subprocess.run(['nm', '--undefined-only', '--format=posix', str(elf_path)], capture_output=True, text=True, check=True).stdout
                imports = sorted({line.split()[0] for line in output.splitlines() if line.split()})
                for name in imports:
                    users[name].append(path.stem)
                artifacts.append({'app': path.stem, 'sha256': hashlib.sha256(blob).hexdigest(), 'mtime_ns': path.stat().st_mtime_ns,
                                  'imports': imports, 'outside_literal_resolver_inventory': sorted(set(imports) - native - literal_candidates)})
            except (ValueError, OSError, subprocess.SubprocessError, tarfile.TarError, KeyError) as error:
                errors.append({'path': str(path), 'error': str(error)})
    names = set(kernel_decl) | set(sdk_decl) | set(definitions)
    rows = []
    for name in sorted(names):
        rows.append({'name': name, 'kernel_declared': name in kernel_decl, 'sdk_declared': name in sdk_decl,
                     'definitions': definitions[name], 'generated_resolver_candidate': name in native,
                     'artifact_users': users[name], 'signature_drift': name in kernel_decl and name in sdk_decl and kernel_decl[name] != sdk_decl[name]})
    report = {'scope': 'Static source inventory plus undefined ELF symbols in dist/*.bp. No runtime health or deletion safety claim. cfg/macros, dynamic lookup, inline ABI types and external consumers need separate review.',
              'kernel': str(kernel), 'blueprints': str(bp), 'summary': {
                  'artifacts_scanned': len(artifacts), 'artifact_errors': len(errors), 'unique_imports': len(users),
                  'kernel_cabi_declarations': len(kernel_decl), 'sdk_cabi_declarations': len(sdk_decl),
                  'generated_resolver_candidates': len(native), 'cabi_with_artifact_users': sum(bool(users[n]) for n in native),
                  'cabi_without_artifact_users': sum(not users[n] for n in native)},
              'cabi': rows, 'artifacts': artifacts, 'errors': errors}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report['summary'], indent=2))
    if errors:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
