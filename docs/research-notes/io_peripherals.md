# Radxa Orion O6 (CIX P1 / CD8180 "Sky1"): I/O and low-speed peripherals, from the point of view of a from-scratch Rust OS

Research date: 2026-10-25. Three kinds of evidence:
(a) **Local probes of the user's own board.** openSUSE Tumbleweed, Linux 7.2.11-1-default, UEFI+ACPI. DMI reports BIOS "1.3.1", dated 2026-07-16. Probes were run as a normal user: `lspci`, `lsusb -t`, sysfs under `/sys/bus/acpi/devices`, `/proc/interrupts`, `modinfo`, `ethtool -i`. The raw ACPI tables (DSDT and others) could not be read: they are root-only and sudo needed a password. These items are marked "(local probe …)".
(b) **Vendor and community sources.** Radxa docs, product brief and schematic; the CIX upstream wiki; Sky1-Linux kernel patches, which are ports of the vendor BSP; CNX Software.
(c) **Reference driver sources.** Linux, OpenBSD, FreeBSD, Redox, and the rcore-os/tgoskits Rust drivers.

Scratch extraction used for this note: the vendor-derived `sky1-orion-o6.dts` and `sky1.dtsi`, taken from Sky1-Linux patch 0001. This DTS is the BSP flavour, not the slimmer mainline DTS.

---

## 1. Physical inventory of the Orion O6 (and O6N differences)

### Takeaway
The O6 is a Mini-ITX board (170×170 mm). It has five PCIe Gen4 root complexes: x8 to the x16 slot, x4 to M.2 M-key, x2 to M.2 E-key, and x1 + x1 to two RTL8126 5GbE NICs. It has ten USB controllers: six SuperSpeed and four HS-only. Two USB-C ports are driven by RTS5453H PD controllers, one with DP alt-mode. Displays are HDMI (through a DP-to-HDMI bridge), DP 1.4 and eDP. There are 2× MIPI CSI, ALC256 HD-Audio, a 40-pin header, a PWM fan header and a CR1220-backed HYM8563 RTC. An ITE IT5570E EC shares an 8 MB 1.8 V SPI NOR with the BIOS. The board has several 3-pin debug UART headers and an **unpopulated** SPI TPM footprint. Wi-Fi/BT is **not on the board**: it is an M.2 E-key add-in. The O6N is a smaller variant (CD8160, 2.5GbE, UFS, 2× M.2 M-key, B-key, 12 V DC).

