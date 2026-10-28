# Userland, application platform and graphics stack for a new Rust OS on the Radxa Orion O6 (CIX P1 / "Sky1" / CD8180)

Research date: 2026-10-25. All facts are date-stamped where the source gives a date. "Inference" bullets are my reasoning and estimates, not sourced facts. Some items I believe from prior knowledge but could not re-verify this session are listed under Gaps and marked "unverified". Naming note: CIX P1 = CIX "Sky1" = CD8180. The Orange Pi 6 Plus uses the same SoC, so its bring-up reports apply directly to the O6.

---

## 1. GPU acceleration on a non-Linux kernel (Mali-G720): how tightly Mesa/PanVK depends on Linux uAPIs, what PanVK supports on G720, what other OSes did, and whether to emulate DRM, build a Magma-like API, or write a Rust Vulkan driver

### Takeaway
As of Oct 2026, Mesa's PanVK advertises Vulkan 1.4 (and Panfrost GL ES 3.1) on the Mali-G720 (v12). It is not formally conformant on G720. It has been shown working on this exact SoC through Linux's `panthor` CSF kernel driver, with out-of-tree SoC patches. PanVK's kernel dependency is narrow and well defined: about 17 panthor ioctls plus GEM/PRIME (dma-buf), syncobj/timeline-syncobj and BO mmap. Mesa also isolates it behind its `pan_kmod` layer, which community forks have already retargeted to Arm's kbase driver. The best reuse-to-effort ratio is to keep Mesa as unmodified as possible. Two ways to do that: expose a panthor-compatible "render node" ABI from a Rust CSF kernel driver (using Tyr, the Rust port of panthor, and panthor itself as references), or add a small `pan_kmod` backend for a native interface. Writing your own Vulkan driver in Rust is a multi-year project.

### Cited Findings

