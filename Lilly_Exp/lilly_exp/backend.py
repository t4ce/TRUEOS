from __future__ import annotations

import importlib
import sys
from dataclasses import dataclass
from pathlib import Path

import numpy as np


RIFE_COMMIT = "17d8c7a1005b37f4c97bfee04e316aaec7fdc536"
RIFE_MODEL = "4.25"
RIFE_MODEL_SHA256 = (
    "e63d481b7ae5d4a4e6ad7ac5b410ff78f3bf7be3b51b2e38ca8152747abde5b4"
)


@dataclass(frozen=True)
class BackendInfo:
    device: str
    gpu_name: str | None
    torch_version: str
    rife_model: str = RIFE_MODEL
    rife_commit: str = RIFE_COMMIT
    rife_model_sha256: str = RIFE_MODEL_SHA256


class RifeBackend:
    """Loads Practical-RIFE and exposes its final flow for RGBA payload warping."""

    def __init__(self, rife_dir: Path, device: str = "auto") -> None:
        rife_dir = rife_dir.resolve()
        checkpoint = rife_dir / "train_log" / "flownet.pkl"
        model_source = rife_dir / "train_log" / "IFNet_HDv3.py"
        if not checkpoint.is_file() or not model_source.is_file():
            raise RuntimeError(
                f"RIFE runtime is incomplete at {rife_dir}; run Lilly_Exp/setup.sh"
            )

        import torch

        if device == "auto":
            selected = "cuda" if torch.cuda.is_available() else "cpu"
        else:
            selected = device
        if selected == "cuda" and not torch.cuda.is_available():
            raise RuntimeError("CUDA was requested but torch.cuda.is_available() is false")

        rife_path = str(rife_dir)
        if rife_path not in sys.path:
            sys.path.insert(0, rife_path)

        ifnet_module = importlib.import_module("train_log.IFNet_HDv3")
        warp_module = importlib.import_module("model.warplayer")
        self._torch = torch
        self._warp = warp_module.warp
        self.device = torch.device(selected)
        self.net = ifnet_module.IFNet().to(self.device)

        state = torch.load(checkpoint, map_location=self.device, weights_only=True)
        if any(key.startswith("module.") for key in state):
            state = {
                key.removeprefix("module."): value
                for key, value in state.items()
                if key.startswith("module.")
            }
        load_result = self.net.load_state_dict(state, strict=False)
        allowed_extra_prefixes = ("teacher.", "caltime.")
        unexpected = [
            key
            for key in load_result.unexpected_keys
            if not key.startswith(allowed_extra_prefixes)
        ]
        if load_result.missing_keys or unexpected:
            raise RuntimeError(
                "RIFE checkpoint does not match its inference network: "
                f"missing={load_result.missing_keys}, unexpected={unexpected}"
            )
        self.net.eval()

        gpu_name = None
        if self.device.type == "cuda":
            gpu_name = torch.cuda.get_device_name(self.device)
        self.info = BackendInfo(
            device=str(self.device),
            gpu_name=gpu_name,
            torch_version=torch.__version__,
        )

    def interpolate_payload(
        self,
        motion0: np.ndarray,
        motion1: np.ndarray,
        payload0: np.ndarray,
        payload1: np.ndarray,
        timestep: float,
        inference_scale: float = 1.0,
        blend_mode: str = "rife",
    ) -> np.ndarray:
        """Infer motion from RGB and apply the same flow/mask to RGBA payloads."""

        torch = self._torch
        if motion0.shape != motion1.shape or payload0.shape != payload1.shape:
            raise ValueError("endpoint arrays must have matching shapes")
        if motion0.shape[:2] != payload0.shape[:2]:
            raise ValueError("motion and payload dimensions must match")
        if motion0.shape[2] != 3 or payload0.shape[2] != 4:
            raise ValueError("expected HxWx3 motion and HxWx4 payload arrays")
        if not 0.0 < timestep < 1.0:
            raise ValueError("timestep must be strictly between 0 and 1")

        height, width = motion0.shape[:2]
        multiple = max(64, int(round(64 / inference_scale)))
        padded_h = ((height + multiple - 1) // multiple) * multiple
        padded_w = ((width + multiple - 1) // multiple) * multiple

        def tensor(array: np.ndarray):
            value = torch.from_numpy(array.transpose(2, 0, 1)).unsqueeze(0)
            value = value.to(device=self.device, dtype=torch.float32)
            return torch.nn.functional.pad(
                value, (0, padded_w - width, 0, padded_h - height), mode="replicate"
            )

        motion0_t = tensor(motion0)
        motion1_t = tensor(motion1)
        payload0_t = tensor(payload0)
        payload1_t = tensor(payload1)
        scale_list = [
            16 / inference_scale,
            8 / inference_scale,
            4 / inference_scale,
            2 / inference_scale,
            1 / inference_scale,
        ]

        with torch.inference_mode():
            flows, mask_logits, _ = self.net(
                torch.cat((motion0_t, motion1_t), dim=1),
                timestep=timestep,
                scale_list=scale_list,
            )
            flow = flows[-1]
            mask = torch.sigmoid(mask_logits)
            warped0 = self._warp(payload0_t, flow[:, :2])
            warped1 = self._warp(payload1_t, flow[:, 2:4])
            rife_merged = warped0 * mask + warped1 * (1.0 - mask)
            if blend_mode == "rife":
                merged = rife_merged
            elif blend_mode == "temporal-alpha":
                alpha0 = warped0[:, 3:4].clamp(0.0, 1.0)
                alpha1 = warped1[:, 3:4].clamp(0.0, 1.0)
                both_opaque = (alpha0 > 1e-4) & (alpha1 > 1e-4)
                weight0 = torch.where(both_opaque, mask * alpha0, alpha0)
                weight1 = torch.where(both_opaque, (1.0 - mask) * alpha1, alpha1)
                weight_sum = weight0 + weight1
                rgb = (
                    warped0[:, :3] * weight0 + warped1[:, :3] * weight1
                ) / weight_sum.clamp_min(1e-6)
                rgb = torch.where(weight_sum > 1e-6, rgb, rife_merged[:, :3])
                alpha = (
                    alpha0 * (1.0 - timestep) + alpha1 * timestep
                ).clamp(0.0, 1.0)
                merged = torch.cat((rgb, alpha), dim=1)
            else:
                raise ValueError(f"unknown payload blend mode: {blend_mode}")

        result = (
            merged[0, :, :height, :width]
            .clamp(0.0, 1.0)
            .permute(1, 2, 0)
            .cpu()
            .numpy()
        )
        return result
