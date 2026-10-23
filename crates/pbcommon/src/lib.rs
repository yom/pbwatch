use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AncMode {
    NoiseCancellation,
    Transparency,
    Adaptive,
    Off,
    #[default]
    Unknown,
}

impl AncMode {
    /// Returns the canonical snake_case key used in state files and sprite lookups.
    pub fn as_key(self) -> &'static str {
        match self {
            AncMode::NoiseCancellation => "noise_cancellation",
            AncMode::Transparency      => "transparency",
            AncMode::Adaptive          => "adaptive",
            AncMode::Off               => "off",
            AncMode::Unknown           => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Event {
    Connect,
    #[default]
    Update,
    Disconnect,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BudsState {
    /// Format version — always 1.
    pub schema: u32,
    pub event: Event,
    /// Bluetooth MAC address, uppercase (e.g. "AB:CD:EF:01:23:45").
    pub mac: Option<String>,
    /// Fast Pair Model ID (from 0x03/0x01 GFPS message or BlueZ Modalias).
    pub model_id: Option<u32>,
    /// Human-readable model name derived from model_id.
    pub model_name: Option<String>,
    /// BlueZ Device1.Alias (or DEVICE_LABEL from config).
    pub device_name: Option<String>,
    /// Battery percentage 0-100, or None when the bud is in the case.
    pub left: Option<u8>,
    pub right: Option<u8>,
    pub case: Option<u8>,
    pub left_charging: bool,
    pub right_charging: bool,
    pub case_charging: bool,
    pub anc: AncMode,
    pub anc_supported: Vec<AncMode>,
}

/// Returns `$XDG_RUNTIME_DIR/{sub}`, falling back to `/run/user/{uid}/{sub}`.
pub fn runtime_dir(sub: &str) -> std::path::PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
    std::path::PathBuf::from(base).join(sub)
}
