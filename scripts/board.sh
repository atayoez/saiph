#!/usr/bin/env bash
# Run the probe on the O6 itself (bare metal, entered at EL2) via systemd-boot.
#
#   scripts/board.sh install   build, copy to the ESP, run it once on the next boot
#   scripts/board.sh report    show the bare-metal report; commit it and the ACPI
#                              dump to reports/board-<time the probe ran>/
#   scripts/board.sh remove    delete the probe and its menu entry from the ESP
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
efi=$root/target/aarch64-unknown-uefi/debug/probe.efi
entry=orion-probe.conf
oneshot_var=/sys/firmware/efi/efivars/LoaderEntryOneShot-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f

# Print an EFI string variable (4 attribute bytes, then UTF-16LE text).
efi_string() {
  [[ -e $1 ]] && tail -c +5 "$1" | iconv -f UTF-16LE -t UTF-8 | tr -d '\0'
}

case ${1:-} in
  install)
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
    ' sh "$esp" "$efi" "$entry"
    if [[ $(efi_string "$oneshot_var") == "$entry" ]]; then
      echo "Armed: the next boot runs the probe once, then shows the systemd-boot menu."
    else
      echo "Warning: LoaderEntryOneShot is not set to $entry; the next boot may skip the probe." >&2
    fi
    read -r -p "Reboot now? [y/N] " answer
    if [[ $answer == [yY]* ]]; then
      systemctl reboot
    fi
    ;;

  report)
    # One root call: pack report.txt and the acpi folder into a tar stream;
    # tar keeps each file's time, so report.txt's time says when the probe ran.
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    if ! pkexec sh -c 'cd "$1/EFI/orion-probe" && [ -f report.txt ] && tar -cf - report.txt $(ls -d acpi 2>/dev/null)' sh "$esp" | tar -xf - -C "$tmp"; then
      echo "No report on the ESP. Run 'scripts/board.sh install' and reboot first." >&2
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
    pkexec sh -c 'rm -rf "$1/EFI/orion-probe" "$1/loader/entries/$2"' sh "$esp" "$entry"
    # A one-time boot that still points at the deleted entry is pointed back
    # at the normal default entry.
    if [[ $(efi_string "$oneshot_var") == "$entry" ]]; then
      pkexec bootctl set-oneshot @default
    fi
    echo "Removed the probe and its menu entry from the ESP."
    ;;

  *)
    echo "usage: $0 install | report | remove" >&2
    exit 2
    ;;
esac
