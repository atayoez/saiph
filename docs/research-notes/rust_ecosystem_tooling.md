# Rust ecosystem, toolchain, reusable code and dev/test infrastructure for a from-scratch aarch64 OS on the Radxa Orion O6 (CIX P1 / CD8180 "Sky1"), as of 2026-10-25

Research date: 2026-10-25. "Local probe" means a command run during this research on the user's own Orion O6 (openSUSE Tumbleweed, kernel 7.2.11-1-default, QEMU 11.1.2, rustc stable 1.99.1 / nightly 1.101.0-nightly 2026-10-19). Those are first-hand observations, not web sources. Crate versions, licenses and dates come from the crates.io API, fetched 2026-10-25. Repo licenses and last-push dates come from the GitHub API, fetched the same day.

---

## (a) Rust toolchain features for aarch64 kernel work (status October 2026)

### Takeaway
As of Rust 1.99.1, stable Rust covers most of what a kernel needs: `aarch64-unknown-none(-softfloat)` at Tier 2, stable `asm!`/`global_asm!`, asm goto, `#[unsafe(naked)]`, edition 2024, and the `sve`/`sve2`/`mte`/`paca`/`pacg`/`bti` target features. A pinned nightly is still needed for:
- `build-std`
- custom JSON targets (destabilized in 1.95)
- `-Zbranch-protection` (PAC/BTI codegen)
- SVE and MTE intrinsics
- `derive(CoercePointee)`, arbitrary self types and `allocator_api`
- most sanitizers (KASAN/KHWASAN/KCFI)

### Cited Findings

