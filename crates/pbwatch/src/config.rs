use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Context;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Config {
    /// Bluetooth MAC addresses of the Pixel Buds to monitor (e.g. ["AB:CD:EF:01:23:45"]).
    pub macs:          Vec<String>,
    /// Switch to A2DP (high-quality audio) profile on each connect.
    pub heal_audio:    bool,
    pub device_label:  String,
    pub bg_color:        String,
    pub text_color:      String,
    #[serde(alias = "accent_color")]
    pub highlight_color: String,
    pub ring_color:      String,
    pub warn_color:      String,
    pub critical_color:  String,
    pub opacity:         f64,
    pub auto_close:      u32,
    pub image_size:      u32,
    pub battery_warn:    u8,
    pub battery_critical: u8,
    pub anc_adaptive:  String,
    pub sprite_set:    Option<String>,
    /// Show battery/ANC text alongside the widget image in notifications.
    pub show_text:     bool,
    /// Named color theme (e.g. "Nord", "Dracula"). Overrides all six colour fields when set.
    pub theme:         Option<String>,
    /// Font size as a percentage of normal (100 = default, 120 = 20% larger).
    pub font_size:     u32,

    // Deprecated single-MAC field from older configs. Read-only: auto-migrated
    // into `macs` by validate_and_clamp, never written back.
    #[serde(skip_serializing)]
    device_mac: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            macs:          vec![],
            heal_audio:    false,
            device_label:  String::new(),
            bg_color:        "#1e1e2e".into(),
            text_color:      "#cdd6f4".into(),
            highlight_color: "#89b4fa".into(),
            ring_color:      "#ffffff20".into(),
            warn_color:      "#f9e2af".into(),
            critical_color:  "#f38ba8".into(),
            opacity:         0.0,
            auto_close:      5000,
            image_size:      200,
            battery_warn:    50,
            battery_critical: 10,
            anc_adaptive:  "auto".into(),
            sprite_set:    None,
            show_text:     false,
            theme:         None,
            font_size:     100,
            device_mac:    String::new(),
        }
    }
}

impl Config {
    /// Loads config: TOML path first, then legacy paths.
    pub fn load() -> anyhow::Result<Self> {
        let toml = Self::toml_path();
        if toml.exists() {
            return Self::load_from(&toml);
        }
        // Migrate transparently from old ~/.config/pbwidget/config.toml
        let old_toml = Self::legacy_toml_path();
        if old_toml.exists() {
            log::info!("config: migrating {:?} → {:?}", old_toml, toml);
            if let Some(parent) = toml.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::copy(&old_toml, &toml).is_ok() {
                return Self::load_from(&toml);
            }
            return Self::load_from(&old_toml);
        }
        let legacy = Self::legacy_path();
        if legacy.exists() {
            log::warn!("config: ~/.pbwidget is deprecated; move it to {:?}", toml);
            let mut cfg = Self::load_legacy(&legacy)?;
            cfg.validate_and_clamp();
            return Ok(cfg);
        }
        Ok(Self::default())
    }

