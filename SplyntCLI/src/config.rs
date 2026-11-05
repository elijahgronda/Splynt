//! Saved sign-in and preferences.
//!
//! The password is never written to disk. Like the iOS app's Keychain entry,
//! the file keeps the Subsonic salt and the token `md5(password + salt)`,
//! which is all a request needs.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub server: String,
    pub username: String,
    pub salt: String,
    pub token: String,
    pub volume: f32,
    /// 0 streams the original file. Anything else asks the server to
    /// transcode down to that bitrate.
    pub max_bitrate: u32,
    /// "auto", "halfblocks" or "off".
    pub images: String,
    pub settings: crate::settings::Settings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: String::new(),
            username: String::new(),
            salt: String::new(),
            token: String::new(),
            volume: 0.8,
            max_bitrate: 0,
            images: "auto".into(),
            settings: Default::default(),
        }
    }
}

impl Config {
    pub fn signed_in(&self) -> bool {
        !self.server.is_empty() && !self.username.is_empty() && !self.token.is_empty()
    }

    pub fn path() -> PathBuf {
        if let Some(p) = std::env::var_os("SPLYNT_CONFIG") {
            return PathBuf::from(p);
        }
        let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join(".config").join("splynt").join("config.toml")
    }

    pub fn load() -> Self {
        let mut c: Self = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default();
        // Older configs had only a bitrate cap.
        if c.max_bitrate > 0 && c.settings.streaming_quality == crate::settings::Quality::Original {
            c.settings.streaming_quality = match c.max_bitrate {
                0..=160 => crate::settings::Quality::Low,
                161..=288 => crate::settings::Quality::Medium,
                _ => crate::settings::Quality::High,
            };
            c.max_bitrate = 0;
        }
        c
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(text) = toml::to_string_pretty(self) else { return };
        if std::fs::write(&path, text).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

pub fn cache_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("SPLYNT_CACHE") {
        return PathBuf::from(p);
    }
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("splynt")
}
