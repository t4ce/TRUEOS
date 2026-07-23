from __future__ import annotations

import hashlib
import json
import os
import shutil
import tempfile
import time
import uuid
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Callable, Literal

import numpy as np
from PIL import Image
from scipy.ndimage import binary_erosion, distance_transform_edt

from .backend import RifeBackend


QuantizeMode = Literal["pair", "none"]
EnsembleMode = Literal["none", "median", "medoid"]
QUALITY_GATE = {
    "minimum_alpha_iou": 0.85,
    "minimum_edge_f1_with_1px_tolerance": 0.70,
    "maximum_rgb_mae_on_shared_opaque": 0.05,
    "minimum_alpha_area_ratio": 0.85,
    "maximum_alpha_area_ratio": 1.15,
}


@dataclass(frozen=True)
class InterpolationSettings:
    work_scale: int = 8
    timestep: float = 0.5
    alpha_threshold: float = 0.5
    quantize: QuantizeMode = "pair"
    background: int = 127
    inference_scale: float = 1.0
    blend_mode: Literal["temporal-alpha", "rife"] = "rife"
    ensemble: EnsembleMode = "medoid"
    face_only: bool = False


@dataclass(frozen=True)
class FrameReport:
    input_a: str
    input_b: str
    input_a_sha256: str
    input_b_sha256: str
    output: str
    width: int
    height: int
    opaque_pixels_a: int
    opaque_pixels_b: int
    opaque_pixels_output: int
    opaque_colors_output: int
    alpha_values: list[int]
    transparent_rgba_zero: bool
    inference_candidates: int
    inference_seconds: float
    selected_candidate: str
    face_region_bounds: list[int] | None
    changed_pixels_outside_face: int | None
    settings: dict[str, object]


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _source_frames(input_dir: Path) -> list[Path]:
    sources = sorted(input_dir.glob("frame_*.png"))
    names = [source.name for source in sources]
    four_frame_names = [f"frame_{index:02d}.png" for index in range(1, 5)]
    seven_frame_names = [f"frame_{index:02d}.png" for index in range(1, 8)]
    if names == four_frame_names:
        return sources
    if names == seven_frame_names:
        return sources[::2]
    else:
        raise ValueError(
            f"{input_dir} must contain an exact four-frame or seven-frame "
            f"canonical layout; found {names}"
        )


def discover_frame_sets(input_root: Path) -> list[Path]:
    frame_dirs = sorted(
        {
            frame.parent
            for frame in input_root.rglob("frame_*.png")
            if frame.is_file()
        }
    )
    if not frame_dirs:
        raise ValueError(f"{input_root} contains no frame_*.png sets")
    for frame_dir in frame_dirs:
        _source_frames(frame_dir)
    return frame_dirs


def load_rgba(path: Path) -> np.ndarray:
    with Image.open(path) as image:
        rgba = np.asarray(image.convert("RGBA"), dtype=np.uint8)
    if rgba.ndim != 3 or rgba.shape[2] != 4:
        raise ValueError(f"{path} is not RGBA-compatible")
    return rgba


def validate_lilly_input(path: Path, rgba: np.ndarray) -> None:
    if rgba.shape != (128, 128, 4):
        raise ValueError(f"{path} must be exactly 128x128 RGBA")
    alpha_values = set(np.unique(rgba[:, :, 3]).tolist())
    if not alpha_values.issubset({0, 255}):
        raise ValueError(f"{path} has non-binary alpha values: {sorted(alpha_values)}")
    if 255 not in alpha_values:
        raise ValueError(f"{path} contains no opaque pixels")


def extend_opaque_rgb(rgba: np.ndarray) -> np.ndarray:
    """Fill transparent RGB with the nearest opaque pixel's colour."""

    rgb = rgba[:, :, :3].copy()
    opaque = rgba[:, :, 3] > 0
    if opaque.all():
        return rgb
    if not opaque.any():
        raise ValueError("cannot extend RGB for a fully transparent image")
    nearest = distance_transform_edt(~opaque, return_distances=False, return_indices=True)
    transparent = ~opaque
    rgb[transparent] = rgb[
        nearest[0][transparent],
        nearest[1][transparent],
    ]
    return rgb


def _resize_rgb(rgb: np.ndarray, size: tuple[int, int], resample: int) -> np.ndarray:
    return np.array(
        Image.fromarray(rgb, mode="RGB").resize(size, resample=resample),
        dtype=np.uint8,
        copy=True,
    )


