#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
model_name=LFM2.5-350M-Q8_0.gguf
model_revision=bb7ee58b243e4cede04187e323e760b04f8a0091
model_sha256=be036a757295e550098b85e13f6af2735d0fa73b41e1156a40c7d8e8e32a5766
model_base_url="https://huggingface.co/LiquidAI/LFM2.5-350M-GGUF/resolve/$model_revision"

runtime_version=b10075
runtime_commit=76f46ad29d61fd8c1401e8221842934bf62a6064
runtime_name=llama-b10075-bin-ubuntu-x64.tar.gz
runtime_sha256=fb4bb65ef3d2c7006b420a9ff786e802f6780ac318632942e56551f5f9a2e98a
runtime_url="https://github.com/ggml-org/llama.cpp/releases/download/$runtime_version/$runtime_name"

curl -fL --retry 4 --retry-delay 2 --continue-at - \
    --output "$script_dir/$model_name" "$model_base_url/$model_name"
printf '%s  %s\n' "$model_sha256" "$script_dir/$model_name" | sha256sum --check

curl -fL --retry 4 --output "$script_dir/LICENSE.LFM-1.0" "$model_base_url/LICENSE"
curl -fL --retry 4 --output "$script_dir/UPSTREAM_README.md" "$model_base_url/README.md"

download_tmp=$(mktemp -d)
trap 'rm -rf -- "$download_tmp"' EXIT
curl -fL --retry 4 --retry-delay 2 \
    --output "$download_tmp/$runtime_name" "$runtime_url"
printf '%s  %s\n' "$runtime_sha256" "$download_tmp/$runtime_name" | sha256sum --check

mkdir -p "$script_dir/runtime"
tar -xzf "$download_tmp/$runtime_name" -C "$script_dir/runtime"

runtime_source="$script_dir/runtime/llama-b10075-src"
if [[ ! -d "$runtime_source/.git" ]]; then
    git clone --filter=blob:none --no-checkout \
        https://github.com/ggml-org/llama.cpp.git "$runtime_source"
    git -C "$runtime_source" checkout --detach "$runtime_commit"
fi
actual_runtime_commit=$(git -C "$runtime_source" rev-parse HEAD)
if [[ "$actual_runtime_commit" != "$runtime_commit" ]]; then
    printf 'llama.cpp source is %s, expected %s.\n' \
        "$actual_runtime_commit" "$runtime_commit" >&2
    exit 1
fi

printf 'Installed and verified %s with llama.cpp %s.\n' "$model_name" "$runtime_version"
