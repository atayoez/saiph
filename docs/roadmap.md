# Grow a Rust microkernel standards-first on Orion O6

Build a **capability microkernel in Rust**, booted by the stock UEFI firmware (which already hands over at EL2) and described to the kernel through ACPI. Its kernel objects follow seL4 and Atmosphere. All of its `unsafe` code lives in one small crate, as in Asterinas. Completion rings in the style of LionsOS and io_uring are its only IPC. Every driver runs as an unprivileged, restartable server fenced by the SoC's SMMUv3. This design fits the board unusually well. About half of the Orion O6 is standards-based and set up by firmware in ACPI mode: GICv3 with an ITS, a 1 GHz generic timer, a PL011 console, five firmware-trained PCIe root complexes on one ECAM segment, NVMe, ten standard xHCI controllers, an SBSA watchdog, and the UEFI framebuffer and clock. You can therefore reach a 12-core system with networking and storage without a single CIX register document. The other half is CIX-specific: SCMI power and DVFS, five Linlon-D60 display engines, the Immortalis-G720 GPU, the Linlon-V video codec and the Zhouyi NPU. All of it can be driven using open GPL/MIT reference code plus redistributable firmware. Four things are effectively closed: ISP image processing, DSP codecs, the security engine and on-device NPU model compilation. No Rust kernel has been documented on this SoC. Windows, OpenBSD, NetBSD, FreeBSD and the C++ microkernel Managarm have all booted on CIX P1 hardware, which gives you working references. Budget roughly **8–16 full-time months to a headless, networked multicore system, and 2.5–5 full-time years to use the GPU, display, video and NPU natively**, consistent with what Asahi and Redox needed. At half-time, double those figures. The first month should go to protection and diagnosis: back up the SPI flash with a 1.8 V programmer, read the raw ID registers with a tiny UEFI app, and grow the kernel nucleus under KVM on the O6 before touching bare metal.

## A capability microkernel fits a board that is half standard

By 2026 the choice for a new Rust kernel has narrowed to three families:
- **Capability microkernels with user-space drivers:** seL4, Atmosphere, Redox, Managarm, Hubris.
- **Language-isolated single-address-space kernels with a small unsafe core:** Asterinas, and Theseus as an extreme version.
- **Hybrids that reuse drivers through virtualization:** LionsOS, Genode.

