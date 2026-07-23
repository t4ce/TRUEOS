from __future__ import annotations

import argparse
import json
from dataclasses import asdict
from pathlib import Path

from .backend import RifeBackend
from .pipeline import InterpolationSettings, interpolate_pair, interpolate_sequence


EXP_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_RIFE_DIR = EXP_ROOT / ".runtime" / "Practical-RIFE"


def _settings(args: argparse.Namespace) -> InterpolationSettings:
    return InterpolationSettings(
        work_scale=args.work_scale,
        timestep=args.timestep,
        alpha_threshold=args.alpha_threshold,
        quantize=args.quantize,
        background=args.background,
        inference_scale=args.inference_scale,
    )


def _add_common_options(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--work-scale", type=int, default=4, choices=(1, 2, 4, 8))
    parser.add_argument("--timestep", type=float, default=0.5)
    parser.add_argument("--alpha-threshold", type=float, default=0.5)
    parser.add_argument("--quantize", choices=("pair", "none"), default="pair")
    parser.add_argument("--background", type=int, default=127)
    parser.add_argument(
        "--inference-scale", type=float, default=1.0, choices=(0.5, 1.0, 2.0)
    )
    parser.add_argument("--device", choices=("auto", "cuda", "cpu"), default="auto")
    parser.add_argument("--rife-dir", type=Path, default=DEFAULT_RIFE_DIR)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="lilly-exp",
        description="Alpha-safe Practical-RIFE experiments for Lilly frames",
    )
    commands = parser.add_subparsers(dest="command", required=True)

    doctor = commands.add_parser("doctor", help="verify the isolated RIFE runtime")
    doctor.add_argument("--device", choices=("auto", "cuda", "cpu"), default="auto")
    doctor.add_argument("--rife-dir", type=Path, default=DEFAULT_RIFE_DIR)

    pair = commands.add_parser("pair", help="interpolate one frame pair")
    pair.add_argument("input_a", type=Path)
    pair.add_argument("input_b", type=Path)
    pair.add_argument("output", type=Path)
    pair.add_argument("--report", type=Path)
    _add_common_options(pair)

    sequence = commands.add_parser("sequence", help="interpolate a four-frame set")
    sequence.add_argument("input_dir", type=Path)
    sequence.add_argument("output_dir", type=Path)
    sequence.add_argument("--loop", action="store_true")
    _add_common_options(sequence)
    return parser


def main() -> None:
    args = build_parser().parse_args()
    backend = RifeBackend(args.rife_dir, device=args.device)

    if args.command == "doctor":
        print(json.dumps(asdict(backend.info), indent=2))
        return

    if args.command == "pair":
        report = interpolate_pair(
            backend,
            args.input_a,
            args.input_b,
            args.output,
            _settings(args),
        )
        report_json = json.dumps(asdict(report), indent=2) + "\n"
        if args.report:
            args.report.parent.mkdir(parents=True, exist_ok=True)
            args.report.write_text(report_json, encoding="utf-8")
        print(report_json, end="")
        return

    report = interpolate_sequence(
        backend,
        args.input_dir,
        args.output_dir,
        _settings(args),
        loop=args.loop,
    )
    summary = {
        "output_directory": report["output_directory"],
        "generated_frames": len(report["frames"]),
        "loop": report["loop"],
        "backend": report["backend"],
    }
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()

