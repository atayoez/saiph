# CIX P1 (CD8180 / "Sky1") on the Radxa Orion O6: on-chip accelerators and multimedia blocks, and how drivable each is from a from-scratch Rust kernel

Research date: 2026-10-25. Sources are web pages and the vendor git trees as read today, plus read-only checks on the user's own Orion O6 (openSUSE Tumbleweed, Linux 7.2.11, UEFI+ACPI boot). Items marked **(local observation)** come from that board: `/sys/bus/acpi/devices`, `modinfo` and `/proc/config.gz`. They have no URL. The DSDT binary could not be dumped because sudo needed a password. The DSDT *ASL source* is public, though, and was used instead (see the HID map).

Main vendor sources used throughout:
- Official CIX mainline patch set (ACPI boot): https://github.com/cixtech/cix-linux-main. Directories `patches-6.18`, `patches-7.0` and `patches-7.3`; last push 2026-10-23.
- CIX DKMS drivers: https://github.com/cixtech/cix_opensource__vpu_driver (branch `cix_mainline_dev`), https://github.com/cixtech/cix_opensource__npu_driver (branch `cix_mainline_dev`), https://github.com/cixtech/cix_opensource__isp_driver (6.6 branches only).
- CIX closed userspace and firmware bundle: https://github.com/cixtech/cix_proprietary__cix_proprietary (branch `cix_p1_k6.6_master`).
- CIX ACPI table source (BSD-2-Clause-Patent): https://github.com/cixtech/cix_opensource__release__edk2-platforms/tree/cix_p1_dev/Platform/CIX/Sky1/Drivers/AcpiSocTables
- Community distro and patch tracker: https://github.com/Sky1-Linux

---

## Q0. What IP block is behind each CIX ACPI HID on the user's board?

### Takeaway
Almost every HID can be identified from the vendor's ACPI ASL sources and the `acpi_device_id` tables in CIX's patches. The multimedia blocks are as follows. GPU = CIXH5000. Display = Linlon-D60 DPUs (CIXH5010) driving Trilinear DP transmitters (CIXH502F). VPU = Linlon-V (CIXH3010). NPU = Zhouyi V3 (CIXH4000). ISP = Arm China "armcb" ISP (CIXH3021) plus a CSI-2 receiver (CIXH3026). Audio = an HDA controller (CIXH6020), Cadence I2S (CIXH6010/6011) and a Cadence HiFi5 DSP (CIXH6000). DMA = Arm DMA-350 (CIXH1006/CIXHA014). Every one of these mainline Linux drivers that exists is DT-only, which is why none binds under ACPI on 7.2.11.

### Cited Findings
- **(local observation)** These ACPI namespace paths are present and all unbound except TEE0:
  - GPU: `\_SB.GPU_` = CIXH5000 and `\_SB.GPUP` = CIXH5001.
  - NPU: `\_SB.NPU0` = CIXH4000, with child cores `NPU0.CRE0..CRE2` = CIXH4010 (three cores).
  - Video: `\_SB.VPU0` = CIXH3010.
  - Camera: `\_SB.V4L2` = CIXH3020, `ISP0` = CIXH3021, `ISP1..3` = CIXH3022, `ISPM` = CIXH3025, `VIHW` = CIXH3026. Under I2C: `I2C0.MTR0` = CIXH3023 and `I2C0.IIS0`/`I2C1.IIS1` = CIXH3024 (image sensors).
  - Display: `DPU0..4` = CIXH5010, `AEU0..4` = CIXH5011, `DP00..04` = CIXH502F, `VDP0..9` = CIXH503F, `EDP0` = CIXH5040, `DPBL` = CIXH5041, `CGFX` = CIXH5050, `LNCL` = CIXH50C0.
  - Audio: `DSP_` = CIXH6000, `I2S0..2` = CIXH6010, `I2S3..9` = CIXH6011, `HDA_` = CIXH6020, `ADSS` = CIXH6060 (with `ACLK` = CIXH6061 and `ARST` = CIXH6062), `SNDC` = CIXH6070.
  - DMA and timer: `DMA1` = CIXH1006, `DMA0` = CIXHA014, `TMR0` = CIXH1007.
  - Platform: `MBX0..7` = CIXHA001, `SCMI` = CIXHA006 (with `SCMI.DVFS` = CIXHA008 and `SCMI.CLKS` = CIXHA009), `CCLK` = CIXHA010, `PMA_` = CIXHA012, `PMGM` = CIXHA013, `EC0_` = CIXHA015, `UCRU/CRU0/GCRU/ACRU/DCRU` = CIXHA018, `PDC0` = CIXHA019, `RST0/RST1` = CIXHA020/21, `TEE0` = CIXHA022 (bound to optee), `TREE` = CIXHA023, `HWMN` = CIXHA024, `PEP0` = CIXHA026, `REST` = CIXA1019.
  - Low-speed peripherals: `GPI0..6` = CIXH1002/1003, `SPI0/1` = CIXH2001, `XSPI` = CIXH2002, `I2C0..7` = CIXH200B, `I3C0/1` = CIXH200C, `PWM0/1` = CIXH2011.
  - PCIe: `PRC0..4` = CIXH2020 (root complexes) and `PCP0..2` = CIXH2023 (PHYs).
  - USB: `SUB*/HUB*` = CIXH2030, `CUB*` = CIXH2031, `U2P*` = CIXH2032, `UCP0..3` = CIXH2033, `U3P4` = CIXH2034.
  - Ethernet: `MAC0/1` = CIXH7020.
