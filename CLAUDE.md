# CLAUDE.md

Notes for the next Claude session working on Saiph. Read this first, then
`docs/decisions.md` (every decision and the newer options not taken yet),
`FINDINGS.md` (what the hardware and Saiph measured on the board) and
`docs/roadmap.md` (the long-term plan, ten phases).

## What this is

Saiph is a from-scratch Rust operating system for exactly one machine: the
Radxa Orion O6 (CIX P1 SoC). "Orion" always means the board, never the OS.
It is a learning and research project of one person, built in small steps,
GPL-3.0-or-later, public at https://github.com/atayoez/saiph (account
`atayoez`, author `Atay Özcan <atay@oezcan.me>`). The board is also the
development machine: an aarch64 openSUSE Tumbleweed host, so `cargo kernel
kvm` runs on the same cores Saiph targets.

## How to work with the maintainer

- **They are new to low-level development.** Before each step, explain the
  concept as a short lesson; afterwards, explain what the new output lines
  mean. Plain words, no unexplained jargon.
- **Ask before every design choice** (the AskUserQuestion tool, with a
  recommendation first). Never deliver a build full of choices they did not
  make; a finished multi-decision build made unasked was thrown away once.
- **For every choice, say which option is newer and look up newer
  alternatives on the web first**: current versions and dates, what Linux,
  seL4, Asterinas, Redox and others do *now*, and whether the O6 hardware
  supports it (check the ID registers in `reports/board-*/report.txt` or
  FINDINGS.md). They push back when an option list misses the newest
  technique (e.g. "Linux changed its scheduler after 7.0", which led to a
  release-by-release check).
- **Newest crates**: check crates.io for release dates, prefer the newest
  stable release of a maintained crate, run `cargo update`, and say which
  version was picked and why.
- **Keep to what they picked.** Reread docs/decisions.md before proposing a
  design; build within their earlier choices (e.g. they chose Cargo-native
  commands, not an xtask).
