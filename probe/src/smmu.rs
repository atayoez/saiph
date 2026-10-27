//! Group D, part 2: the SMMUv3 units.
//!
//! An SMMU is an IOMMU: it translates and checks the memory addresses that
//! devices use for DMA, the way the MMU does for programs. Its ID registers
//! give the architecture version (AIDR) and product (IIDR); CR0.SMMUEN says
//! whether the firmware left it switched on.

use crate::mmio::read32;
use crate::report::{Report, say};

/// Product from SMMU_IIDR: implementer in bits 11..0 (0x43b = Arm),
/// product ID in bits 31..20.
fn product(iidr: u32) -> &'static str {
    if iidr & 0xfff != 0x43b {
        return "not an Arm design";
    }
    match iidr >> 20 {
        0x483 => "Arm MMU-600",
        0x487 => "Arm MMU-700",
        0x48a => "Arm MMU-L1",
        0x498 => "Arm MMU-S3",
        _ => "unknown Arm SMMU",
    }
}

pub fn report(r: &mut Report, base: u64) {
    say!(r, "SMMU at {base:#x}:");
    let mut read = |offset: u64| match read32(base + offset) {
        Ok(v) => Some(v),
        Err(e) => {
            say!(r, "  {e}");
            None
        }
    };
    let (Some(idr0), Some(idr5), Some(iidr), Some(aidr), Some(cr0), Some(gbpa)) =
        (read(0x00), read(0x14), read(0x18), read(0x1c), read(0x20), read(0x44))
    else {
        return;
    };
    // AIDR: major version in bits 7..4 (0 means SMMUv3), minor in bits 3..0.
    say!(r, "  SMMU_AIDR = {aidr:#010x}  SMMUv{}.{}", 3 + ((aidr >> 4) & 0xf), aidr & 0xf);
    say!(r, "  SMMU_IIDR = {iidr:#010x}  {} r{}p{}", product(iidr), (iidr >> 16) & 0xf, (iidr >> 12) & 0xf);
    // IDR0 bit 1 (S1P) / bit 0 (S2P): stage 1 and stage 2 translation.
    say!(r, "  SMMU_IDR0 = {idr0:#010x}  stage-1 translation {}, stage-2 translation {}",
        if idr0 >> 1 & 1 == 1 { "yes" } else { "no" }, if idr0 & 1 == 1 { "yes" } else { "no" });
    // IDR5 bits 2..0 (OAS): the largest physical address it can produce.
    let oas = [32, 36, 40, 42, 44, 48, 52, 56].get((idr5 & 7) as usize).copied().unwrap_or(0);
    say!(r, "  SMMU_IDR5 = {idr5:#010x}  output addresses up to {oas} bits");
    // CR0 bit 0 (SMMUEN). While it is off, GBPA bit 20 (ABORT) decides
    // whether device DMA passes through untranslated or is blocked.
    let enabled = cr0 & 1 == 1;
    say!(r, "  SMMU_CR0  = {cr0:#010x}  left {} by the firmware", if enabled { "ENABLED" } else { "disabled" });
    if !enabled {
        say!(r, "  SMMU_GBPA = {gbpa:#010x}  while disabled, device DMA is {}", if gbpa >> 20 & 1 == 1 { "blocked" } else { "passed through" });
    }
}
