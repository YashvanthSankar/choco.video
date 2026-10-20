//! Windows. M1 stub: whisper on CPU, Media Foundation encode. Real detection lands in M6.

use super::*;

pub struct WindowsBackend {
    info: HardwareInfo,
}

impl WindowsBackend {
    pub fn detect() -> Self {
        let mut info = CpuBackend::detect().info();
        info.os = "windows".into();
        Self { info }
    }
}

impl Backend for WindowsBackend {
    fn info(&self) -> HardwareInfo {
        self.info.clone()
    }
    fn speech_engines(&self) -> Vec<SpeechEngine> {
        vec![SpeechEngine::Whisper {
            device: Device::Cpu,
        }]
    }
    fn hw_decoders(&self) -> Vec<HwCodec> {
        vec![HwCodec::Software]
    }
    fn hw_encoders(&self) -> Vec<HwCodec> {
        vec![HwCodec::MediaFoundation]
    }
}