**Release cadence and current versions**
- Rust releases in the window: 1.85.0 (2025-02-20) through 1.99.0 (2026-10-01) and 1.99.1 (2026-10-15). 1.85 made edition 2024 stable. — [rust-lang/rust RELEASES.md](https://github.com/rust-lang/rust/blob/master/RELEASES.md)
- The user's O6 has stable 1.99.1 (2026-10-13 build), nightly 1.101.0-nightly (2026-10-19), and pinned 1.85 and 1.96 toolchains. — Local probe (`rustc --version`, `rustup toolchain list`)
- Since 1.86, rustc on AArch64 Linux is built with ThinLTO + PGO, "up to 30% faster". This matters for native builds on the O6. — [RELEASES.md 1.86.0](https://github.com/rust-lang/rust/blob/master/RELEASES.md)

**Targets**
- `aarch64-unknown-none` ("Bare ARM64, hardfloat") and `aarch64-unknown-none-softfloat` are Tier 2 and ship `core` + `alloc` (no `std`). The two differ in whether the FPU is used for passing arguments. — [rustc book: aarch64-unknown-none](https://www.rustmax.net/library/rustc-book/platform-support/aarch64-unknown-none); [rustup component history (softfloat)](https://rust-lang.github.io/rustup-components-history/aarch64-unknown-none-softfloat.html)
- Built-in spec for `aarch64-unknown-none` on nightly 1.101:
  - `features: +v8a,+strict-align,+neon`
  - `max-atomic-width: 128`
  - `relocation-model: static`
  - `disable-redzone: true`
  - `panic-strategy: abort`
  - `linker: rust-lld`
  - inline stack probes
  - pre-link arg `--fix-cortex-a53-843419`
  - `supported-sanitizers: [kcfi, kernel-address, kernel-hwaddress]`

  — Local probe (`rustc +nightly --print target-spec-json -Zunstable-options --target aarch64-unknown-none`)
- The installed nightly also lists `aarch64-unknown-uefi` and `aarch64-unknown-linux-pauthtest`. `aarch64-unknown-uefi` has been Tier 2 since 1.67. `aarch64-unknown-linux-pauthtest` was added as Tier 3 in 1.98.0. — Local probe; [RELEASES.md 1.67.0 / 1.98.0](https://github.com/rust-lang/rust/blob/master/RELEASES.md)
- 1.89.0: enabling `neon` on `aarch64-unknown-none-softfloat` now warns, because LLVM does not properly support mixing code with and without that feature. 1.96.0: JSON targets for aarch64 softfloat must set `rustc_abi: "softfloat"`. — [RELEASES.md](https://github.com/rust-lang/rust/blob/master/RELEASES.md)

**Custom targets and build-std**
- **1.95.0 compatibility break:** JSON target specs are now destabilized and need `-Z unstable-options`. Cargo gained `-Z json-target-spec`. The stated reason is to keep JSON specs unstable even after `build-std` stabilizes. — [RELEASES.md 1.95.0](https://github.com/rust-lang/rust/blob/master/RELEASES.md)
- `build-std` is still unstable. The 2026 project goal is "Accepted", with Eric Huss as Cargo champion. The plan is to finish the design of the follow-up RFCs (#3874, #3875) and then implement. — [Rust goals 2026: build-std](https://rust-lang.github.io/goals/2026/build-std.html)
- Rust-for-Linux needs `build-core` support from this goal (RFC 3873). — [Rust goals 2026: Rust-for-Linux compiler features](https://goals.rust-lang.org/2026/rust-for-linux-compiler-features.html)

**Inline asm, naked functions and related language features**
- 1.87.0: `asm_goto` stabilized.
- 1.88.0: `naked_functions` stabilized (written `#[unsafe(naked)]`), plus `let_chains` (edition 2024) and `cfg_boolean_literals`.
- 1.93.0: `asm_cfg` (`#[cfg]` on asm lines) stabilized.
- 1.99.0: C-variadic function definitions stabilized, including naked C-variadic functions. 1.99.0 also makes `global_asm!` account for globally enabled target features.

  — [RELEASES.md](https://github.com/rust-lang/rust/blob/master/RELEASES.md); release coverage: [LinuxToday on 1.99](https://www.linuxtoday.com/blog/rust-1-99-released-with-c-variadic-functions-new-stable-apis/)
- Other language changes relevant to kernels:
  - 1.82.0: `&raw const` / `&raw mut`, `unsafe extern` blocks, unsafe attributes (`#[unsafe(no_mangle)]`), nested `offset_of!`.
  - 1.85.0: async closures.
  - 1.86.0: safe functions may carry `#[target_feature]`; trait-object upcasting.
  - 1.87.0: most `std::arch` intrinsics without pointer args are callable from safe code when the caller has the features enabled.
  - 1.89.0: `repr128`.
  - 1.90.0: **"Allow volatile access to non-Rust memory, including address 0"**, which matters for MMIO.
  - 1.92.0: `&raw` on union fields allowed in safe code.
  - 1.95.0: `if let` match guards.
  - 1.97.0: `cfg(target_has_atomic_primitive_alignment)`.
  - 1.99.0: `UnsafeCell` contents are guaranteed accessible without going through `get`.

  — [RELEASES.md](https://github.com/rust-lang/rust/blob/master/RELEASES.md)
- Compiler flags now stable: `-Cdwarf-version` (1.88.0) and `-Cjump-tables=bool` (1.93.0, formerly `-Zno-jump-tables`). 1.89.0 defaults to non-leaf frame pointers on aarch64-linux. — [RELEASES.md](https://github.com/rust-lang/rust/blob/master/RELEASES.md)

**Target features**
- Stable aarch64 `target_feature` names (Rust Reference):
  - Vector: `sve`, `sve2`, `sve2-aes`, `sve2-bitperm`, `sve2-sha3`, `sve2-sm4`, `f32mm`, `f64mm`.
  - Security and memory: `mte` (FEAT_MTE + MTE2), `paca`, `pacg`, `bti`, `pan`, `ssbs`, `dit`, `sb`.
  - Other: `rand`, `lse`, `rcpc`/`rcpc2`, `spe`, `pmuv3`, `tme`, `vh`, `i8mm`, `bf16`.

  `#[target_feature]` on aarch64 has been stable since 1.61. — [Rust Reference: codegen attributes, aarch64](https://doc.rust-lang.org/reference/attributes/codegen.html#aarch64); [RELEASES.md 1.61.0](https://github.com/rust-lang/rust/blob/master/RELEASES.md)
- Nightly 1.101's LLVM knows newer features that are **not** in the Reference's stable table: `sve2p1`, `sve2p2`, `gcs`, `pauth-lr`, `mops`, `cssc`, `hbc`, `lse2`, `wfxt`, `ecv`, `ls64`, and arch levels `v9a`, `v9.2a`, `v9.4a`, `v9.5a`, `v9.6a`. — Local probe (`rustc +nightly --print target-features --target aarch64-unknown-none-softfloat`), cross-checked against the [Rust Reference table](https://doc.rust-lang.org/reference/attributes/codegen.html#aarch64)

**SVE intrinsics (unstable)**
- `core::arch::aarch64` SVE/SVE2 intrinsics (e.g. `svldnf1_s64`) are nightly-only under `stdarch_aarch64_sve` (tracking issue #145052). NEON has been stable since 1.59. — [core::arch::aarch64::svldnf1_s64 docs](https://doc.rust-lang.org/core/arch/aarch64/fn.svldnf1_s64.html); [kernel.org rustdoc of core_arch](https://rust.docs.kernel.org/src/core/stdarch/crates/core_arch/src/aarch64/mod.rs.html)
- SVE types depend on RFC 3838 (`#[rustc_scalable_vector(N)]`). stdarch integration PR #149297 was opened in November 2025. Scalable vectors are a Rust project goal. — [rust-lang/rust PR #149297](https://github.com/rust-lang/rust/pull/149297); [Goal 2025H2: scalable vectors](https://rust-lang.github.io/goals/2025h2/scalable-vectors.html)
- New stable NEON intrinsics in the window: AArch64 NEON fp16 (1.94.0), `__jcvt` (1.95.0), `vdot*` (1.98.0). — [RELEASES.md](https://github.com/rust-lang/rust/blob/master/RELEASES.md)

**MTE (intrinsics unstable, codegen partial)**
- `core::arch::aarch64::mte` wraps `irg`/`gmi`/`ldg`/`stg`/`addg`/`subp` as ACLE names: `__arm_mte_create_random_tag`, `__arm_mte_exclude_tag`, `__arm_mte_get_tag`, `__arm_mte_increment_tag`, `__arm_mte_ptrdiff`, `__arm_mte_set_tag`.
  - Gating: unstable `stdarch_aarch64_mte` (tracking #129010).
  - `set_tag` needs 16-byte alignment and tags a whole granule.

  — [core_arch mte.rs source (nightly)](https://doc.rust-lang.org/nightly/src/core/stdarch/crates/core_arch/src/aarch64/mte.rs.html)
- On `aarch64-unknown-none`, nightly rejects `-Zsanitizer=memtag`, `hwaddress` and `shadow-call-stack` ("not supported for this target"). `kernel-address`, `kernel-hwaddress` and `kcfi` are accepted, but only with a rebuilt `core`, because they are target modifiers. — Local probe

**Branch protection (PAC/BTI)**
- `-Zbranch-protection` (PAC-ret/BTI) is still unstable. Tracking issue #113369 is open. Its future work includes a prebuilt std compiled with mitigations. — [rust-lang/rust #113369](https://github.com/rust-lang/rust/issues/113369)
- `-Zbranch-protection` is now a **target modifier**: the whole crate graph must use the same value. — [This Month in Rust OSDev, March 2026](https://rust-osdev.com/this-month/2026-03/)
- Local check:
  - Stable 1.99.1 rejects `-Cbranch-protection` ("unknown codegen option").
  - Nightly accepts `-Zbranch-protection=bti,pac-ret,gcs` and errors "mixing `-Zbranch-protection` will cause an ABI mismatch" against the prebuilt core.
  - So it needs `-Zbuild-std`.

  — Local probe
- Rust-for-Linux's 2026–2027 goal lists unstable flags the kernel uses on arm64:
  - `-Zbranch-protection`, `-Zfixed-x18`
  - `-Zsanitizer=kernel-address`, `-Zsanitizer=shadow-call-stack`, `-Zsanitizer=kcfi`
  - `-Zfunction-sections`, `-Zdebuginfo-compression`
  - future needs: `-Zsanitizer=kernel-hwaddress`, `-Zmin-function-alignment`, `-Zrandomize-layout`

  Compiler-team support is "Medium" and there are no task owners. — [Rust goals 2026: RfL compiler features](https://goals.rust-lang.org/2026/rust-for-linux-compiler-features.html)
- A 2026 project goal proposes PAC support on a dedicated Linux target (`pauthtest`). It notes that Rust code is incompatible with PAC-ABI programs today. — [rust-lang/goals 2026: aarch64 pointer authentication pauthtest](https://github.com/rust-lang/goals/blob/main/src/2026/aarch64_pointer_authentication_pauthtest.md)

**Rust-for-Linux language features**
- `derive(CoercePointee)` is still nightly-only. Its stabilization is blocked on `arbitrary_self_types`. The 2026 goal plan is to decouple `Deref`/`Receiver`, run a gated experiment, then attempt stabilization a second time. — [Rust goals 2026: arbitrary self types](https://goals.rust-lang.org/2026/arbitrary-self-types.html); [CoercePointee nightly docs](https://doc.rust-lang.org/nightly/core/marker/derive.CoercePointee.html)
- Neither `allocator_api`, `arbitrary_self_types` nor `CoercePointee` appears as stabilized anywhere in RELEASES.md through 1.99.1. — [RELEASES.md](https://github.com/rust-lang/rust/blob/master/RELEASES.md) (grep, 2026-10-25)

**Async in `no_std`**
- `embassy-executor` 0.10.0 (2026-03-20, MIT OR Apache-2.0) has a `platform-cortex-ar` feature (Cortex-A/R) besides `platform-cortex-m`, `platform-riscv32`, `platform-spin` and `platform-std`. So an async executor for bare-metal Cortex-A exists off the shelf. — [crates.io: embassy-executor 0.10.0](https://crates.io/crates/embassy-executor)

### Inferences
- **Recommended baseline:**
  - Pinned nightly in `rust-toolchain.toml`.
  - `aarch64-unknown-none-softfloat` for the kernel (avoids saving FP/SIMD state on every exception entry), with `-Ctarget-feature=+v9.2a`-equivalent features enabled per module.
  - Avoid custom JSON targets; post-1.95 they are an extra unstable surface.
  - `-Zbuild-std=core,alloc` if PAC/BTI or sanitizers are wanted across the whole image, since the prebuilt `core` cannot be mixed with them.
- **MTE on stable:** MTE instructions (`irg`, `stg`, `st2g`, `ldg`) can be used today via stable `asm!` with the stable `mte` target feature. Only the convenient intrinsics are nightly. SVE2 in kernel code (e.g. memcpy/crypto) is similar: `asm!` or `#[target_feature(enable="sve2")]` with inline asm works on stable, while the SVE intrinsic types are nightly-only.
- **Hardening has no stable path:** compiler-driven PAC/BTI/SCS for a kernel only exists on nightly. A "future-proof" design can still hand-sign return addresses in its own asm entry/exit stubs (`paciasp`/`autiasp`, `bti c` landing pads) on stable. It should keep the build ready to flip on `-Zbranch-protection` once #113369 lands.

### Gaps
- No stabilization date found for `-Zbranch-protection`, `build-std`, SVE intrinsics (RFC 3838), MTE intrinsics, `arbitrary_self_types` or `allocator_api`. All are open or tracking as of Oct 2026.
- Not verified: whether `-Zfixed-x18` and SCS work with `aarch64-unknown-none` when `core` is rebuilt (Rust-for-Linux may use its own target spec).
- No authoritative 2026 doc found on Rust's own guidance for MMIO beyond the 1.90 volatile-to-non-Rust-memory change. The crate-level patterns are covered in (b).

---

## (b) Reusable crates: maturity and license (crates.io data as of 2026-10-25)

### Takeaway
A permissively licensed (MIT/Apache-2.0) base layer for an Armv9 kernel now exists and is actively maintained. It comes mainly from Google (AVF team) and the TrustedFirmware "arm-firmware-crates" group:
- page tables, GICv2/3/4, PL011, generic timer
- PSCI, SMCCC, FF-A, SCMI
- system registers, MMIO wrappers, exception vectors

rust-osdev supplies ACPI+AML, UEFI, PCI types and virtio-spec. The rcore-os "tgoskits" monorepo has young but directly relevant drivers: RTL8125 NIC, SMMUv3, xHCI host, NVMe, AHCI, SCMI, GIC, PCIe. Watch for MPL-2.0 (fdt, ostd, bare-test) and GPL (lwext4_rust) components.

### Cited Findings

**Arm architecture and firmware-interface crates** (versions, licenses and dates from crates.io)

| Crate | Latest (date) | License | Source / notes |
|---|---|---|---|
| aarch64-paging | 0.12.3 (2026-10-22) | MIT OR Apache-2.0 | Google; "manipulate AArch64 VMSA page tables"; features `alloc`. [crates.io](https://crates.io/crates/aarch64-paging), [GitHub](https://github.com/google/aarch64-paging) |
| arm-gic | 0.10.0 (2026-09-25) | MIT OR Apache-2.0 | "driver for the Arm GIC version 2, 3 or 4"; features `el2`, `el3`, `fakes`; now hosted at git.trustedfirmware.org/arm-firmware-crates. [crates.io](https://crates.io/crates/arm-gic) |
| arm-pl011-uart | 0.5.1 (2026-08-13) | MIT OR Apache-2.0 | TrustedFirmware arm-firmware-crates. [crates.io](https://crates.io/crates/arm-pl011-uart) |
| arm-generic-timer | 0.3.1 (2026-09-25) | MIT OR Apache-2.0 | TrustedFirmware. [crates.io](https://crates.io/crates/arm-generic-timer) |
| arm-psci | 0.2.0 (2025-10-29) | MIT OR Apache-2.0 | TrustedFirmware. [crates.io](https://crates.io/crates/arm-psci) |
| smccc | 0.2.3 (2026-05-28) | MIT OR Apache-2.0 | Google; "SMCCC 1.4 and PSCI 1.1 on aarch32 and aarch64". [crates.io](https://crates.io/crates/smccc) |
| arm-ffa | 0.5.0 (2026-04-02) | MIT OR Apache-2.0 | FF-A support library, so the OS can talk to OP-TEE/SPMC via FF-A. [crates.io](https://crates.io/crates/arm-ffa) |
| arm-scmi | 0.2.0 (2026-09-22) | MIT OR Apache-2.0 | "Arm SCMI driver"; very new (62 total downloads). [crates.io](https://crates.io/crates/arm-scmi) |
| arm-sysregs | 0.5.1 (2026-09-16) | MIT OR Apache-2.0 | "Armv8-A system register helper". [crates.io](https://crates.io/crates/arm-sysregs) |
| aarch64-cpu | 11.2.0 (2025-11-29) | MIT/Apache-2.0 | rust-embedded; ~1.08M downloads. [crates.io](https://crates.io/crates/aarch64-cpu) |
| aarch64-rt | 0.4.3 (2026-02-23) | MIT OR Apache-2.0 | Google; "startup code and exception vector for aarch64 Cortex-A". [crates.io](https://crates.io/crates/aarch64-rt) |
| safe-mmio | 0.3.2 (2026-09-28) | MIT OR Apache-2.0 | Google; "safe MMIO device access, especially in systems with an MMU". [crates.io](https://crates.io/crates/safe-mmio) |
| tock-registers | 0.10.1 (2025-10-31) | MIT/Apache-2.0 | Tock register-interface DSL. [crates.io](https://crates.io/crates/tock-registers) |
| volatile-register | 0.2.2 (2023-10-20) | MIT OR Apache-2.0 | Older, no recent releases. [crates.io](https://crates.io/crates/volatile-register) |
| dtoolkit | 0.4.1 (2026-09-25) | Apache-2.0 OR MIT | Google; FDT parse and manipulate. [crates.io](https://crates.io/crates/dtoolkit) |

**Firmware tables, boot and buses**

| Crate | Latest (date) | License | Notes |
|---|---|---|---|
| acpi | 6.1.1 stable (2026-03-08); 7.0.0-pre1 (2026-10-23) | MIT/Apache-2.0 | rust-osdev; table parsing **plus AML interpreter** (feature `aml`). [crates.io](https://crates.io/crates/acpi), [GitHub](https://github.com/rust-osdev/acpi) |
| aml | 0.16.4 (2023-06-13) | MIT/Apache-2.0 | Old standalone AML crate. The AML interpreter now lives in `acpi` (feature `aml`). [crates.io](https://crates.io/crates/aml) |
| uefi / uefi-raw | 0.41.0 / 0.17.0 (2026-09-21) | MIT OR Apache-2.0 | rust-osdev uefi-rs; ~2.7M downloads. [crates.io](https://crates.io/crates/uefi) |
| fdt | 0.1.5 stable (2023-02-05); 0.2.0-alpha3 (2026-10-01) | **MPL-2.0** | [crates.io](https://crates.io/crates/fdt) |
| fdt-parser | 0.5.1 | MPL-2.0 | [crates.io](https://crates.io/crates/fdt-parser) |
| flat_device_tree | 3.1.1 (2024-06-07) | MPL-2.0 | [crates.io](https://crates.io/crates/flat_device_tree) |
| pci_types | 0.10.1 (2026-02-03) | MIT/Apache-2.0 | rust-osdev. [crates.io](https://crates.io/crates/pci_types) |
| virtio-drivers | 0.13.0 (2026-03-03) | MIT | rcore-os; guest drivers. [crates.io](https://crates.io/crates/virtio-drivers) |
| virtio-spec | 0.4.0 (2026-09-12) | MIT OR Apache-2.0 | rust-osdev (Hermit lineage). [crates.io](https://crates.io/crates/virtio-spec) |
| limine | 0.6.5 (2026-06-07) | MIT OR Apache-2.0 | Limine boot protocol bindings. The Limine bootloader is BSD-2-Clause. [crates.io](https://crates.io/crates/limine), [Limine GitHub](https://github.com/Limine-Bootloader/Limine) |
| xhci | 0.9.2 (2023-07-19) | MIT OR Apache-2.0 | rust-osdev register definitions; stale since 2023. [crates.io](https://crates.io/crates/xhci) |

- An Oct 2025 PR to `acpi` made it handle QEMU's aarch64 ACPI tables, tested inside Redox. — [rust-osdev/acpi PR #261](https://github.com/rust-osdev/acpi/pull/261)

**Allocators, sync and utility crates**

| Crate | Latest (date) | License |
|---|---|---|
| talc | 5.1.1 (2026-09-09) | MIT |
| buddy_system_allocator | 0.13.0 (2026-03-30) | MIT |
| linked_list_allocator | 0.10.6 (2026-04-14) | Apache-2.0/MIT |
| spin | 0.12.3 (2026-08-17) | MIT |
| zerocopy | 0.8.63 stable (2026-10-13); 0.9.0-alpha.0 | BSD-2-Clause OR Apache-2.0 OR MIT |
| bitflags | 2.13.2 (2026-09-10) | MIT OR Apache-2.0 |
| heapless | 0.9.3 | MIT OR Apache-2.0 |
| intrusive-collections | 0.10.3 | MIT OR Apache-2.0 |
| embedded-io | 0.7.1 | MIT OR Apache-2.0 |

— [crates.io API](https://crates.io/) (each crate page, e.g. [talc](https://crates.io/crates/talc), [zerocopy](https://crates.io/crates/zerocopy))

**Networking, graphics and filesystems**

| Crate | Latest (date) | License | Notes |
|---|---|---|---|
| smoltcp | 0.14.0 (2026-08-17) | **0BSD** | ~9.3M downloads. [crates.io](https://crates.io/crates/smoltcp) |
| embassy-net | 0.9.1 | MIT OR Apache-2.0 | Async stack on smoltcp. [crates.io](https://crates.io/crates/embassy-net) |
| embedded-graphics | 0.8.2 (2026-02-15) | MIT OR Apache-2.0 | [crates.io](https://crates.io/crates/embedded-graphics) |
| ext4-view | **1.0.0 (2026-09-14)** | MIT OR Apache-2.0 | no_std read-only ext2/3/4. [crates.io](https://crates.io/crates/ext4-view) |
| ext4_rs | 1.3.3 | MIT | Read/write ext4 in Rust. [crates.io](https://crates.io/crates/ext4_rs) |
| lwext4_rust | 0.2.0 | **GPL-2.0** | Wraps C lwext4. [crates.io](https://crates.io/crates/lwext4_rust) |
| fatfs | 0.3.6 (2023-01-17) | MIT | Stale. [crates.io](https://crates.io/crates/fatfs) |
| embedded-sdmmc | 0.10.0 | MIT OR Apache-2.0 | FAT on SD. [crates.io](https://crates.io/crates/embedded-sdmmc) |
| redoxfs | 0.9.1 (2026-07-01) | crates.io field "non-standard" | The Redox repo is MIT. [crates.io](https://crates.io/crates/redoxfs) |

**rcore-os "tgoskits" monorepo** (ArceOS/StarryOS/Axvisor; repo license Apache-2.0, last push 2026-10-23)
- Contents:
  - `drivers/net/realtek-rtl8125` 0.3.3: "Minimal Realtek RTL8125 PCIe Ethernet driver", **MIT**.
  - `drivers/iommu/arm-smmu-v3` 0.1.0: "Arm SMMUv3 Stage 1 translation driver".
  - `drivers/usb/usb-host` = crab-usb 0.12.1: "A usb host for embedded systems", Apache-2.0.
  - `drivers/blk/nvme-driver` 0.8.2: MIT.
  - `drivers/blk/ahci-driver` 0.2.2.
  - `drivers/firmware/arm-scmi-rs` 0.1.11: MIT, no_std SCMI.
  - `drivers/firmware/acpica-interpreter` 0.1.0: "Freestanding ACPICA interpreter with a caller-provided OS services backend", **Apache-2.0 AND BSD-3-Clause**, vendors ACPICA C.
  - `drivers/intc/arm-gic-driver` 0.18.1: MIT, has a GICv3 NMI example.
  - `drivers/pci/pcie` 0.7.1: MIT.
  - `platforms/somehal` 0.11.3: Apache-2.0 HAL.
  - SoC support for Rockchip, Phytium and others; nothing CIX-specific found.

  — [GitHub rcore-os/tgoskits](https://github.com/rcore-os/tgoskits) (tree and Cargo.toml files read via GitHub API); [crates.io: crab-usb](https://crates.io/crates/crab-usb), [nvme-driver](https://crates.io/crates/nvme-driver), [somehal](https://crates.io/crates/somehal)
- Other drivercraft/arceos crates:
  - `ixgbe-driver` 0.1.1 (Apache-2.0)
  - `arm_pl031` RTC (GPL-3.0-or-later OR Apache-2.0 OR MulanPSL-2.0)
  - `bare-test` 0.8.3 (MPL-2.0, "A test framework for bare metal")

  — [crates.io: ixgbe-driver](https://crates.io/crates/ixgbe-driver), [arm_pl031](https://crates.io/crates/arm_pl031), [bare-test](https://crates.io/crates/bare-test)

**OS frameworks and other projects published as crates**
- `ostd` 0.19.0 (2026-10-12, MPL-2.0): Asterinas's OS framework. — [crates.io: ostd](https://crates.io/crates/ostd)
- `hermit-kernel` 0.13.0 (2026-02-25, MIT OR Apache-2.0). — [crates.io: hermit-kernel](https://crates.io/crates/hermit-kernel)
- `sel4` 0.0.12 on crates.io dates from 2017 (Robigalia, stale). The live seL4 Rust support is the [seL4/rust-sel4](https://github.com/seL4/rust-sel4) GitHub repo, which the GitHub API reports with no detected license, last push 2026-10-01. — [crates.io: sel4](https://crates.io/crates/sel4)

**Redox userspace drivers** (in the `redox-os/base` repo, MIT License, "Copyright (c) 2017 Redox OS")
- Storage: `ahcid`, `nvmed`, `ided`, `usbscsid`, `virtio-blkd`, `bcm2835-sdhcid`
- Network: `e1000d`, `ixgbed`, `rtl8139d`, **`rtl8168d`**, `virtio-netd`
- USB: `xhcid`, `usbhubd`
- Graphics: `vesad`, `virtio-gpud`, `ihdgd`, `fbcond`
- System: `acpid`, `pcid`

— [GitLab redox-os/base drivers tree](https://gitlab.redox-os.org/redox-os/base/-/tree/main/drivers) (via GitLab API); [LICENSE](https://gitlab.redox-os.org/redox-os/base/-/raw/main/LICENSE)

### Inferences
- **MIT/Apache base stack for the O6:**
  - `aarch64-rt` (or hand-written boot asm) + `aarch64-paging` + `arm-gic` (GICv3 + ITS) + `arm-generic-timer` + `arm-pl011-uart`
  - `smccc`/`arm-psci` for CPU bring-up via PSCI (MADT gives MPIDRs)
  - `acpi` (with `aml`) for MADT/GTDT/IORT/MCFG/PPTT/PCCT/DBG2 parsing and DSDT evaluation
  - `uefi` for a UEFI stub loader
  - `safe-mmio` or `tock-registers` for registers
  - `talc`/`buddy_system_allocator`, `smoltcp`, `ext4-view`
- **Port candidates for the O6's devices:**
  - NIC: Redox `rtl8168d` and tgoskits `realtek-rtl8125` (both MIT) are the closest permissive bases for RTL8126. RTL8126 is in the r8169 family; see (d).
  - USB: tgoskits `crab-usb` or Redox `xhcid` for xHCI.
  - Storage: tgoskits `nvme-driver` or Redox `nvmed` for NVMe.
  - IOMMU: tgoskits `arm-smmu-v3` as a starting point for SMMUv3.
- **Two ACPI routes:**
  - Pure-Rust `acpi`+`aml` (permissive, less battle-tested on vendor DSDTs).
  - Vendored ACPICA (`acpica-interpreter`; Apache-2.0 AND BSD-3-Clause; the same engine NetBSD used on this board, see (f)).

  CIX's DSDT is large and vendor-specific (CIXH* devices, AML SCMI clock methods, PowerResources). ACPICA is the lower-risk route for real-board AML. The pure-Rust interpreter is the more "Rust-native" research route.
- **License watch:** MPL-2.0 (fdt, fdt-parser, ostd, bare-test, Hubris) is file-level copyleft and fine for a permissive OS if those files stay MPL. GPL items (lwext4_rust, m1n1 ≥2.0, Linux drivers) would force GPL on derived code.

### Gaps
- No Rust crate found for CIX-specific blocks: DPU/display, the CIX PCIe controller beyond ECAM, CIX GPT timer, Sky1 pinctrl/clock, CIX mailbox/SCMI transport, the 30 TOPS NPU.
- Not assessed: code quality, test coverage and real-hardware validation of the tgoskits drivers (SMMUv3, crab-usb, RTL8125). They are recent and low in downloads (e.g. nvme-driver ~5k).
- Not checked: whether `arm-gic` 0.10 supports GICv3 ITS/LPIs fully. Needed for MSI from PCIe NVMe/NICs.
- No current Rust Wi-Fi (rtw89-class) driver found.

---

## (c) Reference Rust kernels and drivers worth studying

### Takeaway
The best aarch64 references are:
- **Google AVF** (vmbase/pvmfw/crosvm) for clean, permissive bare-metal Rust on Armv8/9 under a hypervisor.
- **Redox** for a microkernel with userspace drivers and fresh aarch64 work (SMP in Aug 2026).
- **tgoskits/ArceOS/StarryOS** for a large catalogue of aarch64 drivers.
- **Asterinas** (0.19, Oct 2026) for framekernel and safe-Rust design plus Verus verification. AArch64 support there is only initial.
- **Rust-for-Linux drivers** (Tyr, Nova, rnull, Binder, Asahi AGX) for driver-abstraction design. These are GPL-2.0, except Tyr and Asahi AGX, which are dual GPL-2.0/MIT.
- **Hubris** for design ideas only (Cortex-M).
- **Theseus**: dormant since 2024.

### Cited Findings

**Google AVF and crosvm**
- vmbase is a Rust crate and static library for `no_std` binaries in aarch64 VMs under crosvm (pVM firmware, VM bootloader, kernel). It provides:
  - an entry point with MMU identity mapping, caches and stack
  - an exception vector
  - a UART `println!`
  - shutdown/reboot
  - optional heap, page tables and PSCI libraries

  — [AOSP vmbase README](https://android.googlesource.com/platform/packages/modules/Virtualization/+/f0271f36388ec9630d89ff8b3ee4cb22e2ca3eaf/libs/libvmbase/README.md)
- pvmfw is the first code in a protected VM; it verifies the payload and derives per-VM secrets. AVF is ARM64-only and uses pKVM. crosvm is a Rust VMM. — [AVF overview](https://source.android.com/docs/core/virtualization)
- crosvm: BSD-3-Clause, active (last push 2026-10-24). — [GitHub google/crosvm](https://github.com/google/crosvm)

**Hubris and Theseus**
- Hubris: "lightweight, memory-protected, message-passing kernel for deeply embedded systems", MPL-2.0, active (2026-10-24). It targets microcontrollers, not aarch64 A-profile. — [GitHub oxidecomputer/hubris](https://github.com/oxidecomputer/hubris)
- Theseus: MIT, last push 2024-09-22. Effectively dormant. — [GitHub theseus-os/Theseus](https://github.com/theseus-os/Theseus)

**Asahi**
- m1n1 was MIT until 1.9.9. From 2.0.0 it is GPL-2.0-or-later, and new code is GPL only. — [m1n1 README (License section)](https://github.com/AsahiLinux/m1n1)
- The Asahi AGX GPU driver files carry `SPDX-License-Identifier: GPL-2.0-only OR MIT`. — [AsahiLinux/linux drivers/gpu/drm/asahi/driver.rs](https://github.com/AsahiLinux/linux/blob/asahi/drivers/gpu/drm/asahi/driver.rs)

**Rust-for-Linux drivers in mainline** (mainline latest tag v7.3)
- Tyr (Rust Mali CSF driver):
  - Targets Mali-G610 (Valhall v10, CSF) as a Rust port of Panthor.
  - Timeline: stub in v6.18; Rust page-table abstraction v7.0; GEM shmem helpers v7.1; Rust GPUVM v7.2 (Aug 2026); MCU boot in v7.3.
  - About 3,700 lines with 6 unsafe lines.
  - Not yet a Panthor replacement; job scheduling is stalled on abstractions.

  — [Collabora, "Upstreaming Tyr", 2026-10-21](https://www.collabora.com/news-and-blog/blog/2026/10/21/upstreaming-tyr-rust-drm-and-the-linux-kernel/)
- Tyr's license is `GPL-2.0 or MIT`. — [torvalds/linux drivers/gpu/drm/tyr/tyr.rs](https://github.com/torvalds/linux/blob/master/drivers/gpu/drm/tyr/tyr.rs)
- The O6 GPU is an Arm Immortalis-G720 MC10, per CIX's own slides. — [CIX/Linaro LIS25-139 "ACPI support on Radxa Orion O6" slides](https://www.kitefor.events/download/410)
- Nova (Rust NVIDIA driver):
  - nova-core skeleton in 6.15; GSP boot for Ampere in 6.19; Turing prep in 7.0/7.1.
  - The 7.4 DRM-Next submission adds NovaCoreApi, VRAM buddy allocation, TLB flushing and MMU v2/v3 page tables.
  - "Not yet ready for end-user use" (March 2026).

  — [Phoronix: Nova Linux 7.4](https://phoronix.com/news/NVIDIA-Nova-Linux-7.4-Changes); [Phoronix: Rust DRM 7.1](https://www.phoronix.com/news/Rust-DRM-For-Linux-7.1)
- Mainline Rust driver licenses:
  - nova-core (`drivers/gpu/nova-core/nova_core.rs`): GPL-2.0
  - rnull (`drivers/block/rnull/rnull.rs`): GPL-2.0
  - Rust Binder (`drivers/android/binder/rust_binder_main.rs`): GPL-2.0
  - Rust PHY driver `ax88796b_rust.rs`: GPL-2.0
  - `rust/kernel/lib.rs`: GPL-2.0
  - No `.rs` files under `drivers/nvme/host`, so the Rust NVMe driver is not in mainline.

  — [torvalds/linux](https://github.com/torvalds/linux) (files read via GitHub API, 2026-10-25)

**Redox (aarch64 progress, 2026)**
- April 2026: ARM64 dynamic linking. — [OSNews Redox topic](https://www.osnews.com/topic/redox-os/); [This Month in Redox, April 2026](https://www.redox-os.org/news/this-month-260430/)
- July 2026: Amlogic Meson UART support, plus fixes for FP register corruption, PTE shareability, DeviceTree panics and mmap memory type. — [This Month in Redox, July 2026](https://www.redox-os.org/news/this-month-260731/)
- August 2026: aarch64 multi-core (SMP) "needs more testing", an io_uring-like API, NUMA work and ~5% IPC reduction. — [This Month in Redox, Aug 2026](https://www.redox-os.org/news/this-month-260831/); [Phoronix Aug 2026](https://www.phoronix.com/news/Redox-OS-August-2026)
- Earlier: Raspberry Pi 4 booted to login in Oct 2024 via DT. QEMU is Redox's main ARM test platform. — [Phoronix Oct 2024](https://phoronix.com/news/Redox-OS-For-October-2024); [rust-osdev/acpi PR #261](https://github.com/rust-osdev/acpi/pull/261)

**ArceOS / StarryOS / Axvisor (tgoskits)**
- `cargo xtask arceos qemu --arch aarch64`, `cargo xtask starry qemu --arch aarch64` and `cargo xtask axvisor qemu --arch aarch64` are documented paths. CI pins QEMU 11.1.1. — [tgoskits README](https://github.com/rcore-os/tgoskits)
- arceos-org/arceos: Apache-2.0, last push 2026-06-01, with development consolidated into tgoskits. Starry-OS/StarryOS: Apache-2.0. — [GitHub arceos](https://github.com/arceos-org/arceos), [StarryOS](https://github.com/Starry-OS/StarryOS)

**Asterinas**
- 0.19.0 (Oct 2026) added initial AArch64 support (with x86_64, RISC-V and LoongArch64), an initial DRM subsystem and X.Org. LWN calls the Arm support "minimal but usable". — [Phoronix: Asterinas 0.19](https://phoronix.com/news/Asterinas-0.19-Released); [LWN](https://lwn.net/Articles/1100055/)
- Asterinas repo: MPL-2.0, ~5k stars. — [GitHub asterinas/asterinas](https://github.com/asterinas/asterinas)
- vostd formally verifies parts of OSTD with Verus, focused on memory management and sync. It found real bugs (UB, deadlocks, overflows). The related CortenMM paper won SOSP 2025 Best Paper. — [GitHub asterinas/vostd](https://github.com/asterinas/vostd)

**Hermit and seL4**
- Hermit kernel: Apache-2.0 on GitHub (crate states MIT OR Apache-2.0), active (2026-10-15). — [GitHub hermit-os/kernel](https://github.com/hermit-os/kernel)
- seL4 Rust userspace support: [seL4/rust-sel4](https://github.com/seL4/rust-sel4), active 2026-10-01.

### Inferences
- **Boot/MMU/exception scaffolding:** vmbase and aarch64-rt are the closest "Google-quality" permissive templates.
- **Driver structure:** Redox's userspace daemons (`pcid` spawning `nvmed`/`xhcid`/`rtl8168d`) show a microkernel driver layout that maps well onto the O6's mostly standard PCIe/xHCI/NVMe hardware.
- **Mali CSF GPU:** Tyr is the only Rust driver for Arm's CSF Mali family and is dual GPL-2.0/MIT, so its MIT side can be reused. It targets G610, and the O6 has a G720 (a newer CSF generation), so firmware interface differences must be checked.
- **GPL rule of thumb:**
  - Reading GPL code to understand hardware is generally fine.
  - Translating or porting it line by line makes a derivative work that must stay GPL.
  - Prefer dual-licensed sources (Tyr, Asahi AGX, rtw89 GPL-2.0 OR BSD-3-Clause) or permissive Rust drivers (Redox MIT, tgoskits MIT/Apache).

### Gaps
- Not confirmed: Hermit's current aarch64 hardware support and whether it boots via ACPI or UEFI.
- Not confirmed: Theseus aarch64 maturity (repo dormant).
- Not found: Asterinas aarch64 platform list (QEMU virt only?).
- Not found: the status of the out-of-tree Rust NVMe driver.
- No public source compares Mali G720 and G610 CSF firmware interfaces in a way relevant to Tyr.

---

## (d) Driver-reuse strategies for a non-Linux kernel

### Takeaway
On the O6 the cheapest high-value path is:
1. Write native Rust drivers for the standard, spec-defined devices (PL011, GICv3/ITS, generic timer, PCIe ECAM, NVMe, xHCI, SMMUv3, SBSA watchdog). Several MIT/Apache Rust bases exist.
2. Use the UEFI GOP framebuffer early.
3. Handle the hard vendor and complex devices (RTL8852BE Wi-Fi, Immortalis GPU, CIX DPU/VPU/NPU) either with a Linux driver VM exporting virtio (the LionsOS model) or via a LinuxKPI/DDE-style shim.

Licensing decides the shim route. rtw89 is GPL-2.0 OR BSD-3-Clause, so it is shim-friendly. r8169 is GPL-2.0-only.

### Cited Findings

**FreeBSD LinuxKPI**
- FreeBSD runs Linux wireless drivers (iwlwifi, rtw88, rtw89, mt76) through LinuxKPI.
  - Late 2025: rtw89 fine on RX but stuck on basic TX rates, and can panic. mt76 blocked on LinuxKPI `struct page` conflicts. A drm-kmod PR was opened to address `struct page` changes.
  - Q1 2026: iwlwifi updated with FreeBSD-specific tweaks for the `mld` sub-driver.

  — [FreeBSD status report Q4 2025: LinuxKPI 802.11](https://www.freebsd.org/status/report-2025-10-2025-12/lkpi-wireless/); [FreeBSD status report Q1 2026](https://www.freebsd.org/status/report-2026-01-2026-03/lkpi-wireless/)
- FreeBSD `rtw89(4)` (May 2026): only 802.11a/b/g supported so far, n/ac/ax "yet to come". It "does not seem to work (reliably) on machines with more than 4GB of main memory". — [man rtw89(4)](https://man.freebsd.org/rtw89/4)

**Licenses of the Linux drivers for the O6's NIC and Wi-Fi**
- Linux `rtw89` (the RTL8852BE driver) is `GPL-2.0 OR BSD-3-Clause`. — [torvalds/linux rtw89/rtw8852be.c](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/realtek/rtw89/rtw8852be.c)
- Linux `r8169` (RTL8126A support, MAC VER_70) is `GPL-2.0-only`. It loads firmware blobs `rtl_nic/rtl8126a-2.fw` / `rtl8126a-3.fw`. — [torvalds/linux r8169_main.c](https://github.com/torvalds/linux/blob/master/drivers/net/ethernet/realtek/r8169_main.c)

**Genode DDE Linux**
- Genode moved from one big shared Linux emulation to per-driver tailored environments (`lx_kit`/`dde_linux`). Its porting guide is "partially out of date" and points to the Genode Foundations book. — [Genode porting guide](https://genode.org/documentation/developer-resources/porting_device_drivers)
- Estimated new-port cost is 1–3 person-months per subsystem. — [Genodians 2021: dde_linux experiments](https://genodians.org/skalk/2021-04-06-dde-linux-experiments)
- 21.08 introduced the new DDE-Linux approach, with A64 network and framebuffer drivers. — [Genode 21.08 release notes](https://genode.org/documentation/release-notes/21.08)
- A 2022 list thread indicates the new lx_kit was first developed for ARM64. — [Genode users list, Sept 2022](https://lists.genode.org/pipermail/users/2022-September/008061.html) (secondary summary; not re-read in full)

**Linux driver VM exporting virtio (seL4 LionsOS)**
- LionsOS reuses unmodified Linux drivers by running each one in a minimally configured Linux kernel in a per-device VM. A statically linked UIO program replaces init and maps the shared sDDF queues. The VMM turns signals into guest IRQs. — [LionsOS paper, arXiv 2501.06234](https://arxiv.org/pdf/2501.06234)
- Costs and status:
  - Default driver-VM footprint ~106 MiB, ~15 MiB for an optimised block VM.
  - About 2 weeks per device class.
  - VirtIO implementations exist for console, block, net and audio (Sept 2025).
  - The approach is cheap in engineering effort but has a "significant performance cost".

  — [Trustworthy Systems, seL4 Summit 2025 slides](https://sel4.systems/Summit/2025/slides/trustworthy-systems.pdf); [TS device virtualisation](https://trustworthy.systems/projects/drivers/devvirt)

**UEFI firmware facilities**
- The O6 firmware provides "Secure boot, GOP display support". USB, PCIe (ECAM) and SMMU are described with standard ACPI objects. — [CIX LIS25-139 slides](https://www.kitefor.events/download/410)
- NetBSD on the O6 used a UEFI-provided simple framebuffer (`genfb0` 1920x1080x32) as its console. — [NetBSD port-arm, Jan 2025](https://mail-index.netbsd.org/port-arm/2025/01/19/msg009109.html)

**Native Rust bases**
- Permissive Rust bases for standard devices exist (see (b)): Redox `rtl8168d`/`nvmed`/`xhcid` (MIT) and tgoskits `realtek-rtl8125` (MIT), `nvme-driver` (MIT), `crab-usb` (Apache-2.0), `arm-smmu-v3`. — [Redox base](https://gitlab.redox-os.org/redox-os/base); [tgoskits](https://github.com/rcore-os/tgoskits)

### Inferences

| Strategy | Pros | Cons | Fit for the O6 |
|---|---|---|---|
| Native Rust drivers | Best for learning and performance | Most work | Standard devices: NVMe, xHCI, GICv3/ITS, SMMUv3, RTL8126 (spec-light, but Redox/tgoskits RTL8168/8125 code is MIT) |
| UEFI GOP early | Display with zero driver work | No modesetting after ExitBootServices | Early framebuffer until a DPU driver exists (if ever) |
| Linux driver VM + virtio | Unmodified drivers; no GPL contamination of the kernel, since Linux runs as a separate program | Needs a working EL2 hypervisor in the new OS, SMMUv3 stage-2 for DMA isolation, a VMM and virtio back-ends; ~100 MiB RAM per VM | Wi-Fi (rtw89), GPU, VPU/NPU |
| LinuxKPI / DDE shim | Near-native performance | Large ongoing maintenance; links GPL code into the kernel unless the driver is dual-licensed | rtw89, because it is GPL-2.0 OR BSD-3-Clause; still a big job |
| Porting Rust-for-Linux drivers via a shim of the `kernel` crate | Rust-to-Rust | The `kernel` crate API changes every release and is GPL-2.0 (Tyr is GPL-2.0 OR MIT) | Only worth it for Tyr as a GPU reference |

- **LinuxKPI caution:** FreeBSD's experience shows the shim alone is years of work. Its rtw89 still lacks HT/VHT/HE and has a >4 GB RAM bug, which matters on a 16 GB machine. This route is unattractive for a research OS.

### Gaps
- No 2026 status found for NetBSD rump kernels (anykernel) as a driver-reuse vehicle. Not researched in depth.
- No source found on anyone exporting Linux drivers via virtio-over-vhost-user from a KVM/Linux guest to a non-Linux host OS on Arm in 2026, beyond LionsOS on seL4.
- No published evaluation found of Genode's dde_linux on 2026 releases (the 2025–2026 release notes were not retrieved).

---

## (e) Dev and test infrastructure: emulators, KVM, real-board loop, debugging, CI, verification

### Takeaway
The fastest loop on this machine has three tiers:
1. **KVM on the O6 host** (`-accel kvm -cpu host`, GICv3) for EL1 kernel iteration with real A720/A520 features. Local probes confirm MTE, SVE (128-bit) and PAuth pass through. Nested virtualization and GICv4 do not.
2. **QEMU TCG** `-cpu max` / `neoverse-n2` / `cortex-a710` for EL2/EL3, SMMUv3 and MTE fault testing, plus `sbsa-ref` for firmware-shaped (TF-A + EDK2) boots.
3. **The real board** over the 115200-8N1 UART with UEFI boot.

Arm's free AEM FVP covers Armv9.x features beyond the hardware.

### Cited Findings

**Local QEMU/KVM probes** (2026-10-25)
- QEMU 11.1.2 CPU models include `cortex-a710`, `neoverse-n2`, `neoverse-v1`, `neoverse-n1`, `a64fx`, `max`, `host`. There is **no `cortex-a720`/`a520`/`neoverse-v3` model**. Machines include `virt` (alias of virt-11.1) and `sbsa-ref`. — Local probe (`qemu-system-aarch64 -cpu help`, `-machine help`)
- TCG with `-M virt,mte=on`:
  - Starts with `cortex-a710`, `neoverse-n2` and `max`.
  - Fails with `neoverse-v1` and `a64fx`: "MTE requested, but not supported by the guest CPU".
  - `-M virt,iommu=smmuv3 -cpu max` starts.

  — Local probe
- KVM on the O6:
  - `/dev/kvm` present.
  - `-accel kvm -cpu host` starts with `mte=on`, `sve=on,sve128=on` and `pauth=on`.
  - `gic-version=4` is refused: "KVM does not support GICv4 emulation".
  - `virtualization=on` is refused: "host kernel KVM does not support providing Virtualization extensions to the guest CPU".

  — Local probe
- Emulated devices available: `nvme`, `qemu-xhci`/`nec-usb-xhci`, `e1000e`, `igb`, `rtl8139`, `virtio-net-pci`. — Local probe (`-device help`)
- QEMU's TCG feature list includes FEAT_MTE3 (since QEMU 6.1). FEAT_MTE4 is not implemented. — [QEMU A-profile emulation docs](https://qemu.readthedocs.io/en/master/system/arm/emulation.html); [QEMU work item: Implement FEAT_MTE4](https://gitlab.com/qemu-project/qemu/-/work_items/3116)
- `sbsa-ref` is a "static system" modeled on BSA/SBSA for firmware and OS standards testing.
  - Firmware must match its fixed layout; QEMU passes a minimal DT to EL3 firmware.
  - Hardware: GICv3 (ITS added in platform version 0.2), E1000E and Bochs display on PCIe, AHCI and XHCI as system-bus devices (xHCI since 0.3), SBSA watchdog, CPU topology (0.4).

  — [QEMU sbsa-ref docs](https://qemu.readthedocs.io/en/master/system/arm/sbsa.html)

**Arm FVP**
- The AEM Base RevC FVP is free to use without a license (registration may be required). Newer builds document features up to Armv9.7 via `cluster0.has_arm_v9-7`-style parameters. It is a functional model, not a cycle model. — [Arm Community forum: v9.5/v9.6 AEM FVP](https://community.arm.com/support-forums/f/dev-platforms-forum/57624/arm-a-profile-v9-5-and-v9-6-aem-fvp); [TF-A FVP support docs](https://trustedfirmware-a-sandbox.readthedocs.io/en/latest/plat/arm/fvp/fvp-support.html)

**Real-board serial and UEFI**
- O6 serial console: 115200 8N1 carries EDK2, board debug and OS output. The board has three UART headers, and uart5 emits Secure BootROM debug logs. — [Radxa forum: Firmware on serial console?](https://forum.radxa.com/t/firmware-on-serial-console/26077)
- The BIOS menu is reachable over the debug serial port. — [Radxa docs: install BIOS](https://docs.radxa.com/en/orion/o6/bios/install-bios)
- UEFI HTTP Boot must be enabled at EDK2 build time. When present it appears as "EFI Network" in the boot menu. Not confirmed for the CIX EDK2. — [iPXE: UEFI HTTP chainloading](https://ipxe.org/appnote/uefihttp)

**CI and verification**
- tgoskits' CI and container pin QEMU 11.1.1 and drive everything through `cargo xtask`. — [tgoskits README](https://github.com/rcore-os/tgoskits)
- `bare-test` (MPL-2.0) is a test framework for bare-metal Rust. — [crates.io: bare-test](https://crates.io/crates/bare-test)
- Verification tools are all active as of Oct 2026: Verus (MIT, push 2026-10-25), Kani (Apache-2.0, 2026-10-24), Miri (Apache-2.0, 2026-10-25). — [verus-lang/verus](https://github.com/verus-lang/verus), [model-checking/kani](https://github.com/model-checking/kani), [rust-lang/miri](https://github.com/rust-lang/miri)
- Asterinas's vostd is the reference for Verus-verifying kernel memory management in Rust. — [asterinas/vostd](https://github.com/asterinas/vostd)

### Inferences
- **Recommended loop:**
  - Kernel built as a UEFI app or stub, or as a raw image loaded by QEMU `-kernel`.
  - KVM `-M virt,gic-version=3 -cpu host,sve128=on,mte=on -accel kvm` for sub-second boots that exercise real MTE/SVE/PAC.
  - Pass the O6's own ACPI tables only for parser testing. QEMU virt generates its own ACPI when booted with EDK2 (`-bios QEMU_EFI.fd`), which exercises the ACPI-only path.
  - Use the QEMU gdbstub (`-s -S`) with `gdb-multiarch` or `lldb`.
- **No nested virt:** the host KVM refuses nested virtualization, so EL2 (hypervisor or VHE) kernel code cannot be tested under KVM on this board. Use TCG `-M virt,virtualization=on -cpu max` (slow but complete), the FVP, or bare metal.
- **Firmware path:** test it with `sbsa-ref` + TF-A + EDK2, which resembles the O6 (GICv3+ITS, xHCI, ACPI from EDK2).
- **Real board:** UEFI from USB/NVMe ESP, or network boot if CIX EDK2 enables PXE/HTTP (unverified), plus the UART for logs. A JTAG path is unknown.
- **CI:** use `cargo test` for host-testable crates (page tables, ACPI parsing, allocators), with Miri for unsafe-heavy data structures and Kani proofs for MMIO/bitfield and page-table invariants. Run QEMU TCG boot smoke tests with a custom `#[test_case]` harness that exits via PSCI SYSTEM_OFF or semihosting. Hosted GitHub runners have no KVM for aarch64 guests unless self-hosted on the O6.
- **CPU model:** the closest QEMU model to the O6's A720 is `neoverse-n2` or `cortex-a710` (both Armv9.0 with MTE in TCG). `max` gives the newest features, including ones the O6 lacks such as SME.

### Gaps
- Not verified: whether CIX EDK2 for the O6 enables PXE/UEFI HTTP Boot.
- Not verified: whether the board exposes usable JTAG/SWD (CoreSight DAP) and which probes (e.g. Lauterbach, J-Link, OpenOCD) support the CIX P1.
- Not found: the current FVP release version number or the exact Armv9.x levels in the 2026 build.
- No Rust-specific tracing/profiling tooling (e.g. SPE/PMU use) found for bare-metal aarch64 in 2026.
- Fuzzing setups for no_std kernels (e.g. syzkaller-like harnesses for non-Linux kernels) not researched.

---

## (f) Existing OS support on the Orion O6 / CIX P1 (precedent for ACPI-only bring-up)

### Takeaway
Besides Linux:
- OpenBSD lists the O6/O6N as supported.
- NetBSD booted in ACPI mode in Jan 2025, single CPU, with NVMe, GICv3+ITS, PL011, PCIe and framebuffer. xHCI and RTL8126 did not attach.
- FreeBSD boots but runs all cores at 1 GHz because the firmware exposes no P-states.
- Windows 11 arm64 installs, with few drivers.

**No evidence was found of Redox, ArceOS/StarryOS, Asterinas, Hermit, seL4, Genode or Haiku running on the O6/CIX P1.** A Rust OS booting there would likely be a first.

Lessons for ACPI-only bring-up:
- Console discovery is via DBG2. SPCR is not present on the user's current firmware.
- CIX's DSDT reuses DT bindings via `_DSD` with private `CIXH*` HIDs and AML-driven SCMI clocks.
- SMP, PPTT topology and DVFS are where other OSes stumble.

### Cited Findings

**OpenBSD**
- OpenBSD/arm64 lists "Radxa Orion O6" and "Orion O6N" under the Cix CD8180 SoC. The latest supported release is OpenBSD 8.0. — [OpenBSD/arm64](https://www.openbsd.org/arm64.html)
- Jan 2025 arm@openbsd.org thread:
  - The firmware exposed DBG2 but no SPCR. Patrick Wildt suggested enabling SPCR or waiting for a DBG2 diff from Mark Kettenis.
  - Radxa rebuilt EDK2 with SPCR enabled. Linux used it (pl011 @ 0x40e0000, 115200), but OpenBSD lost both serial and HDMI, NetBSD got HDMI only, and FreeBSD got no output.
  - ACPI OEM IDs: CIXTEK / SKY1EDK2.

  — [arm@openbsd.org, 2025-01-21](https://www.mail-archive.com/arm@openbsd.org/msg02959.html); [thread start](https://www.mail-archive.com/arm@openbsd.org/msg02946.html)
- Radxa worked with the FreeBSD/NetBSD/OpenBSD projects, and OpenBSD booted via ACPI with a standard install image. — [CNX Software, 2025-03-03](https://www.cnx-software.com/2025/03/03/software-progress-and-source-code-release-for-cix-p1-armv9-soc-and-orion-o6-motherboard/)

**NetBSD**
- NetBSD 10.99.12 GENERIC64 (Jan 18 2025) attached via ACPI (ACPICA 20240827, root through `acpifdt0`):
  - **Attached:** CPU0 (0x410fd811 = A720), `gicvthree0` with ITS #0 @ 0xe050000, `armgtmr0` (1000 MHz), `plcom0-2` (PL011), four `acpipchb` PCIe host bridges (x4 @ 8 GT/s), `nvme0`, `genfb0` (1920x1080 framebuffer), ACPI battery/fan/thermal zones and power button.
  - **Not configured:** CPU1–CPU11, `XHC0 (PNP0D10)` and USB0–3, both RTL8126 (10ec:8126), a MediaTek 14c3:7925 Wi-Fi card, RTC (RX8900), audio codec. Warning: "no TOD clock present".

  — [NetBSD port-arm, 2025-01-19](https://mail-index.netbsd.org/port-arm/2025/01/19/msg009109.html)

**FreeBSD**
- FreeBSD boots, but all cores are stuck at 1 GHz. The poster attributes this to reduced-hardware ACPI with an FADT P-state count of 0, and asks about SCMI/CPPC frequency control. No replies are shown. — [FreeBSD forums: CPU frequency stuck at 1 GHz on Radxa Orion O6](https://forums.freebsd.org/threads/cpu-frequency-stuck-at-1-ghz-on-radxa-orion-o6-arm-cortex-a720-no-cpufreq-support.99702/post-722951)

**Windows on Arm**
- The SystemReady SR-certified firmware runs Windows 11 arm64 "without issues", but there are "precious few drivers". — [Jeff Geerling, 2025](https://www.jeffgeerling.com/blog/2025/radxa-orion-o6-brings-arm-midrange-pc/)

**Linux** (precedent and pain points)
- Mainline has `arch/arm64/boot/dts/cix/sky1-orion-o6.dts`, `sky1-xcp.dts` and `sky1.dtsi` (DT path). — [torvalds/linux arch/arm64/boot/dts/cix](https://github.com/torvalds/linux/tree/master/arch/arm64/boot/dts/cix)
- CIX's ACPI design (LIS25 talk):
  - **Standard ACPI:** GTDT (EL1/EL2 and virtual timers, GT block, SBSA watchdog); PCIe as PNP0A08 with ECAM (0x20000000 in the example); USB as PNP0D10 with `_UPC`/`_PRW`; ACPI 6.2 pin-control resources.
  - **Private HIDs reusing DT drivers through `_DSD`:** the CIX GPT timer is "not ACPI compliant" and uses private `_HID` CIXH1007. VPU is CIXH3010, GPU CIXH5000, NPU CIXH4000.
  - **Firmware-mediated control:** clocks via an AML-based SCMI approach (`acpi_clk_*` methods to CSU_PM/SCMI); power domains via `PowerResource` and `_PR0`/`_PR3`; CPU idle via `_LPI`/PSCI; CPU DVFS via CPPC over an SCMI fastchannel with AMU counters; an EC with fan/thermal methods.
  - **Kernel gaps:** the arm64 ACPI framework lacks support for clock, pinctrl, devfreq and thermal.

  — [CIX/Linaro LIS25-139 slides](https://www.kitefor.events/download/410); [Class Central listing of the talk](https://www.classcentral.com/course/youtube-lis25-139-acpi-support-on-radxa-orion-o6-456079)
- Community issues:
  - Power-off/reboot failures on CIX's patched 7.0 kernel. — [geerlingguy/sbc-reviews #62](https://github.com/geerlingguy/sbc-reviews/issues/62)
  - "ACPI mode needs firmware improvements, 23MB of blobs scare potential contributors away".
  - PPTT overrides injected via dracut `acpi_override` fix topology.

  — [Radxa forum "Orion O6 Debug Party" thread (p.24)](https://forum.radxa.com/t/orion-o6-debug-party-invitation/25054?page=24) (secondary summary)
- Recommended UEFI settings for mainline: O/S Hardware Description = ACPI, "Enable ACPI SCMI" = Enabled, EDK2 ≥ 1.2.1. — [cix-p1-kernel-deploy script](https://github.com/AkaseToshiyuki/cix-p1-kernel-deploy)
- Radxa ships edk2-cix firmware packages. The `radxa-pkg/edk2-cix` repo is GPL-3.0 per the GitHub API (packaging repo), last push 2026-07-16. — [Radxa downloads](https://docs.radxa.com/en/orion/download); [GitHub radxa-pkg/edk2-cix](https://github.com/radxa-pkg/edk2-cix)

**Haiku**
- The Haiku ARM64 port reached the desktop **in QEMU** (March 2026 update) and is not on real hardware yet. Nothing O6-specific. — [OSNews: Haiku on arm64 boots to desktop in QEMU](https://www.osnews.com/story/144799/haiku-on-arm64-boots-to-desktop-in-qemu/)

**User's own board** (local probes, 2026-10-25)
- ACPI tables in `/sys/firmware/acpi/tables`: `APIC (MADT) CSRT DBG2 DSDT FACP FPDT GTDT IORT MCFG PCCT PPTT SDEI SSDT1 SSDT2`. There is **no SPCR**, and no SRAT/SLIT/HEST/BERT. — Local probe
- CPU parts 0xd80 (Cortex-A520) and 0xd81 (Cortex-A720); MIDR of cpu0 and cpu11 is 0x410fd811 (A720 r0p1); 12 CPUs online. — Local probe (`/proc/cpuinfo`, sysfs MIDR)
- Linux hwcaps:
  - Present: `sve sve2 svebitperm svesha3 svesm4 sveaes svei8mm svebf16 mte mte3 paca pacg flagm2 frint i8mm bf16 dgh ecv afp wfxt sb dit lrcpc ilrcpc uscat`.
  - Absent: `bti`, `rng`, `ssbs`, `sme`.
- Sanitized ID registers: `ID_AA64PFR1_EL1.MTE=3`, `.BT=0`; `ID_AA64PFR0_EL1.SVE=1`; `ID_AA64ISAR0_EL1.RNDR=0`; `ID_AA64ISAR2_EL1.APA3=5` (QARMA3 PAuth with FPACCOMBINE-level support).
- **The running kernel cmdline contains `arm64.nobti`**, so BT=0 and the missing `bti` hwcap come from a boot option, not proof the hardware lacks BTI. The kernel config has `CONFIG_ARM64_BTI=y`, `PTR_AUTH(_KERNEL)=y`, `MTE=y`, `SVE=y`, `SME=y`, `GCS=y`, `POE=y`.

— Local probe (`/proc/cmdline`, `/proc/config.gz`, MRS of ID registers from EL0)

### Inferences
- **Console:** with no SPCR and DBG2 present, early console discovery should parse DBG2 (as Linux's earlycon and OpenBSD's planned diff do). Alternatively, hard-code PL011 @ 0x040e0000 (from the OpenBSD thread) as a fallback.
- **SMP:** NetBSD's single-CPU result suggests a trap. All 12 cores must be started via PSCI CPU_ON using MPIDRs from the MADT, and the 4+4+4 cluster topology (A720 big/medium + A520) needs PPTT parsing that copes with CIX's known-buggy PPTT.
- **What works with standard ACPI:** GICv3+ITS, generic timer, PL011, PCIe ECAM, NVMe and framebuffer. These are the minimum viable set.
- **Needs work:** xHCI (PNP0D10 platform device, a CIX USB controller wrapper, possibly needing power/clock via AML) and RTL8126 (PCIe; needs a driver plus `rtl8126a-*.fw` firmware).
- **Interpreter:** DVFS, clocks and power domains need either an AML interpreter executing CIX's SCMI-backed methods or a direct SCMI client over the PCCT/mailbox. The FreeBSD 1 GHz issue shows the cost of skipping this.
- **BTI on the host:** while the host runs with `arm64.nobti`, KVM guests probably also won't see BTI (KVM exposes sanitized host features). Re-test after removing it. It may have been added as a workaround (the user's notes mention NVIDIA/WebKit quirks).
- **Wi-Fi varies by unit:** NetBSD's unit had a MediaTek MT7925 card (14c3:7925), while the user's board reportedly has RTL8852BE. The Wi-Fi module is an M.2 card, so the right driver depends on the installed card.

### Gaps
- **No sources found for Redox, ArceOS, StarryOS, Asterinas, Hermit, seL4, Genode or other hobby OSes on the Orion O6 / CIX P1.** Searches returned nothing (as of 2026-10-25).
- Not determined: whether OpenBSD 8.0's O6 support uses ACPI or DT, or which devices it supports (the hardware page lists the board only).
- No 2026 update found on NetBSD's O6 status beyond Jan 2025.
- FreeBSD: no source on which drivers attach (NVMe/xHCI/RTL8126) beyond the cpufreq issue.
- Windows: no 2026 driver-status source found (CIX may ship Windows drivers; unverified).
- The CIX LIS25 slides' "public source code" section and full upstream-ACPI status were not fully extracted.
- Raw (unsanitized) BTI/RNG/SSBS support on the CIX P1 cores was not verified at EL1. SSBS is likely hidden by Linux's A520/A720 speculative-SSBS erratum workaround (inference from knowledge, unverified here).
