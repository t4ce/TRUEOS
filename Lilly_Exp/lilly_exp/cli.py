from __future__ import annotations

import argparse
import json
from dataclasses import asdict
from pathlib import Path

from .backend import RifeBackend
from .pipeline import (
    InterpolationSettings,
    evaluate_sequence,
    interpolate_library,
    interpolate_pair,
    interpolate_sequence,
    promote_library,
)


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
        blend_mode=args.blend_mode,
        ensemble=args.ensemble,
        face_only=args.face_only,
    )


def _add_common_options(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--work-scale", type=int, default=8, choices=(1, 2, 4, 8, 16)
    )
    parser.add_argument("--timestep", type=float, default=0.5)
    parser.add_argument("--alpha-threshold", type=float, default=0.5)
    parser.add_argument("--quantize", choices=("pair", "none"), default="pair")
    parser.add_argument("--background", type=int, default=127)
    parser.add_argument(
        "--inference-scale", type=float, default=1.0, choices=(0.5, 1.0, 2.0)
    )
    parser.add_argument(
        "--blend-mode",
        choices=("temporal-alpha", "rife"),
        default="rife",
    )
    parser.add_argument(
        "--ensemble",
        choices=("none", "median", "medoid"),
        default="medoid",
        help=(
            "refinement strategy (default: medoid, a 12-pass "
            "background/direction/horizontal-flip ensemble)"
        ),
    )
    parser.add_argument(
        "--face-only",
        action="store_true",
        help=(
            "generate only the inner facial state; copy alpha and every pixel "
            "outside the face from the preceding canonical frame"
        ),
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

    library = commands.add_parser(
        "library",
        help="generate mirrored seven-frame sets for an entire Lilly tree",
    )
    library.add_argument("input_root", type=Path)
    library.add_argument("output_root", type=Path)
    _add_common_options(library)

    promote = commands.add_parser(
        "promote",
        help="preflight or atomically promote a staged library refresh",
    )
    promote.add_argument("staging_root", type=Path)
    promote.add_argument("target_root", type=Path)
    promote.add_argument(
        "--apply",
        action="store_true",
        help="perform replacement after preflight; omitted means check only",
    )

    evaluate = commands.add_parser(
        "evaluate", help="predict held-out keyframes and calculate accuracy metrics"
    )
    evaluate.add_argument("input_dir", type=Path)
    evaluate.add_argument("output_dir", type=Path)
    evaluate.add_argument("--loop", action="store_true")
    _add_common_options(evaluate)
    return parser


def main() -> None:
    args = build_parser().parse_args()
    if args.command == "promote":
        report = promote_library(
            args.staging_root,
            args.target_root,
            apply=args.apply,
        )
        print(
            json.dumps(
                {
                    "staging_root": report["staging_root"],
                    "target_root": report["target_root"],
                    "checked_frame_sets": report["checked_frame_sets"],
                    "promoted_frame_sets": report["promoted_frame_sets"],
                    "applied": report["applied"],
                },
                indent=2,
            )
        )
        return

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

    if args.command == "evaluate":
        report = evaluate_sequence(
            backend,
            args.input_dir,
            args.output_dir,
            _settings(args),
            loop=args.loop,
        )
        print(
            json.dumps(
                {
                    "output_directory": report["output_directory"],
                    "evaluated_keyframes": len(report["evaluations"]),
                    "inference_seconds": sum(
                        item["prediction"]["inference_seconds"]
                        for item in report["evaluations"]
                    ),
                    "passes_quality_gate": report["passes_quality_gate"],
                    "mean_metrics": report["mean_metrics"],
                    "backend": report["backend"],
                },
                indent=2,
            )
        )
        return

    if args.command == "library":
        report = interpolate_library(
            backend,
            args.input_root,
            args.output_root,
            _settings(args),
            progress=lambda index, total, relative: print(
                f"[{index:02d}/{total:02d}] {relative}",
                flush=True,
            ),
        )
        print(
            json.dumps(
                {
                    "output_root": report["output_root"],
                    "frame_sets": report["frame_set_count"],
                    "generated_frames": report["generated_frame_count"],
                    "inference_seconds": report["inference_seconds"],
                    "backend": report["backend"],
                },
                indent=2,
            )
        )
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
        "inference_seconds": sum(
            item["inference_seconds"] for item in report["frames"]
        ),
        "loop": report["loop"],
        "backend": report["backend"],
    }
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
