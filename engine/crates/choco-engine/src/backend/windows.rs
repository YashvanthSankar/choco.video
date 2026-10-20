//! Windows x64. Whisper on CPU (Vulkan arrives with M6 step 6), Media Foundation and the GPU
//! vendor encoders for video, chosen from the adapters DXGI reports.

use super::gpu::{self, Gpu};
use super::*;

pub struct WindowsBackend {
    info: HardwareInfo,
    gpus: Vec<Gpu>,
}

impl WindowsBackend {
    pub fn detect() -> Self {
        let mut gpus = dxgi_adapters();
        gpu::sort_gpus(&mut gpus);
        let info = HardwareInfo {
            os: "windows".into(),
            os_version: os_build().unwrap_or_default(),
            arch: std::env::consts::ARCH.into(),
            cpu_cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            memory_bytes: total_memory(),
            gpu: gpus.first().map(|g| g.name.clone()),
        };
        Self { info, gpus }
    }
}

impl Backend for WindowsBackend {
    fn info(&self) -> HardwareInfo {
        self.info.clone()
    }
    fn speech_engines(&self) -> Vec<SpeechEngine> {
        vec![SpeechEngine::Whisper { device: Device::Cpu }]
    }
    fn hw_decoders(&self) -> Vec<HwCodec> {
        vec![HwCodec::Software]
    }
    fn hw_encoders(&self) -> Vec<HwCodec> {
        gpu::encoder_order(&self.gpus)
    }
}

/// Hardware adapters from DXGI, without Microsoft's software adapter.
fn dxgi_adapters() -> Vec<Gpu> {
    use ::windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};
    let mut out = Vec::new();
    // SAFETY: plain COM calls; every interface is released when it drops.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return out };
    let mut i = 0;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(i) } {
        i += 1;
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else { continue };
        if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
            continue;
        }
        let Some(vendor) = gpu::vendor_from_pci(desc.VendorId) else { continue };
        let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
        out.push(Gpu { name: String::from_utf16_lossy(&desc.Description[..len]), vendor, dedicated: desc.DedicatedVideoMemory as u64 });
    }
    out
}

/// "10.0.26100". RtlGetVersion, because GetVersionEx reports 6.2 to apps without a manifest.
fn os_build() -> Option<String> {
    use ::windows::Wdk::System::SystemServices::RtlGetVersion;
    use ::windows::Win32::System::SystemInformation::OSVERSIONINFOW;
    let mut v = OSVERSIONINFOW { dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32, ..Default::default() };
    // SAFETY: v is a properly sized, initialised OSVERSIONINFOW.
    unsafe { RtlGetVersion(&mut v) }.ok().ok()?;
    Some(format!("{}.{}.{}", v.dwMajorVersion, v.dwMinorVersion, v.dwBuildNumber))
}

fn total_memory() -> Option<u64> {
    use ::windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    // SAFETY: m is a properly sized MEMORYSTATUSEX.
    unsafe { GlobalMemoryStatusEx(&mut m) }.ok()?;
    Some(m.ullTotalPhys)
}
