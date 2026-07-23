#!/usr/bin/env bash
set -euo pipefail

EXP_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
VENV_DIR="$EXP_ROOT/.venv"
RUNTIME_DIR="$EXP_ROOT/.runtime"
RIFE_DIR="$RUNTIME_DIR/Practical-RIFE"
RIFE_COMMIT="17d8c7a1005b37f4c97bfee04e316aaec7fdc536"
MODEL_URL="https://drive.usercontent.google.com/download?id=1ZKjcbmt1hypiFprJPIKW0Tt0lr_2i7bg&export=download&confirm=t"
MODEL_SHA256="e63d481b7ae5d4a4e6ad7ac5b410ff78f3bf7be3b51b2e38ca8152747abde5b4"
MODEL_ARCHIVE="$RUNTIME_DIR/rife-v4.25.zip"

mkdir -p "$RUNTIME_DIR"

if [[ ! -x "$VENV_DIR/bin/python" ]]; then
    python3 -m venv "$VENV_DIR"
fi

"$VENV_DIR/bin/python" -m pip install --upgrade pip
"$VENV_DIR/bin/python" -m pip install --editable "$EXP_ROOT"

if [[ ! -d "$RIFE_DIR/.git" ]]; then
    git clone https://github.com/hzwer/Practical-RIFE.git "$RIFE_DIR"
fi

if [[ -n "$(git -C "$RIFE_DIR" status --porcelain)" ]]; then
    echo "Refusing to replace a modified Practical-RIFE runtime: $RIFE_DIR" >&2
    exit 1
fi

git -C "$RIFE_DIR" fetch --depth 1 origin "$RIFE_COMMIT"
git -C "$RIFE_DIR" checkout --detach "$RIFE_COMMIT"

if [[ ! -f "$RIFE_DIR/train_log/flownet.pkl" ]]; then
    archive_tmp="$MODEL_ARCHIVE.partial"
    curl --location --fail --retry 3 "$MODEL_URL" --output "$archive_tmp"
    actual_sha=$(sha256sum "$archive_tmp" | awk '{print $1}')
    if [[ "$actual_sha" != "$MODEL_SHA256" ]]; then
        echo "RIFE model checksum mismatch: expected $MODEL_SHA256, got $actual_sha" >&2
        exit 1
    fi
    mv "$archive_tmp" "$MODEL_ARCHIVE"
    "$VENV_DIR/bin/python" -m zipfile -e "$MODEL_ARCHIVE" "$RIFE_DIR"
fi

"$VENV_DIR/bin/lilly-exp" doctor

