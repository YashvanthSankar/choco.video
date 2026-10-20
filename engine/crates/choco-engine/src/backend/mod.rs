//! Hardware detection and acceleration choice. The only place platform differences live.

use serde::{Deserialize, Serialize};

mod cpu;
pub mod gpu;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "windows")]
mod windows;

pub use cpu::CpuBackend;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareInfo {
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub cpu_cores: usize,
    /// Total physical or unified memory in bytes, if it could be read.
    pub memory_bytes: Option<u64>,
    pub gpu: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    Metal,
    Vulkan,
    Cpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SpeechEngine {
    /// Apple SpeechAnalyzer, macOS 26+. Zero download.
    AppleSpeech,
    /// whisper.cpp on the given device. Needs a model download.
    Whisper { device: Device },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HwCodec {
    VideoToolbox,
    D3d11va,
    Nvenc,
    Amf,
    Qsv,
    MediaFoundation,
    Software,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRecommendation {
    /// Whisper model id to download, or None when the OS speech engine covers it.
    pub whisper_model: Option<String>,
    /// Whether the opt-in local picker model fits comfortably in memory.
    pub local_llm_ok: bool,
}

pub trait Backend: Send + Sync {
    fn info(&self) -> HardwareInfo;
    /// Speech engines usable on this machine, best first.
    fn speech_engines(&self) -> Vec<SpeechEngine>;
    fn hw_decoders(&self) -> Vec<HwCodec>;
    fn hw_encoders(&self) -> Vec<HwCodec>;

    fn recommended_models(&self) -> ModelRecommendation {
        let engines = self.speech_engines();
        let mem = self.info().memory_bytes.unwrap_or(0);
        ModelRecommendation {
            whisper_model: if engines.first() == Some(&SpeechEngine::AppleSpeech) {
                None
            } else if mem >= 8 * GIB {
                Some(crate::models_registry::WHISPER_DEFAULT.to_string())
            } else {
                Some(crate::models_registry::WHISPER_SMALL.to_string())
            },
            local_llm_ok: mem >= 16 * GIB,
        }
    }
}

const GIB: u64 = 1024 * 1024 * 1024;

/// The backend for this machine.
pub fn detect() -> Box<dyn Backend> {
    #[cfg(target_os = "macos")]
    {
        Box::new(mac::MacBackend::detect())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsBackend::detect())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Box::new(CpuBackend::detect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_reports_something_sane() {
        let b = detect();
        let info = b.info();
        assert!(info.cpu_cores >= 1);
        assert!(
            !b.speech_engines().is_empty(),
            "every machine has at least whisper on CPU"
        );
        assert!(b.hw_encoders().contains(&HwCodec::Software) || !b.hw_encoders().is_empty());
    }

    #[test]
    fn cpu_backend_always_works() {
        let b = CpuBackend::detect();
        assert_eq!(
            b.speech_engines(),
            vec![SpeechEngine::Whisper {
                device: Device::Cpu
            }]
        );
        assert_eq!(b.hw_encoders(), vec![HwCodec::Software]);
    }
}
