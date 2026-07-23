from __future__ import annotations

import argparse
import json
from dataclasses import asdict
from pathlib import Path

from .backend import FilmBackend
from .pipeline import (
    InterpolationSettings,
    evaluate_sequence,
    interpolate_pair,
    interpolate_sequence,
    run_corpus,
)


EXP_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MODEL_DIR = (
    EXP_ROOT
    / ".runtime"
    / "models"
    / "film_net"
    / "Style"
    / "saved_model"
)


def _add_backend_options(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--device", choices=("cpu", "auto"), default="cpu")
    parser.add_argument("--model-dir", type=Path, default=DEFAULT_MODEL_DIR)


def _add_common_options(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--work-scale", type=int, choices=(1, 2, 4), default=1)
    parser.add_argument("--timestep", type=float, default=0.5)
    parser.add_argument("--alpha-threshold", type=float, default=0.4)
    parser.add_argument("--quantize", choices=("pair", "none"), default="pair")
    parser.add_argument(
        "--color-mode",
        choices=("gray", "matte", "premultiplied"),
        default="matte",
    )
    _add_backend_options(parser)


def _settings(args: argparse.Namespace) -> InterpolationSettings:
    return InterpolationSettings(
        work_scale=args.work_scale,
        timestep=args.timestep,
        alpha_threshold=args.alpha_threshold,
        quantize=args.quantize,
        color_mode=args.color_mode,
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="lilly-film",
        description="Single-step FILM experiments for Lilly's large arm motion",
    )
    commands = parser.add_subparsers(dest="command", required=True)
    doctor = commands.add_parser("doctor")
    _add_backend_options(doctor)

    pair = commands.add_parser("pair")
    pair.add_argument("input_a", type=Path)
    pair.add_argument("input_b", type=Path)
    pair.add_argument("output", type=Path)
    pair.add_argument("--report", type=Path)
    _add_common_options(pair)

    sequence = commands.add_parser("sequence")
    sequence.add_argument("input_dir", type=Path)
    sequence.add_argument("output_dir", type=Path)
    sequence.add_argument("--loop", action="store_true")
    _add_common_options(sequence)

    evaluate = commands.add_parser("evaluate")
    evaluate.add_argument("input_dir", type=Path)
    evaluate.add_argument("output_dir", type=Path)
    evaluate.add_argument("--loop", action="store_true")
    _add_common_options(evaluate)

    corpus = commands.add_parser("corpus")
    corpus.add_argument("input_root", type=Path)
    corpus.add_argument("output_root", type=Path)
    corpus.add_argument("--loop", action="store_true")
    _add_common_options(corpus)
    return parser


def main() -> None:
    args = build_parser().parse_args()
    backend = FilmBackend(args.model_dir, device=args.device)
    if args.command == "doctor":
        print(json.dumps(backend.doctor_report(), indent=2))
        return
    settings = _settings(args)
    if args.command == "pair":
        report = interpolate_pair(
            backend, args.input_a, args.input_b, args.output, settings
        )
        payload = json.dumps(asdict(report), indent=2) + "\n"
        if args.report:
            args.report.parent.mkdir(parents=True, exist_ok=True)
            args.report.write_text(payload, encoding="utf-8")
        print(payload, end="")
        return
    if args.command == "sequence":
        report = interpolate_sequence(
            backend, args.input_dir, args.output_dir, settings, args.loop
        )
        print(
            json.dumps(
                {
                    "output_directory": report["output_directory"],
                    "generated_frames": len(report["frames"]),
                    "inference_seconds": sum(
                        item["inference_seconds"] for item in report["frames"]
                    ),
                    "loop": report["loop"],
                    "backend": report["backend"],
                },
                indent=2,
            )
        )
        return
    if args.command == "evaluate":
        report = evaluate_sequence(
            backend, args.input_dir, args.output_dir, settings, args.loop
        )
        print(
            json.dumps(
                {
                    "output_directory": report["output_directory"],
                    "evaluated_keyframes": len(report["evaluations"]),
                    "passes_quality_gate": report["passes_quality_gate"],
                    "mean_metrics": report["mean_metrics"],
                    "backend": report["backend"],
                },
                indent=2,
            )
        )
        return
    report = run_corpus(
        backend, args.input_root, args.output_root, settings, args.loop
    )
    print(
        json.dumps(
            {
                "output_root": report["output_root"],
                "sequences": len(report["sequences"]),
                "elapsed_seconds": report["elapsed_seconds"],
                "quality_gate_passes": {
                    item["relative_directory"]: item["evaluation_report"][
                        "passes_quality_gate"
                    ]
                    for item in report["sequences"]
                },
                "backend": report["backend"],
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
