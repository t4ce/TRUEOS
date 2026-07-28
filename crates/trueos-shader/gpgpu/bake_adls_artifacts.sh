#!/usr/bin/env bash
set -euo pipefail

# Compatibility entry point. Every maintained Intel GPGPU artifact is now
# published through the repository's pinned C++-for-OpenCL bakery.

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${script_dir}/../../.." && pwd)"

exec make -C "${trueos_root}" intel-gpu-bake-cpp-artifacts "$@"
