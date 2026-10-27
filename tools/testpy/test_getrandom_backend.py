#!/usr/bin/env python3
"""Exercise the real kernel getrandom callback and ring adapter on the host.

The kernel cannot run ordinary host tests. Extract its callback unchanged and
replace only the entropy producer so both successful and failed fills can be
checked without hardware. Cargo's temporary project leaves repository lockfiles
and build settings untouched.
"""

import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def run_contracts() -> None:
    source = (ROOT / "src/tyche.rs").read_text()
    callbacks = re.findall(
        r'^#\[unsafe\(no_mangle\)\]\nunsafe extern "Rust" fn '
        r'__getrandom_v03_custom\(.*?^}',
        source,
        re.MULTILINE | re.DOTALL,
    )
    if len(callbacks) != 1:
        raise ValueError(f"expected one kernel getrandom callback, got {len(callbacks)}")

    harness = r'''
use core::sync::atomic::{AtomicBool, Ordering};
static ENTROPY_AVAILABLE: AtomicBool = AtomicBool::new(true);

fn fill_bytes(dest: &mut [u8]) -> bool {
    if !ENTROPY_AVAILABLE.load(Ordering::Relaxed) {
        return false;
    }
    for (index, byte) in dest.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(37).wrapping_add(13);
    }
    true
}
'''
    harness += callbacks[0]
    harness += r'''

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::MaybeUninit;
    use ring::rand::{SecureRandom, SystemRandom};

    #[test]
    fn kernel_backend_and_ring_preserve_fallible_fill_contract() {
        let mut bytes = [MaybeUninit::<u8>::uninit(); 1032];
        let initialized = getrandom::fill_uninit(&mut bytes[1..]).unwrap();
        for (index, byte) in initialized.iter().enumerate() {
            assert_eq!(*byte, (index as u8).wrapping_mul(37).wrapping_add(13));
        }

        let mut bytes = [0; 65];
        SystemRandom::new().fill(&mut bytes).unwrap();
        for (index, byte) in bytes.iter().enumerate() {
            assert_eq!(*byte, (index as u8).wrapping_mul(37).wrapping_add(13));
        }

        ENTROPY_AVAILABLE.store(false, Ordering::Relaxed);
        getrandom::fill(&mut []).unwrap();
        SystemRandom::new().fill(&mut []).unwrap();
        let error = getrandom::fill(&mut bytes).unwrap_err();
        assert_eq!(error, getrandom::Error::new_custom(1));
        assert_eq!(error.raw_os_error(), None);
        assert!(SystemRandom::new().fill(&mut bytes).is_err());
    }
}
'''

    getrandom = json.dumps(str(ROOT / "vendor/getrandom-0.4.3"))
    ring = json.dumps(str(ROOT / "vendor/ring-0.17.14"))
    with tempfile.TemporaryDirectory(prefix="trueos-getrandom-kernel-") as temporary:
        directory = Path(temporary)
        (directory / "src").mkdir()
        (directory / "src/lib.rs").write_text(harness)
        manifest = directory / "Cargo.toml"
        manifest.write_text(
            '[package]\nname="trueos-getrandom-kernel-contract"\n'
            'version="0.0.0"\nedition="2024"\n'
            '[dependencies]\n'
            f'getrandom={{path={getrandom},default-features=false}}\n'
            f'ring={{path={ring},default-features=false}}\n'
            '[patch.crates-io]\n'
            f'getrandom={{path={getrandom}}}\n'
        )
        environment = os.environ.copy()
        environment["RUSTFLAGS"] = '--cfg getrandom_backend="custom"'
        subprocess.run(
            ["cargo", "test", "--offline", "--manifest-path", str(manifest),
             "--target", "x86_64-unknown-linux-gnu", "--quiet"],
            cwd=directory,
            env=environment,
            check=True,
        )


if __name__ == "__main__":
    run_contracts()
