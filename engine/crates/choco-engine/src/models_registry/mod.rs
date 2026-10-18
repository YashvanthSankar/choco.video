//! Runtime-downloaded models. Nothing here ships in the installer.
//! Only Whisper (Windows and macOS < 26) and the opt-in local picker model are ever listed.

use crate::model::Progress;
use crate::ProgressFn;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const STAGE: &str = "models";

/// Default Whisper model where the OS has no speech engine.
pub const WHISPER_DEFAULT: &str = "whisper-large-v3-turbo-q5_0";
/// Smaller Whisper for low-memory machines and fast previews.
pub const WHISPER_SMALL: &str = "whisper-base-q5_1";
/// Tiny English model used by tests.
pub const WHISPER_TEST: &str = "whisper-tiny.en-q5_1";

/// Pinned Hugging Face revision of ggerganov/whisper.cpp (MIT). Files at a revision are immutable.
const WHISPER_REV: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    Whisper,
    LocalLlm,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelManifest {
    pub id: &'static str,
    pub kind: ModelKind,
    pub file: &'static str,
    pub url: String,
    pub size: u64,
    /// sha256 as published by the upstream LFS store.
    pub sha256: &'static str,
    pub license: &'static str,
    pub min_memory_gb: u32,
}

pub fn manifests() -> Vec<ModelManifest> {
    let w = |id, file: &'static str, size, sha256, min_memory_gb| ModelManifest {
        id,
        kind: ModelKind::Whisper,
        file,
        url: format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/{WHISPER_REV}/{file}"),
        size,
        sha256,
        license: "MIT",
        min_memory_gb,
    };
    vec![
        w(WHISPER_DEFAULT, "ggml-large-v3-turbo-q5_0.bin", 574_041_195, "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2", 8),
        w(WHISPER_SMALL, "ggml-base-q5_1.bin", 59_707_625, "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898", 4),
        w(WHISPER_TEST, "ggml-tiny.en-q5_1.bin", 32_166_155, "c77c5766f1cef09b6b7d47f21b546cbddd4157886b3b5d6d4f709e91e66c7c2b", 2),
    ]
}

pub fn find(id: &str) -> Option<ModelManifest> {
    manifests().into_iter().find(|m| m.id == id)
}

/// `<data dir>/choco.video/models`, overridable with `CHOCO_MODELS_DIR`.
pub fn default_dir() -> PathBuf {
    std::env::var_os("CHOCO_MODELS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("choco.video").join("models"))
}

pub fn path_for(dir: &Path, m: &ModelManifest) -> PathBuf {
    dir.join(m.file)
}

/// Installed when the file exists at the expected size. Checksums are verified at download time.
pub fn is_installed(dir: &Path, m: &ModelManifest) -> bool {
    fs::metadata(path_for(dir, m)).map(|md| md.len() == m.size).unwrap_or(false)
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("models: unknown model {0}")]
    Unknown(String),
    #[error("models: download failed: {0}")]
    Http(String),
    #[error("models: checksum mismatch for {id}: expected {expected}, got {actual}")]
    Checksum { id: String, expected: String, actual: String },
    #[error("models: io: {0}")]
    Io(#[from] std::io::Error),
}

/// Download with resume into `<dir>/<file>.part`, verify sha256, then rename into place.
pub fn pull(dir: &Path, id: &str, progress: ProgressFn) -> Result<PathBuf, ModelError> {
    let m = find(id).ok_or_else(|| ModelError::Unknown(id.into()))?;
    fs::create_dir_all(dir)?;
    let dest = path_for(dir, &m);
    if is_installed(dir, &m) {
        progress(Progress::new(STAGE, 1.0, format!("{id} already installed")));
        return Ok(dest);
    }
    let part = dir.join(format!("{}.part", m.file));
    let mut have = fs::metadata(&part).map(|md| md.len()).unwrap_or(0);
    if have > m.size {
        fs::remove_file(&part)?;
        have = 0;
    }

    if have < m.size {
        let mut req = ureq::get(&m.url);
        if have > 0 {
            req = req.header("Range", &format!("bytes={have}-"));
        }
        let resp = req.call().map_err(|e| ModelError::Http(e.to_string()))?;
        let resumed = resp.status().as_u16() == 206;
        if !resumed {
            have = 0;
        }
        let mut file = OpenOptions::new().create(true).write(true).append(resumed).truncate(!resumed).open(&part)?;
        let mut reader = resp.into_body().into_reader();
        let mut buf = vec![0u8; 1 << 20];
        let mut last = std::time::Instant::now();
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            have += n as u64;
            if last.elapsed().as_millis() >= 500 {
                progress(Progress::new(STAGE, have as f32 / m.size as f32, format!("downloading {id}")));
                last = std::time::Instant::now();
            }
        }
        file.flush()?;
    }

    progress(Progress::new(STAGE, -1.0, format!("verifying {id}")));
    let actual = sha256_file(&part)?;
    if actual != m.sha256 {
        let _ = fs::remove_file(&part);
        return Err(ModelError::Checksum { id: id.into(), expected: m.sha256.into(), actual });
    }
    fs::rename(&part, &dest)?;
    progress(Progress::new(STAGE, 1.0, format!("{id} installed")));
    Ok(dest)
}

pub fn remove(dir: &Path, id: &str) -> Result<bool, ModelError> {
    let m = find(id).ok_or_else(|| ModelError::Unknown(id.into()))?;
    let mut removed = false;
    for p in [path_for(dir, &m), dir.join(format!("{}.part", m.file))] {
        if p.exists() {
            fs::remove_file(p)?;
            removed = true;
        }
    }
    Ok(removed)
}

fn sha256_file(p: &Path) -> std::io::Result<String> {
    let mut f = File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_are_well_formed() {
        let ms = manifests();
        assert!(ms.iter().any(|m| m.id == WHISPER_DEFAULT));
        for m in &ms {
            assert_eq!(m.sha256.len(), 64, "{}", m.id);
            assert!(m.url.contains(WHISPER_REV), "{}", m.id);
            assert!(m.size > 1_000_000, "{}", m.id);
        }
        // Size principle: nothing listed as a default exceeds 600 MB.
        assert!(find(WHISPER_DEFAULT).unwrap().size < 600_000_000);
    }

    #[test]
    fn sha256_matches_known_vector() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("abc");
        fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