def _resize_float(channel: np.ndarray, size: tuple[int, int], resample: int) -> np.ndarray:
    image = Image.fromarray(channel.astype(np.float32), mode="F")
    return np.asarray(image.resize(size, resample=resample), dtype=np.float32)


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
    values = padded.reshape(-1).tolist()
    palette.putpalette(values)
    quantized = Image.fromarray(rgb, mode="RGB").quantize(
        palette=palette,
        dither=Image.Dither.NONE,
    )
    return np.array(quantized.convert("RGB"), dtype=np.uint8, copy=True)


def _finalize_interpolated(
    interpolated: np.ndarray,
    width: int,
    height: int,
    settings: InterpolationSettings,
    palette: np.ndarray,
) -> np.ndarray:
    rgb_work = np.rint(interpolated[:, :, :3] * 255.0).astype(np.uint8)
    alpha_work = interpolated[:, :, 3]
    rgb = _resize_rgb(rgb_work, (width, height), Image.Resampling.BOX)
    alpha_coverage = _resize_float(
        alpha_work, (width, height), Image.Resampling.BOX
    )
    alpha = np.where(alpha_coverage >= settings.alpha_threshold, 255, 0).astype(
        np.uint8
    )

    if settings.quantize == "pair":
        rgb = _quantize_to_palette(rgb, palette)
    elif settings.quantize != "none":
        raise ValueError(f"unknown quantization mode: {settings.quantize}")

    rgb[alpha == 0] = 0
    return np.dstack((rgb, alpha))


def _face_region_mask(width: int, height: int) -> np.ndarray:
    """Return Lilly's conservative inner-face mask at canonical coordinates."""

    if (width, height) != (128, 128):
        raise ValueError("the Lilly face mask requires 128x128 frames")
    y, x = np.ogrid[:height, :width]
    ellipse = ((x - 64.0) / 20.0) ** 2 + ((y - 56.0) / 15.0) ** 2 <= 1.0
    return ellipse & (y >= 43) & (y <= 69)


def _composite_face_only(
    generated: np.ndarray, carrier: np.ndarray
) -> tuple[np.ndarray, np.ndarray]:
    """Apply generated RGB only inside the face while preserving carrier alpha."""

    if generated.shape != carrier.shape:
        raise ValueError("generated and carrier frames must have matching shapes")
    height, width = carrier.shape[:2]
    face_mask = _face_region_mask(width, height)
    replace = (
        face_mask
        & (carrier[:, :, 3] == 255)
        & (generated[:, :, 3] == 255)
    )
    result = carrier.copy()
    result[:, :, :3][replace] = generated[:, :, :3][replace]
    return result, face_mask


