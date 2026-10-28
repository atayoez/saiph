# State of the Art in OS Kernel Architecture and Mechanisms (2024–2026), for a from-scratch Rust research OS on the Radxa Orion O6

Research date: 2026-10-25. Each fact carries the date it was observed or published. Material from before 2024 is labelled **[background]**. Unless marked "established", the results listed are the authors' own claims or prototypes.

Target hardware (as the assignment gives it): CIX P1 SoC with 4x Cortex-A520 and 8x Cortex-A720, 16 GB LPDDR5, SMMUv3, a GICv3-family interrupt controller, MTE3/PAC/SVE2, UEFI+ACPI. The hardware facts confirmed from sources are listed under Key Question 7.

---

## 1. Kernel architectures and where each stands in 2025–2026

### Takeaway
Rust kernels have moved past toy status. Five show it: a peer-reviewed framekernel with Linux ABI compatibility (Asterinas, ATC'25, AArch64 added Oct 2026), a Verus-verified L4-style microkernel (Atmosphere, SOSP'25), a production embedded OS on about 10 million machines (Tock, SOSP'25), a Rust microkernel shipping io_uring-like rings and ARM64 SMP (Redox, Aug 2026), and Rust userland on the most mature verified kernel (seL4 plus rust-sel4 5.0.0, Jul 2026). The two industrial capability systems are not Rust kernels: seL4 is C plus Isabelle proofs and Zircon is C++. Fuchsia has stalled commercially since the 2023 layoffs. Managarm, an async microkernel, already boots on the Orion O6 via ACPI in its hardware CI.

### Cited Findings

**seL4 / Microkit / LionsOS / sDDF**
- Releases: seL4 14.0.0, Microkit 2.1.0 and rust-sel4 3.0.0 on 26 Nov 2025; seL4 15.0.0, Microkit 2.2.0 and rust-sel4 4.0.0 on 31 Mar 2026; seL4 16.0.0 (breaking API changes), Microkit 2.3.0 and rust-sel4 5.0.0 (breaking) on 22 Jul 2026 — [seL4 News 2025](https://sel4.systems/news/2025.html); [seL4 News 2026](https://sel4.systems/news/2026.html)
- Proofcraft finished the proof that seL4 enforces confidentiality on AArch64 on 24 Aug 2026, which "finishes the formal proof of security isolation for AArch64 code" under the stated assumptions. The MCS functional-correctness proof for RISC-V was finished on 29 Jun 2026, and a port to 64-bit Arm is planned under DARPA PROVERS — [seL4 News 2026](https://sel4.systems/news/2026.html)
- Verified AArch64 configurations cover C-level functional correctness (including the fast path), integrity/availability and confidentiality. Hypervisor mode is included, with the kernel at EL2. Platforms include RPi4/5, ROCKPro64, i.MX8M and Zynq ZCU102/106. None is a CIX/Orion platform. The AArch64 MCS proofs are still "in progress". Device address translation (System MMU/IOMMU), debug interfaces and kernel boot are **not** covered by any verified configuration — [seL4 verified configurations](https://docs.sel4.systems/projects/sel4/verified-configurations.html)
- rust-sel4 is userspace-only. It provides `sel4`/`sel4-sys` bindings, a root-task runtime, a `sel4-microkit` protection-domain runtime, a CapDL initializer and experimental `sel4-async-*` crates. The crates are not on crates.io, and the project is tested against seL4 16.0.0 and Microkit 2.3.0 (observed Oct 2026) — [rust-sel4 GitHub](https://github.com/seL4/rust-sel4)
- In its Sept 2025 status, Trustworthy Systems listed multi-kernel support in progress, Rust drivers, and decoupling sDDF from Microkit (search-result summary; the slide PDF could not be text-extracted) — [seL4 Summit 2025 TS slides](https://sel4.systems/Summit/2025/slides/trustworthy-systems.pdf)
- LionsOS paper (arXiv v1 8 Jan 2025, v2 27 May 2025). The architecture is static and use-case-specific. Principles: "radical simplicity", separation of concerns, least privilege, design for verification. I/O uses lock-free bounded single-producer/single-consumer shared-memory queues and seL4 notifications, which are sent only on empty→non-empty or full→non-full transitions. "Virtualisers" multiplex devices and translate DMA addresses. LionsOS runs on AArch64, x86_64 and RV64 — [LionsOS paper (HTML v2)](https://arxiv.org/html/2501.06234v2)
- LionsOS performance (authors' claim). On Arm unicore with a 1 Gb/s NIC, Linux plateaus at about 500 Mb/s. LionsOS reaches the full 1 Gb/s with about a third of the CPU spare, and its RTT is 3–10x lower. On x86 with a 10 Gb/s NIC, Linux saturates the CPU at 4 Gb/s while LionsOS keeps about 20% spare. The file system is slower than Linux beyond five files because there is no directory cache. Dynamic core management and component migration are still in progress — [LionsOS paper (HTML v2)](https://arxiv.org/html/2501.06234v2)
- Genode Labs released an experimental Sculpt OS 25.10 on seL4 (8 Nov 2025) — [seL4 News 2025](https://sel4.systems/news/2025.html)

**Asterinas (framekernel)**
- Definition (ATC'25). A single address space, as in a monolith, split into a privileged framework (OSTD) that alone may use `unsafe` and de-privileged services written entirely in safe Rust. OSTD exposes safe abstractions: UserMode/VmSpace, SpinLock/RCU/Mutex, IrqLine/IoMem/DmaCoherent/DmaStream — [Asterinas paper arXiv 2506.03876](https://arxiv.org/html/2506.03876v1); [USENIX ATC'25 page](https://www.usenix.org/conference/atc25/presentation/peng-yuke)
- TCB by the "linked code size" metric: Asterinas 14.0% (10,571 of 75,285 LoC), versus Tock 43.8%, Theseus 62.4% and RedLeaf 66.1%. About 210 Linux syscalls are implemented. Sensitive memory is held in typed frames, while user-mapped and DMA memory is "untyped", allowing only POD copy. No memory is DMA-able by default. IOMMU interrupt remapping is used to stop interrupt spoofing — [Asterinas paper](https://arxiv.org/html/2506.03876v1)
- Performance claim: roughly on par with Linux 5.15. LMbench geometric mean is 1.08, Nginx 1.17, Redis 1.31, and SQLite lags. All runs were in QEMU on a **single core**, with SMP scalability listed as future work. Soundness was checked with "KernMiri" (Miri over OSTD unit tests, 100% unsafe-block coverage), which found 2 UB bugs. The paper has no formal verification — [Asterinas paper](https://arxiv.org/html/2506.03876v1)
- Asterinas 0.19.0 (13 Oct 2026):
  - Codebase over 250k lines, about 200k of them Rust. 55 contributors to this release.
  - AArch64 added as the 4th architecture (after x86-64, RISC-V and LoongArch) at Tier 2. It is "minimal but usable", on QEMU `virt` only: GICv3, generic timer, PL011, PL031, virtio-mmio, BusyBox shell with vDSO. SMP and real hardware are not mentioned.
  - OSTD gained a hypervisor module of about 2,000 unsafe lines, Intel VMX only.
  - Asterinas NixOS runs Xorg and self-hosts its own build.
  - [Asterinas 0.19.0 announcement](https://asterinas.github.io/2026/10/13/announcing-asterinas-0.19.0.html)
- `vostd` is a Verus-verified version of OSTD, focused on the memory-management and synchronization subsystems. It is "transitional": sync and id-alloc track v0.19.0, other parts track v0.16.0. Verification found UB, deadlocks, overflows and reachable panics in upstream. LLM-generated proofs (KVerus) are accepted with an "AI-assist" label — [asterinas/vostd](https://github.com/asterinas/vostd)

**Atmosphere (Verus-verified microkernel)**
- SOSP'25 paper, 12 Oct 2025, DOI 10.1145/3731569.3764821 — [ACM DOI](https://doi.org/10.1145/3731569.3764821); [SOSP'25 accepted](https://sigops.org/s/conferences/sosp/2025/accepted.html)
- An L4-like Rust microkernel with drivers, network stack and file systems in user space. The kernel provides only address spaces, page tables, coarse memory management and threads. Pages are passed over endpoints. Functional correctness is shown by refinement in Verus, and a big kernel lock simplifies verification. Proof-to-code ratio is 7.5:1. The project page does not state the hardware architecture — [Atmosphere project](https://mars-research.github.io/projects/atmo/)

**Redox OS**
- Aug 2026 report:
  - "Redox Rings" is a userspace-based ring-buffer API "comparable to Linux io_uring" that replaces an earlier kernel-based attempt. It reports about 14–15x NVMe I/O in a benchmark that bypasses RedoxFS.
  - Initial NUMA support, a 5% IPC overhead cut, and **ARM64 multi-core** support ("more testing needed").
  - An EEVDF scheduler with priorities and work stealing.
  - Scheme path handling and fd allocation moved to user space.
  - A namespace manager mediates `openat`. The `contain` sandbox uses per-process filter schemes.
  - [This Month in Redox Aug 2026](https://www.redox-os.org/news/this-month-260831/)
- Earlier in 2026: ARM64 CPU feature detection (Mar), ARM64 dynamic linking, Amlogic UART and real-hardware ARM64 fixes (Jul) — [Redox news search summary / Jul 2026](https://www.redox-os.org/news/this-month-260731/); [Mar 2026](https://www.redox-os.org/news/this-month-260331/)

**Managarm (async microkernel)**
- Status as of 13 Jan 2026:
  - A "fully asynchronous microkernel-based OS" with Linux source-level compatibility; systemd is the default init.
  - SMP with a new load balancer based on decaying-average CPU time.
  - Boots via UEFI/Limine/GRUB. RISC-V boots Weston.
  - **The hardware CI includes a Raspberry Pi 4 and an Orion O6 (ACPI).**
  - nvidia-open modesetting works on real hardware.
  - [Managarm end-of-2025 update](https://managarm.org/2026/01/13/end-of-year-update.html)

**Fuchsia / Zircon**
- Zircon is an object-capability kernel derived from LK, mostly C++. The January 2023 layoffs hit 16% of Fuchsia staff. Fuchsia shipped to the Nest Hub (2021/2023). The latest release is F31, 22 Jul 2026 — [Wikipedia: Fuchsia](https://en.wikipedia.org/wiki/Fuchsia_(operating_system))
- In DFv2, drivers run as components and talk only via FIDL, replacing DFv1's Banjo. The docs still say the driver framework "has not achieved ABI stability". No source declares the migration complete — [Fuchsia DFv1→DFv2 migration](https://fuchsia.dev/fuchsia-src/development/drivers/migration)
- Starnix is a Rust userspace component implementing the Linux syscall interface (RFC-0082, accepted Mar 2021) **[background]** — [RFC-0082](https://fuchsia.dev/docs/contribute/governance/rfcs/0082_starnix)
- "Microfuchsia" (2024) is an experimental way to run Fuchsia as a pKVM/AVF guest for trusted apps on Android. No 2025–26 shipping status was found — [AOSP microfuchsia](https://android.googlesource.com/platform/packages/modules/Virtualization/+/f0271f36388ec9630d89ff8b3ee4cb22e2ca3eaf/microfuchsia/); [OSNews 2024](https://www.osnews.com/story/140162/google-is-bringing-fuchsia-os-to-android-devices-but-not-in-the-way-youd-think/)

**Theseus, Twizzler, Hubris, Tock, unikernels, multikernels**
- Theseus is "under active development… not yet mature". aarch64 is "an ongoing effort, but most of the core subsystems are complete", and a full build of all crates is not supported on it. Intralingual design moves resource management into the compiler via affine types (observed Oct 2026) — [Theseus GitHub](https://github.com/theseus-os/Theseus). The original OSDI'20 paper is **[background]**: [OSDI'20](https://www.usenix.org/conference/osdi20/presentation/boos)
- Twizzler has been rebuilt as a "pure Rust implementation of the Twizzler kernel". It is a research OS for memory-hierarchy programming models (invariant references, NVM) and "not quite production ready" (observed Oct 2026) — [Twizzler GitHub](https://github.com/twizzler-operating-system/twizzler)
- Hubris (Oxide) is for microcontrollers. It has no runtime task creation or destruction, no dynamic resource allocation, and no privileged drivers. Crashing drivers are isolated and restarted. The project claims about 2,000 lines of Rust — [Hubris](https://hubris.oxide.computer/)
- Tock:
  - The SOSP'25 paper (arXiv 23 Mar 2026) reports deployment in data-center root-of-trust chips and "millions of laptops", about 10M computers — [Tock arXiv 2603.22585](https://arxiv.org/abs/2603.22585)
  - Tock 2.2 (2025) needs no nightly Rust features and has CI on physical boards — [Tock 2.2](https://tockos.org/blog/2025/tock2.2)
- Unikraft Release 13 "Adrastea" (2 Sep 2026) runs on ARM64 hosts. Versioning conflicts with the "0.18.0" shown on the main site — [Unikraft R13](https://unikraft.com/docs/releases/r13-adrastea); [unikraft.org](https://unikraft.org)
- Hermit's `aarch64-unknown-hermit` target is Tier 3 in rustc — [rustc platform support: hermit](https://doc.rust-lang.org/rustc/platform-support/hermit.html)
- Multikernel **[background]**. Barrelfish (SOSP'09) treats the machine as a network of cores, with no shared state at the lowest level, message passing and replicated state. It claims a natural fit for heterogeneous cores — [Multikernel SOSP'09](https://home.cs.colorado.edu/~rhan/CSCI_5573_Fall_2011/Papers/sosp09-barrelfish.pdf); [Baumann retrospective 2021](https://www.sigops.org/2021/fish-in-a-barrel-an-insiders-retrospective-of-the-sosp09-multikernel-paper/)
- seL4 "clustered multikernel" **[background]**. Nodes span multiple CPUs under a big lock, with a proof-lifting approach (von Tessin 2013). Heiser (2019) argued that separate kernel images make sense once cache-line migration cost dominates syscall cost — [TS multiprocessor verification (archived)](https://trustworthy.systems/projects/OLD/cmp); [seL4 devel list 2019](https://lists.sel4.systems/hyperkitty/list/devel@sel4.systems/message/H4CH7GNHS5TWUEU6BT2OWPL735ZMRKYN/)
- Genode's Sculpt OS 25.10 is a combined multi-kernel image (NOVA, hw, NOVAe, seL4, Fiasco.OC), plus a prototype Windows 11 VM on Genode@NOVA (2025). Genode 26.05 was released on 29 May 2026 — [Genode Wikipedia / year-end report summary](https://en.wikipedia.org/wiki/Genode)
- Instructional OS "Proto" (SOSP'25, UVA) runs on commodity portable hardware, with multicore, USB, DMA and a window manager. It is built as incremental prototypes driven by applications — [Proto arXiv 2504.17984](https://arxiv.org/abs/2504.17984)

### Inferences
- The live design space for a new Rust kernel has narrowed to three families:
  - Capability microkernels with user-space drivers (seL4, Atmosphere, Redox, Managarm, Hubris, Fuchsia).
  - Language-isolated single-address-space kernels with an unsafe core (Asterinas/OSTD; Theseus is a more extreme take).
  - Hybrids that use virtualization for driver reuse (LionsOS libvmm, Genode).
- RedLeaf, Theseus and Twizzler are low-activity research artifacts, best used as idea sources rather than foundations.
- No Rust kernel in this survey has evidence of running on Orion-class Armv9 hardware with SMP, except Managarm (C++, not Rust), which is in the Orion O6 hardware CI. Redox's ARM64 SMP dates from Aug 2026 and is untested at scale. The user's kernel would be among the first Rust kernels on this SoC, and Managarm's O6 ACPI bring-up code is a valuable reference.
- Every recent Arm-capable system that does well (seL4/LionsOS, Asterinas, Redox) uses a few small, well-specified privileged primitives plus shared-memory queues. The architecture choice is mostly about *where the isolation boundary sits*: the hardware MMU between processes, or the Rust type system inside one address space.

### Gaps
- No primary 2025–2026 source on RedLeaf activity. It appears only in Asterinas's TCB comparison.
- Text of the seL4 Summit 2025 Trustworthy Systems slides could not be extracted, so the multikernel/SMP and sDDF roadmap details are search-summary level.
- No 2025–26 primary source on Zircon kernel changes, Starnix maturity, or Fuchsia strategy beyond release numbers.
- The Atmosphere paper's target ISA (likely x86-64) and performance numbers were not confirmed from the project page.
- Hubris's "2,000 lines" figure is the project's own claim and may describe the kernel only.

---

## 2. Modern mechanisms: async rings, zero-copy IPC, IOMMU-isolated drivers, capabilities, schedulers, EAS, tickless/RT, MTE, PAC/BTI/GCS, CHERI, safe extensibility, live update, record/replay, observability

### Takeaway
The durable mechanisms are:
- Completion-based shared-memory rings (io_uring, Redox Rings, sDDF queues).
- User-space drivers behind an IOMMU.
- Capability-checked handles.
- Heterogeneity-aware scheduling driven by an energy model.
- Memory tagging plus control-flow integrity as low-cost hardening.

io_uring's 2023–2025 security and observability record shows that an async interface must be designed with per-operation authorization and tracing from day one. eBPF is being challenged by safe-Rust extensions (Rex) and by proof-guided verifiers.

### Cited Findings

**Async rings and zero-copy**
- io_uring network zero-copy receive landed in Linux 6.15 (2025). It needs NIC header/data split and initially host memory only — [Phoronix](https://www.phoronix.com/news/Linux-6.15-IO_uring); [kernel docs iou-zcrx](https://docs.kernel.org/6.15/networking/iou-zcrx.html)
- Google (Jun 2023) **[background]** reported that 60% of its kCTF submissions exploited io_uring. It disabled io_uring on production servers and ChromeOS and blocks it from Android apps via seccomp — [Phoronix](https://mail.phoronix.com/news/Google-Restricting-IO_uring)
- In Apr 2025, ARMO's PoC rootkit showed io_uring operations evading syscall-hook-based runtime security tools (Falco, Tetragon) — [Security Boulevard / ARMO](https://securityboulevard.com/2025/04/armo-io_uring-interface-creates-security-blind-spot-in-linux/)
- Redox Rings (Aug 2026) is userspace-based, io_uring-like, and reports 14–15x NVMe throughput in a RedoxFS-bypass benchmark — [Redox Aug 2026](https://www.redox-os.org/news/this-month-260831/)
- LionsOS/sDDF uses SPSC shared-memory queues with transition-only notifications. Location-transparent queues let components spread across cores without multithreaded code, but cross-core communication roughly doubled CPU cost on x86 — [LionsOS paper](https://arxiv.org/html/2501.06234v2)
- SOSP'25 includes "How to Copy Memory? Coordinated Asynchronous Copy as a First-Class OS Service" and "Aeolia: A Fast and Secure Userspace Interrupt-Based Storage Stack" — [SOSP'25 accepted](https://sigops.org/s/conferences/sosp/2025/accepted.html)

**User-space drivers and IOMMU**
- Asterinas uses an IOMMU so that only "untyped" memory can be DMA-mapped, and interrupt remapping to prevent spoofing. DMA pooling reduces IOTLB invalidations, but SQLite VACUUM drops to 72% of Linux with the IOMMU on — [Asterinas paper](https://arxiv.org/html/2506.03876v1)
- seL4's proofs exclude SMMU/IOMMU configuration — [seL4 verified configurations](https://docs.sel4.systems/projects/sel4/verified-configurations.html)
- LionsOS NIC drivers are 8.4x (i.MX8), 26.9x (S905X3) and 4.5x (x86) smaller than Linux's (search summary of paper table) — [LionsOS paper PDF](https://arxiv.org/pdf/2501.06234)
- Redox added PCI multi-vector MSI-X for user-space drivers (Aug 2026) — [Redox Aug 2026](https://www.redox-os.org/news/this-month-260831/)

**Scheduling (pluggable, EEVDF, heterogeneous/EAS)**
- EEVDF replaced CFS in Linux 6.6 (2023) **[background]**. It is a proportional-share model using lag and virtual deadlines, with fewer heuristics and tunables — [Phoronix](https://www.phoronix.com/news/Linux-6.6-EEVDF-Merged); [Linux Magazine](https://www.linux-magazine.com/Issues/2025/301/EEVDF)
- sched_ext (BPF-defined scheduling classes) has been in mainline since Linux 6.12 (Nov 2024). LAVD and BPFLand build heterogeneity-aware CPU domains — [arXiv 2502.10000](https://arxiv.org/pdf/2502.10000)
- sched_ext plans (LPC 2025, reported 23 Dec 2025): hierarchical and composable schedulers, re-implementing some C in Rust, GPU awareness, **energy-aware abstractions**. scx_lavd is used by Meta on servers — [Phoronix](https://www.phoronix.com/news/sched-ext-future-plans-2026)
- Linux EAS (current docs) needs:
  - An Energy Model of per-performance-domain power-cost tables.
  - Asymmetric CPU capacity (`SD_ASYM_CPUCAPACITY_FULL`).
  - The schedutil governor.
  - Frequency- and CPU-invariant PELT.

  `find_energy_efficient_cpu()` simulates placing the waking task in each performance domain and picks the lowest-energy option. When any CPU exceeds 80% of capacity, the root domain becomes "over-utilized" and EAS falls back to load balancing — [kernel docs: sched-energy](https://docs.kernel.org/scheduler/sched-energy.html)
- Redox has EEVDF with priorities and improved work stealing (Aug 2026). Managarm has a decaying-average load balancer (Jan 2026) — [Redox Aug 2026](https://www.redox-os.org/news/this-month-260831/); [Managarm](https://managarm.org/2026/01/13/end-of-year-update.html)

**Memory tagging, PAC/BTI/GCS, CHERI**
- Hardware Tag-Based KASAN (arm64 MTE + TBI) is aimed at in-field use and "can be used in production", with low overhead. It has sync, async and asymm modes and 16-byte granules. It covers slab, page_alloc and non-executable vmalloc but **not stack or globals**, and only reports the first bug before tag checking is disabled — [kernel docs: KASAN](https://docs.kernel.org/dev-tools/kasan.html)
- TikTag (Jun 2024): speculative gadgets leak MTE tags with over 95% success in under 4 s on Pixel 8, demonstrated against Chrome and the Linux kernel — [arXiv 2406.08719](https://arxiv.org/html/2406.08719V1)
- FEAT_MTE3 adds asymmetric tag-check mode (sync loads, async stores) — [QEMU emulation doc mirror](https://git.cs.tu-dortmund.de/SYS-OSS/FRET-qemu/src/branch/wcet_system/docs/system/arm/emulation.rst)
- A GrapheneOS forum post claims Cortex-A720 MTE2/MTE3 is enabled by an SoC integration pin (BROADCASTMTE), so MTE availability depends on the SoC vendor (unverified secondary claim) — [GrapheneOS forum](https://discuss.grapheneos.org/d/41564-pixel-11-doesnt-meet-the-grapheneos-security-standards-and-may-be-skipped/31)
- Arm Guarded Control Stack (hardware shadow stack) user support is in Linux 6.13 (merged Nov 2024–Jan 2025), via `prctl(PR_SET_SHADOW_STACK_STATUS)`, with GCC 15 codegen support. Linux does not support GCS without return-address checking — [kernel docs GCS](https://docs.kernel.org/6.13/arch/arm64/gcs.html); [Phoronix 6.13 arm64](https://www.phoronix.com/news/Linux-6.13-ARM64-Features)
- CHERIoT RTOS (SOSP'25, Arm among the authors) provides fine-grained memory-safe compartments on low-cost embedded devices — [SOSP'25 accepted](https://sigops.org/s/conferences/sosp/2025/accepted.html)
- Morello is an experimental CHERI board from the UKRI DSbD programme **[background]** — [Arm newsroom](https://newsroom.arm.com/news/morello-research-program-hits-major-milestone-with-hardware-now-available-for-testing)

**Safe extensibility**
- Rex (ATC'25): kernel extensions written in safe Rust and compiled to native code. It replaces the eBPF verifier with language safety plus a small runtime for exceptions, stack safety and termination, aimed at the "language-verifier gap". As of LPC 2025 it is not upstream — [USENIX ATC'25 Rex](https://www.usenix.org/conference/atc25/presentation/jia); [Phoronix via scour](https://scour.ing/p/https://www.phoronix.com/news/Linux-Kernel-Rust-Rex)
- SOSP'25 papers on extensions: "Prove It to the Kernel: Precise Extension Analysis via Proof-Guided Abstraction Refinement", "eBPF Misbehavior Detection: Fuzzing with a Specification-Based Oracle", "cache_ext: Customizing the Page Cache with eBPF" — [SOSP'25 accepted](https://sigops.org/s/conferences/sosp/2025/accepted.html)

**Live update / hot-swap**
- Linux Live Update Orchestrator (LUO) was merged for 6.19 (Dec 2025). It builds on Kexec HandOver (KHO) to preserve selected resources, such as memfds backing guest RAM, across a kexec reboot. Its main use case is cloud hypervisors — [Phoronix](https://www.phoronix.com/news/Linux-6.19-Live-Update-LUO); [LUO v4 patch](https://lkml.iu.edu/hypermail/linux/kernel/2509.3/06394.html)
- LionsOS supports late loading and dynamic component updates within a static architecture — [LionsOS paper](https://arxiv.org/html/2501.06234v2)
- Hubris restarts crashed drivers in place — [Hubris](https://hubris.oxide.computer/)
- Theseus lists live evolution and fault recovery as goals (still "not yet mature") — [Theseus GitHub](https://github.com/theseus-os/Theseus)

**Memory-management mechanisms**
- CortenMM (SOSP'25 Best Paper) drops Linux-style VMA trees and programs the MMU page tables directly through a transactional interface with scalable locking. Its concurrent code is formally verified (Verus, via vostd), and it claims 1.2–26x over Linux on real applications. It is used by Asterinas — [CortenMM search summary / vostd](https://github.com/asterinas/vostd); [SOSP'25 accepted](https://sigops.org/s/conferences/sosp/2025/accepted.html)
- Atmosphere's syscalls let threads map and unmap pages and pass pages over endpoints (L4-style user-managed address spaces) — [Atmosphere](https://mars-research.github.io/projects/atmo/)

### Inferences
- **Rings:** io_uring's lessons transfer directly. Each ring operation should carry a capability that is checked at submission. Ring activity should also be a first-class trace event source, since Linux's "blind spot" came from tracing tools hooking syscalls rather than operations. A microkernel whose IPC already *is* the ring gets both properties naturally.
- **Scheduling:** EAS requires a real energy model, meaning per-OPP power for the A520 and A720 clusters. Since no energy model or performance domains appear in the upstream O6 device tree (see Q7), the user would have to measure power per OPP themselves (e.g., via SCMI power/perf telemetry if exposed) or rely on ACPI CPPC. A pluggable scheduler interface in the sched_ext spirit, but in-kernel Rust traits, is a cheap way to keep policy swappable for experiments.
- **MTE:** It suits kernel heap hardening (HW-tag-KASAN-style async/asymm mode) and user-space allocators. It is not a confidentiality boundary, given TikTag. Whether the CIX P1 actually enables MTE must be checked at runtime (`ID_AA64PFR1_EL1.MTE`), because A720 MTE is an integration option.
- **CFI:** PAC, BTI and GCS are cheap to adopt in a Rust kernel through rustc target features and branch-protection flags. GCS is the newest and the least supported by toolchains (GCC 15; rustc support not verified here).
- **Live update:** Kernel-component hot-swap is easiest when the kernel is small and services are processes. In a microkernel, "live update" reduces to restarting or replacing a user-space server with state handoff (the Hubris and LionsOS style), which is far more tractable than Theseus-style in-kernel crate swapping or Linux KHO/LUO.

### Gaps
- PAC/BTI usage in Rust kernels: no 2025–26 primary source fetched (rustc `-Z branch-protection` status not verified).
- No source found for deterministic record/replay as an OS feature in 2024–26 (e.g., rr on aarch64, or deterministic OS designs). This remains open.
- No source on tickless/real-time design specific to 2024–26 Rust kernels. seL4 MCS (proof in progress on AArch64) is the closest.
- CHERI Alliance and Morello status for 2025–26 was not established. No evidence was found of CHERI on any Armv9 application core.
- No independent replication of the CortenMM, LionsOS or Asterinas performance claims was found.
- sched_ext energy-aware abstractions are only a plan (Dec 2025). No merged code was confirmed.

---

## 3. Formal methods for Rust kernels: Verus, Kani, Prusti, Creusot, Flux, Aeneas

### Takeaway
Verus is the clear leader for kernel-scale verification of Rust in 2024–2026. It has two SOSP'25 kernels (Atmosphere, and CortenMM, a Best Paper) and the SOSP'24 tool paper (Distinguished Artifact), with LLM-assisted proof generation emerging in 2026. Flux has shown fast, low-annotation verification of an isolation property on a production kernel (TickTock, SOSP'25). Kani is the practical bounded-model-checking tool for `unsafe` blocks. For a solo developer, the practical recipe is Kani for unsafe primitives, Flux or Verus for a few critical invariants (page tables, capability derivation, allocator), and no attempt at full functional correctness.

### Cited Findings
- Verus (SOSP'24, Distinguished Artifact Award). Case studies include an OS page table, NUMA-aware replication (NrOS lineage), a crash-safe storage system and a concurrent allocator: about 6.1K implementation lines against 31K proof lines. It claims 3–61x faster verification than the prior state of the art — [Verus MSR page](https://www.microsoft.com/en-us/research/publication/verus-a-practical-foundation-for-systems-verification/); [Verus paper copy](https://www.andrew.cmu.edu/user/chanheec/verus-sys.pdf)
- Atmosphere has a 7.5:1 proof-to-code ratio, "significantly lower than in prior approaches". It uses a big lock to keep proofs simple — [Atmosphere](https://mars-research.github.io/projects/atmo/)
- CortenMM (SOSP'25 Best Paper) verifies concurrent MMU-manipulating code — [vostd](https://github.com/asterinas/vostd); [CortenMM paper record](https://dblp.org/pid/02/4293)
- vostd (Oct 2026) verifies OSTD sync and id-alloc with Verus, has found real bugs upstream, and accepts KVerus (ASE 2026) and StarVerus (KDD 2026) LLM-generated proofs with an "AI-assist" label — [vostd](https://github.com/asterinas/vostd); [KVerus arXiv 2605.03822](https://arxiv.org/html/2605.03822v1)
- A Verus-verified page-table library with refinement-based proofs is available (syswonder/verified-pt) — [verified-pt](https://www.gitlink.org.cn/syswonder/verified-pt)
- TickTock (SOSP'25) uses Flux, an SMT-based refinement-type checker for Rust, to verify process isolation for all ARMv7-M platforms Tock supports plus three RV32 platforms. It found several isolation bugs in Tock, redesigned the process abstraction to be simpler and faster on critical paths, and **verifies in about half a minute** — [TickTock paper](https://cseweb.ucsd.edu/~dstefan/pubs/rindisbacher:2025:ticktock.pdf); [PLSysSec/tock](https://github.com/PLSysSec/tock)
- Kani is a bit-precise model checker for Rust, "especially useful for verifying unsafe code". Firecracker used it on its rate limiter and VirtIO transport (2023, found a bandwidth rounding bug). A 2026 Kani paper reports contracts moving from panic-freedom to functional correctness, finding six new bugs, and over 16,000 harnesses per change in the Rust std verification campaign — [AWS blog](https://aws.amazon.com/fr/blogs/opensource/how-open-source-projects-are-using-kani-to-write-better-software-in-rust/); [Kani ASE 2026](https://conf.researchr.org/details/ase-2026/ase-2026-industry-showcase/37/Kani-A-Model-Checker-for-Rust)
- Asterinas's own soundness check for OSTD used dynamic UB detection (KernMiri), not proofs — [Asterinas paper](https://arxiv.org/html/2506.03876v1)
- seL4 (C + Isabelle/HOL) remains the only kernel with completed AArch64 functional-correctness, integrity and confidentiality proofs (the latter completed 24 Aug 2026) — [seL4 News 2026](https://sel4.systems/news/2026.html)
- LionsOS cites a high-performance NIC driver verified with Pancake (Zhao et al. 2025, "Verifying Device Drivers with Pancake") — [LionsOS paper](https://arxiv.org/html/2501.06234v2)

### Inferences
- Effort scales badly. Even at Atmosphere's 7.5:1, a 10k-line kernel would need about 75k lines of proof. A solo developer should verify only the TCB nucleus (page-table manager, frame allocator, capability table), as vostd and CortenMM do, and treat the rest with Miri/Kani/fuzzing.
- Designing for verification has payoffs even without proofs. A big kernel lock (Atmosphere), a static architecture (LionsOS/Hubris) and SPSC queues all simplify reasoning and match a microkernel with a small kernel.
- LLM-assisted Verus proof synthesis (KVerus/StarVerus, 2026) makes Verus-heavy work less costly for small teams. Upstream already accepts such proofs, though the claims are recent and need independent validation.
- Flux's half-minute verification time makes it the best fit for continuous checking of one or two key invariants, such as "user-space can only access its own memory region".

### Gaps
- No 2024–26 kernel-scale use of Prusti, Creusot or Aeneas was found in this pass, so there is no evidence either way about their kernel readiness.
- The Verus tool's handling of AArch64 inline assembly, memory ordering (weak memory) and MMIO was not established. The verified kernels cited may target x86 or abstract hardware.
- Exact Atmosphere LoC and verification time were not found.

---

## 4. Storage and system-management trends

### Takeaway
Copy-on-write filesystems remain the norm, but bcachefs was ejected from mainline Linux in 6.18 (2025) over process friction. That is a reminder that filesystem maturity takes a decade. Image-based, verified, atomic system updates (OSTree → composefs + fs-verity, bootc) are the durable system-management trend. Capability-scoped namespaces (Fuchsia components, Redox's 2026 namespace manager and filter schemes) remain the modern replacement for a global root.

### Cited Findings
- bcachefs was marked "externally maintained" in 6.17 and removed in 6.18 (about 117k lines). It now lives as a DKMS module, and openSUSE disabled it from 6.18 on — [LWN](https://lwn.net/Articles/1040120); [Linuxiac](https://linuxiac.com/linux-kernel-6-18-to-ship-without-bcachefs/); [Wikipedia](https://en.wikipedia.org/wiki/Bcachefs). One source (Linux Journal) dates the excision to 6.17, which conflicts — [Linux Journal](https://www.linuxjournal.com/content/bcachefs-ousted-mainline-kernel-move-dkms-and-what-it-means)
- composefs replaces OSTree checkout directories with a reproducible image pointing into a content-addressed object store, using an fs-verity digest on the kernel command line. Combined with overlayfs, EROFS and fs-verity, it chains Secure Boot to a verified root. bootc ships kernel, initrd/UKI and rootfs as an OCI image (2022–2024 **[background]**), and DevConf.CZ 2026 shows composefs-backed bootc on Fedora 44 — [Alexander Larsson blog 2022](https://blogs.gnome.org/alexl/2022/06/02/using-composefs-in-ostree/); [All Systems Go 2024](https://cfp.all-systems-go.io/all-systems-go-2024/talk/HVEZQQ/); [DevConf.CZ 2026](https://pretalx.devconf.info/devconf-cz-2026/talk/WNQEYL/)
- Redox (Aug 2026) moved scheme path handling into user space and added a namespace manager mediating `openat`. The `contain` sandbox gives each process a filter scheme holding a namespace fd; full `chroot` is still in progress — [Redox Aug 2026](https://www.redox-os.org/news/this-month-260831/)
- Redox Rings brought "large improvements for RedoxFS and RAMFS" — [Redox Aug 2026](https://www.redox-os.org/news/this-month-260831/)
- Fuchsia DFv2 exposes drivers as components whose capabilities other components consume through FIDL (capability routing rather than a global /dev) — [Fuchsia DFv2 migration](https://fuchsia.dev/fuchsia-src/development/drivers/migration)
- LionsOS's file-system layer is weak: no directory cache, and synchronous cleanup on close — [LionsOS paper](https://arxiv.org/html/2501.06234v2)

### Inferences
- For a solo research OS, writing a production CoW filesystem is a multi-year sink. A content-addressed, read-only, verified system image (composefs/EROFS-like: Merkle-tree verified blobs plus a small manifest) with A/B boot via UEFI is cheap to build, robust, and "future-proof". A simple log-structured or CoW store can then serve mutable user data later.
- Capability-scoped namespaces (per-process root handles, as in Plan 9, Fuchsia and Redox) fit naturally with a capability microkernel and need no global VFS in the kernel.

### Gaps
- 2024–26 developments for ZFS (OpenZFS), btrfs and RedoxFS internals were not researched in this pass.
- Plan 9 namespaces are well-known background but were not re-sourced here.
- No source found on A/B update schemes in Rust OSes specifically.

---

## 5. Virtualization as an OS feature (EL2 kernels, pKVM, driver-domain VMs, Arm CCA)

### Takeaway
Running the kernel at EL2 and offering VMs as first-class objects is established practice on Arm. seL4's verified AArch64 configuration runs at EL2, pKVM underpins Android's AVF, and Asterinas added a hypervisor module to OSTD in Oct 2026 (x86 VMX only). Reusing drivers by running Linux in a VM (LionsOS libvmm, Genode) is the most realistic way for a small team to get the GPU, NPU, USB, Wi-Fi and display on a complex SoC, at a known performance cost. Arm CCA is not a practical target here: host KVM support was still unmerged in Dec 2025, and no evidence was found that the CIX P1 implements RME.

### Cited Findings
- seL4's AArch64 verified configurations include hypervisor mode, with the kernel in EL2 — [seL4 verified configurations](https://docs.sel4.systems/projects/sel4/verified-configurations.html)
- LionsOS driver VMs:
  - A minimal Linux guest runs one userspace program that uses Linux UIO to reach LionsOS driver queues. The seL4 VMM relays IRQs, and faults are contained to the VM.
  - libvmm is a VMM library on sDDF and Microkit, intended for reusing networking and GPU drivers. A UNSW project page notes that this is "efficient in terms of engineering cost" but "comes at a significant performance cost".
  - [LionsOS paper](https://arxiv.org/pdf/2501.06234); [TS virtualisation project](https://www.trustworthy.systems/projects/virtualisation/); [UNSW research topic](https://www.unsw.edu.au/engineering/student-life/undergraduate-research-opportunities/advertised-taste-research-areas/analyse-and-optimise-driver-vms)
- seL4 Summit 2026 included a LionsOS talk on running Windows and desktop Linux on an x86-64 VMM — [sel4summit2026 sched](https://sel4summit2026.sched.com/event/2Prsw)
- Android AVF is ARM64-only. pKVM extends KVM so the host cannot access the memory of protected guests — [AVF overview](https://source.android.com/docs/core/virtualization); [AVF architecture](https://source.android.com/docs/core/virtualization/architecture?hl=de)
- In Asterinas 0.19.0 (Oct 2026), OSTD owns the virtualization mechanism (hardware state, stage-2 translation, world switch) in about 2,000 unsafe lines, while policy is safe Rust. A minimal hypervisor is about 100 lines of safe Rust. Only Intel VMX is supported, with `/dev/kvm` as a goal — [Asterinas 0.19.0](https://asterinas.github.io/2026/10/13/announcing-asterinas-0.19.0.html)
- Arm CCA: realm *guest* support was merged in Linux 6.14-rc1. *Host* KVM support was at v12 in Dec 2025, still targets RMM v1.0, and the author expects to rework it for RMM v2.0 before merging — [CCA KVM v12](https://lkml.iu.edu/hypermail/linux/kernel/2512.2/02159.html); [CCA v7](https://lkml.rescloud.iu.edu/2502.1/08295.html)
- Genode/NOVA: a multi-kernel Sculpt 25.10 image, a Windows 11 VM prototype on NOVA, and daily VM use on the Seoul VMM (2025) — [Genode Wikipedia](https://en.wikipedia.org/wiki/Genode); [NOVA](https://hypervisor.org/)
- Neutrality joined the seL4 Foundation (Mar 2026) to build the "Atoll" virtualisation platform on seL4 — [seL4 News 2026](https://sel4.systems/news/2026.html)
- SOSP'25 includes "Ghost in the Android Shell: Pragmatic Test-oracle Specification of a Production Hypervisor" (pKVM) and "The Design and Implementation of a Virtual Firmware Monitor" — [SOSP'25 accepted](https://sigops.org/s/conferences/sosp/2025/accepted.html)

### Inferences
- On the Orion O6, the GPU (Immortalis-G720), NPU, VPU, USB, PCIe Wi-Fi and display controller are complex and partly vendor-tree-only. A Linux driver VM (stage-2 isolated, with devices assigned through the SMMUv3) is probably the only way a solo developer gets a usable desktop or network quickly. Native Rust drivers would be reserved for the timer, GIC, UART, NVMe, virtio and possibly Ethernet.
- Designing the kernel to run at EL2 from the start (stage-2 tables and VM objects as kernel capabilities) keeps that path open. The general requirement is that UEFI hands control to the OS at EL2, but this was not confirmed for the O6 firmware (see Gaps).
- CCA/RME should be treated as out of scope for this board.

### Gaps
- Whether the Orion O6 UEFI firmware enters the OS at EL2 (needed for KVM or a custom hypervisor) was not confirmed by any source. Search returned nothing board-specific.
- Whether the CIX P1 implements FEAT_RME: no source.
- No 2025 upstream pKVM merge status was found.
- No measured cost for LionsOS driver VMs (only the qualitative "significant performance cost").

---

## 6. Durable bets ("future-proof") versus fads, based on 2023–2026 venues and industry adoption

### Takeaway
The durable bets are:
- Rust (or another memory-safe language) for the kernel.
- Capability-based isolation with user-space drivers.
- Shared-memory async rings for I/O and IPC.
- Partial formal verification of the TCB with Verus or Flux.
- Energy-aware heterogeneous scheduling.
- Hardware memory-safety and control-flow features (MTE/PAC/BTI/GCS).
- Image-based verified updates.

The riskier or narrower bets are:
- Language-only isolation for *untrusted* code.
- Unikernels for general-purpose use.
- Exposing a huge async kernel surface without per-op authorization (the io_uring lesson).
- Arm CCA on consumer silicon.
- Building on any single corporate OS's trajectory (Fuchsia).

### Cited Findings
- SOSP'25 alone carried Atmosphere (Verus microkernel), TickTock (Flux), Tock (Rust, 10M deployments), CortenMM (verified MM, Best Paper), CHERIoT RTOS, Rex-adjacent eBPF analysis papers and "μFork: Supporting POSIX fork Within a Single-Address-Space OS" — [SOSP'25 accepted](https://sigops.org/s/conferences/sosp/2025/accepted.html)
- ATC'25 carried Asterinas (framekernel) and Rex (safe-Rust kernel extensions) — [ATC'25 Asterinas](https://www.usenix.org/conference/atc25/presentation/peng-yuke); [ATC'25 Rex](https://www.usenix.org/conference/atc25/presentation/jia)
- SOSP'24 gave Verus its Distinguished Artifact award — [Verus MSR](https://www.microsoft.com/en-us/research/publication/verus-a-practical-foundation-for-systems-verification/)
- Industry: Tock runs in data-center RoTs and millions of laptops. Hubris runs Oxide service processors. Android's pKVM/AVF is ARM64-only and shipping — [Tock arXiv](https://arxiv.org/abs/2603.22585); [Hubris](https://hubris.oxide.computer/); [AVF](https://source.android.com/docs/core/virtualization)
- seL4 ecosystem growth: new members in 2025–26 include MEP, Cyberagentur, Gapfruit, Neutrality, Fraunhofer AISEC and ANU. The Foundation became an independent Swiss association (17 Dec 2025) — [seL4 News 2025](https://sel4.systems/news/2025.html); [2026](https://sel4.systems/news/2026.html)
- Fuchsia: 2023 layoffs (16% of staff); Nest Hub is the only shipping device line; releases continue (F31, Jul 2026) — [Wikipedia: Fuchsia](https://en.wikipedia.org/wiki/Fuchsia_(operating_system))
- io_uring's security track record led Google to restrict it — [Phoronix](https://mail.phoronix.com/news/Google-Restricting-IO_uring)
- MTE tags leak speculatively (TikTag 2024) — [arXiv 2406.08719](https://arxiv.org/html/2406.08719V1)
- Linux keeps absorbing research-like mechanisms: EEVDF (6.6), sched_ext (6.12), GCS (6.13), io_uring zero-copy receive (6.15), LUO live update (6.19) — sources cited in Q2.
- OSDI 2026 (Seattle, 13–15 Jul 2026) added an "Operational Systems" track (19 papers designated) — [OSDI'26 message](https://www.usenix.org/sites/default/files/osdi26-message.pdf)

### Inferences
- **Durable:** The repeated convergence across academia (seL4, Atmosphere, LionsOS), industry (Fuchsia, Hubris, Tock, pKVM) and Linux itself (io_uring, user-space-driver frameworks, sched_ext) on *small privileged core + typed shared-memory channels + capabilities* is strong evidence that this architecture will age well.
- **Durable:** Verus-style verification is on an upswing, with best papers in 2025, LLM proof assistance in 2026 and industrial use in vostd. It is a safe skill investment.
- **Durable but hardware-dependent:** MTE and GCS. Treat them as hardening layers, not security boundaries.
- **Narrow/fad risk:** Unikernels and library OSes remain niche (cloud and appliance guests). Releases continue, but there is no general-purpose adoption, so they make a poor primary architecture for a 12-core desktop-class board (useful only as a guest workload).
- **Narrow/fad risk:** Pure single-address-space, single-privilege-level designs (Theseus) depend on compiler correctness and cannot run untrusted native binaries. 2025's μFork shows people are still patching POSIX holes in SAS designs.
- **Unclear:** sched_ext-style BPF scheduling inside a research kernel. The idea (policy outside the TCB) is durable, but BPF itself is Linux-specific baggage. Rust trait-object policies, or Asterinas-style "safe policy injection", give the same benefit.

### Gaps
- No systematic count of 2023–2026 OSDI/SOSP/EuroSys/ASPLOS/HotOS papers by architecture type was done. The full OSDI'26, EuroSys'25/'26, ASPLOS'25/'26 and HotOS'25 programs were not retrieved.
- No adoption data for Redox or Managarm (user counts).

---

## 7. Candidate architectures for this project (Orion O6, solo/small team, Rust, research + future-proof)

### Takeaway
Three viable candidates:
- **(A) An seL4/Atmosphere-style capability microkernel in Rust at EL2**, with async shared-memory rings as the only IPC primitive, user-space drivers behind SMMUv3, and a Linux driver VM for complex SoC blocks. This is the best balance of safety, learning value, future-proofing and driver effort.
- **(B) An Asterinas-style framekernel.** Single address space, a small Verus-verified unsafe core, safe-Rust services, a Linux ABI. Best raw performance and app compatibility, but most of the driver work happens in the privileged image.
- **(C) A Theseus-style intralingual SAS/SPL kernel.** The most novel research and live-evolution story, with the weakest isolation and the smallest ecosystem.

A hybrid option is A with a per-cluster "multikernel-lite" scheduler, using the A520/A720 split as two scheduling domains.

### Cited Findings (hardware and enabling facts specific to the Orion O6)
- Upstream `sky1.dtsi` (CIX P1, v6 patch, Apr 2025):
  - 12 CPUs in one `cpu-map` cluster.
  - cpu0–3 are `arm,cortex-a520` with `capacity-dmips-mhz = 403`. cpu4–11 are `arm,cortex-a720` with `capacity-dmips-mhz = 1024`.
  - PSCI 1.0 via SMC.
  - `arm,gic-v3` with GICD at 0x0e010000, GICR covering 12 redistributors, and a `gic-v3-its` (MSI).
  - PPI partitions separate the A520 and A720 PMUs.
  - Timers include hyp-phys and hyp-virt PPIs.
  - No SMMU node and no energy-model or performance-domain data in that patch.
  - [LKML sky1 dts v6](https://lkml.iu.edu/hypermail/linux/kernel/2504.1/11376.html)
- CIX deferred upstream ACPI work until after the IP drivers. Ubuntu's 2026 concept image boots via ACPI on a Linux 7.0 CIX kernel tree with out-of-tree patches. CIX published the P1 TRM and OS/firmware developer guides (Dec 2025) — [CNX Mar 2025](https://www.cnx-software.com/2025/03/03/software-progress-and-source-code-release-for-cix-p1-armv9-soc-and-orion-o6-motherboard/); [OMG Ubuntu May 2026](https://www.omgubuntu.co.uk/2026/05/ubuntu-cix-p1-support/amp); [CNX Dec 2025 TRM release](https://www.cnx-software.com/2025/12/13/cix-releases-p1-cpu-trm-and-de)
- Radxa's Debian 13 path for mainline validation uses a 7.0.0-rc5 kernel and asks users to enable "ACPI SCMI" in the BIOS — [Radxa docs](https://docs.radxa.com/en/orion/o6/getting-started/introduction)
- Managarm's hardware CI already runs an Orion O6 in ACPI mode (Jan 2026) — [Managarm](https://managarm.org/2026/01/13/end-of-year-update.html)
- Asterinas AArch64 runs on QEMU `virt` with GICv3 only (Oct 2026) — [Asterinas 0.19.0](https://asterinas.github.io/2026/10/13/announcing-asterinas-0.19.0.html)
- seL4 AArch64 EL2 configuration is verified, but SMMU and multicore are outside the proofs — [seL4 verified configurations](https://docs.sel4.systems/projects/sel4/verified-configurations.html)
- EAS needs an energy model plus asymmetric capacity, and falls back to load balancing above 80% utilization — [kernel docs: sched-energy](https://docs.kernel.org/scheduler/sched-energy.html)

### Inferences

**Candidate A: "Capability microkernel in Rust at EL2, async-ring IPC, user-space drivers + Linux driver VM"** (lineage: seL4/Microkit, Atmosphere, LionsOS/sDDF, Redox Rings, Managarm, Hubris)

- *Kernel scope:*
  - Capability table (CNode-like) and untyped-memory retyping, or Atmosphere-style page passing.
  - Address spaces and threads.
  - SPSC ring endpoints plus notifications as the *only* IPC (sDDF-style, with completion semantics like io_uring).
  - IRQ capabilities via GICv3/ITS.
  - SMMUv3 stream-table and context-descriptor management as a kernel or privileged-server capability.
  - Stage-2 VM objects at EL2.
  - Timer/tickless scheduling.
- *Performance on this SoC:* LionsOS evidence suggests a lean microkernel with SPSC queues can beat Linux on I/O-heavy paths on Arm, which offsets IPC costs. Cross-core queue costs (LionsOS on x86 roughly doubled CPU) argue for placing producer and consumer on the same cluster and scheduling per cluster (A520 domain and A720 domain), which is a "multikernel-lite" pattern. GICv3 ITS and MSI suit per-core driver queues.
- *Safety:* Hardware isolation between all services (MMU+SMMU) and Rust inside each. The nucleus is small enough (target under 10k LoC) to verify the page-table, capability and allocator invariants in Verus, possibly reusing ideas or code from verified-pt and Atmosphere, with Kani on unsafe asm and MMIO glue. MTE async/asymm mode on the kernel heap and user allocators, and PAC/BTI/GCS everywhere.
- *Driver effort:* Native Rust drivers for the UART (PL011-compatible?), GIC, generic timer, NVMe, Ethernet and virtio. Everything else (GPU, NPU, VPU, USB, Wi-Fi, display) goes through a Linux driver VM with SMMU-assigned devices, accepting the performance cost. ACPI parsing (MADT, IORT for the SMMU, GTDT, PPTT for topology) is unavoidable. Managarm's O6 ACPI support is a reference.
- *Learning value:* Very high. It covers MMU, EL2, SMMU, GIC, capabilities, IPC design, user-space driver frameworks, verification, and heterogeneous scheduling with a measured energy model.
- *Future-proofing:* Highest. It matches where seL4, Fuchsia, Hubris, LionsOS and Redox all converged. Live update becomes "restart or replace a server with state handoff".
- *Main risks:* No Linux ABI, so a POSIX/Linux personality server is needed (Managarm and Starnix show it is possible but large). Multicore kernel design is unverified everywhere (even seL4). Start with a big lock (Atmosphere) or per-cluster kernel instances.

**Candidate B: "Asterinas-style framekernel"** (lineage: Asterinas/OSTD + vostd + CortenMM)

- *Kernel scope:* One privileged address space. A small `unsafe` framework (OSTD-like: frames, typed vs untyped memory, page tables à la CortenMM, IRQ, DMA via SMMU, user-mode entry, EL2 virtualization module) with everything else in safe Rust, including drivers and a Linux syscall layer.
- *Performance:* No IPC boundary crossings, so it is near-Linux or better (single-core claims in ATC'25; CortenMM claims 1.2–26x on MM-heavy workloads). It is the best fit for maximizing raw throughput on 12 cores, though Asterinas's own SMP scalability is still in progress.
- *Safety:* A sound TCB of about 14% of LoC (Asterinas metric), but a logic bug or compiler or `unsafe` bug anywhere in the privileged image is fatal. Driver faults are not contained by hardware (only by Rust). The SMMU still constrains DMA.
- *Driver effort:* Highest. Every device needs an in-kernel safe-Rust driver, and Linux driver VMs are possible later but the Asterinas hypervisor is x86-only so far. The Linux ABI gives instant userland (BusyBox, Nix) and is a huge productivity win for testing.
- *Learning value:* High for language-based OS design and verification (vostd), and lower on hardware isolation and IPC.
- *Practical option:* Fork or contribute to Asterinas's AArch64 port (QEMU-only, Tier 2, Oct 2026) to get to the O6 faster. This does not satisfy "kernel from scratch" but could serve as a reference.

**Candidate C: "Theseus-style intralingual single-address-space/single-privilege-level"** (lineage: Theseus, Singularity **[background]**, RedLeaf)

- *Kernel scope:* Everything is a Rust crate loaded into one address space at one privilege level. Isolation and resource management come from ownership and affine types, and crates can be swapped live (live evolution, fault recovery).
- *Performance:* Zero-cost calls between "processes" and no TLB or syscall overhead. Potentially excellent on the A520s, where syscall and IPC overheads hurt most.
- *Safety:* The weakest against untrusted or native code. It relies on rustc soundness, and the Asterinas comparison shows Theseus's linked-code TCB at 62.4%. It cannot run arbitrary Linux or C binaries without reintroducing hardware isolation (cf. μFork, SOSP'25). MTE and PAC add limited help.
- *Driver effort:* Moderate. Drivers are crates, but nothing is reused from Linux unless an EL2 VM is added.
- *Learning value:* The highest research novelty (live evolution, state management), but the least transferable to mainstream OS practice. Theseus's own aarch64 port is incomplete.
- *Future-proofing:* Lowest. It is a niche research direction with little 2024–26 venue activity.

**Recommendation sketch (inference)**

1. Primary: Candidate A.
2. Borrow from B the "small unsafe core + safe-Rust everything else + Verus on page tables/allocator" discipline *inside* the microkernel, plus CortenMM's direct-page-table MM.
3. Borrow from C the idea of restartable, hot-swappable *user-space* components (Hubris/LionsOS style) rather than in-kernel crate swapping.
4. Hardware-specific choices:
   - Boot via UEFI at EL2, parsing ACPI (MADT, GTDT, IORT, PPTT, SPCR).
   - Run the kernel at EL2 with VHE-style EL2&0 translation regime, if supported.
   - Model the A520 and A720 clusters as separate scheduling domains with a measured energy model, and use EEVDF within each domain.
   - Use GICv3 ITS for per-queue MSIs and SMMUv3 for every DMA-capable driver.
   - Enable MTE (if `ID_AA64PFR1_EL1` reports it on CIX P1), PAC, BTI and GCS.
   - Start with QEMU `virt` plus GICv3 for development, then move to the O6.

### Gaps
- Whether the "8x A720 in two performance tiers" split shows up in firmware: the Apr 2025 DT gives all A720 cores the same `capacity-dmips-mhz` (1024). The tier split probably comes from different max frequencies or ACPI CPPC data, which was not confirmed.
- The SMMUv3 instance count, stream IDs and IORT details for the O6 were not found (no SMMU node in the upstream DT patch).
- MTE availability on the shipping CIX P1 (integration-dependent per the A720 forum claim) was not confirmed. SVE2 vector length was also not confirmed.
- No source confirms UEFI hands off at EL2 on the O6, which candidate A's EL2 design depends on. QEMU and other Arm UEFI platforms usually do.
- No measured per-OPP power data or SCMI power telemetry details for the CIX P1 were found, so EAS energy-model inputs would have to be measured by the user.
