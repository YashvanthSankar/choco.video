//! Universal fallback. Must pass the full test suite on its own; it is what GPU-less CI runs.

use super::*;

pub struct CpuBackend {
    info: HardwareInfo,
}

impl CpuBackend {
    pub fn detect() -> Self {
        Self {
            info: HardwareInfo {
                os: std::env::consts::OS.to_string(),
                os_version: String::new(),
                arch: std::env::consts::ARCH.to_string(),
                cpu_cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
                memory_bytes: None,
                gpu: None,
            },
        }
    }
}

impl Backend for CpuBackend {
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
        vec![HwCodec::Software]
    }
}