    /// Loads and parses a TOML config file.
    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        let mut src = String::new();
        std::fs::File::open(path)
            .with_context(|| format!("cannot open {path:?}"))?
            .read_to_string(&mut src)
            .with_context(|| format!("cannot read {path:?}"))?;
        let mut cfg: Config = toml::from_str(&src)
            .with_context(|| format!("cannot parse {path:?}"))?;
        cfg.validate_and_clamp();
        Ok(cfg)
    }

    /// Parses the legacy env-file format (`~/.pbwidget`).
    pub fn load_legacy(path: &Path) -> anyhow::Result<Self> {
        let mut src = String::new();
        std::fs::File::open(path)
            .with_context(|| format!("cannot open {path:?}"))?
            .read_to_string(&mut src)
            .with_context(|| format!("cannot read {path:?}"))?;

        let mut cfg = Config::default();
        for line in src.lines() {
            let line = line.trim();
            // Skip comments and blank lines
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, val)) = line.split_once('=') else { continue };
            let key = key.trim().to_ascii_uppercase();
            let key = key.as_str();
            // Strip inline comments preceded by whitespace (preserves `#ff0000`).
            let val = {
                let b = val.as_bytes();
                let end = (1..b.len())
                    .find(|&i| b[i] == b'#' && b[i-1].is_ascii_whitespace())
                    .unwrap_or(b.len());
                val[..end].trim()
            };
            let val = val.trim_matches('"').trim_matches('\'');

            match key {
                "BG_COLOR"         => cfg.bg_color       = val.into(),
                "TEXT_COLOR"       => cfg.text_color      = val.into(),
                "ACCENT_COLOR" | "HIGHLIGHT_COLOR" => cfg.highlight_color = val.into(),
                "RING_COLOR"       => cfg.ring_color      = val.into(),
                "ANC_ADAPTIVE"     => cfg.anc_adaptive    = val.to_ascii_lowercase(),
                "DEVICE_MAC"       => cfg.macs.push(val.into()),
                "DEVICE_LABEL"     => cfg.device_label    = val.into(),
                "OPACITY"          => {
                    if let Ok(v) = val.parse() { cfg.opacity = v; }
                }
                "AUTO_CLOSE"       => {
                    if let Ok(v) = val.parse() { cfg.auto_close = v; }
                }
                "IMAGE_SIZE"       => {
                    if let Ok(v) = val.parse() { cfg.image_size = v; }
                }
                "BATTERY_WARN"     => {
                    if let Ok(v) = val.parse() { cfg.battery_warn = v; }
                }
                "BATTERY_CRITICAL" => {
                    if let Ok(v) = val.parse() { cfg.battery_critical = v; }
                }
                _ => {}
            }
        }
        Ok(cfg)
    }

    /// Clamps out-of-range values and migrates deprecated fields.
    pub fn validate_and_clamp(&mut self) {
        // Migrate deprecated single-MAC field into macs list.
        if self.macs.is_empty() && !self.device_mac.is_empty() {
            log::warn!(
                "config: 'device_mac' is deprecated; \
                 replace it with: macs = [\"{}\"]",
                self.device_mac
            );
            self.macs.push(std::mem::take(&mut self.device_mac));
        }
        self.opacity = self.opacity.clamp(0.0, 1.0);
        self.image_size = self.image_size.clamp(100, 1024);
        if self.battery_warn > 100 {
            self.battery_warn = 100;
        }
        if self.battery_critical >= self.battery_warn {
            self.battery_critical = self.battery_warn.saturating_sub(1);
        }
        if let Some(ref key) = self.sprite_set {
            const VALID: &[&str] = &["default", "pro", "pro2", "a-series"];
            if !VALID.contains(&key.as_str()) {
                log::warn!("config: unknown sprite_set {:?} (valid: {})", key,
                    VALID.join(", "));
            }
        }
        if self.font_size == 0 { self.font_size = 100; }
        self.font_size = self.font_size.min(300);
        if let Some(name) = self.theme.clone() {
            match resolve_theme(&name) {
                Some(t) => {
                    self.bg_color        = t.bg_color;
                    self.text_color      = t.text_color;
                    self.highlight_color = t.highlight_color;
                    self.ring_color      = t.ring_color;
                    self.warn_color      = t.warn_color;
                    self.critical_color  = t.critical_color;
                }
                None => log::warn!("config: theme {:?} not found", name),
            }
        }
    }

    /// `~/.config/pbwatch/config.toml`
    pub fn toml_path() -> PathBuf {
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs_home().join(".config"));
        base.join("pbwatch").join("config.toml")
    }

    /// `~/.config/pbwidget/config.toml` (old path, migrated automatically)
    pub fn legacy_toml_path() -> PathBuf {
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs_home().join(".config"));
        base.join("pbwidget").join("config.toml")
    }

    /// `~/.pbwidget` (legacy env-file format)
    pub fn legacy_path() -> PathBuf {
        dirs_home().join(".pbwidget")
    }
}

// ── Theme resolution ──────────────────────────────────────────────────────────

#[derive(serde::Deserialize, Clone)]
struct Theme {
    name:            String,
    bg_color:        String,
    text_color:      String,
    #[serde(alias = "accent_color")]
    highlight_color: String,
    ring_color:      String,
    warn_color:      String,
    critical_color:  String,
}

#[derive(serde::Deserialize)]
struct ThemeFile {
    themes: Vec<Theme>,
}

