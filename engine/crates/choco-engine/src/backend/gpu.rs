//! GPU vendors and the encoder order they imply. Pure logic, built and tested on every OS;
//! `windows.rs` fills it from DXGI.

use super::HwCodec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub vendor: Vendor,
    /// Dedicated video memory in bytes; 0 for integrated GPUs.
    pub dedicated: u64,
}

/// PCI vendor id → vendor. `None` for Microsoft's Basic Render Driver (0x1414), a software
/// adapter that must never be chosen for encoding.
pub fn vendor_from_pci(id: u32) -> Option<Vendor> {
    match id {
        0x10DE => Some(Vendor::Nvidia),
        0x1002 | 0x1022 => Some(Vendor::Amd),
        0x8086 => Some(Vendor::Intel),
        0x1414 => None,
        _ => Some(Vendor::Other),
    }
}

/// Discrete GPUs first (largest dedicated memory first), then integrated: a hybrid laptop should
/// encode on its discrete GPU.
pub fn sort_gpus(gpus: &mut [Gpu]) {
    gpus.sort_by_key(|g| std::cmp::Reverse(g.dedicated));
}

/// The Windows encoder order for these GPUs: one vendor encoder per vendor present, in GPU order,
/// then Media Foundation (its hardware MFT, then its software encoder, which every non-N edition
/// has). Intel's QSV is skipped until the compositor can emit NV12; Intel GPUs are still reached
/// through Media Foundation's hardware MFT.
pub fn encoder_order(gpus: &[Gpu]) -> Vec<HwCodec> {
    let mut v = Vec::new();
    for g in gpus {
        let c = match g.vendor {
            Vendor::Nvidia => HwCodec::Nvenc,
            Vendor::Amd => HwCodec::Amf,
            Vendor::Intel | Vendor::Other => continue,
        };
        if !v.contains(&c) {
            v.push(c);
        }
    }
    v.push(HwCodec::MediaFoundation);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(name: &str, vendor: Vendor, dedicated: u64) -> Gpu {
        Gpu { name: name.into(), vendor, dedicated }
    }

    #[test]
    fn vendors_and_the_software_adapter() {
        assert_eq!(vendor_from_pci(0x10DE), Some(Vendor::Nvidia));
        assert_eq!(vendor_from_pci(0x1002), Some(Vendor::Amd));
        assert_eq!(vendor_from_pci(0x8086), Some(Vendor::Intel));
        assert_eq!(vendor_from_pci(0x1414), None, "Microsoft Basic Render Driver is skipped");
        assert_eq!(vendor_from_pci(0x5143), Some(Vendor::Other));
    }

    #[test]
    fn hybrid_laptop_encodes_on_the_discrete_gpu_first() {
        let mut gpus = vec![g("Intel Iris Xe", Vendor::Intel, 0), g("RTX 4060 Laptop", Vendor::Nvidia, 8 << 30)];
        sort_gpus(&mut gpus);
        assert_eq!(gpus[0].vendor, Vendor::Nvidia);
        assert_eq!(encoder_order(&gpus), vec![HwCodec::Nvenc, HwCodec::MediaFoundation]);
    }

    #[test]
    fn amd_desktop_and_no_gpu() {
        assert_eq!(encoder_order(&[g("RX 7800 XT", Vendor::Amd, 16 << 30)]), vec![HwCodec::Amf, HwCodec::MediaFoundation]);
        assert_eq!(encoder_order(&[]), vec![HwCodec::MediaFoundation], "a VM or CI runner still has the software MFT");
        assert_eq!(encoder_order(&[g("Iris", Vendor::Intel, 0)]), vec![HwCodec::MediaFoundation]);
    }
}
