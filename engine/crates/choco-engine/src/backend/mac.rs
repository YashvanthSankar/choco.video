//! macOS, Apple Silicon. SpeechAnalyzer on macOS 26+, whisper on Metal otherwise. VideoToolbox for video.

use super::*;
use std::ffi::{CStr, CString, c_char, c_int, c_void};

unsafe extern "C" {
    fn sysctlbyname(
        name: *const c_char,
        oldp: *mut c_void,
        oldlenp: *mut usize,
        newp: *mut c_void,
        newlen: usize,
    ) -> c_int;
}

fn sysctl_bytes(name: &str) -> Option<Vec<u8>> {
    let cname = CString::new(name).ok()?;
    let mut len: usize = 0;
    // SAFETY: first call queries the size; pointers are valid for the duration of the call.
    let rc = unsafe { sysctlbyname(cname.as_ptr(), std::ptr::null_mut(), &mut len, std::ptr::null_mut(), 0) };
    if rc != 0 || len == 0 {
        return None;
    }
    let mut buf = vec![0u8; len];
    // SAFETY: buf has `len` bytes of writable storage.
    let rc = unsafe { sysctlbyname(cname.as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) };
    if rc != 0 {
        return None;
    }
    buf.truncate(len);
    Some(buf)
}

fn sysctl_string(name: &str) -> Option<String> {
    let bytes = sysctl_bytes(name)?;
    CStr::from_bytes_until_nul(&bytes).ok().map(|c| c.to_string_lossy().into_owned())
}

fn sysctl_u64(name: &str) -> Option<u64> {
    let bytes = sysctl_bytes(name)?;
    (bytes.len() == 8).then(|| u64::from_ne_bytes(bytes.try_into().unwrap()))
}

/// Parse "26.1.2" into 26.
pub(crate) fn major_version(v: &str) -> u32 {
    v.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0)
}

pub struct MacBackend {
    info: HardwareInfo,
    apple_speech: bool,
}

impl MacBackend {
    pub fn detect() -> Self {
        let os_version = sysctl_string("kern.osproductversion").unwrap_or_default();
        let info = HardwareInfo {
            os: "macos".into(),
            os_version: os_version.clone(),
            arch: std::env::consts::ARCH.into(),
            cpu_cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            memory_bytes: sysctl_u64("hw.memsize"),
            gpu: sysctl_string("machdep.cpu.brand_string"),
        };
        let apple_speech =
            cfg!(feature = "apple-speech") && major_version(&os_version) >= 26 && crate::transcribe::apple::helper_path().is_some();
        Self { info, apple_speech }
    }
}

impl Backend for MacBackend {
    fn info(&self) -> HardwareInfo {
        self.info.clone()
    }
    fn speech_engines(&self) -> Vec<SpeechEngine> {
        let mut v = Vec::new();
        if self.apple_speech {
            v.push(SpeechEngine::AppleSpeech);
        }
        v.push(SpeechEngine::Whisper { device: if self.info.arch == "aarch64" { Device::Metal } else { Device::Cpu } });
        v
    }
    fn hw_decoders(&self) -> Vec<HwCodec> {
        vec![HwCodec::VideoToolbox, HwCodec::Software]
    }
    fn hw_encoders(&self) -> Vec<HwCodec> {
        // VideoToolbox also provides the software fallback path on macOS.
        vec![HwCodec::VideoToolbox]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_major_version() {
        assert_eq!(major_version("27.0.1"), 27);
        assert_eq!(major_version("14"), 14);
        assert_eq!(major_version(""), 0);
    }

    #[test]
    fn reads_real_sysctls() {
        let b = MacBackend::detect();
        assert!(b.info.memory_bytes.unwrap_or(0) > 0);
        assert!(!b.info.os_version.is_empty());
    }
}