/// Looks up a theme by name (case-insensitive).
/// Tries /usr/share/pbwatch/themes.json first, then falls back to embedded themes.
fn resolve_theme(name: &str) -> Option<Theme> {
    let system = std::path::Path::new("/usr/share/pbwatch/themes.json");
    if system.exists() {
        if let Ok(data) = std::fs::read_to_string(system) {
            if let Ok(f) = serde_json::from_str::<ThemeFile>(&data) {
                if let Some(t) = find_theme(&f.themes, name) {
                    return Some(t);
                }
            }
        }
    }
    let embedded = include_str!("../assets/themes.json");
    let f: ThemeFile = serde_json::from_str(embedded).expect("embedded themes.json is valid");
    find_theme(&f.themes, name)
}

fn find_theme(themes: &[Theme], name: &str) -> Option<Theme> {
    themes.iter().find(|t| t.name.eq_ignore_ascii_case(name)).cloned()
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn default_roundtrips_toml() {
        let cfg = Config::default();
        let serialized = toml::to_string(&cfg).expect("serialize");
        let back: Config = toml::from_str(&serialized).expect("deserialize");
        assert_eq!(cfg.bg_color,         back.bg_color);
        assert_eq!(cfg.battery_warn,     back.battery_warn);
        assert_eq!(cfg.battery_critical, back.battery_critical);
        assert_eq!(cfg.image_size,       back.image_size);
    }

    #[test]
    fn load_from_toml_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, r##"
bg_color = "#ff0000"
image_size = 400
battery_warn = 30
battery_critical = 5
"##).unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.bg_color, "#ff0000");
        assert_eq!(cfg.image_size, 400);
        assert_eq!(cfg.battery_warn, 30);
        assert_eq!(cfg.battery_critical, 5);
        // unset fields keep defaults
        assert_eq!(cfg.text_color, "#cdd6f4");
    }

    #[test]
    fn load_legacy_format() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".pbwidget");
        fs::write(&path, "BG_COLOR=#123456\nTEXT_COLOR=\"#abcdef\"\nOPACITY=0.85\n").unwrap();
        let cfg = Config::load_legacy(&path).unwrap();
        assert_eq!(cfg.bg_color, "#123456");
        assert_eq!(cfg.text_color, "#abcdef");
        assert!((cfg.opacity - 0.85).abs() < 1e-9);
        // unset fields keep defaults
        assert_eq!(cfg.image_size, 200);
    }

    #[test]
    fn image_size_clamped() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "image_size = 50\n").unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.image_size, 100);
    }

    #[test]
    fn battery_critical_clamped() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        // critical >= warn → critical = warn - 1
        fs::write(&path, "battery_warn = 30\nbattery_critical = 30\n").unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert!(cfg.battery_critical < cfg.battery_warn,
            "critical={} warn={}", cfg.battery_critical, cfg.battery_warn);
    }

    #[test]
    fn font_size_clamped() {
        let dir = TempDir::new().unwrap();
        let zero_path = dir.path().join("zero.toml");
        fs::write(&zero_path, "font_size = 0\n").unwrap();
        assert_eq!(Config::load_from(&zero_path).unwrap().font_size, 100);

        let huge_path = dir.path().join("huge.toml");
        fs::write(&huge_path, "font_size = 500\n").unwrap();
        assert_eq!(Config::load_from(&huge_path).unwrap().font_size, 300);

        let ok_path = dir.path().join("ok.toml");
        fs::write(&ok_path, "font_size = 120\n").unwrap();
        assert_eq!(Config::load_from(&ok_path).unwrap().font_size, 120);
    }

    #[test]
    fn device_mac_migrated_to_macs() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, r#"device_mac = "AB:CD:EF:01:23:45""#).unwrap();
        let cfg = Config::load_from(&path).unwrap();
        // device_mac must be migrated into the macs list
        assert!(cfg.macs.iter().any(|m| m == "AB:CD:EF:01:23:45"),
            "device_mac not migrated: {:?}", cfg.macs);
    }

    #[test]
    fn legacy_inline_comment_stripped() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".pbwidget");
        fs::write(&path, "BG_COLOR=#ff0000 # some comment\nTEXT_COLOR=#123456\n").unwrap();
        let cfg = Config::load_legacy(&path).unwrap();
        assert_eq!(cfg.bg_color, "#ff0000", "inline comment not stripped from value");
        assert_eq!(cfg.text_color, "#123456");
    }
}
