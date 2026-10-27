#!/usr/bin/env bash
# Build the probe and boot it in QEMU under AAVMF (openSUSE qemu-uefi-aarch64).
#
#   scripts/run-qemu.sh [tcg|kvm]    default: tcg
#
#   tcg  enters at EL2 like the O6 (virtualization=on, -cpu max)
#   kvm  runs on the O6's own CPU cores at EL1 (-cpu host -accel kvm)
#
# The probe boots as \EFI\BOOT\BOOTAA64.EFI from target/esp, because this
# firmware has no UEFI Shell. Quit QEMU with Ctrl+A, then X.
set -euo pipefail

mode=${1:-tcg}
root=$(cd "$(dirname "$0")/.." && pwd)

case $mode in
  tcg) machine=(-M virt,gic-version=3,virtualization=on -cpu max) ;;
  kvm) machine=(-M virt,gic-version=3 -cpu host -accel kvm) ;;
  *) echo "usage: $0 [tcg|kvm]" >&2; exit 2 ;;
esac

cargo build --manifest-path "$root/Cargo.toml" -p probe --target aarch64-unknown-uefi

esp=$root/target/esp
mkdir -p "$esp/EFI/BOOT"
cp "$root/target/aarch64-unknown-uefi/debug/probe.efi" "$esp/EFI/BOOT/BOOTAA64.EFI"

exec qemu-system-aarch64 "${machine[@]}" -m 1G \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/qemu/aavmf-aarch64-code.bin \
  -drive if=pflash,format=raw,snapshot=on,file=/usr/share/qemu/aavmf-aarch64-vars.bin \
  -drive if=virtio,format=raw,readonly=on,file=fat:"$esp" \
  -nographic
