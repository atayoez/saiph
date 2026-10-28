# Lessons, realistic timelines and future-proofing for a from-scratch Rust OS on the Radxa Orion O6 (CIX P1)

Research date: 2026-10-25. Every dated fact carries the date the source gives. "Local observation" means I read it on the user's own Orion O6 (hostname dev box, openSUSE aarch64, kernel 7.2.11-1-default, UEFI firmware 1.3.1 dated 2026-07-16) during this session. Those observations have no URL. They are first-hand, but they come from Linux's sanitized view of the hardware, so they should be re-checked from the new kernel at EL1.

---

## (A1) Asahi Linux: bring-up timeline, team size, m1n1 hypervisor, upstreaming, 2025 leadership and burnout lessons

### Takeaway
Asahi got from announcement (Dec 2020) to a usable alpha installer in about 15 months (Mar 2022). It had a GPU rendering under Linux in about 2 years (late 2022) and conformant OpenGL 4.6 / Vulkan 1.3 in about 3.5 years. This was a small core team building on Linux's existing generic drivers, and their force multiplier was a custom hypervisor (m1n1) that traced the vendor OS. The 2025 departures of the founder, the Rust GPU-driver author and the graphics lead show the main risks: one person carrying the project, user entitlement, upstream friction, and a downstream patch stack of more than 1,000 patches.

