#!/usr/bin/env bash

# Optional output dir via first arg or D env var; default to a user-writable path.
D="${D:-${1:-$HOME/igd-host}}"
mkdir -p "$D"

lspci -k -s 00:02.0 > "$D/pci.txt"
ls -la /sys/class/drm > "$D/drm-tree.txt"
for c in /sys/class/drm/card1-*; do
  [ -e "$c" ] || continue
  echo "== $(basename "$c") ==" >> "$D/connectors.txt"
  for f in status enabled dpms modes; do
    [ -f "$c/$f" ] && { echo "-- $f --" >> "$D/connectors.txt"; cat "$c/$f" >> "$D/connectors.txt"; }
  done
done
journalctl -b --no-pager | grep -i 'i915\|drm\|00:02.0' > "$D/journal-i915.txt"

echo "Wrote diagnostics to: $D"