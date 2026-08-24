#!/usr/bin/env python3
"""Build and run a temporary, instrumented Mesa ANV HelioC capture.

This utility is intentionally separate from bake.py.  It copies an exact Mesa
revision into --work-dir, patches only that copy, selects its Intel ICD through
VK_DRIVER_FILES, and feeds the usual hosted HelioC dumper.  A successful run is
compiler/state evidence only: bake.py remains the sole gate for HELIOA output.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


TRUEOS = Path(__file__).resolve().parents[2]
DEFAULT_MESA = TRUEOS.parent / "bak/reference/mesa"
MESA_REVISION = "6fb261147bbb4cc488ea9f16fb3b6fe02105332e"
PATCH = Path(__file__).with_name("mesa-helioc-capture-6fb2611.patch")
FOLLOWUP_PATCH = Path(__file__).with_name("mesa-helioc-capture-followup-6fb2611.patch")
IDENTITY_PATCH = Path(__file__).with_name("mesa-helioc-capture-identity-followup-6fb2611.patch")
SHADER_SERIALIZE_PATCH = Path(__file__).with_name("mesa-helioc-capture-shader-serialize-6fb2611.patch")
REQUIRED_TOOLS = ("meson", "ninja", "bison", "flex", "pkg-config", "tar")
REQUIRED_PKGCONFIG = ("expat", "libdrm", "libzstd", "vulkan")
BOOTSTRAP_PACKAGES = (
    "meson", "ninja-build", "bison", "flex", "libfl-dev",
    "libdrm-dev", "libdrm2", "libdrm-intel1", "libdrm-radeon1",
    "libdrm-nouveau2", "libdrm-amdgpu1", "libpciaccess-dev",
    "libpciaccess0", "libexpat1-dev", "libexpat1", "libzstd-dev", "libzstd1", "libvulkan-dev",
    "libvulkan1", "glslang-tools", "spirv-tools", "spirv-tools-dev",
    "spirv-tools-headers", "libclc-21", "libclc-21-dev",
    "libllvmspirvlib-21-dev", "libllvmspirvlib21.1", "llvm-spirv-21",
    "libclang-cpp21", "libclang-cpp21-dev", "libclang-21-dev",
    "python3-mako", "python3-markupsafe",
)


def run(command: list[str], *, cwd: Path | None = None, env: dict[str, str] | None = None) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def output(command: list[str], *, cwd: Path) -> str:
    return subprocess.check_output(command, cwd=cwd, text=True).strip()


def require_host_prerequisites(env: dict[str, str]) -> None:
    missing_tools = [tool for tool in REQUIRED_TOOLS if shutil.which(tool, path=env["PATH"]) is None]
    if missing_tools:
        raise SystemExit(
            "instrumented ANV build blocked: required host tool(s) missing: "
            + ", ".join(missing_tools)
            + ". Install nothing system-wide; use a temporary tool prefix or --bootstrap-tools."
        )
    missing_pc = [package for package in REQUIRED_PKGCONFIG
                  if subprocess.run(["pkg-config", "--exists", package], env=env).returncode]
    if missing_pc:
        raise SystemExit(
            "instrumented ANV build blocked: pkg-config development dependency missing: "
            + ", ".join(missing_pc)
            + ". A source build of the matched ICD requires headers as well as temporary executables."
        )


def temporary_tool_prefix(work: Path, env: dict[str, str]) -> None:
    """Extract build tools, headers, and libraries below work; install nothing."""
    if shutil.which("apt") is None or shutil.which("dpkg-deb") is None:
        raise SystemExit("--bootstrap-tools requires apt and dpkg-deb; neither system packages nor Mesa were changed")
    debs = work / "bootstrap-debs"
    prefix = work / "bootstrap-root"
    debs.mkdir(parents=True, exist_ok=True)
    run(["apt", "download", *BOOTSTRAP_PACKAGES], cwd=debs)
    for deb in sorted(debs.glob("*.deb")):
        run(["dpkg-deb", "-x", str(deb), str(prefix)])
    tool_dirs = [prefix / "usr/bin", prefix / "bin"]
    env["PATH"] = os.pathsep.join(str(path) for path in tool_dirs if path.exists()) + os.pathsep + env["PATH"]
    python_dirs = sorted((prefix / "usr/lib").glob("python3*/dist-packages"))
    if (prefix / "usr/lib/python3/dist-packages").is_dir():
        python_dirs.append(prefix / "usr/lib/python3/dist-packages")
    if python_dirs:
        env["PYTHONPATH"] = os.pathsep.join(map(str, python_dirs)) + os.pathsep + env.get("PYTHONPATH", "")
    pkgconfig_dirs = [
        prefix / "usr/lib/x86_64-linux-gnu/pkgconfig",
        prefix / "usr/lib/pkgconfig",
        prefix / "usr/share/pkgconfig",
    ]
    env["PKG_CONFIG_LIBDIR"] = os.pathsep.join(
        str(path) for path in pkgconfig_dirs if path.is_dir()
    )
    env["PKG_CONFIG_SYSROOT_DIR"] = str(prefix)
    env["CMAKE_PREFIX_PATH"] = os.pathsep.join(
        (str(prefix / "usr"), "/usr/lib/llvm-21")
    ) + os.pathsep + env.get("CMAKE_PREFIX_PATH", "")
    library_dirs = [
        prefix / "usr/lib/x86_64-linux-gnu",
        prefix / "usr/lib/llvm-21/lib",
        prefix / "usr/lib",
    ]
    library_path = os.pathsep.join(str(path) for path in library_dirs if path.is_dir())
    if library_path:
        env["LIBRARY_PATH"] = library_path + os.pathsep + env.get("LIBRARY_PATH", "")
        env["LD_LIBRARY_PATH"] = library_path + os.pathsep + env.get("LD_LIBRARY_PATH", "")
    include_dirs = [
        prefix / "usr/lib/llvm-21/include",
        prefix / "usr/include",
    ]
    include_path = os.pathsep.join(str(path) for path in include_dirs if path.is_dir())
    if include_path:
        env["CPATH"] = include_path + os.pathsep + env.get("CPATH", "")
    bison_data = prefix / "usr/share/bison"
    if bison_data.is_dir():
        env["BISON_PKGDATADIR"] = str(bison_data)
    print(f"temporary build-tool prefix: {prefix}")


def copy_and_patch(source: Path, work: Path) -> Path:
    if not source.is_dir():
        raise SystemExit(f"Mesa source is absent: {source}")
    if output(["git", "rev-parse", "HEAD"], cwd=source) != MESA_REVISION:
        raise SystemExit(f"Mesa source must be exactly {MESA_REVISION}")
    if output(["git", "status", "--porcelain"], cwd=source):
        raise SystemExit("Mesa reference source is dirty; refusing to copy an unpinned tree")
    destination = work / "mesa-src"
    if destination.exists():
        raise SystemExit(f"temporary Mesa source already exists: {destination}; choose a new --work-dir")
    # The pinned reference uses sparse checkout. Archive the exact Git tree so
    # required directories such as src/drm-shim are present without changing
    # the reference checkout's sparse specification.
    source_archive = work / "mesa-source.tar"
    run(["git", "archive", "--format=tar", "--output", str(source_archive), MESA_REVISION], cwd=source)
    destination.mkdir()
    run(["tar", "-xf", str(source_archive), "-C", str(destination)])
    source_archive.unlink()
    run(["git", "init", "-q"], cwd=destination)
    run(["git", "add", "-A"], cwd=destination)
    run(["git", "-c", "user.name=trueos", "-c", "user.email=trueos@localhost", "commit", "-qm", "mesa-base"], cwd=destination)
    for patch in (PATCH, FOLLOWUP_PATCH, IDENTITY_PATCH, SHADER_SERIALIZE_PATCH):
        run(["git", "apply", "--check", str(patch)], cwd=destination)
        run(["git", "apply", str(patch)], cwd=destination)
    return destination


def locate_icd(build: Path, work: Path) -> Path:
    matches = sorted(build.rglob("intel_icd.*.json"))
    if len(matches) != 1:
        raise SystemExit(f"instrumented ANV build produced {len(matches)} Intel ICD manifests, expected one")
    manifest = json.loads(matches[0].read_text())
    library = Path(manifest["ICD"]["library_path"])
    # Meson writes its configured install path into the build-tree manifest.
    # Never accept that path (or an accidentally installed system ICD): bind
    # the capture manifest to the unique library produced by this exact build.
    candidates = list(build.rglob(library.name))
    if len(candidates) != 1:
        raise SystemExit(f"cannot resolve unique matched Intel ICD library {library.name}")
    library = candidates[0].resolve()
    manifest["ICD"]["library_path"] = str(library)
    selected = work / "instrumented-intel-icd.json"
    selected.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    return selected


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mesa-src", type=Path, default=DEFAULT_MESA)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--bootstrap-tools", action="store_true",
                        help="extract the pinned Mesa build dependencies beneath work-dir using apt download")
    args = parser.parse_args()
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    if args.bootstrap_tools:
        temporary_tool_prefix(work, env)
    require_host_prerequisites(env)
    source = copy_and_patch(args.mesa_src.resolve(), work)
    build = work / "mesa-build"
    run([
        "meson", "setup", str(build), str(source),
        "-Dvulkan-drivers=intel", "-Dgallium-drivers=[]",
        "-Dplatforms=[]", "-Dopengl=false", "-Dgles1=disabled", "-Dgles2=disabled",
        "-Dglx=disabled", "-Degl=disabled", "-Dgbm=disabled", "-Dllvm=enabled",
        "-Dxmlconfig=disabled", "-Dshader-cache=disabled", "-Dvalgrind=disabled",
        "-Dlibunwind=disabled", "-Dzstd=enabled", "-Dzlib=disabled",
        # `drm-shim` builds the common shim support; `intel` enters the Intel
        # tools subdirectory where libintel_noop_drm_shim.so is defined.
        "-Dtools=intel,drm-shim", "-Dbuildtype=release",
    ], env=env)
    # Building the entire `intel` tools family pulls unrelated diagnostic
    # binaries into this capture (and their optional zlib link surface).  The
    # exact ICD and no-op shim targets contain every dependency we need.
    run([
        "ninja", "-C", str(build),
        "src/intel/vulkan/libvulkan_intel.so",
        "src/intel/vulkan/intel_icd.x86_64.json",
        "src/intel/tools/libintel_noop_drm_shim.so",
    ], env=env)
    icd = locate_icd(build, work)
    shim_matches = sorted(build.rglob("libintel_noop_drm_shim.so"))
    if len(shim_matches) != 1:
        raise SystemExit("instrumented ANV build did not produce the no-op DRM shim")
    capture = work / "capture"
    capture.mkdir(exist_ok=True)
    env.update({
        "VK_DRIVER_FILES": str(icd),
        "LD_PRELOAD": str(shim_matches[0]),
        "INTEL_STUB_GPU_DEVICE_ID": "4680",
        # PCI revision is part of the HelioC target identity.  KMD revision
        # is separate Mesa state and stays explicitly zero for this shim.
        "TRUEOS_HELIOC_STUB_PCI_REVISION": "0x0c",
        "TRUEOS_HELIOC_STUB_KMD_REVISION": "0",
        "TRUEOS_HELIOC_ANV_DUMP_DIR": str(capture),
        "TRUEOS_HELIOC_CAPTURE_IDENTITY": "noop-drm-shim:8086:4680:r0c",
    })
    command = [sys.executable, str(Path(__file__).with_name("bake.py")), "--helioc",
               "--device-id", "0x4680", "--work-dir", str(capture),
               "--out", str(capture / "must-not-exist.helio")]
    result = subprocess.run(command, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    (capture / "instrumented-helioc.log").write_text(result.stdout)
    print(result.stdout, end="")
    output_path = capture / "must-not-exist.helio"
    if result.returncode == 0:
        raise SystemExit("fail-closed HelioC bakery unexpectedly emitted success")
    if output_path.exists():
        raise SystemExit("fail-closed HelioC bakery left a HELIOA output behind")
    for message in (
        "symbolic address/ownership map for the command, binding-table, sampler, indirect-descriptor, and render-target state",
        "physical UHD 770 PCI r0c retirement/ISA proof (the explicit no-op shim proves compiler identity, not execution on target silicon)",
        "HelioC preflight stopped; no HELIOA emitted: missing capture datum(s):",
    ):
        if message not in result.stdout:
            raise SystemExit(f"instrumented HelioC capture omitted expected fail-closed message: {message}")
    if not any(capture.rglob("helioc-anv-*.bin")):
        raise SystemExit("instrumented ANV ran but produced no state trace")
    metadata_files = list(capture.glob("helioc-capture-metadata.json"))
    if len(metadata_files) != 1:
        raise SystemExit("instrumented ANV ran but produced no unique capture metadata")
    metadata = json.loads(metadata_files[0].read_text())
    if metadata.get("instrumented_identity") != "noop-drm-shim:8086:4680:r0c":
        raise SystemExit("capture metadata did not authenticate the explicit no-op shim identity")
    print("instrumented capture completed; package emission remains intentionally refused")


if __name__ == "__main__":
    main()