def interpolate_pair(
    backend: RifeBackend,
    input_a: Path,
    input_b: Path,
    output: Path,
    settings: InterpolationSettings,
) -> FrameReport:
    rgba0 = load_rgba(input_a)
    rgba1 = load_rgba(input_b)
    validate_lilly_input(input_a, rgba0)
    validate_lilly_input(input_b, rgba1)

    if settings.work_scale not in {1, 2, 4, 8, 16}:
        raise ValueError("work_scale must be one of 1, 2, 4, 8, or 16")
    if not 0.0 < settings.alpha_threshold < 1.0:
        raise ValueError("alpha_threshold must be strictly between 0 and 1")
    if not 0 <= settings.background <= 255:
        raise ValueError("background must be between 0 and 255")

    height, width = rgba0.shape[:2]
    work_size = (width * settings.work_scale, height * settings.work_scale)
    nearest = Image.Resampling.NEAREST

    filled0 = extend_opaque_rgb(rgba0)
    filled1 = extend_opaque_rgb(rgba1)
    alpha0 = rgba0[:, :, 3].astype(np.float32) / 255.0
    alpha1 = rgba1[:, :, 3].astype(np.float32) / 255.0

    filled0_work = _resize_rgb(filled0, work_size, nearest).astype(np.float32) / 255.0
    filled1_work = _resize_rgb(filled1, work_size, nearest).astype(np.float32) / 255.0
    alpha0_work = _resize_float(alpha0, work_size, nearest)
    alpha1_work = _resize_float(alpha1, work_size, nearest)

    payload0 = np.concatenate((filled0_work, alpha0_work[:, :, None]), axis=2)
    payload1 = np.concatenate((filled1_work, alpha1_work[:, :, None]), axis=2)
    if settings.ensemble == "none":
        candidate_specs = ((settings.background, False, False),)
    elif settings.ensemble in {"median", "medoid"}:
        backgrounds = tuple(sorted({32, settings.background, 224}))
        candidate_specs = tuple(
            (background, reverse, flip)
            for background in backgrounds
            for reverse in (False, True)
            for flip in (False, True)
        )
    else:
        raise ValueError(f"unknown ensemble mode: {settings.ensemble}")

    candidates: list[np.ndarray] = []
    inference_started = time.perf_counter()
    for background_value, reverse, flip in candidate_specs:
        background = np.float32(background_value / 255.0)
        motion0 = (
            filled0_work * alpha0_work[:, :, None]
            + background * (1.0 - alpha0_work[:, :, None])
        )
        motion1 = (
            filled1_work * alpha1_work[:, :, None]
            + background * (1.0 - alpha1_work[:, :, None])
        )
        candidate_motion0, candidate_motion1 = motion0, motion1
        candidate_payload0, candidate_payload1 = payload0, payload1
        candidate_timestep = settings.timestep
        if reverse:
            candidate_motion0, candidate_motion1 = motion1, motion0
            candidate_payload0, candidate_payload1 = payload1, payload0
            candidate_timestep = 1.0 - settings.timestep
        if flip:
            candidate_motion0 = np.flip(candidate_motion0, axis=1).copy()
            candidate_motion1 = np.flip(candidate_motion1, axis=1).copy()
            candidate_payload0 = np.flip(candidate_payload0, axis=1).copy()
            candidate_payload1 = np.flip(candidate_payload1, axis=1).copy()
        candidate = backend.interpolate_payload(
            candidate_motion0,
            candidate_motion1,
            candidate_payload0,
            candidate_payload1,
            timestep=candidate_timestep,
            inference_scale=settings.inference_scale,
            blend_mode=settings.blend_mode,
        )
        if flip:
            candidate = np.flip(candidate, axis=1).copy()
        candidates.append(candidate)
    inference_seconds = time.perf_counter() - inference_started

    palette = _pair_palette(rgba0, rgba1)
    if settings.ensemble == "none":
        output_rgba = _finalize_interpolated(
            candidates[0], width, height, settings, palette
        )
        selected_candidate = (
            f"background={candidate_specs[0][0]},reverse=false,flip=false"
        )
    else:
        candidate_stack = np.stack(candidates, axis=0)
        candidate_median = np.median(candidate_stack, axis=0)
        if settings.ensemble == "median":
            interpolated = candidate_median
            output_rgba = _finalize_interpolated(
                interpolated, width, height, settings, palette
            )
            selected_candidate = "per-pixel-median"
        else:
            if settings.face_only:
                face_mask = _face_region_mask(width, height)
                face_mask_work = _resize_float(
                    face_mask.astype(np.float32),
                    work_size,
                    Image.Resampling.NEAREST,
                ) >= 0.5
                medoid_values = candidate_stack[:, face_mask_work, :]
                median_values = candidate_median[face_mask_work, :]
            else:
                medoid_values = candidate_stack
                median_values = candidate_median
            distances = np.mean(
                np.abs(medoid_values - median_values[None, ...]),
                axis=tuple(range(1, medoid_values.ndim)),
            )
            selected_index = int(np.argmin(distances))
            output_rgba = _finalize_interpolated(
                candidate_stack[selected_index], width, height, settings, palette
            )
            background_value, reverse, flip = candidate_specs[selected_index]
            selected_candidate = (
                f"background={background_value},"
                f"reverse={str(reverse).lower()},flip={str(flip).lower()}"
            )

    face_region_bounds = None
    changed_pixels_outside_face = None
    if settings.face_only:
        output_rgba, face_mask = _composite_face_only(output_rgba, rgba0)
        face_y, face_x = np.nonzero(face_mask)
        face_region_bounds = [
            int(face_x.min()),
            int(face_y.min()),
            int(face_x.max() + 1),
            int(face_y.max() + 1),
        ]
        changed_pixels_outside_face = int(
            np.count_nonzero(
                np.any(output_rgba[~face_mask] != rgba0[~face_mask], axis=1)
            )
        )

    output.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(output_rgba, mode="RGBA").save(output, format="PNG")

    alpha_values = sorted(np.unique(output_rgba[:, :, 3]).astype(int).tolist())
    opaque_colors = np.unique(
        output_rgba[:, :, :3][output_rgba[:, :, 3] == 255], axis=0
    )
    report = FrameReport(
        input_a=str(input_a.resolve()),
        input_b=str(input_b.resolve()),
        input_a_sha256=sha256_file(input_a),
        input_b_sha256=sha256_file(input_b),
        output=str(output.resolve()),
        width=width,
        height=height,
        opaque_pixels_a=int(np.count_nonzero(rgba0[:, :, 3])),
        opaque_pixels_b=int(np.count_nonzero(rgba1[:, :, 3])),
        opaque_pixels_output=int(np.count_nonzero(output_rgba[:, :, 3])),
        opaque_colors_output=int(len(opaque_colors)),
        alpha_values=alpha_values,
        transparent_rgba_zero=bool(
            np.all(output_rgba[output_rgba[:, :, 3] == 0] == 0)
        ),
        inference_candidates=len(candidates),
        inference_seconds=inference_seconds,
        selected_candidate=selected_candidate,
        face_region_bounds=face_region_bounds,
        changed_pixels_outside_face=changed_pixels_outside_face,
        settings=asdict(settings),
    )
    if report.alpha_values not in ([0, 255], [255]):
        raise RuntimeError(f"generated alpha invariant failed: {report.alpha_values}")
    if not report.transparent_rgba_zero:
        raise RuntimeError("generated transparent-pixel invariant failed")
    return report


