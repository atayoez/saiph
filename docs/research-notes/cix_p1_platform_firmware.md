# CIX P1 (CD8180 / "Sky1") on the Radxa Orion O6: core platform, boot chain and firmware, for a from-scratch Rust kernel

Research date: 2026-10-25. Mainline Linux at this date is in the 7.3 cycle (`VERSION = 7, PATCHLEVEL = 3` in torvalds/linux Makefile, [kernel.org](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/Makefile)).

Two kinds of evidence appear below:
- **[Local]**: observed directly on the user's own board on 2026-10-25: Radxa Orion O6, BIOS `1.3.1` dated 2026-07-16 (from `/sys/class/dmi/id/bios_version` and `bios_date`), openSUSE Tumbleweed kernel `7.2.11-1-default`, ACPI boot. Read without root via sysfs/procfs, plus a small EL0 `mrs` test program. Root-only reads were not possible because sudo asked for a password: raw ACPI tables, dmesg, `/sys/firmware/fdt` and ESRT entries.
- **URL-cited**: from primary or secondary sources. Items that come only from a search-result summary, not a fetched page, are marked *(snippet)*.

---

## 1. SoC composition: CPU clusters, DSU, caches, interconnect, process, memory, TDP

### Takeaway
The CPU complex is a single DynamIQ cluster of 12 cores, all sharing a 12 MB L3. In DT terms that is 4x Cortex-A520 at MPIDR 0x000–0x300 and 8x Cortex-A720 at 0x400–0xB00. The 8 A720s are split into four 2-core DVFS domains: "medium" M0/M1 and "big" B0/B1. On the user's board the per-pair maximum clocks are 2.6 / 2.5 / 2.3 / 2.2 GHz, with the A520s at 1.8 GHz. Memory is 128-bit LPDDR5 (Radxa: 5500 MT/s, up to 64 GB) and the process is TSMC 6 nm. CIX has not published the DSU revision, the interconnect IP or an official TDP.

### Cited Findings
- CIX's own Linaro Connect 2025 slides describe the CD8180 as "Armv9.2 Architecture", with a "12-Core CPU": Quad Big Cortex-A720, Quad Medium Cortex-A720, Quad Little Cortex-A520, and "12MB L3 shared across all cores". The GPU is Immortalis-G720 MC10 and the NPU "up to 30TOPs". — [Linaro Connect 2025, "ACPI support on Radxa Orion O6", CIX (PDF)](https://www.kitefor.events/download/410)
- Mainline `sky1.dtsi` CPU nodes:
  - cpu@0/100/200/300 are `arm,cortex-a520` with `capacity-dmips-mhz = <403>` and SCMI perf domain `SKY1_PERF_CPU_L`.
  - cpu@400/500 are `arm,cortex-a720` in `SKY1_PERF_CPU_M0`; cpu@600/700 in `M1`; cpu@800/900 in `B0`; cpu@a00/b00 in `B1`. The A720s have capacity 1024.
  - All 12 cores are in one `cpu-map` `cluster0` (one DSU).
  - `enable-method = "psci"`. The SoC is licensed BSD-3-Clause, "Copyright 2025 Cix Technology Group".
  - Source: [torvalds/linux arch/arm64/boot/dts/cix/sky1.dtsi](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm64/boot/dts/cix/sky1.dtsi)
- [Local] MIDR values:
  - A720 = `0x410fd811` (part 0xd81, r0p1). A520 = `0x410fd801` (part 0xd80, r0p1).
  - Linux logical CPUs 0,1,6–11 are A720 and 2–5 are A520 (`/sys/devices/system/cpu/cpu*/regs/identification/midr_el1`).
- [Local] cppc_cpufreq maximum frequency per frequency-domain pair:
  - cpu0–1: 2.6 GHz; cpu10–11: 2.5 GHz; cpu6–7: 2.3 GHz; cpu8–9: 2.2 GHz; cpu2–5 (A520): 1.8 GHz. The minimum is 800 MHz everywhere.
  - Linux `cpu_capacity` values: 1024 / 984 / 905 / 866 / 279.
  - The ACPI processor-container hierarchy is `\_SB.C000.{C001: CPU0-1, C002: CPU2-5, C003: CPU6-7, C004: CPU8-9, C005: CPU10-11}`.
