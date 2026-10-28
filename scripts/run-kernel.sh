#!/usr/bin/env bash
# Cargo runner for the kernel (see .cargo/config.toml):
#
#   cargo kernel             boot it in QEMU TCG (enters at EL2, like the O6)
#   cargo kernel -- kvm      boot it under KVM on the O6's own cores (EL1)
#   cargo kernel -- tcg fault-test
#                            anything after the mode becomes the kernel
#                            command line (here: crash on purpose)
#
# Cargo passes the built kernel ELF as $1. This script builds a FAT disk
# image like a USB stick: Limine (from the openSUSE limine package) as
# \EFI\BOOT\BOOTAA64.EFI, its config next to it, and the kernel at
# \EFI\saiph\kernel. The kernel powers QEMU off when it is done; quit
# earlier with Ctrl+A, then X. Needs mtools.
#
# The machine has a screen (ramfb) that is not shown anywhere; while QEMU
# runs, scripts/qemu-screenshot.sh saves what is on it as a PNG.
set -euo pipefail

kernel=$1
mode=${2:-tcg}
cmdline=${*:3}
root=$(cd "$(dirname "$0")/.." && pwd)
limine_efi=/usr/share/limine/BOOTAA64.EFI

case $mode in
  tcg) machine=(-M virt,gic-version=3,virtualization=on -cpu max) ;;
  kvm) machine=(-M virt,gic-version=3 -cpu host -accel kvm) ;;
  *) echo "usage: cargo kernel [-- tcg|kvm]" >&2; exit 2 ;;
esac
[[ -f $limine_efi ]] || { echo "missing $limine_efi: pkexec zypper install limine" >&2; exit 1; }

work=$root/target/kernel-qemu
img=$work/esp.img
rm -rf "$work"
mkdir -p "$work"
/usr/sbin/mkfs.fat -C -F 32 -n SAIPH "$img" 65536 >/dev/null
mmd -i "$img" ::/EFI ::/EFI/BOOT ::/EFI/saiph
mcopy -i "$img" "$limine_efi" ::/EFI/BOOT/BOOTAA64.EFI
# The config, with a kernel command line added if one was given.
cp "$root/kernel/limine.conf" "$work/limine.conf"
if [[ -n $cmdline ]]; then
  printf '    cmdline: %s\n' "$cmdline" >> "$work/limine.conf"
fi
mcopy -i "$img" "$work/limine.conf" ::/EFI/BOOT/limine.conf
mcopy -i "$img" "$kernel" ::/EFI/saiph/kernel

exec qemu-system-aarch64 "${machine[@]}" -m 1G \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/qemu/aavmf-aarch64-code.bin \
  -drive if=pflash,format=raw,snapshot=on,file=/usr/share/qemu/aavmf-aarch64-vars.bin \
  -drive if=virtio,format=raw,file="$img" \
  -device ramfb -qmp unix:"$work/qmp.sock",server=on,wait=off \
  -nographic -no-reboot
