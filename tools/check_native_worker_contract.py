#!/usr/bin/env python3
"""Read-only source contract check for the pinned native Rust worker ABI.

This complements the CABI pack guard, which only covers trueos_cabi_* names.
It does not substitute for compiling either side or checking packed imports.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import re

SYMBOLS = ("trueos_service_lane_submit_job", "trueos_service_lane_available_capacity")


def signature(source: str, symbol: str, definition: bool) -> str:
    source = re.sub(r"//[^\n]*", "", source)
    prefix = r'pub extern "Rust" fn ' if definition else r"pub fn "
    matches = list(re.finditer(prefix + re.escape(symbol) + r"\s*\(", source))
    if len(matches) != 1:
        raise ValueError(f"{symbol}: expected exactly one {'definition' if definition else 'declaration'}")
    start = matches[0].end() - 1
    end = source.index("{" if definition else ";", start)
    result = source[start:end].strip().replace("BlockingJobFn", "Box<dyn FnOnce() + Send + 'static>")
    # Parameter names are not ABI; these two signatures only have one named
    # parameter. Keep Rust ABI, ownership type, bounds, and output exact.
    result = re.sub(r"\bjob\s*:\s*", "", result)
    result = re.sub(r"\s+", "", result).replace(",)", ")")
    return result


def check(kernel: Path, sdk: Path) -> None:
    relative = Path("crates/trueos-v/src/worker_abi.rs")
    sources = [root.joinpath(relative).read_text() for root in (kernel, sdk)]
    implementation = (kernel / "src/r/blocking.rs").read_text()
    loader = (kernel / "src/hv/blueprint/blueprint.rs").read_text()
    for root, source in zip((kernel, sdk), sources):
        if 'unsafe extern "Rust"' not in source:
            raise ValueError(f"{root}: native worker declarations must use Rust ABI")
        if "pub mod worker_abi;" not in (root / "crates/trueos-v/src/lib.rs").read_text():
            raise ValueError(f"{root}: native worker declarations are not exposed")
    for symbol in SYMBOLS:
        contracts = [signature(source, symbol, False) for source in sources]
        contracts.append(signature(implementation, symbol, True))
        if len(set(contracts)) != 1:
            raise ValueError(f"{symbol}: kernel SDK / Blueprint SDK / implementation mismatch: {contracts}")
        arm = re.search(r'"' + symbol + r'"\s*=>\s*\{([^}]+)\}', loader)
        if not arm or f"crate::r::blocking::{symbol} as *const () as usize" not in arm[1]:
            raise ValueError(f"{symbol}: required loader export missing or points to another function")
    vmcall = (kernel / "src/hv/vmcall.rs").read_text()
    constants = dict(re.findall(r"pub const (OP_\w+): u32 = (0x[0-9a-fA-F]+)", vmcall))
    code = int(constants["OP_BP_SERVICE_LANE_CAPACITY"], 16)
    collisions = [name for name, value in constants.items() if int(value, 16) == code]
    if collisions != ["OP_BP_SERVICE_LANE_CAPACITY"]:
        raise ValueError(f"native capacity VMCALL collision: {collisions}")
    if "OP_BP_SERVICE_LANE_CAPACITY =>" not in vmcall:
        raise ValueError("native capacity VMCALL dispatch missing")
    print("native-worker-contract: declarations=match implementations=match exports=present vmcall=unique")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kernel", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--blueprints", type=Path, required=True)
    args = parser.parse_args()
    try:
        check(args.kernel, args.blueprints)
    except (OSError, ValueError, KeyError) as error:
        raise SystemExit(f"native-worker-contract: {error}") from error


if __name__ == "__main__":
    main()