def _checker_composite(image: Image.Image) -> Image.Image:
    rgba = image.convert("RGBA")
    width, height = rgba.size
    tile = 8
    y, x = np.indices((height, width))
    checker = np.where(((x // tile + y // tile) % 2)[:, :, None], 205, 235)
    background = np.concatenate(
        (np.repeat(checker, 3, axis=2), np.full((height, width, 1), 255)), axis=2
    ).astype(np.uint8)
    base = Image.fromarray(background, mode="RGBA")
    return Image.alpha_composite(base, rgba).convert("RGB")


def write_previews(frame_paths: list[Path], output_dir: Path) -> None:
    frames = [Image.open(path).convert("RGBA") for path in frame_paths]
    try:
        zoom = 4
        rendered = [
            _checker_composite(frame).resize(
                (frame.width * zoom, frame.height * zoom),
                resample=Image.Resampling.NEAREST,
            )
            for frame in frames
        ]
        sheet = Image.new(
            "RGB",
            (sum(frame.width for frame in rendered), max(frame.height for frame in rendered)),
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


def interpolate_sequence(
    backend: RifeBackend,
    input_dir: Path,
    output_dir: Path,
    settings: InterpolationSettings,
    loop: bool,
) -> dict[str, object]:
    sources = _source_frames(input_dir)
    if output_dir.exists() and any(output_dir.iterdir()):
        raise FileExistsError(f"output directory is not empty: {output_dir}")
    output_dir.mkdir(parents=True, exist_ok=True)

    reports: list[FrameReport] = []
    output_frames: list[Path] = []
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
                backend,
                source,
                next_source,
                generated_output,
                settings,
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


def interpolate_library(
    backend: RifeBackend,
    input_root: Path,
    output_root: Path,
    settings: InterpolationSettings,
    progress: Callable[[int, int, Path], None] | None = None,
) -> dict[str, object]:
    """Generate mirrored seven-frame sets for an entire Lilly asset tree."""

    input_root = input_root.resolve()
    output_root = output_root.resolve()
    frame_sets = discover_frame_sets(input_root)
    if output_root.exists() and any(output_root.iterdir()):
        raise FileExistsError(f"output directory is not empty: {output_root}")
    output_root.mkdir(parents=True, exist_ok=True)

    results: list[dict[str, object]] = []
    total = len(frame_sets)
    for index, source_dir in enumerate(frame_sets, start=1):
        relative_dir = source_dir.relative_to(input_root)
        if progress is not None:
            progress(index, total, relative_dir)
        result = interpolate_sequence(
            backend,
            source_dir,
            output_root / relative_dir,
            settings,
            loop=False,
        )
        results.append(
            {
                "relative_directory": str(relative_dir),
                "source_directory": str(source_dir),
                "output_directory": result["output_directory"],
                "frames": result["frames"],
            }
        )

    report = {
        "input_root": str(input_root),
        "output_root": str(output_root),
        "frame_set_count": total,
        "generated_frame_count": sum(len(item["frames"]) for item in results),
        "inference_seconds": sum(
            frame["inference_seconds"]
            for item in results
            for frame in item["frames"]
        ),
        "settings": asdict(settings),
        "backend": asdict(backend.info),
        "sets": results,
    }
    (output_root / "library-report.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    return report


def promote_library(
    staging_root: Path,
    target_root: Path,
    apply: bool = False,
) -> dict[str, object]:
    """Validate and atomically replace canonical frame directories from staging."""

    staging_root = staging_root.resolve()
    target_root = target_root.resolve()
    library_report_path = staging_root / "library-report.json"
    if not library_report_path.is_file():
        raise FileNotFoundError(f"missing staging manifest: {library_report_path}")
    library_report = json.loads(library_report_path.read_text(encoding="utf-8"))
    if Path(library_report["input_root"]).resolve() != target_root:
        raise ValueError(
            "staging manifest input root does not match promotion target: "
            f"{library_report['input_root']} != {target_root}"
        )
    settings = library_report["settings"]
    if not settings.get("face_only"):
        raise ValueError("refusing to promote a non-face-only staging manifest")
    if settings.get("work_scale") != 16 or settings.get("ensemble") != "medoid":
        raise ValueError("promotion requires the 16x medoid HighSettings profile")

    target_sets = discover_frame_sets(target_root)
    target_relatives = {path.relative_to(target_root) for path in target_sets}
    manifest_relatives = {
        Path(item["relative_directory"]) for item in library_report["sets"]
    }
    if target_relatives != manifest_relatives:
        raise ValueError("staging and target frame-set layouts do not match")

    expected_stage_names = [f"frame_{index:02d}.png" for index in range(1, 8)]
    original_output_positions = (1, 3, 5, 7)
    preflight: list[tuple[Path, Path]] = []
    original_hashes: dict[str, list[str]] = {}
    refreshed_hashes: dict[str, list[str]] = {}
    for item in library_report["sets"]:
        relative = Path(item["relative_directory"])
        target_dir = target_root / relative
        staged_dir = staging_root / relative
        staged_frames = sorted(staged_dir.glob("frame_*.png"))
        if [path.name for path in staged_frames] != expected_stage_names:
            raise ValueError(f"invalid staged frame layout: {staged_dir}")

        current_sources = _source_frames(target_dir)
        allowed_current_names = {path.name for path in target_dir.glob("frame_*.png")}
        unexpected = [
            path
            for path in target_dir.iterdir()
            if not path.is_file() or path.name not in allowed_current_names
        ]
        if unexpected:
            raise ValueError(
                f"target frame directory contains non-frame entries: {unexpected}"
            )

        source_hashes = [sha256_file(path) for path in current_sources]
        staged_original_hashes = [
            sha256_file(staged_dir / f"frame_{position:02d}.png")
            for position in original_output_positions
        ]
        if source_hashes != staged_original_hashes:
            raise ValueError(f"staged originals do not match target: {relative}")

        for staged_frame in staged_frames:
            rgba = load_rgba(staged_frame)
            validate_lilly_input(staged_frame, rgba)
            if not np.all(rgba[rgba[:, :, 3] == 0] == 0):
                raise ValueError(
                    f"staged frame has nonzero transparent RGB: {staged_frame}"
                )
        for frame_report in item["frames"]:
            if (
                frame_report["inference_candidates"] != 12
                or frame_report["changed_pixels_outside_face"] != 0
                or frame_report["face_region_bounds"] != [44, 43, 85, 70]
                or not frame_report["transparent_rgba_zero"]
            ):
                raise ValueError(f"staged invariant failure: {relative}")

        relative_key = str(relative)
        original_hashes[relative_key] = source_hashes
        refreshed_hashes[relative_key] = [
            sha256_file(path) for path in staged_frames
        ]
        preflight.append((staged_dir, target_dir))

    promoted = 0
    if apply:
        swaps: list[tuple[Path, Path]] = []
        try:
            for staged_dir, target_dir in preflight:
                temporary_dir = Path(
                    tempfile.mkdtemp(
                        prefix=f".{target_dir.name}.refresh-",
                        dir=target_dir.parent,
                    )
                )
                for frame_name in expected_stage_names:
                    shutil.copy2(staged_dir / frame_name, temporary_dir / frame_name)
                previous_dir = target_dir.parent / (
                    f".{target_dir.name}.before-refresh-{uuid.uuid4().hex}"
                )
                os.replace(target_dir, previous_dir)
                try:
                    os.replace(temporary_dir, target_dir)
                except BaseException:
                    os.replace(previous_dir, target_dir)
                    shutil.rmtree(temporary_dir, ignore_errors=True)
                    raise
                swaps.append((target_dir, previous_dir))
                promoted += 1
        except BaseException:
            for target_dir, previous_dir in reversed(swaps):
                failed_dir = target_dir.parent / (
                    f".{target_dir.name}.failed-refresh-{uuid.uuid4().hex}"
                )
                os.replace(target_dir, failed_dir)
                os.replace(previous_dir, target_dir)
                shutil.rmtree(failed_dir, ignore_errors=True)
            raise
        for _, previous_dir in swaps:
            shutil.rmtree(previous_dir)

    report = {
        "staging_root": str(staging_root),
        "target_root": str(target_root),
        "checked_frame_sets": len(preflight),
        "promoted_frame_sets": promoted,
        "applied": apply,
        "settings": settings,
        "backend": library_report["backend"],
        "original_hashes": original_hashes,
        "refreshed_hashes": refreshed_hashes,
    }
    report_name = "promotion-report.json" if apply else "promotion-preflight.json"
    (staging_root / report_name).write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    return report


def _prediction_metrics(prediction: np.ndarray, target: np.ndarray) -> dict[str, float]:
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
    predicted_edge_count = np.count_nonzero(predicted_edge)
    target_edge_count = np.count_nonzero(target_edge)
    target_distance = distance_transform_edt(~target_edge)
    predicted_distance = distance_transform_edt(~predicted_edge)
    edge_precision = (
        float(np.count_nonzero(predicted_edge & (target_distance <= 1.5)))
        / predicted_edge_count
        if predicted_edge_count
        else 0.0
    )
    edge_recall = (
        float(np.count_nonzero(target_edge & (predicted_distance <= 1.5)))
        / target_edge_count
        if target_edge_count
        else 0.0
    )
    edge_f1 = (
        2.0 * edge_precision * edge_recall / (edge_precision + edge_recall)
        if edge_precision + edge_recall
        else 0.0
    )
    rgba_exact = float(np.all(prediction == target, axis=2).mean())
    return {
        "alpha_iou": float(intersection / union) if union else 1.0,
        "alpha_area_ratio": float(predicted_area / target_area) if target_area else 1.0,
        "edge_f1_with_1px_tolerance": edge_f1,
        "rgb_mae_on_shared_opaque": rgb_mae,
        "rgba_exact_fraction": rgba_exact,
    }


def _passes_quality_gate(metrics: dict[str, float]) -> bool:
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
    backend: RifeBackend,
    input_dir: Path,
    output_dir: Path,
    settings: InterpolationSettings,
    loop: bool,
) -> dict[str, object]:
    """Predict held-out keyframes from their two temporal neighbours."""

    sources = _source_frames(input_dir)
    if output_dir.exists() and any(output_dir.iterdir()):
        raise FileExistsError(f"output directory is not empty: {output_dir}")
    output_dir.mkdir(parents=True, exist_ok=True)

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

    results: list[dict[str, object]] = []
    preview_paths: list[Path] = []
    for name, endpoint_a, endpoint_b, target_path in evaluations:
        prediction_path = output_dir / f"predicted_{name}.png"
        frame_report = interpolate_pair(
            backend,
            endpoint_a,
            endpoint_b,
            prediction_path,
            settings,
        )
        prediction = load_rgba(prediction_path)
        target = load_rgba(target_path)
        metrics = _prediction_metrics(prediction, target)
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
        preview_paths.extend((endpoint_a, prediction_path, target_path, endpoint_b))

    metric_names = tuple(results[0]["metrics"].keys())
    means = {
        name: float(np.mean([result["metrics"][name] for result in results]))
        for name in metric_names
    }
    report = {
        "input_directory": str(input_dir.resolve()),
        "output_directory": str(output_dir.resolve()),
        "loop": loop,
        "backend": asdict(backend.info),
        "quality_gate": QUALITY_GATE,
        "passes_quality_gate": all(
            bool(result["passes_quality_gate"]) for result in results
        ),
        "mean_metrics": means,
        "evaluations": results,
    }
    (output_dir / "evaluation.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    write_previews(preview_paths, output_dir)
    return report
