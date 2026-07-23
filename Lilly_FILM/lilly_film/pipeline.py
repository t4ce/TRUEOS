from __future__ import annotations

import hashlib
import json
import shutil
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Dict, List, Literal, Sequence, Tuple

import numpy as np
from PIL import Image
from scipy.ndimage import binary_erosion, distance_transform_edt

from .backend import FilmBackend


QuantizeMode = Literal["pair", "none"]
ColorMode = Literal["gray", "matte", "premultiplied"]
QUALITY_GATE = {
    "minimum_alpha_iou": 0.85,
    "minimum_edge_f1_with_1px_tolerance": 0.70,
    "maximum_rgb_mae_on_shared_opaque": 0.05,
    "minimum_alpha_area_ratio": 0.85,
    "maximum_alpha_area_ratio": 1.15,
}


@dataclass(frozen=True)
class InterpolationSettings:
    work_scale: int = 1
    timestep: float = 0.5
    alpha_threshold: float = 0.4
    quantize: QuantizeMode = "pair"
    color_mode: ColorMode = "matte"


@dataclass(frozen=True)
class FrameReport:
    input_a: str
    input_b: str
    input_a_sha256: str
    input_b_sha256: str
    output: str
    width: int
    height: int
    inference_calls: int
    inference_seconds: float
    alpha_values: List[int]
    transparent_rgba_zero: bool
    opaque_colors_output: int
    settings: Dict[str, object]


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_rgba(path: Path) -> np.ndarray:
    with Image.open(path) as image:
        return np.asarray(image.convert("RGBA"), dtype=np.uint8)


def validate_lilly_input(path: Path, rgba: np.ndarray) -> None:
    if rgba.shape != (128, 128, 4):
        raise ValueError(f"{path} must be exactly 128x128 RGBA")
    alpha_values = set(np.unique(rgba[:, :, 3]).tolist())
    if not alpha_values.issubset({0, 255}):
        raise ValueError(f"{path} has non-binary alpha: {sorted(alpha_values)}")
    if 255 not in alpha_values:
        raise ValueError(f"{path} contains no opaque pixels")


def _resize_rgb(
    rgb: np.ndarray, size: Tuple[int, int], resample: int
) -> np.ndarray:
    return np.array(
        Image.fromarray(rgb, mode="RGB").resize(size, resample=resample),
        dtype=np.uint8,
        copy=True,
    )


def _resize_float(
    channel: np.ndarray, size: Tuple[int, int], resample: int
) -> np.ndarray:
    image = Image.fromarray(channel.astype(np.float32), mode="F")
    return np.asarray(image.resize(size, resample=resample), dtype=np.float32)


def _resize_float_rgb(
    rgb: np.ndarray, size: Tuple[int, int], resample: int
) -> np.ndarray:
    return np.stack(
        [_resize_float(rgb[:, :, index], size, resample) for index in range(3)],
        axis=2,
    )


def _pair_palette(rgba0: np.ndarray, rgba1: np.ndarray) -> np.ndarray:
    colors = np.concatenate(
        (
            rgba0[:, :, :3][rgba0[:, :, 3] > 0],
            rgba1[:, :, :3][rgba1[:, :, 3] > 0],
        ),
        axis=0,
    )
    return np.unique(colors, axis=0)


def _quantize_to_palette(rgb: np.ndarray, colors: np.ndarray) -> np.ndarray:
    if len(colors) == 0 or len(colors) > 256:
        return rgb
    palette = Image.new("P", (1, 1))
    padded = np.repeat(colors[-1:, :], 256, axis=0).astype(np.uint8)
    padded[: len(colors)] = colors
    palette.putpalette(padded.reshape(-1).tolist())
    quantized = Image.fromarray(rgb, mode="RGB").quantize(
        palette=palette,
        dither=Image.Dither.NONE,
    )
    return np.array(quantized.convert("RGB"), dtype=np.uint8, copy=True)


