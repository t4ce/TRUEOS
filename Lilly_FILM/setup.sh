#!/usr/bin/env bash
set -euo pipefail

EXP_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
RUNTIME_DIR="$EXP_ROOT/.runtime"
PYTHON_DIR="$RUNTIME_DIR/python"
PYTHON_ARCHIVE="$RUNTIME_DIR/python-3.9.25.tar.gz"
PYTHON_URL="https://github.com/astral-sh/python-build-standalone/releases/download/20251031/cpython-3.9.25%2B20251031-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"
PYTHON_SHA256="fe04e8b27bd69ca2144fc542428f8b9b5287b6a2e45516a4acfe2c2bc3102773"
VENV_DIR="$EXP_ROOT/.venv"
FILM_SOURCE="$RUNTIME_DIR/frame-interpolation"
FILM_COMMIT="69f8708f08e62c2edf46a27616a4bfcf083e2076"
MODEL_DIR="$RUNTIME_DIR/models/film_net/Style/saved_model"

download_checked() {
    local file_id=$1
    local expected_sha=$2
    local destination=$3
    local partial="${destination}.partial"

    if [[ -f "$destination" ]]; then
        local existing_sha
        existing_sha=$(sha256sum "$destination" | awk '{print $1}')
        if [[ "$existing_sha" == "$expected_sha" ]]; then
            return
        fi
        echo "Checksum mismatch for existing file: $destination" >&2
        exit 1
    fi

    mkdir -p "$(dirname -- "$destination")"
    curl --location --fail --retry 3 \
        "https://drive.usercontent.google.com/download?id=${file_id}&export=download&confirm=t" \
        --output "$partial"
    local actual_sha
    actual_sha=$(sha256sum "$partial" | awk '{print $1}')
    if [[ "$actual_sha" != "$expected_sha" ]]; then
        echo "FILM model checksum mismatch for $destination" >&2
        echo "Expected $expected_sha, got $actual_sha" >&2
        exit 1
    fi
    mv "$partial" "$destination"
}

mkdir -p "$RUNTIME_DIR"

if [[ ! -x "$PYTHON_DIR/bin/python3.9" ]]; then
    curl --location --fail --retry 3 "$PYTHON_URL" --output "${PYTHON_ARCHIVE}.partial"
    printf '%s  %s\n' "$PYTHON_SHA256" "${PYTHON_ARCHIVE}.partial" |
        sha256sum --check
    mv "${PYTHON_ARCHIVE}.partial" "$PYTHON_ARCHIVE"
    mkdir -p "$PYTHON_DIR"
    tar -xzf "$PYTHON_ARCHIVE" --strip-components=1 -C "$PYTHON_DIR"
fi

if [[ ! -x "$VENV_DIR/bin/python" ]]; then
    "$PYTHON_DIR/bin/python3.9" -m venv "$VENV_DIR"
fi

"$VENV_DIR/bin/python" -m pip install --upgrade "pip==25.3"
"$VENV_DIR/bin/python" -m pip install --requirement "$EXP_ROOT/requirements.lock"
"$VENV_DIR/bin/python" -m pip install --editable "$EXP_ROOT" --no-deps

if [[ ! -d "$FILM_SOURCE/.git" ]]; then
    git clone https://github.com/google-research/frame-interpolation.git "$FILM_SOURCE"
fi
if [[ -n "$(git -C "$FILM_SOURCE" status --porcelain)" ]]; then
    echo "Refusing to replace a modified FILM source tree: $FILM_SOURCE" >&2
    exit 1
fi
git -C "$FILM_SOURCE" fetch --depth 1 origin "$FILM_COMMIT"
git -C "$FILM_SOURCE" checkout --detach "$FILM_COMMIT"

download_checked \
    "1_oyM-LBAK9o7-bNWf1jG8VvBYeqpmSUr" \
    "8c47323923bc4826b730dd882c8c7700761aa3ac03b2c8180d3ffc82d18111f9" \
    "$MODEL_DIR/variables/variables.data-00000-of-00001"
download_checked \
    "1ceC2kbJs3U1dMMrp4hNIpoHRFxO33SFC" \
    "d19bb117eb9abe6121b5711649bb7d5d1c4fe1912b9deabbdafa2be3f5a273e5" \
    "$MODEL_DIR/variables/variables.index"
download_checked \
    "1dT85Z-HyYsiUgIQbOgYFjwWPOw8en1RC" \
    "0291f451e35e62a042fa49a1341af1dc8a94632188a24a16b71a9516e9fc6853" \
    "$MODEL_DIR/keras_metadata.pb"
download_checked \
    "1nfi15im3LQvCx84ZRiNcfMuodDkRL_Ei" \
    "4df311e80e9a7282b362a7e93bef22a1ce4f84e7cdeda01f246894545eaaf985" \
    "$MODEL_DIR/saved_model.pb"

"$VENV_DIR/bin/lilly-film" doctor

