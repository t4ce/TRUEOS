#!/usr/bin/env python3
"""Export one reviewed ShaderToy Image pass as a named C++ for OpenCL kernel."""

from __future__ import annotations

import argparse
from pathlib import Path

from adapter import AdapterError, adapt


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--kernel-name", required=True)
    parser.add_argument("--foveated", action="store_true", help="emit the reviewed two-pass focus ABI")
    args = parser.parse_args()

    try:
        source = args.source.read_text(encoding="utf-8")
        generated = adapt(source, kernel_name=args.kernel_name, foveated=args.foveated)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(generated, encoding="utf-8")
    except (OSError, UnicodeError, AdapterError) as error:
        parser.error(str(error))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
