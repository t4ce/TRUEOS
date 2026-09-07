"""Compare the driver's Xe-LP HiZ sizing formulas with pinned Mesa ISL. No GPU IO."""
from pathlib import Path
import shlex
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
build = root / '.codex_tmp/trueos-adj-instrumented-rpls/mesa-build'
ninja = (build / 'build.ninja').read_text()
line = ninja.split('build src/intel/isl/libisl.a.p/isl.c.o:')[1].split(' ARGS = ', 1)[1].splitlines()[0]
args = shlex.split(line)
libs = (list((build / 'src/intel/isl').glob('*.a'))
        + [build / 'src/intel/dev/libintel_dev.a']
        + list((build / 'src/util').glob('*.a'))
        + [build / 'src/c11/impl/libmesa_util_c11.a'])
with tempfile.TemporaryDirectory(prefix='trueos-hiz-isl-') as tmp:
    binary = str(Path(tmp) / 'reference')
    subprocess.run(['cc', *args, str(root / 'tools/hiz_isl_reference.c'), '-o', binary,
                    '-Wl,--gc-sections', '-Wl,--start-group', *map(str, libs),
                    '-Wl,--end-group', '-lm', '-ldl', '-l:libzstd.so.1', '-lz', '-l:libdrm.so.2'],
                   cwd=build, check=True)
    subprocess.run([binary], check=True)
