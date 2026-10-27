#!/usr/bin/env bash
# Build the probe and boot it in QEMU under AAVMF (openSUSE qemu-uefi-aarch64).
#
#   scripts/run-qemu.sh [tcg|kvm]    default: tcg
#
#   tcg  enters at EL2 like the O6 (virtualization=on, -cpu max)
#   kvm  runs on the O6's own CPU cores at EL1, with MTE passed through
#        (-cpu host -accel kvm, mte=on)
#
# The probe boots as \EFI\BOOT\BOOTAA64.EFI from target/esp, because this
# firmware has no UEFI Shell. QEMU stops by itself once the probe prints
# "Probe finished."; quit earlier with Ctrl+A, then X. The whole serial output
# is also kept in target/serial.log.
set -euo pipefail

mode=${1:-tcg}
root=$(cd "$(dirname "$0")/.." && pwd)

case $mode in
  tcg) machine=(-M virt,gic-version=3,virtualization=on -cpu max) ;;
  kvm) machine=(-M virt,gic-version=3,mte=on -cpu host -accel kvm) ;;
  *) echo "usage: $0 [tcg|kvm]" >&2; exit 2 ;;
esac

cargo build --manifest-path "$root/Cargo.toml" -p probe --target aarch64-unknown-uefi

esp=$root/target/esp
mkdir -p "$esp/EFI/BOOT"
cp "$root/target/aarch64-unknown-uefi/debug/probe.efi" "$esp/EFI/BOOT/BOOTAA64.EFI"

log=$root/target/serial.log
pidfile=$root/target/qemu.pid
rm -f "$pidfile"
: > "$log"

# When the probe returns, the firmware's boot manager moves on to network
# boot and then waits in a boot-device menu, so stop QEMU as soon as the
# probe says it is done.
(
  until grep -aq 'Probe finished\.' "$log"; do sleep 0.5; done
  kill "$(cat "$pidfile")" 2>/dev/null || true
) &
watcher=$!
trap 'kill $watcher 2>/dev/null || true' EXIT

qemu-system-aarch64 "${machine[@]}" -m 1G \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/qemu/aavmf-aarch64-code.bin \
  -drive if=pflash,format=raw,snapshot=on,file=/usr/share/qemu/aavmf-aarch64-vars.bin \
  -drive if=virtio,format=raw,readonly=on,file=fat:"$esp" \
  -nographic -pidfile "$pidfile" | tee "$log"
