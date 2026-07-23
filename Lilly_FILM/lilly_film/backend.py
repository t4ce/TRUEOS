from __future__ import annotations

import hashlib
import os
import platform
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Dict, Tuple

import numpy as np


FILM_COMMIT = "69f8708f08e62c2edf46a27616a4bfcf083e2076"
FILM_MODEL = "Style"
MODEL_FILES = {
    "keras_metadata.pb": (
        "0291f451e35e62a042fa49a1341af1dc8a94632188a24a16b71a9516e9fc6853"
    ),
    "saved_model.pb": (
        "4df311e80e9a7282b362a7e93bef22a1ce4f84e7cdeda01f246894545eaaf985"
    ),
    "variables/variables.data-00000-of-00001": (
        "8c47323923bc4826b730dd882c8c7700761aa3ac03b2c8180d3ffc82d18111f9"
    ),
    "variables/variables.index": (
        "d19bb117eb9abe6121b5711649bb7d5d1c4fe1912b9deabbdafa2be3f5a273e5"
    ),
}


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


@dataclass(frozen=True)
class BackendInfo:
    device: str
    python_version: str
    tensorflow_version: str
    film_model: str
    film_commit: str
    model_files_sha256: Dict[str, str]


class FilmBackend:
    """Loads the official FILM Style SavedModel and performs one direct step."""

    def __init__(self, model_dir: Path, device: str = "cpu") -> None:
        model_dir = model_dir.resolve()
        for relative, expected_sha in MODEL_FILES.items():
            path = model_dir / relative
            if not path.is_file():
                raise RuntimeError(
                    "FILM runtime is incomplete; run Lilly_FILM/setup.sh"
                )
            actual_sha = _sha256(path)
            if actual_sha != expected_sha:
                raise RuntimeError(
                    "FILM model checksum mismatch for "
                    f"{path}: expected {expected_sha}, got {actual_sha}"
                )

        if device not in {"cpu", "auto"}:
            raise ValueError("device must be 'cpu' or 'auto'")
        if device == "cpu":
            os.environ["CUDA_VISIBLE_DEVICES"] = "-1"
        os.environ.setdefault("TF_CPP_MIN_LOG_LEVEL", "3")

        import tensorflow as tf

        self._tf = tf
        self._model = tf.compat.v2.saved_model.load(str(model_dir))
        visible_gpus = tf.config.list_physical_devices("GPU")
        selected_device = "gpu" if visible_gpus else "cpu"
        if device == "cpu":
            selected_device = "cpu"
        self.info = BackendInfo(
            device=selected_device,
            python_version=platform.python_version(),
            tensorflow_version=tf.__version__,
            film_model=FILM_MODEL,
            film_commit=FILM_COMMIT,
            model_files_sha256=dict(MODEL_FILES),
        )

    def interpolate(
        self,
        frame0: np.ndarray,
        frame1: np.ndarray,
        timestep: float = 0.5,
    ) -> Tuple[np.ndarray, float]:
        if frame0.shape != frame1.shape:
            raise ValueError("FILM endpoint arrays must have matching shapes")
        if frame0.ndim != 3 or frame0.shape[2] != 3:
            raise ValueError("FILM expects HxWx3 arrays")
        if frame0.dtype != np.float32 or frame1.dtype != np.float32:
            raise ValueError("FILM arrays must be float32")
        if not 0.0 < timestep < 1.0:
            raise ValueError("timestep must be strictly between 0 and 1")

        inputs = {
            "x0": frame0[None, ...],
            "x1": frame1[None, ...],
            "time": np.array([[timestep]], dtype=np.float32),
        }
        started = time.perf_counter()
        result = self._model(inputs, training=False)["image"][0].numpy()
        elapsed = time.perf_counter() - started
        return np.clip(result, 0.0, 1.0).astype(np.float32), elapsed

    def doctor_report(self) -> Dict[str, object]:
        sample0 = np.zeros((128, 128, 3), dtype=np.float32)
        sample1 = np.ones((128, 128, 3), dtype=np.float32)
        result, elapsed = self.interpolate(sample0, sample1)
        return {
            "backend": asdict(self.info),
            "smoke_test": {
                "input_size": [128, 128],
                "output_size": list(result.shape[:2]),
                "inference_seconds": elapsed,
                "finite": bool(np.isfinite(result).all()),
            },
        }
