# Orion O6: hardware findings

Measured by the probe (`probe/`) on the board itself, at EL2, started once from
systemd-boot with `scripts/board.sh install`. No OS was running, so nothing
filtered the values. Section letters (§A–§H) refer to the probe's report.

- **Board:** Radxa Orion O6, CIX P1 (CD8180), 16 GB.
- **Firmware:** Radxa BIOS 1.3.1 (2026-07-16), EDK II revision 0x3, UEFI 2.70.
- **BIOS settings during the runs:** MTE on, SPCR on, network stack off.
- **Linux kernel command line:** `arm64.nobti` removed.
- **Raw reports:** `reports/board-2026-10-28-1801/` is the latest complete run,
  with all cores and the A520 comparison. Each report folder also holds the raw
  ACPI tables (`acpi/*.dat`; decode with `iasl -d *.dat`).

## Summary

| Question | Answer | Where |
|---|---|---|
| Exception level at hand-off | **EL2** | header |
| Boot core | Cortex-A720 r0p1, MPIDR `0x0a00` (core 10) | §A |
| Core types | **4× Cortex-A520 r0p1** (MPIDR `0x000`–`0x300`), **8× Cortex-A720 r0p1** (`0x400`–`0xb00`) | §G |
| BTI | **present** on all cores; Linux hid it only because of `arm64.nobti` | §A, §G |
| MTE | **MTE3** on all cores; asynchronous mode supported (MTE_frac=0) | §A |
| MTE tag storage | **works at EL2** (STG/LDG round trip), with the BIOS MTE option on | §H |
| PAuth | **QARMA3, PAuth2 + FPAC + FPACCOMBINE**, constant PAC field (PAC_frac=1), generic signing (PACGA) with QARMA3 | §A |
| GCS / SME / RNDR | all **absent** | §A |
| SVE | **SVE2**; vector length 128 bits (from Linux earlier, not probed) | §A |
| AArch32 | **none** at any exception level (EL0–EL3 are AArch64-only) | §A raw PFR0 |
| Physical addresses | **40 bits** (1 TiB) | §A |
| Page sizes | 4K, 16K and 64K; **no LPA2** | §A |
| Virtualization | **VHE yes**, **nested virtualization no** | §A |
| RAS | **RAS v1.1** | §A |
| MPAM | **v1.1** (hidden under KVM) | §A |
| Activity monitors (AMU) | **present** (useful for frequency-invariant load tracking) | §A raw PFR0 |
| Profiling / debug | PMUv3.7, **SPE v1.2 and TRBE on the A720 only**, 6 breakpoints, 4 watchpoints | §A, §G |
| Interrupt controller | **Arm GIC-700 r2p1, GICv4.1** (direct virtual LPI injection, RVPEID) | §D |
| SMMU | **one Arm MMU-700 r1p2, SMMUv3.2**, stage 1 + 2, 40-bit output; **left disabled, device DMA passes through** | §D |
| Console UART | **SPCR: PL011 at `0x040D0000`, 115200** (= Linux `ttyAMA0`) | §C, `acpi/spcr.dat` |
| Debug UART | DBG2: PL011 at `0x040E0000` | §C |
| Entropy | **EFI RNG protocol only** (algorithm RAW). No RNDR, no SMCCC TRNG | §E |
| Firmware calls | PSCI 1.1 and SMCCC 1.2 via SMC | §E |
| Starting other cores | **no EFI MP services**; PSCI `CPU_ON`/`CPU_OFF` works (all 11 started and stopped) | §G |
| Network boot | stack disabled in the BIOS by choice; NICs still load, no PXE, no boot entries | §F |
| UEFI Shell | **present** in the boot menu | §F |
| Generic timer | **1 GHz** (`CNTFRQ_EL0`) | Saiph boot |
| Screen (UEFI GOP via Limine) | 3440×1440, 32 bits per pixel, mapped as **device memory** (uncached) | Saiph boot |
| Limine's EL2 hand-off | `HCR_EL2` `0x488000002`: **E2H 1, TGE 1** (a VHE host) | Saiph boot |
| PSCI `SYSTEM_RESET` | **works** | Saiph boot |

## Details

### CPUs

The A520 and A720 report the **same instruction-set and security features**.
Only two fields differ (§G, "compared with the boot core"):

| Field | A720 | A520 | Meaning |
|---|---|---|---|
| `ID_AA64MMFR1_EL1.SpecSEI` | 0 | 1 | the A520 may raise an SError when a speculative read hits an error |
| `ID_AA64DFR0_EL1.PMSVer` | 3 | 0 | no Statistical Profiling Extension on the A520 |

The MADT gives **every core power-efficiency class 0**. The firmware does not
mark big and little cores, so the kernel has to classify each core by its
`MIDR_EL1`.

### Caches