### Cited Findings
**Timeline (dated)**
- Dec 2020: Hector Martin announces the project. Jan 2021: work formally starts after crowdfunding, and Alyssa Rosenzweig joins for the GPU. Apr 2021: initial M1 support reaches mainline Linux. Most of 2021 goes to reverse-engineering the boot process. — [Wikipedia: Asahi Linux](https://en.wikipedia.org/wiki/Asahi_Linux)
- Mar 2022: experimental alpha installer (Arch Linux ARM, minimal, or UEFI environment). Jul 2022: M1 Ultra / Mac Studio plus early M2 support. — [Wikipedia](https://en.wikipedia.org/wiki/Asahi_Linux)
- GPU: Rosenzweig rendered triangles less than a month after starting (Jan 2021), driving the GPU through the *macOS* driver interface rather than a Linux driver. — [Gigazine, 2022-11-30](https://www.gigazine.net/gsc_news/en/20221130-asahi-linux-m1-gpu-driver)
- Jun 2022: "first triangle" with a fully open stack. Phoronix later corrected that this ran in the m1n1-based environment, not a proper Linux driver stack. — [Phoronix](https://www.phoronix.com/news/Asahi-Linux-First-Triangle)
- Sep 2022: Asahi Lina's Rust DRM kernel driver renders a spinning cube, before HDMI output even worked. Oct 2022: the driver passes more than 99% of dEQP-GLES2. — [Phoronix](https://www.phoronix.com/news/Rust-Apple-DRM-Cube-Milestone); [Gigazine 2022-10-24](https://www.gigazine.net/gsc_news/en/20221024-asahi-linux-gpu-driver-asahi-lina)
- Lina first worked on improving the m1n1 hypervisor *for GPU tracing* before writing the kernel driver. — [Gigazine](https://www.gigazine.net/gsc_news/en/20221130-asahi-linux-m1-gpu-driver)
- Dec 2022: Ars Technica describes a **four-person team** getting the Apple GPU working. Aug 2023: Fedora partnership. Oct 2023: Fedora Asahi Remix beta. ~Jan 2024: stable. Feb 2024: OpenGL 4.6 / ES 3.2 conformance. Aug 2024: Vulkan 1.3 driver. — [Wikipedia](https://en.wikipedia.org/wiki/Asahi_Linux)

**m1n1 hypervisor (tracing methodology)**
- To avoid the copyright risk of disassembling Apple's drivers, the team recorded hardware accesses made by the official driver. This is the technique Nouveau had used. Instead of patching the macOS kernel, they built a from-scratch hypervisor on top of the m1n1 bootloader and experimentation tool that runs *unmodified macOS* on the M1 and logs MMIO. Python on a separate machine controls the hypervisor. The display coprocessor interface "changes every macOS version". — [Asahi Progress Report Aug 2021](https://asahilinux.org/2021/08/progress-report-august-2021/); [The Register 2021-08-18](https://www.theregister.com/2021/08/18/asahi_linux_progress_apple_silicon)
- Mesa's Asahi driver includes a wrapper library for the macOS GPU interface that dumps submitted work with agxdecode, which is also used for reverse engineering. — [search summary of Mesa asahi docs; mirror](https://gitlab.neroreflex.duckdns.org/NeroReflex/mesa/raw/commit/f2aba8564c2efdfd60d22d052de54052ac115ae0/docs/drivers/asahi.rst)

**2025 leadership change and burnout**
- 2025-02-13: Hector Martin resigns as project lead, effective immediately. He cites burnout, demanding users (complaints about missing M3/M4 support) and Linus Torvalds's handling of Rust integration. The week before, he had already stepped down as upstream Apple Silicon maintainer. — [Phoronix](https://www.phoronix.com/news/Hector-Martin-Resigns-Asahi); [The Register](https://www.theregister.com/2025/02/13/ashai_linux_head_quits/)
- New governance: **seven leads with equal power** (Rosenzweig, chaos_princess, Davide Cavalca, Neal Gompa, James Calligeros, Janne Grunau, Sven Peter). The stated lesson is that "a large project cannot rest on a single person's shoulders". Funding moves from marcan's personal Patreon to Open Collective (fiscal host: Open Source Collective), because individual Patreon or GitHub Sponsors accounts cannot sustain a team. — [Asahi "Passing the torch", 2025-02-13](https://asahilinux.org/2025/02/passing-the-torch/)
- Upstreaming is the top priority. The downstream kernel carries **more than 1,000 Apple Silicon patches** not in mainline, and rebasing on fast-moving upstream is "laborious". The team plans CI across many devices to stop regressions, and M3/M4 is explicitly deprioritized until upstreaming and CI improve. — [Asahi "Passing the torch"](https://asahilinux.org/2025/02/passing-the-torch/)
- Mar 2025: Asahi Lina indefinitely pauses Apple GPU driver work ("I no longer feel safe working on Linux GPU drivers"). The Rust DRM driver was not yet finished or upstreamed. — [Phoronix](https://www.phoronix.com/news/Asahi-Lina-Steps-Down-Linux-GPU); [The Register 2025-03-20](https://www.theregister.com/2025/03/20/asahi_linux_asahi_lina/)
- Aug 2025: Alyssa Rosenzweig steps away after the M1/M2 graphics drivers were working. — [Phoronix](https://phoronix.com/news/Rosenzweig-Leaving-Apple-Asahi)
- Rust GPU driver upstreaming: it depends on "a significant number of Rust abstractions that will take a long time to upstream". Mesa userspace is upstream but cannot probe until the kernel UAPI is merged. The UAPI header was proposed for Linux 6.16 (2025). — [Phoronix](https://www.phoronix.com/news/Linux-6.16-Ashai-UAPI-Header); [rust-for-linux.com](https://rust-for-linux.com/apple-agx-gpu-driver)

**M3 (the "next SoC generation" lesson), 2025–2026**
- Dec 2025: Sven Peter's 39C3 talk on early M3 support. — [Wikipedia](https://en.wikipedia.org/wiki/Asahi_Linux)
- Feb 2026: on M3, keyboard, touchpad, Wi-Fi, NVMe and USB3 work with local m1n1/kernel patches, "roughly where M1 was at its first Arch Linux ARM beta". Jul 2026: Linux 7.2 has initial M3 boot support. Aug 2026: "almost ready" to cut a downstream release with M3, with USB3/Thunderbolt working. Upstreaming "will take more time". — [Phoronix "Apple M3 with Asahi continues..."](https://phoronix.com/news/Apple-M3-Asahi-Linux-2026); [Phoronix "Hopes to ship a release soon with M3"](https://www.phoronix.com/news/Asahi-Linux-M3-Release-Soon)
- Wikipedia lists "Sep 2026: support for M3 chips added" and Fedora Asahi Remix 44 (Apr 2026). **Unconfirmed against a primary Asahi post.** — [Wikipedia](https://en.wikipedia.org/wiki/Asahi_Linux)

### Inferences
- With roughly 4–7 core people and all of Linux's generic subsystems already available, Asahi needed about 15 months for an alpha and about 24 months for a first GPU render. A *from-scratch kernel* on the Orion O6 has to write those generic subsystems as well, so a solo or very small team should expect a multiple of Asahi's per-device times, not a fraction. The Orion O6 is easier in one respect: it has UEFI/ACPI and a published TRM (see B4), so there is no boot-chain reverse engineering.
- A hypervisor-tracing setup like m1n1 maps well onto the Orion O6. The board boots Linux via UEFI, so a thin EL2 hypervisor written in Rust could run the *vendor Linux kernel* (with its CIX drivers) and log MMIO for undocumented blocks such as the CIX display, the Zhouyi NPU and the Mali-G720 firmware interface. This is a legal and practical way to learn hardware without reading GPL driver source line by line. (Inference; not done by anyone for CIX that I found.)
- Next-generation SoC bring-up costs about 1–1.5 years even for a mature, tooled team (Asahi M3 ran from about late 2025 to Sep 2026). Budget for that if the OS later targets a CIX P1 successor.
- Governance lesson: keep the bus factor above 1 from day one (docs, CI, a second maintainer), set expectations about unsupported hardware early, and avoid carrying large patch stacks against fast-moving third-party trees.

### Gaps
- No primary source found for an exact "first Linux boot on M1" date (it is implied by mainline support landing in Apr 2021).
- No 2026 confirmation of whether the Asahi Rust GPU driver or its UAPI was merged upstream.
- Exact team head-count over time is not given in any source (only "four-person" for GPU in Dec 2022 and seven leads in 2025).

---

## (A2) Redox OS: what took longest, funding, aarch64/real-hardware history, 2025–2026 progress

### Takeaway
Redox is now about 11 years old (first commit 2015) and is still **not self-hosting**, even though a self-hosting toolchain partly ran as early as 2017. AArch64 was QEMU-only in 0.8 (2022) and had partial Raspberry Pi 3B+ support in 0.9 (Sep 2024). It only got real-SoC fixes and initial multi-core support in mid-2026. Funding is modest: about $37k in 2025 plus grants, with 40+ significant contributors. Drivers, real hardware and POSIX/relibc compatibility are the long poles.

### Cited Findings
- Redox marked its 10th anniversary in 2025. In 2025 it had 40+ significant contributors, $17,000 in community donations and $20,000 from Jeremy Soller (received before the nonprofit formed). Some features are grant-funded, for example the os-test suite by Jonas "Sortie" Termansen, part-funded by **NLnet / NGI Zero Commons**. — [This Month in Redox, Dec 2025](https://redox-os.org/news/this-month-251231/)
- Self-hosting: a 2017 GSoC project got binutils, GCC, Make, Dash, Curl, LLVM and rustc running with limitations. In 2019 Soller said Redox was "a few months of work away" from self-hosting, which did not happen. In Dec 2025, compiling os-test on Redox was described as progress "on our way to the self-hosting goal". — [The Register 2019-11-29](https://www.theregister.co.uk/2019/11/29/after_four_years_rusty_os_nearly_selfhosting/); [Phoronix GSoC](https://www.phoronix.com/news/Redox-OS-GSoC-Self-Hosting); [TMiR Dec 2025](https://redox-os.org/news/this-month-251231/)
- 2025 also brought "big steps toward Wayland", "baby steps toward hardware-accelerated graphics" and a first attempt at a smartphone. — [TMiR Dec 2025](https://redox-os.org/news/this-month-251231/)
- AArch64 history: 0.8.0 (Nov 2022) booted on QEMU but not on real AArch64 hardware. 0.9.0 (Sep 2024) made "significant progress" on ARM64, including partial Raspberry Pi 3B+ support. — [Phoronix 0.8](https://www.phoronix.com/news/Redox-OS-0.8-Released); [ComputerBase](https://www.computerbase.de/2022-11/redox-os-0-8-0-rust-betriebssystem-x86-64-arm64/); [Redox 0.9.0](https://redox-os.org/news/release-0.9.0)
- Apr 2026: better real-hardware booting, an updated ACPI crate and faster multi-core boot. — [Phoronix](https://www.phoronix.com/news/Redox-OS-April-2026)
- Jul 2026: ARM64 real-hardware fixes. Amlogic Meson UART (tested on S905W2), FP-register corruption, DeviceTree panics, and **preserving UEFI cacheability attributes in bootloader memory mappings**, which fixed hangs at kernel entry on S905W2 "and other possible boards". — [TMiR Jul 2026](https://redox-os.org/news/this-month-260731/)
- Aug 2026: initial multi-core (SMP) support on AArch64, an io_uring-like API, NUMA, and lower IPC overhead (per a Phoronix headline and search summary; the article itself returned HTTP 403 to me). — [Phoronix Aug 2026](https://phoronix.com/news/Redox-OS-August-2026)
- Sep 2026: ARM64 work-stealing fix, relibc ARM64 TLS/weak-symbol/dlclose fixes, graphics drivers moved to a **Linux-DRM-compatible API**, and a network block device "to speed up future network boot". Direct context switch on IPC cut IPC latency by 24%, and mapping physical memory with 2 MiB pages cut TLB misses by 75% in a synthetic benchmark. Funding outreach continued at RustConf 2026, and new EEVDF scheduler work ran under RSoC 2026. — [TMiR Sep 2026](https://www.redox-os.org/news/this-month-260930/); [Redox news index](https://www.redox-os.org/news/)

### Inferences
- What took longest at Redox: (1) a POSIX-compatible libc (relibc) and porting real software, (2) real-hardware drivers, and (3) non-x86 real hardware. Its AArch64 port needed about 4 years from QEMU-only (2022) to SMP on real SoCs (2026).
- The Jul 2026 UEFI-cacheability bug is a concrete warning for the Orion O6 path. When taking over from UEFI, honour the UEFI memory map attributes (EFI_MEMORY_WB/UC/WC/XP) in the very first page tables, or the kernel can hang at entry on real silicon even though it works on QEMU.
- Redox's move to a Linux-DRM-compatible driver API is a signal that reusing Linux userspace graphics (Mesa) beats inventing a new GPU API. This matters for Mali-G720 via Panthor/PanVK (see A4/B5).

### Gaps
- No Redox AArch64 board list for 2026 was found. There is no evidence Redox runs on CIX P1.
- The Aug 2026 Phoronix article body could not be fetched (403), so details of the "io_uring-like API" are unverified.

---

## (A3) Other comparable projects: SerenityOS, Genode/Sculpt, seL4, Fuchsia, Theseus, Haiku ARM64, Managarm, ArceOS/StarryOS, Hubris

### Takeaway
Projects that write *everything* themselves (SerenityOS, Haiku) gained broad features on x86 but took years to reach ARM64, and both are still at early stage or QEMU-only on ARM64 in 2024–2026. Projects that reuse Linux drivers in sandboxes (Genode) or cut scope hard (Hubris, seL4) reached real Arm hardware and stayed there. Corporate funding (Fuchsia) does not guarantee longevity: Google cut the about-400-person team by 16% in Jan 2023 and dropped its workstation and Chrome-port efforts.

### Cited Findings
**SerenityOS (everything from scratch)**
- Started by Andreas Kling in 2018 (initial release listed 2018-10-10). Kling went full-time in 2021, funded by community donations. License BSD-2-Clause. Third-party code is discouraged (own browser engine LibWeb). AArch64 and RISC-V support described as "early stage" as of an Apr 2024 update. — [Wikipedia: SerenityOS](https://en.wikipedia.org/wiki/SerenityOS)
- 2024-06-03: Kling forks Ladybird and steps down as SerenityOS lead. Ladybird drops SerenityOS as a target so it *can use third-party libraries*. 2024-07-01: Ladybird Browser Initiative nonprofit, funded by GitHub co-founder Chris Wanstrath. 2024-10-04: Kling leaves the SerenityOS maintainer list. — [LWN](https://lwn.net/Articles/976822/); [Wikipedia](https://en.wikipedia.org/wiki/SerenityOS); [heise](https://heise.de/-9773962)

**Genode / Sculpt OS (component OS on real hardware, Linux-driver reuse)**
- Sculpt ran on 64-bit ARM from release 20.02 (all drivers for the i.MX8). 20.11 targeted the NXP i.MX8 EVK. — [Genode Wikipedia / release notes summary](https://en.wikipedia.org/wiki/Genode); [release notes](https://genode.org/documentation/release-notes/)
- Genode reuses *unmodified Linux driver code* in sandboxed components (the "DDE" device-driver-environment approach). In 21.11 the PinePhone display and touchscreen Linux drivers were reused unchanged. The first Sculpt image for the PinePhone came in Feb 2023 "after more than two years of tireless development". 24.08 moved Linux-based PC drivers to 6.6. 24.11 updated i.MX Linux drivers to 6.6, enabling the MNT Pocket Reform. — [Genode news 2021](https://genode.org/news/2021); [genodians PinePhone](https://genodians.org/topics-pinephone); [release notes](https://genode.org/documentation/release-notes/)
- License: AGPLv3 with optional commercial license (seL4 Summit 2025 slides). Supported consumer hardware: MNT Reform (i.MX8MQ), PinePhone, x86 notebooks. — [Sculpt OS slides, seL4 Summit 2025](https://sel4.systems/Summit/2025/slides/sculpt-os.pdf)
- 2026 cadence: Sculpt OS 26.04 (2026-04-30) with all Linux-based drivers updated to **Linux 6.18**. Genode 26.05 (2026-05-29) completed the move from GitHub to Codeberg. — [genode.org news 2026](https://genode.org/news/2026); [Genode 26.05](https://genode.org/news/genode-os-framework-release-26.05); [Phoronix](https://phoronix.com/news/genode-os-26.05-released)

**seL4 on Arm**
- Oct 2024 (seL4 Summit): Proofcraft reports the AArch64 functional-correctness proof is complete, putting AArch64 on par with Arm32/RISC-V64/x86-64. Integrity is also complete. Aug 2026: the **confidentiality proof on AArch64** is complete, the last piece of the security-isolation proofs, with UK NCSC support. — [seL4 Discourse](https://sel4.discourse.group/t/sel4-security-proofs-now-complete-on-aarch64/1074); [seL4 roadmap](https://sel4.systems/roadmap.html)
- The roadmap page (pre-announcement) listed confidentiality as targeted for Q2/26, so it arrived about one quarter late. — [seL4 roadmap](https://sel4.systems/roadmap.html)

**Fuchsia (Google investment and retrenchment)**
- Jan 2023: 16% of the **about 400-strong Fuchsia team** was cut, against 6% company-wide. Chrome-on-Fuchsia and smart-speaker efforts were wound down, workstation builds discontinued and the Chrome port deprecated. First commercial device: the Nest Hub in 2021. — [9to5Google 2023-01-21](https://9to5google.com/2023/01/21/fuchsia-area-120-google-layoffs/); [OSnews](https://www.osnews.com/story/135778/googles-fuchsia-and-area-120-see-significant-cuts-in-layoffs/); [PCWorld](https://www.pcworld.com/article/1482845/the-future-of-googles-fuchsia-os-doesnt-look-rosy.html)
- Apr 2024: "microfuchsia", a Fuchsia build that boots under QEMU/pKVM, possibly to replace microdroid for secure workloads on Android. Not officially confirmed. — [Android Authority](https://androidauthority.com/microfuchsia-on-android-3457788)
- Fuchsia engineering practice worth copying: published API levels are frozen forever, components target a single API level, the package is stamped with an ABI revision the platform checks at run time, FIDL elements are tagged with levels (RFC-0083), and CTF compatibility tests run at tip of tree. — [Fuchsia API levels](https://fuchsia.dev/docs/concepts/versioning/api_levels); [RFC-0083](https://fuchsia.dev/fuchsia-src/contribute/governance/rfcs/0083_fidl_versioning); [FIDL compat testing](https://fuchsia.dev/fuchsia-src/development/testing/ctf/fidl_api_compatibility_testing)

**Theseus (Rust research OS)**
- OSDI 2020 paper (Boos et al., Rice/Yale) covers "intralingual design" (the compiler enforces OS invariants) and avoiding "state spill" so components can be live-swapped. Latest dated activity is 2023 (KISV 2023 paper on verified memory management). I found no current release or roadmap, so it is likely dormant. — [USENIX OSDI'20](https://www.usenix.org/conference/osdi20/presentation/boos); [Theseus papers page](https://www.theseus-os.com/Theseus/book/misc/papers_presentations.html); [The Register 2021-01-14](https://www.theregister.com/2021/01/14/rust_os_theseus/)

**Haiku ARM64**
- Mar 2026: Haiku ARM64 boots, but only in QEMU. Late Apr–May 2026: the QEMU port is "mostly stable" (virtio-SCSI/net, xHCI, up to 8 CPUs SMP). A kernel crash and double-free bugs remain, and nightlies are "not yet functional". The developer's goal is an M1 MacBook Air. — [Phoronix boot](https://www.phoronix.com/news/Haiku-OS-ARM64-Booting); [Phoronix SMP](https://phoronix.com/news/Haiku-OS-ARM64-SMP); [Linuxiac](https://linuxiac.com/haiku-arm64-progress-brings-usable-builds-closer/); [Haiku April 2026 report](https://discuss.haiku-os.org/t/haiku-activity-contract-report-april-2026-haiku-project/19199)

**Managarm**
- Microkernel with asynchronous I/O. Supports x86_64, aarch64 and riscv64. Rust tier-3 target `aarch64-unknown-managarm-mlibc`, with upstream LLVM support since 21.1.0. The handbook only documents QEMU x86_64 nightlies, and I found no real aarch64 board evidence. — [rustc platform support: managarm](https://doc.rust-lang.org/rustc/platform-support/managarm.html); [Managarm handbook](https://docs.managarm.org/handbook/print.html)

**ArceOS / StarryOS (Tsinghua component OS in Rust)**
- ArceOS is a modular Rust unikernel (Unikraft-inspired) running on QEMU virt for aarch64 and `PLATFORM=aarch64-raspi4`. StarryOS is a monolithic, Linux-ABI-compatible kernel built from ArceOS components, targeting RISC-V64, LoongArch64 and AArch64. The `starryos` crate was at 0.5.25 on 2026-07-10, depending on `axplat-dyn` (dynamic platform layer). An AArch64 vCPU crate (`arm_vcpu`) exists for ArceOS's hypervisor. — [docs.rs starryos](https://docs.rs/crate/starryos/0.5.0); [ArceOS fork README](https://github.com/coolyjg/arceos); [docs.rs arm_vcpu](https://docs.rs/crate/arm_vcpu/0.5.15)

**Hubris (Oxide)**
- About 2,000 lines of Rust kernel (2021). Tasks are fixed at build time, with no runtime task creation, no dynamic resource allocation, no privileged-mode drivers and no C. Crashing drivers can be restarted. It is paired with the Humility debugger, which can dump all task state. MPL-licensed. Built because no off-the-shelf RTOS met Oxide's needs for its service processor. — [Cliff Biffle, "On Hubris and Humility" (OSFC 2021)](https://cliffle.com/blog/on-hubris-and-humility/); [OSnews](https://www.osnews.com/story/134288/oxide-announces-hubris-os/)
- Oxide shipped its first rack (with Hubris on the service processors) in Jul 2023, about 1.5 years after the Hubris announcement. — [Slashdot / The New Stack](https://news.slashdot.org/story/23/07/29/192248)

### Inferences
- **Milestone timing patterns** (from the sources above):
  - First boot to stable QEMU SMP can take years for a volunteer port: Haiku ARM64 only reached it in 2026, and Redox AArch64 SMP also landed in 2026.
  - Real-board ports by a *professional* team still take about 2 years per board (Genode PinePhone, >2 years).
  - Formal verification of one architecture takes years (seL4 AArch64: functional correctness 2024, confidentiality 2026).
  - Narrow-scope kernels (Hubris) ship in about 1.5–2 years.
- **Sustainability factors**: projects survive when they (a) reuse drivers (Genode tracks Linux 6.18), (b) have a funded core (Genode Labs, Proofcraft/seL4 Foundation, Oxide), or (c) limit scope (Hubris). Lead burnout or departure (SerenityOS 2024, Asahi 2025) is the most common failure mode for hobby projects, and corporate re-prioritisation (Fuchsia 2023) is the failure mode for corporate ones.
- SerenityOS's "no third-party code" purity is the reason Ladybird left: the policy blocked library reuse. For a learning OS that aims for "drivers for all on-board hardware", strict purity would mean writing a Vulkan driver, a Wi-Fi stack and a PCIe NVMe/USB stack yourself. Choose deliberately what to reuse (Mesa, smoltcp, embedded-graphics, etc.).

### Gaps
- Fuchsia's 2025–2026 status (headcount, microfuchsia shipping) is not confirmed by any primary source I found.
- I found no ArceOS/StarryOS list of physical AArch64 boards beyond Raspberry Pi 4.
- I found no hard dates for SerenityOS's internal milestones (first GUI, x86_64 port) or for Hubris's internal milestones.

---

## (A4) Recurring practical advice: emulator-first, real-hardware CI, serial/net-boot loops, scope, docs, community, licensing

### Takeaway
The projects that succeeded on real hardware all built **fast iteration loops** (m1n1 chainloading plus a hypervisor controlled from a host, net boot, QEMU first). They invested in **CI on real devices** (Asahi's stated 2025 priority) and **tracked upstreams rather than forking** (Genode, Asahi). Licensing decides what can be reused: GPL Linux drivers can be ported into AGPL/GPL systems (Genode), while MIT/Apache/BSD/MPL kernels must re-implement or isolate them.

### Cited Findings
- Asahi's dev loop used m1n1 plus a hypervisor driven by Python on a host machine to run and trace the vendor OS. — [The Register 2021-08-18](https://www.theregister.com/2021/08/18/asahi_linux_progress_apple_silicon); [Asahi Aug 2021](https://asahilinux.org/2021/08/progress-report-august-2021/)
- Asahi named CI across many devices and patch-stack reduction as 2025 priorities, ahead of new hardware. — [Asahi Feb 2025](https://asahilinux.org/2025/02/passing-the-torch/)
- Redox added a network block device driver specifically "to speed up future network boot" (Sep 2026) and expanded VM/hardware setup and debugging docs. — [TMiR Sep 2026](https://www.redox-os.org/news/this-month-260930/)
- Redox real-hardware bugs (UEFI cacheability, FP-register corruption, DT panics) surfaced only on real ARM SoCs, not in QEMU. — [TMiR Jul 2026](https://redox-os.org/news/this-month-260731/)
- Genode runs Linux drivers unmodified in sandboxes and keeps them current (6.6 in 2024, 6.18 in 2026). — [genode release notes](https://genode.org/documentation/release-notes/); [Sculpt 26.04](https://genode.org/news/2026)
- Licensing choices: SerenityOS is BSD-2 with no third-party code ([Wikipedia](https://en.wikipedia.org/wiki/SerenityOS)). Genode is AGPLv3 plus commercial ([Sculpt slides](https://sel4.systems/Summit/2025/slides/sculpt-os.pdf)). Hubris is MPL ([OSnews](https://www.osnews.com/story/134288/oxide-announces-hubris-os/)).
- Hubris pairs its kernel with a purpose-built debugger (Humility) that inspects every task on a live system or from a dump. — [cliffle.com](https://cliffle.com/blog/on-hubris-and-humility/)

### Inferences
- **Recommended dev loop for the Orion O6** (inference from the above plus local observations):
  1. QEMU `virt` (GICv3, PL011, virtio) plus an Arm FVP for Armv9.x features the board lacks.
  2. A UEFI application loader on the O6. Firmware already exposes a DBG2 debug-port table (see B3), so early serial works without device-specific code.
  3. UEFI HTTP/PXE boot or a tiny net-boot stub, so each build reaches the board in seconds without swapping NVMe or USB media.
  4. A remotely controlled power relay plus a serial capture script, giving a real-hardware CI cell (LAVA- or labgrid-style).
  5. Optionally an EL2 tracing hypervisor (m1n1-style) to run vendor Linux and log MMIO for undocumented blocks.
- **Licensing**: MIT/Apache-2.0 (the Rust ecosystem default) maximises reuse of Rust crates and BSD code but forbids copying Linux GPL driver code. You can still *read the TRM*, trace hardware, and reuse MIT-licensed Mesa userspace (Mesa is mostly MIT). GPLv2-compatible licensing (GPL/AGPL) enables Genode-style Linux driver reuse but cuts off some permissive-only reuse. MPL-2.0 (Hubris) is a middle ground with file-level copyleft. A from-scratch Rust OS that wants Mali (Panthor/PanVK) should note that the *kernel* side is GPL in Linux while Mesa userspace is MIT. (General licensing knowledge, not sourced in this research.)
- **Scope control**: every successful small project picked one flagship goal (Asahi: one SoC family; Hubris: one controller; Genode: Sculpt on a few boards). For the O6, a realistic ordering is serial → MMU/timer/GICv3 → SMP (12 cores, heterogeneous A520/A720) → PCIe/NVMe → USB (xHCI) → Ethernet (RTL8126 5GbE) → GOP framebuffer GUI → native display → GPU → NPU.

### Gaps
- I found no source quantifying iteration-loop speedups or CI adoption across hobby OS projects. The advice above is synthesis.

---

## (B1) Arm A-profile extensions through Armv9.8 (2026) that matter for kernel design, and what the CIX P1 actually implements

### Takeaway
The CIX P1's Cortex-A720/A520 (Armv9.2) give **SVE2 (128-bit), MTE3 (asymmetric MTE), PAuth (QARMA3 with FPAC), ECV, WFxT and AFP**. They lack **SME/SME2, GCS, POE/PIE, THE, D128, FEAT_RNG** and, unexpectedly, report **no BTI**. Future cores (Arm C1/Lumex Armv9.3 with SME2, shipping since late 2025; C2 in 2026) and Armv9.4+ features (GCS, POE, THE, D128) need kernel hooks that should be designed in now as optional, feature-detected paths: page-table abstraction, per-thread state save, and control-stack memory.

### Cited Findings
**What this board has (local observation, user's Orion O6, 2026-10-25)**
- CPU parts: 4× Cortex-A520 r0p1 (MIDR part 0xd80, max 1.8 GHz) and 8× Cortex-A720 r0p1 (0xd81, max 2.6 GHz). The same feature list is reported on all 12 cores (homogeneous ISA, which matters for scheduling SVE/MTE threads). — local `lscpu` / `/proc/cpuinfo`
- HWCAPs: `fp asimd aes pmull sha1 sha2 crc32 atomics fphp asimdhp cpuid asimdrdm jscvt fcma lrcpc dcpop sha3 sm3 sm4 asimddp sha512 sve asimdfhm dit uscat ilrcpc flagm sb paca pacg dcpodp sve2 sveaes svepmull svebitperm svesha3 svesm4 flagm2 frint svei8mm svebf16 i8mm bf16 dgh mte ecv afp mte3 wfxt`. **Absent**: `bti`, `rng`, `sme*`, `gcs`, `poe`, `mops`, `lse128`. — local `/proc/cpuinfo`
- Kernel-sanitized ID registers, read via MRS emulation: ID_AA64PFR1_EL1.BT=0 (no BTI reported), MTE=3, SME=0, GCS=0. ID_AA64ISAR0_EL1.RNDR=0 (no RNDR/RNDRRS). ID_AA64ISAR2_EL1 = 0x5102, i.e. APA3=5 (**PAuth with QARMA3 plus FPAC/FPACCOMBINE**) and WFxT=2. SVE default vector length is 16 bytes (**128-bit SVE**). — local observation (program in session scratchpad). **Caveat**: BTI is architecturally mandatory from Armv8.5. Seeing BT=0 on Armv9.2 cores (with CONFIG_ARM64_BTI=y in this kernel) is surprising and should be verified by reading ID_AA64PFR1_EL1 at EL1 in your own kernel. It could be a firmware or erratum masking issue.
- The running openSUSE kernel is built with CONFIG_ARM64_GCS/POE/SME/BTI/MTE/PTR_AUTH=y, 4K pages, 48-bit VA/PA. — local `/boot/config-7.2.11-1-default`

**Architecture roadmap (Arm's annual posts)**
- **Armv8.9/9.4 (2022-09-29)**: Permission Indirection and Overlays (PIE/POE: PTE indexes a permission array in a register; separate EL1/EL0 overlay registers let user space restrict permissions without syscalls or TLBI). Translation Hardening (THE: a "Protected" attribute, AssuredOnly, the RCW read-check-write instruction). **128-bit translation tables (D128)** with larger OA and more software bits. **Guarded Control Stack (GCS)**: BL pushes the return address onto a protected stack, which is checked on return; the OS allocates and maps GCS memory and manages it across context switches and longjmp. SME2. MTE stage-2 tag-access traps and store-only checking. PMU snapshots. New RAS exception. — [Arm A-profile 2022, M. Weidmann](https://developer.arm.com/community/arm-community-blogs/b/architectures-and-processors-blog/posts/arm-a-profile-architecture-2022)
- **Armv9.5 (2023-10-05)**: FP8. **FEAT_HDBSS / FEAT_HACDBS** (hardware dirty-page logging and cleaning for VM live migration). **Checked Pointer Arithmetic** (detects corruption of bits [63:56], i.e. MTE tags). **PC+SP as PAC modifier**. RME/CCA tweaks (NS-only GPTs). Address-range breakpoints. SError delegation. EL3 config write-traps. — [Arm A-profile 2023](https://developer.arm.com/community/arm-community-blogs/b/architectures-and-processors-blog/posts/arm-a-profile-architecture-developments-2023)
- **Armv9.6 (2024-10-01)**: MPAM Domains. TRBE/SPE VM interface. Cache maintenance to outer caches beyond PoC. Granular Data Isolation (new NSP/SA physical address spaces on RME). GPT scaling. **Bitwise locking of EL1 system registers**. **Unprivileged load/store instructions for OS access to app memory**. SME structured sparsity and quarter-tile ops. Compare-and-branch. A preview of a new GIC announced for early 2025. — [Arm A-profile 2024](https://developer.arm.com/community/arm-community-blogs/b/architectures-and-processors-blog/posts/arm-a-profile-architecture-developments-2024)
- **Armv9.7 (2025-10-02)**: **Targeted TLBI broadcast "Domains"** (direct TLBI to chiplet groups). **MPAMv2** (independent PARTID/PMG, 16-bit PMG, in-memory ID translation for virtualization). Separate kernel/user pointer-auth controls. Nested-hypervisor optimisations. **GICv5**. MXFP6 AI type. Video-codec instructions. — [Arm A-profile 2025](https://developer.arm.com/community/arm-community-blogs/b/architectures-and-processors-blog/posts/arm-a-profile-architecture-developments-2025)
- **Armv9.8 (2026-10-05)**: **Conditional Fault instructions** for bounds/null checks, explicitly motivated by Rust ("Memory-safe languages, such as Rust, are increasingly used across the Arm ecosystem"). Scoped release operations (SLBND/SRLS). Single-copy-atomic granularity reporting. SVE RCpc load/store. SVE 128-bit-element permutes. Future technologies: **configurable contiguous-region sizes** for translation entries, SPE packet filtering, IEEE-754-2019 instructions. — [Arm A-profile 2026](https://developer.arm.com/community/arm-community-blogs/b/architectures-and-processors-blog/posts/arm-a-profile-architecture-developments-2026)

**Linux enablement as a reference for maturity**
- LPA2 (52-bit VA/PA with 4K/16K pages) merged for Linux 6.9 (Mar 2024). A Dec 2024 stable fix limits PA to 48 bits when LPA2 is configured but not enabled, because otherwise mappings with PA bits [51:48] get corrupted. — [arm64 pull for 6.9](https://lkml.rescloud.iu.edu/2403.1/05198.html); [PATCH v3 1/6](https://lkml.iu.edu/hypermail/linux/kernel/2412.1/06479.html)
- GCS user-space support merged in Linux 6.13 (alongside the AT_HWCAP3 and Arm CCA protected-VM work). It is per-thread and advertised via HWCAP_GCS. — [Phoronix 6.13](https://www.phoronix.com/news/Linux-6.13-ARM64-Features); [kernel docs gcs.rst](https://docs.kernel.org/arch/arm64/gcs.html)
- CVE-2026-23345 (Mar 2026): GCS PTE macros set PTE_SHARED bits that overlap output-address bits under LPA2, causing kernel panics. This is an example of feature *interactions* in PTE encoding. — [SentinelOne CVE DB (third-party)](https://www.sentinelone.com/vulnerability-database/cve-2026-23345/)
- FEAT_D128: Arm's (Anshuman Khandual) RFC V1 (Feb 2026) and V2 (May 2026), then a 17-patch "Enable 128 bit page table entries" series (2026-07-29). It adds a build-time ARM64_D128 option. A D128 kernel halts on non-D128 hardware (no runtime fallback). It needs FEAT_S1PIE alongside, and does not yet support 56-bit VA, skip-level, KVM or KASAN. Not confirmed merged. — [LKML 2607.3](https://lkml.iu.edu/2607.3/10515.html); [RFC V2 14/14](https://lists.infradead.org/pipermail/linux-arm-kernel/2026-May/1133836.html); [Phoronix](https://phoronix.com/news/Arm-Linux-FEAT-D128-Patches)

**SME2 is now in shipping consumer cores**
- Sep 2025: Arm Lumex CSS with C1-Ultra/Premium/Pro/Nano (**Armv9.3, SME2**) and Mali-G1. — [Arm newsroom](https://newsroom.arm.com/news/announcing-lumex-css-platform-ai-era); [TechRadar](https://www.techradar.com/pro/arm-unveils-new-lumex-ai-focused-smartphone-cpus-with-some-impressive-stats)
- MediaTek Dimensity 9500 (1× C1-Ultra, 3× C1-Premium, 4× C1-Pro; Armv9.3, SME2) ships in the vivo X300 and OPPO Find X9 series (late 2025). — [MediaTek](https://www.mediatek.com/products/smartphones/mediatek-dimensity-9500); [Arm OPPO case study](https://www.arm.com/company/success-library/oppo-find-x9-series)
- 2026-09-08: Arm CSS for Mobile 2 with C2-Ultra (Armv9.3-A, SME2) and Mali G2-Ultra. — [CNX Software Armv9 news](https://www.cnx-software.com/news/armv9/)

### Inferences
**Kernel design implications ("design in the hooks now")**
- **Page-table abstraction**: implement a generic `PageTable<Format>` trait over (granule 4K/16K/64K) × (descriptor VMSAv8-64 or VMSAv9-128) × (permission model: direct AP bits or PIE indirection). Start with 4K/48-bit VMSAv8-64 (what the O6 runs), but keep descriptor encode/decode behind a type so LPA2 (52-bit), PIE/POE and D128 slot in later. Linux shows that bolting D128 on later is so invasive it is a build-time switch without runtime fallback. A new kernel can do better with runtime-selected formats via generics plus feature detection.
- **Per-thread state is growing**: SVE (128-bit now, up to 2048-bit by architecture), SME (ZA tile plus streaming mode), GCS pointer, POR_EL0 (POE), MTE TCF/tag state, PAuth keys. Design the context-switch save area as an extensible, lazily-saved, feature-flagged structure with variable size.
- **Memory safety stack**: use MTE (present, MTE3 asymmetric mode) for a tagged kernel heap and user allocators now. Use PAuth (present, with FPAC so failures fault immediately) for kernel return addresses and Rust `-Z branch-protection=pac-ret`. Plan GCS (shadow stack) and BTI as later CFI layers. BTI must be re-checked on this board.
- **No FEAT_RNG on the O6**: kernel entropy has to come from elsewhere, for example the UEFI RNG protocol at boot, an SMCCC TRNG firmware call (if implemented), or a TPM. Verify which exists.
- **Heterogeneous scheduling**: A520 and A720 share an identical ISA here, but Arm's next DSU and C1 clusters add SME2 that may be per-cluster. Keep CPU-capability masks per core in the scheduler.
- **TLB invalidation**: abstract TLBI behind a "shootdown domain" API, because Armv9.7 adds targeted broadcast domains, and Armv9.8 configurable contiguous regions change how large mappings are formed.
- **Armv9.8 Conditional Fault instructions** reduce the cost of Rust bounds checks. Expect them in LLVM codegen in a few years. There is nothing to do in the kernel except handle the new fault syndrome cleanly.

### Gaps
- MTE4 sub-features (canonical tag checking, store-only refinements, tag-check-fault without TFSR) and FEAT_PAuth_LR were not described in the blog posts I fetched. The 2023 post covers only "Checked Pointer Arithmetic" and "PC+SP as PAC modifier". Exact FEAT_ names and versions need the Arm ARM (DDI0487) "Feature" list.
- POE's Linux merge version (believed 6.12) was not confirmed by a source in this session.
- I could not confirm from Arm's TRM whether the Cortex-A720/A520 implement BTI or LPA2. Locally BT=0. LPA2 is not visible from user space.

---

## (B2) GICv5, SMMUv3 evolution, CXL on Arm

### Takeaway
GICv5 (spec preview 2025, part of the Armv9.7 cycle) is a clean break from GICv3/v4. Its building blocks are IRS, ITS and IWB, with a system-register-only CPU interface, IPIs implemented as LPIs, and in-memory interrupt state tables. Linux got host support in the 2025 cycle (with KVM GICv3-guest compatibility), and ACPI support was still in review in early 2026. The CIX P1 is a **GICv3 + ITS + SMMUv3** system, so the kernel needs a GICv3 driver now and an interrupt-controller abstraction that can take a GICv5 driver later.

### Cited Findings
- GICv5 components: one or more **IRS** (Interrupt Routing Service), zero or more **ITS**, and zero or more **IWB** (Interrupt Wire Bridge). Interrupt types are PPI, SPI and LPI. The IRS discovers SPIs and parses Interrupt AFFinity IDs (IAFFIDs). LPI state lives in an **Interrupt State Table (IST)** of one or two levels (L2 allocated on demand). The ITS reuses the GICv3/v4 MSI-parent model with per-device Device Tables and ITTs. The IWB converts wires into messages with fixed event IDs. **IPIs are LPIs**. There are separate IRQ domains for PPI/SPI/LPI, and the CPU interface is via **ICC_*/ICH_* system registers** (a new GCIE field in ID_AA64PFR2_EL1). — [LWN 1018742, v2 22-patch host driver series by Lorenzo Pieralisi, 2025-04-24](https://lwn.net/Articles/1018742/)
- Jul 2025: Marc Zyngier's pull request for "GICv5 initial host support" (routing and delivery, wired interrupts, MSIs, ITS). A v7 31-patch host driver series was posted in Jul 2025. It appears to have landed in the Linux 6.17 cycle (inferred from merge-tag dates, not explicitly stated). — [LKML git pull mirror](https://lore.jethome.com/lkml/87y0slur4t.ffs@tglx/t/); [PATCH v7 00/31](https://lkml.iu.edu/hypermail/linux/kernel/2507.0/04547.html)
- 2025-06-20: KVM series (Sascha Bischoff) to run **unmodified GICv3 guests on GICv5 hosts** via FEAT_GCIE_LEGACY (nested virt excluded). — [LWN 1026316](https://lwn.net/Articles/1026316/)
- ACPI: "Code first ACPI boot support" for GICv5. Not-for-merge Oct 2025, v2 Dec 2025, v3 Jan 2026. GICv5 firmware lists one or more IRS MADT entries instead of a single GICv3 entry, and the GSI namespace is split between PPI/SPI and IWB domains. A known IWB probe-ordering issue exists. Rafael Wysocki said the dependent ACPICA patches would be applied "shortly" (Jan 2026). — [LKML v3](https://lkml.iu.edu/hypermail/linux/kernel/2601.1/09721.html); [LKML v2 reply](https://lkml.iu.edu/2601.1/11102.html)
- TF-A has GICv5 FVP build instructions, so FVP models support GICv5 for experimentation. — [TF-A docs commit](https://git.trustedfirmware.org/plugins/gitiles/next/TF-A/trusted-firmware-a/+/64c8342071fa33244b9794009049197d8ca89fa5)
- Arm's 2025 post describes GICv5 as re-architected to scale from single die to multi-chip with better virtualization efficiency. — [Arm A-profile 2025](https://developer.arm.com/community/arm-community-blogs/b/architectures-and-processors-blog/posts/arm-a-profile-architecture-developments-2025)
- On the O6, MSI-X interrupts of the PCIe dGPU (NVIDIA at 0000:c1:00.0) appear as `ITS-PCI-MSIX` (GICv3 ITS). Platform drivers `arm-smmu-v3` and an `smmu3.0x000000000b1b0000` IOMMU instance are present. ACPI tables present: APIC (MADT), CSRT, DBG2, DSDT, FACP, FPDT, GTDT, IORT, MCFG, PCCT, PPTT, SDEI, SSDT×2. **No SPCR, SRAT, HMAT or TPM2 table** was listed. — local observation (`/proc/interrupts`, `/sys/class/iommu`, `/sys/firmware/acpi/tables`)

### Inferences
- An interrupt-controller trait with `enable/disable/route(affinity)/ack/eoi/send_ipi/alloc_msi` operations, plus *hierarchical IRQ domains* (PPI/SPI/LPI/MSI), cover both GICv3+ITS (the O6 today) and GICv5 (IRS/ITS/IWB). Do not hard-code "IPI = SGI": GICv5 IPIs are LPIs.
- Prefer system-register CPU-interface access (ICC_*_EL1) even on GICv3 (the O6 GICv3 supports it). It is the only path in GICv5, and it avoids MMIO GICC.
- SMMUv3 is the IOMMU on the O6 (via IORT). Build DMA isolation for user-space drivers on SMMUv3 stage-1 from the start, which suits a microkernel with user-space drivers.
- CXL on Arm: I found no evidence that CIX P1 or its siblings support CXL. Treat CXL as out of scope for this hardware class.

### Gaps
- No register-level GICv5 vs GICv3 comparison from the spec (Arm AES0070) was retrieved. Whether GICv5 ACPI support has merged by Oct 2026 is unconfirmed.
- SMMUv3.x revision features (v3.3/v3.4 etc.) and CXL-on-Arm status were not researched in depth. No sources were gathered.
- The SMMU revision implemented on the CIX P1 is unknown (it can be read from SMMU_IDR registers).

---

## (B3) Platform standards: SystemReady (bands), BSA/SBSA, BBR (SBBR/EBBR), UEFI and ACPI versions

### Takeaway
Arm restructured SystemReady into two bands (SRS 3.0, Nov 2024). The **SystemReady band** is UEFI + ACPI using the SBBR recipe, with BSA (+SBSA for servers). The **SystemReady Devicetree band**, which replaces IR, is EBBR + DT. The Orion O6 is certified **SystemReady SR v2.5** (Apr 2025). An OS built purely on UEFI (boot services, GOP, memory map, RNG) + ACPI (MADT/GTDT/IORT/PPTT/MCFG/DBG2) + BSA-mandated IP (GICv3, generic timer, PL011/SBSA UART, SMMUv3, PCIe ECAM) will port to other SystemReady boards with little effort.

### Cited Findings
- SRS 3.0 (Nov 2024) defines two bands: **SystemReady band** and **SystemReady Devicetree band**. Current spec versions in a 2025 UEFI Forum talk: **BBR v2.2 (Jun 2025), BSA v1.2 (Jul 2025), BBSR v1.4 (Jul 2025)**. — [UEFI Forum talk "Integrate Arm SystemReady" (Arm, Wang)](https://uefi.org/sites/default/files/resources/15.%20Integrate%20Arm%20SystemReady_Arm_Wang_FINAL.pdf)
- SystemReady band systems must conform to BSA, the SBSA supplement (servers) and the SBBR recipe of BBR. Non-servers use BSA + BBR. BBSR (secure boot, firmware update, TPM) is optional but strongly recommended. — [UEFI Forum talk](https://uefi.org/sites/default/files/resources/15.%20Integrate%20Arm%20SystemReady_Arm_Wang_FINAL.pdf); [Arm SystemReady](https://arm.com/systemready)
- The SystemReady IR program was "updated and replaced" by the **SystemReady Devicetree band**: BSA-compliant hardware, the EBBR recipe of BBR (UEFI subset), firmware-provided DT for mainline Linux, Ethernet requirements, and BBSR recommended. — [Arm Devicetree Band page](https://developer.arm.com/Architectures/Arm%20SystemReady%20Devicetree%20Band); [Arm Aspen docs](https://arm-auto-solutions.docs.arm.com/en/v2.0/rd-aspen/design/systemready_devicetree.html); [Neoverse RD docs](https://neoverse-reference-design.docs.arm.com/en/main/_sources/features/systemready_acs.rst.txt)
- Major FAQ revision Dec 2024. Devicetree band guide v3.0 in Dec 2024, folded into the main FAQ by Oct 2025 (guide v4.4). Specs are updated roughly every six months. — [search summary of Arm docs](https://documentation-service.arm.com/static/68f78cde587fd957f5f00584)
- **Orion O6: Arm SystemReady SR v2.5 certification (Apr 2025)**. It was tested with UEFI firmware 9.0.0, and Windows PE, SLES 15 SP6, RHEL 9.5 and Ubuntu 24.10 booted. **Only the eight Cortex-A720 cores were enabled in testing.** — [CNX Software 2025-04-30](https://www.cnx-software.com/2025/04/30/radxa-orion-o6-mini-itx-motherboard-gets-arm-systemready-sr-v2-5-certification/)
- Radxa/CIX upstream strategy (Mar 2025): upstream **Device Tree first**, ACPI later, because ACPI needs common Arm code changes. The vendor kernel is 6.1 (DT) and 6.6 (ACPI). — [CNX Software 2025-03-03](https://www.cnx-software.com/2025/03/03/software-progress-and-source-code-release-for-cix-p1-armv9-soc-and-orion-o6-motherboard/)
- Radxa's Debian 13 docs describe an open-source CIX driver stack on a mainline dev kernel (7.0.0-rc5), while recommending the closed 6.6 branch for stability. — [Radxa docs O6N Debian 13](https://docs.radxa.com/en/orion/o6n/other-os/debian13)
- UEFI **2.11** (with PI 1.9) released 2024-12-17, including a hot-plug memory attribute and "Arm memory allocation mandates". **ACPI 6.6** released (Wikipedia: 2025-05-13), including SRAT/CPPC changes and S0-idle GPE handling. — [UEFI Forum 2.11 PR](https://uefi.org/node/5138); [BusinessWire 2024-12-17](https://www.businesswire.com/news/home/20241217759265/en); [Wikipedia ACPI](https://en.wikipedia.org/wiki/ACPI)
- The user's board runs firmware "1.3.1" dated 2026-07-16. — local observation (`/sys/class/dmi/id/bios_version`, `bios_date`)

### Inferences
- **Standards-first kernel boot path for portability**: a UEFI stub that gathers the memory map, ACPI RSDP, GOP framebuffer and EFI RNG, then ExitBootServices. Then parse MADT (GIC), GTDT (timer and watchdog), PPTT (cache/cluster topology: big.LITTLE), IORT (SMMU and ITS mapping of PCIe RIDs), MCFG (PCIe ECAM) and DBG2/SPCR (console). Keep a DT path too, since the Devicetree band, Raspberry Pi and most SBCs use DT, and CIX's upstream Linux work is DT-first.
- **Implementing an AML interpreter is unavoidable** for ACPI-described CIX devices (DSDT/SSDT). The Rust `acpi`/`aml` crates (the rust-osdev ecosystem, also used by Redox per its Apr 2026 "updated ACPI crate") are the likely starting point (inference; crate maturity not verified here).
- The O6 does not list an SPCR table even though SBBR normally expects one. Use DBG2 for the console, or verify on newer firmware. SR certification enabled only the A720 cores, so expect firmware quirks when bringing up all 12 cores (including the A520s) via PSCI/MADT.
- **PCCT present**: CIX firmware likely exposes SCMI or CPPC via PCC mailboxes for DVFS and power. Implement an SCMI client early (power, clocks, sensors are the usual SCMI domains). This is an inference to verify against the CIX TRM.

### Gaps
- I did not retrieve the current SRS version as of Oct 2026, or whether BSA v1.2 / BBR v2.2 have since been superseded.
- I did not confirm whether any CIX P1 board holds a SystemReady band (v3.x) certificate, as opposed to the older SR v2.5.
- ACPI 6.6's exact release date comes only from Wikipedia.

---

## (B4) Successor and sibling hardware: CIX roadmap, other CIX P1 boards, other Armv9 desktop/dev platforms

### Takeaway
No direct "P2" successor to the CIX P1 has been announced as of Oct 2026. CIX launched the **ClawCore** family (Mar 2026), still **Armv9.2** with Immortalis-G720, so it is essentially a P1-class respin. The P1 sibling ecosystem is broad (Orion O6/O6N, Orange Pi 6/6 Plus, Minisforum MS-R1, MetaComputing Framework-13 laptop, Lenovo AI Host devices), and that makes the P1 a good portability base. Other Armv9 targets: Qualcomm Snapdragon X2 (official Linux preview Sep 2026), NVIDIA DGX Spark/GB10 (vendor kernel, not mainline), Apple M1–M3 via Asahi, and MediaTek Genio Pro 5100 (Armv9.2, 2026).

### Cited Findings
**CIX roadmap**
- 2026-03-23: **CIX ClawCore Armv9.2 family**. ClawCore-P: 12 cores at 3.2 GHz, Immortalis-G720, 45 TOPS, up to 64 GB LPDDR5, shipping from Mar 2026. ClawCore-A: 8 cores at 3.0 GHz, 80 TOPS (200 with a PCIe card), full-chain ECC, aimed at 24/7 use. A third SKU is not described. — [CNX Software CIX category](https://www.cnx-software.com/category/processors/cix/); [CNX Armv9 news](https://www.cnx-software.com/news/armv9/)
- CIX plans to use new funding (nearly RMB 1 billion) for "next generation" agent CPUs. No name or date given. — [Jon Peddie Research](https://www.jonpeddie.com/news/cix-technology-has-nearly-rmb-1-billion-riding-on-the-agent-cpu/)
- Dec 2025: **CIX released the P1 (CD8180/CD8160) Technical Reference Manual** plus developer guides for GPU (Immortalis-G720, and NVIDIA/AMD dGPUs), the AI accelerator, OS install (Android/Linux/Windows) and firmware/BIOS. — [CNX Software 2025-12-13](https://www.cnx-software.com/category/processors/cix/)
- CNX's 2025 review called the P1 the most exciting Arm SoC of 2025 but "overhyped in 2024", with software support as the weak point. A community thread (IgorsLab) notes the key debug thread went quiet after Nov 2025. — [CNX category page](https://www.cnx-software.com/category/processors/cix/); [IgorsLab forum](https://www.igorslab.de/community/posts/331884/)

**Other CIX P1 boards and devices (confirmed)**
- **Radxa Orion O6** (mini-ITX, CD8180, announced Dec 2024, from $199). — [CNX 2024-12-18](https://cnx-software.com/2024/12/18/radxa-orion-o6-mini-itx-motherboard-is-powered-by-cix-p1-12-core-armv9-soc-with-a-30-tops-ai-accelerator)
- **Radxa Orion O6N** (Nano-ITX, **CD8160** variant, big cores 2.6 vs 2.8 GHz, 2.5GbE, UFS + NVMe, up to 64 GB). Announced 2025-10-14. — [CNX category](https://www.cnx-software.com/category/processors/cix/)
- **Orange Pi 6 Plus** (dual 5GbE, 2× M.2 M-key, five display outputs, up to 64 GB; $223.90/16 GB, $268.89/32 GB). Announced 2025-10-15. **Orange Pi 6** (90×90 mm, dual 2.5GbE, up to 32 GB). Announced 2025-06-25. — [CNX category](https://www.cnx-software.com/category/processors/cix/)
- **Minisforum MS-R1** (CP8180, up to 64 GB LPDDR5x, dual 10GbE, PCIe x16, Debian 12, $500+). Announced 2025-11-11. **MetaComputing AI PC** in a Framework Laptop 13 chassis (CP8180, probably the first P1 laptop). Announced 2025-12-04. — [CNX category](https://www.cnx-software.com/category/processors/cix/)
- CD8180 and CP8180 are "almost the same" P1 SoC (CP8180 positioned for AI mini PCs). Both have 4× A720 big + 4× A720 medium + 4× A520. — [CNX Software](https://www.cnx-software.com/category/processors/cix/)
- Lenovo launched the AI Host P7 and AI Host mini on CIX silicon in May 2026. Canonical shipped Ubuntu for CIX P1 (May 2026, per a Spanish-language report). — [search summary: Jon Peddie / foro3d](https://foro3d.com/ru/2026/mayo/canonical-lanza-ubuntu-para-el-chip-chino-cix-p1-con-arm-y-ia.html) (secondary source; treat as unconfirmed)

**Other Armv9 desktop/dev platforms**
- **Qualcomm Snapdragon X2** (Plus / Elite / Elite Extreme): at Snapdragon Summit (Sep 2026) Qualcomm announced an early Linux developer preview (custom kernel + Debian 13). Much enablement is already upstream, with Hexagon NPU and Adreno/Mesa work ongoing. Debian by end of 2026, Ubuntu in H1 2027, and HP/ASUS/HUMAIN Linux support in H1 2027. Laptops first; desktop support is "not planned" per one report. — [Phoronix](https://www.phoronix.com/news/Qualcomm-Talks-Up-X2-Linux); [HotHardware](https://hothardware.com/news/qualcomm-unleashes-linux-on-snapdragon-x2-with-npu-and-gpu-support); [Gigazine 2026-09-25](https://www.gigazine.net/gsc_news/en/20260925-qualcomm-snapdragon-x2-linux)
- **NVIDIA DGX Spark / GB10**: NVIDIA validates only DGX OS (Ubuntu + NVIDIA kernel). Grace-Blackwell SoC and ConnectX-7 patches are not in mainline. Community Arch/Rocky/RHEL kernels exist, plus experimental Nouveau/NVK. I found no public description of its UEFI/ACPI implementation. — [NVIDIA dev forum](https://forums.developer.nvidia.com/t/can-dgx-spark-work-with-non-dgx-os/379085); [linux-dgx-spark (Arch)](https://github.com/RageLtd/linux-dgx-spark); [Tom's HW Italy](https://www.tomshw.it/hardware/nouveau-e-nvk-arrivano-su-nvidia-dgx-spark-con-patch-sperimentali)
- **Apple Silicon**: M1/M2 are mature downstream in Asahi. M3 has initial mainline boot in Linux 7.2 and a downstream release was imminent in Aug–Sep 2026. — [Phoronix](https://www.phoronix.com/news/Asahi-Linux-M3-Release-Soon)
- **MediaTek Genio Pro 5100** (2026-03-12): Armv9.2, 1× X925 + 3× X4 + 4× A720, Immortalis-G925, 50+ TOPS, 3 nm, aimed at edge AI/AIoT. — [CNX Armv9 news](https://www.cnx-software.com/news/armv9/)
- **Arm CSS for Mobile 2** (2026-09-08): C2-Ultra (Armv9.3, SME2). — [CNX Armv9 news](https://www.cnx-software.com/news/armv9/)
- Proxmox VE 9.2 for arm64 (2026-08-07) supports NVIDIA Grace Hopper/Vera platforms, but not DT-only boards such as Raspberry Pi. — [CNX Armv9 news](https://www.cnx-software.com/news/armv9/)

### Inferences
- **Best portability targets after the O6**, ranked by expected effort for an ACPI/UEFI-first Rust kernel:
  1. **Other CIX P1 boards** (O6N, Orange Pi 6/6 Plus, MS-R1). Same SoC, different board-level Ethernet, PCIe topology and displays, so mostly ACPI-table differences. ClawCore-P is likely near-identical (Armv9.2, same GPU).
  2. **QEMU virt / Arm FVP** for Armv9.4+ features (GCS, POE, D128, GICv5) before silicon exists.
  3. **Ampere Altra/AmpereOne workstations** (SystemReady SR servers with GICv3/SMMUv3/ACPI; not researched here).
  4. **Snapdragon X2 laptops**, which are DT/ACPI-hybrid with Qualcomm-specific firmware.
  5. **Apple Silicon**, building on Asahi's m1n1 and documentation, but non-standard (AIC interrupt controller, no UEFI/ACPI natively).
  6. **DGX Spark**, vendor-locked.
- The Orion O6 is a *conservative* Armv9.2 baseline with no SME, GCS or POE. Pairing it with an FVP or QEMU for newer features is the realistic way to "future-proof" the code without waiting for hardware.

### Gaps
- I found no source naming a CIX "P2" or a direct P1 successor with Armv9.3+ cores. The relation of ClawCore to P1 (respin vs new die) is not stated.
- Raspberry Pi 6: I found no announcement. The Raspberry Pi 5 uses Cortex-A76 (Armv8.2), not Armv9 (background knowledge, not sourced here).
- AmpereOne workstation availability and its architecture level (Armv8.6+ custom cores) were not researched in this session.
- I did not confirm the DGX Spark firmware model (UEFI+ACPI is likely, as with Grace servers, but this is unsourced).

---

## (B5) Software-architecture practices for longevity: versioned IDLs and capabilities, WASI component model, HAL via ACPI/DT, versioned driver interfaces, Rust toolchain/editions, reproducible builds

### Takeaway
The strongest documented precedent is Fuchsia's model: frozen, numbered API levels, FIDL elements tagged with availability, an ABI revision stamped into each package and checked at load, and CTF compatibility tests. For an application ABI, **WASI 0.3 (ratified 2026-06-11)** makes async native in the Component Model and coexists with 0.2. That makes it a credible portable app ABI layered over a capability-based native IPC. Rust's edition system (2024 edition stable since 1.85, 2025-02-20) gives opt-in, mostly automated migration. Pin toolchains and editions per crate.

### Cited Findings
- **Fuchsia API levels**: "the set of APIs and their semantics at a specific API level will never change once that API level has been published". A component targets exactly one API level. The SDK stamps the ABI revision into the package, and the platform runs it only if it supports that level. Levels go in-development → stable → unsupported. FIDL versioning (RFC-0083) defines API levels as 63-bit integers and covers API, not ABI. FIDL is "much farther along" than other parts of the system interface. RFC-0232 allows bindings for multiple API levels. — [API levels](https://fuchsia.dev/docs/concepts/versioning/api_levels); [RFC-0083](https://fuchsia.dev/fuchsia-src/contribute/governance/rfcs/0083_fidl_versioning); [RFC-0232](https://fuchsia.dev/docs/contribute/governance/rfcs/0232_fidl_bindings_for_multiple_api_levels); [CTF FIDL compat testing](https://fuchsia.dev/fuchsia-src/development/testing/ctf/fidl_api_compatibility_testing)
- **WASI 0.3.0** released and ratified 2026-06-11. Async moved into the Canonical ABI (async functions in WIT, `stream<T>`/`future<T>`). It replaces 0.2's non-composing start/finish/subscribe pattern, and runtimes can host 0.2 and 0.3 side by side. Wasmtime is the reference implementation. Sources conflict on which Wasmtime version enables it by default (43+ vs 45/46). — [wasi.dev P3](https://wasi.dev/releases/wasi-p3); [Bytecode Alliance](https://bytecodealliance.org/articles/WASI-0.3); [Component Model async](https://component-model.bytecodealliance.org/design/async.html)
- **Rust 2024 edition** entered beta 2025-01-09 and shipped stable in Rust 1.85 on 2025-02-20 (the fourth edition after 2015/2018/2021). It is opt-in, with migration via `cargo fix --edition`. 1.85.1 (2025-03-18) fixed doctest merging. — [Rust blog: 2024 beta](https://blog.rust-lang.org/2025/01/22/rust-2024-beta/); [Phoronix 1.85](https://www.phoronix.com/news/Rust-1.85-Released); [releases.rs](https://releases.rs/docs/1.85.1/)
- **Hubris' static-configuration model** (tasks fixed at build time, drivers unprivileged and restartable) removes whole classes of attack surface "by construction". — [cliffle.com](https://cliffle.com/blog/on-hubris-and-humility/)
- **Theseus' "state spill" principle** keeps components swappable and live-updatable by not letting one component hold state on behalf of another. — [Theseus PLOS 2017](https://www.theseus-os.com/kevinaboos/docs/theseus_plos2017.pdf); [OSDI'20](https://www.usenix.org/conference/osdi20/presentation/boos)
- **ArceOS's component model** splits the platform layer into crates (`axplat-dyn`), so StarryOS and the ArceOS hypervisor share drivers across architectures. — [docs.rs starryos](https://docs.rs/crate/starryos/0.5.0)
- **Redox chose a Linux-DRM-compatible graphics driver API** (Sep 2026) to ease porting of graphics userspace. — [TMiR Sep 2026](https://www.redox-os.org/news/this-month-260930/)
- **Genode tracks Linux driver versions** release by release (6.6 → 6.18), isolating each driver in its own component. — [Sculpt 26.04](https://genode.org/news/2026)

### Inferences
- **Recommended longevity stack** (synthesis):
  1. Capability-based microkernel IPC with an IDL that has *availability annotations* (Fuchsia-style `@available(added=N, removed=M)`), plus a CI job that diffs the IDL surface per release. FIDL itself, Cap'n Proto, or a custom IDL using Rust proc-macros all work. The key is frozen numbered levels and an ABI stamp checked at process load.
  2. Native driver ABI: versioned driver-framework protocols (DFv2-style) over the same IPC. Drivers run unprivileged and restartable (Hubris/Genode), with SMMUv3 DMA isolation.
  3. Application ABI: WASI 0.3 components as the portable app format, plus a native Rust SDK. Optionally add a Linux-ABI personality later, as StarryOS and Fuchsia's Starnix did, to run existing binaries.
  4. Hardware abstraction: one firmware-description layer that produces a common device tree model from either ACPI (DSDT/SSDT via an AML interpreter) or FDT. Drivers bind on `_HID`/`_CID` or `compatible`.
  5. Toolchain: pin `rust-toolchain.toml` (nightly date if using `build-std`/`asm` features) and the edition per crate. Plan an edition migration (2024 → next) as a scheduled task. Use custom target JSON only where `aarch64-unknown-none` (tier 2) is insufficient.
  6. Reproducible builds: hermetic, pinned crate lockfiles and vendored dependencies, plus deterministic image assembly, so CI on real hardware can bisect regressions. This mirrors Asahi's stated need for CI and patch-stack discipline.
- **Use QEMU/FVP CI matrices across feature sets** (with and without SVE/SME/MTE/GCS/POE, GICv3 vs GICv5, 4K vs 16K granule, VMSAv8 vs D128) so that feature-detected code paths stay tested even before hardware has them.

### Gaps
- I did not research the stability guarantees of Cap'n Proto or other IDLs, Zircon-style DFv2 specifics, or the WASI component-model maturity for *kernel-hosted* (non-Wasmtime) runtimes. Whether a no_std Wasm runtime (e.g. wasmi or wasmtime's no_std mode) supports WASI 0.3 components as of Oct 2026 is unconfirmed.
- I found nothing on Rust's next-edition (2027) plans.
- I found no source on reproducible-build practice specifically in hobby OS projects.