def interpolate_pair(
    backend: FilmBackend,
    input_a: Path,
    input_b: Path,
    output: Path,
    settings: InterpolationSettings,
) -> FrameReport:
    rgba0 = load_rgba(input_a)
    rgba1 = load_rgba(input_b)
    validate_lilly_input(input_a, rgba0)
    validate_lilly_input(input_b, rgba1)
    if settings.work_scale not in {1, 2, 4}:
        raise ValueError("work_scale must be one of 1, 2, or 4")
    if not 0.0 < settings.alpha_threshold < 1.0:
        raise ValueError("alpha_threshold must be strictly between 0 and 1")

    height, width = rgba0.shape[:2]
    work_size = (width * settings.work_scale, height * settings.work_scale)
    nearest = Image.Resampling.NEAREST
    alpha0 = rgba0[:, :, 3].astype(np.float32) / 255.0
    alpha1 = rgba1[:, :, 3].astype(np.float32) / 255.0
    rgb0 = rgba0[:, :, :3].astype(np.float32) / 255.0
    rgb1 = rgba1[:, :, :3].astype(np.float32) / 255.0

    if settings.color_mode == "gray":
        color0 = rgb0 * alpha0[:, :, None] + np.float32(0.5) * (
            1.0 - alpha0[:, :, None]
        )
        color1 = rgb1 * alpha1[:, :, None] + np.float32(0.5) * (
            1.0 - alpha1[:, :, None]
        )
    elif settings.color_mode in {"matte", "premultiplied"}:
        color0 = rgb0 * alpha0[:, :, None]
        color1 = rgb1 * alpha1[:, :, None]
    else:
        raise ValueError(f"unknown color mode: {settings.color_mode}")
    color0_work = (
        _resize_rgb(
            np.rint(color0 * 255.0).astype(np.uint8),
            work_size,
            nearest,
        ).astype(np.float32)
        / 255.0
    )
    color1_work = (
        _resize_rgb(
            np.rint(color1 * 255.0).astype(np.uint8),
            work_size,
            nearest,
        ).astype(np.float32)
        / 255.0
    )
    alpha0_work = _resize_float(alpha0, work_size, nearest)
    alpha1_work = _resize_float(alpha1, work_size, nearest)
    alpha_rgb0 = np.repeat(alpha0_work[:, :, None], 3, axis=2)
    alpha_rgb1 = np.repeat(alpha1_work[:, :, None], 3, axis=2)

    color_work, color_seconds = backend.interpolate(
        color0_work,
        color1_work,
        settings.timestep,
    )
    if settings.color_mode == "matte":
        white0 = rgb0 * alpha0[:, :, None] + (1.0 - alpha0[:, :, None])
        white1 = rgb1 * alpha1[:, :, None] + (1.0 - alpha1[:, :, None])
        white0_work = (
            _resize_rgb(
                np.rint(white0 * 255.0).astype(np.uint8),
                work_size,
                nearest,
            ).astype(np.float32)
            / 255.0
        )
        white1_work = (
            _resize_rgb(
                np.rint(white1 * 255.0).astype(np.uint8),
                work_size,
                nearest,
            ).astype(np.float32)
            / 255.0
        )
        white_work, alpha_seconds = backend.interpolate(
            white0_work,
            white1_work,
            settings.timestep,
        )
        alpha_work = np.clip(
            1.0 - np.mean(white_work - color_work, axis=2),
            0.0,
            1.0,
        ).astype(np.float32)
    else:
        alpha_rgb_work, alpha_seconds = backend.interpolate(
            alpha_rgb0.astype(np.float32),
            alpha_rgb1.astype(np.float32),
            settings.timestep,
        )
        alpha_work = np.mean(alpha_rgb_work, axis=2).astype(np.float32)

    color = _resize_float_rgb(
        color_work,
        (width, height),
        Image.Resampling.BOX,
    )
    alpha_coverage = _resize_float(
        alpha_work,
        (width, height),
        Image.Resampling.BOX,
    )
    if settings.color_mode in {"matte", "premultiplied"}:
        straight_rgb = color / np.maximum(
            alpha_coverage[:, :, None], np.float32(1e-4)
        )
    else:
        straight_rgb = color
    rgb = np.rint(np.clip(straight_rgb, 0.0, 1.0) * 255.0).astype(np.uint8)
    alpha = np.where(
        alpha_coverage >= settings.alpha_threshold, 255, 0
    ).astype(np.uint8)
    if settings.quantize == "pair":
        rgb = _quantize_to_palette(rgb, _pair_palette(rgba0, rgba1))
    elif settings.quantize != "none":
        raise ValueError(f"unknown quantize mode: {settings.quantize}")
    rgb[alpha == 0] = 0
    output_rgba = np.dstack((rgb, alpha))

    output.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(output_rgba, mode="RGBA").save(output, format="PNG")
    alpha_values = sorted(
        np.unique(output_rgba[:, :, 3]).astype(int).tolist()
    )
    opaque_colors = np.unique(
        output_rgba[:, :, :3][output_rgba[:, :, 3] == 255],
        axis=0,
    )
    report = FrameReport(
        input_a=str(input_a.resolve()),
        input_b=str(input_b.resolve()),
        input_a_sha256=sha256_file(input_a),
        input_b_sha256=sha256_file(input_b),
        output=str(output.resolve()),
        width=width,
        height=height,
        inference_calls=2,
        inference_seconds=color_seconds + alpha_seconds,
        alpha_values=alpha_values,
        transparent_rgba_zero=bool(
            np.all(output_rgba[output_rgba[:, :, 3] == 0] == 0)
        ),
        opaque_colors_output=int(len(opaque_colors)),
        settings=asdict(settings),
    )
    if report.alpha_values not in ([0, 255], [255]):
        raise RuntimeError("generated alpha invariant failed")
    if not report.transparent_rgba_zero:
        raise RuntimeError("transparent-pixel invariant failed")
    return report