**PanVK / Panfrost status on Mali-G720 (v12, "5th Gen")**
- Mesa's Panfrost doc table lists G720 (5th Gen, v12): OpenGL ES 3.1, OpenGL 3.1, **Vulkan 1.4**, OpenCL 3.0. G725 (v13) and the G1-series (v14) are listed the same way. Conformance footnotes exist only for other GPUs: G610 has a Vulkan 1.4 submission, G57/G610 have GL ES 3.1 submissions, and G310/G610 have OpenCL 3.0 submissions. No conformance footnote for G720. Experimental PanVK GPUs need `PAN_I_WANT_A_BROKEN_VULKAN_DRIVER=1`. The page is "latest documentation" and gives no version. — [Mesa Panfrost docs](https://docs.mesa3d.org/drivers/panfrost.html)
- 2025-07-29: Collabora enabled Vulkan 1.4 for PanVK on v10+ in Mesa 25.2. They said "the Vulkan 1.4 support is not yet formally conformant", and that it met the Roadmap 2022/2024 profile requirements. Stated goals: match the Arm DDK's performance, improve app compatibility, and bring up Bifrost and v9. — [Collabora: PanVK now supports Vulkan 1.4](https://www.collabora.com/news-and-blog/news-and-events/panvk-now-supports-vulkan-1.4.html)
- Mesa 25.1 (Apr 2025) added Panfrost and PanVK support for Mali 5th Gen v12/v13 (G720/G725). — [Phoronix](https://www.phoronix.com/news/Mesa-25.1-Newer-Mali-5th-Gen)
- PanVK was declared Vulkan 1.2 conformant on Mali-G610 in May 2025. — [9to5Linux](https://9to5linux.com/panvk-open-source-vulkan-driver-for-arm-mali-gpus-is-now-vulkan-1-2-conformant)
- Mesa 26.2.0-rc1 (2026-07-15) adds Panfrost support for Mali G1-series GPUs. This is a secondary source. — [linuxcompatible.org](https://www.linuxcompatible.org/story/mesa-2620rc1-and-2615-released-opencl-31-support-vulkan-updates-and-stability-fixes/)

**Same-SoC (CIX Sky1) bring-up evidence: what the GPU needs**
- Orange Pi 6 Plus (Sky1) report dated 2026-02-13:
  - panthor probes a "Mali-G720-Immortalis" (ID 0xc870, 10 shader cores, MC10) at 900 MHz, with 6 DVFS OPPs from SCMI.
  - **OpenGL ES 3.1 and Vulkan 1.4.328 via PanVK, Mesa 25.3.3**, on kernel 6.19.0-rc8-sky1.
  - glmark2-es2-drm score 3079 at 1080p. Vulkan buffer fill 37.4 GB/s, copy 21.4 GB/s.
  - Limitations: "Vulkan compute is unstable for large workloads (OOM / DeviceLost)", PanVK is non-conformant, and SIGKILL of GPU processes can make the panthor MCU reset fail, which needs a reboot.
  - Bring-up quirks: under ACPI, the SCMI power SET for `gpu_pd` returned success but did not power the GPU, so `acpi=off` (device tree) was "mandatory". The GPU reset had to be deasserted directly.
  - Firmware: `arm/mali/arch12.8/mali_csffw.bin`.
  - Display path: Linlon-D60 DPU → Trilin-DPSUB → PS185 DP-to-HDMI bridge.
  - Source: [visorcraft/orange-pi-6-plus-gpu](https://github.com/visorcraft/orange-pi-6-plus-gpu)
- The Sky1-Linux project claims GPU acceleration (Vulkan/GL ES) works via panthor on a 6.18.2 LTS kernel carrying 68 patches. It lists the "GPU init sequence" as not submitted upstream. — [Sky1-Linux](https://github.com/Sky1-Linux)
- CIX's vendor GPU kernel module is an out-of-tree `mali_kbase` based on Arm Mali DDK r54p1, from the cix-go-2025q4 SDK. — [Sky1-Linux/cix-gpu-kmd](https://github.com/Sky1-Linux/cix-gpu-kmd)
- Arm's Karunika Choo posted patches adding G720 (and G310/G510/G710/G715/G725) support to panthor (2025). — [Radxa forum](https://forum.radxa.com/t/support-for-the-mali-g720-coming-to-panthor-kernel-driver/26391)
- A Sky1 installer ships both the vendor driver and "panthor-cix" (the PanVK path). The Orion O6 is marked "user-reported working". — [ncz-os/cix-installer](https://github.com/ncz-os/cix-installer)

**The kernel interface PanVK actually needs (panthor uAPI)**
- Panthor ioctls: DEV_QUERY, VM_CREATE/DESTROY, **VM_BIND** (map/unmap/sync-only, optionally async on a VM queue), VM_GET_STATE, BO_CREATE, BO_MMAP_OFFSET, GROUP_CREATE/DESTROY, **GROUP_SUBMIT**, GROUP_GET_STATE, TILER_HEAP_CREATE/DESTROY, BO_SET_LABEL, SET_USER_MMIO_OFFSET, BO_SYNC, BO_QUERY_INFO.
  - Sync ops reference a **syncobj or timeline syncobj** (WAIT/SIGNAL).
  - Groups bind to one VM and hold priority queues with ring buffers and core masks.
  - Tiler heaps use chunk sizes from 128 KiB to 8 MiB.
  - Source: [include/uapi/drm/panthor_drm.h](https://raw.githubusercontent.com/torvalds/linux/master/include/uapi/drm/panthor_drm.h)
- Mesa's drm-shim is an LD_PRELOAD library that fakes the DRM kernel interface so Panfrost can run without Mali hardware. It defaults to mocking a Mali-G52; `PAN_GPU_ID` selects another GPU. Caveats from its docs: libc interception is "rather glibc-specific and fragile", and it can't handle DRM fds passed over X11. — [Mesa drm-shim docs](https://docs.mesa3d.org/drivers/panfrost/drm-shim.html)
- A community fork adds a **kbase backend in Mesa's shared `pan_kmod` layer**, selected by a build option, so Panfrost/PanVK runs on Arm's proprietary kbase instead of panfrost/panthor DRM. — [yo-www/panvk](https://github.com/yo-www/panvk)
  - A Termux packaging PR for "panvk-kbase" notes the series is 172 patches. — [TUR PR #2962](https://github.com/termux-user-repository/tur/pull/2962)
  - G720-specific kbase/CSF PanVK research builds exist; they disclaim conformance. — [gpcabbay/panvk-g720-kbase-csf](https://github.com/gpcabbay/panvk-g720-kbase-csf)

**Rust kernel-side reference: Tyr**
- Tyr is a Rust port of panthor for CSF Mali. Its goal is feature parity and eventually exposing panthor's uAPI so it can be a drop-in for PanVK. Collabora estimated it would "probably take a couple of years". The upstream code is experimental: it powers up the GPU on RK3588, queries metadata via MMIO, and exposes it via DEV_QUERY. A downstream branch is "well ahead". — [Collabora: Introducing Tyr](https://www.collabora.com/news-and-blog/news-and-events/introducing-tyr-a-new-rust-drm-driver.html); [rust-for-linux.com Tyr](https://rust-for-linux.com/tyr-gpu-driver)
- Tyr was merged via drm-rust-next for Linux 6.18 (late 2025). — [Phoronix](https://phoronix.com/news/Rust-DRM-Drivers-Linux-6.18-Tyr)

**Precedent: FreeBSD (LinuxKPI / drm-kmod), including on this SoC**
- `drm-panthor-kmod`: Linux v7.0 panthor built against FreeBSD drm-kmod's DRM core and LinuxKPI.
  - drm-kmod is based on Linux 6.13 and lacks the GEM shmem helper, so newer GPUVM/GPU-scheduler/shmem code is bundled into the module with a `panthor_` prefix. The GPU MMU uses Linux 7.0 io-pgtable (Arm LPAE).
  - On CIX Sky1 it binds to ACPI device **CIXH5000**. Before any register access it brings up the power domain (TF-A), clocks (SCMI), a power resource, resets and Q-channel clock gating via a `sky1_scmi` driver. Devfreq/OPP is replaced by custom 50 ms-sampling DVFS on the SCMI perf domain (72–1000 MHz).
  - Status: CSF firmware boots (interface v3.13) and `/dev/dri/renderD128` appears. Not yet working: runtime suspend, DVFS, and **mmap of imported dma-bufs**.
  - Source: [JamesKane/drm-panthor-kmod](https://github.com/JamesKane/drm-panthor-kmod)
- FreeBSD 15.0 forum report: panfrost attaches on a Rockchip board but reports "Mali GPU clock is unknown". DRM/VOP2 display and panfrost work only on different kernel configs. — [FreeBSD forums](https://forums.freebsd.org/threads/how-to-run-freebsd-on-new-boards-built-on-rockchip-35xx.88430/page-13)

**Precedent: Fuchsia Magma**
- Magma splits the driver into a Magma System Driver (MSD, "analogous to a kernel driver on other operating systems") and a client ICD that implements Vulkan.
  - The ICD opens `/loader-gpu-devices/class/gpu` and calls `magma_device_import`, and "calls to `magma_*` functions gradually replace `ioctl()` calls".
  - ICDs must be statically linked and may reference only `libc.so` and `libzircon.so`.
  - Fuchsia-specific Vulkan extensions: `VK_FUCHSIA_external_memory`, `VK_FUCHSIA_external_semaphore`, `VK_FUCHSIA_buffer_collection` (sysmem).
  - Magma supports only UMA devices.
  - `msd-arm-mali` is cited as the example "unified MSD". MSDs must be open source but not GPL.
  - Prerequisites: a reference Linux Vulkan implementation and a Vulkan 1.1/1.2-conformant ICD.
  - Source: [Fuchsia Magma porting guide](https://fuchsia.dev/docs/development/graphics/magma/concepts/porting)
- The Magma test strategy lists "ARM Mali – Bifrost" among supported GPUs. — [Magma test strategy](https://fuchsia.dev/docs/development/graphics/magma/concepts/test_strategy)
- The Magma GN build has a `build_libvulkan_arm_mali` argument and a board flag for devices that ship a Mali libvulkan ICD. — [magma.gni](https://fuchsia.googlesource.com/fuchsia/+/0c00fd3/src/graphics/lib/magma/gnbuild/magma.gni)
- The Magma API is formalised in RFC-0198. — [RFC-0198](https://fuchsia.dev/docs/contribute/governance/rfcs/0198_magma_api_design)
- The `msd-arm-mali` source tree (include/, src/, tests/) exists, but its README/metadata did not say which Mali generations (JM vs CSF) it supports. — [msd-arm-mali tree](https://fuchsia.googlesource.com/fuchsia/+/main/src/graphics/drivers/msd-arm-mali/)

**Precedent: Managarm (DRM uAPI emulation on a microkernel)**
- Mesa's vendored `include/drm-uapi` (`drm.h`, `dma-buf.h`) carries a "mesa: add managarm support" commit dated 2023-11-20. Mesa upstream therefore accepted tweaks for a non-Linux OS that implements the DRM uAPI. This is a mirror/secondary source. — [Mesa drm-uapi mirror](https://gitlab.neroreflex.duckdns.org/NeroReflex/mesa/src/commit/2a0d196abf1cd85e18ff0adfd05703ed4838d779/include/drm-uapi)
- The Managarm handbook lists Weston and kmscon as supported. Graphics support: Intel G45 and VBE on hardware; virtio-gpu, Bochs VBE and VMware SVGA in VMs. — [Managarm Handbook](https://docs.managarm.org/handbook/print.html)
- An undated (likely older) OSDev post by a Managarm developer marks Bochs/virtio DRM drivers, libdrm, and Mesa with GBM and EGL as done. It marks virtio hardware rendering, libudev and Weston as not done. This conflicts with the handbook on Weston; the handbook is probably newer. — [OSDev forum](https://forum.osdev.org/viewtopic.php?p=281801)

**Precedent: Redox (Rust microkernel)**
- Jan 2026: Redox started its own Intel GPU driver. It does modesetting only, with no hardware acceleration (Kaby Lake / Tiger Lake). — [Phoronix](https://www.phoronix.com/news/Redox-OS-Own-Intel-GPU-Driver)
- Redox began implementing basic read-only Linux DRM APIs to ease driver porting. — [OSNews](https://www.osnews.com/story/144138/redox-gets-basic-linux-drm-support/)
- Mar 2026: bjorn3 extended the DRM API, added GPU memory mapping to the Intel driver, and added shadow buffers for write-combining memory. — [This Month in Redox, March 2026](https://www.redox-os.org/news/this-month-260331/)
- Priorities (2025-09): the first GPU goal is virtio-gpu acceleration "probably using virglrenderer", then Intel. — [Redox priorities 2025/26](https://www.redox-os.org/news/development-priorities-2025-09/)
- Aug 2026: fixed EGL partial rendering on Mesa3D and EGL on the GTK3 port. This is software Mesa; no accelerated GPU was reported. — [This Month in Redox, August 2026](https://www.redox-os.org/news/this-month-260831/)

**Precedent: Genode**
- 17.08: an experimental GPU multiplexer for Intel Gen8 with Mesa 11.2.2 (OpenGL 4.5). Trusted and untrusted GL clients run side by side. — [Genode 17.08 release notes](https://genode.org/documentation/release-notes/17.08)
- 21.08: improved Intel GPU multiplexer, display driver decoupled from the GPU driver, revamped Mesa stack. — [Genode 21.08 announcement](https://lists.genode.org/pipermail/users/2021-August/007758.html)
- 2022: Intel Gen12 (Xe) enabled in the multiplexer. — [Genode roadmap 2023 thread](https://lists.genode.org/pipermail/users/2023-January/008177.html)
- Vulkan was a stated intent in 2021 and 2023. A shipped release was not confirmed. — [Genode roadmap 2022](https://lists.genode.org/pipermail/users/2022-January/007881.html); [Genode roadmap 2023](https://lists.genode.org/pipermail/users/2023-January/008177.html)

**Precedent: Haiku**
- 2021: X512 ported RADV (after lavapipe) and then ran Zink over RADV, with Blender working. The main obstacle was the need for a DRM-like kernel driver. — [Phoronix: RADV on Haiku](https://www.phoronix.com/news/RADV-Port-WIP-For-Haiku); [Phoronix: Zink+RADV on Haiku](https://www.phoronix.com/news/Haiku-OS-Zink-RADV)
- I found no 2025–2026 Haiku GPU status. The 2025 GSoC/mentorship listing still has "GPU acceleration" as a topic. — [Haiku forum](https://discuss.haiku-os.org/t/gsoc-2025-my-interest-in-the-haiku-mentorship-program-gpu-acceleration-support/16447)

### Inferences
- **G720 is a CSF (Command Stream Frontend) GPU.** A kernel driver must load the `arch12.8` CSF firmware, run the MCU, and schedule "groups" onto firmware command-stream slots. It must also manage the GPU MMU via LPAE page tables, grow the tiler heap, and handle faults and resets. This is a substantially bigger job than an old Job-Manager Mali driver. Panthor (C), Tyr (Rust, partial) and the FreeBSD port are three readable references. The FreeBSD port shows the Sky1 glue: CIXH5000, SCMI clocks/perf, TF-A power domain, resets.
- **Option comparison.** The effort figures are my estimates for one experienced developer, from "kernel boots on O6" to "vkcube plus a wgpu triangle".

| Option | What you build | Mesa changes | Reuse unlocked | Est. effort | Risk |
|---|---|---|---|---|---|
| **A. Panthor-compatible render-node ABI** (DRM uAPI subset: version/caps, GEM handles, PRIME fd import/export, syncobj + timeline, mmap offsets, 17 panthor ioctls) | Rust CSF driver + thin "drm" char-device/ioctl personality | ~none (Managarm precedent: small header tweaks) | PanVK, Panfrost GL, Zink, libdrm, gbm, Smithay/wlroots DRM backends (with a KMS subset) | 6–12 PM kernel driver + 1–3 PM uAPI layer; KMS for the Linlon-D60 DPU extra 3–6 PM | Must track panthor uAPI churn; Linux ioctl semantics leak into the OS design |
| **B. Native GPU API + `pan_kmod` backend in Mesa** | Rust CSF driver with an idiomatic capability/IPC API; ~1 C file in Mesa's pan_kmod | Small but out-of-tree, plus a WSI path | PanVK/Panfrost; not libdrm-based tools | Kernel same as A; Mesa backend 1–2 PM | Rebasing on Mesa; kbase forks show it works but accumulate patches |
| **C. Magma-style split (MSD + ICD over IPC)** | Rust MSD + IPC protocol; patch ICD ioctl calls → `magma_*`; own external-memory/semaphore extensions | Medium (Fuchsia keeps a Mesa fork) | PanVK via fork; clean security story | Kernel similar + 2–4 PM ICD glue | Fork maintenance; Fuchsia notes Magma is UMA-only (fine for O6) |
| **D. Own Vulkan driver in Rust** | Full Vulkan runtime + Valhall/5th-gen ISA compiler | n/a | Nothing from Mesa | Multi-year (PanVK has years of Collabora effort behind it) | Very high; conformance unrealistic |

- **Recommendation for a Rust-first research OS:** A hybrid of A and B.
  1. Write the CSF driver in Rust, modelled on panthor/Tyr. Make its native interface capability-based: BO = memory capability, VM = address-space object, syncobj = native fence/timeline object, dma-buf = transferable memory handle.
  2. Expose a panthor-uAPI-compatible personality, either in-kernel or as a userspace translation library replacing `libdrm` calls, so stock PanVK works on day one.
  3. Move to a `pan_kmod` backend later if the DRM personality becomes a burden.
  This mirrors Managarm's approach to reuse and Fuchsia's separation of concerns.
- **Bring-up shortcut.** Run Mesa with drm-shim, or a Rust "fake panthor" in userspace on Linux, to test the translation layer before the real kernel driver exists. Use virtio-gpu/venus in QEMU on the O6 (KVM on Armv9) as the first accelerated target before touching the Mali hardware. This follows Redox's "virtio first" plan.
- **dma-buf semantics matter beyond the GPU.** The same "shareable buffer + fence" primitive is needed for compositor scan-out, the video decoder, camera and NPU zero-copy. The FreeBSD port's broken "mmap of imported dma-bufs" shows this is a common sticking point. Design it once as a kernel object.
- **Display (KMS) is separate work.** The O6's display path (Linlon-D60 + DP/HDMI bridge) is a separate driver from the GPU. The UEFI GOP framebuffer is an adequate first target for a software compositor.
- **Expect instability at the top end.** Same-SoC reports show Vulkan compute OOM/DeviceLost on large workloads and MCU reset failures after SIGKILL (Feb 2026). Even on Linux, robust GPU reset and recovery on this SoC is unsolved, so a research OS should budget time for fault handling.

### Gaps
- No Khronos conformance entry for PanVK on G720 was found. Whether Collabora has submitted G720 (v12) for 1.4 conformance by Oct 2026 is unknown.
- Which Mali generations Fuchsia's `msd-arm-mali` supports (Job Manager vs CSF) is unknown; the tree had no README. Whether Fuchsia's Mali Vulkan ICD is Arm's proprietary blob or a Mesa port is also unknown.
- Whether Mesa upstream has merged a kbase or non-DRM `pan_kmod` backend is unknown; only forks were found.
- SerenityOS VirGL/virtio-gpu 3D work: no source found this session. I believe SerenityOS had experimental VirGL in 2022 (unverified).
- Genode on Arm Mali: Genode's Lima/Mali-400 work on PinePhone is unverified, and I found no Genode CSF Mali work.
- 2026 Managarm GPU status (any hardware-accelerated driver): not found.
- Haiku accelerant model details and 2026 Mesa status: not found.
- Exact Tyr status in 2026, and whether it runs Vulkan workloads downstream: not found.

---

## 2. Graphics stack above the driver: wgpu/Vello, compositors and Wayland (Smithay, cosmic-comp, niri), whether Wayland is worthwhile on a non-Linux OS, Rust GUI toolkits and their platform-abstraction needs, and text stacks

### Takeaway
The Rust graphics ecosystem is mature enough to build the whole desktop above the driver: wgpu 29.x (Mar 2026), Vello 0.6, Smithay-based compositors, and Slint/iced/egui/Xilem toolkits. The key decision is the window-system protocol. Adopting Wayland unlocks existing toolkits and COSMIC apps for little protocol work, because Smithay and wayland-rs are pure Rust. The main porting work is Smithay's backends (DRM/KMS, input, session), not the protocol. Redox and Haiku both concluded Wayland is the most practical compatibility route.

### Cited Findings
- **wgpu:** v29.0.1 released 2026-03-26 (wgpu-core/hal/naga/types). 29.0.2 and 29.0.3 followed. — [newreleases.io: wgpu v29.0.1](https://newreleases.io/project/github/gfx-rs/wgpu/release/v29.0.1)
- **Vello:** v0.6.0 upgraded to wgpu v26 for Bevy 0.17 compatibility. This was reported in Linebender's October 2025 update. — [Linebender in October 2025](https://linebender.org/blog/tmil-22/)
- **Masonry and Xilem:** a draft Linebender post says version 0.4 of both was released in October 2025. This is a draft, so treat it as unconfirmed. — [linebender.github.io PR #130](https://github.com/linebender/linebender.github.io/pull/130.diff)
- **Slint:** the platform abstraction is the `platform::Platform` trait, installed with `set_platform()` before creating components, plus a `WindowAdapter`. Slint can be built without any built-in backend. Renderers:
  - FemtoVG over OpenGL, for custom platforms where "your code delivers events to Slint"
  - `renderer-femtovg-wgpu`, which renders into a wgpu texture
  - a software renderer
  - Sources: [Slint platform docs](https://docs.slint.dev/latest/docs/rust/slint/platform/); [Slint backends & renderers](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/); [FemtoVG renderer](https://docs.slint.dev/latest/docs/rust/slint/platform/femtovg_renderer/)
- **Redox platform work (Aug 2026):** the Orbital login manager was ported to **winit and softbuffer**, "enabling future Wayland testing". Orbital also gained rootless display opening on `inputd`. — [This Month in Redox, August 2026](https://www.redox-os.org/news/this-month-260831/)
- **Redox and COSMIC (Sept 2025):** COSMIC Terminal, Files, Editor, Reader and Store run on Redox, but "we don't currently support the COSMIC Compositor". "Porting Wayland will be a big step towards supporting the COSMIC Compositor". — [Redox priorities 2025/26](https://www.redox-os.org/news/development-priorities-2025-09/)
- A Redox "Port cosmic-comp" issue notes that Rust crates already have upstream Redox support, which makes cosmic-comp the easiest Wayland option for Redox. — [Redox issue #1408](https://gitlab.redox-os.org/redox-os/redox/-/issues/1408)
- **Smithay compositors:** cosmic-comp is built on Smithay, and niri was started on Smithay "to speed up compositor development". Catacomb, MagmaWM and Pinnacle also use it. — [LWN: niri tour](https://lwn.net/Articles/1025866); [List of display servers](https://en.wikipedia.org/wiki/List_of_display_servers)
- **Non-Linux Wayland status:** Wikipedia marks COSMIC as not available on the BSDs. FreeBSD-targeted compositors (Hikari, Wayfire) use wlroots. — [List of display servers](https://en.wikipedia.org/wiki/List_of_display_servers)
- **Haiku Wayland:** a compatibility layer uses modified libwayland. It provides an ABI-compatible `libwayland-client.so`, and the "server" is an in-process add-on (`wayland-server-inproc.so`) that maps, for example, `wl_surface_commit` → `BView::SetViewBitmap()`. There is no separate compositor; Haiku's app_server draws. It targets GTK3/4, and the post argues Wayland is easier to implement than X11. — [Haiku forum: Wayland compatibility layer](https://discuss.haiku-os.org/t/my-progress-in-wayland-compatibility-layer/12373); [linux.org.ru](https://www.linux.org.ru/news/opensource/17016555)

### Inferences
- **Recommended stack.** wgpu (Vulkan backend → PanVK) as the universal GPU API. Vello (wgpu compute-based 2D) or tiny-skia/softbuffer as a CPU fallback. A Smithay-based compositor (or a cosmic-comp/niri fork) speaking Wayland to clients. winit as the app-side window abstraction. Toolkits on top: Slint, iced, egui, Xilem.
  - wgpu's Vulkan-only path means a working PanVK unlocks every wgpu app. Until then, softbuffer plus a CPU renderer gives a usable desktop.
  - Redox has already ported winit and softbuffer, so its patches are a template for adding a new `target_os` to those crates.
- **Wayland is worth adopting as the app protocol** even on a non-POSIX-shaped kernel:
  1. Toolkits (winit, GTK, Qt, SDL) all have Wayland clients.
  2. The protocol only needs Unix-domain-socket-like byte streams with fd/handle passing plus shared memory. A capability OS can provide these naturally.
  3. Haiku's in-process approach shows a minimal path. Redox's choice shows the community consensus.
  - The OS-specific pieces are Smithay's backends: DRM/KMS output, GBM/EGL or Vulkan allocation, libinput/udev input, logind-style sessions. Plan to write a native backend for each, or emulate those Linux interfaces (which ties back to Section 1, option A).
- **Estimated effort (mine):**
  - Smithay software compositor on a GOP framebuffer with native input: 2–4 PM.
  - GPU-accelerated composition via a dma-buf/syncobj-equivalent and the GLES or Vulkan renderer: another 2–4 PM once PanVK works.
- **Slint is the lowest-friction toolkit for a bare platform.** Its `Platform`/`WindowAdapter` traits plus the software renderer let you run a GUI directly on a framebuffer before any compositor or GPU exists.
- **Text:** cosmic-text and parley (shaping/layout), with swash/skrifa (rasterization), are pure Rust and OS-agnostic apart from font discovery. They need only a font directory or fontconfig replacement. Unverified this session; see Gaps.

### Gaps
- No primary source was fetched for cosmic-text, parley, swash/skrifa or fontique, nor for iced/egui 2026 versions and their exact platform-abstraction requirements (they use winit and wgpu/glow or tiny-skia; unverified).
- No 2026 Linebender (Vello/Xilem) release notes were found. Whether Vello reached a newer release in 2026 is unknown.
- I found no FreeBSD port of Smithay, cosmic-comp or niri to cite, despite community reports I recall (unverified).
- Whether any Redox Wayland compositor was running by Oct 2026 is unknown; August 2026 only mentions "future Wayland testing".

---

## 3. libc / POSIX / Rust std: relibc portability, musl/picolibc, adding a new OS target to Rust std, and WASI (0.2/0.3, component model) as the native app ABI

### Takeaway
relibc is a portable POSIX libc written in Rust with a clean `platform/{linux,redox}` split. Adding a third platform module for a new kernel is the most Rust-first route to POSIX and C-ecosystem compatibility. It is actively maintained in 2026 and includes Linux AArch64 support. For Rust programs, a Tier-3 `aarch64-unknown-<youros>` target whose std "PAL" proxies to an OS runtime crate (Motor OS's 2025 approach) is the established pattern. WASI 0.3.0 (final 2026-06-11, Wasmtime 46+ by default) brings native async to the component model. That makes WASI P3 a credible, future-proof second ABI for sandboxed apps, though nobody has shipped a WASI-native general-purpose OS.

### Cited Findings
- **relibc design:**
  - "a portable POSIX C standard library written in Rust... currently supports Redox and Linux". — [redox-os/relibc](https://github.com/redox-os/relibc)
  - It selects the platform at compile time (`src/platform/redox`, `src/platform/linux`). On Linux it is "a thin wrapper on system calls". On Redox, more logic lives in userspace; for example, `fork` is built from file operations on a `thisproc` scheme. — [Redox book: Libraries and APIs](https://doc.redox-os.org/book/libraries-apis.html); [Redox porting strategy](https://redox-os.org/news/porting-strategy)
- **relibc activity (Aug 2026):**
  - Safety philosophy documented: unsafe code restricted to leaf functions.
  - Socket and `getaddrinfo` unsafe code moved into leaf functions.
  - `SCM_RIGHTS` fix in `recvmsg`.
  - `rlct_clone` for **Linux ARM64**; TLS load-offset fix on ARM64; 64 KiB-paged ELF loading on Linux ARM64.
  - `tzset`/`mktime`, `tcgetsid`, `MSG_NOSIGNAL`, and an epoll optimisation.
  - Redox AArch64 multi-core support landed.
  - Source: [This Month in Redox, August 2026](https://www.redox-os.org/news/this-month-260831/)
- **Known relibc gaps (Sept 2025):** controlling terminal, timer/alarm functions, timerfds. Languages with their own runtimes need "some fairly complex hooks in relibc". — [Redox priorities 2025/26](https://www.redox-os.org/news/development-priorities-2025-09/)
- **Other relibc users:** a DragonOS-hosted fork of relibc exists, but how far that port went is unknown. — [DragonOS relibc mirror](https://git.mirrors.dragonos.org.cn/DragonOS-Community/relibc/src/9e8cd0bf0736251614a23275048762877c3ad471?lang=sr-SP)
- **Rust std for a new OS, Motor OS:** `x86_64-unknown-motor` was added as a no-std Tier-3 target. PR #147765 (Oct 2025) adds std, where "all it does is proxy std pal calls to moto-rt". — [rust-lang/rust PR #147765](https://github.com/rust-lang/rust/pull/147765)
- **Rust std for a new OS, Xous:** Xous's maintainer described adding libstd support for a new OS on the stable channel using an out-of-tree target file, without rebuilding the whole ecosystem (2021). Xous also has an upstream Tier-3 RISC-V target. — [xobs: Porting Rust std to a new OS](https://xobs.io/porting-rust-standard-library-to-a-new-operating-system/); [Rust platform-support (mirror)](https://github.com/Sword-Destiny/rust/blob/72dd53c8e52e3f6995cfa29a66dcde28a691beb8/src/doc/rustc/src/platform-support.md)
- **Tier-3 policy:** targets "should attempt to implement as much of the standard libraries as possible, but may leave some code unimplemented". They are not built or tested by the Rust project. — [Rust platform-support (mirror)](https://github.com/Sword-Destiny/rust/blob/72dd53c8e52e3f6995cfa29a66dcde28a691beb8/src/doc/rustc/src/platform-support.md)
- **WASI 0.3:**
  - 0.3.0 released 2026-06-11; 0.3.1 on 2026-08-11 (adds `map<K,V>`); patch releases every two months.
  - Native async via component-model `async func`, `stream<T>`, `future<T>`. `wasi:io` removed. `wasi:sockets` consolidated from 7 interfaces to 2. `wasi:http` gains `service`/`middleware` worlds. Filesystem methods are async. `wall-clock` renamed `system-clock`.
  - Wasmtime 46+ enables WASI 0.3 and `component-model-async` by default (43–45 needed flags).
  - Source: [WASI.dev: WASI 0.3](https://wasi.dev/releases/wasi-p3); [WASI v0.3.0 release](https://github.com/WebAssembly/WASI/releases/tag/v0.3.0)
- **WASI-native OS precedent, Munal OS:** an experimental Rust OS, built as a single EFI binary, that sandboxes apps in WASM (embedded **wasmi**) *instead of* virtual-memory isolation. It uses cooperative scheduling and polling drivers, runs on VirtIO only, and has not run on real hardware. No WASI claim was found. — [askannz/munal-os](https://github.com/askannz/munal-os); [OSNews](https://www.osnews.com/story/142549/munal-os-experimental-operating-system-fully-written-in-rust-as-an-efi-binary/)

### Inferences
- **A layered ABI strategy fits a research OS:**
  1. A native Rust syscall/IPC API wrapped in an `os-rt` crate.
  2. A Tier-3 `aarch64-unknown-<os>` Rust target whose std PAL calls `os-rt`, as Motor OS does. Start with an out-of-tree target JSON plus `-Zbuild-std`, as Xous did, and upstream later.
  3. relibc with a new `platform/<os>` module for C/POSIX software. The Linux module is the template because it is a thin syscall wrapper; the Redox module shows how to emulate POSIX on a non-Unix kernel.
  4. WASI 0.3 via Wasmtime as the sandboxed, portable app ABI.
  - musl and picolibc are fallback options. They are C, and musl assumes Linux syscall numbers, so porting means rewriting its syscall layer. relibc's Rust-native design fits the "Rust-first" goal better.
- **Estimated effort (mine):**
  - Rust std PAL for a new OS, enough for threads, fs, net, time and env so cargo crates work: 1–3 PM.
  - relibc new-platform module enough to build bash/coreutils-class software: 2–4 PM. Signals, ptys/controlling terminals, `fork` and dynamic linking are the long tail; Redox has taken years and still lists gaps.
- **WASI 0.3 as a "native" ABI is attractive** because its async streams and futures map well onto a message-passing/capability kernel, and the component model gives typed IPC.
  - Running Wasmtime (Cranelift JIT on aarch64) requires executable-memory mapping (W^X toggling), signals or exception handling for traps, and threads. It is simplest to run Wasmtime itself on the Rust std target.
  - Wasmtime's "custom platform" support (unverified, see Gaps) could avoid the full std port early on.
  - No source shows a general-purpose OS using WASI as its primary ABI, so this would be novel research.
- **Arm MTE3 on the O6** could be exposed through relibc's allocator (tagged heap) as a research feature for C code. Not sourced; see Gaps.

### Gaps
- Not verified this session: Wasmtime's `no_std`/custom-platform embedding support in 2026, WAMR's aarch64 AOT status, and whether a `wasm32-wasip3` Rust target exists (the WASI page mentions only the `wasip3` crate and wit-bindgen).
- Whether Motor OS's std PR merged into a stable Rust release is unknown.
- Whether Xous std is fully upstream: not confirmed.
- No sources were checked on Hermit's or Fuchsia's std ports (both exist upstream: `aarch64-unknown-hermit`, `aarch64-unknown-fuchsia`; unverified this session).
- Any non-Redox kernel that has adopted relibc in production: none found.
- MTE support in relibc or Rust allocators: not researched.

---

## 4. Networking: smoltcp vs porting a full TCP/IP stack (lwIP, Asterinas, Fuchsia's netstack3) for 2x 5GbE, plus TLS

### Takeaway
smoltcp is the easiest Rust stack to embed and is used by Rust OSes such as Asterinas. It lacks congestion control and other "widely deployed features", and its published benchmarks are 2017-era loopback figures (2.2–4.5 Gbps). It is fine for bring-up but not sufficient for saturating 2x 5GbE over real networks. Fuchsia's netstack3 is the most complete production Rust stack, but it is tightly integrated with Fuchsia. A pragmatic plan: start with smoltcp, then either harden it (congestion control, offloads, multi-queue) or extract netstack3's core.

### Cited Findings
- **smoltcp:**
  - Event-driven and "designed for bare-metal, real-time systems". Needs no heap. Claims "~Gbps" against the Linux TCP stack in loopback. Missing "many widely deployed features, usually because no one implemented them yet". — [smoltcp docs](https://docs.rs/smoltcp/)
  - The README's example benchmark gives ~2.2 Gbps read and ~4.5 Gbps write on an i7-7500U with Linux 4.9 (2017), over a tap interface. — [smoltcp README (M-Labs mirror)](https://git.m-labs.hk/renet/renet/blame/branch/master/README.md)
- **Asterinas** (Rust framekernel, Linux-ABI-compatible, 210+ syscalls) uses smoltcp for TCP. "smoltcp... lacks congestion control", which partly explains why Asterinas beat Linux on its TCP benchmarks (paper, June 2025). — [Asterinas paper (arXiv 2506.03876)](https://arxiv.org/pdf/2506.03876)
- **Fuchsia netstack3:**
  - "a networking stack being developed from scratch in Rust". — [Contributing to Netstack3](https://fuchsia.dev/fuchsia-src/contribute/contributing-to-netstack/netstack3)
  - 2021 goal: functional parity with the Go-based Netstack2 (gVisor-derived), using Rust's type and memory safety. — [Netstack3 roadmap 2021](https://fuchsia.dev/docs/contribute/roadmap/2021/netstack3)
  - Migration was ongoing: a change "disable[d] abandoned migrations so that any issues with Netstack3 are more likely to surface before we delete Netstack2". — [Fuchsia integration roll](https://fuchsia.googlesource.com/integration/+/65d15b77d1c9dab2c307cb5952a7c9a420d30483); [fuchsia.net.stackmigrationdeprecated](https://fuchsia.dev/reference/fidl/fuchsia.net.stackmigrationdeprecated)
- **Redox:**
  - Plans to "implement ring buffers in our network stack as well, once the disk I/O work stabilizes" (Sept 2025). — [Redox priorities 2025/26](https://www.redox-os.org/news/development-priorities-2025-09/)
  - The "Redox Rings" ring-buffer API "significantly improve[d] I/O performance" for NVMe, RedoxFS and RAMFS (Aug 2026). — [This Month in Redox, August 2026](https://www.redox-os.org/news/this-month-260831/)
  - "Proper OpenSSH support" (Jan 2026). — [Phoronix: Redox January 2026](https://www.phoronix.com/news/Redox-OS-January-2026)

### Inferences
- **Line-rate arithmetic:** 5 Gb/s at a 1500-byte MTU (1538 bytes on the wire) is about 406 kpps per port, or about 813 kpps for both. With 8x Cortex-A720 this is reachable in Rust only with:
  - multi-queue NIC drivers with per-core queues and RSS
  - batching through io_uring-style submission/completion rings (like Redox Rings)
  - checksum, TSO and GRO offloads
  - zero-copy buffer ownership between driver, stack and app
  The stack's data structures matter less than this I/O architecture.
- **Recommendation:**
  - Phase 1: smoltcp over a ring-based NIC driver for bring-up. It is no_std and proven in Rust OSes.
  - Phase 2: add congestion control (CUBIC/BBR), SACK and window scaling as needed, and per-core sharded sockets. Alternatively, evaluate extracting `netstack3-core`, which is designed with bindings separated from core (inferred from the crate names `netstack3_core`/`netstack3_base`; unverified).
  - Porting lwIP gives more features than smoltcp but is C and single-threaded by design (unverified). It adds little over smoltcp for a Rust-first OS.
- **TLS:** use rustls with a pure-Rust or aws-lc-rs/ring crypto provider. ring and aws-lc have Armv8 crypto-extension assembly, so AES-GCM/SHA throughput should keep up with 5 GbE on A720 cores. Not sourced this session; see Gaps.
- **Estimated effort (mine):** smoltcp integration plus a socket API: 1–2 PM. A high-performance multi-queue path toward 5 GbE line rate: 4–8 PM, excluding the NIC driver.

### Gaps
- The Orion O6's 5GbE controller model (I believe Realtek RTL8126 on PCIe; unverified) and whether a Rust driver exists: not researched (kernel scope).
- No 2025–2026 smoltcp benchmarks on real NICs found.
- netstack3's current default status on Fuchsia devices and its portability outside Fuchsia: not confirmed.
- No sources fetched for rustls 2026 status, provider options or performance on Armv9.
- How Asterinas wraps smoltcp (the `aster-bigtcp` crate name I recall): not confirmed.

---

## 5. Audio: designs for a modern Rust audio server (PipeWire-like graph) and cpal backends

### Takeaway
No mature Rust PipeWire-equivalent server was found. The proven design to copy is PipeWire's: a graph of nodes, possibly in separate processes, exchanging buffers over shared memory via fd passing, with a realtime scheduler. On the client side, cpal is the de facto Rust audio API and has a `custom` feature for user-defined backends. A new OS can therefore add a cpal host for its own audio server without forking cpal.

### Cited Findings
- PipeWire is "a graph-based, low-latency engine" covering PulseAudio and JACK use cases. Nodes "can be implemented as separate processes, communicating with sockets and exchanging multimedia content using fd passing". — [Bootlin: An introduction to PipeWire](https://bootlin.com/blog/an-introduction-to-pipewire/)
- **cpal host backends:** ALSA by default on Linux and BSD, with optional JACK, PipeWire and PulseAudio. CoreAudio on macOS and iOS. WASAPI on Windows, with optional ASIO and JACK. AAudio on Android. Web Audio / Audio Worklet on wasm. A **`custom` feature enables user-defined backends** (see `examples/custom.rs`). — [RustAudio/cpal](https://github.com/RustAudio/cpal)
- A Mozilla cubeb maintainer: "if someone shows up and writes a high quality pipewire backend in rust, I don't see why we wouldn't review it carefully". New C/C++ backends will not be added. — [mozilla/cubeb #705](https://github.com/mozilla/cubeb/issues/705)
- The search found no Rust-written PipeWire alternative. — [AlternativeTo: PipeWire alternatives](https://alternativeto.net/software/pipewire)

### Inferences
- **Suggested design.** A Rust audio server process owns the I2S/HDA/USB-audio drivers. It exposes a graph of nodes and ports, using:
  - shared-memory ring buffers plus eventfd-like doorbells (the same "buffer + fence" kernel object as Section 1)
  - a quantum-based realtime scheduler thread pinned to an A720 core, with a priority class from the kernel
  - format conversion and resampling via pure-Rust DSP crates
  - Clients use a cpal `custom` host, and a WASI component interface could later expose audio streams to sandboxed apps.
- **Estimated effort (mine):** a minimal mixer server with a cpal backend over USB-audio or HDMI audio: 2–3 PM. A PipeWire-class graph with routing, policy and Bluetooth: 9–12+ PM.
- **Driver work.** The O6's on-board audio path (codec plus CIX audio DSP; the Sky1 firmware set includes DSP firmware per the GPU bring-up repo) will need its own driver work. USB audio class devices are the fastest route to sound. Inference; see Gaps.

### Gaps
- No Rust audio-graph server projects were found. Rust graph engines such as "dasp", "fundsp" or "knyst" exist (unverified, not researched).
- The O6's on-board audio codec and DSP and their documentation: not researched.
- cpal latest version and date: not stated on the repo page.

---

## 6. Filesystems and packaging: RedoxFS, Rust ext4/btrfs/FAT implementations, immutable/atomic images, package management

### Takeaway
Pure-Rust filesystem options in 2026 are uneven:
- ext4: mature read-only (`ext4-view`), plus early read/write crates (`ext4plus` beta, `am-fs-ext4` with JBD2 journaling, `efs` for ext2).
- RedoxFS: a native Rust option with recent performance work.
- btrfs: no Rust implementation found.

For a research OS, the most defensible design is an immutable, content-addressed read-only system image with a small writable data volume. Use FAT for the EFI system partition and read-only ext4 for exchanging data with Linux.

### Cited Findings
- `ext4-view` 0.9.3: read-only, `no_std` with `alloc`; write access is an explicit non-goal. — [docs.rs ext4-view](https://docs.rs/crate/ext4-view/latest)
- `ext4plus` (SeeleOS): a fork of ext4-view adding write and async support, `no_std` + alloc, pre-0.1.0 beta. Only found on unofficial mirrors. — [ext4plus mirror](https://verifico.duckdns.org/SeeleOS/ext4plus)
- `am-fs-ext4` 0.4.1: "pure-Rust ext2/3/4 read/write driver with journaling through JBD2", exposing a C ABI. no_std status unknown. — [docs.rs am-fs-ext4](https://docs.rs/crate/am-fs-ext4/0.4.1)
- `efs` 0.5.0: no_std Unix-FS library with ext2 read/write; "early development so bugs are expected". — [lib.rs efs](https://lib.rs/crates/efs)
- **RedoxFS (Aug 2026):**
  - Redox Rings significantly improved NVMe, RedoxFS and RAMFS I/O.
  - Off-by-one file-locking bugs fixed; this helped SQLite and libsoup.
  - `O_SYMLINK` added for cross-scheme symlinks.
  - Installer and package-extraction progress UI; `rldd` ported as `ldd`.
  - Source: [This Month in Redox, August 2026](https://www.redox-os.org/news/this-month-260831/)
- **Redox outlook (Sept 2025):** expects confidence "that the Redox file system can be used for persistent storage" after developer dogfooding. Plans a VirtioFS service and "tweak[s to] our package manager". — [Redox priorities 2025/26](https://www.redox-os.org/news/development-priorities-2025-09/)

### Inferences
- **Recommended layout for a research OS on the O6:**
  - ESP (FAT32, via a Rust FAT crate such as `fatfs`; unverified)
  - A/B **read-only system images**, content-addressed and verified with a Merkle tree (dm-verity/fs-verity style). Mount the active image read-only and switch atomically on update.
  - A writable `/data` volume using RedoxFS (adopt or fork) or a simple log-structured FS of your own.
  - Read-only ext4 via `ext4-view` to read existing Linux disks. The O6 already boots Linux from NVMe.
- **Packaging.** Content-addressed bundles (Nix/OSTree-like store paths, or Redox's pkgar format, which is unverified this session) fit an immutable image naturally. WASI components (Section 3) could be distributed as signed `.wasm` packages with capability manifests, Fuchsia-package style.
- **Estimated effort (mine):** read-only image FS plus verified boot chain: 2–3 PM. Production-quality writable FS with journaling or CoW: 6–12+ PM; reusing RedoxFS lowers this. Rust btrfs: not recommended (no base implementation found).

### Gaps
- Not verified: RedoxFS's features (CoW, encryption, checksums) and pkgar's design.
- No Rust btrfs, XFS or bcachefs implementation was found.
- `fatfs` crate status in 2026: not checked.
- No sources were gathered on immutable-image tooling (OSTree, systemd-sysupdate, Fuchsia blobfs) as references.

---

## 7. NPU userland (Zhouyi X2 / CIX NOE): open-source status, OS/libc requirements, ONNX Runtime/TFLite, open compilers (TVM/IREE/ExecuTorch), and CPU (SVE2/i8mm/bf16) or Vulkan-compute fallbacks

### Takeaway
The runtime path is surprisingly open; the compiler is the closed part.
- **Open:** the NPU kernel driver (GPL) and Arm China's user-mode driver `libaipudrv` (Apache-2.0, in `Compass_NPU_Driver`). Arm China also publishes the parser, optimizer, a TVM fork and integration scripts on GitHub.
- **Closed:** the final model compiler / code generator (Compass "AIPU builder"; `libaiputoolchain.so`) and CIX's `libnoe` runtime. The CIX NOE SDK compiler ships only as an x86_64 Linux Python 3.10 wheel.

A new OS can realistically run **precompiled `.cix` models**: port the ~4.1.0 `armchina_aipu.h` ioctl ABI to a Rust kernel driver, and either port `libaipudrv` (C, as FreeBSD did) or rewrite it in Rust. Model compilation would stay offline on an x86 Linux host. For on-device flexible inference, CPU paths (llama.cpp with KleidiAI i8mm/SVE kernels) are currently more reliable than PanVK Vulkan compute on this SoC.

### Cited Findings
- **CIX NOE SDK (Radxa docs, "25_Q3" release):**
  - Host compiler: `CixBuilder-6.1.3407.2-cp310-none-linux_x86_64.whl`, "only compatible with Python 3.10".
  - Device packages: `cix-noe-umd_2.0.2_arm64.deb` and `cix-npu-driver_2.0.1_arm64.deb`, plus apt packages `cix-drivers-dkms`, `cix-noe-umd`, `cix-npu-onnxruntime` and `task-cix-npu`.
  - The UMD is a shared library that coordinates with the NPU driver for allocation and task submission.
  - Source: [Radxa O6 NPU environment setup](https://docs.radxa.com/en/orion/o6/app-development/artificial-intelligence/env-setup)
- **FreeBSD port of the UMD (`aipu-umd`):**
  - Arm China's `libaipudrv` **4.1.0** from `Compass_NPU_Driver` (commit f35bac0), Apache-2.0 for `umd/`, ported to FreeBSD for the CIX Sky1 NPU (Orion O6, Orange Pi 6 Plus). Zhouyi **X2, architecture v3**.
  - Kernel side: CIX's 6.2.0 driver = Arm China 4.1.0 plus a Sky1 layer and two cache ioctls. It shares ioctl structs via `include/kmd/armchina_aipu.h`. Arm China 4.2.0+ changed `aipu_buf_desc`/`aipu_job_desc`, which breaks compatibility, so the port pins 4.1.0.
  - Models: compiled `.cix` files come from CIX's model hub on ModelScope. `libaipudrv` loads the embedded ELF graph, and CIX's `libnoe` reads `.cix` files directly.
  - ONNX Runtime: a `ZhouyiExecutionProvider` compiles models at session start with Arm China's Compass compiler, which "is binary-only" and requires int8 QDQ models.
  - Source: [JamesKane/aipu-umd](https://github.com/JamesKane/aipu-umd)
- **`Compass_NPU_Driver`:** KMD is "a standard Linux char driver" (`aipu.ko`). UMD is `libaipudrv.so`. An x86_64 simulator build exists. Architecture mapping: v1→z1, v2→z2/z3/x1, **v3→x2**, v3_2→x3p. The license was not shown in the captured page. — [Arm-China/Compass_NPU_Driver](https://github.com/Arm-China/Compass_NPU_Driver)
- **Arm China's open GitHub repos:** Compass_Unified_Parser, Compass_Optimizer, Compass_Integration, **Compass_Apache_TVM** (a TVM fork for model support and heterogeneous execution), Compass_NPU_Driver and Model_zoo. — [Zhouyi NPU tutorial: links](https://zhouyi-npu-tutorial.readthedocs.io/en/latest/_sources/9_link.md.txt)
- Arm China's 2024-03 RFC proposed upstreaming the Zhouyi driver into Linux's accel subsystem. At that time it had an "open-source kernel and user-space stack". — [Phoronix (2024-03-28)](https://www.phoronix.com/news/Arm-China-Zhouyi-NPU-Linux-RFC)
- Radxa announced open Zhouyi NPU documentation (2025). — [Radxa on X](https://x.com/RadxaComputer/status/1907376952689701101); [Zhouyi NPU Tutorials](https://zhouyi-npu-tutorial.readthedocs.io)
- **tinygrad `ZHOUYI` backend:**
  - Generates bindings from the KMD UAPI header `armchina_aipu.h` and issues **driver ioctls directly** (no libnoe). It still **requires the vendor's `libaiputoolchain.so`** compiler library.
  - Uses WBUF/SLOT ioctls for zero-copy weights; without them weights are copied every token (about 2–3 s/token).
  - On an O6N: Ternary Bonsai 2 27B at 2.0 tok/s (about 5.6 with speculative decoding), Qwen3.8-27B at about 0.7 tok/s, Ornith 9B at about 4.4 tok/s.
  - Decoding is "limited by the NPU's roughly 24 GB/s DDR port".
  - Source: [allbilly/tinygrad-npu-examples](https://github.com/allbilly/tinygrad-npu-examples)
- **CPU inference on CIX P1:**
  - KleidiAI-accelerated llama.cpp, Qwen2-1.5B Q4_0: prompt processing about 186 t/s vs 36 without KleidiAI; token generation about 33 vs 26 t/s. Community post, Dec 2025. — [aijishu community post](https://community.aijishu.com/a/1060000000520901)
  - Radxa's docs pin llama.cpp commit 4aced7a and build with Armv9 + i8mm/dotprod flags + KleidiAI (SME/I8MM/dotprod matmul kernels). — [Radxa O6 llama.cpp docs](https://docs.radxa.com/en/orion/o6/app-development/artificial-intelligence/llama-cpp)
  - A Radxa forum user saw no KleidiAI gain on Qwen3-30B-A3B. The default build lacked NEON kernels because of a CPU-feature-detection problem, and 7 threads was the best setting. — [Radxa forum: llama.cpp benchmarks](https://forum.radxa.com/t/llama-cpp-benchmarks/27813)
- **Vulkan compute for ML on this GPU:**
  - The same forum thread reports Vulkan "far slower than CPU": Qwen3-32B at about 1.1 t/s generation on Vulkan vs about 2 t/s on CPU. — [Radxa forum: llama.cpp benchmarks](https://forum.radxa.com/t/llama-cpp-benchmarks/27813)
  - A same-SoC report says Vulkan compute is "unstable for large workloads (OOM / DeviceLost)" (Feb 2026). — [visorcraft/orange-pi-6-plus-gpu](https://github.com/visorcraft/orange-pi-6-plus-gpu)
  - A review of the O6N says open-source Vulkan compute works for AI workloads. — [Boiling Steam O6N review](https://boilingsteam.com/radxa-orion-o6n-review/)

### Inferences
- **Feasible NPU path on a new OS:**
  1. Implement the Zhouyi KMD in Rust, exposing the **4.1.0 `armchina_aipu.h` ioctl semantics**: buffer alloc/free, job submit/poll, plus the CIX cache ioctls and the WBUF/SLOT zero-copy ioctls tinygrad uses. The GPL Linux driver can serve as a spec reference, but deriving code from it has licensing implications for a non-GPL OS. A clean-room Rust driver informed by docs is safer.
  2. Port `libaipudrv` 4.1.0 (Apache-2.0 C++) against relibc, or rewrite its loader and submit path in Rust. FreeBSD did this natively with clang + gmake.
  3. Run precompiled `.cix` models from the ModelScope hub, or compile them offline on x86_64 Linux with CixBuilder.
  - On-device compilation is blocked by the binary-only compiler (x86_64 Linux wheel, Python 3.10). Running it under a Linux-ABI compatibility layer on aarch64 is not possible, because the binary is x86_64.
- **Effort (mine):** Rust NPU KMD plus libaipudrv port running a `.cix` MobileNet: 2–4 PM, assuming documentation and the open UMD suffice. A Rust "tinygrad-style" direct runtime: an extra 2–3 PM.
- **Open compilers:** No upstream IREE, ExecuTorch or Apache TVM backend targeting Zhouyi was found; only Arm China's TVM fork. These remain viable for **CPU (SVE2/i8mm/bf16) and Vulkan (SPIR-V)** targets. IREE's Vulkan/SPIR-V and llvm-cpu backends would run on PanVK and A720 respectively, but neither was verified on this SoC.
- **Practical inference priority for a research OS:**
  1. CPU first. llama.cpp or ggml with KleidiAI, or the Rust crates Candle/Burn. These need only threads, mmap and a std target.
  2. NPU second, for int8 vision and speech models where `.cix` files exist. LLM decode on the NPU is DDR-port-bound at about 24 GB/s per the tinygrad data.
  3. Vulkan compute last, until PanVK compute stability on CSF improves.

### Gaps
- Exact license of the full Compass_NPU_Driver repo, and which Compass components (parser/optimizer open; "gbuilder"/code-generator binary) are closed: partly inferred, not confirmed from the repos.
- Whether CIX's `libnoe` and `cix-npu-onnxruntime` are redistributable, and which glibc or Debian versions they target: not stated in the Radxa docs.
- TFLite delegate support for Zhouyi: not found.
- Candle and Burn SVE2/i8mm/bf16 kernel support on aarch64 in 2026: not researched. Whether KleidiAI's SME path applies is doubtful, since the A720/A520 lack SME (unverified).
- Whether Arm China's Linux accel upstreaming landed by 2026: not found.
- NPU hardware documentation sufficient for a clean-room driver (register-level): unknown beyond the "open documentation" announcement.

---

## 8. Self-hosting and dev tooling: how Redox got rustc/cargo running natively, and what is needed to port git, ssh, a shell (nushell, ion) and editors

### Takeaway
Redox reached native rustc + cargo in January 2026, on its third attempt, after fixing kernel, signal and networking issues. It made its first merge request from inside Redox, using COSMIC Edit under QEMU. Full self-hosting is still limited: zero-dependency crates, and dynamic-linking work in relibc's `ld.so`. The lesson: self-hosting rustc is gated by libc completeness (signals, threads, mmap, dynamic loading, process spawning), not by Rust itself. Tooling like OpenSSH and editors arrive once relibc/POSIX coverage is good.

### Cited Findings
- **January 2026:** Cargo and rustc began working on Redox, enabling compilation of Rust CLI/TUI programs. "It took three attempts" and required fixing kernel, signals and networking issues. Anhad Singh submitted the first MR from within Redox (QEMU, COSMIC Edit), and it fixed relibc. Same month: "proper OpenSSH support", capability-based security infrastructure, and USB input latency cut from about 100 ms to 30 ms. Published 2026-02-09. — [Phoronix: Redox January 2026](https://www.phoronix.com/news/Redox-OS-January-2026); [heise](https://heise.de/-11173992)
- **Sept 2025, before the milestone:** "we have not had Cargo and the Rust compiler working consistently on Redox". Moving the upstream Rust compiler was "mostly solved". A self-hosted debugger was a secondary 2026 goal. — [Redox priorities 2025/26](https://www.redox-os.org/news/development-priorities-2025-09/)
- **Self-hosting tracking issue (undated):** rustc can build hello world, and cargo builds zero-dependency crates. — [Redox issue #1390](https://gitlab.redox-os.org/redox-os/redox/-/issues/1390)
- **Earlier history:** a 2023 GSoC report had rustc working only via direct invocation "until Cargo is ported". — [Redox GSoC self-hosting report 1](https://redox-os.org/news/gsoc-self-hosting-1/)
- **Static vs dynamic linking (undated):** a Chinese-language report says rustc could no longer run with static linking alone, so the focus shifted to relibc's dynamic linker (`ld_so`). The date is unclear. — [iThome](https://www.ithome.com.tw/news/134546)
- **Aug 2026:** a TLS-offset fix on ARM64 resolved a `tokio` panic in the package manager; `rldd` was ported as `ldd`. — [This Month in Redox, August 2026](https://www.redox-os.org/news/this-month-260831/)

### Inferences
- **Self-hosting prerequisites checklist,** derived from Redox's blockers:
  - a working dynamic linker or a fully static LLVM toolchain
  - robust `posix_spawn`/`fork`+`exec`
  - signals, `pthread` and TLS
  - large `mmap` and `mprotect`; rustc/LLVM need several GB, and 16 GB on the O6 is enough
  - file locking (cargo uses it)
  - a networking stack plus TLS, so cargo can fetch crates; or vendored registries offline
- **Ordering for a new OS:**
  1. Cross-compile everything from Linux on the O6 itself (aarch64 host → aarch64 target, fast).
  2. Bring up a Rust std target plus relibc.
  3. Port Rust-native tools first. Their only dependency is std:
     - shells: nushell or ion (Redox's shell)
     - editors: helix, or COSMIC Edit once a GUI exists
     - git: gitoxide (`gix`), as a Rust alternative to C git
     - ssh: russh, as a Rust alternative to OpenSSH
     - plus ripgrep, coreutils (uutils)
  4. Then C tools (git, OpenSSH, LLVM/rustc) via relibc.
  5. Last, native rustc. Expect 6–12 PM after the libc is solid, based on Redox's multi-year path.
- **WASI shortcut.** Many of these tools also compile to `wasm32-wasip2`/WASI and could run under Wasmtime before native POSIX is complete. This is useful for early interactivity. Inference; Wasmtime itself needs the std target first.
- **Hardware advantage.** The O6's 12 Armv9 cores and 16 GB LPDDR5 are enough for native rustc builds once the OS can run it.

### Gaps
- Redox's exact rustc porting technique (static LLVM vs `ld.so`, patches carried) in 2026: not documented in the sources found.
- Whether Redox's self-hosting works on AArch64 (vs x86_64 only): not stated.
- nushell, helix, gitoxide and russh portability to non-Unix `target_os` values: not researched this session. Redox's ports of COSMIC apps and OpenSSH suggest the libc route works.
- Debugger (gdb/lldb) porting requirements (ptrace-equivalent): not researched.
