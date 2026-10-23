"""Build the real feature-selected MicroFont bundle for host Rust harnesses."""
from pathlib import Path
import os
import subprocess

ROOT = Path(__file__).resolve().parents[2]
CRATE = ROOT / 'vendor/microfont'


def build_microfont(output, features=('hex',), source_dir=None):
    """Compile build.rs and lib.rs with the same selected features as Cargo.

    Standalone rustc keeps host tests independent of the kernel target and its
    build-std configuration. The crate's real packer still selects every byte.
    """
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    out_dir = output.parent / (output.stem + '-bundle')
    out_dir.mkdir(parents=True, exist_ok=True)
    build_script = out_dir / 'build-script'
    subprocess.run(['rustc', '--edition=2024', str(CRATE / 'build.rs'), '-o',
                    str(build_script)], check=True)
    env = dict(os.environ)
    for key in list(env):
        if key.startswith('CARGO_FEATURE_'):
            del env[key]
    env.update(CARGO_MANIFEST_DIR=str(CRATE), OUT_DIR=str(out_dir),
               CARGO_CFG_TARGET_OS='linux', CARGO_PKG_VERSION='3.9.9')
    if source_dir is not None:
        env['MICROFONT_SOURCE_DIR'] = str(Path(source_dir).resolve())
    else:
        env.pop('MICROFONT_SOURCE_DIR', None)
    for feature in features:
        env['CARGO_FEATURE_' + feature.upper().replace('-', '_')] = '1'
    subprocess.run([str(build_script)], env=env, check=True, capture_output=True,
                   text=True)
    command = ['rustc', '--edition=2024', '--crate-name', 'microfont', '--crate-type',
               'rlib', '-A', 'dead_code']
    for feature in features:
        command.extend(['--cfg', f'feature="{feature}"'])
    command.extend([str(CRATE / 'src/lib.rs'), '-o', str(output)])
    subprocess.run(command, env=env, check=True)
    return output