From `CLIDR_EL1`/`CCSIDR_EL1` (§B, §G). These cores use the 64-bit CCSIDR
format (FEAT_CCIDX). All lines are 64 bytes.

| Core | L1 data | L1 instruction | Private L2 | Shared cache |
|---|---|---|---|---|
| A720 | 64K, 4-way, 256 sets | 64K, 4-way | 512K, 8-way, 1024 sets | L3: 12M, 12-way, 16384 sets |
| A520 | 32K, 4-way, 128 sets | 32K, 4-way | **none** | reported as its L2: same 12M, 12-way, 16384 sets |

The A520's level 2 and the A720's level 3 have the same raw CCSIDR value
(`0x3fff0000005a`), so they are one shared cache. **The PPTT on BIOS 1.3.1 has
no cache entries at all**, only the processor hierarchy, so the registers are
the only source of cache geometry.

### Interrupt controller

- **Addresses:** GICD `0x0e010000`, GICR region `0x0e090000` (3 MiB), ITS
  `0x0e050000`. Interrupt IDs go up to 543.
- **IIDR:** `0x0402143b` on all three parts.
- **Known bug:** Linux's GIC-700 erratum 2941627 quirk matches this IIDR (r2p1).
  If the kernel uses split priority drop and deactivate (EOImode=1), it needs
  that workaround.

### SMMU

One MMU-700 at `0x0b1b0000` (IORT). Firmware leaves `SMMU_CR0.SMMUEN` = 0 and
`SMMU_GBPA.ABORT` = 0, so **every device can DMA anywhere until the kernel turns
the SMMU on**. Which PCIe root complexes sit behind it is in `acpi/iort.dat`.

### Console

SPCR exists only while its BIOS option is on. With SPCR present and no
`console=` on the kernel command line, Linux used the serial port as its only
console, and the boot messages and splash disappeared from the screen. Fixed
in `/etc/kernel/cmdline` with `console=ttyAMA0,115200 console=tty0`. The last
entry is the main console.

### ACPI tables (16)

XSDT, FACP, DSDT, SSDT ×2 (one from Radxa, `ORIONO6`), APIC (MADT), IORT, PPTT,
GTDT, MCFG, SPCR, DBG2, PCCT, CSRT, SDEI, FPDT. All checksums are valid.

### Saiph's first boot (2026-10-28)

The kernel itself, booted by Limine through a one-time systemd-boot entry,
ran to the end on the fourth try: PAC and BTI on at EL2, frame allocator,
its own page tables, then a reset through PSCI `SYSTEM_RESET` 60 s later.
Values it printed on the way:

- **EL2 as Limine leaves it:** `HCR_EL2` `0x488000002` (E2H 1, TGE 1),
  `SCTLR_EL2` `0x30d0199d`, `TCR_EL2` `0x2b5103510` (40-bit physical
  addresses, 48-bit virtual, 4K granule, write-back cached and inner-shareable
  table walks). Under QEMU the only difference is the physical-address size.
- **Kernel load address:** physical `0x44c2x0000`, above 4 GiB; it moves a
  little from boot to boot.
- **Screen:** 3440×1440, mapped as device memory, so writes reach the monitor
  without any cache maintenance. (In QEMU, ramfb is mapped write-back.)

The first three runs stopped after the exception self-test. The fault,
decoded from the last-resort squares Saiph draws when even its fault report
faults, was a BTI fault (`ESR` `0x36000002`, from `BLR`) at the first indirect
call. The kernel had been built without branch protection: `cargo` was run
from outside the repository, so `.cargo/config.toml` was never read and the
build fell back to rustup's prebuilt `core`. The hardware was right; the
build is now guarded against it (see `kernel/build.rs`).

## What this means for the kernel

1. **It starts at EL2 with VHE available.** It can run as an EL2 kernel without
   nested virtualization.
2. **Turn on BTI, PAC (QARMA3 is cheap; FPAC makes failures trap at once) and
   MTE (asynchronous or asymmetric mode).** They work identically on every core.
3. **Page tables:** 4K pages and 40-bit physical addresses are enough for 16 GB;
   LPA2 is not available.
4. **Tell core types apart by `MIDR_EL1`, not the MADT.** Expect SPE only on A720s.
5. **Read cache geometry from `CLIDR`/`CCSIDR`.** Don't use the PPTT.
6. **GIC-700 GICv4.1:** watch erratum 2941627 if using EOImode=1.
7. **Enable the SMMU early.** Until then, device DMA is unrestricted.
8. **First console:** the PL011 at `0x040D0000` (from SPCR), 115200 baud.
9. **Get an entropy seed from `EFI_RNG_PROTOCOL` before `ExitBootServices`.**
   Nothing else provides randomness.
10. **Bring up secondary cores with PSCI `CPU_ON`.** That path is proven by
    the probe.