Each family has strong recent evidence:
- Atmosphere is a Verus-verified L4-style Rust microkernel with drivers, network stack and file systems in user space. It reached functional-correctness proofs at a **7.5:1 proof-to-code ratio**, partly by using a big kernel lock ([Atmosphere](https://mars-research.github.io/projects/atmo/)).
- LionsOS uses lock-free single-producer/single-consumer shared-memory queues and sends notifications only on empty→non-empty transitions. On an Arm core with a 1 Gb/s NIC it reaches full line rate with about a third of the CPU spare, while Linux plateaus at about 500 Mb/s ([LionsOS paper](https://arxiv.org/html/2501.06234v2)).
- Asterinas showed that a framekernel can confine `unsafe` to **14.0% of linked code**, against Tock's 43.8%, Theseus's 62.4% and RedLeaf's 66.1%. Its benchmarks, however, ran in QEMU on a single core, with SMP left as future work ([Asterinas paper](https://arxiv.org/html/2506.03876v1)).
- Asterinas's AArch64 port, added in October 2026, is "minimal but usable" on QEMU `virt` only ([Asterinas 0.19.0](https://asterinas.github.io/2026/10/13/announcing-asterinas-0.19.0.html)).

The microkernel wins on this board because most of what you must drive natively is PCIe, xHCI and NVMe hardware. These fit the user-space-driver model naturally. The SoC has the two hardware pieces that model needs: an SMMUv3 to confine each driver's DMA, and a GICv3 ITS to give each driver queue its own MSI-X vector. Linux 7.2 on your board already routes NVMe, both NICs and the Wi-Fi card through `ITS-PCI-MSI`/`ITS-PCI-MSIX` domains with `arm-smmu-v3` bound (local probe, 2026-10-25).

The microkernel also matches the research purpose. It teaches MMU, EL2, SMMU, GIC, capability and IPC design in a way a monolith hides. It also turns "live update" into "restart a server with state handoff", which Hubris and LionsOS already do ([Hubris](https://hubris.oxide.computer/)).

The table below weighs the alternatives against your three constraints: from-scratch Rust, learning first, and full use of the board over time.

| Option | Isolation boundary | Fit to the O6 | Main cost | Verdict |
|---|---|---|---|---|
| **A. Capability microkernel at EL2, rings as IPC, user-space drivers** (seL4 / Atmosphere / LionsOS / Managarm lineage) | MMU + SMMU between every service; Rust inside each | Standards-based PCIe/xHCI/NVMe suit user-space drivers; SMMUv3 + ITS present; firmware already enters at EL2 | IPC overhead; a POSIX layer is needed; multicore kernels are unverified everywhere, even in seL4 | **Recommended** |
| **B. Framekernel** (Asterinas/OSTD) | Rust type system inside one privileged image | Best raw throughput; a Linux ABI gives instant userland | One bad `unsafe` anywhere in the privileged image is fatal; every driver lives in it | Borrow its "one unsafe crate" discipline |
| **C. Intralingual single address space** (Theseus) | Compiler only | Zero-cost calls | Cannot contain untrusted native code; Theseus inactive since Sept 2024 ([GitHub](https://github.com/theseus-os/Theseus)) | Borrow ideas only |
| **D. Monolithic Linux-ABI kernel** (StarryOS-style) | User/kernel split | Fast path to BusyBox | Rebuilds Linux with less novelty; drivers stay privileged | Poor fit for research goals |
| **E. Multikernel** (Barrelfish) | Per-core kernels with message passing | Heterogeneous cores | Complexity; little ecosystem | Borrow per-cluster ideas later |
| **F. seL4 + rust-sel4 userland** | Verified C kernel | AArch64 security proofs complete as of Aug 2026 ([seL4 news](https://sel4.systems/news/2026.html)) | Not a new Rust kernel; SMMU, multicore and boot are outside the proofs ([verified configs](https://docs.sel4.systems/projects/sel4/verified-configurations.html)) | Reference, not foundation |

### What the recommended kernel contains

Keep the privileged kernel small, with a target of under about 15k lines. Its object set:
- untyped memory and frames
- stage-1 address spaces and threads
- ring endpoints with notifications
- IRQ handlers for GIC SPIs and LPIs
- SMMU contexts as "IOMMU domain" capabilities
- a shareable buffer-plus-fence object, the equivalent of dma-buf and syncobj, designed once and used by GPU, display, video, camera and NPU
- later, stage-2 VM objects

Rings carry completion semantics like io_uring. Every operation must carry a capability that is checked at submission and emitted as a trace event. That rule is the lesson of io_uring's record:
- Google found that **60% of its kCTF exploit submissions targeted io_uring** and restricted it in production ([Phoronix](https://mail.phoronix.com/news/Google-Restricting-IO_uring)).
- ARMO showed that io_uring operations slip past syscall-hooking security tools ([Security Boulevard](https://securityboulevard.com/2025/04/armo-io_uring-interface-creates-security-blind-spot-in-linux/)).

A microkernel whose IPC *is* the ring gets per-operation authorization and observability for free.

Start with a big kernel lock, as Atmosphere does. Split it only when profiling on 12 cores shows contention.

Verify only the nucleus:
- **Kani** for bit-level MMIO and `unsafe` harnesses ([AWS on Kani](https://aws.amazon.com/fr/blogs/opensource/how-open-source-projects-are-using-kani-to-write-better-software-in-rust/)).
- **Verus** for the page-table manager, frame allocator and capability derivation, following Asterinas's vostd ([vostd](https://github.com/asterinas/vostd)).
- **Flux** for one isolation invariant checked on every commit. TickTock verified Tock's process isolation with Flux in about half a minute ([TickTock](https://cseweb.ucsd.edu/~dstefan/pubs/rindisbacher:2025:ticktock.pdf)).

Run the kernel at EL2 using the EL2&0 translation regime (VHE). The same binary can then run at EL1 when it is a KVM guest. That matters because KVM on your O6 refuses `virtualization=on` (no nested virtualization) and refuses GICv4 emulation (local probe). Fast iteration therefore happens at EL1 under KVM, and EL2-only features are tested in QEMU TCG or on bare metal.

### What the silicon offers a new kernel

The CPU complex is one DynamIQ cluster:
- 4x Cortex-A520 and 8x Cortex-A720, all r0p1, sharing a 12 MB L3 ([sky1.dtsi](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm64/boot/dts/cix/sky1.dtsi); [CIX Linaro Connect slides](https://www.kitefor.events/download/410)).
- 6 nm process ([Tom's Hardware](https://www.tomshardware.com/pc-components/cpus/chinas-newest-homegrown-ai-chip-matches-industry-standard-at-45-tops-6nm-arm-based-12-core-cixin-p1-starting-mass-production)).

Read from EL0 on your board (local probe):
- **SVE2 at a 128-bit vector length**, **MTE3** (asymmetric tag checking), **PAuth with QARMA3 and FPAC** (`ID_AA64ISAR2.APA3=5`, so a failed authentication faults immediately), ECV, WFxT, BF16/I8MM and the SHA3/SM3/SM4 extensions.
- Absent: SME, MOPS and **FEAT_RNG**.
- The generic timer runs at exactly 1 GHz, so one tick is one nanosecond, a convenient base for a tickless scheduler.

Four firmware facts change the kernel's design:
- **Caches:** on BIOS 1.3.1 the firmware's PPTT has no cache entries at all (measured), and the community firmware carries a patch to "correct the PPTT cache layout" ([orion-o6/firmware](https://github.com/orion-o6/firmware)). Discover cache geometry from `CLIDR_EL1`/`CCSIDR_EL1` instead. The probe found 64K+64K L1, a 512K L2 and the shared 12M L3 on the A720; on the A520 it found 32K+32K L1, then the same shared 12M.
- **Entropy:** with no `RNDR`, entropy must come from the EFI RNG protocol at boot, an SMCCC TRNG call if TF-A implements one, or OP-TEE's RNG trusted app. That app is the only OP-TEE device bound on your board (local probe).
- **Errata:** Linux applies Cortex-A520 erratum 2966298 ("executing a TLBI before returning to EL0") and Cortex-A720 erratum 3456091 on SSBS writes. It also applies GIC-700 erratum 2941627 when split EOI/deactivate mode is used ([silicon-errata.rst](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/Documentation/arch/arm64/silicon-errata.rst)). Your exception-return path needs the first; your GIC driver needs the third if you use EOImode=1.
- **CPU numbering:** index CPUs by MPIDR, never by logical number. Under ACPI the boot CPU is an A720, while in the devicetree MPIDR 0x0 is an A520.

Heterogeneity is where this board rewards research. The eight A720s are not two clean tiers (see the reconciliation section). Linux on your board reports five distinct capacity classes, 1024/984/905/866/279, across four A720 DVFS pairs and the A520 quad (local probe).

Linux's Energy-Aware Scheduling needs three things ([kernel docs](https://docs.kernel.org/scheduler/sched-energy.html)):
- a per-performance-domain energy model
- asymmetric CPU capacity
- frequency-invariant load tracking

It also gives up and load-balances once any CPU passes 80% utilization. No energy model exists for this SoC, so you must measure per-OPP power yourself. That is a genuine research contribution on a board whose idle draw is already **16–17 W** ([CNX Software](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-developer-guides-for-gpu-ai-accelerator-os-and-firmware-bios/)).

Make scheduling policy a swappable Rust trait, in the spirit of sched_ext. Within each capacity class use EEVDF, which replaced CFS in Linux 6.6.

Hardening layers:
- **MTE:** use async or asymmetric mode on the kernel heap. Treat it as hardening, not a security boundary, because TikTag leaked MTE tags speculatively with over 95% success ([TikTag](https://arxiv.org/html/2406.08719V1)).
- **PAC:** enable it now.
- **BTI:** enable it once confirmed at EL1.
- **GCS:** design for it but do not expect it. GCS is an Armv9.4 feature and these are Armv9.2 cores.

## Half the board needs no CIX documentation

In ACPI mode the firmware does the CIX-specific setup before your kernel runs:
- **PCIe:** firmware trains the links. Mainline Linux binds no CIX PCIe driver under ACPI (local probe: `pci-sky1` carries only devicetree aliases).
- **USB:** firmware has already initialised the controller clocks, resets and mode straps. Re-running CIX's own init sequence "killed xHCI controllers that were already active" ([Sky1-Linux patch 0086](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0086-usb-cdns3-Skip-destructive-hardware-reinit-under-ACP.patch)).
- **Pins:** firmware pre-muxes them ([patch 0091](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0091-PCI-sky1-Switch-ACPI-from-MCFG-quirk-to-vendor-scan-.patch)).

The two tables below sort every on-board block into what you can drive from public standards, and what needs CIX-specific knowledge.

The upstream status referred to is CIX's own tracker ([cixtech wiki, edited 2026-10-19](https://github.com/cixtech/linux-mainline/wiki)):
- Merged: base DT, mailbox, UART and clock (6.17); I2C/I3C (6.18); SPI, pinctrl, PCIe and HDA (6.19); reset, GPIO, power domain and DPU (7.1); CPU idle and DVFS (7.2); DMA (7.3).
- In review: USB, I2S and DP.
- Planned: the GPU.
- "Won't do": VPU, CSI, GMAC and system PM.

### Platform core and standards-based I/O

| Block | What it is on the O6 | Docs and blobs | Best references (licence) | Difficulty |
|---|---|---|---|---|
| CPU, PSCI, idle | 8x A720 + 4x A520, PSCI 1.0 over SMC; core and cluster power-down state IDs 0x0010000 / 0x1010000 ([sky1.dtsi](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm64/boot/dts/cix/sky1.dtsi)) | Public Arm specs; TF-A source is public | `smccc`, `arm-psci`, `aarch64-cpu` (MIT/Apache) | Moderate (heterogeneity, errata) |
| GIC | GICv3 programming model; ITS at 0x0e050000; 256 KiB redistributor stride, which suggests a GICv4.1-capable GIC-700 (unconfirmed) | Public Arm spec | `arm-gic` 0.10 with EL2 support ([crates.io](https://crates.io/crates/arm-gic)) | Easy; ITS moderate |
| Timers, watchdog | 1 GHz generic timer; SBSA watchdog from GTDT; CIX "GPT" broadcast timer, which CIX calls "not ACPI compliant" ([CIX slides](https://www.kitefor.events/download/410)) | Public | `arm-generic-timer` | Trivial |
| PMU and debug | PMUv3 split by core type; firmware describes no SPE, TRBE, CoreSight, MPAM or RAS tables (local probe) | Public | — | Easy (PMU); the rest is unreachable |
| SMMUv3 | One instance at 0x0b1b0000, described in IORT (local probe) | Public Arm spec | tgoskits `arm-smmu-v3` ([tgoskits](https://github.com/rcore-os/tgoskits)) | Moderate |
| Console | PL011 UART2 at 0x040D0000, 115200 8N1, 3.3 V header; named by the SPCR table when its BIOS option is on; DBG2 names a second PL011 at 0x040E0000 ([Radxa UART](https://docs.radxa.com/en/orion/o6/system-use/uart)) | Public | `arm-pl011-uart` | Trivial |
| UEFI services | GOP framebuffer, runtime `GetTime`, reset, ESRT | UEFI spec; the GOP comes from closed DXE drivers ([edk2-non-osi](https://github.com/cixtech/cix_opensource__release__edk2-non-osi/tree/cix_p1_dev/Silicon/CIX/Sky1/Drivers)) | `uefi` 0.41 ([crates.io](https://crates.io/crates/uefi)) | Trivial |
| ACPI and AML | MADT, GTDT, IORT, MCFG, PPTT (no cache entries), PCCT, DBG2, SPCR, plus a DSDT full of `CIXH*` devices with AML-driven SCMI clock methods | ACPI spec; CIX ASL source is BSD-2-Clause-Patent ([AcpiSocTables](https://github.com/cixtech/cix_opensource__release__edk2-platforms/tree/cix_p1_dev/Platform/CIX/Sky1/Drivers/AcpiSocTables)) | `acpi` 6.1.1 with AML ([crates.io](https://crates.io/crates/acpi)); vendored ACPICA | Moderate to hard |
| PCIe | 5 Cadence root complexes: x8 to the slot, x4 to M.2 M-key, x2 to E-key, 2x x1 to the NICs; one ECAM segment at 0x2000_0000 split by bus range | PCIe spec; the Cadence registers are only in the gated TRM | `pci_types`, tgoskits `pcie` | Easy to moderate, if you never retrain links |
| NVMe | M.2 M-key, Gen4 x4 | NVMe spec | tgoskits `nvme-driver` (MIT), Redox `nvmed` (MIT) | Moderate |
| USB host | **10 xHCI controllers** (6 with 10 Gb/s SuperSpeed, 4 USB 2.0-only), each with a single root port, exposed as PNP0D10 | xHCI spec | `crab-usb` ([crates.io](https://crates.io/crates/crab-usb)), Redox `xhcid` (MIT) | Moderate to hard (a whole USB stack) |
| Ethernet | 2x RTL8126A 5GbE on Gen3 x1 links | No datasheet; the `rtl8126a-2.fw` patch is optional and redistributable ([r8169](https://github.com/torvalds/linux/blob/master/drivers/net/ethernet/realtek/r8169_main.c)) | OpenBSD `rge` supports the 8126 (BSD) ([man](https://man.openbsd.org/rge)); tgoskits RTL8125 (MIT) ([crates.io](https://crates.io/crates/realtek-rtl8125)) | Moderate |

### CIX-specific blocks and accelerators

| Block | IP and status | Docs, firmware, licensing | References | Difficulty |
|---|---|---|---|---|
| Power, clocks, DVFS | SCMI to the CSU_PM power processor over the CIX mailbox (perf 0x13, clock 0x14), SCMI-over-SMC to TF-A (power 0x11), CPPC over PCC | SCMI spec public; PM firmware closed and fused | `arm-scmi` 0.2 (MIT/Apache); mailbox reference is 274 lines of GPL C; ACPI SCMI needs base-protocol discovery ([patch 0074](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0074-firmware-arm_scmi-Add-ACPI-boot-support-for-CIX-Sky1.patch)) | Moderate; **prerequisite for every accelerator** |
| Low-speed I/O | Cadence I2C ×8 and SPI ×2 (the Zynq IP), I3C, Cadence GPIO, CIX pinctrl, PWM, fan controller | Public Zynq-era docs; CIX parts only as 200–600-line GPL drivers ([patch 0065](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0065-treewide-Add-ACPI-device-IDs-for-CIX-Sky1-SoC-periph.patch)) | Linux, U-Boot | Easy to moderate; respect `_STA` (I2C2/3/6 are hidden) |
| Thermal, fan, power button, EC | Six ACPI thermal zones, a two-state ACPI fan, PWRB; ITE IT5570E EC on a hidden I2C bus ([schematic](https://dl.radxa.com/orion/o6/hw/radxa_orion_o6_v1.20_schematic.pdf)) | ACPI; EC firmware is in the shared SPI ROM | `cix-fan` hwmon ([patch 0064](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0064-hwmon-Add-CIX-Sky1-fan-controller-driver.patch)) | Moderate (needs AML); leave the EC alone |
| RTC | HYM8563 on hidden I2C3; Linux uses UEFI `GetTime` | Public datasheet | `uefi` crate | Easy; pick one owner |
| DMA | 2x Arm DMA-350 | Public Arm TRM | `arm-dma350` plus CIX ACPI patch ([0016](https://github.com/cixtech/cix-linux-main/blob/main/patches-7.3/0016-dma-arm-dma350-add-acpi-support-for-cix-soc.patch)) | Easy to moderate |
| Audio | HDA "IPBLOQ" controller with a Realtek ALC256-family codec; 10 Cadence I2S; Tensilica HiFi5 DSP | HDA spec public; DSP firmware closed | `cix-ipbloq.c` ([0013](https://github.com/cixtech/cix-linux-main/blob/main/patches-7.3/0013-DPTSW-26459-sound-hda-rework-cix-ipbloq-based-on-mai.patch)); tgoskits Intel HDA | HDA easy to moderate; DSP blocked and low value |
| Display | 5x Arm China Linlon-D60 DPUs (register-compatible with Mali-D71), Trilinear DP 1.4 TX with DSC/MST, combo PHYs; HDMI through a DP-to-HDMI bridge; eDP | Gated TRM; komeda Linlon-D6 support merged in 7.1 ([dri-devel](https://lore.gitlab.freedesktop.org/drm-ai-reviews/20260313033119.33686-2-cunyuan.liu@cixtech.com/T/)) | CIX `linlon-dp` stack, **27,775 lines** of GPL ([patches-7.3](https://github.com/cixtech/cix-linux-main/tree/main/patches-7.3)); FreeBSD komeda port ([JamesKane](https://github.com/JamesKane/drm-komeda-kmod)) | Medium-hard for one output; hard for all outputs with MST, HDCP and Alt Mode |
| GPU | Immortalis-G720 MC10, a v12 CSF Mali, 10 shader cores, MMIO at 0x15010000 ([visorcraft](https://github.com/visorcraft/orange-pi-6-plus-gpu)) | No public register manual; `arch12.8/mali_csffw.bin` is closed but redistributable under Arm's EULA (per its `LICENCE.mali_csffw` text, read on your board) | Tyr (GPL-2.0 OR MIT); CIX `panthor_sky1.c` (GPL-2.0 or MIT) ([00112](https://github.com/cixtech/cix-linux-main/blob/main/patches-7.3/00112-drm-panthor-Sky1-CIXH5000-ACPI-bring-up.patch)); Panthor | Very hard but unblocked |
| Video (VPU) | Linlon-V (Mali-V lineage), 4 cores; 8K60 decode, 8K30 encode | Firmware protocol headers in the GPL driver; per-codec `.fwb` blobs with no named licence | `mvx` driver, about 39k lines ([VPU repo](https://github.com/cixtech/cix_opensource__vpu_driver/tree/cix_mainline_dev)) | Medium-hard; check the blob licence |
| NPU | Zhouyi X2 (architecture v3), 3 core nodes, about 30 TOPS | Kernel driver GPL; UMD `libaipudrv` 4.1.0 Apache-2.0; **compiler closed, x86-only** | FreeBSD UMD port ([aipu-umd](https://github.com/JamesKane/aipu-umd)); `aipu` 6.2.0 ([NPU repo](https://github.com/cixtech/cix_opensource__npu_driver/tree/cix_mainline_dev)) | Kernel side medium; end to end needs the closed toolchain |
| Camera | 2x MIPI CSI-2, a CIX CSI receiver, an Arm China "armcb" ISP | ISP is programmed by a closed `isp_app` daemon plus a 25.9 MB `DDR0.IMG` and tuning files ([cix_proprietary](https://github.com/cixtech/cix_proprietary__cix_proprietary)) | GPL 6.6-only shim ([ISP repo](https://github.com/cixtech/cix_opensource__isp_driver)) | Raw capture easy-medium; ISP blocked |
| USB-C PD, OTG, DP Alt Mode | 2x Realtek RTS5453H on I2C1 with a proprietary register interface (not UCSI); Cadence OTG glue | None public; PD firmware is on the chip | `rts5453` 611 lines and `cdnsp-sky1` (GPL) ([patch 0004](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0004-usb-phy-Add-CIX-Sky1-USB-and-PHY-drivers.patch)) | Defer; host mode works without it |
| Security engine, sensor hub, PM processor | Root of trust, sensor fusion MCU, power processor | Undocumented, closed firmware | — | **Blocked** |
| SPI NOR / XSPI | 8 MB, 1.8 V, shared with the EC; XSPI disabled in ACPI | — | — | **Do not touch from the OS** |

**Effectively blocked:**
- ISP processing, the DSP codecs and the security engine.
- On-device NPU compilation. The compiler ships only as an x86_64 Python 3.10 wheel ([Radxa NPU setup](https://docs.radxa.com/en/orion/o6/app-development/artificial-intelligence/env-setup)).
- Replacing the security-engine or PM firmware. Both are verified against a fused CIX key ([orion-o6/firmware](https://github.com/orion-o6/firmware)).
- CPU tracing via SPE, TRBE or CoreSight, and firmware-first RAS, because the firmware publishes no tables for them.

Not blocked, but out of scope under your rules:
- The Wi-Fi/Bluetooth module is an M.2 E-key add-in (an RTL8852BE on your unit), not part of the board ([Radxa A8](https://docs.radxa.com/en/accessories/wireless-a8)). Its Linux driver is about 9.8 MB of C across all chips ([rtw89](https://github.com/torvalds/linux/tree/master/drivers/net/wireless/realtek/rtw89)). It belongs with the RTX 5060 Ti in the out-of-scope column.
- The SoC's Cadence GEM Ethernet MAC is not wired out on the O6.

**Documentation** is the other gating factor:
- **The TRM:** CIX released a 6,179 + 3,051-page manual in December 2025. It requires manually approved registration, it barred redistribution (Radxa's mirror was removed at CIX's request), and its coverage is lopsided: about 5,000 pages on PCIe, three on the CPU, and nothing on DVFS ([CNX Software](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-developer-guides-for-gpu-ai-accelerator-os-and-firmware-bios/)).
- **The working documentation** is the BSD-3-Clause mainline devicetree, the BSD-2-Clause-Patent ACPI source, and the GPL/MIT vendor drivers.

Adopt a clean-room rule from day one:
- License the kernel and your own servers **MIT OR Apache-2.0**, matching `aarch64-paging`, `arm-gic`, `uefi` and `acpi`.
- Read GPL code for behaviour but do not translate it.
- Prefer dual-licensed or permissive sources: Tyr and `panthor_sky1.c` (GPL-2.0 or MIT), OpenBSD `rge`, Redox and tgoskits (MIT).
- Keep any Linux-derived code inside a separately licensed driver server. In a microkernel that server is an ordinary process.

## Where the notes disagree, your own board settles it

The research notes conflict on ten points. In almost every case the deciding evidence is either a primary project source or a direct reading from your own board, which beats marketing pages and search-result absences.

*Update 2026-10-28:* the rows marked **(measured)** were settled by the bare-metal probe at EL2, with no OS in between. See [`FINDINGS.md`](../FINDINGS.md) and `reports/board-2026-10-28-1801/`.

| Question | Conflicting claims | Stronger evidence | Verdict |
|---|---|---|---|
| Which non-Linux OSes boot on CIX P1? | One note found "no public measurements of a non-Linux OS"; another said Managarm had "no real aarch64 board evidence" | Primary project sources: OpenBSD's hardware page, NetBSD's boot log, Managarm's own update, FreeBSD driver repos | **At least five boot**; no Rust kernel is documented |
| How are the A720s tiered? | Marketing: 4 "big" at 2.6 GHz (one page and CNX say 2.8) + 4 "medium" at 2.4; firmware config: 2.6 / 2.4 / A520 2.2; DT: all A720s identical | Your board's CPPC limits plus the mainline DT topology | **Identical cores in four 2-core DVFS domains**, with per-pair ceilings of 2.6 / 2.5 / 2.3 / 2.2 GHz |
| Is BTI present? | One note: "unexpectedly report no BTI" | Bare metal: `ID_AA64PFR1.BT=1` on all 12 cores. `arm64.nobti` was only hiding it, and Linux (with the NVIDIA driver) boots fine without it | **Present (measured)** |
| Can the kernel use GCS? | The architecture survey recommends enabling GCS | `GCS=0` in the ID registers, also on bare metal; GCS is an Armv9.4 feature | **Not on this board (measured)**; test it on FVP/QEMU |
| Console UART base | 0x040D0000 (your board, mainline DT) vs 0x040E0000 (an OpenBSD thread about a January 2025 SPCR test firmware) | Bare-metal ACPI: SPCR (present when its BIOS option is on) names a PL011 at 0x040D0000, 115200 baud, which is Linux's `ttyAMA0`; DBG2 names a second PL011 at 0x040E0000 | **Both exist (measured)**: console at 0x040D0000 (SPCR), debug port at 0x040E0000 (DBG2) |
| Does UEFI enter the OS at EL2? | Architecture survey: unconfirmed | `/dev/kvm` and a `vgic` maintenance IRQ on your board; the probe reads `CurrentEL` = 2 | **Yes (measured)** |
| Is MTE usable? | "Integration-dependent, unconfirmed" | Bare metal: `MTE=3` on all cores, and an STG/LDG tag round trip at EL2 works with the BIOS MTE option on | **Usable (measured)** with the BIOS option on; a usable-RAM bug affects 32/64 GB boards, not your 16 GB one |
| Is the PCIe SMMU disabled on BIOS 1.3.x? | A forum opinion says yes | Bare metal: one Arm MMU-700 r1p2 (SMMUv3.2) at 0x0b1b0000 with `SMMU_CR0.SMMUEN=0` and global bypass; Linux's `arm-smmu-v3` turns it on later | **The firmware leaves it off, with DMA passing through (measured)**; the OS must enable it. IORT lists which root ports sit behind it |
| What feeds the HDMI port? | PS185 bridge confirmed only on the Orange Pi 6 Plus | O6 schematic sheet "DP to HDMI" plus a Sky1-Linux PS185 bridge patch | **A DP-to-HDMI bridge**, probably a PS185 |
| NPU name and core count | "Zhouyi V3" vs "Zhouyi X2" | Arm China maps architecture v3 to the X2 product | **X2 product, v3 architecture**; 3 `CRE` nodes vs `core_mask = 3` still unresolved |

### Five non-Linux OSes already boot on this SoC

The "nothing but Linux" claim is a search gap, not a fact:
- **OpenBSD/arm64** lists the Orion O6 and O6N under the Cix CD8180 ([OpenBSD](https://www.openbsd.org/arm64.html)).
- **NetBSD** booted in ACPI mode in January 2025 ([port-arm](https://mail-index.netbsd.org/port-arm/2025/01/19/msg009109.html)). It used only CPU0 and attached GICv3 with the ITS, the timer, PL011, the PCIe bridges, NVMe and a 1920x1080 framebuffer. It left CPU1–11, every xHCI controller and both RTL8126 NICs unconfigured.
- **FreeBSD** boots, but its cores stay at 1 GHz because it has no CPPC/SCMI frequency path ([FreeBSD forums](https://forums.freebsd.org/threads/cpu-frequency-stuck-at-1-ghz-on-radxa-orion-o6-arm-cortex-a720-no-cpufreq-support.99702/post-722951)). On the same SoC (an Orange Pi 6 Plus), FreeBSD ports of Panthor and komeda run sway via ACPI ([drm-panthor-kmod](https://github.com/JamesKane/drm-panthor-kmod)).
- **Windows 11 arm64** installs with "precious few drivers" ([Jeff Geerling](https://www.jeffgeerling.com/blog/2025/radxa-orion-o6-brings-arm-midrange-pc/)).
- **Managarm**, an asynchronous C++ microkernel, runs an Orion O6 in ACPI mode in its hardware CI ([Managarm](https://managarm.org/2026/01/13/end-of-year-update.html)). The note that found "no real aarch64 board evidence" relied on Managarm's handbook ([handbook](https://docs.managarm.org/handbook/print.html)), which documents only QEMU nightlies. The project's own January 2026 update is the more specific and more recent primary source.

So your kernel would probably be **the first Rust kernel** on the CIX P1, but not the first non-Linux one. Managarm is the closest architectural analogue to study. NetBSD's log shows exactly where a newcomer stalls: secondary CPUs, USB and the NICs.

### The A720 "tiers" are really four binned DVFS pairs

The CIX slides describe "Quad Big Cortex-A720, Quad Medium Cortex-A720, Quad Little Cortex-A520" ([CIX slides](https://www.kitefor.events/download/410)). Radxa's page gives the big cores "up to 2.6 GHz" and the medium cores 2.4 GHz ([Radxa](https://radxa.com/products/orion/o6/)). The community-decoded power config lists big domains at 800–2600 MHz, medium at 800–2400 MHz, and A520s up to 2200 MHz ([edk2-cix-unlocked](https://github.com/Neol00/edk2-cix-unlocked)).

The hardware description disagrees with all of these. The mainline devicetree gives all eight A720s the same `capacity-dmips-mhz = <1024>`, places all twelve cores in one `cpu-map` cluster, and splits the A720s into four SCMI perf domains, M0, M1, B0 and B1 ([sky1.dtsi](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm64/boot/dts/cix/sky1.dtsi)). Every A720 on your board reports the same MIDR, `0x410fd811`.

Your board's CPPC limits show what the firmware actually allows (local probe):
- cpu0–1: 2.6 GHz
- cpu10–11: 2.5 GHz
- cpu6–7: 2.3 GHz
- cpu8–9: 2.2 GHz
- the A520s: 1.8 GHz

The same community source says newer Radxa firmware "moved frequency control into SCP firmware" and no longer honours the decoded tables. That explains why the nominal 2.4/2.2 GHz figures differ from what you see.

The firmware-reported numbers on your board are the strongest evidence. They mean the kernel must read each domain's ceiling at runtime, from CPPC "Highest Performance" or SCMI perf levels, and schedule over **five capacity classes**, not a hard-coded big/medium/little table. The 2.8 GHz figure appears in some CD8180 marketing, but nothing your firmware exposes reaches it.

## Stock UEFI at EL2 is the right front door

### Keep the closed boot chain, own everything after ExitBootServices

The boot chain runs ([orion-o6/firmware](https://github.com/orion-o6/firmware)):
1. BootROM.
2. The security engine's signed `bootloader1`, which holds BL1/BL2 and the power-management (SCP) firmware and is verified against a fused CIX key.
3. A FIP with TF-A BL31 (v2.7-based) and OP-TEE.
4. A FIP with EDK2 as BL33.

The SystemReady certificate records "ATF: v2.7" and SR v2.5 with SBSA level 6 ([certificate](https://dl.radxa.com/orion/o6/images/bios/SystemReady/latest/arm-systemready-sr-certification-radxa_sz.pdf)). TF-A, OP-TEE and EDK2 sources are public, and the FIP can be re-signed with the OEM key that the packaging tool holds.

Stay on stock firmware (your board runs BIOS 1.3.1, dated 2026-07-16) and pin that version. History shows BIOS updates regress:
- BIOS 0.3.0 broke Fedora's ACPI boot ([Radxa forum](https://forum.radxa.com/t/orion-o6-debug-party-invitation/25054?page=23)).
- BIOS 1.2.1 broke GPUs in the x16 slot on rev 1.0 boards ([Interfacing Linux](https://interfacinglinux.com/2026/04/27/orion-o6-o6n-debian-13-update-with-kernel-7-0-and-npu-vpu-support/)).

Rebuilding BL33 yourself becomes reasonable only once you hold a programmer and a verified flash dump. Reasons to do it would be adding cache entries to the PPTT or enabling HTTP boot.

The kernel ships as a UEFI application built with the `uefi` crate. It collects the memory map, the ACPI RSDP, the GOP framebuffer and EFI RNG output, then calls `ExitBootServices` and continues at EL2.

The first page tables must honour the UEFI memory attributes (WB/UC/WC/XP). Redox's July 2026 report traced hangs at kernel entry on a real Arm SoC to exactly this mistake, which QEMU never exposed ([This Month in Redox, Jul 2026](https://redox-os.org/news/this-month-260731/)).

Keep the UEFI runtime services mapped for `GetTime` and reset. Pick that, or a native HYM8563 driver, as the single owner of the RTC. Never use both.

### ACPI first, devicetree as a cross-check

Make ACPI the primary hardware description and keep a DT path as a secondary one. Three reasons favour ACPI:
- CIX's own Linux patch set now targets ACPI boot ([cix-linux-main](https://github.com/cixtech/cix-linux-main)).
- Firmware pre-initialises USB, PCIe and pin muxing only in ACPI mode.
- ACPI keeps you portable to other SystemReady machines.

The cost is an AML interpreter. Power resources poke power-gate registers, and clock control runs through "AML-based SCMI" methods that forward to the power processor ([CIX slides](https://www.kitefor.events/download/410)).

In a microkernel the interpreter is just another server, which makes the choice cheap to reverse. Start with rust-osdev's pure-Rust `acpi` crate and its AML feature; that is the on-mission choice. Keep tgoskits' vendored ACPICA (Apache-2.0 AND BSD-3-Clause), the engine NetBSD used on this board, as a fallback if CIX's DSDT defeats it.

The BSD-3-Clause `sky1.dtsi` remains your best register map: addresses, IRQs and SCMI channels. The firmware ships a DTB and lets you select DT or ACPI under Device Manager, O/S Hardware Description Selection ([Interfacing Linux](https://interfacinglinux.com/2026/04/27/orion-o6-o6n-debian-13-update-with-kernel-7-0-and-npu-vpu-support/)).

### Power and clocks go through one SCMI server

Write an SCMI client early, because every accelerator needs it:
- Implement the base, perf, clock, sensor and power protocols.
- Support two transports: the CIX mailbox doorbell with its 128-byte shared memory, and SMC function `0xc2000001` ([sky1.dtsi](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm64/boot/dts/cix/sky1.dtsi)).

CIX's own ACPI Panthor patch shows why the SMC transport matters. The GPU powers on only after a raw SMC SCMI `POWER_STATE_SET` for domain 21. Until then, "any GPU register read causes an unrecoverable SError" ([patch 00112](https://github.com/cixtech/cix-linux-main/blob/main/patches-7.3/00112-drm-panthor-Sky1-CIXH5000-ACPI-bring-up.patch)).

Make that server the **single owner** of each mailbox channel. AML methods talk to the same power processor, Sky1-Linux had to serialise USB init to avoid SCMI mailbox races ([patch list](https://github.com/Sky1-Linux/linux-sky1/tree/main/patches)), and your board's firmware disables the ACPI `SCMI.DVFS` and `SCMI.CLKS` nodes, which hints at an ownership policy (local probe).

CPU DVFS uses CPPC `_CPC` through SCMI fast channels and PCC. Idle uses `_LPI` mapped to PSCI `CPU_SUSPEND`. FreeBSD's 1 GHz ceiling is the price of skipping this layer.

### A three-tier test loop, plus a second board

Your O6 is also your daily workstation, so every bare-metal test costs a workstation reboot. Shape the loop around that.

**Tier 1: host tests.** Pure crates (page tables, ACPI parsing, allocators, the capability table) run as ordinary `cargo test` natively on aarch64, under Miri and Kani.

**Tier 2: KVM on the O6.** Run `-M virt,gic-version=3 -cpu host -accel kvm`. On your board this passes through MTE, 128-bit SVE and PAuth (local probe), which gives sub-second boots on real A720 features.

**Tier 3: emulators for what KVM cannot do.**
- QEMU TCG with `virtualization=on,iommu=smmuv3 -cpu max` covers EL2, SMMUv3 and MTE fault testing. QEMU 11.1.2 has no Cortex-A720 model, so `neoverse-n2` or `cortex-a710` are the closest (local probe).
- `sbsa-ref` with TF-A and EDK2 gives a firmware-shaped GICv3+ITS/xHCI/ACPI machine ([QEMU sbsa-ref](https://qemu.readthedocs.io/en/master/system/arm/sbsa.html)).
- Arm's free AEM FVP covers Armv9.x features the board lacks ([Arm forum](https://community.arm.com/support-forums/f/dev-platforms-forum/57624/arm-a-profile-v9-5-and-v9-6-aem-fvp)).

**Real hardware.** Buy a **second Orion O6** (launched from $199 ([CNX](https://cnx-software.com/2024/12/18/radxa-orion-o6-mini-itx-motherboard-is-powered-by-cix-p1-12-core-armv9-soc-with-a-30-tops-ai-accelerator))) as a dedicated bench target:
- Connect a USB-UART on UART2 (GND/TX/RX only, never VCC). Also watch UART4 for power-firmware logs and UART5 for the BootROM ([Radxa UART](https://docs.radxa.com/en/orion/o6/system-use/uart)).
- Add a power relay and a capture script, making a labgrid-style CI cell. Asahi named CI across real devices as its top 2025 priority ([Asahi](https://asahilinux.org/2025/02/passing-the-torch/)).
- Check whether CIX's EDK2 enables PXE or HTTP boot; it is unverified. If it does, each build reaches the board in seconds. Otherwise boot from a USB stick or the ESP.

**Two experiments worth trying, both unverified on this board:**
- VFIO-pass the second RTL8126 (bus 31) into a KVM guest. You could then develop the NIC driver against real silicon without rebooting, provided the device sits in its own IOMMU group.
- Later, build an m1n1-style EL2 tracing hypervisor that runs CIX's vendor Linux and logs MMIO to the display, VPU and NPU. Asahi used this technique to learn Apple's hardware without disassembling Apple's drivers ([Asahi Aug 2021](https://asahilinux.org/2021/08/progress-report-august-2021/)). Your kernel's EL2 VM objects make it a natural Phase 9 project.

**Toolchain.**
- Pin a nightly in `rust-toolchain.toml` and target `aarch64-unknown-none-softfloat`.
- Use `-Zbuild-std=core,alloc`. `-Zbranch-protection` (PAC/BTI) is still unstable and is now a target modifier that cannot be mixed with the prebuilt `core` ([#113369](https://github.com/rust-lang/rust/issues/113369); [Rust OSDev, Mar 2026](https://rust-osdev.com/this-month/2026-03/)).
- Avoid custom JSON targets. Rust 1.95 destabilised them ([RELEASES.md](https://github.com/rust-lang/rust/blob/master/RELEASES.md)).
- GitHub's hosted runners cannot run aarch64 KVM guests, so register the O6 itself as a self-hosted runner for the KVM tier.

## Reuse Mesa, relibc and Wayland rather than reinventing them

### Layer the application ABI

SerenityOS shows the cost of refusing third-party code: Ladybird left the project precisely so it *could* use libraries ([LWN](https://lwn.net/Articles/976822/)). For a learning OS that wants the hardware fully used, choose reuse deliberately at the layers where reuse is cheap.

Stack four application interfaces:
1. **Native IPC:** a capability-based Rust API wrapped in an `os-rt` crate.
2. **Rust `std`:** a Tier-3 `aarch64-unknown-<yours>` target whose std platform layer calls `os-rt`. This is the pattern Motor OS upstreamed in 2025 ([PR #147765](https://github.com/rust-lang/rust/pull/147765)).
3. **C and POSIX:** relibc, a POSIX libc written in Rust, extended with a third `platform/<yours>` module beside its Linux and Redox ones ([relibc](https://github.com/redox-os/relibc)).
4. **Sandboxed apps:** WASI 0.3 through Wasmtime. Since June 2026 WASI 0.3 makes async native in the component model, which maps cleanly onto message-passing IPC ([WASI 0.3](https://wasi.dev/releases/wasi-p3)). No general-purpose OS uses WASI as a primary ABI, so this layer is novel research.

Self-hosting is gated by libc completeness, not by Rust. Redox got rustc and cargo working only in January 2026, on its third attempt, after fixing kernel, signal and networking problems ([Phoronix](https://www.phoronix.com/news/Redox-OS-January-2026)). Until then, cross-compile on Linux on the same O6 and port std-only tools first: uutils, nushell, helix, gitoxide.

### Storage, networking and audio

**Storage:**
- Ship an immutable, content-addressed, Merkle-verified system image with A/B slots, following the composefs/fs-verity model ([composefs](https://blogs.gnome.org/alexl/2022/06/02/using-composefs-in-ostree/)).
- Add a small writable data volume, either your own log-structured store or a RedoxFS fork.
- Read existing Linux disks with `ext4-view`, which reached 1.0 in September 2026 ([crates.io](https://crates.io/crates/ext4-view)).
- Do not write a production CoW filesystem. bcachefs was ejected from mainline after a decade of work ([LWN](https://lwn.net/Articles/1040120)).

**Networking:**
- Start with smoltcp (0BSD) over a ring-based NIC driver.
- smoltcp lacks congestion control ([Asterinas paper](https://arxiv.org/pdf/2506.03876)), and two 5 GbE ports at 1500-byte MTU mean roughly 813k packets per second. Reaching line rate needs multi-queue drivers, batching rings, checksum/TSO offloads and zero-copy buffers, more than a better stack.
- Use rustls for TLS.

**Audio:**
- Start with the HDA controller and ALC256 codec.
- Write a small graph server modelled on PipeWire.
- Give clients cpal's `custom` backend feature, so applications need no fork ([cpal](https://github.com/RustAudio/cpal)).

### Graphics: GOP first, then a Panthor-compatible GPU personality

Climb a ladder in which every rung is usable on its own.

**Rungs 1 and 2: the GOP framebuffer.** Start with Slint's software renderer drawing straight into the GOP framebuffer. Its `Platform`/`WindowAdapter` traits need nothing else ([Slint](https://docs.slint.dev/latest/docs/rust/slint/platform/)). Next, add a Smithay-based Wayland compositor on the same framebuffer. Smithay underpins cosmic-comp and niri ([LWN](https://lwn.net/Articles/1025866)). Redox already runs COSMIC apps and treats Wayland as its compatibility path ([Redox priorities](https://www.redox-os.org/news/development-priorities-2025-09/)).

**Rung 3: native modesetting.** Drive one Linlon-D60 pipe to one DP output, using komeda's D71 back end as the behavioural reference ([komeda](https://github.com/torvalds/linux/tree/master/drivers/gpu/drm/arm/display/komeda)).

**Rung 4: the GPU.** The G720 is a CSF Mali. The driver must:
- power the GPU on via SMC SCMI, enable its clocks, pulse its reset and enable Q-channel gating
- program AArch64 4K LPAE GPU page tables ([panthor_mmu.c](https://github.com/torvalds/linux/blob/master/drivers/gpu/drm/panthor/panthor_mmu.c))
- load the `arch12.8` firmware and boot the MCU
- schedule command-stream groups onto the firmware's slots

Tyr, the Rust port of Panthor, reached MCU boot in Linux 7.3 in about 3,700 lines with six `unsafe` lines. Even so, it still cannot submit jobs after a year of funded work ([Collabora](https://www.collabora.com/news-and-blog/blog/2026/10/21/upstreaming-tyr-rust-drm-and-the-linux-kernel/)).

Expose a **Panthor-compatible uAPI personality** over your native capability objects. Unmodified Mesa PanVK and Panfrost then run on day one. PanVK needs only about 17 Panthor ioctls plus GEM/PRIME, syncobj and buffer mmap ([panthor_drm.h](https://raw.githubusercontent.com/torvalds/linux/master/include/uapi/drm/panthor_drm.h)), and Managarm proves Mesa accepts a non-Linux DRM implementation. If the personality becomes a burden, move later to a small native backend in Mesa's `pan_kmod` layer; forks already retarget that layer to Arm's kbase.

Above Vulkan, wgpu and Vello make the whole Rust GUI ecosystem available.

Set expectations from same-SoC Linux evidence:
- Mesa lists the G720 at Vulkan 1.4 with no conformance submission ([Mesa](https://docs.mesa3d.org/drivers/panfrost.html)).
- An Orange Pi 6 Plus scored 3,079 in glmark2, but saw Vulkan compute fail with OOM or DeviceLost on large workloads, and MCU reset failures after killing a GPU process ([visorcraft](https://github.com/visorcraft/orange-pi-6-plus-gpu)).

### NPU, video and camera

The NPU is open at runtime and closed at compile time:
1. Implement a Rust kernel driver with the `armchina_aipu.h` 4.1.0 ioctl semantics, plus the WBUF/SLOT zero-copy ioctls that tinygrad uses.
2. Port Arm China's Apache-2.0 `libaipudrv` against relibc, as FreeBSD did ([aipu-umd](https://github.com/JamesKane/aipu-umd)).
3. Run precompiled `.cix` models, compiling offline on an x86 Linux host.

LLM decoding on the NPU is limited by its roughly 24 GB/s memory port ([tinygrad examples](https://github.com/allbilly/tinygrad-npu-examples)). CPU inference is the stronger first target: KleidiAI-accelerated llama.cpp raised prompt processing on this SoC from about 36 to 186 tokens per second ([aijishu](https://community.aijishu.com/a/1060000000520901)). On this GPU, Vulkan inference ran slower than the CPU ([Radxa forum](https://forum.radxa.com/t/llama-cpp-benchmarks/27813)).

For the VPU, implement the host side of the MVE firmware message queues, using the open protocol headers. Clarify the `.fwb` licence before redistributing the blobs.

For the camera, stop at raw CSI-2 capture plus a software ISP. The hardware ISP is programmed by a closed daemon.

## Ten phases with measurable exits

The durations below are full-time-equivalent months for one experienced developer; at 20 hours a week, roughly double them. They are my estimates, anchored on comparable projects:
- **Asahi** had a four-to-seven-person core team and all of Linux's generic subsystems to build on. It still took about **15 months to an alpha installer**, about two years to a GPU rendering under Linux, and about 3.5 years to conformant OpenGL 4.6 and Vulkan 1.3 ([Asahi](https://en.wikipedia.org/wiki/Asahi_Linux)).
- **Redox's AArch64 port** spent about four years going from QEMU-only to SMP on real SoCs: QEMU-only in 2022 ([Phoronix](https://www.phoronix.com/news/Redox-OS-0.8-Released)), real-SoC multicore in 2026 ([Redox Aug 2026](https://www.redox-os.org/news/this-month-260831/)).
- **Haiku's ARM64 port** reached stable SMP only in QEMU, in 2026 ([Phoronix](https://phoronix.com/news/Haiku-OS-ARM64-SMP)).
- **Genode's** professional team needed "more than two years" for its PinePhone image ([genodians](https://genodians.org/topics-pinephone)).
- **Hubris**, narrowly scoped, shipped in about 1.5 years ([Slashdot](https://news.slashdot.org/story/23/07/29/192248)).

You start with advantages they lacked: UEFI, ACPI, SystemReady firmware, and open reference drivers. You also start without Linux's generic subsystems, which roughly cancels the advantage.

| Phase | FTE months | What gets built | Exit criteria |
|---|---|---|---|
| **0. Groundwork** | 0.5–1 | Flash backup; ACPI dump; UEFI probe app; repo, pinned toolchain, QEMU/KVM runner, CI | Verified flash dump on disk; probe prints raw ID registers, `CurrentEL=2`, cache geometry, GIC and SMMU ID registers on the O6; QEMU smoke test passes in CI |
| **1. Nucleus in QEMU/KVM** | 2–4 | Boot, MMU with UEFI attributes, frame allocator, exceptions, GICv3 via system registers, tickless timer, capabilities, address spaces, threads, rings, user mode, ELF init; SMP via PSCI | 12 vCPUs under KVM boot in under 1 s; two processes exchange 10⁶ ring messages; capability revocation tests pass; all `unsafe` sits in one crate with Kani harnesses |
| **2. Bare-metal SMP** | 1.5–3 | ACPI MADT/GTDT/DBG2/PPTT (the PPTT has no cache entries), all 12 cores by MPIDR, errata, MTE + PAC, watchdog, UEFI RTC, PSCI reset/off, entropy | All 12 cores (including the A520s, which SystemReady testing disabled) survive a 24-hour IPI/migration stress; MTE traps an injected use-after-free; PAC faults on a forged return |
| **3. Standard I/O servers** | 4–8 | ECAM enumeration without link retrains, ITS + IORT, per-driver SMMU contexts, NVMe, RTL8126, smoltcp, 10x xHCI with hub/HID/storage, FAT ESP | NVMe throughput ≥80% of Linux on the same drive; TCP ≥2 Gb/s per port; USB keyboard works on every port; a crashed driver restarts without a reboot; images network-boot |
| **4. Power and energy-aware scheduling** | 2–4 | AML server, SCMI server, CPPC DVFS, `_LPI` idle, thermal and fan, Cadence I2C/GPIO, measured energy model, pluggable scheduler | All five capacity classes scale frequency; idle wall power ≤ Linux on the same board; latency tasks land on A720s and background work on A520s with energy per job ≤ Linux |
| **5. Userland ABI** | 3–6 | `os-rt`, Rust std target, relibc platform, Wasmtime/WASI 0.3, verified A/B image | uutils, helix and nushell run; a C test suite passes on relibc; a WASI 0.3 component runs; a failed update rolls back |
| **6. Framebuffer desktop and audio** | 2–4 | Slint, then a Smithay compositor on GOP; HDA + ALC256; audio graph server; DMA-350 | Wayland clients render; headphone playback via cpal custom host without dropouts |
| **7. Native display and GPU** | 10–20 | Linlon-D60 + DP TX KMS on one output, then more; Rust CSF driver; Panthor uAPI personality; Mesa; wgpu | Native modeset to 4K on DP; unmodified PanVK runs vkcube; wgpu triangle; glmark2 ≥70% of Linux on the same board (Linux scored 3,079 on a same-SoC Orange Pi 6 Plus); GPU recovers after a client is killed |
| **8. Video, NPU, camera** | 5–10 | VPU firmware queues, NPU driver + `libaipudrv`, raw CSI, CPU inference with SVE2/I8MM | 4K H.265/AV1 decodes into a shared buffer shown zero-copy; a ModelScope `.cix` model matches Linux output; raw frames captured from an attached sensor |
| **9. Research extensions** | Open-ended | Verus proofs of the nucleus, EL2 tracing hypervisor, optional Linux driver VMs, USB-C role/Alt Mode, DP MST, I2S, native rustc | Chosen per interest |

Cumulatively:
- Phases 0–3 reach a headless, networked, storage-backed 12-core system in roughly **8–16 FTE months**. That is in line with Asahi's 15-month alpha, which had more people but less to write.
- Phases 4–6 add power management, a usable ABI and a framebuffer desktop, totalling about **1.5–2.5 FTE years**. This is a natural stopping point for a research OS. The instructional Proto OS shows that multicore, USB, DMA and a window manager on commodity hardware is a coherent achievement in itself ([Proto](https://arxiv.org/abs/2504.17984)).
- Phases 7–8 are where "full use of the hardware" lives, and they cost as much as everything before them.

If you would rather reach the accelerators early, a LionsOS-style Linux driver VM is the bridge. LionsOS reports about two weeks per device class and roughly 106 MiB per VM, at a "significant performance cost" ([seL4 Summit 2025 slides](https://sel4.systems/Summit/2025/slides/trustworthy-systems.pdf)). Treat it as scaffolding rather than the destination, because it trades away the Rust-native learning you are after.

## Design hooks now for Armv9.4, GICv5 and the next CIX chip

The O6 is a conservative Armv9.2 baseline: no SME, GCS, POE or D128. Arm's shipping and announced cores already go further:
- Lumex C1 cores are Armv9.3 with SME2 ([Arm](https://newsroom.arm.com/news/announcing-lumex-css-platform-ai-era)).
- Armv9.4 added permission indirection and overlays, translation hardening, 128-bit page tables and the Guarded Control Stack ([Arm 2022](https://developer.arm.com/community/arm-community-blogs/b/architectures-and-processors-blog/posts/arm-a-profile-architecture-2022)).
- Armv9.7 added targeted TLB-invalidate domains and GICv5 ([Arm 2025](https://developer.arm.com/community/arm-community-blogs/b/architectures-and-processors-blog/posts/arm-a-profile-architecture-developments-2025)).
- Armv9.8 added conditional-fault instructions explicitly motivated by Rust bounds checks ([Arm 2026](https://developer.arm.com/community/arm-community-blogs/b/architectures-and-processors-blog/posts/arm-a-profile-architecture-developments-2026)).

The cheapest future-proofing is a handful of abstractions that cost little now and are painful to add later.

**Page tables.** Make them generic over granule, descriptor format (VMSAv8-64 vs D128) and permission model (direct AP bits vs indirection), selected at runtime. Linux's D128 series is a build-time switch whose kernel halts on hardware without D128 ([LKML](https://lkml.iu.edu/2607.3/10515.html)). That is what retrofitting looks like.

**Per-thread state.** Make the saved state an extensible, lazily-saved, feature-flagged area. It must grow to hold SVE up to 2048 bits, SME's ZA tile, the GCS pointer, `POR_EL0`, MTE control and PAuth keys.

**Interrupts.** Hide the controller behind hierarchical IRQ domains and use the system-register CPU interface even on GICv3. Never hard-code "IPI = SGI": in GICv5, IPIs are LPIs ([LWN](https://lwn.net/Articles/1018742/)).

**TLB invalidation and CPU capabilities.** Put TLB invalidation behind a shootdown-domain API, and keep per-core capability masks in the scheduler. Future clusters will not share an identical ISA the way the A520s and A720s do here.

**System interfaces.** Version every IDL with frozen, numbered API levels and an ABI stamp checked at process load, as Fuchsia does ([Fuchsia API levels](https://fuchsia.dev/docs/concepts/versioning/api_levels)). Make WASI 0.3 components the portable app format.

**Firmware description.** Funnel ACPI and FDT into one common device model, and bind drivers by `_HID`/`_CID` or `compatible`.

**Test matrix.** Run a QEMU/FVP CI matrix that toggles SVE, SME, MTE, GCS, POE, GICv3/v5, 4K/16K granules and D128. Feature-detected paths then stay tested before silicon exists. QEMU does not yet implement MTE4 ([QEMU docs](https://qemu.readthedocs.io/en/master/system/arm/emulation.html)).

**Portability targets.** The natural ones are the other CIX P1 boards: O6N, Orange Pi 6 and 6 Plus, and the Minisforum MS-R1. After them comes CIX's March 2026 ClawCore family, which is still Armv9.2 with the same Immortalis-G720 ([CNX](https://www.cnx-software.com/category/processors/cix/)).

## Scope, firmware quirks and GPU depth dominate the risks

| Risk | Why it is real here | Mitigation |
|---|---|---|
| Scope creep and burnout | Asahi lost its founder, its Rust GPU author and its graphics lead in 2025; its post-mortem lesson was that "a large project cannot rest on a single person's shoulders" ([Asahi](https://asahilinux.org/2025/02/passing-the-torch/)) | One flagship goal per phase; treat Phase 6 as a legitimate finish line; write docs as you go |
| Bricking the board | A bad DSU OPP or power config "can prevent boot" with "no software recovery" ([edk2-cix-unlocked](https://github.com/Neol00/edk2-cix-unlocked)); the NOR is 1.8 V, and 3.3 V "may permanently damage" it ([Radxa BIOS](https://docs.radxa.com/en/orion/o6/low-level-dev/bios)) | 1.8 V programmer, verified dump, never write the XSPI from your OS |
| Heterogeneous bring-up quirks | SystemReady certification ran with only the eight A720s enabled ([certificate](https://dl.radxa.com/orion/o6/images/bios/SystemReady/latest/arm-systemready-sr-certification-radxa_sz.pdf)), and Windows crashes with the A520s on ([CNX](https://www.cnx-software.com/2025/04/30/radxa-orion-o6-mini-itx-motherboard-gets-arm-systemready-sr-v2-5-certification/)); the PPTT has no cache entries | Bring up A720s first, then A520s; use architectural registers over firmware tables |
| PCIe and SMMU state left by firmware | Firmware leaves the controllers "partially configured", and retraining destabilises the x2 link ([patch 0090](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0090-PCI-sky1-Add-MCFG-quirk-for-CIX-Sky1-Cadence-PCIe-co.patch)) | No link resets or ASPM; check SMMU state at ExitBootServices |
| SCMI channel contention | AML and OS both reach the power processor | One SCMI server owns each channel |
| GPU depth | Tyr still cannot submit jobs after a year of funded work; robust GPU recovery is unsolved even on Linux | Budget 10–20 FTE months; reuse Mesa unmodified; build reset handling early |
| Licence contamination and NDA terms | Key references are GPL; the TRM carries confidentiality terms | Clean-room rule; dual-licensed sources first; GPL only in separate servers |
| Nightly toolchain churn | build-std, branch protection and SVE/MTE intrinsics remain unstable ([build-std goal](https://rust-lang.github.io/goals/2026/build-std.html)) | Pin the nightly date; isolate unstable features in the nucleus; write hand-signed PAC prologues in asm |
| Vendor trajectory | CIX lists VPU, CSI, GMAC and system PM as "won't do" upstream ([cixtech wiki](https://github.com/cixtech/linux-mainline/wiki)) | Archive vendor trees and blobs locally; rely on standards wherever possible |

## The first month: back up, probe, boot

These steps protect the board, replace guesses with measurements, and give you a working loop before any real kernel code exists.

| Order | Action | Done when |
|---|---|---|
| 1 | Buy a CH347 or CH341A programmer with a 1.8 V adapter; dump the 8 MB W25Q64JW twice and compare hashes; keep the BIOS 1.3.1 `cix_flash_all.bin` ([Radxa BIOS](https://docs.radxa.com/en/orion/o6/low-level-dev/bios)) | Two identical dumps stored off-board |
| 2 | Run `acpidump` as root, then `iasl -d`; capture DSDT, IORT, PCCT, MADT and PPTT with ACPI SCMI both on and off, and diff them | Decompiled tables committed as private test fixtures |
| 3 | Write a UEFI probe app with the `uefi` crate that prints to GOP and UART2: `CurrentEL`, raw `ID_AA64*` registers (BTI, LPA2, VHE, NV, RAS, MPAM), `CLIDR`/`CCSIDR`, `GICD_IIDR`, `SMMU_IDR*`, EFI RNG presence, the SMCCC TRNG version, and an MTE `STG`/`LDG` round-trip | Every "unconfirmed" hardware fact in this report is answered |
| 4 | Wire USB-UART adapters to UART2, UART4 and UART5 and script logging | Firmware, PM and BootROM logs captured on every boot |
| 5 | Create the workspace: pinned nightly, `aarch64-unknown-none-softfloat`, `-Zbuild-std`, a `nucleus` crate holding all `unsafe`, an `xtask` runner for KVM, TCG and the board; MIT OR Apache-2.0 plus a written clean-room policy | `cargo xtask run kvm` boots a stub that prints and exits through PSCI |
| 6 | Get a second O6 (or another P1 board) on the bench with serial and a power relay | Unattended reboot-and-capture works |
| 7 | Read the LionsOS and Atmosphere papers, the Asterinas OSTD design, the CIX ACPI slides, and Managarm's and NetBSD's O6 bring-up code and logs | Short architecture decision records for IPC, capabilities, scheduling and the firmware-description layer |

## Conclusion

The Orion O6's ACPI mode changes the nature of the project. On a typical Arm SoC, a new kernel spends its first year reverse-engineering clocks, pins and PHYs. Here the firmware does that work for half the board, and the "SoC bring-up" problem shrinks to a PC-like one: parse tables, drive standard controllers, leave the firmware's state alone. The hard knowledge for the other half is not locked behind an NDA. It sits in readable GPL/MIT code and redistributable blobs, and the only truly closed doors are the ISP, the DSP, the security engine and the NPU compiler. So the binding constraint is your time, not access. That argues for sequencing by learning value and treating the accelerators as deep-dive projects you choose, not a checklist.

Two research questions on this board are unanswered anywhere. First: energy-aware scheduling across five firmware-binned capacity classes on a chip that idles at 16–17 W, with no published energy model. Second: whether capability-checked completion rings can give a microkernel io_uring-class throughput on two 5 GbE ports and NVMe without io_uring's security record. Either would make the OS more than a port: it would produce results nobody has measured on this silicon. Being the first Rust kernel on the CIX P1 is a reachable side effect of doing that work well.
