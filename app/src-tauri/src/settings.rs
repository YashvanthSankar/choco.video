//! User settings (JSON in the app config folder) and the provider key (macOS Keychain only).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const KEYCHAIN_SERVICE: &str = "video.choco.app";
const KEYCHAIN_USER: &str = "provider-api-key";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderSettings {
    /// "opencode", "custom", "apple" or "local" (the offline picker).
    pub kind: String,
    pub base_url: String,
    pub score_model: String,
    pub summary_model: String,
}

impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            kind: "apple".into(),
            base_url: choco_engine::highlights::picker::openai_compat::OPENCODE_ZEN_BASE_URL.into(),
            score_model: String::new(),
            summary_model: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Defaults {
    pub style: String,
    pub preset: String,
    /// "auto", "static" or "fit".
    pub layout: String,
    pub max_clips: usize,
    pub min_score: u8,
    pub output_dir: String,
}

impl Default for Defaults {
    fn default() -> Self {
        let out = dirs_movies().join("choco");
        Self {
            style: "bold".into(),
            preset: "shorts".into(),
            layout: "auto".into(),
            max_clips: 10,
            min_score: 55,
            output_dir: out.to_string_lossy().into_owned(),
        }
    }
}

/// The user's videos folder: ~/Movies on macOS, %USERPROFILE%\Videos on Windows.
fn dirs_movies() -> PathBuf {
    dirs::video_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Movies")))
        .unwrap_or_else(std::env::temp_dir)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub onboarded: bool,
    pub provider: ProviderSettings,
    pub defaults: Defaults,
    /// "auto", "apple" or "whisper".
    pub speech: String,
    /// BCP-47, e.g. "en-US". Empty = automatic.
    pub language: String,
    /// Opt-in: look for a new version on GitHub when the app opens. Off unless the user turns it
    /// on; "Check for updates" in Settings works either way because the user asked.
    pub check_updates: bool,
}

impl Settings {
    pub fn path(config_dir: &Path) -> PathBuf {
        config_dir.join("settings.json")
    }

    pub fn load(config_dir: &Path) -> Settings {
        std::fs::read_to_string(Self::path(config_dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| Settings {
                speech: "auto".into(),
                ..Default::default()
            })
    }

    pub fn save(&self, config_dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
        let tmp = config_dir.join("settings.json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(tmp, Self::path(config_dir)).map_err(|e| e.to_string())
    }
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).map_err(|e| e.to_string())
}

pub fn key_get() -> Option<String> {
    entry().ok()?.get_password().ok().filter(|k| !k.is_empty())
}

pub fn key_set(key: &str) -> Result<(), String> {
    entry()?.set_password(key.trim()).map_err(|e| e.to_string())
}

pub fn key_delete() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_and_defaults() {
        let dir = std::env::temp_dir().join(format!("choco-settings-{}", std::process::id()));
        let mut s = Settings::load(&dir);
        assert!(!s.onboarded);
        assert_eq!(s.defaults.layout, "auto");
        s.onboarded = true;
        s.provider.kind = "opencode".into();
        s.save(&dir).unwrap();
        assert_eq!(Settings::load(&dir), s);
        // Unknown or missing fields fall back to defaults.
        std::fs::write(Settings::path(&dir), r#"{"onboarded":true}"#).unwrap();
        let t = Settings::load(&dir);
        assert!(t.onboarded && t.defaults.max_clips == 10);
        let _ = std::fs::remove_dir_all(dir);
    }
}
