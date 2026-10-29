# Decisions

What has been decided for Saiph so far. In these docs "Orion" always means the
board (Radxa Orion O6), never the OS.

Every decision is checked against the newest alternatives first, including
whether the O6 supports them. The whole list was reviewed that way on
2026-10-29; "Newer, not adopted" notes what was found and why it waits.

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
  the boot handover. Reviewed 2026-10-29: 12.9.6, base revision 6 and the
  `limine` crate 0.6.5 are the newest. The loader will be a small separate
  crate on `aarch64-unknown-uefi` with `uefi` (0.41 at the time); nmbl and
  UKIs are Linux-only.
- **Target** (2026-10-28): `aarch64-unknown-none-softfloat` with `-Zbuild-std`
  and `-Zbranch-protection=bti,pac-ret`, on the newest nightly, unpinned.
  No newer Armv9 target exists (2026-10-29), and Linux uses the same
  branch-protection flags. **Since 2026-10-29 the code is compiled for
  `cortex-a520`** (`-Ctarget-cpu`), the core type both of the O6's clusters
  can run: LSE atomics, WFxT and the other Armv9.2 instructions the two
  share. Newer, not adopted: GCS (`gcs`) and PAuth_LR (`pc`), which the O6
  lacks.
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
  every script, with the newest Unicode version (18.0 in September 2026):
  full font coverage, shaping, right-to-left text, grapheme clusters and
  wide characters. Planned stack (reviewed 2026-10-29): HarfRust for
  shaping (rustybuzz is archived), ICU4X for segmentation and character
  properties, `unicode-bidi` fed with ICU4X data, `unicode-width`, and
  `skrifa` plus a rasterizer such as `zeno` for fonts loaded at runtime.
  Fonts: Iosevka (actively released; Fira Code's last release was 2021),
  then Sarasa Gothic for CJK, Noto per script, GNU Unifont as last resort.
  Text is UTF-8 throughout already. It builds on the kernel heap (there
  since 2026-10-29) and needs font files too big to compile in (Limine can
  load them as modules).
- **Pages** (2026-10-29): **16 KiB**, not 4 KiB, like Apple's systems and
  Android 15+: fewer TLB misses and table levels per byte, 32 MiB blocks.
  Virtual addresses stay 48-bit (four levels), because Limine's direct map
  starts at `0xffff_0000_0000_0000`. Frames are 16 KiB too.
- **Switching page tables** (2026-10-29): the Linux way. The O6's cores have
  FEAT_BBM level 2, which still allows a TLB conflict abort when old and new
  tables map an address with different sizes (only BBML3, a 2025 extension,
  rules that out). So the switch runs from a page identity-mapped through
  TTBR0: turn TTBR1 walks off, flush the TLB, install the new tables.
- **Kernel heap** (2026-10-29): [talc](https://github.com/SFBdragon/talc)
  5.1.1 does the allocating (`kernel/src/heap.rs`). The heap has its own
  region of addresses, from `0xffff_d000_0000_0000` (at most 1 TiB), and
  grows on demand: when talc runs out, the kernel takes frames from the
  frame allocator and maps them at the region's end, at least 2 MiB at a
  time, so its frames need not be contiguous. It never shrinks yet. Newer,
  not adopted yet: per-core caches in front of it (Linux's "sheaves", 2025),
  once there are several cores.
- **Locks** (2026-10-29): Saiph's own ticket lock on `lock_api` 0.5, whose
  per-acquisition guards are new in 0.5. A ticket lock is fair, so the
  A520 cores can't be starved by the faster A720s; waiting cores sleep in
  `wfe` until the lock word changes, as Linux does. The lock's *type* says
  whether it masks interrupts while held (as in Asterinas and Linux 7.3's
  Rust `SpinLockIrq`), so a lock that interrupt handlers use can't be taken
  with interrupts on. This replaced the `spin` crate (test-and-set, unfair,
  still on `lock_api` 0.4).
- **Interrupts and timer** (2026-10-29): the GIC-700 through the GICv3
  system-register interface (GICv5 is newer, but not on the O6). All
  interrupts are group 1 with one priority, so handlers are never
  interrupted; EOImode 0, which needs no workaround for GIC-700 erratum
  2941627. Timer: the virtual timer, which at EL2 with VHE is the EL2
  virtual timer (Linux's default since 2026), its interrupt taken from the
  GTDT; tickless one-shot deadlines, the core sleeping in `wfi`; the
  counter read through `CNTVCTSS_EL0` (FEAT_ECV) where the CPU has it.
  Handlers do little and print nothing yet (the console has no lock).
  Newer, not adopted yet: pseudo-NMIs through the priority mask (the O6 has
  no FEAT_NMI), `wfet`/`wfit` for bounded waits, a timer wheel for many
  timeouts, and threaded handlers once there is a scheduler.
- **Cores** (2026-10-29): started through PSCI `CPU_ON`, the standard (PSCI
  1.1 on the O6) and independent of Limine, rather than Limine's MP
  feature. ACPI's parking protocol is older and not offered by the O6; the
  MADT's multiprocessor-wakeup mailbox is x86-only. A starting core gets
  the boot core's own register values (EL2/VHE, page tables, vectors, PAC
  key), switches its MMU on from a page mapped 1:1 in the lower half and
  jumps into the kernel. All cores share one PAC key for now; Linux gives
  every task its own, which needs tasks first.
- **Per-core data** (2026-10-29): `cpu_local!` statics, the newest form in
  Rust kernels (Asterinas; Linux's per-CPU areas in C): a linker section,
  copied for each core from a snapshot taken at boot, with TPIDR_EL1 holding
  each core's offset to its copy. Chosen over a per-core struct reached
  through TPIDR, and over an array indexed by core number.
- **Kernel stacks** (2026-10-29): each in its own address range above an
  unmapped guard page (Linux's VMAP_STACK). Kernel code runs on SP_EL0 and
  exceptions on a separate per-core stack (SP_EL2/SP_EL1), so even a stack
  overflow is reported.
- **Console** (2026-10-29): one lock, held for a whole `write!`, masking
  interrupts; fault reports and panics write without it.
- **Physical memory**: a bitmap frame allocator. Newer, not adopted yet:
  LLFree (lock-free, per-core reservations), for when the other cores run.
- **Users** (2026-10-28): single-user. No root, no user accounts, no logins.
  Programs stay isolated from each other and from the kernel; that protection
  just isn't organised around who is logged in. How programs get their rights
  is decided together with the kernel type.
- **Kernel type:** not decided yet. It becomes binding with the syscall, IPC
  and user-mode design. The roadmap recommends a capability microkernel.
  Newest options as of 2026-10-29: seL4 16.0 (AArch64 isolation proofs
  complete, MCS scheduling contexts) and Asterinas's "framekernel" rule (all
  `unsafe` code in one crate, parts verified with Verus). For program
  rights: capabilities with rights masks; WASI 0.3 components for sandboxed
  apps.
