#!/usr/bin/env python3

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
TOOLS_DIR = ROOT / "tools" / "xe_lp_shader_bake"
DEFAULT_GENERATED = ROOT / ".codex_tmp" / "generated_simple.rs"
DEFAULT_VERT = TOOLS_DIR / "simple_triangle.vert"
DEFAULT_FRAG = TOOLS_DIR / "simple_triangle.frag"
DEFAULT_DUMPER_SRC = TOOLS_DIR / "simple_triangle_dump.c"
DEFAULT_OUT_DIR = ROOT / ".codex_tmp" / "host_shader_validation"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "One-shot host-side validation for the TRUEOS trivial triangle. "
            "Compiles GLSL to SPIR-V, builds the Vulkan dumper, runs it, and "
            "validates a generated shader artifact against the host proof log."
        )
    )
    parser.add_argument("--vert", type=Path, default=DEFAULT_VERT)
    parser.add_argument("--frag", type=Path, default=DEFAULT_FRAG)
    parser.add_argument("--generated", type=Path, default=DEFAULT_GENERATED)
    parser.add_argument("--out-dir", type=Path, default=DEFAULT_OUT_DIR)
    parser.add_argument("--keep-going", action="store_true", help="Keep partial outputs on failure.")
    parser.add_argument(
        "--require-verified-artifact",
        action="store_true",
        help="Also require TRIANGLE_PIPELINE_NOTE to say verified=1.",
    )
    parser.add_argument(
        "--vs-bin",
        type=Path,
        help="Optional fresh VS machine-code blob to package before validation.",
    )
    parser.add_argument(
        "--ps-bin",
        type=Path,
        help="Optional fresh PS machine-code blob to package before validation.",
    )
    parser.add_argument(
        "--vs-meta",
        type=Path,
        help="Optional VS metadata JSON used with --vs-bin.",
    )
    parser.add_argument(
        "--ps-meta",
        type=Path,
        help="Optional PS metadata JSON used with --ps-bin.",
    )
    parser.add_argument(
        "--note-prefix",
        default="mesa-host-validation target=gfx125",
        help="Prefix for emit_generated_rs.py when packaging a fresh artifact.",
    )
    parser.add_argument(
        "--verified",
        type=int,
        choices=(0, 1),
        default=0,
        help="verified= value to embed when packaging a fresh artifact.",
    )
    return parser.parse_args()


def run(cmd: list[str], *, cwd: Path | None = None, env: dict[str, str] | None = None, log_path: Path | None = None) -> None:
    print("+", " ".join(cmd))
    result = subprocess.run(
        cmd,
        cwd=cwd,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    if log_path is not None:
        log_path.write_text(result.stdout)
    else:
        sys.stdout.write(result.stdout)
    if result.returncode != 0:
        raise SystemExit(result.returncode)


def ensure_tool(name: str) -> str:
    path = shutil.which(name)
    if path is None:
        raise SystemExit(f"required tool not found in PATH: {name}")
    return path


def detect_glsl_compiler() -> list[str]:
    glslang = shutil.which("glslangValidator")
    if glslang is not None:
        return [glslang, "-V"]
    glslc = shutil.which("glslc")
    if glslc is not None:
        return [glslc]
    raise SystemExit("need glslangValidator or glslc in PATH")


def pkg_config_libs() -> list[str]:
    pkg_config = shutil.which("pkg-config")
    if pkg_config is None:
        return ["-lvulkan"]
    result = subprocess.run(
        [pkg_config, "--cflags", "--libs", "vulkan"],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if result.returncode != 0:
        return ["-lvulkan"]
    return result.stdout.split() or ["-lvulkan"]


def compile_spirv(compiler: list[str], src: Path, stage_suffix: str, dst: Path) -> None:
    if compiler[0].endswith("glslangValidator"):
        cmd = compiler + ["-S", stage_suffix, "-o", str(dst), str(src)]
    else:
        cmd = compiler + ["-o", str(dst), str(src)]
    run(cmd)


def maybe_package_fresh_artifact(args: argparse.Namespace) -> None:
    fresh_args = [args.vs_bin, args.ps_bin, args.vs_meta, args.ps_meta]
    if all(value is None for value in fresh_args):
        return
    if any(value is None for value in fresh_args):
        raise SystemExit(
            "fresh packaging requires --vs-bin, --ps-bin, --vs-meta, and --ps-meta together"
        )
    args.generated.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        sys.executable,
        str(TOOLS_DIR / "emit_generated_rs.py"),
        "--vs-bin",
        str(args.vs_bin),
        "--ps-bin",
        str(args.ps_bin),
        "--vs-meta",
        str(args.vs_meta),
        "--ps-meta",
        str(args.ps_meta),
        "--out",
        str(args.generated),
        "--note-prefix",
        args.note_prefix,
        "--verified",
        str(args.verified),
    ]
    run(cmd)


def main() -> None:
    args = parse_args()
    cc = ensure_tool("cc")
    compiler = detect_glsl_compiler()
    args.out_dir.mkdir(parents=True, exist_ok=True)
    args.generated.parent.mkdir(parents=True, exist_ok=True)

    maybe_package_fresh_artifact(args)

    vs_spv = args.out_dir / "simple_triangle.vert.spv"
    fs_spv = args.out_dir / "simple_triangle.frag.spv"
    dumper_bin = args.out_dir / "simple_triangle_dump"
    dump_log = args.out_dir / "simple_triangle_dump.log"
    exec_dump_dir = args.out_dir / "pipeline_exec"

    try:
        compile_spirv(compiler, args.vert, "vert", vs_spv)
        compile_spirv(compiler, args.frag, "frag", fs_spv)

        compile_cmd = [cc, str(DEFAULT_DUMPER_SRC), "-o", str(dumper_bin), *pkg_config_libs()]
        run(compile_cmd)

        env = os.environ.copy()
        exec_dump_dir.mkdir(parents=True, exist_ok=True)
        env["TRUEOS_EXECUTABLE_DUMP_DIR"] = str(exec_dump_dir)
        run([str(dumper_bin), str(vs_spv), str(fs_spv)], env=env, log_path=dump_log)

        verify_cmd = [
            sys.executable,
            str(TOOLS_DIR / "verify_artifact.py"),
            str(args.generated),
            "--dump-log",
            str(dump_log),
        ]
        if args.require_verified_artifact:
            verify_cmd.append("--require-verified")
        run(verify_cmd)

        cache_blob = exec_dump_dir / "pipeline_cache.bin"
        if cache_blob.exists():
            extract_dir = args.out_dir / "cache_extract"
            run(
                [
                    sys.executable,
                    str(TOOLS_DIR / "extract_from_pipeline_cache.py"),
                    str(cache_blob),
                    str(exec_dump_dir),
                    "--out-dir",
                    str(extract_dir),
                ]
            )
    except BaseException:
        if not args.keep_going and args.out_dir.exists():
            pass
        raise

    print(f"host validation ok: {args.generated}")
    print(f"artifacts:")
    print(f"  spv: {vs_spv}")
    print(f"  spv: {fs_spv}")
    print(f"  dumper: {dumper_bin}")
    print(f"  log: {dump_log}")
    print(f"  exec: {exec_dump_dir}")
    if (args.out_dir / "cache_extract").exists():
        print(f"  cache_extract: {args.out_dir / 'cache_extract'}")


if __name__ == "__main__":
    main()