def _checker_composite(image: Image.Image) -> Image.Image:
    rgba = image.convert("RGBA")
    width, height = rgba.size
    y, x = np.indices((height, width))
    checker = np.where(((x // 8 + y // 8) % 2)[:, :, None], 205, 235)
    background = np.concatenate(
        (
            np.repeat(checker, 3, axis=2),
            np.full((height, width, 1), 255),
        ),
        axis=2,
    ).astype(np.uint8)
    return Image.alpha_composite(
        Image.fromarray(background, mode="RGBA"), rgba
    ).convert("RGB")


def write_previews(frame_paths: Sequence[Path], output_dir: Path) -> None:
    frames = [Image.open(path).convert("RGBA") for path in frame_paths]
    try:
        rendered = [
            _checker_composite(frame).resize(
                (frame.width * 4, frame.height * 4),
                resample=Image.Resampling.NEAREST,
            )
            for frame in frames
        ]
        sheet = Image.new(
            "RGB",
            (
                sum(frame.width for frame in rendered),
                max(frame.height for frame in rendered),
            ),
            color=(235, 235, 235),
        )
        cursor = 0
        for frame in rendered:
            sheet.paste(frame, (cursor, 0))
            cursor += frame.width
        sheet.save(output_dir / "contact-sheet.png", format="PNG")
        frames[0].save(
            output_dir / "preview.png",
            format="PNG",
            save_all=True,
            append_images=frames[1:],
            duration=65,
            loop=0,
            disposal=2,
            blend=0,
        )
    finally:
        for frame in frames:
            frame.close()


def _source_frames(input_dir: Path) -> List[Path]:
    sources = sorted(input_dir.glob("frame_*.png"))
    if len(sources) != 4:
        raise ValueError(
            f"{input_dir} must contain exactly four frame_*.png files"
        )
    return sources


def _require_empty_output(output_dir: Path) -> None:
    if output_dir.exists() and any(output_dir.iterdir()):
        raise FileExistsError(f"output directory is not empty: {output_dir}")
    output_dir.mkdir(parents=True, exist_ok=True)


def interpolate_sequence(
    backend: FilmBackend,
    input_dir: Path,
    output_dir: Path,
    settings: InterpolationSettings,
    loop: bool,
) -> Dict[str, object]:
    sources = _source_frames(input_dir)
    _require_empty_output(output_dir)
    reports: List[FrameReport] = []
    output_frames: List[Path] = []
    for index, source in enumerate(sources):
        original_output = output_dir / f"frame_{index * 2 + 1:02d}.png"
        shutil.copy2(source, original_output)
        if sha256_file(source) != sha256_file(original_output):
            raise RuntimeError(f"source copy hash mismatch for {source}")
        output_frames.append(original_output)
        if index < len(sources) - 1:
            next_source = sources[index + 1]
        elif loop:
            next_source = sources[0]
        else:
            continue
        generated_output = output_dir / f"frame_{index * 2 + 2:02d}.png"
        reports.append(
            interpolate_pair(
                backend, source, next_source, generated_output, settings
            )
        )
        output_frames.append(generated_output)
    report = {
        "input_directory": str(input_dir.resolve()),
        "output_directory": str(output_dir.resolve()),
        "loop": loop,
        "backend": asdict(backend.info),
        "frames": [asdict(item) for item in reports],
    }
    (output_dir / "report.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    write_previews(output_frames, output_dir)
    return report


def prediction_metrics(
    prediction: np.ndarray, target: np.ndarray
) -> Dict[str, float]:
    predicted_mask = prediction[:, :, 3] == 255
    target_mask = target[:, :, 3] == 255
    intersection = np.count_nonzero(predicted_mask & target_mask)
    union = np.count_nonzero(predicted_mask | target_mask)
    predicted_area = np.count_nonzero(predicted_mask)
    target_area = np.count_nonzero(target_mask)
    both = predicted_mask & target_mask
    if np.any(both):
        rgb_mae = float(
            np.abs(
                prediction[:, :, :3][both].astype(np.int16)
                - target[:, :, :3][both].astype(np.int16)
            ).mean()
            / 255.0
        )
    else:
        rgb_mae = 1.0
    structure = np.ones((3, 3), dtype=bool)
    predicted_edge = predicted_mask ^ binary_erosion(
        predicted_mask, structure=structure, border_value=0
    )
    target_edge = target_mask ^ binary_erosion(
        target_mask, structure=structure, border_value=0
    )
    target_distance = distance_transform_edt(~target_edge)
    predicted_distance = distance_transform_edt(~predicted_edge)
    predicted_count = np.count_nonzero(predicted_edge)
    target_count = np.count_nonzero(target_edge)
    precision = (
        float(np.count_nonzero(predicted_edge & (target_distance <= 1.5)))
        / predicted_count
        if predicted_count
        else 0.0
    )
    recall = (
        float(np.count_nonzero(target_edge & (predicted_distance <= 1.5)))
        / target_count
        if target_count
        else 0.0
    )
    edge_f1 = (
        2.0 * precision * recall / (precision + recall)
        if precision + recall
        else 0.0
    )
    return {
        "alpha_iou": float(intersection / union) if union else 1.0,
        "alpha_area_ratio": (
            float(predicted_area / target_area) if target_area else 1.0
        ),
        "edge_f1_with_1px_tolerance": edge_f1,
        "rgb_mae_on_shared_opaque": rgb_mae,
        "rgba_exact_fraction": float(
            np.all(prediction == target, axis=2).mean()
        ),
    }


def _passes_quality_gate(metrics: Dict[str, float]) -> bool:
    return bool(
        metrics["alpha_iou"] >= QUALITY_GATE["minimum_alpha_iou"]
        and metrics["edge_f1_with_1px_tolerance"]
        >= QUALITY_GATE["minimum_edge_f1_with_1px_tolerance"]
        and metrics["rgb_mae_on_shared_opaque"]
        <= QUALITY_GATE["maximum_rgb_mae_on_shared_opaque"]
        and QUALITY_GATE["minimum_alpha_area_ratio"]
        <= metrics["alpha_area_ratio"]
        <= QUALITY_GATE["maximum_alpha_area_ratio"]
    )


def evaluate_sequence(
    backend: FilmBackend,
    input_dir: Path,
    output_dir: Path,
    settings: InterpolationSettings,
    loop: bool,
) -> Dict[str, object]:
    sources = _source_frames(input_dir)
    _require_empty_output(output_dir)
    evaluations = [
        ("frame_02", sources[0], sources[2], sources[1]),
        ("frame_03", sources[1], sources[3], sources[2]),
    ]
    if loop:
        evaluations.extend(
            (
                ("frame_04_loop", sources[2], sources[0], sources[3]),
                ("frame_01_loop", sources[3], sources[1], sources[0]),
            )
        )
    results = []
    preview_paths: List[Path] = []
    for name, endpoint_a, endpoint_b, target_path in evaluations:
        prediction_path = output_dir / f"predicted_{name}.png"
        frame_report = interpolate_pair(
            backend, endpoint_a, endpoint_b, prediction_path, settings
        )
        metrics = prediction_metrics(
            load_rgba(prediction_path), load_rgba(target_path)
        )
        results.append(
            {
                "name": name,
                "endpoint_a": str(endpoint_a.resolve()),
                "endpoint_b": str(endpoint_b.resolve()),
                "target": str(target_path.resolve()),
                "target_sha256": sha256_file(target_path),
                "prediction": asdict(frame_report),
                "metrics": metrics,
                "passes_quality_gate": _passes_quality_gate(metrics),
            }
        )
        preview_paths.extend(
            (endpoint_a, prediction_path, target_path, endpoint_b)
        )
    metric_names = tuple(results[0]["metrics"].keys())
    means = {
        name: float(np.mean([item["metrics"][name] for item in results]))
        for name in metric_names
    }
    report = {
        "input_directory": str(input_dir.resolve()),
        "output_directory": str(output_dir.resolve()),
        "loop": loop,
        "backend": asdict(backend.info),
        "quality_gate": QUALITY_GATE,
        "passes_quality_gate": all(
            bool(item["passes_quality_gate"]) for item in results
        ),
        "mean_metrics": means,
        "evaluations": results,
    }
    (output_dir / "evaluation.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    write_previews(preview_paths, output_dir)
    return report


def run_corpus(
    backend: FilmBackend,
    input_root: Path,
    output_root: Path,
    settings: InterpolationSettings,
    loop: bool,
) -> Dict[str, object]:
    source_dirs = sorted(
        path for path in input_root.rglob("*_frames") if path.is_dir()
    )
    if not source_dirs:
        raise ValueError(f"no *_frames directories found under {input_root}")
    _require_empty_output(output_root)
    started = time.perf_counter()
    sequences = []
    for source_dir in source_dirs:
        relative = source_dir.relative_to(input_root)
        item_root = output_root / relative
        sequence = interpolate_sequence(
            backend,
            source_dir,
            item_root / "sequence",
            settings,
            loop,
        )
        evaluation = evaluate_sequence(
            backend,
            source_dir,
            item_root / "evaluation",
            settings,
            loop,
        )
        sequences.append(
            {
                "relative_directory": str(relative),
                "sequence_report": sequence,
                "evaluation_report": evaluation,
            }
        )
    report = {
        "input_root": str(input_root.resolve()),
        "output_root": str(output_root.resolve()),
        "loop": loop,
        "elapsed_seconds": time.perf_counter() - started,
        "backend": asdict(backend.info),
        "sequences": sequences,
    }
    (output_root / "corpus.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    return report
