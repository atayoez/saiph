#!/usr/bin/env bash
# Run Saiph or the hardware probe on the O6 itself (bare metal) via systemd-boot.
#
#   scripts/board.sh kernel    build Saiph, copy it with Limine to the ESP and boot
#                              it once on the next boot. It shows its output on
#                              the screen, then reboots into Linux after 60 s.
#   scripts/board.sh probe     build the probe, copy it to the ESP, run it once
#   scripts/board.sh report    show the probe's bare-metal report; commit it and
#                              the ACPI dump to reports/board-<time the probe ran>/
#   scripts/board.sh remove    delete Saiph, the probe and their menu entries
#                              from the ESP
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
esp=/boot/efi
probe_efi=$root/target/aarch64-unknown-uefi/debug/probe.efi
probe_entry=orion-probe.conf
kernel_elf=$root/target/aarch64-unknown-none-softfloat/debug/kernel
kernel_entry=saiph.conf
limine_efi=/usr/share/limine/BOOTAA64.EFI
oneshot_var=/sys/firmware/efi/efivars/LoaderEntryOneShot-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f

# Print an EFI string variable (4 attribute bytes, then UTF-16LE text).
efi_string() {
  [[ -e $1 ]] && tail -c +5 "$1" | iconv -f UTF-16LE -t UTF-8 | tr -d '\0'
}

# Check that the one-time boot points at entry $1 (named $2), then offer
# to reboot.
armed_then_reboot() {
  if [[ $(efi_string "$oneshot_var") == "$1" ]]; then
    echo "Armed: the next boot runs $2 once, then shows the systemd-boot menu."
  else
    echo "Warning: LoaderEntryOneShot is not set to $1; the next boot may skip $2." >&2
  fi
  read -r -p "Reboot now? [y/N] " answer
  if [[ $answer == [yY]* ]]; then
    systemctl reboot
  fi
}

case ${1:-} in
  kernel)
    cargo build --manifest-path "$root/Cargo.toml" -p kernel --target aarch64-unknown-none-softfloat
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
    armed_then_reboot "$kernel_entry" Saiph
    ;;

  probe)
    cargo build --manifest-path "$root/Cargo.toml" -p probe --target aarch64-unknown-uefi
    # Copy the probe, drop the previous run's report (so `report` can never
    # show a stale one), write the menu entry and arm the one-time boot.
    pkexec sh -c '
      set -e
      mkdir -p "$1/EFI/orion-probe"
      cp "$2" "$1/EFI/orion-probe/probe.efi"
      rm -rf "$1/EFI/orion-probe/report.txt" "$1/EFI/orion-probe/acpi"
      printf "title   Orion probe\nefi     /EFI/orion-probe/probe.efi\n" > "$1/loader/entries/$3"
      bootctl set-oneshot "$3"
    ' sh "$esp" "$probe_efi" "$probe_entry"
    armed_then_reboot "$probe_entry" "the probe"
    ;;

  report)
    # One root call: pack report.txt and the acpi folder into a tar stream;
    # tar keeps each file's time, so report.txt's time says when the probe ran.
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    if ! pkexec sh -c 'cd "$1/EFI/orion-probe" && [ -f report.txt ] && tar -cf - report.txt $(ls -d acpi 2>/dev/null)' sh "$esp" | tar -xf - -C "$tmp"; then
      echo "No report on the ESP. Run 'scripts/board.sh probe' and reboot first." >&2
      exit 1
    fi
    cat "$tmp/report.txt"

    stamp=$(date -r "$tmp/report.txt" +%Y-%m-%d-%H%M)
    dest=$root/reports/board-$stamp
    if [[ -e $dest ]]; then
      echo
      echo "Already saved as ${dest#"$root"/}/"
    else
      mkdir -p "$root/reports"
      cp -r "$tmp" "$dest"
      chmod -R u+rw,go+r "$dest"
      git -C "$root" add -- "$dest"
      git -C "$root" commit -q -m "Add bare-metal probe report from $(date -r "$tmp/report.txt" '+%Y-%m-%d %H:%M')" -- "$dest"
      echo
      echo "Saved and committed as ${dest#"$root"/}/"
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
    echo "Removed Saiph, the probe and their menu entries from the ESP."
    ;;

  *)
    echo "usage: $0 kernel | probe | report | remove" >&2
    exit 2
    ;;
esac
