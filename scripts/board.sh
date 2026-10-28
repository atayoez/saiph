#!/usr/bin/env bash
# Run Saiph on the O6 itself (bare metal) via systemd-boot.
#
#   scripts/board.sh kernel    build Saiph, copy it with Limine to the ESP and boot
#                              it once on the next boot. It shows its output on
#                              the screen, then reboots into Linux after 60 s.
#   scripts/board.sh remove    delete Saiph and its menu entry from the ESP (and
#                              the old hardware probe's, if it is still there)
#
# The ESP (/boot/efi) is root-only, so each subcommand does its ESP work in
# a single pkexec call (one password dialog). The build runs as you.
#
# How the one-time boot works: systemd-boot builds its menu from files in
# loader/entries/, and `bootctl set-oneshot` stores the EFI variable
# LoaderEntryOneShot. On the next boot systemd-boot starts that entry and
# deletes the variable, so it happens once.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
# Cargo reads .cargo/config.toml (build-std, branch protection) from the
# directory it runs in, not from --manifest-path, so build from the root.
cd "$root"
esp=/boot/efi
# Where the hardware probe installed itself (the probe is no longer in the
# repository; see the git tag probe-final). `remove` cleans it up.
probe_entry=orion-probe.conf
kernel_elf=$root/target/aarch64-unknown-none-softfloat/debug/kernel
kernel_entry=saiph.conf
limine_efi=/usr/share/limine/BOOTAA64.EFI
oneshot_var=/sys/firmware/efi/efivars/LoaderEntryOneShot-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f

# Print an EFI string variable (4 attribute bytes, then UTF-16LE text).
efi_string() {
  [[ -e $1 ]] && tail -c +5 "$1" | iconv -f UTF-16LE -t UTF-8 | tr -d '\0'
}


case ${1:-} in
  kernel)
    cargo build -p kernel --target aarch64-unknown-none-softfloat
    [[ -f $limine_efi ]] || { echo "missing $limine_efi: pkexec zypper install limine" >&2; exit 1; }
    # Limine reads limine.conf from its own folder. The board's copy adds a
    # command line telling the kernel to reboot 60 s after it finishes.
    conf=$(mktemp)
    trap 'rm -f "$conf"' EXIT
    { cat "$root/kernel/limine.conf"; printf '    cmdline: reboot-after=60\n'; } > "$conf"
    pkexec sh -c '
      set -e
      mkdir -p "$1/EFI/saiph"
      cp "$2" "$1/EFI/saiph/limine.efi"
      cp "$3" "$1/EFI/saiph/limine.conf"
      cp "$4" "$1/EFI/saiph/kernel"
      printf "title   Saiph\nefi     /EFI/saiph/limine.efi\n" > "$1/loader/entries/$5"
      bootctl set-oneshot "$5"
    ' sh "$esp" "$limine_efi" "$conf" "$kernel_elf" "$kernel_entry"
    if [[ $(efi_string "$oneshot_var") == "$kernel_entry" ]]; then
      echo "Armed: the next boot runs Saiph once, then shows the systemd-boot menu."
    else
      echo "Warning: LoaderEntryOneShot is not set to $kernel_entry; the next boot may skip Saiph." >&2
    fi
    read -r -p "Reboot now? [y/N] " answer
    if [[ $answer == [yY]* ]]; then
      systemctl reboot
    fi
    ;;

  remove)
    pkexec sh -c 'rm -rf "$1/EFI/orion-probe" "$1/loader/entries/$2" "$1/EFI/saiph" "$1/loader/entries/$3"' \
      sh "$esp" "$probe_entry" "$kernel_entry"
    # A one-time boot that still points at a deleted entry is pointed back
    # at the normal default entry.
    oneshot=$(efi_string "$oneshot_var" || true)
    if [[ $oneshot == "$probe_entry" || $oneshot == "$kernel_entry" ]]; then
      pkexec bootctl set-oneshot @default
    fi
    echo "Removed Saiph (and the old probe, if it was there) from the ESP."
    ;;

  *)
    echo "usage: $0 kernel | remove" >&2
    exit 2
    ;;
esac
