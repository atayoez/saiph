#!/usr/bin/env bash
# Build the probe and boot it in QEMU under AAVMF (openSUSE qemu-uefi-aarch64).
#
#   scripts/run-qemu.sh [tcg|kvm]    default: tcg
#
#   tcg  enters at EL2 like the O6 (virtualization=on, -cpu max), with an
#        SMMUv3 and emulated MTE tag memory (mte=on)
#   kvm  runs on the O6's own CPU cores at EL1, with MTE passed through
#        (-cpu host -accel kvm, mte=on)
#
# Each run builds a fresh FAT disk image (a virtual USB stick) holding the
# probe as \EFI\BOOT\BOOTAA64.EFI, because this firmware has no UEFI Shell.
# Afterwards everything the probe wrote under \EFI\orion-probe is copied to
# target/qemu/out/ and report.txt is printed. QEMU stops by itself once the
# probe prints "Probe finished."; quit earlier with Ctrl+A, then X. The whole
# serial output is kept in target/qemu/serial.log. Needs mtools.
set -euo pipefail

mode=${1:-tcg}
root=$(cd "$(dirname "$0")/.." && pwd)

case $mode in
  tcg) machine=(-M virt,gic-version=3,virtualization=on,iommu=smmuv3,mte=on -cpu max -smp 4) ;;
  kvm) machine=(-M virt,gic-version=3,mte=on -cpu host -accel kvm -smp 4) ;;
  *) echo "usage: $0 [tcg|kvm]" >&2; exit 2 ;;
esac

cargo build --manifest-path "$root/Cargo.toml" -p probe --target aarch64-unknown-uefi

work=$root/target/qemu
img=$work/esp.img
log=$work/serial.log
pidfile=$work/qemu.pid
rm -rf "$work"
mkdir -p "$work"

# A 64 MiB FAT32 image without a partition table, like a USB stick
# formatted as one big FAT volume. mkfs.fat works on a plain file as a
# normal user; mtools copies files in and out without mounting anything.
/usr/sbin/mkfs.fat -C -F 32 -n PROBE "$img" 65536 >/dev/null
mmd -i "$img" ::/EFI ::/EFI/BOOT
mcopy -i "$img" "$root/target/aarch64-unknown-uefi/debug/probe.efi" ::/EFI/BOOT/BOOTAA64.EFI
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
  -drive if=virtio,format=raw,file="$img" \
  -nographic -pidfile "$pidfile" | tee "$log"

if mcopy -s -n -i "$img" ::/EFI/orion-probe "$work/out" 2>/dev/null; then
  printf '\n== %s ==\n' "$work/out/report.txt"
  cat "$work/out/report.txt"
fi
