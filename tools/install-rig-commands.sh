#!/usr/bin/env bash
# Cargo discovers external cargo-* commands on PATH.
set -euo pipefail
root=$(cd -- "$(dirname -- "$0")/.." && pwd)
bin="${CARGO_HOME:-$HOME/.cargo}/bin"
mkdir -p "$bin"
for name in rig rigreal; do
    target="$bin/cargo-$name"
    if [[ -e $target || -L $target ]]; then
        if [[ $(readlink -f -- "$target") != "$root/tools/rig-build.sh" ]]; then
            echo "Refusing to replace existing command: $target" >&2
            exit 1
        fi
    else
        ln -s "$root/tools/rig-build.sh" "$target"
    fi
done
# Migrate the old command only when this checkout installed its symlink.
legacy="$bin/cargo-rigdev"
if [[ -L $legacy && $(readlink -f -- "$legacy") == "$root/tools/rig-build.sh" ]]; then
    rm -- "$legacy"
fi
echo "Installed cargo rig and cargo rigreal in $bin (must be on PATH)."
