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
  kernel at EL2 with VHE on the O6, at EL1 under KVM. Since 2026-10-29 Limine
  is **bundled** (`limine/`: the official, signature-checked 12.9.6 release
  file plus its licences), so nobody has to install it. Later Saiph gets
  **its own UEFI loader** and stops depending on Limine; that only changes
  the boot handover.
- **Target** (2026-10-28): `aarch64-unknown-none-softfloat` with `-Zbuild-std`
  and `-Zbranch-protection=bti,pac-ret`, on the newest nightly, unpinned.
- **Build** (2026-10-28): Cargo-native, no scripts. Every command is a Cargo
  command: aliases in `.cargo/config.toml` (`cargo kernel`, `cargo board`,
  `cargo screenshot`, plus plain `cargo test`), each crate's own target
  (`forced-target`), and Cargo's runner hook, which hands the built kernel to
  the `runner` crate (Rust). build-std and branch protection are passed only
  by `cargo kernel`, so host crates and IDE checks build normally.
  `bindeps` (the runner depending on the kernel *as a file*) is the planned
  next step, but nightly Cargo panics today when it meets build-std
  ("no entry found for key", tried 2026-10-28); check again on new nightlies.
- **Console font** (2026-10-28): Fira Code 6.2, Regular. It is under the SIL
  Open Font License 1.1 (`kernel/fonts/FiraCode-LICENSE.txt`); that licence
  must travel with any copy of the font or of a kernel built with it.
  `kernel/build.rs` renders it into letter images at build time. Ligatures
  are not drawn: the console draws one image per character.
- **Unicode** (2026-10-28, requirement): the console must render text in
  every script, with the newest Unicode version: full font coverage (a main
  font plus fallbacks, like the Noto family), shaping (`rustybuzz`),
  right-to-left text (`unicode-bidi`), grapheme clusters and wide characters.
  Text is UTF-8 throughout already. It builds on the kernel heap (there
  since 2026-10-29) and needs font files too big to compile in (Limine can
  load them as modules).
- **Kernel heap** (2026-10-29): [talc](https://github.com/SFBdragon/talc)
  5.1.1 does the allocating (`kernel/src/heap.rs`). The heap has its own
  region of addresses, from `0xffff_d000_0000_0000` (at most 1 TiB), and
  grows on demand: when talc runs out, the kernel takes frames from the
  frame allocator and maps them at the region's end, at least 2 MiB at a
  time, so its frames need not be contiguous. It never shrinks yet. The
  heap, the frame allocator and the running page tables are each behind a
  spin lock from [spin](https://github.com/zesterer/spin-rs) 0.12.3. These
  locks don't mask interrupts; that has to be solved before an interrupt
  handler may allocate.
- **Users** (2026-10-28): single-user. No root, no user accounts, no logins.
  Programs stay isolated from each other and from the kernel; that protection
  just isn't organised around who is logged in. How programs get their rights
  is decided together with the kernel type.
- **Kernel type:** not decided yet. It becomes binding with the syscall, IPC
  and user-mode design. The roadmap recommends a capability microkernel.