- **Commit every finished step without asking** (clear message explaining
  why, ending with the Co-Authored-By line from the session's instructions).
  **Push only when they ask.** Never commit `.idea/dictionaries/`.
- **Never `git checkout`/`git restore`/`git reset` a file that has
  uncommitted work.** For a temporary experiment, change it with the same
  tool both ways and compare a checksum.
- Run checks yourself. Things that change how the machine boots (EFI
  variables, the ESP, kernel command lines) are theirs: give them the
  command. `cargo board` is theirs to run, since it arms a one-time boot and
  offers to reboot the machine.
- They like progress in visible steps: build, test in QEMU (TCG and KVM),
  commit, then ask them for a board run.

## Commands

All commands are Cargo commands (`.cargo/config.toml` holds the aliases;
run them inside the repository, because Cargo reads that file from the
current directory). Newest nightly, unpinned, with `rust-src`.

| Command | What it does |
|---|---|
| `cargo kernel` | build and boot in QEMU TCG: EL2 with VHE like the board, GICv4, 4 cores, QARMA3 PAC |
| `cargo kernel kvm` | the same under KVM on this machine's cores: EL1, KVM's GICv3, real timing |
| `cargo kernel tcg <words>` | words become the kernel command line (see below) |
| `cargo screenshot [file.png]` | QEMU's screen, while a VM runs |
| `cargo board` | install on the ESP and arm a one-time boot on the O6 (the maintainer runs it) |
| `cargo board log` | print the log Saiph saved in UEFI variables on its last board boot |
| `cargo board remove` | remove Saiph from the ESP |
| `cargo test` | host unit tests: frames (11, incl. an 8-thread race test), paging (10), runner (5) |
| `cargo clippy -p kernel` | lint; four old warnings remain (screen.rs precedence, memory.rs precedence, paging `is_multiple_of`, runner `chunks_exact`) |

A QEMU run powers off at the end. Kernel command-line words: `fault-test`,
`pac-test`, `bti-test`, `nested-test`, `stack-test`, `wx-test`,
`lock-order-test` (each faults or panics on purpose; the kernel then halts
and QEMU stays up, so wrap such runs in `timeout`), `reboot-after=N` (the
board uses 60). Redirect QEMU output to a file and read the relevant part;
the firmware prints a lot first.

**After a board run, read the log yourself** instead of asking for photos:
`cargo board log`, or the files
`/sys/firmware/efi/efivars/SaiphLog*-85a3298b-98bf-462c-8985-29bb1d4e9363`
(skip the first 4 bytes of each: attributes). Then record what the board
showed in FINDINGS.md.

## Layout

- `kernel/` — the kernel (`forced-target = aarch64-unknown-none-softfloat`).
  `src/main.rs` is the boot sequence: `kmain` on Limine's stack, then
  `continue_boot` on the boot core's own stacks.
- `frames/` — lock-free bitmap frame allocator (host-tested, `no_std`).
- `paging/` — page-table builder, 16 KiB granule, W^X by construction
  (host-tested, `no_std`).
- `runner/` — the Cargo runner: builds the FAT boot image (hadris-fat),
  starts QEMU, takes screenshots, installs on the ESP, reads the boot log.
- `limine/` — bundled Limine 12.9.6 (signature-checked; README says how to
  update) and `limine.conf`.
- `docs/` — decisions.md, roadmap.md, research-notes/, logo and SVG diagrams
  (boot-flow.svg, address-space.svg).
- `FINDINGS.md`, `reports/` — what the probe and Saiph measured on the board.

## The kernel today

Boot order (main.rs): screen console → ACPI (SPCR, FADT/PSCI, MADT, GTDT,
read while Limine's tables still map them) → exceptions → PAC + BTI →
frame allocator → own page tables (switch) → heap → EFI runtime tables →
own stacks → GIC + interrupts → timer → `sched::init` → other cores →
scheduler on the boot core + self-test → save the log → reboot countdown
or power off.

Facts worth knowing before changing anything:

- **EL2 with VHE** on the board (EL1 under KVM). With HCR_EL2.E2H the
  `_EL1` register names reach the EL2 registers; code uses `_EL1` names.
  `TPIDR_EL1` is *not* redirected and holds the per-core offset at both ELs.
- **Code is compiled for `cortex-a520`** with `-Zbranch-protection=bti,pac-ret`
  (only through the `cargo kernel` alias). Returns are `retaa`, so the
  kernel needs a CPU with PAC. A plain/IDE build has no landing pads;
  `protect::built_with_branch_protection` detects that and leaves PAC/BTI
  off.
- **Address space** (upper half, 48-bit, 16 KiB pages, 32 MiB blocks):
  kernel image `0xffff_ffff_8000_0000`; stacks `0xffff_e000_…` (1 MiB slot
  per core, then 128 KiB per thread, each stack above an unmapped guard
  page); heap `0xffff_d000_…` (grows on demand, talc); device window
  `0xffff_c000_0000_0000 + phys`; direct map `0xffff_0000_0000_0000 + phys`
  (Limine's HHDM; it forces 48-bit VA, so 4 table levels). The lower half
  is off (TCR.EPD0) except during the moments below.
- **Lower half, 4 KiB granule (TG0):** mmu.rs keeps 1:1 pages for the early
  UART, the table-switch trampoline and the secondary-core entry page.
  efi.rs builds separate 4 KiB tables mapping the UEFI runtime regions 1:1
  and switches to them only during firmware calls.
- **Table switches are break-before-make** from an identity-mapped
  trampoline (the O6 has BBM level 2, not BBML3; see vm.rs). TCR.TG1 is
  changed from 4 KiB to 16 KiB inside it.
- **Locks** (sync.rs): own ticket lock on lock_api 0.5,
  `SpinLock<T, Guardian, Level>`. `IrqsOff` masks interrupts while held
  (nested, per-core count); `IrqsOn` panics if taken in an interrupt
  handler. Every lock names a level in `sync::order`:
  RunQueue < Heap < PageTables < Firmware < GicConfig < Test < Console.
  Taking a lock at or below a held level panics with both names (checked on
  every acquisition, per core); `comes_before::<A, B>()` asserts a nesting
  at compile time. Prefer atomics or `sync::Once<T>` (write once, read
  lock-free) when one value is enough; the frame allocator, the interrupt
  handler table and the GIC ranges need no lock.
- **Per-core data:** `cpu_local!` statics in the `.cpu_local` section
  (percpu.rs); other cores get copies of a snapshot taken at boot.
  `smp::this_core()` is the core number (boot core = 0).
- **Stacks:** kernel code runs on SP_EL0 (per-core or per-thread kernel
  stack); exceptions switch to SP_EL2/SP_EL1, the per-core exception stack,
  so stack overflows get reported. **IRQs taken from SP_EL0 save their
  frame on the thread's own stack** (vector slot 1 does `msr SPSel, #0`),
  so the scheduler can switch inside an interrupt.
- **Interrupts:** GIC-700 via system registers, group 1, one priority (no
  nesting), EOImode 0 (avoids erratum 2941627). SGIs: 0 self-test, 1 ping
  from the boot core, 2 reschedule. Virtual timer: PPI 28 at EL2, 27 under
  KVM (from the GTDT).
- **Scheduler** (sched.rs, sched/policy.rs): per-core run queues, idle
  cores steal and get kicked with SGI 2, a `Policy` trait with seL4-style
  scheduling contexts (priority, budget/period) plus EEVDF among equal
  priorities, lazy tickless preemption (timer set only for the next slice
  end, budget event or wake-up; RESCHED `NOW` vs `LAZY`). A switch holds
  the core's run-queue lock and the next thread releases it
  (`finish_switch`); a switched-out thread stays `on_cpu` until then.
  `schedule()` must be called with no lock held (the lock order enforces
  it). Board result: weights 2:1 → 1.99–2.02x CPU time, 2 ms/10 ms budget
  → 20.2%, 36 threads stolen across all 12 cores.
- **Console:** one IrqsOff lock per `write!`; `console::Raw` skips it (fault
  reports and panics). It keeps the newest 32 KiB as a log, which
  `efi::save_log` writes to UEFI variables at the end of boot and after a
  fault or panic.

## Gotchas met so far

- Cargo reads `.cargo/config.toml` from the current directory: a build
  started elsewhere lost build-std and branch protection, and BTI faulted
  on the board. Hence the alias design and the runtime check.
- QEMU TCG's `-cpu max` has features the O6 lacks (GCS, NMI, RNG, LPA2, SME,
  nested virtualisation): check ID registers before using a feature. KVM
  `-cpu host` is the faithful model. TCG's timer fires late, so budget
  tests overshoot there (about 25% instead of 20%).
- AArch64 `orr` immediates must be bitmask-encodable (`#0x9` is not; use
  two `orr`s).
- Rendering the SVGs: ImageMagick's policy blocks SVG; use headless Firefox
  with a temporary profile in the scratchpad (`firefox --headless
  --screenshot`).
- The early boot runs on Limine's stack with SPSel = 1 and interrupts
  masked; secondary cores start with the MMU off and read their `Boot`
  block through physical addresses (the boot core cleans it to RAM first).

## Known simplifications (on purpose, for now)

Finished threads' stacks are never freed. Budgets refill periodically, not
as sporadic servers. All cores share one PAC key (Linux uses per-task keys,
which need tasks). The console lock is held while drawing, which can be
long on the 3440×1440 screen. The firmware's code regions are mapped
writable and executable during UEFI calls (no memory attributes table
used). The SMMU is still off (device DMA is unrestricted) and MTE is not on
yet. Fira Code is still the console font; the plan is Iosevka plus HarfRust
and ICU4X for full Unicode (decisions.md).

## Docs to keep current with each step

- `docs/decisions.md`: the decision, the date, and a "Newer, not adopted
  yet" note where relevant.
- `README.md`: "What works", the roadmap checkboxes, the feature notes. The
  maintainer's house style for GitHub: permafrost-style README, PayPal
  donate badges (paypal.me/atayozc), SVG diagrams.
- `FINDINGS.md`: every board result, with numbers.
- `docs/boot-flow.svg`, `docs/address-space.svg` when the boot order or the
  address space changes (render them and look before committing).

## Where things stand (2026-10-31)

Roadmap Phase 1 (the nucleus) is done except address spaces, user mode,
the first user program (ELF), capabilities and IPC rings. Phase 2 pieces
already done: all 12 cores by MPIDR, PAC/BTI, PSCI reset/off, the boot log;
still open there: MTE, errata, watchdog, UEFI RTC, entropy, SMMU.

Next step suggested to the maintainer: **address spaces and a first user
program**. Start with a research round (what Linux, seL4, Asterinas, Redox
do today, what is newest), then the lesson and the questions. The kernel
type is still undecided and becomes binding there; the roadmap recommends
a capability microkernel (seL4-style objects, Asterinas's "all `unsafe` in
one crate" rule). The program-rights model is meant to be single-user: no
root, no accounts, isolation by capabilities.