### Cited Findings
- **SoC and form factor.** Mini-ITX 170×170 mm. Recommended PSU ≥65 W. SoC is the CIX P1 (TSMC 6 nm): 4× A720 big, 4× A720 medium, 4× A520, 12 MB L3. LPDDR5 128-bit, 8–64 GB — [Radxa O6 docs](https://docs.radxa.com/en/orion/o6); [Product brief v1.2](https://dl.radxa.com/orion/o6/docs/radxa_orion_o6_product_brief.pdf)
- **Back panel** (product brief v1.2, 2026-04-17 revision):
  - DP 1.4 at 4K@120 with MST
  - HDMI 2.0 at 4K@60, no CEC
  - 2× RJ45 at 10/100/1000/2500/5000 Mbps
  - USB-C port 1: USB 3.2 Gen2 10 Gb/s + PD
  - USB-C port 2: USB 3.2 Gen2 + DP Alt Mode (4K@60) + PD
  - 2× USB-A 3.2 Gen2, 2× USB-A 2.0
  - 3.5 mm combo jack (32 Ω headphone drive, mic in)
  
  Source: [Product brief](https://dl.radxa.com/orion/o6/docs/radxa_orion_o6_product_brief.pdf)
- **Internal connectors:**
  - 24-pin ATX
  - 4-pin CPU fan header with PWM + TACH
  - M.2 M-key (PCIe Gen4 x4, NVMe)
  - M.2 E-key ("PCIe Gen4 x2 lanes + USB", for Wi-Fi 6E + BT modules)
  - x16 physical slot, Gen4 x8 electrical
  - eDP with touch-panel interface (up to 4Kp60)
  - 2× MIPI CSI, each configurable as 4-lane or 2-lane
  - Front-panel system header (power, reset, LEDs), front USB 2.0 header (2 ports), HD-Audio front header
  - CR1220 RTC battery holder
  - "Serial console header"
  
  Source: [Product brief](https://dl.radxa.com/orion/o6/docs/radxa_orion_o6_product_brief.pdf)
- **Power inputs:** standard ATX PSU, or USB-C PD at 20 V, or "dummy" USB-C 20 V. Do not connect several at once. ≥65 W recommended — [Product brief §5.1](https://dl.radxa.com/orion/o6/docs/radxa_orion_o6_product_brief.pdf)
- **Hardware-info page details:**
  - PCIe slot power: up to 60 W on ATX, 15 W on USB-C power.
  - M.2 M-key accepts 2230/2242/2260/2280.
  - USB-C0 supports OTG; USB-C1 is HOST. Each Type-C port can source 3 A.
  - USB-A 3.2: 1 A per port. USB 2.0 ports share 1.4 A.
  - eDP: 40-pin 0.5 mm IPEX with BL_EN/PWM pins.
  - "TPM: footprint present, unsoldered".
  - BIOS flash: SOP8 socket, 8 MB (64 Mbit), 1.8 V.
  - LEDs: green = power; blue solid = BIOS booted, blue blinking = OS booted.
  - Buttons: power (hold 4 s forces off) and reset.
  - BOOT strap header.
  
  The page's interface table says x16 slot "Gen x4" while its description says Gen4 x8. This is an internal inconsistency; the brief and DT say x8 — [Radxa hardware-info](https://docs.radxa.com/en/orion/o6/hardware-use/hardware-info)
- **Debug UARTs.** There are several 3-pin (GND/TX/RX, 3.3 V) headers. UART2 carries BIOS and OS debug logs. UART4 carries power management (voltage/frequency monitoring). UART5 carries secure BootROM logs. A separate EC UART carries EC logs — [Radxa hardware-info](https://docs.radxa.com/en/orion/o6/hardware-use/hardware-info)
- **40-pin header** (as published; Radxa's table is partly garbled):
  - I2C4 on GPIO062/061 (pins 3/5). I2C2 is also available (GPIO056/055).
  - UART3 on pins 8/10; UART0 and UART1 are also available.
  - SPI2 (GPIO044/045, CS0 GPIO139, CS1 GPIO140)
  - PWM0/PWM1 (GPIO095/096)
  - I2S2 and I2S4
  - FAN_OUT0/FAN_TACH0 on GPIO099/100
  
  Source: [Radxa hardware-info](https://docs.radxa.com/en/orion/o6/hardware-use/hardware-info)
- **Schematic v1.20** (sheet index dated 2025-01-16) is public. Sheets include: SKY1 PCIe/USB/I2C/GPIO, M.2 SSD, M.2 WLAN/BT, 40-pin, "DP to HDMI", "PCIE 5G_Ethernet_1/2", "PCIE X8 SLOT", "USB-C0/C2", "PD", "Audio CODEC", "FAN CON", "TPM", "SPI-ROM", "EC Module" and "I2C_ID" — [O6 schematic v1.20](https://dl.radxa.com/orion/o6/hw/radxa_orion_o6_v1.20_schematic.pdf); listing on [Radxa downloads](https://docs.radxa.com/en/orion/download)
- **Named ICs found in the schematic text:**
  - **ITE IT5570E** (EC)
  - **Realtek RTS5453H** ×2 (USB-C PD)
  - **Realtek ALC256** (HD-Audio codec)
  - **HYM8563TS** (RTC)
  - **Winbond W25R64JW** on a sheet labelled "BIOS&EC SHARED ROM"
  - **Infineon SLB9672XU2** on a sheet labelled "SPI TPM 2.0 (OPTION)"
  - **NXP PCA9535PW** at I2C 0x24, which carries the BOARD_ID (DRAM-size) straps
  - PCA9517/PCA9306 I2C buffers/level shifters
  
  Source: [O6 schematic v1.20](https://dl.radxa.com/orion/o6/hw/radxa_orion_o6_v1.20_schematic.pdf). I extracted these with `pdftotext`; much of the PDF is raster, so the list may be incomplete.
- **The HDMI port** is fed by a DP-to-HDMI bridge. A Sky1-Linux patch adds a "DRM bridge chain for PS185 DP to HDMI" — [Sky1-Linux patch list](https://github.com/Sky1-Linux/linux-sky1/tree/main/patches) (patch 0022)
- **Wi-Fi is user-added.** Radxa's own E-key module is the "Wireless Module A8": RTL8852BE, Wi-Fi over PCIe, BT 5.2 over USB. One forum report (Fedora 42 on an Orion) saw an rfkill hard-block until a BIOS fix — [Radxa A8 docs](https://docs.radxa.com/en/accessories/wireless-a8); [Radxa forum: A8 not working on Orion](https://forum.radxa.com/t/radxa-a8-wireless-module-not-working-on-orion/28783)
- **Orion O6N** (announced 2025-10-14):
  - SoC: CD8160 variant (big cores up to 2.6 GHz vs 2.8 GHz)
  - Nano-ITX 120×120 mm
  - 2× **2.5GbE** instead of 5GbE (chip not named in the article)
  - **2× M.2 M-key** Gen4 x4, a **UFS module connector**, an M.2 **B-key** (4G/5G) and an E-key
  - x16 slot at Gen4 x8
  - 1× USB-C PD + 1× USB-C DP-alt, 2× USB-A 3.2, 3× USB-A 2.0
  - Up to 3 displays
  - 12 V DC barrel/4-pin input + ATX + USB-C 20 V
  - "Six UARTs via headers"
  - Wi-Fi not included
  
  Source: [CNX Software on O6N](https://www.cnx-software.com/?p=158462)
- **Board and BIOS versions.** Radxa ships separate BIOS images for O6 and O6N. The download page lists O6 BIOS 1.1.0-1 and O6N 1.2.4 — [Radxa downloads](https://docs.radxa.com/en/orion/download). The user's board reports BIOS **1.3.1 (2026-07-16)** — (local probe: `/sys/class/dmi/id/bios_version`, `bios_date`). The download page may lag behind newer releases; this was not verified.

### Inferences
- The Sky1-Linux README lists "Realtek RTL8126 (5GbE) and RTL8125 (2.5GbE)" drivers for O6/O6N. The O6N's 2.5GbE NICs are therefore very likely RTL8125 (also 10ec:8125-family, r8169-driven). A driver for the O6's RTL8126 would mostly carry over to the O6N.
- The Radxa docs give "USB-C0 OTG, USB-C1 host". The vendor DT instead marks both Type-C controllers (usbss_0 and usbss_2) as OTG with role-switch endpoints. Sources disagree on which Type-C port is dual-role. For host-only use in a new OS this does not matter.

### Gaps
- The schematic block diagram is an image. I did not extract the exact pin/lane routing for each connector (for example, which USB2 controller feeds the M.2 E-key, the front header and the rear USB 2.0 ports).
- O6N schematic v1.11 is available but was not analysed.

---

## 2. PCIe (Cadence-based root complexes, ECAM under ACPI, MSI)

### Takeaway
Under ACPI the five root complexes are presented as generic ECAM through **MCFG**. The UEFI firmware has already trained the links, and **mainline Linux binds no CIX-specific PCIe driver in ACPI mode**. Every endpoint enumerates and gets MSI/MSI-X through the **GICv3 ITS** described in IORT. A new OS can therefore start with a generic ECAM + ITS (or INTx) driver.

There are caveats. A community porter found that firmware leaves the controllers "partially configured": I/O windows, TLP filtering and power-state transitions. The vendor kernel approach replaces PNP0A08 with a full CIX re-init through the CIXH2020 devices. Expect occasional link quirks (notably on the Wi-Fi x2 port) and **no I/O-port space**.

### Cited Findings
- **Controller map** (vendor DT `sky1.dtsi`; all `compatible = "cix,sky1-pcie-host"`, `max-link-speed = <4>`):

  | Controller | Regs base | Bus range | Lanes |
  |---|---|---|---|
  | x8 | 0x0A010000 | 0xC0–0xFF | 8 |
  | x4 | 0x0A070000 | 0x90–0xBF | 4 |
  | x2 | 0x0A0C0000 | 0x60–0x8F | 2 |
  | x1_1 | 0x0A0E0000 | 0x30–0x5F | 1 |
  | x1_0 | 0x0A0D0000 | 0x00–0x2F | 1 |

  The x8 controller's reg-names are "reg", "rcsu", "cfg" (ECAM window at 0x2C000000, 64 MB) and "msg". It uses `msi-map` onto `its_pcie` and `vendor-id = <0x1f6c>`. PERST# lines are on S5 GPIO0 pins 1–5 — [Sky1-Linux patch 0001 (DTS/DTSI)](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0001-arm64-dts-cix-Add-Sky1-SoC-and-Radxa-Orion-O6-device.patch)
- **Observed on the user's board** (local probe: `lspci -nn` and sysfs `current_link_speed`/`max_link_speed`): root ports are all `17cd:0100` "CIX PCIe Root Port" at buses 00/30/60/90/C0, each able to run 16 GT/s.

  | Bus | Device | Link (current / max) |
  |---|---|---|
  | 01:00.0 | RTL8126 | 8 GT/s x1 (device max) |
  | 31:00.0 | RTL8126 | 8 GT/s x1 (device max) |
  | 61:00.0 | RTL8852BE | 2.5 GT/s x1 (device max); root port max x2 |
  | 91:00.0 | Crucial P3 | 8 GT/s x4 (Gen3 drive); root port max Gen4 x4 |
  | C1:00.0 | NVIDIA | currently 16 GT/s x2 on a root port with max x8 (out of scope; possibly link power management) |

  So: C0 = x16 slot, 90 = M.2 M-key, 60 = M.2 E-key, 00 and 30 = the two 5GbE NICs.
- **MSI-X works under mainline ACPI through the GIC ITS.** `/proc/interrupts` shows `ITS-PCI-MSIX-0000:91:00.0 nvme0q0..q7`, `ITS-PCI-MSIX-0000:01:00.0 enp1s0`, and `ITS-PCI-MSI-0000:61:00.0 rtw89_pci`. An Arm SMMUv3 is also active (`arm-smmu-v3-evtq/gerror/priq` IRQs). Firmware tables present: APIC, CSRT, DBG2, DSDT, FACP, FPDT, GTDT, IORT, MCFG, PCCT, PPTT, SDEI, SSDT1, SSDT2 (no SPCR, no TPM2) — (local probe: `/proc/interrupts`, `ls /sys/firmware/acpi/tables`)
- **Mainline status.** The Sky1 PCIe host driver was merged for **v6.19-rc1** (Hans Zhang, 2025-10-20) — [cixtech/linux-mainline wiki](https://github.com/cixtech/linux-mainline/wiki) (wiki last edited 2026-10-19). The openSUSE 7.2.11 build ships `pci-sky1.ko` (`CONFIG_PCI_SKY1_HOST=m`), but its only aliases are `of:…cix,sky1-pcie-host`, i.e. **DT-only**. No mainline module carries any `acpi*:CIXH*` alias — (local probe: `modinfo pci_sky1`; `grep 'acpi\*:CIX' modules.alias` → empty)
- **What firmware leaves undone under ACPI** (Sky1-Linux patch 0090, 2026): "Firmware brings links up but leaves controllers partially configured — missing RC BAR setup for I/O windows, missing TLP filtering, and HW power state transitions enabled." The patch matched MCFG OEM ID "CIXTEK" / table ID "SKY1EDK2". It noted a "Known issue: link quiesce/retrain cycle causes WiFi (x2 controller) link instability" — [patch 0090](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0090-PCI-sky1-Add-MCFG-quirk-for-CIX-Sky1-Cadence-PCIe-co.patch)
- **Follow-up patch 0091** switched to the vendor-kernel pattern: "block PNP0A08 root bridges with an ACPI scan handler and let pci-sky1.c probe CIXH2020 platform devices directly … full init sequence (PHY, reset, GPIO, straps, controller registers, link training)". It filters `_CRS` windows "that exceed ARM64's IO_SPACE_LIMIT". It also hit an fw_devlink chain "CIXHA010→CIXH2023→CIXH2020" (clock → PCIe PHY → RC) — [patch 0091](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0091-PCI-sky1-Switch-ACPI-from-MCFG-quirk-to-vendor-scan-.patch); [patch 0092](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0092-PCI-sky1-Always-claim-CIXH2020-in-scan-handler-to-pr.patch); [patch 0093](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0093-PCI-sky1-Fix-ACPI-probe-with-proper-RSTL-reset-and-r.patch)
- **ACPI devices on the user's board:**
  - CIXH2020 PRC0–4 (`_STR` "PCIe 0..4 Device"), all status 0xF, unbound
  - CIXH2023 PCP0–2 (PCIe PHYs)
  - PNP0A08 ×5 host bridges, used by Linux
  
  Source: (local probe: `/sys/bus/acpi/devices/*/{path,status,description}`)
- **SystemReady status.** The board passed **Arm SystemReady SR v2.5** with UEFI 9.0.0 (2025-04-11): SBBR/BBR firmware, BSA 1.0c and SBSA Level 6 incl. PCIe. WinPE 26100, SLES 15 SP6, RHEL 9.5 and Ubuntu 24.10 were tested, with A520 cores disabled because Windows BSODs with them — [CNX, 2025-04-30](https://www.cnx-software.com/2025/04/30/radxa-orion-o6-mini-itx-motherboard-gets-arm-systemready-sr-v2-5-certification/)
- **Register documentation.** CIX published a P1 TRM in Dec 2025. Part 1 (6,179 pages) covers system, CPU, GPU, NPU, VPU, DPU, SMMU and **PCIe**. Download requires a CIX developer account with manual approval. Reader comments say Radxa's mirror was removed at CIX's request — [CNX, 2025-12-13](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-de)
- **Root-port ID collision.** The DT driver programs vendor/device ID 0x1f6c:0x0001. Under ACPI the root ports report Cadence's default `17cd:0100`. That ID is also matched by Linux's `cdnsp_pci` and `cdns3_pci_wrap` (USB PCI-wrapper drivers for Cadence dev boards), so both modules **autoload but bind nothing** — (local probe: `modinfo cdnsp_pci` → `alias pci:v000017CDd00000100…`; `cdns3_pci_wrap` same; `lsmod` shows 0 users). The user's observation that "cdnsp_pci / cdns3_pci_wrap loaded" is therefore a modalias artefact. It does **not** mean USB runs over PCIe.

### Inferences
- **A minimal Rust PCIe stack is enough at first.** Parse MCFG. Walk ECAM for segment 0, buses 00–FF (the five RCs occupy disjoint bus ranges). Assign or keep firmware-assigned BARs (64-bit prefetchable windows at 0x18_0000_0000+ for x8, per DT `ranges`). Program MSI-X to the ITS doorbell through an ITS driver (MAPD/MAPTI/MAPC commands); IORT gives the RID→DeviceID mapping. That should bring up NVMe and the NICs without touching Cadence registers, because Linux 7.2 does exactly that today.
- **Do not rely on PCI I/O space.** None of the on-board devices need it.
- **Avoid aggressive link PM.** Retraining or quiescing links (ASPM, secondary bus reset, hot reset) is where firmware-left state bites. For a research OS: leave links as firmware trained them and avoid bus resets.
- **SMMU.** If firmware leaves SMMUv3 enabled with "boot-active" stream IDs (Sky1-Linux patches 0035/0039 add boot-active SIDs for PCIe/DPU), DMA may depend on SMMU state at ExitBootServices. Plan an early check of SMMU_CR0/GBPA. Writing an SMMUv3 driver is optional, and Rust prior art exists (see §8).

### Gaps
- The raw MCFG, IORT and DSDT (`_OSC`, `_PRT`, `_CRS` windows, IORT RMR nodes) could not be dumped without root. The MCFG OEM IDs are known only from the Sky1-Linux patch text.
- lore.kernel.org returned 403, so I could not read the upstream pci-sky1 cover letter for its exact non-standard behaviour (for example, whether root-bus config access needs special handling). Patch 0093 notes "root bus config space access to always use reg_base (matching mainline cdns_pci_map_bus)", which hints that bus-0 config is served from the RC register block in DT mode.
- I found no source on SMMU state at OS handoff (bypass vs translating).

---

## 3. USB (Cadence USBSSP / cdnsp) and Type-C / PD

### Takeaway
In ACPI mode **all ten USB controllers are presented as standard xHCI (PNP0D10)** and are driven by Linux's generic `xhci-plat`. Firmware has already done the CIX-specific clock, reset, strap and PHY setup. A generic Rust xHCI driver is therefore sufficient for host mode. There is no USB4/Thunderbolt.

Type-C is handled by two **Realtek RTS5453H** PD controllers on I2C1 (0x30/0x31). They negotiate power and DP-alt autonomously and expose a proprietary I2C status interface, **not UCSI**. Role switching, OTG and device mode need CIX-specific glue (cdnsp-sky1, PHY drivers) that is only in vendor or community trees. Mainline USB glue is still "Reviewing".

### Cited Findings
- **Topology observed** (local probe: `lsusb -t`, sysfs `usbN/../firmware_node/path`): 10 controllers, all bound to `xhci-hcd`, each root hub with a single port.
  - `\_SB.XHC0..XHC5`: six controllers with a USB2 root hub and a **10000M** SS root hub each.
  - `\_SB.USB0..USB3`: four controllers with **480M only**.
  - Observed devices: BT 13d3:3570 on USB2 (bus 15); USB webcam on USB1; Logitech receivers on XHC5 and USB0.
- **Vendor glue enumerates the SoC's controllers** as U3_TYPEC_DRD, U3_TYPEC_HOST0/1/2, U3_TYPEA_CTRL0/1 and U2_HOST0–3. Each mode is set by an S5-domain strap bit through a syscon (`sky1_set_mode_by_id`). The glue uses SCMI clocks "sof_clk", "usb_aclk", "lpm_clk", "usb_pclk" — [Sky1-Linux patch 0004 (cdnsp-sky1.c, 797 lines)](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0004-usb-phy-Add-CIX-Sky1-USB-and-PHY-drivers.patch)
- **Controller addresses and DT modes** (vendor DTS/DTSI). All are `cdns,usbssp`, with reg-names "otg", "dev", "xhci".

  | Controller | Base | DT mode / notes |
  |---|---|---|
  | usbss_0 | 0x09010000 | `dr_mode = "otg"` + usb-role-switch to RTS5453 connector 0 |
  | usbss_1 | 0x09080000 | host; USB3 PHY disabled; USB2 PHY is `usb-nop-xceiv` |
  | usbss_2 | 0x090F0000 | otg + role switch to RTS5453 connector 1 |
  | usbss_3 | 0x09160000 | host; USB3 PHY disabled |
  | usbss_4 / usbss_5 | 0x091D0000 / 0x091E0000 | host; share a dual-port `usb3_phy4` (the two USB-A 3.2 ports) |
  | usbhs_0..3 | 0x09260000 / 0x09290000 / 0x092C0000 / 0x092F0000 | host, HS-only |

  Source: [Sky1-Linux patch 0001](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0001-arm64-dts-cix-Add-Sky1-SoC-and-Radxa-Orion-O6-device.patch)
- **Why the generic driver suffices under ACPI** (Sky1-Linux patch 0086): "Under ACPI, firmware has already initialized USB controller clocks, resets, and mode configuration. The xHCI register space is driven by the generic xhci-plat driver via PNP0D10 platform devices". Re-running the CIX `drd_init` "killed xHCI controllers that were already active" — [patch 0086](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0086-usb-cdns3-Skip-destructive-hardware-reinit-under-ACP.patch). A companion patch "phy-cix-usbdp: Skip PHY reset under ACPI boot" (0087) — [patch list](https://github.com/Sky1-Linux/linux-sky1/tree/main/patches)
- **ACPI USB-side devices** on the user's board are all unbound in mainline (local probe):
  - CIXH2030 (SUB0–5, HUB0–3) and CIXH2031 (CUB*): cdnsp-sky1 glue
  - CIXH2032 (U2P0–9): USB2 PHYs
  - CIXH2033 (UCP0–3): USB/DP combo PHYs
  - CIXH2034 (U3P4): USB3 PHY
  - CIXH200D (`\_SB.I2C1.PD10`, `PD11`): RTS5453
  
  The ID→driver mapping comes from Sky1-Linux patches: cdnsp-sky1.c, phy-cix-usb2.c, phy-cix-usbdp.c, phy-cix-usb3.c and rts5453.c — [patch 0065](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0065-treewide-Add-ACPI-device-IDs-for-CIX-Sky1-SoC-periph.patch) and patch 0004
- **RTS5453H interface.** The driver is described as "Realtek RTS5453H USB Type-C Power Delivery controller, providing USB role switching and DisplayPort alternate mode". It uses plain I2C regmap registers (`RTS5453_REG_DATA_CTRL 0x50`, `RTS5453_REG_DATA_STATUS 0x5F`, 6-byte status) with bits for orientation, USB2/USB3 connect, data role, DP connect, HPD state/IRQ and power source/sink. It is 611 lines. In the vendor O6 DTS both PD nodes are `status = "disabled"` with the comment "Temporarily disabled - race condition with cdns_role_stop" — [patch 0004](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0004-usb-phy-Add-CIX-Sky1-USB-and-PHY-drivers.patch); [patch 0001](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0001-arm64-dts-cix-Add-Sky1-SoC-and-Radxa-Orion-O6-device.patch)
- **Mainline status** per the CIX wiki: "USB — Reviewing" (Peter Chen, 2026-04-21); "USB3 PHY — Reviewing"; "USB/DP Combo PHY — Planning" — [cixtech wiki](https://github.com/cixtech/linux-mainline/wiki)
- **Documentation.** The TRM Part 2 (3,051 pages) covers **USB** among other blocks (registration required) — [CNX, 2025-12-13](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-de). The xHCI spec itself is public (Intel), and the generic xhci-plat driver needs no CIX docs.

### Inferences
- For a Rust OS: discover `PNP0D10` `_CRS` (MMIO + GSIV) from the DSDT, or hard-code the ten MMIO bases. Run a generic xHCI driver on each, then add hub, HID, mass-storage and Bluetooth class drivers. **No CIX register access is needed for host mode**, provided you do not reset the controllers through the CIX glue path. Plain xHCI HCRST should be checked carefully; Linux's xhci-plat does do HCRST and it works.
- The USB-C ports work as hosts out of the box. PD power input is negotiated by the RTS5453H firmware with no OS involvement (the board powers up from PD before any OS runs).
- Device/OTG mode needs the Cadence gadget side ("dev" regs), CIX straps and the RTS5453 role events. That makes it a late, low-priority feature.
- DP alt-mode needs the display stack (other researchers' scope) plus RTS5453 HPD events.
- Each controller has a single port, so a new OS needs **10 xHCI instances**: an OS design that assumes "one xHCI" will need generalising early.

### Gaps
- The exact ACPI-node → physical-connector map (XHC0..5/USB0..3 ↔ rear ports/front header/M.2 E-key) is not documented. The XHCn↔usbss_n ordering is an inference.
- I did not determine whether the RTS5453H firmware is field-updatable or whether the ACPI firmware talks to it (for example in AML).

---

## 4. Ethernet: 2× Realtek RTL8126 5GbE

### Takeaway
Each NIC is a standard PCIe function (10ec:8126, MAC "RTL8126A", Gen3 x1 link). There is **no public Realtek datasheet**, but the register interface is the well-trodden r8169/RTL8125 family and there are several open reference drivers: Linux r8169, OpenBSD/FreeBSD/NetBSD `rge(4)`, the DPDK r8169 PMD, and Realtek's out-of-tree r8126. The PHY/MCU firmware patch is optional in Linux. A basic polled or MSI-X Rust driver is a moderate project. The best Rust starting points are the no_std RTL8125 driver in rcore-os/tgoskits (RTL8125A/B only) and Redox's small rtl8168d (8168/8169 only).

### Cited Findings
- **Observed** (local probe: `ethtool -i enp1s0/enp49s0`): driver r8169, `firmware-version: rtl8126a-2_0.0.2 02/01/24` (the `rtl_nic/rtl8126a-2.fw` patch). One MSI-X vector each. hwmon sensors `r8169_0_100:00` and `r8169_0_3100:00`. NIC LEDs exposed as `enp1s0-[0-3]::lan`.
- **Linux r8169** maps XID 0x649 → `RTL_GIGA_MAC_VER_70 "RTL8126A"` with `rtl_nic/rtl8126a-2.fw`, and XID 0x64a with `rtl8126a-3.fw`. It adds `MAC_5000FD` for ≥VER_70. `r8169_main.c` is ~7,080 lines (~188 KB); `r8169_phy_config.c` is ~41 KB. If the firmware request fails the driver frees the struct and continues, so the firmware is non-fatal — [r8169_main.c](https://github.com/torvalds/linux/blob/master/drivers/net/ethernet/realtek/r8169_main.c) (fetched 2026-10-25, lines 66–67, 147–148, 2905–2928, 6595–6597); directory sizes from the [GitHub API listing](https://github.com/torvalds/linux/tree/master/drivers/net/ethernet/realtek)
- **BSD rge(4).**
  - OpenBSD `rge(4)` supports RTL8125, RTL8126 (10/100/1000/2500/5000) and RTL8127 — [OpenBSD rge(4)](https://man.openbsd.org/rge)
  - FreeBSD `rge(4)` was ported from OpenBSD by Adrian Chadd and covers RTL8126 — [FreeBSD rge(4)](https://man.freebsd.org/rge(4))
  - NetBSD `rge(4)` lists the 8126 — [NetBSD rge.4](https://nxr.netbsd.org/raw/src/share/man/man4/rge.4)
  - OpenBSD `if_rge.c` is ~132 KB + `if_rgereg.h` 16 KB. Commit 2026-10-13: "Split the rge(4) microcode into sys/dev/microcode/rge". Another commit added "RX queues/RSS/TSO support … for RTL8127" — [OpenBSD sys/dev/pci](https://github.com/openbsd/src/tree/master/sys/dev/pci); [commit log](https://github.com/openbsd/src/commits/master/sys/dev/pci/if_rge.c)
  - FreeBSD `sys/dev/rge`: if_rge.c ~69 KB, if_rge_hw.c ~71 KB, **if_rge_microcode.h ~421 KB** (MAC/PHY MCU tables embedded as C arrays) — [FreeBSD sys/dev/rge](https://github.com/freebsd/freebsd-src/tree/main/sys/dev/rge)
- **DPDK** has an r8169 poll-mode driver covering RTL8125 (2.5G) and RTL8126 (5G) — [DPDK r8169 guide](https://doc.dpdk.org/guides/_sources/nics/r8169.rst.txt)
- **Vendor driver.** Realtek's out-of-tree r8126 (as carried by Sky1-Linux) is ~17,257 lines in `r8126_n.c` alone, plus PTP, RSS, DASH and fiber modules. Sky1-Linux also adds "IRQ affinity hint", "Enable WoL and RSS support" and a "shutdown panic" fix — [Sky1-Linux patch 0008](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0008-net-Add-CIX-Sky1-networking-drivers.patch); patches 0014/0042/0055 in the [list](https://github.com/Sky1-Linux/linux-sky1/tree/main/patches)
- **Windows.** A forum user reports the RTL8126 working on Windows 11 on the O6 with a Realtek driver (Wi-Fi did not) — [Radxa forum thread](https://forum.radxa.com/t/how-to-configure-cpu-frequency-to-2-6ghz-and-ethernet-wifi-driver-questions/25876) (seen via search snippet only)
- **Rust prior art:**
  - `realtek-rtl8125` v0.3.3 (created 2026-05-08, updated 2026-09-09), "Minimal Realtek RTL8125 PCIe Ethernet driver", in rcore-os/tgoskits. It is ~800 lines of lib.rs plus hw/queue/registers/descriptor modules. It handles `Rtl8125A`/`Rtl8125B` chip versions with EPHY init tables and OCP PHY base 0xa400; there is no 8126 entry — [crates.io realtek-rtl8125](https://crates.io/crates/realtek-rtl8125); [tgoskits source](https://github.com/rcore-os/tgoskits/tree/main/drivers/net/realtek-rtl8125)
  - Redox `rtl8168d` (~460 lines Rust, `ids = { 0x10ec = [0x8168, 0x8169] }`) — [Redox base drivers](https://gitlab.redox-os.org/redox-os/base/-/tree/main/drivers/net)

### Inferences
- **Recommended path.** Port or extend the tgoskits RTL8125 Rust driver to RTL8126 (MAC_VER_70), cross-checking against Linux `rtl_hw_start_8126a` and OpenBSD rge's R26 paths. The RTL8125 and RTL8126 share descriptor format and interrupt layout in r8169's "8125 family" code paths.
- **Bring-up phases.** Start without the firmware patch, then add `rtl8126a-2.fw` loading. Its format is the r8169 "rtl_fw" opcode list, documented by `r8169_firmware.c` (~5 KB). The file is redistributable in linux-firmware.
- **5 Gb/s performance.** The Gen3 x1 link (~7.9 Gb/s raw) is sufficient. Getting near line rate needs:
  - MSI-X with interrupt moderation and large RX/TX rings
  - Checksum offload; TSO for TX-heavy loads
  - Cache-coherent DMA. The PCIe path is coherent through the CI-700 interconnect in DT terms; Sky1-Linux adds `dma-coherent` to SMMU nodes.
  - Possibly multi-queue/RSS. OpenBSD and Realtek added RSS; mainline r8169 is single-queue and still does 5G.
  
  A Rust driver with a zero-copy buffer pool should reach multi-Gb/s on A720 cores. This is not measured.
- **Difficulty:** medium. The main risks are undocumented PHY bring-up sequences (copy from r8169/rge) and EEE/ASPM quirks (disable both).

### Gaps
- No public RTL8126 datasheet or programming guide exists (Realtek NDA). I found no RTL8126 support in any Rust project (tgoskits' crate is 8125-only).
- I did not quantify mainline r8169 throughput on this board.

---

## 5. Wi-Fi/Bluetooth: RTL8852BE (user-added M.2 E-key card)

### Takeaway
The RTL8852BE is an add-in card, not part of the board. Radxa sells it as "Wireless Module A8", and the user's BT USB ID 13d3:3570 (IMC Networks/AzureWave) suggests an AzureWave-built module. Wi-Fi via `rtw89` is by far the hardest on-board-ish driver: megabytes of C, mandatory firmware, RF calibration, regulatory/TX-power tables, and a full 802.11 + WPA stack on top. Bluetooth is tractable at the transport level: USB btusb plus Realtek firmware download. A full BT host stack is still a big project. Recommendation: **skip Wi-Fi early and use the 5GbE NICs**.

### Cited Findings
- **Observed** (local probe: `lspci`, `lsusb`, `/proc/interrupts`):
  - Wi-Fi: `61:00.0 10ec:b852` RTL8852BE on `rtw89_8852be` (MSI via ITS), link 2.5 GT/s x1.
  - BT: `13d3:3570 IMC Networks Bluetooth Radio` on USB2 controller `\_SB.USB2`, bound to btusb/btrtl.
  - Firmware installed: `rtw89/rtw8852b_fw-2.bin.xz` (plus -1 and unversioned) and `rtl_bt/rtl8852bu_fw.bin.xz` + `rtl8852bu_config.bin.xz` (local probe: `modinfo rtw89_8852b`, `modinfo btrtl`, `ls /usr/lib/firmware`).
- **ACPI power plumbing** (local probe): PRP0001 regulator-fixed nodes `\_SB.VWL0` (Wi-Fi) and `\_SB.VBT0` (BT) exist. Sky1-Linux had to "Fix WiFi RFKill GPIO on Orion O6 and O6N" (patch 0018) — [patch list](https://github.com/Sky1-Linux/linux-sky1/tree/main/patches)
- **Radxa A8 module:** M.2 2230, RTL8852BE, Wi-Fi 6 up to 1201 Mbps at 80 MHz, BT 5.2 over USB — [Radxa A8 docs](https://docs.radxa.com/en/accessories/wireless-a8)
- **rtw89 size** (mainline, fetched 2026-10-25): 105 .c/.h files, ~9.8 MB total for all chips. Core files: core.c 231 KB, fw.c 388 KB, mac.c 225 KB, phy.c 267 KB, pci.c 137 KB, regd.c 40 KB. 8852B-specific: rtw8852b.c 38 KB, rtw8852b_common.c 73 KB, rtw8852b_rfk.c 135 KB, rtw8852b_rfk_table.c 33 KB, **rtw8852b_table.c 749 KB** — [rtw89 directory](https://github.com/torvalds/linux/tree/master/drivers/net/wireless/realtek/rtw89)
- **A Rust Wi-Fi driver exists for another chip:** `aic8800` "OS-independent AIC8800 Wi-Fi driver core" in rcore-os/tgoskits — [tgoskits drivers/net/aic8800](https://github.com/rcore-os/tgoskits/tree/main/drivers/net/aic8800). `openipc-rtl88xx` is a Rust USB RX-only driver for older rtl88xx (FPV use) — [crates.io openipc-rtl88xx](https://crates.io/crates/openipc-rtl88xx)
- **Redox OS:** its hardware notes list Wi-Fi as unsupported — [Redox HARDWARE.md (older snapshot)](https://git.berlin.ccc.de/vinzenz/redox/src/commit/2a0cec1bdb637da999c86ff8287142daf5cd24ef/HARDWARE.md)

### Inferences
- **Rough Wi-Fi effort** for a new OS: rtw89-equivalent PCI DMA rings, firmware download and H2C/C2H mailbox, efuse/calibration, BB/RF table loading and per-country TX-power limits. Then on top: an 802.11 MAC layer (scanning, association), WPA2/3 4-way handshake and crypto, and regulatory policy. That is plausibly person-years at production quality. Even a "connect to one WPA2 AP" demo is a multi-month effort.
- **Alternatives:**
  - (1) Skip Wi-Fi; Ethernet is far easier.
  - (2) A USB Wi-Fi dongle still needs an 802.11 stack and vendor firmware, so it is not much easier.
  - (3) Use the BT side first: btusb is a standard USB HCI transport. Realtek needs a vendor-command firmware download (`rtl8852bu_fw.bin` + `config.bin`, see Linux `btrtl.c`) before the controller is usable.
- **Firmware licensing:** the Realtek blobs are redistributable in linux-firmware. The OS needs a filesystem or initrd path to load them.

### Gaps
- I did not count lines in btrtl.c or quantify a minimal BT host stack. I found no Rust implementation of rtw89 or of btrtl firmware download.

---

## 6. Storage and boot media

### Takeaway
The O6 has **NVMe only** on board (M.2 M-key, Gen4 x4) plus USB mass storage. There is no eMMC, SD or SATA. UFS exists only on the O6N via a module connector. Firmware lives in an 8 MB 1.8 V SPI NOR (W25R64JW) that is shared with the EC and socketed. NVMe is fully standards-based and has plentiful Rust prior art.

### Cited Findings
- **Storage interfaces:** one M.2 M-key (PCIe Gen4 x4) for NVMe; no other storage is listed — [Radxa O6 docs](https://docs.radxa.com/en/orion/o6); [product brief](https://dl.radxa.com/orion/o6/docs/radxa_orion_o6_product_brief.pdf). Radxa OS images ship as "USB / NVMe", and a separate UFS image exists for **O6N only** — [Radxa downloads](https://docs.radxa.com/en/orion/download)
- **Observed:** `91:00.0 c0a9:540a` Crucial P3 on `nvme` with 9 MSI-X vectors (nvme0q0..q7 + admin) via ITS, at Gen3 x4 — (local probe)
- **BIOS flash:** SOP8 socket, 8 MB/64 Mbit, 1.8 V — [Radxa hardware-info](https://docs.radxa.com/en/orion/o6/hardware-use/hardware-info). The schematic labels it "BIOS&EC SHARED ROM" with a Winbond W25R64JW — [O6 schematic](https://dl.radxa.com/orion/o6/hw/radxa_orion_o6_v1.20_schematic.pdf)
- **The SoC's XSPI** (`cdns,xspi-nor` @0x04180000) appears in ACPI as CIXH2002 `\_SB.XSPI` with **status 0 (disabled)** — (local probe). The vendor DT also disables `&xspi` (flash@0 jedec,spi-nor) on the O6 — [patch 0001](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0001-arm64-dts-cix-Add-Sky1-SoC-and-Radxa-Orion-O6-device.patch)
- **Rust NVMe drivers:** `nvme-driver` v0.8.2 (rcore-os/tgoskits, updated 2026-09-09) — [crates.io](https://crates.io/crates/nvme-driver); Redox `nvmed` — [Redox base storage drivers](https://gitlab.redox-os.org/redox-os/base/-/tree/main/drivers/storage)

### Inferences
- **Boot flow for a research OS:** UEFI loads a Rust EFI app or kernel from the ESP on NVMe or a USB stick (the `uefi` crate). Keep the NVMe driver generic (NVMe 1.4+, MSI-X via ITS or polled).
- **Do not write the SPI NOR from the OS.** The XSPI is firmware-reserved and shared with the EC, so a write risks bricking the board. Leave firmware updates to the UEFI capsule (ESRT is present: `/sys/firmware/efi/esrt`) or to an external programmer.

### Gaps
- I did not check whether UEFI exposes the NOR through `EFI_FIRMWARE_MANAGEMENT_PROTOCOL`/capsules for OS-driven updates beyond the presence of ESRT.

---

## 7. Low-speed peripherals and system devices (UART, I2C, SPI, GPIO/pinctrl, PWM/fan, RTC, watchdog, thermal, buttons, LEDs, TPM, mailbox/SCP, EC)

### Takeaway
Under ACPI only the **SBBR-standard** devices get drivers in mainline Linux 7.2:
- one PL011 UART (UART2 @0x040D0000) through ARMH0011
- the SBSA generic watchdog (GTDT)
- the EFI RTC through UEFI runtime services
- six ACPI thermal zones
- the ACPI fan (2-state)
- the ACPI power button
- OP-TEE (RNG PTA only)

Every CIX low-speed block (Cadence I2C/SPI/GPIO/I3C, CIX pinctrl, PWM, I2S, mailbox, DMA-350, SCMI) is described in the DSDT under CIXHxxxx IDs but is **unbound**: mainline CIX drivers are DT-only, and ACPI IDs exist only in community/vendor patches. Firmware pre-muxes pins and powers blocks.

Several blocks are licensed IP with **public** register docs: PL011, Cadence I2C/SPI (the same IP as Xilinx Zynq), Arm DMA-350, SBSA watchdog and the SCMI protocol. The rest (pinctrl, PWM, fan, mailbox, GPIO) are small enough to learn from 200–600-line vendor drivers or the CIX TRM.

### Cited Findings — ACPI ID → IP → driver map
Columns: ACPI ID / node(s) and status on the user's board (local probe); the matching driver from Sky1-Linux ACPI patches; DT base and IP.

| ACPI ID (nodes, status) | Block / IP | Linux driver (ACPI ID added in) | DT base(s) |
|---|---|---|---|
| ARMH0011 COM0–3 (only COM2 = 0xF, others 0) | Arm PL011 UARTs (FCH UART0–3) | mainline `sbsa-uart` (bound to COM2 → ttyAMA0, iomem 0x40D0000) | uart0–3 @0x040B0000/0x040C0000/0x040D0000/0x040E0000; uart4 @0x06540000 (PM "CSU"); uart5 @0x05020000 (SE) |
| CIXH200B I2C0–7 (I2C2/3/6 status 0) | Cadence I2C r1p14 | i2c-cadence (patch 0065) | 0x04010000 … 0x04080000 |
| CIXH200C I3C0–1 (status 0) | Cadence I3C master | i3c-master-cdns (0065) | i3c0 @0x040F0000 |
| CIXH2001 SPI0–1 (0xF) | Cadence SPI r1p6 ("cix,sky1-spi-r1p6") | spi-cadence (0065) | 0x04090000, 0x040A0000 |
| CIXH2002 XSPI (status 0) | Cadence XSPI NOR | spi-cadence-quadspi (0065) | 0x04180000 |
| CIXH1002 GPI0, CIXH1003 GPI1–6 (0xF) | Cadence GPIO r1p02 (4 FCH + 3 S5 banks) | gpio-cadence (patch 0058) | FCH 0x04120000–0x04150000; S5 0x16004000–0x16006000 |
| CIXHA016 MUX0, CIXHA017 MUX1 (0xF) | CIX pinctrl (main + S5) | pinctrl-sky1 (patch 0080 etc.) | iomuxc / iomuxc_s5 |
| CIXH2011 PWM0 (0xF), PWM1 (0) | CIX PWM ("cix,sky1-pwm") | pwm-sky1 (291 lines, patch 0012) | 0x04110000, 0x04111000 |
| CIXHA024 HWMN (status 3) | CIX fan controller (PWM + tach) | cix-fan hwmon (patch 0064) | — |
| CIXH1005 (not present here) | CIX watchdog | sky1_wdt (431 lines) | 0x16003000 |
| CIXH1007 TMR0 | CIX GPT timer (2×32-bit → 64-bit) | timer-sky1-gpt (patch 0063) | — |
| CIXH1006 DMA1, CIXHA014 DMA0 | Arm **DMA-350** (FCH "dma350-full" @0x04190000; audio @0x07010000) | arm-dma350 (0065) | — |
| CIXHA001 MBX0–7 (MBX2/3 = 0) | CIX mailbox ("cix,sky1-mbox") | cix-mailbox (274 lines; ACPI in 0071/0075) | AP↔PM 0x06590000/0x065A0000; AP↔SE 0x05060000/0x05070000; AP↔SensorHub 0x08090000/0x080A0000; AP↔DSP 0x070F0000/0x07100000 |
| CIXHA004 SHM0, CIXHA005 SHM1 | SCMI shared memory | arm_scmi shmem (0072) | — |
| CIXHA006 SCMI (+CIXHA008 DVFS, CIXHA009 CLKS = 0) | SCMI over mailbox to the PM/SCP | arm_scmi mailbox transport (0061/0074) | — |
| CIXHA010 CCLK; CIXHA018 UCRU/CRU0/GCRU/ACRU/DCRU | ACPI clock shim; syscon register blocks | clk-sky1-acpi (0076); syscon (0062) | — |
| CIXHA020 RST0, CIXHA021 RST1 | CIX reset controllers (main, FCH) | reset-sky1 (421 lines) | — |
| CIXA1019 (seen in patches) | CIX "RSTL/RSNL/DLKL" custom DSDT tables for reset, resource naming and device links | cix-acpi-resource-lookup (0077) | — |
| CIXH6010 I2S0–2, CIXH6011 I2S3–9 (only I2S5–9 = 0xF) | Cadence I2S ("cdns,sky1-i2s-sc" for i2s0–2, "-mc" for i2s3–9) | vendor ASoC only | 0x07020000 … 0x070B0000 |
| CIXH6020 HDA_ | CIX "IPBLOQ" HD-Audio controller (+ ALC256 codec) | snd-hda-cix-ipbloq (mainline v6.19, DT-only; ACPI in patch 0083) | 0x070C0000 |
| CIXH6060/6061/6062 ADSS/ACLK/ARST | Audio subsystem clock/reset | clk/reset-sky1-audss (0065) | — |
| CIXH6000 DSP_ | Cadence HiFi5 DSP | cix_dsp_rproc | 0x07000000 |
| CIXH200D PD10/PD11 (under I2C1) | Realtek RTS5453H PD | rts5453 (0065) | I2C1 0x30/0x31 |
| CIXH7020 MAC0/1 (status 0) | Cadence GEM (not wired on O6) | macb (0065) | 0x09320000 |
| CIXHA022 TEE0 | OP-TEE | mainline optee (bound) | — |
| CIXHA015 EC0_ (status 3) | Board EC (ITE IT5570E) | none found | I2C6 0x76 in vendor DT |
| CIXHA019 PDC0, CIXHA026 PEP0, CIXHA012/13 PMA_/PMGM, CIXHA000 PMMX, CIXHA023 TREE | Power-domain/wake controller, Windows-style PEP, PM helpers | not mapped | — |
| ERTC0000 ERTC (status **0**) | "External RTC" placeholder, disabled | — | — |
| PRP0001 LEDS (gpio-leds, status 0xB); PVC0–4, VWL0, VBT0, VUS0/4/5, CPE4 (regulator-fixed) | GPIO LEDs and GPIO-switched supplies (PCIe, Wi-Fi, BT, USB VBUS) | leds-gpio / regulator-fixed (fwnode path in 0093) | — |

Sources for the table:
- Statuses and paths: (local probe: `/sys/bus/acpi/devices/*/{path,status,modalias}`).
- ID→driver mapping: [Sky1-Linux patch 0065](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0065-treewide-Add-ACPI-device-IDs-for-CIX-Sky1-SoC-periph.patch), [0058](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0058-gpio-cadence-Add-edge-IRQ-PM-wake-and-ACPI-support.patch), [0061](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0061-firmware-arm_scmi-Add-ACPI-support-to-transport-driv.patch), [0062](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0062-mfd-syscon-Add-ACPI-platform-driver-support.patch), [0063](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0063-clocksource-Add-CIX-Sky1-GPT-timer-driver.patch), [0064](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0064-hwmon-Add-CIX-Sky1-fan-controller-driver.patch), [0074](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0074-firmware-arm_scmi-Add-ACPI-boot-support-for-CIX-Sky1.patch), [0077](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0077-soc-cix-Add-ACPI-resource-lookup-driver.patch), [0080](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0080-drivers-Fix-ACPI-boot-failures-for-CIX-Sky1-peripher.patch), plus 0003/0004/0012 for line counts.
- DT addresses and compatibles: [patch 0001 sky1.dtsi](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0001-arm64-dts-cix-Add-Sky1-SoC-and-Radxa-Orion-O6-device.patch).
- The I2S split (6010 = i2s0–2, 6011 = i2s3–9) is inferred from node counts matching DT "sc"/"mc" compatibles.

The camera, display, GPU and NPU IDs (CIXH30xx, CIXH50xx, CIXH40xx, CIXH5000) are outside this note's scope. Sky1-Linux maps:
- CIXH5010 → linlon-dp DPU
- CIXH502F → trilin DPTX
- CIXH503F → virtual display
- CIXH5040 → eDP panel
- CIXH5041 → pwm-backlight
- CIXH5000 → panthor

Also, CIXH3023 `\_SB.I2C0.MTR0` matches the vendor DT "motor,ms42919" at I2C0 0x40, and CIXH3024 IIS0/IIS1 under I2C0/I2C1 match camera sensors at 0x34/0x36 — [patch 0001](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0001-arm64-dts-cix-Add-Sky1-SoC-and-Radxa-Orion-O6-device.patch).

### Cited Findings — per device
- **Mainline is DT-only for every CIX block.** In the openSUSE 7.2.11 kernel, `pinctrl_sky1`, `cix_mailbox`, `i2c_cadence`, `spi_cadence`, `gpio_cadence`, `arm_dma350` and `snd_hda_cix_ipbloq` expose only `of:` aliases (for example `cix,sky1-pinctrl`, `cix,sky1-mbox`, `cdns,i2c-r1p14`, `cix,sky1-spi-r1p6`, `cdns,gpio-r1p02`, `arm,dma-350`). No module has an `acpi*:CIX*` alias — (local probe: `modinfo`, `modules.alias`)
- **Mainline merge timeline** (CIX wiki, last edited 2026-10-19):

  | Version | Merged blocks |
  |---|---|
  | v6.17 | basic DT, mailbox, UART, clock |
  | v6.18 | I2C & I3C |
  | v6.19 | SPI, pinctrl, PCIe, HDA |
  | v7.1 | reset, GPIO, power domain, DPU |
  | v7.2 | CPU idle, CPU DVFS |
  | v7.3-rc1 | DMA |

  "Reviewing": USB, USB3 PHY, I2S, DP. "Planning": USB/DP combo PHY, GPU. "Won't do": VPU, CSI, RPMSG, GMAC, system PM. PWM, watchdog, fan and RTC are not listed — [cixtech/linux-mainline wiki](https://github.com/cixtech/linux-mainline/wiki). A CIX defconfig pull for v7.2-rc1 enables Sky1 pinctrl, PCIe host and Cadence GPIO — [GIT PULL CIX defconfig v7.2](https://ratatoskr.run/linux-arm-kernel/2026/05/8993766)
- **Pin muxing under ACPI is done by firmware.** Sky1-Linux patch 0091: "pinctrl-sky1: Accept ACPI boot (firmware handles pin muxing)" — [patch 0091](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0091-PCI-sky1-Switch-ACPI-from-MCFG-quirk-to-vendor-scan-.patch)
- **UARTs.**
  - Only `\_SB.COM2` (ARMH0011:02) is enabled. It is bound to `sbsa-uart` as `ttyAMA0` at iomem 0x40D0000.
  - The 32 `ttyS*` devices are empty serial8250 placeholders (port 0x0, iomem 0x0, type 0), **not real hardware**.
  - DBG2 is present; **no SPCR table** — (local probe: `/sys/class/tty/*/{iomem_base,type}`).
  - The schematic names the console net "UART2_UEFI". UART4/5 are "CSU" (PM/SE) consoles — [schematic](https://dl.radxa.com/orion/o6/hw/radxa_orion_o6_v1.20_schematic.pdf); [Radxa hardware-info](https://docs.radxa.com/en/orion/o6/hardware-use/hardware-info)
- **I2C bus population** (vendor O6 DTS):

  | Bus | Devices |
  |---|---|
  | I2C0 | camera sensor 0x34, ms42919 motor 0x40 |
  | I2C1 | RTS5453H 0x30/0x31, camera sensor 0x36 |
  | I2C2 | Goodix GT911 touch 0x14 (eDP touch header) |
  | I2C3 | **HYM8563 RTC 0x51** |
  | I2C4 | 40-pin header (an optional NFC node is disabled) |
  | I2C5 | optional hid-over-i2c 0x3a (disabled) |
  | I2C6 | **"google,cros-ec-i2c" EC at 0x76**, with cix-ec-pwrkey and cix-ec-sleevekey enabled; EC pwm/pwm-fan/gpio children disabled |
  | I2C7 | disabled |

  Source: [patch 0001 DTS](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0001-arm64-dts-cix-Add-Sky1-SoC-and-Radxa-Orion-O6-device.patch). In ACPI, I2C2, I2C3 and I2C6 are status 0 (hidden from the OS) — (local probe)
- **Cadence I2C documentation.** The IP is the same Cadence IIC used in Xilinx Zynq-7000 ("Cadence IP … T-CS-PE-0007-100, Version R1p10f2"). U-Boot's zynq_i2c/i2c-cdns drivers define its register struct (control, status, address, data, ISR, transfer size, timeout, IMR/IER/IDR) — [U-Boot zynq_i2c.c](https://third-party-mirror.googlesource.com/u-boot/+/087cf44fcd237d965ecccd6cf9e52de8d3c51a2e/drivers/i2c/zynq_i2c.c); [U-Boot i2c-cdns.c](https://third-party-mirror.googlesource.com/u-boot/+/bd65d23c7bce18a15157d23854439df4eecb151b/drivers/i2c/i2c-cdns.c). "cdns,i2c-r1p14" is version 1.4, which fixes r1p10 hold-bit bugs — [LKML 2015 patch](https://lkml.iu.edu/hypermail/linux/kernel/1507.1/02500.html). The Linux `spi-cadence` driver matches both `xlnx,zynq-spi-r1p6` and `cix,sky1-spi-r1p6`, i.e. the same SPI IP as Zynq — (local probe: `modinfo spi_cadence`)
- **CIX TRM Part 2** covers "USB, the FCH (GPIO, SPI, I2C, UART, I3C, timer, XSPI, DMA), DDR, ISP, DP, audio, MIPI, and debug". Mailbox, pinctrl, clocks, SCMI and PWM are **not listed**; PWM is covered in the "Linux DT Development Guide". Registration is required — [CNX, 2025-12-13](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-de)
- **RTC.**
  - The board RTC is a **HYM8563TS** on I2C3 (0x51), backed by the CR1220 — [schematic](https://dl.radxa.com/orion/o6/hw/radxa_orion_o6_v1.20_schematic.pdf); [patch 0001](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0001-arm64-dts-cix-Add-Sky1-SoC-and-Radxa-Orion-O6-device.patch)
  - Under ACPI, Linux uses **rtc-efi** (UEFI runtime GetTime/SetTime). `ERTC0000 \_SB.ERTC` and `\_SB.I2C3` are status 0 — (local probe: `/sys/class/rtc/rtc0/name` = rtc-efi)
  - Sky1-Linux adds "rtc-hym8563: Add second-level wake-up support" (patch 0060) — [patch list](https://github.com/Sky1-Linux/linux-sky1/tree/main/patches)
  - Radxa notes that removing the battery doesn't immediately reset BIOS settings — [Radxa hardware-info](https://docs.radxa.com/en/orion/o6/hardware-use/hardware-info)
- **Watchdog.** Linux binds the **SBSA Generic Watchdog** (identity "SBSA Generic Watchdog", from GTDT) — (local probe: `/sys/class/watchdog/watchdog0/identity`). A separate CIX `sky1-wdt` @0x16003000 exists in DT — [patch 0001](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0001-arm64-dts-cix-Add-Sky1-SoC-and-Radxa-Orion-O6-device.patch)
- **Thermal and fan.**
  - Six ACPI thermal zones (`acpitz`, step_wise) read 49–54.5 °C — (local probe: `/sys/class/thermal/thermal_zone*`)
  - Cooling devices are 12 "Processor" entries plus one **Fan at `\_SB.ECFP` (PNP0C0B, acpi-fan) with max_state 1**, i.e. on/off only — (local probe)
  - In DT, CPU thermal zones (cpu-b0/b1/m0/m1) use trips at 60/85/98 °C and SCMI-backed sensors (`ap_to_pm_scmi` "clocks, perf, sensors, pm_excp") — [patch 0001](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0001-arm64-dts-cix-Add-Sky1-SoC-and-Radxa-Orion-O6-device.patch)
  - The CIX fan controller provides "PWM-based fan speed control … Fan tachometer reading" through ACPI CIXHA024 — [patch 0064](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0064-hwmon-Add-CIX-Sky1-fan-controller-driver.patch)
  - CIX also published an "Active Cooling TRM" (33 pages) — [CNX, 2025-12-13](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-de)
- **Power button and EC.**
  - `PNP0C0C \_SB.PWRB` is bound to acpi-button — (local probe)
  - The EC is an **ITE IT5570E** (schematic sheet "35 EC Module"). Its net names include EC_PWRON, EC_SLP_S3_L/EC_SLP_S5_L, EC_SOC_RST, EC_I2C_ACPI_SCL/SDA, EC_I2C_HID_SCL/SDA, EC_ACPI_INT, PECI and fan TACH0–2. It shares the SPI ROM — [schematic](https://dl.radxa.com/orion/o6/hw/radxa_orion_o6_v1.20_schematic.pdf)
  - ACPI `\_SB.EC0_` (CIXHA015) is status 3 with no driver — (local probe)
- **LEDs.** ACPI `\_SB.LEDS` is PRP0001 "gpio-leds", status 0xB, and `leds_gpio` is loaded. However, `/sys/class/leds` shows only NIC and keyboard LEDs, with no board LEDs — (local probe). Sky1-Linux adds "GPIO LEDs and fix pinctrl on Orion" (patch 0019) — [patch list](https://github.com/Sky1-Linux/linux-sky1/tree/main/patches)
- **TPM.**
  - The footprint is unpopulated ("SPI TPM 2.0 (OPTION)", SLB9672XU2) — [Radxa hardware-info](https://docs.radxa.com/en/orion/o6/hardware-use/hardware-info); [schematic](https://dl.radxa.com/orion/o6/hw/radxa_orion_o6_v1.20_schematic.pdf)
  - There is no TPM2 ACPI table and no `/sys/class/tpm` — (local probe)
  - OP-TEE (`\_SB.TEE0` CIXHA022) is bound, and the only TA device is `optee-ta-ab7a617c-b8e7-4d8f-8301-d09b61036b64`, bound to **optee-rng**. That is a hardware RNG PTA; **no fTPM** was observed — (local probe: `/sys/bus/tee/devices/*`)
- **Mailbox and SCP.**
  - The CIX mailbox driver is 274 lines (vendor), and SCMI runs over it to the "PM" processor.
  - Vendor DT splits SCMI into "Mailbox SCMI - clocks, perf, sensors, pm_excp" (to PM) and "SMC SCMI - power domains (genpd)" (to TF-A) — [patch 0001](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0001-arm64-dts-cix-Add-Sky1-SoC-and-Radxa-Orion-O6-device.patch); [patch 0003](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0003-arm64-cix-Add-Sky1-infrastructure-drivers.patch)
  - ACPI SCMI is not used by mainline: under ACPI the SCMI node needed new code "When no DT node is present, enumerate all protocols reported by the firmware via base protocol discovery" — [patch 0074](https://github.com/Sky1-Linux/linux-sky1/blob/main/patches/0074-firmware-arm_scmi-Add-ACPI-boot-support-for-CIX-Sky1.patch)
  - A PCCT table exists, which Linux CPPC uses for cpufreq; a Sky1-Linux patch tweaks "cpufreq-cppc: Skip redundant frequency updates" (0045) — (local probe for PCCT; [patch list](https://github.com/Sky1-Linux/linux-sky1/tree/main/patches))
- **GPIO IRQ hardening.** Sky1-Linux patches "gpio-cadence: Fix IRQ storm and harden IRQ handling" (0067) and "usb: cdns3: Serialize drd_init to prevent SCMI mailbox …" (0082). Both show real-world pitfalls in GPIO interrupts and SCMI concurrency — [patch list](https://github.com/Sky1-Linux/linux-sky1/tree/main/patches)

### Inferences
- **Practical low-speed plan** for a new Rust OS, in rising order of difficulty:
  1. **PL011 console** on UART2 @0x040D0000 (from DBG2, or ACPI ARMH0011 `_CRS`). Keep the firmware's baud divisor instead of computing one, since the UART clock rate isn't exposed in ACPI. A Rust crate already exists.
  2. **SBSA watchdog** from GTDT: two register frames, public Arm spec.
  3. **RTC through UEFI runtime services.** This requires keeping the runtime-services mapping after ExitBootServices. The alternative is to drive the HYM8563 directly over Cadence I2C3, but firmware hides I2C3 in ACPI and probably uses it from the runtime GetTime path. **Do not drive I2C3 and also call GetTime.** Pick one.
  4. **Cadence I2C, SPI and GPIO** on the buses firmware leaves enabled. I2C0/1/4/5/7 and SPI0/1 are on; I2C2/3/6 are off. Register models are public through Zynq docs and Linux/U-Boot drivers. Pins are pre-muxed under ACPI, so no pinctrl driver is needed initially. A Cadence GPIO driver is ~a few hundred lines.
  5. **Thermal/fan/power-button through ACPI AML.** `_TMP`, fan `_ON/_OFF`, and Notify on PWRB require an AML interpreter: rust-osdev `acpi` v6.x includes one, or port ACPICA, which tgoskits wraps as `acpica-interpreter`. Without AML, use SCMI directly: the sensor-management protocol over the CIX mailbox to the PM. That needs the 274-line mailbox driver, SCMI shared memory and the public SCMI spec, plus the `arm-scmi-rs` crate.
  6. **PWM fan speed control** through CIXHA024 (vendor cix-fan) or PWM (pwm-sky1, 291 lines). The register docs are not in the TRM list, so learn from the vendor C. ACPI's fan is only on/off.
  7. **DMA-350:** only needed for audio/I2S/SPI offload. Arm's DMA-350 TRM is public and a Linux driver exists.
  8. **I2S/HDA audio:** the HDA controller plus ALC256 codec is the cheaper audio path (HDA spec public); I2S needs the vendor ASoC path. It is lower priority.
- **Register-level docs.** Everything "CIX-proprietary" is small: pinctrl ~1.2k lines, mailbox 274, PWM 291, reset 421, fan hwmon small. A research OS can treat the GPL vendor drivers as documentation. Reimplementing them clean-room in Rust is reasonable; copying code is not, because of the GPL license.
- **Unbound gpio-leds.** The board LEDs (`\_SB.LEDS`) depend on the Cadence GPIO controllers, so in mainline-ACPI Linux the gpio-leds device most likely fails to find its GPIO provider. A new OS gets board LEDs only after a Cadence GPIO driver plus `_DSD`/`_CRS` GpioIo parsing.
- **Who owns which device.** Firmware deliberately hides I2C2, I2C3, I2C6, XSPI, ERTC and the EC I2C from the OS (status 0). The likely reason is that firmware or the EC owns them at runtime (RTC, EC, SPI ROM). Respecting `_STA` avoids bus contention.

### Gaps
- The DSDT could not be read (root only), so these remain unknown:
  - how `_TMP` is implemented: PCC/SCMI OpRegion, EC, or something else
  - how the power-button event is delivered (no GED ACPI0013 was enumerated; possibly `_AEI` on GPIO or EC)
  - the exact `_CRS` IRQ numbers for I2C/SPI/GPIO
  - the `_DSD` properties: clock rates for I2C/SPI input clocks, which a driver needs to compute SCL/SCK dividers
- I did not confirm whether the RTC runtime service actually uses HYM8563 over I2C3.
- I found no public register spec for the CIX mailbox, PWM, fan controller or pinctrl beyond vendor drivers. Whether the TRM includes them is unclear: CNX's list says no.
- I did not verify whether OP-TEE offers an fTPM TA on other BIOS versions.

---

## 8. Cross-cutting: standards vs CIX-specific, docs, mainline vs vendor, Rust prior art, difficulty

### Takeaway
Booting with ACPI gives a new OS a "PC-like" subset that is fully standard: ECAM PCIe + GICv3 ITS + SMMUv3 (optional), 10× xHCI, NVMe, PL011, SBSA watchdog, GTDT arch timer, UEFI RTC/GOP, and ACPI thermal/fan/button (with AML). Everything else is CIX-specific: Cadence/CIX low-speed IP, mailbox/SCMI, PHYs, PD, display and audio. Rust prior art is surprisingly rich in 2026, especially rcore-os/tgoskits and the rust-osdev and Trusted-Firmware crates. CIX/vendor sources plus the gated TRM cover most of the rest.

### Cited Findings
- **Rust crates** (crates.io, checked 2026-10-25):
  - `acpi` v6.1.1 (updated 2026-10-23, rust-osdev, "A pure-Rust library for interacting with ACPI"). The old `aml` crate's last release was 0.16.4 in 2023, which suggests AML moved into `acpi` — [crates.io acpi](https://crates.io/crates/acpi); [crates.io aml](https://crates.io/crates/aml)
  - `xhci` v0.9.2 (2023, register definitions) — [crates.io xhci](https://crates.io/crates/xhci)
  - `uefi` v0.41.0 (2026-09-21) — [crates.io uefi](https://crates.io/crates/uefi)
  - `arm-gic` v0.10.0 (2026-09-25; GICv2/3/4, Trusted-Firmware crates) — [crates.io arm-gic](https://crates.io/crates/arm-gic)
  - `arm-pl011-uart` v0.5.1 (2026-08-13) — [crates.io arm-pl011-uart](https://crates.io/crates/arm-pl011-uart)
  - `smccc` v0.2.3 (Google) — [crates.io smccc](https://crates.io/crates/smccc)
  - `safe-mmio` v0.3.2 (Google, 2026-09-28) — [crates.io safe-mmio](https://crates.io/crates/safe-mmio)
  - `aarch64-cpu` v11.2.0 — [crates.io aarch64-cpu](https://crates.io/crates/aarch64-cpu)
  - `tock-registers` v0.10.1 — [crates.io tock-registers](https://crates.io/crates/tock-registers)
  - `virtio-drivers` v0.13.0 — [crates.io virtio-drivers](https://crates.io/crates/virtio-drivers)
- **rcore-os/tgoskits** (ArceOS/StarryOS ecosystem) contains no_std Rust drivers for:
  - `arm-smmu-v3`, `arm-gic-driver`, `arm-scmi-rs` ("ARM SCMI … for no_std embedded environments")
  - `acpica-interpreter` ("Freestanding ACPICA interpreter with a caller-provided OS services backend")
  - `pcie` (generic enumeration), `nvme-driver`, `ahci-driver`
  - `realtek-rtl8125`, `aic8800` Wi-Fi core
  - USB host "CrabUSB" (xHCI, async, no_std, isochronous support), `uvc`
  - `arm_pl031` RTC, Intel HDA
  
  Source: [tgoskits repo](https://github.com/rcore-os/tgoskits) (tree listing 2026-10-25); [crates.io crab-usb](https://crates.io/crates/crab-usb); [crates.io pcie](https://crates.io/crates/pcie)
- **Redox OS** (main branch, 2026): drivers `acpid`, `pcid`, `hwd` (with acpi.rs and devicetree.rs backends), `xhcid` + `usbhubd` + `usbscsid`, `nvmed`, `ahcid`, `e1000d`, `ixgbed`, `rtl8139d`, `rtl8168d`, `virtio-*` — [Redox base/drivers](https://gitlab.redox-os.org/redox-os/base/-/tree/main/drivers). Older reports call Redox ACPI support incomplete and AArch64 "booting on QEMU but not yet working on actual AArch64 hardware" — [Phoronix, Redox 0.8](https://www.phoronix.com/news/Redox-OS-0.8-Released) (older; 2026 Arm status not verified)
- **Gated CIX documents:** TRM Parts 1/2, BIOS Porting Guide (62 pages), "Linux ACPI Board-Level Bring-up Guide (105)". CNX notes the ACPI guide file was a duplicate of the Android guide at release — [CNX, 2025-12-13](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-de)
- **Community trackers:**
  - Sky1-Linux maintains 100 patches (as of 2026-10-25) on LTS 6.18.x and stable 6.19.x, including a full ACPI-boot enablement series (patches 0053–0100) — [Sky1-Linux linux-sky1](https://github.com/Sky1-Linux/linux-sky1); [patch list](https://github.com/Sky1-Linux/linux-sky1/tree/main/patches)
  - Radxa links a community "Orion O6 mainline status" page — [Radxa downloads](https://docs.radxa.com/en/orion/download)

### Inferences — difficulty matrix for a new Rust OS (ACPI boot, firmware-initialised hardware)

| Device | Standard? | Public docs | Linux status (Oct 2026) | Rust prior art | Firmware blobs | Difficulty |
|---|---|---|---|---|---|---|
| PL011 UART2 console | Yes (SBSA/ARMH0011) | Arm TRM | mainline | arm-pl011-uart | none | trivial |
| SBSA watchdog | Yes (GTDT) | Arm SBSA | mainline | small, write own | none | trivial |
| Arch timer / GICv3 + ITS | Yes | Arm specs | mainline | arm-gic, tgoskits gic | none | easy (ITS: moderate) |
| PCIe ECAM (5 RCs) | Mostly (MCFG); firmware-trained | PCIe spec; Cadence/CIX in TRM P1 (gated) | ACPI: generic ECAM works; DT: pci-sky1 v6.19 | tgoskits `pcie` | none | easy–moderate; avoid link resets |
| SMMUv3 | Yes (IORT) | Arm spec | mainline | tgoskits arm-smmu-v3 | none | optional / moderate |
| NVMe | Yes | NVMe spec | mainline | tgoskits nvme-driver, Redox nvmed | none | moderate |
| 10× xHCI (host) | Yes (PNP0D10) | xHCI spec | mainline under ACPI; CIX glue "Reviewing" | xhci crate, CrabUSB, Redox xhcid | none | moderate–hard (whole USB stack) |
| USB-C PD / role / DP-alt | No (RTS5453H proprietary I2C) | none public | vendor/community only | none | PD chip firmware on-chip | skip (host works without it) |
| RTL8126 ×2 | PCI NIC, vendor-specific regs | no datasheet; BSD/Linux/DPDK source | mainline r8169 | tgoskits rtl8125 (8125 only), Redox rtl8168d | rtl8126a-2.fw (optional) | moderate |
| RTL8852BE Wi-Fi (add-in) | PCI, vendor-specific | none | mainline rtw89 | none for this chip (aic8800 Rust core exists) | rtw8852b_fw-2.bin (required) | very hard; skip |
| RTL8852BE BT (USB) | USB HCI + Realtek fw download | Linux btrtl | mainline | none found | rtl8852bu_fw/config (required) | moderate transport; BT stack hard |
| Cadence I2C/SPI | Licensed IP (Zynq-identical) | Xilinx docs + drivers | DT mainline; ACPI IDs only in Sky1-Linux | none found | none | easy |
| Cadence GPIO + CIX pinctrl | CIX-specific | CIX TRM P2 (gated) + vendor C | DT mainline v7.1 / v6.19 | none | none | easy (pinctrl unnecessary under ACPI) |
| PWM / fan hwmon | CIX-specific | vendor C only | vendor/community | none | none | easy–moderate |
| Thermal / fan / power button | ACPI (AML) / SCMI | ACPI + SCMI specs | mainline (acpitz, acpi-fan, acpi-button) | `acpi` crate AML, acpica-interpreter, arm-scmi-rs | none | moderate (AML interpreter) |
| RTC | UEFI runtime / HYM8563 on I2C | UEFI spec / HYM8563 datasheet | mainline rtc-efi | uefi crate | none | easy |
| Mailbox + SCMI to PM | CIX mailbox + Arm SCMI | SCMI spec public; mailbox vendor C | mailbox mainline v6.17 (DT); SCMI-ACPI only in Sky1-Linux | arm-scmi-rs | PM firmware in flash | moderate |
| HDA + ALC256 audio | HDA-style + CIX glue | HDA spec; codec via Linux | mainline v6.19 (DT-only) | tgoskits intel-hda | none | moderate |
| TPM | absent (footprint) | — | — | — | — | n/a |
| EC (IT5570E) | Board-specific (cros-ec-like protocol on I2C6, hidden in ACPI) | none | vendor DT only | — | EC firmware in shared SPI ROM | leave to firmware |

### Gaps
- I found no Rust-for-Linux drivers for any of these specific devices (r8169, rtw89, NVMe in-tree). Out-of-tree Rust NVMe work exists historically, but I did not check its 2026 status.
- I did not confirm whether CrabUSB, the tgoskits PCIe crate or the tgoskits RTL8125 driver have been tested on Sky1 hardware. They target RK3588 and similar boards.
- I found no public measurements of a non-Linux OS (BSD, Redox, other) booting on the Orion O6. Only Windows 11/WinPE and Linux distros are reported under ACPI.
