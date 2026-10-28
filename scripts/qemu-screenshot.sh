#!/usr/bin/env bash
# Save what is on the screen of the QEMU that `cargo kernel` is running.
#
#   scripts/qemu-screenshot.sh [file.png]    default: target/kernel-qemu/screen.png
#
# Talks to QEMU's control socket (QMP, JSON commands). Needs socat.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
sock=$root/target/kernel-qemu/qmp.sock
out=$(realpath -m "${1:-$root/target/kernel-qemu/screen.png}")
[[ -S $sock ]] || { echo "no QEMU running (missing $sock)" >&2; exit 1; }

rm -f "$out"
printf '%s\n' '{"execute":"qmp_capabilities"}' \
  "{\"execute\":\"screendump\",\"arguments\":{\"filename\":\"$out\",\"format\":\"png\"}}" \
  | socat -t 2 - UNIX-CONNECT:"$sock" >/dev/null
[[ -s $out ]] || { echo "QEMU did not save a screenshot" >&2; exit 1; }
echo "$out"