- In CIX's 7.3 patches the `acpi_device_id` tables map the HIDs to drivers as follows ([cix-linux-main patches-7.3](https://github.com/cixtech/cix-linux-main/tree/main/patches-7.3)):

  | HID | Driver in CIX's 7.3 patches |
  |---|---|
  | CIXH5000 | Panthor (patches 00111/00112) |
  | CIXH5010 | `linlondp` |
  | CIXH502F | `cix,sky1-dptx` (Trilinear DP TX) |
  | CIXH503F | `cix,virtual-display` |
  | CIXH5040 | `cix-edp-panel` |
  | CIXH5008 | `cix,display` (this HID is not in the user's ACPI) |
  | CIXH1006 + CIXHA014 | `arm-dma350` (patch 0016) |
  | CIXH6020 | HDA `cix-ipbloq` (patch 0013) |
  | CIXH6000 | `cix_dsp_rproc` (patch 0009) |
  | CIXH6010 | Cadence I2S "sc" (`cdns_i2s_sc.c`) |
  | CIXH6011 | Cadence I2S "mc" (`cdns_i2s_mc.c`) |
  | CIXH6070 | `sky1-card` machine driver (patch 00220) |
- CIXH3010 is matched by the VPU `mvx` driver: [vpu driver mvx_dev.c](https://github.com/cixtech/cix_opensource__vpu_driver/tree/cix_mainline_dev).
- CIXH3021 (`armcb,sky1-isp`) and CIXH3026 (`cix,cix-vi-hw`) are matched by the ISP driver: [isp driver](https://github.com/cixtech/cix_opensource__isp_driver).
- The vendor ASL sources add detail ([AcpiSocTables](https://github.com/cixtech/cix_opensource__release__edk2-platforms/tree/cix_p1_dev/Platform/CIX/Sky1/Drivers/AcpiSocTables), files Dsdt-Dpu/GPU/NPU/Vpu/ISP/Dsp/HDA/Dma/Audss.asl):
  - LNCL (CIXH50C0) is a "Linlon display cluster" whose `_DSD` lists `\_SB.DPU0..DPU4`.
  - CGFX (CIXH5050) carries a `_DEP` on PEP0 and the DP devices. It is probably a Windows-style aggregate graphics node (inference).
  - DPBL (CIXH5041) is the "DP2 backlight".
  - The AEU nodes (CIXH5011) return `_STA = 0` (disabled).
  - CIXH5001 GPUP is the "GPU power supply".
  - CIXHA012 PMA is the GPU reserved-memory region.
  - CIXA1019 REST is a "resource table" that serves as a reserved-memory and reset lookup table.
  - CIXH2034 is a USB 3.0 PHY at 0x09210000.
  - PEP0 is the "Power Engine Plugin".
  - DCRU (CIXHA018) sits at 0x07110000 and is referenced by the DSP.
- CIX's ACPI design reuses the Linux DT drivers. The private `_HID`s ("CIXH3010" for the VPU, "CIXH5000" for the GPU, "CIXH4000" for the NPU) carry DT-style properties in `_DSD`. Power gating is done through ACPI `PowerResource` objects whose `_ON`/`_OFF` methods poke RCSU PGFSM registers, and devices reference them through `_PR0`/`_PR3`. Clocks go through "AML-based SCMI" — [CIX ACPI slides "ACPI support on Radxa Orion O6" (F. Duan, J. Zhou et al., CIX)](https://www.kitefor.events/download/410)
- **(local observation)** Mainline 7.2.11 modules exist, but their aliases are OF-only:
  - `panthor`: `arm,mali-valhall-csf`, `rockchip,rk3588-mali`, `mediatek,mt8196-mali`
  - `komeda`: `armchina,linlon-d6`, `arm,mali-d71`, `arm,mali-d32`
  - `snd-hda-cix-ipbloq`: `cix,sky1-ipbloq-hda`
  - `arm-dma350`: `arm,dma-350`
  - `cix-mailbox`: `cix,sky1-mbox`
  - pinctrl-sky1 and pci-sky1 are also OF-only.

  None has an ACPI ID, which explains why nothing binds on the user's ACPI-booted system.
- One community table is wrong. The README of [Sky1-Linux/acpi-to-dts-tools](https://github.com/Sky1-Linux/acpi-to-dts-tools) maps CIXH4000 to `arm,mali-valhall-csf` (GPU) and CIXH5010 to `cix,sky1-dwc3` (USB). Both the user's ACPI namespace and the vendor patches contradict this: CIXH4000 = NPU and CIXH5010 = DPU. Treat that README as unreliable.

### Inferences
- For a new OS, CIX's ACPI ASL sources (BSD-2-Clause-Patent) plus the Linux `_DSD` property names amount to a machine-readable hardware description. The OS can parse its own DSDT for MMIO, IRQs and properties instead of hard-coding them. It will, however, need an AML interpreter, because power resources and clock methods are AML. Alternatively it can boot with the community DT (`sky1-orion-o6.dtb`) and skip AML.

### Gaps
- The MMIO base addresses come from `#define`s that are not in the AcpiSocTables directory, so most were not recorded here. GPU addresses are known (see Q1). The rest can be read from the user's DSDT with root (`acpidump` / `iasl -d`).
- The functions of CIXHA015 EC0, CIXHA024 HWMN and CIXHA023 TREE were not checked in detail.

---

## Q1. GPU: Arm Immortalis-G720 MC10 (Mali, 5th-gen architecture "v12"). Driver status (Panthor, Tyr, kbase), power and clocks on Sky1, Mesa, and what a Rust kernel must implement

### Takeaway
- **Hardware:** Arm Immortalis-G720 MC10, a 10-core GPU of architecture major 12 (CSF firmware `arch12.8`) with hardware ray tracing.
- **Panthor (C, mainline):** recognises the G720 and is in the user's 7.2.11 kernel. On Sky1 it works only with DT boot plus patches, or with CIX's July 2026 ACPI patches.
- **Tyr (Rust, mainline):** as of v7.3 (October 2026) it can only boot the MCU and supports only the G610 officially. It cannot yet submit jobs.
- **Mesa:** Panfrost (GL ES) and PanVK (Vulkan 1.4) list the G720, with no conformance claim.
- **For a Rust kernel:** the GPU is drivable from open code, using Panthor plus the Panthor uAPI, so Mesa can be reused. It is still the largest single driver effort on the board. The Sky1 power-on sequence is now fully documented in CIX's GPL/MIT patch.

### Cited Findings
- **Product identity:** "Arm® Immortals™ G720 MC10", hardware ray tracing, vendor-stack APIs Vulkan 1.3 / OpenGL ES 3.2 / OpenCL 3.0 — [Radxa Orion O6 Product Brief v1.2](https://dl.radxa.com/orion/o6/docs/radxa_orion_o6_product_brief.pdf). CIX's slides also list "OpenGL 4.0" — [CIX ACPI slides](https://www.kitefor.events/download/410)
- **Measured values on Sky1 (Orange Pi 6 Plus, same SoC)** — [visorcraft/orange-pi-6-plus-gpu](https://github.com/visorcraft/orange-pi-6-plus-gpu):
  - Configuration: "Mali-G720 Immortalis MC10", 10 shader cores (`shader_present=0x550555`), 1 L2 slice, 1 tiler.
  - Identity and placement: GPU ID `0xc870`, GPU MMIO at `0x15010000`, RCSU at `0x15000000`.
  - Clocks: SCMI core clock about 1 GHz, top clock 900 MHz, and 6 DVFS OPPs from the SCMI perf domain.
- **Architecture generation:** Mesa classifies the G720 as "5th Gen (v12)" and the G725 as v13 — [Mesa Panfrost docs](https://docs.mesa3d.org/drivers/panfrost.html). Mainline `panthor_hw.c` maps `GPU_PROD_ID_MAKE(12, 0)` to "Mali-G720-Immortalis" / "Mali-G720". The mainline tree on 2026-10-25 is at `VERSION = 7` / `PATCHLEVEL = 3` — [torvalds/linux panthor_hw.c](https://github.com/torvalds/linux/blob/master/drivers/gpu/drm/panthor/panthor_hw.c)
- **CSF firmware:** Panthor loads `arm/mali/arch12.8/mali_csffw.bin` for this GPU — [visorcraft](https://github.com/visorcraft/orange-pi-6-plus-gpu). **(local observation)** `modinfo panthor` on 7.2.11 lists firmware for arch10.8, 10.10, 10.12, 11.8, 12.8, 13.8 and 14.8. The openSUSE package `kernel-firmware-platform-20260717` ships `/lib/firmware/arm/mali/arch12.8/mali_csffw.bin.xz`.
- **Firmware licence:** **(local observation)** `LICENCE.mali_csffw` in linux-firmware is an Arm "END USER LICENCE AGREEMENT FOR THE MALI FIRMWARE". It grants use, and redistribution of the whole Software, "solely in conjunction with Mali-based products". The firmware is closed but redistributable for a hobby OS on Mali hardware.
- **GPU MMU format:** Panthor programs `AS_TRANSCFG_ADRMODE_AARCH64_4K` and allocates page tables with `alloc_io_pgtable_ops(ARM_64_LPAE_S1, ...)`. These are standard AArch64 stage-1 4 KB-granule LPAE tables — [torvalds/linux panthor_mmu.c](https://github.com/torvalds/linux/blob/master/drivers/gpu/drm/panthor/panthor_mmu.c)
- **Panthor G720 upstreaming history:** Arm's Karunika Choo posted G715 / G720 / G725 / G710 / G510 / G310 support in March 2025 (v2) and again in July 2025 — [Phoronix: Arm Mali newer hardware for Panthor](https://www.phoronix.com/news/Arm-Mali-Newer-Hardware-Panthor), [Phoronix: G725 and friends](https://www.phoronix.com/news/Arm-Mali-G725-Friends-Panthor), [Radxa forum thread](https://forum.radxa.com/t/support-for-the-mali-g720-coming-to-panthor-kernel-driver/26391). Visorcraft runs Panthor on a 6.18.9 / 6.19-rc base — [visorcraft](https://github.com/visorcraft/orange-pi-6-plus-gpu). One search summary claimed Panthor has supported the G720 "since 6.10"; that is likely wrong, since 6.10 supported only the G610. The exact merge version (probably 6.17 or 6.18) was not confirmed.
- **Why Panthor does not bind under ACPI:** the mainline driver matches only DT. Under ACPI boot, the SCMI POWER_STATE_SET for domain 21 (`gpu_pd`) "returns success but does nothing in hardware". The power-enable register at `0x15000014` is "firewalled from EL1"; only TF-A at EL3 can write it. GPU reset is SRC register `0x16000400` bit 6, and the ACPI reset path (`CIXHA020`) fails with `-ENOENT` because the vendor RSTL binding is not in mainline — [visorcraft](https://github.com/visorcraft/orange-pi-6-plus-gpu)
- **CIX's own ACPI Panthor patches** (Jianfeng Liu, dated 2026-07-10, `panthor_sky1.c` licensed "GPL-2.0 or MIT", ~350 lines changed) — [00112-drm-panthor-Sky1-CIXH5000-ACPI-bring-up.patch](https://github.com/cixtech/cix-linux-main/blob/main/patches-7.3/00112-drm-panthor-Sky1-CIXH5000-ACPI-bring-up.patch):
  - **Power-on:** a raw SMC SCMI call. Function ID `0xc2000001` with shared memory at physical `0x84380000` (0x80 bytes) sends POWER protocol `0x11`, message `0x04` POWER_STATE_SET, domain `21`, state 0. TF-A also "configures the IDM ... to allow non-secure (Linux) access to GPU registers". Without that call, "any GPU register read causes an unrecoverable SError".
  - **Resources:** resource 0 = RCSU, resource 1 = GPU registers. ACPI IRQ index 0 = job, 2 = gpu (and 1 = mmu per the ASL order).
  - **Clock names:** `gpu_clk_core` and `gpu_clk_stacks`.
  - **Sequence "from CIX GPU Development Guide":** power domain on → clock enable → IP reset assert/deassert → Q-channel clock gating enable (RCSU offset `0x218`, bit 0).
- **Related CIX patches:**
  - Coherency: the Sky1 GPU is "RN-I on the CHI fabric" and uses ACE-Lite coherency mode — [00114](https://github.com/cixtech/cix-linux-main/blob/main/patches-7.3/00114-drm-panthor-rework-bus-coherency-to-ACE-Lite-ACE-NON.patch)
  - DVFS goes through an SCMI performance domain ("perf" genpd) — [00116](https://github.com/cixtech/cix-linux-main/blob/main/patches-7.3/00116-drm-panthor-SCMI-DVFS-via-the-perf-genpd-on-Sky1.patch)
  - In the ACPI ASL the GPU `_DSD` uses `power-domains = \_SB.SCMI.DVFS` with name "perf" — [Dsdt-GPU.asl](https://github.com/cixtech/cix_opensource__release__edk2-platforms/tree/cix_p1_dev/Platform/CIX/Sky1/Drivers/AcpiSocTables)
- **CIX support status:** the cix-linux-main README marks the GPU "Supported" with Panthor; GPU firmware comes from upstream linux-firmware; "Mesa has supported panthor since v25.1"; the firmware must be set to ACPI; `clk_ignore_unused` is required — [cix-linux-main](https://github.com/cixtech/cix-linux-main). Sky1-Linux lists "GPU init + DVFS" at ~500 lines, "Not submitted" to mainline as of v6.19 — [Sky1-Linux](https://github.com/Sky1-Linux)
- **Vendor DDK:** the closed Arm "kbase" stack is still shipped. Kernel side: `mali_kbase` **r54p1** (`MALI_RELEASE_NAME ?= "r54p1-11eac0"`) — [cix_opensource__gpu_kernel](https://github.com/cixtech/cix_opensource__gpu_kernel). Userspace: `cix-gpu-umd`, about 280 MB of closed libraries — [cix_proprietary](https://github.com/cixtech/cix_proprietary__cix_proprietary). There is also a DKMS packaging of kbase — [Sky1-Linux/cix-gpu-kmd](https://github.com/Sky1-Linux/cix-gpu-kmd)
- **Mesa:** the G720 row is "5th Gen (v12)" with OpenGL ES 3.1, OpenGL 3.1, Vulkan 1.4 and OpenCL 3.0. Conformance submissions are listed only for G52, G57, G310 and G610, not for the G720. PanVK refuses to load on GPUs where support is experimental unless `PAN_I_WANT_A_BROKEN_VULKAN_DRIVER=1` is set — [Mesa Panfrost docs](https://docs.mesa3d.org/drivers/panfrost.html)
- **Measured on Sky1 (2026-02):** Mesa 25.3.3 gives OpenGL ES 3.1 (Panfrost) and Vulkan 1.4.328 (PanVK), labelled "non-conformant". glmark2 scored 3079 on a 2026-02-13 rerun. Problems seen: PanVK compute is unstable for large workloads (OOM / DeviceLost), and SIGKILL of a GPU process can cause a Panthor MCU reset failure — [visorcraft](https://github.com/visorcraft/orange-pi-6-plus-gpu). Sky1-Linux maintains a Mesa fork "with PanVK fixes for Mali-G720/Panthor" — [Sky1-Linux](https://github.com/Sky1-Linux)
- **Tyr (Rust) upstream timeline** — [Collabora, "Upstreaming Tyr", 2026-10-21](https://www.collabora.com/news-and-blog/blog/2026/10/21/upstreaming-tyr-rust-drm-and-the-linux-kernel/):
  - v6.18 (Nov 2025): stub that probes, powers on, queries hardware info and shuts down.
  - v7.0 (Apr 2026): Rust page-table abstraction.
  - v7.1 (Jun 2026): GEM SHMEM helpers.
  - v7.2 (Aug 2026): GPUVM abstraction.
  - v7.3: Tyr boots the MCU.
  - Size: "approximately 3,700 lines", with six unsafe lines.
  - Hardware: only the Mali-G610 (Valhall v10, CSF) is named.
  - Missing: job scheduling (stalled on abstractions), CSF queues/groups, and power management.
- **Tyr in the mainline tree:** files `driver.rs file.rs fw.rs fw/ gem.rs gpu.rs mmu.rs mmu/ regs.rs slot.rs tyr.rs vm.rs` (~143 KB at top level). The OF table matches `rockchip,rk3588-mali` and the generic `arm,mali-valhall-csf` — [torvalds/linux drivers/gpu/drm/tyr](https://github.com/torvalds/linux/tree/master/drivers/gpu/drm/tyr). **(local observation)** The openSUSE 7.2.11 kernel has no Tyr module (`CONFIG_RUSTC_VERSION=0`, Rust not enabled).
- **Tyr architecture and plans** — [LWN "The future for Tyr", 2026-02-03](https://lwn.net/Articles/1055590/):
  - A downstream prototype (Arm, Collabora, Google) runs desktops and games.
  - Job submission is moving from `drm_gpu_scheduler` to a new Rust `JobQueue` that only resolves dependencies before handing work to the firmware scheduler.
  - Required abstractions: GEM shmem, GPUVM/io-pgtable, `drm::DeviceCtx`, the `register!` macro from Nova, and fence-signalling rules.
  - Goals: Vulkan CTS so PanVK runs unchanged; power management and recovery are listed as hard requirements.
  - Earlier: the Tyr prototype ran GNOME, Weston and SuperTuxKart — [Phoronix](https://www.phoronix.com/news/Tyr-Begins-Handling-Work), [Phoronix 6.18](https://www.phoronix.com/news/Rust-DRM-Drivers-Linux-6.18-Tyr)
- **MCU boot sequence used by Tyr:** write `UPDATE` to the address-space `COMMAND` register to activate the configured AS, then write `MCU_CONTROL` to start the MCU. Firmware sections are placed in GEM buffers mapped into a GPU VA space sized to the MMU — [Collabora 2026-10-21](https://www.collabora.com/news-and-blog/blog/2026/10/21/upstreaming-tyr-rust-drm-and-the-linux-kernel/)
- **Non-Linux precedent on Sky1:** Panthor has been ported to FreeBSD via LinuxKPI and "brought up on the CIX Sky1 (Orange Pi 6 Plus)" — [JamesKane/drm-panthor-kmod](https://github.com/JamesKane/drm-panthor-kmod). Sway runs with Panthor rendering into buffers that komeda scans out directly — [JamesKane/drm-komeda-kmod](https://github.com/JamesKane/drm-komeda-kmod)
- **Documentation:** CIX published "CIX-P1-Linux GPU Development Guide-V1.0.pdf" (25 pages), covering the proprietary Arm driver and Panfrost/Panthor configuration. The TRM Part 1 (6,179 pages) covers the GPU at SoC-integration level. The TRM needs registration with manual approval, and Radxa's mirror was removed at CIX's request — [CNX Software 2025-12-13](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-developer-guides-for-gpu-ai-accelerator-os-and-firmware-bios/). Arm does not publish a public Mali register manual; Panthor and Mesa are the de facto documentation (common knowledge, not verified this session).

### Inferences
- **What a from-scratch Rust kernel must implement for the GPU**, following the Panthor and Tyr structure:
  1. Sky1 platform power-on: the SMC SCMI POWER_STATE_SET (domain 21) to TF-A, which also opens IDM access; SCMI clock enable; reset pulse; Q-channel gating.
  2. GPU control registers: read GPU_ID and features, power up L2 and shader cores, handle IRQs (job / mmu / gpu).
  3. MMU: AArch64 4K LPAE stage-1 page tables, address spaces, AS commands (UPDATE, FLUSH, LOCK), and page-fault handling (growable heap for the tiler).
  4. CSF firmware loader: parse the `mali_csffw.bin` sections, map them into the MCU address space, boot the MCU, and set up the global interface (doorbells, ping/watchdog).
  5. CSF scheduling: create command-stream groups and queues with ring buffers, run a group-priority scheduler with timeslicing and residency limits (the firmware has a limited number of CSG slots), and handle sync objects and fences.
  6. Power: DVFS through SCMI perf (domain per the ASL), plus suspend/resume.
  7. A userspace ABI. Implementing the Linux Panthor uAPI (DRM ioctls VM_CREATE/VM_BIND, BO_CREATE, GROUP_CREATE, GROUP_SUBMIT, syncobjs) would let unmodified Mesa Panfrost/PanVK run. That also requires a libdrm-compatible device-node model and dma-buf-like sharing with display.
- **Effort:** Panthor is roughly 520 KB of C (~15k lines; estimate from mainline file sizes, [torvalds/linux panthor](https://github.com/torvalds/linux/tree/master/drivers/gpu/drm/panthor)). Tyr achieved MCU boot in about 3.7k lines of Rust. A hobby Rust kernel reaching "Mesa renders a triangle" is a multi-month to year-scale project.
- **Difficulty: very hard, but unblocked.** All required knowledge is open (GPL/MIT code, a redistributable firmware blob). Nothing is effectively closed except the firmware internals. Tyr's crates, the io-pgtable approach, and Asahi's Rust AGX driver (not researched here) are the closest Rust references.

### Gaps
- The exact mainline version that added G720 (arch 12) support to Panthor was not confirmed.
- Not confirmed: whether the G720 is on PanVK's "experimental" list in current Mesa; any Vulkan or GLES CTS submission for the G720; and whether ray-tracing extensions are exposed by PanVK.
- The GPU MHz/OPP table comes from a community measurement. No official clock figure from CIX was found.
- It was not checked whether CIX's ACPI Panthor patches have been posted to dri-devel. They carry "Co-Authored-By: Claude" tags and are dated July 2026; no lore thread was found.

---

## Q2. Display: controller IP, pipes, outputs and PHYs, Linux driver status, and the UEFI GOP framebuffer

### Takeaway
- **IP:** five Arm China **Linlon-D60** DPUs (Linlon-D6 family, register-compatible with Arm Mali-D71 for basic modesetting, two pipelines each). Each drives one CIX-integrated **Trilinear Technologies DP 1.4 TX** with DSC and MST support. HDMI on boards is a DP-to-HDMI conversion (confirmed on the OPi 6 Plus).
- **Mainline:** has Linlon-D6 support in komeda (v7.1), DT only. The full CIX stack (`linlondp` + `trilin_dptx` + PHYs + HDCP + MST, ~27.8k lines) is out of tree.
- **Early display:** a UEFI GOP is provided by closed EDK2 DXE binaries, so for early boot the GOP linear framebuffer is the trivial path.

### Cited Findings
- **Board outputs:** 1x USB-C with DP Alt Mode (4K@60), 1x HDMI 2.0 (4K@60, "HDMI CEC not supported"), 1x DisplayPort 1.4 (4K@120, MST, dual-stream), and 1x eDP connector (up to 4Kp60, touch-panel interface), with "concurrent quad-display operation" — [Radxa Orion O6 Product Brief v1.2](https://dl.radxa.com/orion/o6/docs/radxa_orion_o6_product_brief.pdf), [CIX ACPI slides](https://www.kitefor.events/download/410)
- **Linlon-D6 in komeda:** Arm China Linlon-D6 support was added to komeda by Cunyuan Liu (cixtech.com). v1 was posted 2026-03-11 and v2 on 2026-03-13. The compatible is `armchina,linlon-d6` with fallback `arm,mali-d71`, described as "register-compatible with Mali-D71" for basic modesetting. `LINLONDP_D6_PRODUCT_ID` = `0x0060`. Reviews: Rob Herring Acked-by (vendor prefix), Liviu Dudau Reviewed-by — [dri-devel v2 thread](https://lore.gitlab.freedesktop.org/drm-ai-reviews/20260313033119.33686-2-cunyuan.liu@cixtech.com/T/). This landed in Linux v7.1 (commit 8fd7576fd6d8) — [JamesKane/drm-komeda-kmod](https://github.com/JamesKane/drm-komeda-kmod). **(local observation)** The 7.2.11 `komeda.ko` alias list includes `armchina,linlon-d6`.
- **Sky1 display topology:** five Linlon-D60 controllers described in ACPI. On FreeBSD, the controllers bind to `CIXH5010`, the DP transmitters to `CIXH502F` and the PHYs to `CIXH2033`. Only DPU4 → DP04 → UCP3 PHY (the OPi6+ HDMI port through a Parade PS185 DP-to-HDMI converter) is wired up, at standard modes up to 1920x1080. Not working: DPU0–3, EDID through the PS185 (which also fails on Linux), DP audio, cursor plane, IOMMU and runtime PM — [JamesKane/drm-komeda-kmod](https://github.com/JamesKane/drm-komeda-kmod)
- **Per-DPU description in ACPI:** two pipelines (`pipeline@0`, `pipeline@1`), `aclk_freq_fixed` 800000000, `cix,linlon-dpu-slave`, `enabled_by_gop` (0 in the template), and many AXI QoS/cache properties. Each DP node has four MMIO regions (DP controller, DSC, DP PHY, RCSU) plus `dp_phy`, `edp-panel` and `cfg_adapter_port` properties and an OF-graph to the DPU — [Dsdt-Dpu.asl](https://github.com/cixtech/cix_opensource__release__edk2-platforms/tree/cix_p1_dev/Platform/CIX/Sky1/Drivers/AcpiSocTables)
- **CIX's out-of-tree display driver:** patch `00100-drm-add-cix-linlon-dp-driver.patch` (2026-05-07, "78 files changed, 27775 insertions") adds `drivers/gpu/drm/cix/`, plus 163 KB of 2026Q2 updates — [cix-linux-main patches-7.3](https://github.com/cixtech/cix-linux-main/tree/main/patches-7.3). It contains:
  - `linlon-dp/` (DPU KMS, pipeline, planes, writeback, colour management; compatibles `armchina,linlon-d8`, `-d6`, `-d2`; ACPI CIXH5010)
  - `dptx/trilin_dptx.c` (3,406 lines), `trilin_drm_mst.c`, `trilin_dptx_audio.c`
  - `trilin_usbdp_phy.c` and `trilin_edp_phy.c`
  - HDCP 2.x (`hdcp2_*`)
  - `cix_edp_panel.c` (CIXH5040) and `cix_virtual.c` (CIXH503F)
- **Kernel configuration names:** `DRM_CIX`, `DRM_LINLONDP` ("Linlon Display Processor"), `DRM_TRILIN_DP_CIX` and `DRM_TRILIN_DPSUB` (DP TX subsystem) — [visorcraft](https://github.com/visorcraft/orange-pi-6-plus-gpu), [Sky1-Linux config README](https://github.com/Sky1-Linux/linux-sky1/blob/main/config/README.md)
- **Status per CIX** (2026-10): USB-C DP Alt Mode works; DP and HDMI are supported; all three have issues rendering VPU-decoded video through GL — [cix-linux-main README](https://github.com/cixtech/cix-linux-main). Sky1-Linux lists Display (DP/HDMI) at ~27,000 lines and USB-C (PD + DP Alt Mode) at ~5,000 lines, both "Not submitted" as of v6.19 — [Sky1-Linux](https://github.com/Sky1-Linux)
- **USB-C:** a CIX patch adds an `rts5453` Type-C/PD controller driver (Realtek RTS5453) — [0026-typec-add-rts5453-driver.patch](https://github.com/cixtech/cix-linux-main/tree/main/patches-7.3)
- **UEFI GOP:** CIX's BIOS feature list includes "Secure boot, GOP display support" — [CIX ACPI slides](https://www.kitefor.events/download/410). The GOP is implemented by **closed binary** DXE drivers `Silicon/CIX/Sky1/Drivers/DpuDxe/DpuDxe.efi` (57 KB) and `LcdGraphicsOutputDxe.efi` (33 KB) in edk2-non-osi — [cix edk2-non-osi](https://github.com/cixtech/cix_opensource__release__edk2-non-osi/tree/cix_p1_dev/Silicon/CIX/Sky1/Drivers). edk2-platforms also ships an AMD Arm64 GOP binary (`Arm64Gop_1_68.efi`) for dGPUs — [cix edk2-platforms](https://github.com/cixtech/cix_opensource__release__edk2-platforms/tree/cix_p1_dev/Platform/CIX/Sky1/Drivers)
- **Documentation:** TRM Part 1 covers "DPU" and Part 2 covers "DP" and "MIPI". It is registration-gated — [CNX Software](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-developer-guides-for-gpu-ai-accelerator-os-and-firmware-bios/)

### Inferences
- **Early display:** use the UEFI GOP framebuffer, with `ExitBootServices` keeping the mode. This is the obvious first step and needs zero DPU or DP code. The `enabled_by_gop` property suggests the Linux driver can inherit a GOP-lit pipeline.
- **Native mode-setting** needs three pieces:
  - A Linlon-D60 (D71-like) DPU driver. Komeda's D71 back end is ~250 KB of C in mainline; [komeda](https://github.com/torvalds/linux/tree/master/drivers/gpu/drm/arm/display/komeda) is a good reference because Arm's Mali-D71 is register-compatible.
  - The Trilinear DP TX with AUX, link training, EDID and optional MST/DSC.
  - The USB-C/DP combo PHY and eDP PHY drivers, plus SCMI clocks and resets.
- **Difficulty:** medium-hard for one DP output at SST without DSC. Hard for all four outputs with MST, HDCP and Alt Mode, which needs a PD controller driver plus DP Alt Mode negotiation.
- **Likely HDMI path on the O6:** the SoC exposes five DP controllers and no HDMI TX was found, so the Orion O6 HDMI 2.0 port is very likely also a DP-to-HDMI converter. This is unconfirmed for the O6; the PS185 is confirmed only on the OPi6+.

### Gaps
- Not verified: which O6 physical connector maps to which DPU/DP index; which converter chip (if any) drives the O6 HDMI port; and which outputs the GOP lights up (HDMI only, or all).
- No public Linlon-D60 or Trilinear DP register manual was found outside the gated TRM.
- The maximum resolution per pipe is unclear. `XRES_MAX`/`YRES_MAX` 4096 appears in CIX code (the virtual display), while the board brief claims DP 4K@120.

---

## Q3. VPU (video codec): IP, codecs, firmware, V4L2 driver status

### Takeaway
The VPU is an Arm China **Linlon-V** codec, the lineage of Arm Mali-V (MVE). It has four cores and does 8K60 decode and 8K30 encode. It is run through firmware (`*.fwb` per codec) by a ~39k-line out-of-tree GPL V4L2 driver (`mvx`). The driver source and the firmware message-protocol headers are open, but the firmware is closed. For an independent OS it is feasible, with medium-hard difficulty.

### Cited Findings
- **Capabilities:**
  - Decode up to 8K@60fps: AV1, H.265, H.264, VP9, VP8, H.263, MPEG-4, MPEG-2.
  - Encode up to 8K@30fps: H.265, H.264, VP9, VP8.
  - Source: [Radxa Orion O6 Product Brief](https://dl.radxa.com/orion/o6/docs/radxa_orion_o6_product_brief.pdf)
- **Driver identity** — [cix_opensource__vpu_driver (cix_mainline_dev)](https://github.com/cixtech/cix_opensource__vpu_driver/tree/cix_mainline_dev):
  - Kconfig `VIDEO_LINLON` ("Linlon VPU family").
  - OF compatibles `arm,mali-mve`, `arm,mali-v500`, `-v550`, `-v61`, `armChina,linlon-v5`, `-v6`, `-v7`, `-v8`; ACPI `CIXH3010`.
  - Hardware-revision enum `MVE_v500`, `MVE_v550`, `MVE_v61`, `MVE_v52_v76`.
  - Firmware protocol headers `external/fw_v2/` and `fw_v3/mve_protocol_def.h` (~95 KB each).
  - V4L2 M2M front end plus a session, scheduler and MMU layer.
- **Licence and packaging:** debian/copyright says "Copyright: 2026 Cix Technology Group Co., Ltd. License: GPL-2+" for `*`. The same repo ships `firmware-binaries/*.fwb` with no separate firmware licence, so the blob terms are ambiguous — [VPU repo](https://github.com/cixtech/cix_opensource__vpu_driver/tree/cix_mainline_dev)
- **Firmware files:**
  - Encode and decode: `h264dec/enc`, `hevcdec/enc`, `vp8dec/enc`, `vp9dec/enc`, `jpegdec/enc`.
  - Decode only: `av1dec`, `mpeg2dec`, `mpeg4dec`, `vc1dec`, `avsdec`, `avs2dec`.
  - Each `.fwb` is about 270–490 KB.
  - Sources: [VPU repo](https://github.com/cixtech/cix_opensource__vpu_driver/tree/cix_mainline_dev), [Sky1-Linux/sky1-firmware](https://github.com/Sky1-Linux/sky1-firmware). Sky1-Linux calls them "proprietary and redistributable" from CIX Technology without naming a licence text.
- **ACPI:** VPU0 (CIXH3010) has two MMIO windows, one IRQ, SCMI perf domain 9 ("perf"), and four child core devices CRE0–CRE3 — [Dsdt-Vpu.asl](https://github.com/cixtech/cix_opensource__release__edk2-platforms/tree/cix_p1_dev/Platform/CIX/Sky1/Drivers/AcpiSocTables)
- **Status:** CIX says "VPU and NPU: supported through external DKMS modules" for its 6.18 / 7.0 / 7.3 patch sets — [cix-linux-main](https://github.com/cixtech/cix-linux-main). Sky1-Linux lists the VPU at ~39,000 lines, "Not submitted" to mainline — [Sky1-Linux](https://github.com/Sky1-Linux). Community userspace: FFmpeg 8.0 with V4L2-M2M patches, GStreamer `v4l2av1dec`, and Firefox/Chromium V4L2-M2M decode — [Sky1-Linux](https://github.com/Sky1-Linux)
- **Closed userspace:** the vendor `cix-vpu-umd` package in the proprietary repo bundles the firmware — [cix_proprietary](https://github.com/cixtech/cix_proprietary__cix_proprietary). CIX also publishes `cix_libva`, `cix_vaapi`, `cix_ffmpeg`, `cix_gstreamer` and `cix_media_engine` repositories (contents not inspected) — [cixtech GitHub org](https://github.com/cixtech)
- **Not this driver:** **(local observation)** mainline 7.2.11 ships `hantro-vpu` (VeriSilicon) and `mali-c55` (an Arm ISP), but neither is used by Sky1. There is no Linlon-V/MVE driver in mainline.

### Inferences
- An independent OS needs to:
  1. Load the closed `.fwb` into VPU memory through the VPU's own MMU.
  2. Implement the MVE host-firmware message queues, using `mve_protocol_def.h` as the spec.
  3. Do buffer and frame management.
  4. Expose its own decode API. This could be V4L2-M2M-compatible so ported FFmpeg works.
- The open GPL driver is effectively the documentation. **Difficulty: medium-hard.** Bitstream parsing happens in firmware, so it is easier than a stateless decoder. The blob licence should be clarified before redistribution.

### Gaps
- The exact Linlon-V model in the CIX P1 (V7? V8?) was not confirmed. The code supports V5–V8 compatibles; the HID path does not reveal the model.
- No public register documentation exists beyond the gated TRM ("VPU" in Part 1). No reverse-engineering effort was found.

---

## Q4. Camera and ISP: MIPI CSI-2 inputs, ISP IP, driver status

### Takeaway
- **Inputs:** two MIPI CSI-2 camera ports (4-lane or 2-lane each).
- **Capture path:** a CIX CSI receiver/DMA ("VIHW") feeds an Arm China ISP that the driver calls "armcb", with I5/I7 register layouts. The kernel driver (GPL, 6.6 only) is a thin V4L2 shim.
- **Closed parts:** the real ISP programming is done by a closed userspace daemon (`isp_app`), a 25 MB `DDR0.IMG` and per-sensor calibration and tuning files.
- **Verdict:** raw CSI capture is plausible for an independent OS. ISP processing is effectively blocked.

### Cited Findings
- **Board:** "Dual MIPI Camera Ports ... Configurable as 4-lane or 2-lane MIPI CSI each" — [Radxa Product Brief](https://dl.radxa.com/orion/o6/docs/radxa_orion_o6_product_brief.pdf). CIX slides list "CSI, CSI-DMA and ISP ACPI driver support, Gstreamer support camera capture and preview" — [CIX ACPI slides](https://www.kitefor.events/download/410)
- **ACPI:**
  - `V4L2` (CIXH3020) has ISP interrupts.
  - `ISP0` (CIXH3021) has two MMIO windows, and `ISP1..3` are CIXH3022.
  - `ISPM` (CIXH3025) is "ISP ispmem".
  - `VIHW` (CIXH3026) has eight CSI IRQs, a normal and an error IRQ for each of four CSI instances.
  - There is a separate `Dsdt-CSI-DMA.asl`.
  - Source: [AcpiSocTables](https://github.com/cixtech/cix_opensource__release__edk2-platforms/tree/cix_p1_dev/Platform/CIX/Sky1/Drivers/AcpiSocTables)
- **Kernel driver** — [cix_opensource__isp_driver](https://github.com/cixtech/cix_opensource__isp_driver):
  - Branches are 6.6 only (`cix_p1_k6.6_master`, `cix_k6.6.89_2026q3`, ...). The repo has 62 files and ~460 KB, all `SPDX: GPL-2.0`.
  - Compatibles `armcb,sky1-isp` / `armcb,isp` (ACPI CIXH3021) and `cix,cix-vi-hw` (CIXH3026).
  - Register headers define `I5_*` and `I7_*` layouts (VIN DAW/DAR, 3A and VOUT0–8 offsets).
  - The ISP subdev ioctl applies a userspace `cmd_buf` (`armcb_isp_hw_apply`), meaning userspace supplies the register programming.
  - Remnants of a Xilinx `XC7Z020` FPGA development setup remain in `armcb_register.h`.
- **Closed userspace** — [cix_proprietary](https://github.com/cixtech/cix_proprietary__cix_proprietary):
  - `cix-isp-umd` contains `isp-daemon.service`, `isp_app` (6.4 MB) and `DDR0.IMG` (25.9 MB).
  - Sensor calibration and tuning files include `imx219.cal`, `327*.tun` (likely IMX327) and `dummy*.cal`/`.tun`.
  - Dozens of online/offline pipeline configs cover 1–4 cameras up to 4224x3136.
  - `lt7911` firmware (an HDMI-to-CSI bridge sensor board) is in `cix_module_fw/sensor`.
- **Status:** not in the cix-linux-main mainline patch set, which has no ISP patches — [cix-linux-main patches-7.3](https://github.com/cixtech/cix-linux-main/tree/main/patches-7.3). Sky1-Linux has no ISP row — [Sky1-Linux](https://github.com/Sky1-Linux). A 2024 review said "no driver support" (old) — [CNX Software 2024-12](https://www.cnx-software.com/2024/12/18/radxa-orion-o6-mini-itx-motherboard-is-powered-by-cix-p1-12-core-armv9-soc-with-a-30-tops-ai-accelerator/)
- **Documentation:** TRM Part 2 covers "ISP" and "MIPI" (gated) — [CNX Software 2025-12](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-developer-guides-for-gpu-ai-accelerator-os-and-firmware-bios/)

### Inferences
- **What "armcb" probably is:** the "I5/I7" naming matches Arm China's "Linlon" ISP product line (i3/i5/i7). The CIX ISP is probably a Linlon-ISP i7-class core. This is not confirmed by a CIX source.
- **What is drivable:**
  - Easy-medium: a CSI-2 receiver plus CSI-DMA driver capturing RAW Bayer to memory, based on the open `cix_vi_hw.c` (~27 KB), plus I2C sensor drivers (IMX219 is well documented).
  - Effectively blocked without reverse engineering of `isp_app` / `DDR0.IMG`: debayer, 3A and tuning in the ISP. Software ISP processing on the CPU or GPU, in the libcamera "soft ISP" style, is the realistic alternative.

### Gaps
- Not determined: what `DDR0.IMG` is (ISP firmware? a reference frame?), whether the ISP has its own microcontroller, the exact ISP IP name and version, and whether a mainline-kernel ISP port exists. The isp repo has no `cix_mainline_dev` branch.

---

## Q5. NPU: Arm China Zhouyi (generation, TOPS), CIX NOE SDK, kernel driver, userspace and compiler, documentation, feasibility of an independent driver

### Takeaway
- **Hardware:** an Arm China **Zhouyi V3** NPU with three cores, marketed at about 30 TOPS (45 TOPS "combined" with CPU and GPU).
- **Kernel driver:** `armchina-npu` / `aipu` 6.2.0 is open (GPL-2.0, out of tree, private ioctl uAPI, not in `drivers/accel`). Arm China asked about accel upstreaming in 2024 with no visible result.
- **Userspace:** the CIX NOE runtime (`libnoe`) is closed and so is the compiler (provided only as a Docker image). Arm China's own Compass UMD is open-source.
- **Verdict:** an independent kernel driver is feasible, but useful workloads depend on closed compilers.

### Cited Findings
- **Driver** — [cix_opensource__npu_driver (cix_mainline_dev)](https://github.com/cixtech/cix_opensource__npu_driver/tree/cix_mainline_dev):
  - DKMS `PACKAGE_NAME="aipu"`, `PACKAGE_VERSION="6.2.0"`, built with `BUILD_AIPU_VERSION_KMD=BUILD_ZHOUYI_V3 BUILD_TARGET_PLATFORM_KMD=BUILD_PLATFORM_SKY1 BUILD_NPU_DEVFREQ=y`.
  - Kconfig "ArmChina Zhouyi NPU" with architecture options v1, v2 ("Z2/Z3/X1"), v3 and v3_2, and SoC glue for default, Allwinner R329 and CIX SKY1.
  - Licensing: `LICENSE.TXT` is GPLv2, sources are `SPDX: GPL-2.0`, and the UAPI header `armchina_aipu.h` is "GPL-2.0 WITH Linux-syscall-note".
  - The UAPI is a private char-device ioctl set (magic `'A'`), including QUERY_CAP, REQ_BUF, SCHEDULE_JOB, QUERY_STATUS, CONFIG_CLUSTERS and the DMA-BUF ioctls; ~28 ioctls in total.
  - The Zhouyi V3/V3_2 headers define TSM (task schedule manager) command registers: CREATE, DESTROY, ABORT and DISPATCH, with cluster, core, pool and QoS fields.
  - Sky1 glue does SCMI perf DVFS, per-core power domains `pd_core0..2` and devfreq.
- **ACPI:** NPU0 (CIXH4000) has one MMIO window, one IRQ, `"core_mask", 3`, SCMI perf domain 8, and child cores CRE0–CRE2 (CIXH4010) — [Dsdt-NPU.asl](https://github.com/cixtech/cix_opensource__release__edk2-platforms/tree/cix_p1_dev/Platform/CIX/Sky1/Drivers/AcpiSocTables). **(local observation)** CRE0..CRE2 are present under `\_SB.NPU0`.
- **Generation and performance:**
  - Community projects call it "CIX P1 / ZHOUYI V3" — [FyrbyAdditive/ms-r1-npu-hack](https://github.com/FyrbyAdditive/ms-r1-npu-hack), [herbstleyd/ms-r1-npu-ubuntu2604](https://github.com/herbstleyd/ms-r1-npu-ubuntu2604).
  - "NPU computing power is up to 30TOPs" — [CIX ACPI slides](https://www.kitefor.events/download/410).
  - The Radxa brief v1.2 (updated 2026-04-17) gives a combined "NPU + CPU + GPU: up to 45 TOPS" and NPU precisions "INT4 / INT8 / INT16 / FP16 / TF32" — [Radxa Product Brief](https://dl.radxa.com/orion/o6/docs/radxa_orion_o6_product_brief.pdf).
  - The commercial name (for example "Zhouyi X2/X3") and the exact 28.8 vs 30 TOPS figure were not confirmed.
- **Mainline status:**
  - **(local observation)** The 7.2.11 kernel has `CONFIG_DRM_ACCEL=y` with only `ARM_ETHOSU`, `QAIC` and `ROCKET` accel drivers; there is no Zhouyi driver.
  - Sky1-Linux: NPU ~12,000 lines, "Not submitted" — [Sky1-Linux](https://github.com/Sky1-Linux).
  - Arm China asked dri-devel and linux-arm-kernel in March/April 2024 about upstreaming into accel. The poster stated the driver is "already open sourced (both UMD and KMD)" — [linux-arm-kernel 2024-03](https://lists.infradead.org/pipermail/linux-arm-kernel/2024-March/915691.html), [dri-devel archive](https://mail-archive.com/dri-devel@lists.freedesktop.org/msg487632.html), [Phoronix](https://www.phoronix.com/news/Arm-China-Zhouyi-NPU-Linux-RFC). No merged accel driver was found as of 2026-10.
- **Userspace:**
  - The CIX `cix-noe-umd` ships `libnoe.so.3.1.0` (1.9 MB, binary), `libnoe-3.1.2` Python wheel and `cix_noe_standard_api.h` — [cix_proprietary](https://github.com/cixtech/cix_proprietary__cix_proprietary).
  - Issue #21, opened 2026-05-09 and still open, asks CIX to open-source libnoe; CIX did not address that request. CIX said a compiler Docker image has been in the SDK "since 26Q1" and a ".cix format specification" was targeted for 26Q3. A commenter (2026-10-18) called NOE a "repackaged (and simultaneously crippled) version" of Arm China's Compass SDK — [cix-linux-main issue #21](https://github.com/cixtech/cix-linux-main/issues/21).
  - Arm China's open driver code is mirrored at GitHub `Arm-China/Compass_NPU_Driver` — [Arm China Gitee](https://gitee.com/armchinaopensource). A FreeBSD port of the Arm China Zhouyi UMD (`libaipudrv 4.1.0`) exists for Sky1 — [JamesKane/aipu-umd](https://github.com/JamesKane/aipu-umd).
- **Documentation:** CIX's "NPU Development Guide" (42 pages) and "NOE SDK and AI ModelHub Development Guide" (47 pages) are public, and the TRM covers the NPU (gated) — [CNX Software 2025-12](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-developer-guides-for-gpu-ai-accelerator-os-and-firmware-bios/). Users could not find the "Zhouyi Compass OpenCL Programming Guide and NPU assembly guide" — [issue #21](https://github.com/cixtech/cix-linux-main/issues/21). No public Zhouyi ISA or register manual was found.

### Inferences
- **Kernel side:** writing a Rust driver equivalent to `aipu` is **medium** difficulty. The workflow is: map memory, write a job descriptor, dispatch through TSM, wait for the IRQ. The GPL source of about 3k lines in the core plus a large `aipu_mm.c` is a complete reference.
- **The hard part is userspace.** Models must be compiled by Arm China/CIX tools into `.cix`/ELF graphs, and execution needs either the closed libnoe or the open Arm China UMD (libaipudrv). An independent OS could port the open Compass UMD and run graphs pre-compiled on Linux, which is a "drivable but toolchain-dependent" outcome. Writing an independent compiler is unrealistic without ISA docs.

### Gaps
- Not determined: the exact Zhouyi product name and core configuration (`core_mask = 3` versus three CRE nodes); TOPS per core; whether Compass_NPU_Driver's UMD supports V3 on Sky1 without NOE; and the licence of the Compass UMD. No reverse-engineering effort was found.

---

## Q6. Audio: board codec and jacks, I2S/TDM, DSP, HDMI/DP audio

### Takeaway
- **Headphone path:** a standard **HD Audio controller** (CIX "IPBLOQ", HID CIXH6020) driving a **Realtek ALC256-family** HDA codec. The controller driver is in mainline (DT-only) and the codec is handled by mainline `alc269.c` with CIX fixups. This is the easiest multimedia block on the board.
- **Other audio blocks:** ten Cadence I2S controllers (3 "sc" + 7 "mc") and a Cadence Tensilica **HiFi5** DSP running closed `dsp_fw.bin` under a Sound Open Firmware-style driver.
- **DP audio:** unfinished upstream.

### Cited Findings
- **Board:** 3.5mm combo jack ("32Ω headphone drive capability", integrated mic input) plus a front-panel "HD Audio" header — [Radxa Product Brief](https://dl.radxa.com/orion/o6/docs/radxa_orion_o6_product_brief.pdf). CIX slides: "HDA controller and codec, Jack detection and Headphone support" — [CIX ACPI slides](https://www.kitefor.events/download/410)
- **Codec:** CIX patches add `ALC256_FIXUP_CIX_SKY1_ORION_O6` ("CIX SKY1 ORION O6 HDA"), plus EVB (ALC256) and Orange Pi 6 Plus (`ALC269_FIXUP_CIX_SKY1_ORAPI_6P`) model fixups in `sound/hda/codecs/realtek/alc269.c`. The 6.6 BSP used SSID `0x10EC129E` (`ALC256_FIXUP_CIX_PHECDA`). Two jack-detectable mics require `suppress_auto_mic` — [0071 patch](https://github.com/cixtech/cix-linux-main/blob/main/patches-7.3/0071-DPTSW-26459-ALSA-hda-realtek-suppress-auto-mic-on-CI.patch), [0013 patch](https://github.com/cixtech/cix-linux-main/blob/main/patches-7.3/0013-DPTSW-26459-sound-hda-rework-cix-ipbloq-based-on-mai.patch)
- **HDA controller:** `sound/hda/controllers/cix-ipbloq.c` (Copyright 2025 Cix) exists in mainline. CIX's patch 0013 reworks it and adds ACPI `CIXH6020` — [0013 patch](https://github.com/cixtech/cix-linux-main/blob/main/patches-7.3/0013-DPTSW-26459-sound-hda-rework-cix-ipbloq-based-on-mai.patch). **(local observation)** 7.2.11 has `CONFIG_SND_HDA_CIX_IPBLOQ=m`, alias `cix,sky1-ipbloq-hda` only.
- **I2S and machine driver:** patch 00220 adds `sound/soc/cix/` with `cdns_i2s_sc.c` (1,312 lines, CIXH6010), `cdns_i2s_mc.c` (1,203 lines, CIXH6011), `card-utils.c` and `sky1-card.c` (CIXH6070) — [00220 patch](https://github.com/cixtech/cix-linux-main/tree/main/patches-7.3). The Audio subsystem clock and reset unit (ADSS/ACRU at 0x07110000) has its own CIX clock and reset drivers (patches 00052–00054, 00061–00062).
- **DSP:**
  - The `dsp_fw.bin` is for the "Cadence Tensilica HiFi5, using rpMSG-lite", driven by the CIX DSP rproc driver — [Sky1-Linux/sky1-firmware](https://github.com/Sky1-Linux/sky1-firmware).
  - CIX patches add `remoteproc/cix_dsp_rproc.c` (1,166 lines, CIXH6000), a DSP IPC driver and `sound/soc/sof/cix/cix-sky1.c` (SOF platform) — [patches 0009/0020/0021](https://github.com/cixtech/cix-linux-main/tree/main/patches-7.3).
  - ACPI: DSP `_DSD` `"firmware-name", "dsp_fw.bin"` with `cix,dsp-ctrl` = DCRU — [Dsdt-Dsp.asl](https://github.com/cixtech/cix_opensource__release__edk2-platforms/tree/cix_p1_dev/Platform/CIX/Sky1/Drivers/AcpiSocTables).
  - The closed `cix-audio-dsp` package contains `dsp_fw.bin` (500 KB) and DSP codec plugins (AAC-ELDv2, FLAC, HE-AACv2 encoder, MP3, Opus, Vorbis, xHE-AAC) — [cix_proprietary](https://github.com/cixtech/cix_proprietary__cix_proprietary).
- **Status:** cix-linux-main marks the headphone jack "supported" and "DP sound" TODO — [cix-linux-main](https://github.com/cixtech/cix-linux-main). Sky1-Linux: Audio (HDA + DSP) ~8,000 lines, "Stalled" — [Sky1-Linux](https://github.com/Sky1-Linux). The FreeBSD komeda port says DP audio has no ALSA support — [drm-komeda-kmod](https://github.com/JamesKane/drm-komeda-kmod). The DP TX has `trilin_dptx_audio.c` in CIX's display patch — [00100 patch](https://github.com/cixtech/cix-linux-main/tree/main/patches-7.3).

### Inferences
- **HDA path:** easy-medium for an independent OS. The Intel HDA spec is public, the CORB/RIRB/stream DMA model is well known, the ALC256 is a mainstream codec with public Linux quirks, and the CIX-specific part is small (clocks, resets, a few registers in `cix-ipbloq.c`). This should be the first audio target.
- **I2S (Cadence):** medium, using the GPL drivers as reference.
- **DSP offload:** optional. It is closed-firmware-dependent and needs remoteproc and rpmsg in the new OS; low value for a research OS.
- **DP/HDMI audio:** it rides on the Trilinear DP TX audio path, so it is tied to the display driver. Medium, but upstream has not finished it either.

### Gaps
- The exact codec part number (ALC256 vs ALC269 variant on the O6) is inferred from the fixup names only; it was not verified on the PCB.
- Not determined: whether the HDA controller deviates from the Intel HDA register map, and the DMA-350 (DMA1 audio DMAC) relationship to I2S.

---

## Q7. Other engines: DMA, crypto/security, sensor hub, misc

### Takeaway
- **DMA:** two **Arm CoreLink DMA-350** instances, a general-purpose one (CIXHA014) and an audio one (CIXH1006). A mainline driver (`arm-dma350`) exists, DT-only, and CIX adds ~1,000 lines for ACPI and Sky1.
- **Security engine:** lives in the secure world and is reached only via OP-TEE and TF-A. A from-scratch OS should treat it as inaccessible.
- **Other microcontrollers:** CIX ships closed firmware for a "sensor fusion hub" (`sfh_fw*.bin`). The board-power and platform firmware runs on an SCMI server behind mailboxes.

### Cited Findings
- **DMA-350:** patch 0016 "dma: arm-dma350: add acpi support for cix soc" changes 1,040 lines and matches ACPI "CIXH1006" and "CIXHA014". The ASL for DMA1 (audio DMAC) has `arm,reg-map` / `arm,ram-map` / `arm,remote-ctrl = \_SB.ACRU` properties — [0016 patch](https://github.com/cixtech/cix-linux-main/blob/main/patches-7.3/0016-dma-arm-dma350-add-acpi-support-for-cix-soc.patch), [Dsdt-Dma.asl](https://github.com/cixtech/cix_opensource__release__edk2-platforms/tree/cix_p1_dev/Platform/CIX/Sky1/Drivers/AcpiSocTables). **(local observation)** 7.2.11 has `CONFIG_ARM_DMA350=m` (alias `arm,dma-350` only).
- **Security engine:** CIX describes a "Comprehensive Security Engine" — [CIX ACPI slides](https://www.kitefor.events/download/410). The CIX diagnostics driver includes `cix_ap2se_ipc` (AP-to-SE IPC) and SDEI handlers for the NI-700 IDM, TZC-400 and TF-A traces — [0066-add-cix_dst-driver.patch](https://github.com/cixtech/cix-linux-main/tree/main/patches-7.3) (124 files, 21,894 insertions; debugging/blackbox, not an accelerator). OP-TEE is CIXHA022 **(local observation: bound to `optee`)**, and the CIX TF-A and OP-TEE sources are published — [cixtech org](https://github.com/cixtech) (repos `cix_opensource__arm-trusted-firmware`, `cix_opensource__tee__op-tee`).
- **Interconnect:** the GPU coherency patch describes a CHI fabric with HN-F/SLC — [00114](https://github.com/cixtech/cix-linux-main/blob/main/patches-7.3/00114-drm-panthor-rework-bus-coherency-to-ACE-Lite-ACE-NON.patch). TZC-400 and NI-700 IDM are named in the dst driver (above). The IDM gates non-secure access to the GPU until TF-A opens it — [00112](https://github.com/cixtech/cix-linux-main/blob/main/patches-7.3/00112-drm-panthor-Sky1-CIXH5000-ACPI-bring-up.patch)
- **Power and clock control:** SCMI over CIX mailboxes (CIXHA001 MBX0..7, CIXHA006 SCMI). Mainline has `cix-mailbox` (DT-only) — **(local observation)**. CIX patches add ACPI support to arm_scmi (25 KB), the CIX mailbox, a CIX clock driver, a reset driver, pmdomain and an "ACPI resource lookup" driver — [patches 0001–0008](https://github.com/cixtech/cix-linux-main/tree/main/patches-7.3). The PPA manual requires enabling "ACPI SCMI" in the BIOS or "some kernel 7.0 drivers will fail to load" — [CIX PPA manual](https://github.com/cixtech/cix-developer-docs/wiki/CIX-PPA-User-Manual-(Open%E2%80%90Source-Driver-Edition))
- **Sensor hub firmware:** `cix_module_fw/sfh_fw/sfh_fw.bin`, `sfh_fw_i3c.bin`, `sfh_fw_sensor.bin` and `sfh_fw_wakeup.bin`, all closed — [cix_proprietary](https://github.com/cixtech/cix_proprietary__cix_proprietary)
- **Documentation:** TRM Part 2 lists FCH (GPIO, SPI, I2C, UART, I3C, timer, XSPI, DMA). CNX does not mention crypto or DSP in the TRM chapter list — [CNX Software](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-developer-guides-for-gpu-ai-accelerator-os-and-firmware-bios/)

### Inferences
- **DMA-350:** easy-medium. Arm publishes CoreLink TRMs, and the mainline driver is small. It is useful early for audio and I2S/SPI.
- **SCMI client over the CIX mailbox:** a prerequisite for every accelerator, since power domains, clocks and DVFS for the GPU, NPU and VPU are SCMI perf/power domains. **Implement it first.** The SCMI spec is public (Arm DEN0056).
- **Security engine:** not usable from the non-secure world. Use the CPU's Armv9 crypto extensions instead.

### Gaps
- No CIX documentation was found on the security engine's capabilities or on what the "sensor fusion hub" MCU is (core type, location).
- The DMA-350 channel counts for each instance were not checked.

---

## Q8. Overall: what an independent OS can realistically drive, and which Rust code exists to learn from

### Takeaway
Ranked from most to least drivable:
1. **Most drivable (open code or public specs, no blobs or only redistributable blobs):** GOP framebuffer, HDA audio with the Realtek codec, DMA-350, SCMI and mailbox, CSI-2 raw capture.
2. **Drivable with a large effort:** the GPU (Panthor/Tyr plus the Arm CSF blob, with Mesa reusable through the Panthor uAPI), display (komeda/Linlon-D6 plus the Trilinear DP TX, GPL), and the VPU (GPL `mvx` driver plus closed `.fwb`).
3. **Kernel-drivable but toolchain-blocked:** the NPU (open GPL kernel driver; closed compiler and libnoe; partially open Arm China UMD).
4. **Effectively blocked:** ISP image processing (closed daemon, firmware image and tuning), the DSP codecs, and the security engine.

Rust prior art is Tyr (Mali CSF, mainline). Nothing Rust-based exists for any CIX-specific block.

### Cited Findings
- **CIX's own matrix (2026-10):**
  - Display: USB-C DP alt, DP and HDMI supported, with VPU-to-GL rendering issues.
  - Audio: headphone jack supported; DP sound TODO.
  - GPU: supported (Panthor).
  - VPU and NPU: external DKMS modules.
  - PCIe works without patches.
  - Source: [cix-linux-main](https://github.com/cixtech/cix-linux-main)
- **Out-of-tree weight versus mainline v6.19** — [Sky1-Linux](https://github.com/Sky1-Linux):

  | Subsystem | Out-of-tree lines (approx.) | Status |
  |---|---|---|
  | Display | 27k | not submitted |
  | Audio | 8k | stalled |
  | USB-C | 5k | not submitted |
  | GPU init | 0.5k | not submitted |
  | NPU | 12k | not submitted |
  | VPU | 39k | not submitted |
  | Ethernet | 53k | not submitted |

  CIX upstreamed pinctrl, mailbox, PCIe and the base DTS in v6.19.
- **Mainline progress since then:** Linlon-D6 in komeda (v7.1) — [drm-komeda-kmod](https://github.com/JamesKane/drm-komeda-kmod). Tyr reaches MCU boot (v7.3) — [Collabora](https://www.collabora.com/news-and-blog/blog/2026/10/21/upstreaming-tyr-rust-drm-and-the-linux-kernel/).
- **Non-Linux precedent:** a FreeBSD port of komeda plus the Trilinear DP and Panthor on Sky1, running sway, using ACPI. Its glue builds DT-style nodes from the ACPI `_DSD`/`CLKT`/`RSTL` tables — [JamesKane/drm-komeda-kmod](https://github.com/JamesKane/drm-komeda-kmod), [JamesKane/drm-panthor-kmod](https://github.com/JamesKane/drm-panthor-kmod)
- **Documentation availability:**
  - TRM (Part 1: 6,179 pages; Part 2: 3,051 pages), registration-gated with manual approval.
  - Public developer guides: GPU (25 pages), NPU (42 pages), NOE (47 pages).
  - No licence was stated for any document.
  - Source: [CNX Software 2025-12-13](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-developer-guides-for-gpu-ai-accelerator-os-and-firmware-bios/)
- **Firmware and blob summary:**

  | Blob | Owner | Terms |
  |---|---|---|
  | GPU `mali_csffw.bin` (arch12.8) | Arm | EULA, redistributable (local licence text) |
  | VPU `*.fwb` | CIX | "proprietary and redistributable", licence unnamed; also in a GPL-labelled repo |
  | DSP `dsp_fw.bin` | CIX | proprietary |
  | ISP `DDR0.IMG` + tuning | CIX | proprietary |
  | NPU | — | no firmware blob; the kernel loads compiled graphs |
  | SFH firmware | CIX | proprietary |

  Sources: [sky1-firmware](https://github.com/Sky1-Linux/sky1-firmware), [cix_proprietary](https://github.com/cixtech/cix_proprietary__cix_proprietary).
- **Boot modes:** CIX's mainline patch set targets ACPI ("O/S Hardware Description Selection" = ACPI, edk2 1.1.0-2) — [cix-linux-main](https://github.com/cixtech/cix-linux-main). Community GPU bring-up used DT with `acpi=off` — [visorcraft](https://github.com/visorcraft/orange-pi-6-plus-gpu).

### Inferences
- **Suggested order for a Rust OS:**
  1. GOP framebuffer.
  2. SCMI client plus CIX mailbox. A DT or AML parser is needed to find the devices.
  3. HDA audio.
  4. DMA-350.
  5. Display (one DP or HDMI output via Linlon-D60 + Trilinear).
  6. GPU (Panthor-uAPI-compatible, with Tyr as the Rust model).
  7. VPU.
  8. NPU kernel side.
  9. Raw CSI capture.
- **Difficulty estimates (subjective):**

  | Block | Difficulty |
  |---|---|
  | GOP | trivial |
  | HDA | easy-medium |
  | DMA-350 | easy-medium |
  | SCMI | medium |
  | Display, one output | medium-hard |
  | Display, all outputs + MST + Alt Mode | hard |
  | VPU | medium-hard |
  | NPU kernel driver | medium |
  | NPU, usable end to end | blocked on closed tools |
  | GPU | very hard |
  | ISP processing | blocked |
  | DSP | blocked / low value |
- **Boot-mode choice:** DT boot (community `sky1-*.dtb`) avoids writing an AML interpreter, but CIX's own support is moving to ACPI. ACPI mode also needs AML for PowerResource `_ON` and clock methods, or the OS can hard-code SCMI calls as CIX's Panthor patch does for the GPU.
- **Rust references:**
  - Tyr: the CSF firmware boot, VM/MMU and GEM structure.
  - The Rust-for-Linux page-table, GPUVM and GEM-shmem abstractions (concepts transferable to a non-Linux kernel).
  - The Nova `register!` macro pattern.
  - None exists for Linlon, Trilinear, Zhouyi or MVE.

### Gaps
- No public, register-level documentation was found for any CIX or Arm China block outside the gated TRM. Whether the TRM's licence allows using it for an open-source OS is unknown.
- No reverse-engineering projects (Asahi-style) were found for the Zhouyi NPU, Linlon-V or the ISP.