- Radxa's product page gives the big A720s "up to 2.6 GHz". One version of the page says 2.8 GHz, which contradicts the rest. The medium A720s are up to 2.4 GHz and the A520s 1.8 GHz. The page also lists "12 MB" shared L3 and "128-bit LPDDR5, 5500 MT/s, up to 64 GB, ~100 GB/s". — [radxa.com Orion O6](https://radxa.com/products/orion/o6/). Other sources claim 6400 MT/s, for example [CNX Orange Pi 6 Plus](https://www.cnx-software.com/2025/10/15/orange-pi-6-plus-cix-p1-sbc-64gb-lpddr5-45-tops-ai-performance/). A reader who rebuilt the BIOS says the code reveals DDR5-6400 support. — [CNX, 2025-03-03](https://www.cnx-software.com/2025/03/03/software-progress-and-source-code-release-for-cix-p1-armv9-soc-and-orion-o6-motherboard/)
- Process node is TSMC 6 nm. — [Tom's Hardware](https://www.tomshardware.com/pc-components/cpus/chinas-newest-homegrown-ai-chip-matches-industry-standard-at-45-tops-6nm-arm-based-12-core-cixin-p1-starting-mass-production). One low-reliability aggregator says 5 nm; disregard it.
- Community-decoded stock `pm_config.bin` (12 DVFS domains, up to 13 OPPs each):
  - CPU Little (A520): 800–2200 MHz. CPU Big G0/G1: 800–2600 MHz. CPU Mid G0/G1: 800–2400 MHz. All CPU domains run at 790–930 mV.
  - DSU: 400–1300 MHz. GPU core: 250–1100 MHz. NPU: 400–1200 MHz. VPU: 150–1200 MHz. MMHUB: 350–750 MHz.
  - Stock power caps: Little 18.5 W, Big G0/G1 19.5 W each, Mid G0/G1 21 W each, DSU 18 W, GPU 20 W, SoC 35 W.
  - Source: [Neol00/edk2-cix-unlocked README](https://github.com/Neol00/edk2-cix-unlocked)
- The CIX vendor PM firmware OPP table for the CPU (shown on the CPU-DVFS slide) runs from 800 MHz/730 mV to 2600 MHz/920 mV. — [Linaro Connect 2025 slides](https://www.kitefor.events/download/410)
- CIX's TRM CPU chapter references `cortex_a720_r0p1_trm.pdf`, `cortex_a520_r0p1_trm.pdf` and `dsu_r2p0_trm.pdf`. A reader corrected the DSU to "r2p1". — [CNX, 2025-12-13](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-developer-guides-for-gpu-ai-accelerator-os-and-firmware-bios/)
- CNX measured high idle power of 16–17 W. — [CNX, 2025-12-13](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-developer-guides-for-gpu-ai-accelerator-os-and-firmware-bios/)
- [Local] Firmware-reported cache topology (PPTT on BIOS 1.3.1) is broken:
  - sysfs shows L1D/L1I/L2 as "shared" across CPUs 0–11 with no `size` attribute, and an L3 shared by 0-1,6-11 only (A520s excluded).
  - The community firmware project carries a `pptt-cache-topology` patch to "correct the PPTT cache layout (private L1/L2, shared 12 MB L3)". — [orion-o6/firmware](https://github.com/orion-o6/firmware)
- [Local] `CTR_EL0 = 0x49444c004`: 64-byte I and D minimum lines, IDC=1, DIC=0, 64-byte MTE tag granule line (TminLine=4). `DCZID_EL0 = 0x4`: `DC ZVA` block = 64 B. RAM visible: `MemTotal: 16179068 kB` (about 15.4 GiB).

### Inferences
- **DSU:** The DSU is almost certainly DSU-120, because A720/A520 r0p1 are DSU-120-generation cores. Whether it is r2p0 or r2p1 is unconfirmed; design for DSU-120 and read `CLUSTERIDR_EL1` at runtime.
- **Big vs medium clocks:** The per-pair max frequencies on the user's board (2.6/2.5 vs 2.3/2.2) do not match the marketing 2.6/2.4. They look like per-chip binning or limits imposed by the SCP firmware. Do not hard-code them: read CPPC `_CPC` "Highest Performance" per domain, or SCMI perf levels in DT mode.
- **CPU numbering:** ACPI and DT number CPUs differently. In ACPI mode, Linux logical CPU0 (always the boot CPU) is an A720, which implies the firmware hands off on an A720 core. In DT, MPIDR 0x0 is an A520. A kernel should index CPUs by MPIDR, never by "CPU number".
- **Caches:** The PPTT cache information on the stock firmware is wrong. A from-scratch kernel should discover cache geometry from `CLIDR_EL1`/`CCSIDR_EL1` (at EL1) rather than trust PPTT.
- **Memory:** A 128-bit LPDDR5 bus at 5500 MT/s gives about 88 GB/s peak, consistent with Radxa's "~100 GB/s" figure (or 6400 MT/s gives about 102 GB/s).

### Gaps
- L2 sizes per core are not published in anything found. The A720 can be configured with 256 or 512 KB, and the A520 with 0–512 KB, possibly shared in a merged-core pair. Read `CCSIDR_EL1` at EL1 on the board.
- The interconnect is not named in any public source (CMN-S3? CI-700? a CIX-proprietary NoC?). The CIX TRM is gated and was not read.
- No official TDP or SoC power envelope exists; only the firmware caps above.
- The exact DSU revision and whether the L3 is truly 12 MB in a single slice are unconfirmed beyond the CIX slide.

---

## 2. GIC, ITS/MSI, SMMU/IORT, generic timer, PMU/SPE/TRBE/CoreSight, MPAM, RAS, SDEI

### Takeaway
The interrupt controller is a GICv3-programming-model GIC with an ITS. The 256 KiB-per-CPU redistributor stride and 192 KiB ITS frame strongly suggest a GICv4.1-capable GIC-700, but this is unconfirmed. The generic timer runs at exactly 1 GHz. PCIe MSIs go through the ITS. SMMUv3 is present (one instance bound under ACPI). PMUv3 is split per core type using PPI partitions. SPE, TRBE, CoreSight, MPAM and RAS (AEST/HEST) are not exposed by the firmware. SDEI is exposed.

### Cited Findings
- Mainline DT:
  - GIC node is `compatible = "arm,gic-v3"`.
  - GICD at `0x0e010000` (size 0x10000). GICR at `0x0e090000`, size `0x300000`, with the comment "GICR * 12".
  - Maintenance interrupt is PPI 9. `#interrupt-cells = <4>`, where the 4th cell is the PPI partition.
  - ITS `arm,gic-v3-its` at `0x0e050000`, size `0x30000`.
  - Two PPI partitions: A520 cpu0–3 and A720 cpu4–11.
  - Source: [sky1.dtsi](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm64/boot/dts/cix/sky1.dtsi)
- Timer: `arm,armv8-timer` with PPIs sec-phys 13, phys 14, virt 11, hyp-phys 10, hyp-virt 12, all level-low. — [sky1.dtsi](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm64/boot/dts/cix/sky1.dtsi)
- [Local] `CNTFRQ_EL0 = 1000000000` (1 GHz) on all three CPU types. Linux clocksource is `arch_sys_counter`. hwcaps include `ecv` and `wfxt`.
- CIX notes that the GTDT describes the Secure EL1, NS EL1, EL2, virtual EL1 and virtual EL2 timers, plus memory-mapped "GT block" and "SBSA Generic Watchdog". Boot firmware initialises CNTFRQ_EL0. A CIX "GPT" timer (`_HID CIXH1007`) serves as the wake-up/broadcast timer when PE timers are powered down, and it is "not ACPI compliant". — [Linaro Connect 2025 slides](https://www.kitefor.events/download/410)
- PMU:
  - DT: `arm,cortex-a520-pmu` and `arm,cortex-a720-pmu`, both on PPI 7 but on different PPI partitions. — [sky1.dtsi](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm64/boot/dts/cix/sky1.dtsi)
  - [Local] Linux shows `armv8_pmuv3_0` (cpus 0-1,6-11) and `armv8_pmuv3_1` (cpus 2-5). No `arm_spe_*`, `trbe` or CoreSight event sources or buses exist under ACPI.
- [Local] `/proc/interrupts` shows `ITS-PCI-MSI`/`ITS-PCI-MSIX` domains for every PCIe endpoint (RTL8126 NICs, RTL8852BE Wi-Fi, NVMe, RTX 5060 Ti).
- DT PCIe RC `msi-map` RID ranges: x1_0 `0x0000+0x3000`, x1_1 `0x3000+0x3000`, x2 `0x6000+0x3000`, x4 `0x9000+0x3000`, x8 `0xc000+0x4000`. — [sky1.dtsi](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm64/boot/dts/cix/sky1.dtsi)
- [Local] ACPI tables present: APIC(MADT), CSRT, DBG2, DSDT, FACP, FPDT, GTDT, IORT, MCFG, PCCT, PPTT, SDEI, SSDT1, SSDT2.
  - There is no AEST, HEST, BERT, EINJ, MPAM, SRAT, SPCR or APMT table, so there is no firmware-first RAS, no MPAM description and no CoreSight/APMT description.
  - Bound drivers: `arm-smmu-v3.0.auto -> arm-smmu-v3` (one SMMUv3 instance from IORT), `sdei.0 -> sdei`, `sbsa-gwdt.0`.
- CIX lists "Armv9, GIC, SMMU, GPIO and Pinctrl support with ACPI" among its ACPI features. — [Linaro Connect 2025 slides](https://www.kitefor.events/download/410)
- A forum poster recommends caution with BIOS 1.3.x because "PCIe SMMU is disabled". This is a personal opinion, not an official note. — [Radxa forum, O6N BIOS changelog](https://forum.radxa.com/t/o6n-bios-changelog/31058) *(snippet)*
- The TRM Part 1 covers "SMMU" and "PCIe". — [CNX, 2025-12-13](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-developer-guides-for-gpu-ai-accelerator-os-and-firmware-bios/)
- Linux has a GIC-700 erratum workaround (#2941627, `ARM64_ERRATUM_2941627`). After `ICC_EOIR1_EL1`, an SPI or ESPI whose effective affinity is a different CPU must be explicitly deactivated via `GICD_ICACTIVER` after a `dsb(sy)`. — [silicon-errata.rst](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/Documentation/arch/arm64/silicon-errata.rst), [irq-gic-v3.c](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/drivers/irqchip/irq-gic-v3.c). The same errata list has MMU-700 entries (#2133013, #2268618, #2812531, #3777127) handled in arm-smmu-v3.

### Inferences
- **GIC version:**
  - A redistributor stride of `0x300000 / 12 = 0x40000` (4 x 64 KiB frames: RD_base, SGI_base, VLPI_base, reserved) is the GICv4/4.1 layout. A GICv3-only GIC uses 128 KiB.
  - A 0x30000 ITS region matches the GICv4.1 ITS, which adds a third page for direct vSGI injection.
  - So the GIC is very likely GIC-700 (GICv4.1). Confirm at boot from `GICD_IIDR` ProductID and `GICD_TYPER2`/`GICR_TYPER.VLPIS` before using vLPI/vSGI. If it is GIC-700, apply the #2941627 workaround whenever EOImode=1 (split drop/deactivate) is used.
- **Timer:** At 1 GHz, 1 tick = 1 ns, which simplifies a tickless scheduler's time base. FEAT_ECV (CNTPOFF) and FEAT_WFxT (`WFET`/`WFIT` with timeout) are available for idle and spin loops.
- **Debug and observability:** Even though the A720 core supports SPE, TRBE and ETE as options, a from-scratch kernel should assume no observability beyond PMUv3, because the firmware does not describe them. MADT GICC SPE/TRBE interrupt fields are presumably 0; this could not be read without root.
- **RAS:** There is no OS-visible RAS error source path. SDEI exists, but no tables register RAS events with it.
- **PCIe:** Whether the PCIe root ports sit behind the SMMU in IORT depends on the BIOS version (see the 1.3.x note). Parse IORT and do not assume.

### Gaps
- GICD_IIDR, GICR_TYPER, the SMMU IDR registers (SMMUv3.1/3.2? MMU-600 or MMU-700?) and the IORT node layout were not read (root needed). The SMMU's MMIO base was not determined.
- Whether the A720s implement SPE/TRBE/ETE in this SoC, and whether the firmware leaves the MDCR_EL3 traps open: unknown.
- MPAM: unknown whether the cores, DSU or interconnect implement it. No ACPI MPAM table exists.

---

## 3. Armv9.x features actually usable by an OS

### Takeaway
Usable now:
- SVE2 at 128-bit VL.
- MTE with MTE3 (asymmetric). Tag storage depends on a BIOS option and is buggy on 64 GB boards.
- PAuth (QARMA3 with FPAC/FPACCOMBINE).
- FEAT_ECV and FEAT_WFxT, BF16/I8MM, the SM3/SM4/SHA3/SHA512 crypto extensions, SB, DIT, FlagM2, FRINTTS, AFP, LRCPC2.

Absent: SME, FEAT_RNG (no `RNDR`), MOPS, HBC, CSSC.

The OS is entered at EL2: KVM works on the stock firmware. BTI is architecturally mandatory, and there is no evidence it is broken on this SoC. The user's `arm64.nobti` sits next to the NVIDIA open driver in `/etc/kernel/cmdline` and is most plausibly a workaround for that driver (inference).

### Cited Findings
- [Local] `/proc/cpuinfo` features:
  - Identical on A720 and A520: `fp asimd evtstrm aes pmull sha1 sha2 crc32 atomics fphp asimdhp cpuid asimdrdm jscvt fcma lrcpc dcpop sha3 sm3 sm4 asimddp sha512 sve asimdfhm dit uscat ilrcpc flagm sb paca pacg dcpodp sve2 sveaes svepmull svebitperm svesha3 svesm4 flagm2 frint svei8mm svebf16 i8mm bf16 dgh mte ecv afp mte3 wfxt`.
  - Not listed: `rng`, `bti` (masked by `arm64.nobti`), `sme*`, `mops`, `hbc`, `cssc`, `ssbs`.
  - `/proc/sys/abi/sve_default_vector_length` = 16 bytes. `sme_default_vector_length` does not exist.
- [Local] EL0-visible (kernel-sanitised) ID registers:
  - `ID_AA64ISAR0 = 0x0021111110212120`: RNDR field = 0, so no FEAT_RNG. TS=2.
  - `ID_AA64ISAR2 = 0x5102`: WFxT=2, GPA3=1, APA3=5, which is PAuth QARMA3 with FEAT_FPACCOMBINE. MOPS=0, BC=0.
  - `ID_AA64PFR1 = 0x300`: MTE=3. The BT field reads 0 because of `arm64.nobti`. SSBS reads 0, consistent with the kernel hiding SSBS for erratum 3194386.
  - `ID_AA64ZFR0 = 0x0000110100110021`: SVE2, SVE-AES/PMULL128, BitPerm, BF16, SHA3, SM4, I8MM.
  - `ID_AA64PFR0 = 0x0001000100110011`. EL2, RME, MPAM, AMU, SEL2 and RAS fields are hidden from EL0, so they cannot be judged from userspace.
- [Local] EL2 hand-off: `/dev/kvm` exists, and `/proc/interrupts` shows GIC PPI 25 `vgic` and PPI 30 `kvm guest ptimer`. Linux only initialises KVM when entered at EL2, so UEFI hands the OS to EL2.
- [Local] The user's `/etc/kernel/cmdline` contains `rd.driver.blacklist=nouveau arm64.nobti`. The NVIDIA open kernel driver `nvidia-open-driver-G07-signed-kmp-meta-595.104.02` is installed.
- `arm64.nobti` was added in 2021 to override the CPU's reported BTI field, "for testing or to work around hardware issues". — [arm64: cpufeatures: Allow disabling of BTI from the command-line (mirror)](https://git.cs.tu-dortmund.de/hamza.kesraoui/sst-linux/commit/93ad55b7852b324a3fd7d46910b88c81deb62357) *(snippet)*. No public source ties `arm64.nobti` to the CIX P1 or the Orion O6 (several targeted searches came up empty).
- MTE status:
  - Radxa announced MTE as available to developers on 2025-08-08, initially only via a self-built `edk2-cix` BIOS. The kernel needs `CONFIG_ARM64_MTE`; `CONFIG_KASAN_HW_TAGS` is only for kernel-side checking.
  - Tag storage costs about 3% of RAM (4-bit tag per 16-byte granule).
  - A BIOS toggle exists. Users disagreed about whether it behaved correctly.
  - Source: [Radxa forum, "MTE feature is now available to developers"](https://forum.radxa.com/t/mte-feature-is-now-available-to-developers/28156)
- MTE bug on 64 GB boards with BIOS 1.3.1: enabling MTE reduces firmware-reported usable memory to about 29 GiB.
  - A forum theory is that the MTE tag-storage (MTSX) carve-out base falls outside the defined RAM regions on 32/64 GB boards.
  - Labelled "firmware-fixable in BL33 early memory sizing".
  - Sources: [radxa-pkg/edk2-cix #39](https://github.com/radxa-pkg/edk2-cix/issues/39), [orion-o6/firmware #8](https://github.com/orion-o6/firmware/issues/8), [Radxa forum MTE on 64 GB](https://forum.radxa.com/t/mte-on-64-gb-failures-explanation-cix-please-help/29781) *(snippets)*
- Errata Linux applies for these cores:
  - Cortex-A520 #2966298 (`ARM64_ERRATUM_2966298`): a speculatively executed unprivileged load may leak privileged data. Workaround: "executing a TLBI before returning to EL0".
  - Cortex-A720 #3456091, handled by `ARM64_ERRATUM_3194386`: an MSR to SSBS is not self-synchronising.
  - Sources: [silicon-errata.rst](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/Documentation/arch/arm64/silicon-errata.rst), [arch/arm64/Kconfig](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm64/Kconfig)
- [Local] With `mitigations=off`, lscpu shows "Not affected" for the x86-centric entries. The Spectre-v2/BHB lines were not captured.

### Inferences
- **BTI:** FEAT_BTI is mandatory from Armv8.5, so the v9.2 A720/A520 implement it. The likely reason for `arm64.nobti` on this user's system is the NVIDIA kernel module: the closed `nv-kernel` object may lack BTI landing pads, which would fault under kernel BTI. This is an inference with no source. A from-scratch Rust kernel without NVIDIA blobs should enable BTI (`-Z branch-protection=bti,pac-ret`, GP bit in stage-1 PTEs) and PAC.
- **PAuth:** The QARMA3 algorithm is cheaper than QARMA5. FPAC makes a failed `AUT*` fault immediately rather than producing a poisoned pointer.
- **Entropy:** With no RNDR/RNDRRS, the kernel needs another entropy source. Options: the SMCCC TRNG interface (if TF-A implements it; probe `SMCCC_TRNG_VERSION`), the EFI RNG protocol at boot (`EFI_RNG_PROTOCOL`, if the EDK2 build provides it), or a security-engine/OP-TEE RNG.
- **SVE/SME:** SVE context is cheap at VL=128 (32 x 128-bit Z registers plus predicates, the same size as NEON plus P registers), so eager save/restore is reasonable. There is no SME/ZA state to manage.
- **MTE:** The CPUs support sync, async and asymmetric (MTE3) tag-check modes. Whether tags are backed by real tag storage depends on the BIOS MTE option and on board RAM size. A kernel must check, at minimum, that `SCR_EL3.ATA` is effectively set (tag instructions don't trap) and that the UEFI memory map or BIOS setting indicate tag storage. A good check: write tags with `STG` and read them back with `LDG` on a test page.
- **Not established for this SoC:** FEAT_LPA2 (52-bit VA/PA with 4K/16K granules), HAFDBS level, E0PD, FEAT_NV/NV2, RME/CCA and GCS are not confirmed. The kernel must probe `ID_AA64MMFR0/1/2_EL1` and `ID_AA64PFR0/1_EL1` at EL1/EL2; EL0 emulation hides these fields. GCS is an Armv9.4 feature, so the v9.2 cores do not have it. RME on a consumer SoC with a closed SE firmware is very unlikely to be enabled even if implemented.

### Gaps
- Raw (unsanitised) values of `ID_AA64MMFR0/1/2`, `ID_AA64PFR0`, `ID_AA64DFR0` and `ID_AA64MMFR3` on this SoC were not obtained. That needs an EL1 probe (e.g., a UEFI app or a tiny kernel module).
- Whether TF-A enables FEAT_NV2/RME/SPE/TRBE (`SCR_EL3`/`MDCR_EL3` configuration) is unknown, although TF-A source is available in `cixtech/bios` (see section 4).
- The definitive reason for `arm64.nobti` is not found in any public source.

---

## 4. Boot chain, open/closed components, management processors, and OS-to-firmware interfaces (PSCI, SCMI, CPPC, PCC, FF-A); DVFS, clock, power and reset control

### Takeaway
The boot chain runs: BootROM → SE (security-engine) firmware in signed `bootloader1`, containing BL1/BL2 and the PM/SCP firmware, verified against a CIX key fused into the chip → FIP `bootloader2` (TF-A BL31 v2.7-based plus OP-TEE BL32) → FIP `bootloader3` (EDK2 BL33). BL31/BL32/BL33 sources are public (`cixtech/bios`) and the FIP can be re-signed with the OEM key the packaging tool holds. The SE and PM firmware are closed and cannot be replaced.

Runtime interfaces:
- PSCI 1.0 over SMC for CPU on/off/idle.
- Under DT: SCMI to the PM processor ("CSU_PM") over the CIX mailbox (perf protocol 0x13, clock protocol 0x14), and SCMI-over-SMC to TF-A (power-domain protocol 0x11).
- Under ACPI: CPPC `_CPC` (backed by SCMI fast-channels and PCC), `_LPI`, AML power resources, and AML methods that wrap SCMI.
- OP-TEE over FF-A/SMC.

### Cited Findings
- Community analysis:
  - The "Security Enclave / bootloader1" covers BL1/BL2 plus the PM firmware (SCP). It "is verified against a CIX key fused into the chip, so it stays stock".
  - The FIP (BL31/32/33) is "re-signed with the OEM key the packaging tool holds" and can be rebuilt; rebuilt images are accepted by the board.
  - Sources are in `cixtech/bios`, branch `cix_p1_community_dev`, a superproject of edk2, edk2-platforms, edk2-non-osi, tf-a, tee and the packaging tool. The `edk2-non-osi` blobs and `bootloader1` are not redistributable.
  - Off-limits to modify: CPU frequency/DVFS (set in the SCP), PSCI and CPU power, runtime errata, OP-TEE.
  - Source: [orion-o6/firmware](https://github.com/orion-o6/firmware)
- SPI flash layout (community, unofficial):

  | Offset | Size | Contents |
  |---|---|---|
  | 0x010000 | | EC firmware |
  | 0x100000 | 4 KB | Firmware header |
  | 0x108000 | 512 KB | XIP region |
  | 0x188000 | 1 MB | `bootloader1.img` (SE/PBL; reads `pm_config`, loads SCP) |
  | 0x288000 | ~992 KB | `bootloader2.img` (FIP: BL31 + BL32) |
  | 0x3F8000 | 16 KB | `memory_config.bin` (DDR) |
  | 0x3FC000 | 4 KB | `pm_config.bin` (OPP tables) |
  | 0x3FD000 | 4 KB | `se_config.bin` |
  | 0x3FE000 | ~4 MB | `bootloader3.img` (FIP: BL33 UEFI) |

  "SE firmware ... reads pm_config early, before the ARM cores leave reset", then loads SCP, which programs the DVFS regulators and TDP caps. Newer Radxa releases "moved frequency control into SCP firmware" and no longer respect `pm_config` OPP tables. — [Neol00/edk2-cix-unlocked](https://github.com/Neol00/edk2-cix-unlocked)
- SystemReady certificate (2025-04-23): firmware "UEFI: 9.0.0 (Built on Apr 11 2025)" and "ATF: v2.7-598b7176a6e2". — [Arm SystemReady certificate (PDF)](https://dl.radxa.com/orion/o6/images/bios/SystemReady/latest/arm-systemready-sr-certification-radxa_sz.pdf)
- Mainline DT firmware nodes:
  - `psci { compatible = "arm,psci-1.0"; method = "smc"; }`
  - Idle states: core sleep `arm,psci-suspend-param = <0x0010000>` (A520: entry/exit 34/100 µs, min residency 3000 µs; A720: 31/79 µs, 3000 µs). Cluster sleep `<0x1010000>` (41/104 µs, 4000 µs).
  - `ap_to_pm_scmi`: `arm,scmi`, mailboxes `mbox_ap2pm`/`mbox_pm2ap`, channel 8. Shared memory: `ap2pm_scmi_mem` 0x06590000 (0x80 bytes); mailbox registers 0x06590080 (SPI 363). `pm2ap_scmi_mem` 0x065a0000; mailbox 0x065a0080 (SPI 359). Protocols `@13` (perf, `#power-domain-cells`) and `@14` (clock).
  - `ap_to_tfa_scmi`: `arm,scmi-smc`, `arm,smc-id = <0xc2000001>`, shared memory at 0x84380000 (0x80 bytes), protocol `@11` (power domains).
  - Other `cix,sky1-mbox` mailboxes: AP↔SE at 0x05060000/0x05070000 (SPI 378/379) and SFH↔AP at 0x08090000/0x080a0000 (SPI 391/392).
  - Source: [sky1.dtsi](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm64/boot/dts/cix/sky1.dtsi)
- CIX's description of ACPI mode (Linaro Connect 2025):
  - CPU idle uses `_LPI` with the entry method PSCI `CPU_SUSPEND` into ATF, which drives the DSU/core PPUs.
  - CPU DVFS: "OS requests the desired performance to the platform over the SCMI fastchannel". `_CPC` exposes Highest/Nominal/Lowest Nonlinear/Lowest Performance, Reference and Delivered Performance Counter registers (AMUv1), Desired Performance and Performance Limited registers. Linux `cpc_write` is a `writel_relaxed` to a mapped address.
  - Device DFS (GPU/NPU/VPU/ISP/interconnect) uses an ACPI "devfreq table" with `SET_TARGET`/`GET_FREQ` methods over PCC to CSU_PM.
  - Clocks use "AML-based SCMI": ACPI methods `acpi_clk_enable/disable/set_rate/get_rate/is_enable` forward over SCMI to the CSU_PM "Clock handler".
  - Power domains use `PowerResource` objects that poke `PGFSM_REG_CTRL2` bits in SystemMemory OperationRegions, with `_PR0`/`_PR3`.
  - Pinctrl uses ACPI 6.2 `PinFunction`/`PinConfig`/`PinGroup` resources.
  - Many devices reuse DT drivers via private `_HID`s plus `_DSD`.
  - Source: [Linaro Connect 2025 slides](https://www.kitefor.events/download/410)
- [Local] ACPI namespace on BIOS 1.3.1:
  - `\_SB.SCMI` (CIXHA006, status 0xF), but `\_SB.SCMI.DVFS` (CIXHA008) and `\_SB.SCMI.CLKS` (CIXHA009) have status 0 (disabled).
  - Mailboxes `\_SB.MBX0–7` (CIXHA001, some enabled) and shared memory `\_SB.SHM0/1` (CIXHA004/5).
  - Clock/reset units `ACRU/DCRU/UCRU/GCRU/CRU0` (CIXHA018), `RST0/RST1` (CIXHA020/021), `PDC0` (CIXHA019), `CCLK` (CIXHA010), pinmux `MUX0/1` (CIXHA016/017), `PEP0` (CIXHA026, a Windows-style PEP), `EC0_` (CIXHA015), `HWMN` (CIXHA024), `TEE0` (CIXHA022, bound to `optee`).
  - Linux 7.2 binds `PCCT` (platform device), `cppc_cpufreq` and `acpi_idle`.
  - `_LPI` states (from `/sys/devices/system/cpu/cpu0/cpuidle`): `LPI-0` Standby (0/0 µs), `LPI-1` Powerdown (latency 360 µs, residency 3000 µs), `LPI-2` ClusterPD (latency 500 µs, residency 10000 µs).
- "Enable ACPI SCMI under the Compliance Configuration menu" is given as a step on BIOS 1.2.x and later. — [Interfacing Linux, 2026-04-27](https://interfacinglinux.com/2026/04/27/orion-o6-o6n-debian-13-update-with-kernel-7-0-and-npu-vpu-support/) *(snippet)*
- Mainline Linux status for the DT power path: "CPU Idle v7.2-rc1", "CPU DVFS v7.2-rc1" (SCMI perf domains), "Power Domain v7.1-rc1", "Reset v7.1-rc1", and "System PM: Won't do". — [cixtech/linux-mainline wiki (edited 2026-10-19)](https://github.com/cixtech/linux-mainline/wiki)
- The CIX developer site says P1 supports TF-A and OP-TEE for secure boot. — [developer.cixtech.com](https://developer.cixtech.com/) *(snippet)*. TRM readers report that the system controller, PM and sensor-fusion processors run firmware that "remains undocumented". — [CNX, 2025-12-13](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-developer-guides-for-gpu-ai-accelerator-os-and-firmware-bios/)

### Inferences
- **Management processors:** At least three exist.
  - SE (security engine): root of trust, runs `bootloader1`, holds the fused key.
  - CSU_PM / SCP (power manager): DVFS, PLLs, PMIC, clocks; reached via SCMI over the `cix,sky1-mbox` doorbell plus shared memory.
  - SFH (sensor fusion hub).
  - There is also an external EC with its own UART and SPI region.
  - None of their firmware is replaceable or documented.
- **DVFS and clock strategy for a new OS:**
  - **DT path (recommended for bring-up):** implement an SCMI client (base, perf 0x13, clock 0x14, power 0x11) over two transports: the CIX mailbox (simple doorbell registers at `0x0659_0080`/`0x065a_0080` plus 128-byte shared memory) and SMC (`0xc2000001`). SCMI is a documented Arm spec (DEN0056) and the mainline `drivers/mailbox/cix-mailbox.c` is a small reference.
  - **ACPI path:** needs a full AML interpreter, `_CPC` (fast-channel MMIO plus PCC subspaces from the PCCT), `_LPI`, `_PSx`/`_PRx`, and the CIX-private clock methods.
  - **Vendor registers (CRU/PGFSM) directly:** possible for resets and power gates (mainline reset and power-domain drivers show the layout), but risky. The PM firmware owns clocks and voltage.
- **PSCI:** PSCI 1.0 with the extended StateID format (0x0010000 = core power-down, 0x1010000 = cluster power-down) is what a kernel uses for SMP bring-up (`CPU_ON` with MPIDRs 0x000–0xB00) and idle.
- **Secure world:** OP-TEE (`CIXHA022`, Linux `optee` plus `ffa_core`) means the secure world speaks FF-A or SMC. A new OS can ignore it initially. It is the path to secure storage and possibly an RNG.

### Gaps
- Whether TF-A implements SMCCC TRNG, SMCCC_ARCH_WORKAROUND_x, SDEI event numbers, or PSCI `SYSTEM_SUSPEND`: not verified. TF-A source exists in `cixtech/bios` and should be checked.
- The PCCT subspace types and channel layout and the `_CPC` register addresses were not dumped (root needed). Run `acpidump`/`iasl -d` with sudo.
- Whether the SCMI-to-PM channel is safe to use from a non-Linux OS while ACPI AML also uses it: unknown. The ACPI mode disables `SCMI.DVFS`/`CLKS` by default, suggesting ownership conflicts.

---

## 5. UEFI/ACPI vs devicetree, SystemReady, BBR/EBBR, capsules, Secure Boot, network boot, GOP

### Takeaway
The stock EDK2 firmware is ACPI-first and SystemReady SR v2.5 certified (SBSA level 6, SBBR). That certification was obtained with only the 8 A720s enabled. The firmware also carries a DTB and offers an ACPI/DT selection in setup. GOP display and UEFI Secure Boot are supported, but Secure Boot is disabled on the user's board. An ESRT is present, suggesting capsule support.

### Cited Findings
- Certificate details:
  - "Arm SystemReady certification: SystemReady SR v2.5", SoC family "CIX P1 CD8180", ACS "SystemReady SR ACS v2.0.0", "SRS v2.2", "BSA v1.0c / SBSA level 6", "SBBR (BBR v1.0)".
  - Tested OSes: Windows PE 26100, SLES 15 SP6, RHEL 9.5, Ubuntu 24.10.
  - "Note that the certification testing was conducted with only eight A720 cores enabled (four A520 cores were disabled)". Issued 04/23/2025.
  - Source: [Arm SystemReady certificate (PDF)](https://dl.radxa.com/orion/o6/images/bios/SystemReady/latest/arm-systemready-sr-certification-radxa_sz.pdf)
- "CIX P1 system is the second SBSA L6 compliant SoC". — [Linaro Connect 2025 slides](https://www.kitefor.events/download/410); also [Radxa O6N page](https://radxa.com/products/orion/o6n/) *(snippet)*. Arm's developer docs call the O6 "SystemReady SR certified and SystemReady band compliant". — [Arm developer doc 102677](https://developer.arm.com/documentation/102677/latest/Set-up-the-Radxa-Orion-O6) *(snippet)*
- CIX says the BIOS has "UEFI + ACPI Table" and "Secure boot, GOP display support". USB and PCIe are "Arm SystemReady" compliant: standard `PNP0D10` xHCI, `PNP0A08` host bridges and standard ECAM through MCFG.
- ECAM layout: a single segment 0 at ECAM base 0x2000_0000, split by bus range:

  | Controller | Buses | Notes |
  |---|---|---|
  | PCIe3 (x1_0) | 0x00–0x1F | |
  | PCIe4 (x1_1) | 0x30–0x4F | |
  | PCIe2 (x2) | 0x60–0x7F | |
  | PCIe1 (x4) | 0x90–0xAF | |
  | PCIe0 (x8) | 0xC0–0xFF | 64 MB ECAM window |

  The other controllers have 48 MB windows. — [Linaro Connect 2025 slides](https://www.kitefor.events/download/410)
- [Local] PCIe topology matches the ECAM layout:
  - Root ports `17cd:0100` (Cadence "CIX PCIe Root Port") at 00:00.0, 30:00.0, 60:00.0, 90:00.0, c0:00.0.
  - Endpoints: RTL8126 5GbE on buses 01 and 31, RTL8852BE Wi-Fi on 61, NVMe on 91, RTX 5060 Ti on c1.
- To pick DT vs ACPI: enter setup with Esc, then Device Manager > O/S Hardware Description Selection. The ACPI SCMI switch is under Compliance Configuration. — [Interfacing Linux, 2026-04-27](https://interfacinglinux.com/2026/04/27/orion-o6-o6n-debian-13-update-with-kernel-7-0-and-npu-vpu-support/) *(snippet)*. The firmware tree ships `Platform/Radxa/Orion/O6/DeviceTree/sky1-orion-o6.dtb`. — [DeepWiki radxa/edk2-platforms](https://deepwiki.com/radxa/edk2-platforms/3.6-radxa-platforms) *(snippet; aggregator)*
- Mainline DT board file: `compatible = "radxa,orion-o6", "cix,sky1"`, `stdout-path = &uart2` (PL011 at 0x040d0000, SPI 298), and a 640 MiB `linux,cma` pool (0x28000000). — [sky1-orion-o6.dts](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm64/boot/dts/cix/sky1-orion-o6.dts)
- Booting mainline requires "BIOS released by Radxa" plus `clk_ignore_unused=1` (as of the 2025 series). — [patchew v6 series](https://patchew.org/linux/20250415072724.3565533-1-peter.chen@cixtech.com/)
- [Local] EFI state:
  - `fw_platform_size` = 64. `SecureBoot` efivar = 0 (disabled). `/sys/firmware/efi/esrt/entries/entry0` exists (contents root-only).
  - `/sys/firmware/fdt` is 693 bytes and root-only; there is no `/sys/firmware/devicetree/base`. This is the EFI-stub's minimal FDT for ACPI boot, not a full DT.
  - The kernel is loaded directly by the bootloader from the ESP (`initrd=\opensuse-tumbleweed\...`).
- The BIOS update package contains `FlashUpdate.efi`, `BurnImage.efi`, `EnrollFromDefaultKeysApp.efi`, `cix_flash_all.bin` and `cix_flash_ota.bin`, and is driven by `startup.nsh` from the UEFI Shell. Radxa's page does not mention capsule updates. — [Radxa docs, Update BIOS](https://docs.radxa.com/en/orion/o6/low-level-dev/bios)
- The community firmware project lists "network boot" among firmware-fixable areas, implying known problems. — [orion-o6/firmware](https://github.com/orion-o6/firmware)
- An early ACPI regression: BIOS 0.3.0-1 failed to boot Fedora 42 in ACPI mode, and users were told to stay on 0.2.2-1. — [Radxa forum Debug Party p.23](https://forum.radxa.com/t/orion-o6-debug-party-invitation/25054?page=23) *(snippet)*. BIOS 1.2.1 broke GPUs in the x16 slot on rev 1.0 boards. — [Interfacing Linux](https://interfacinglinux.com/2026/04/27/orion-o6-o6n-debian-13-update-with-kernel-7-0-and-npu-vpu-support/) *(snippet)*

### Inferences
- **What a from-scratch kernel gets for free on the ACPI/SBSA path:** PL011 UART (SPCR is absent, but DBG2 plus DSDT `ARMH0011` give it), the GTDT timers and SBSA watchdog, GICv3 with ITS, ECAM PCIe, the SMMUv3 described in IORT, PSCI, and xHCI via `PNP0D10`. NVMe and the NICs sit behind standard PCIe. That covers NVMe storage, USB HID and Ethernet (RTL8126, needs a driver) without any CIX-specific driver.
- **What a UEFI loader gets:** GOP gives an early framebuffer on HDMI/DP. A Rust kernel can be a plain UEFI application or stub (e.g., `uefi-rs`), take over from `ExitBootServices` at EL2, and keep the GOP framebuffer as its console.
- **Dev loop:** For a fast loop, UEFI PXE/HTTP boot would be ideal, but its reliability on this firmware is unverified. Fallbacks are a USB stick or the NVMe ESP, or kexec-style chainloading from Linux.
- **Capsules:** The ESRT entry suggests `UpdateCapsule` support for system firmware, but Radxa's documented update path is the UEFI-Shell tool, not capsules.

### Gaps
- PXE/HTTP Boot support in the stock BIOS 1.3.1 was not verified. Check the EDK2 Boot Manager and `NetworkPkg` presence.
- The ESRT fw_class GUID and version could not be read (root needed). Whether `UpdateCapsule` actually works was not tested.
- Whether newer BIOS releases (1.3.x) were re-certified (SystemReady band / SR v3.0) with all 12 cores: not found.
- The EBBR (DT) path is not certified; only SBBR is.

---

## 6. Documentation availability, mainline Linux status (late 2026), vendor BSP

### Takeaway
A CIX P1 TRM exists: Part 1 is 6179 pages and Part 2 is 3051 pages, released December 2025. It is only available after manual-approval registration at developer.cixtech.com, its confidentiality terms bar redistribution, and its coverage is very uneven (PCIe and USB are exhaustive, the CPU section is about 3 pages, DVFS is absent, and the SE/PM firmware is undocumented). The practical "documentation" is the BSD-licensed mainline DT plus the CIX upstream drivers, and the vendor BSP kernels.

As of 2026-10-19, mainline has:
- Base DT, mailbox, UART and clock (6.17).
- I2C/I3C (6.18).
- SPI, pinctrl, PCIe and HDA (6.19).
- Reset, GPIO, power domains and DPU (7.1).
- CPU idle and DVFS (7.2).
- DMA (7.3).

USB, DP and I2S are in review, the GPU is still in planning, and VPU, CSI, GMAC and System PM are "won't do".

### Cited Findings
- TRM:
  - Part 1 V1.0 (6179 pp): system overview, CPU, GPU, NPU, VPU, DPU, SMMU, PCIe. Part 2 V1.0 (3051 pp): USB, FCH (GPIO/SPI/I2C/UART/I3C/timer/XSPI/DMA), DDR, ISP, DP, audio, MIPI, debug.
  - The "Chip Manual" requires registration "which needs manual approval". It was briefly mirrored by Radxa, and a reader reports CIX asked for removal.
  - Reader analysis: about 5000 pages on PCIe, about 1500 on USB registers, CPU 3, DDR 2. Readers say it lacks CPU register details and DVFS descriptions.
  - Also released: BIOS Porting Guide (62 pp), Linux ACPI/DT bring-up and development guides, and an Active Cooling TRM.
  - Source: [CNX, 2025-12-13 (updated 2025-12-17)](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-developer-guides-for-gpu-ai-accelerator-os-and-firmware-bios/)
- Mainline per-component status table: Basic DT, MAILBOX/UART/Clock v6.17-rc1; I2C & I3C v6.18-rc1; SPI, Pinctrl, PCIe, HDA v6.19-rc1; Reset, GPIO, Power Domain, DPU v7.1-rc1; CPU Idle, CPU DVFS v7.2-rc1; DMA v7.3-rc1; USB "Reviewing"; USB3 PHY "Reviewing"; USB/DP combo PHY "Planning"; I2S, DP "Reviewing"; GPU "Planning"; VPU, CSI, RPMSG, GMAC, System PM "Won't do". Board support: Orion O6 v6.17-rc1, OrangePi 6 Plus v7.0-rc1. The page was last edited 2026-10-19. — [cixtech/linux-mainline wiki](https://github.com/cixtech/linux-mainline/wiki)
- Phoronix lists the CIX Sky1 PCIe host driver among the new PCIe controller drivers in 6.19. — [Phoronix](https://www.phoronix.com/news/Linux-6.19-PCI)
- MAINTAINERS entry "ARM/CIX SOC SUPPORT": maintainers Gary Yang and Fugang Duan (cixtech.com); tree `https://github.com/cixtech/linux-mainline.git`; files `arch/arm64/boot/dts/cix/`, `drivers/mailbox/cix-mailbox.c`, bindings `cix.yaml` and `cix,sky1-mbox.yaml`. — [MAINTAINERS](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/MAINTAINERS)
- Recent mainline commits under `arch/arm64/boot/dts/cix` include: "add audss cru", "add sky1 DMA-350 node", "Add CPU idle states for Sky1", "Add SCMI performance domains for CPUFreq on Sky1", "add FCH(S0)/S5 GPIO controllers", "Add scmi powerdomain nodes", "add support for cix sky1 resets", "Add OrangePi 6 Plus board support", "Enable PCIe on the Orion O6 board". — [kernel.org log](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/log/arch/arm64/boot/dts/cix)
- Peripheral IP visible in the mainline DT:
  - Cadence I2C r1p14 (8x, 0x04010000+), Cadence SPI r1p6 (2x), Arm PL011 UARTs 0–3 at 0x040b0000/0x040c0000/0x040d0000/0x040e0000.
  - Cadence I3C (2x), Cadence GPIO r1p02, Arm DMA-350 at 0x04190000.
  - Syscon/reset at 0x04160000 (S0) and 0x16000000 (S5); pinctrl at 0x04170000 and 0x16007000 (S5).
  - Five PCIe RCs `cix,sky1-pcie-host`: x8 @0x0a010000, x4 @0x0a070000, x2 @0x0a0c0000, x1_0 @0x0a0d0000, x1_1 @0x0a0e0000.
  - Source: [sky1.dtsi](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm64/boot/dts/cix/sky1.dtsi)
- Vendor BSP:
  - The initial SDK dump (Feb 2025) at gitlab.com/cix-linux contained Linux 6.1.44. A 6.1 DT kernel and a 6.6 ACPI kernel were planned. Upstreaming is "DT-first, ACPI to follow". — [CNX, 2025-03-03](https://www.cnx-software.com/2025/03/03/software-progress-and-source-code-release-for-cix-p1-armv9-soc-and-orion-o6-motherboard/)
  - CIX slides point to `gitlab.com/cix-linux/cix_opensource/release` (EDK2) and `gitlab.com/cix-linux/cix_opensource/linux` (ACPI kernel). — [Linaro Connect 2025](https://www.kitefor.events/download/410)
  - CIX's site says Linux v6.1 and v6.6 with DT or ACPI. — [developer.cixtech.com](https://developer.cixtech.com/) *(snippet)*
  - The `cixtech` GitHub org has 76 repos, including `cix-linux-main` (updated 2026-10-23; carries Sky1 patches for 6.18.x per [issue #64](https://github.com/cixtech/cix-linux-main/issues/64)), `bios` (2026-08-25), `linux-mainline` (2026-08-31) and `cix_opensource__linux` (2026-10-09). — [github.com/cixtech](https://github.com/cixtech)
  - The Radxa 6.6 ACPI kernel is "linux-sky1". — [Radxa MTE thread](https://forum.radxa.com/t/mte-feature-is-now-available-to-developers/28156)
- Community: the Sky1-Linux project maintains a patched 6.18 kernel and says mainline 6.19 boots to a serial console with NVMe. — [Sky1-Linux](https://github.com/Sky1-Linux) *(snippet)*. There are also Linux 7.2-based community trees for Orange Pi 6 Plus. — [lpennisi73-tech/OPI6PLUS-UBUNTU](https://github.com/lpennisi73-tech/OPI6PLUS-UBUNTU) *(snippet)*
- The Radxa Orion O6 V1.2 schematic is public. — [dl.radxa.com schematic PDF](https://dl.radxa.com/orion/o6/hw/radxa_orion_o6_v1.20_schematic.pdf) *(snippet; not fetched)*

### Inferences
- **Sources of truth:** For a Rust kernel, the BSD-3-Clause `sky1.dtsi` is the most reliable register map (addresses, IRQ numbers, SCMI channels). Mainline drivers (`cix-mailbox.c`, the Sky1 pinctrl/reset/PCIe/power-domain drivers) are GPL. Read them for behaviour, but clean-room any reimplementation if license matters.
- **Coverage gaps:** Anything not upstream is only in vendor 6.6/6.18 trees or binary DKMS drivers: the USB controller and PHYs, DP/eDP combo PHY, GPU (mali_kbase/Panthor), VPU, NPU, ISP and GMAC. USB is SystemReady-standard xHCI under ACPI, which is a big shortcut: the firmware already initialises the PHYs.

### Gaps
- The TRM contents were not read (registration plus NDA-like terms). Whether it documents GIC/SMMU instance parameters, the interconnect, or DSU configuration is unknown.
- No public CIX P1 datasheet with electrical/thermal limits was found.

---

## 7. Debug and recovery

### Takeaway
The OS/BIOS console is PL011 UART2 (`0x040d0000`, ACPI `COM2`/`ARMH0011` UID 3), at 115200 8N1 and 3.3 V on the debug header. Separate UARTs carry PM (UART4), SE BootROM (UART5) and EC logs. The 8 MB, 1.8 V SPI NOR (Winbond W25Q64JW) is socketed on the O6. Updates go through the UEFI Shell with `startup.nsh`. Recovery uses an external CH341A/CH347 programmer with a 1.8 V adapter and flashrom. A bad DSU OPP or firmware can brick the board, which then needs the programmer. No public JTAG/SWD access was found.

### Cited Findings
- UARTs: UART2 carries BIOS and OS debug logs, UART4 power management/voltage/frequency monitoring, UART5 secure BootROM debug logs, plus an EC UART. Wiring is GND/TX/RX at 3.3 V; do not connect VCC. Settings are 115200 baud, 8N1, no flow control. — [Radxa UART Login](https://docs.radxa.com/en/orion/o6/system-use/uart), [Radxa O6 User Manual v0.8 (PDF)](https://dl.radxa.com/orion/o6/docs/radxa_orion_o6_user_manual.pdf) *(snippets; the manual is v0.8, so cross-check against the schematic)*
- [Local] ACPI `\_SB.COM2` (ARMH0011, UID 3) is the only enabled PL011 and is bound to `sbsa-uart`. COM0/1/3 have status 0. Mainline DT `stdout-path = &uart2` at 0x040d0000. — [sky1-orion-o6.dts](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm64/boot/dts/cix/sky1-orion-o6.dts)
- BIOS update and recovery procedure (Radxa docs):
  - UEFI Shell: FAT32 USB stick, Boot Manager → UEFI Shell → `FSx:` → `startup.nsh` (runs `FlashUpdate.efi` with `cix_flash_all.bin`/`cix_flash_ota.bin`). Then power-cycle.
  - Recovery: CH347T with flashrom (`-p ch347_spi:spispeed=15M -w cix_flash_all.bin`), which detects a "Winbond W25Q64.W (8192 kB)". The chip "runs at 1.8 V"; "using 3.3 V may permanently damage the Flash chip". It is socketed on the O6 and on SOP8 pads on the O6N.
  - Source: [Radxa docs, Update BIOS](https://docs.radxa.com/en/orion/o6/low-level-dev/bios)
- Community notes:
  - The chip is W25Q64JWSSIQ (1.8 V; many counterfeits can't do 133 MHz). Pin 1 faces the 40-pin GPIO header.
  - An unstable DSU OPP "can prevent boot" with "no software recovery".
  - A bad `pm_config` is not cleared by removing the CMOS battery.
  - Source: [Neol00/edk2-cix-unlocked](https://github.com/Neol00/edk2-cix-unlocked)
  - "Recovery is off-board with an SPI programmer (CH341A class)". — [orion-o6/firmware](https://github.com/orion-o6/firmware). Separate CH341A guide: [Radxa forum](https://forum.radxa.com/t/guide-flash-bios-with-ch341a-macos-linux/26742) *(snippet)*
- Known firmware and platform issues to plan around:
  - Early firmware and the SystemReady build disabled the four A520s; there is a community patch `a520-enable`. — [orion-o6/firmware](https://github.com/orion-o6/firmware); [certificate](https://dl.radxa.com/orion/o6/images/bios/SystemReady/latest/arm-systemready-sr-certification-radxa_sz.pdf)
  - The PPTT cache topology is wrong (see section 1).
  - With MTE on 64 GB boards, usable RAM drops to about 29 GiB on 1.3.1 (see section 3).
  - 1.3.x reportedly disables the PCIe SMMU *(snippet, opinion)*.
  - The 0.3.0 ACPI regression and the 1.2.1 x16 GPU regression (see section 5).
  - High idle power of 16–17 W. — [CNX](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-developer-guides-for-gpu-ai-accelerator-os-and-firmware-bios/)
  - Mainline needs `clk_ignore_unused=1`. — [patchew](https://patchew.org/linux/20250415072724.3565533-1-peter.chen@cixtech.com/)
- [Local] `kgdboc` is bound on the running Linux, so KGDB over UART is possible as a reference debugging setup.

### Inferences
- **Early console:** For a from-scratch kernel, the simplest early console is a polled PL011 at `0x040D_0000`. Get the base from DBG2 or DSDT `ARMH0011` `_CRS` and keep the baud rate UEFI programmed. Watch UART4/UART5 as well while bringing up PSCI/SCMI: PM-firmware complaints show up there.
- **Dev loop:** The socketed SPI NOR plus a cheap 1.8 V programmer makes firmware experiments (e.g., a custom BL33, or EDK2 changes to fix PPTT/IORT) recoverable. Keep a known-good `cix_flash_all.bin` dump.
- **No JTAG:** Without public JTAG, debugging relies on UART, a GDB stub over serial, and possibly QEMU `virt` with matching cores (`-cpu cortex-a720`? QEMU support for that model is unverified) for the generic parts.

### Gaps
- JTAG/SWD: no source found about an exposed debug header or whether the SE fuses lock the DAP. The schematic (public PDF) should be checked for a JTAG or 10-pin CoreSight footprint.
- Exact debug-header pin numbers and location were not extracted. The docs page `/low-level-dev/serial` returned 404; the user manual and schematic were not fetched.
- Whether a USB "download/recovery" mode exists in the SE BootROM (as on Rockchip maskrom): not documented in any source found. The Radxa BIOS page mentions no recovery button.
