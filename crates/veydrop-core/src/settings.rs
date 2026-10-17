//! Persistent user settings (`settings.json` in the app data directory).

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::fsutil;
use crate::model::Ms;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrustedDevice {
    pub id: String,
    pub name: String,
    pub added_at: Ms,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// "system" | "en" | "ar"
    pub language: String,
    /// "system" | "dark" | "light"
    pub theme: String,
    pub download_dir: String,
    pub device_name: String,
    pub notifications: bool,
    pub launch_at_startup: bool,
    pub minimize_to_tray: bool,
    /// Auto-approve phones that were previously trusted (QR scan still required).
    pub trust_known_devices: bool,
    pub ask_before_receive: bool,
    pub chunk_size_mib: u32,
    /// 0 = unlimited
    pub max_file_size_gib: u32,
    pub max_parallel_uploads: u32,
    pub partial_retention_hours: u32,
    /// 0 = until sharing is stopped
    pub session_minutes: u32,
    pub pairing_seconds: u32,
    /// 0 = automatic
    pub preferred_port: u16,
    pub preferred_ip: Option<String>,
    /// 0 = keep forever
    pub history_retention_days: u32,
    pub trusted_devices: Vec<TrustedDevice>,
}

pub fn default_device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Veydrop PC".to_string())
}

pub fn default_download_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Veydrop")
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: "system".into(),
            theme: "system".into(),
            download_dir: default_download_dir().to_string_lossy().into_owned(),
            device_name: default_device_name(),
            notifications: true,
            launch_at_startup: false,
            minimize_to_tray: true,
            trust_known_devices: false,
            ask_before_receive: true,
            chunk_size_mib: 4,
            max_file_size_gib: 0,
            max_parallel_uploads: 2,
            partial_retention_hours: 24,
            session_minutes: 240,
            pairing_seconds: 180,
            preferred_port: 0,
            preferred_ip: None,
            history_retention_days: 0,
            trusted_devices: vec![],
        }
    }
}

impl Settings {
    /// Clamps values into supported ranges and rejects unusable ones.
    pub fn validate(mut self) -> Result<Self> {
        if !["system", "en", "ar"].contains(&self.language.as_str()) {
            return Err(Error::invalid("language"));
        }
        if !["system", "dark", "light"].contains(&self.theme.as_str()) {
            return Err(Error::invalid("theme"));
        }
        self.device_name = self.device_name.trim().chars().take(48).collect();
        if self.device_name.is_empty() {
            self.device_name = default_device_name();
        }
        if self.download_dir.trim().is_empty() {
            return Err(Error::invalid("downloadDir"));
        }
        self.chunk_size_mib = self.chunk_size_mib.clamp(1, 16);
        self.max_parallel_uploads = self.max_parallel_uploads.clamp(1, 4);
        self.partial_retention_hours = self.partial_retention_hours.clamp(1, 24 * 14);
        self.pairing_seconds = self.pairing_seconds.clamp(30, 900);
        if self.session_minutes != 0 {
            self.session_minutes = self.session_minutes.clamp(5, 24 * 60);
        }
        self.max_file_size_gib = self.max_file_size_gib.min(10_000);
        if self.preferred_port != 0 && self.preferred_port < 1024 {
            return Err(Error::invalid("preferredPort"));
        }
        if let Some(ip) = &self.preferred_ip {
            if ip.parse::<std::net::Ipv4Addr>().is_err() {
                self.preferred_ip = None;
            }
        }
        self.trusted_devices.truncate(64);
        Ok(self)
    }

    pub fn max_file_size_bytes(&self) -> Option<u64> {
        (self.max_file_size_gib > 0).then(|| self.max_file_size_gib as u64 * 1024 * 1024 * 1024)
    }
}

pub struct SettingsStore {
    path: PathBuf,
    current: Mutex<Settings>,
    /// The language the desktop UI resolved for "system" (en or ar). Not
    /// persisted: the desktop reports it on every start.
    system_lang: Mutex<String>,
}

impl SettingsStore {
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join("settings.json");
        let current = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Settings>(&b).ok())
            .and_then(|s| s.validate().ok())
            .unwrap_or_default();
        SettingsStore { path, current: Mutex::new(current), system_lang: Mutex::new("en".into()) }
    }

    pub fn get(&self) -> Settings {
        self.current.lock().clone()
    }

    /// The concrete language ("en" or "ar") shown on the desktop and mirrored
    /// to every connected phone. "system" resolves to what the desktop UI
    /// reported for the operating system.
    pub fn effective_language(&self) -> String {
        match self.current.lock().language.as_str() {
            l @ ("en" | "ar") => l.to_string(),
            _ => self.system_lang.lock().clone(),
        }
    }

    /// Records the language the desktop UI resolved for "system". Anything
    /// other than "ar" is English.
    pub fn set_system_language(&self, lang: &str) {
        *self.system_lang.lock() = if lang == "ar" { "ar".into() } else { "en".into() };
    }

    /// Applies a JSON merge-patch, validates and persists.
    pub fn patch(&self, patch: serde_json::Value) -> Result<Settings> {
        let mut guard = self.current.lock();
        let mut value = serde_json::to_value(&*guard).map_err(|e| Error::Other(e.to_string()))?;
        match (value.as_object_mut(), patch.as_object()) {
            (Some(obj), Some(p)) => {
                for (k, v) in p {
                    obj.insert(k.clone(), v.clone());
                }
            }
            _ => return Err(Error::invalid("settings patch must be an object")),
        }
        let next: Settings = serde_json::from_value(value).map_err(|e| Error::invalid(e.to_string()))?;
        let next = next.validate()?;
        let bytes = serde_json::to_vec_pretty(&next).map_err(|e| Error::Other(e.to_string()))?;
        fsutil::write_atomic(&self.path, &bytes)?;
        *guard = next.clone();
        Ok(next)
    }

    pub fn add_trusted(&self, id: &str, name: &str) -> Result<Settings> {
        let mut list = self.get().trusted_devices;
        list.retain(|d| d.id != id);
        list.push(TrustedDevice { id: id.into(), name: name.into(), added_at: crate::model::now_ms() });
        self.patch(serde_json::json!({ "trustedDevices": list }))
    }

    pub fn is_trusted(&self, id: &str) -> bool {
        self.current.lock().trusted_devices.iter().any(|d| d.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_persists_and_validates() {
        let d = tempfile::tempdir().unwrap();
        let s = SettingsStore::load(d.path());
        let next = s.patch(serde_json::json!({"language":"ar","chunkSizeMib":999})).unwrap();
        assert_eq!(next.language, "ar");
        assert_eq!(next.chunk_size_mib, 16);
        assert!(s.patch(serde_json::json!({"language":"xx"})).is_err());
        assert!(s.patch(serde_json::json!({"preferredPort":80})).is_err());
        let again = SettingsStore::load(d.path());
        assert_eq!(again.get().language, "ar");
        assert_eq!(again.get().chunk_size_mib, 16);
    }
}
