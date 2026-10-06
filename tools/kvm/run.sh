#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
build="$root/bld/kvm"
mkdir -p "$build"
source="$root/tools/kvm/launch.c"
binary="$build/trueos-kvm"
if [ ! -x "$binary" ] || [ "$source" -nt "$binary" ]; then
    temporary=$(mktemp "$build/trueos-kvm.XXXXXX")
    trap 'rm -f "$temporary"' EXIT HUP INT TERM
    "${CC:-cc}" -std=c11 -O2 -g -Wall -Wextra -Werror "$source" -o "$temporary"
    mv -f "$temporary" "$binary"
    trap - EXIT HUP INT TERM
fi
exec "$binary" "$@"
