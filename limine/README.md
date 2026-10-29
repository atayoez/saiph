# Limine, bundled

Saiph is booted by [Limine](https://github.com/limine-bootloader/limine), and
ships its own copy so that nobody has to install it. `cargo kernel` puts this
copy on QEMU's boot disk and `cargo board` on the EFI system partition.

| File | What it is |
|---|---|
| `BOOTAA64.EFI` | Limine 12.9.6 for 64-bit Arm UEFI, unchanged |
| `LICENSE` | Limine's licence (BSD-2-Clause) |
| `3RDPARTY.md` | the licences of the third-party code built into it |
| `limine.conf` | Saiph's boot entry (ours, not part of Limine) |

`BOOTAA64.EFI` comes from the official release archive `limine-binary.tar.xz`
of [v12.9.6](https://github.com/limine-bootloader/limine/releases/tag/v12.9.6).
The archive's signature (`limine-binary.tar.xz.sig`) was checked against the
key that Limine's maintainer publishes on GitHub
([github.com/mintsuki.gpg](https://github.com/mintsuki.gpg)), fingerprint
`05D2 9860 D0A0 668A AEFB 9D69 1F3C 021B ECA2 3821`.

SHA-256 of `BOOTAA64.EFI`:
`11288bad5042853c8fbad27ddf65f9bc53675959a87e0e67908a173ce1571479`

## Updating

Download `limine-binary.tar.xz` and its `.sig` from the newest release, check
the signature against the key above, then replace `BOOTAA64.EFI`, `LICENSE`
and `3RDPARTY.md` with the ones from the archive, and update the version and
checksum here.

Saiph plans to replace Limine with its own UEFI loader later (see
`docs/decisions.md`).
