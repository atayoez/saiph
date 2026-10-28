# Decisions

What has been decided for Saiph so far. In these docs "Orion" always means the
board (Radxa Orion O6), never the OS.

- **Name** (2026-10-28): Saiph, after κ Orionis, the star at Orion's knee.
- **Licence** (2026-10-28): GPL-3.0-or-later for all of Saiph's own code
  (`LICENSE`). Linux is GPL-2.0-only, which cannot be combined with it, so
  Linux drivers are read to learn how the hardware behaves, not translated.
  Code offered under MIT, Apache-2.0 or "GPL-2.0 or later" can be used. The
  Fira Code font keeps its own licence (below).
- **Boot** (2026-10-28): Limine, boot protocol base revision 6. It enters the
  kernel at EL2 with VHE on the O6, at EL1 under KVM.
- **Target** (2026-10-28): `aarch64-unknown-none-softfloat` with `-Zbuild-std`
  and `-Zbranch-protection=bti,pac-ret`, on the newest nightly, unpinned.
- **Build** (2026-10-28): plain Cargo. Each crate names its target with
  `forced-target`; `cargo kernel` runs the kernel in QEMU.
- **Console font** (2026-10-28): Fira Code 6.2, Regular. It is under the SIL
  Open Font License 1.1 (`kernel/fonts/FiraCode-LICENSE.txt`); that licence
  must travel with any copy of the font or of a kernel built with it.
  `kernel/build.rs` renders it into letter images at build time. Ligatures
  are not drawn: the console draws one image per character.
- **Users** (2026-10-28): single-user. No root, no user accounts, no logins.
  Programs stay isolated from each other and from the kernel; that protection
  just isn't organised around who is logged in. How programs get their rights
  is decided together with the kernel type.
- **Kernel type:** not decided yet. It becomes binding with the syscall, IPC
  and user-mode design. The roadmap recommends a capability microkernel.
