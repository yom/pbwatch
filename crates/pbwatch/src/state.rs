use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;
use pbcommon::{BudsState, runtime_dir};

pub struct StateWriter {
    dir:     PathBuf,
    mac_key: String,   // e.g. "AB_CD_EF_01_23_45"
    legacy:  bool,
}

/// Converts a `bluer::Address` to a MAC key string (e.g. `"AB_CD_EF_01_23_45"`).
pub fn addr_to_mac_key(addr: &bluer::Address) -> String {
    addr.to_string().to_uppercase().replace(':', "_")
}

impl StateWriter {
    pub fn new(legacy: bool, addr: &bluer::Address) -> anyhow::Result<Self> {
        let mac_key = addr_to_mac_key(addr);
        let dir = runtime_dir("pbwatch");
        fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create runtime dir {dir:?}"))?;
        Ok(Self { dir, mac_key, legacy })
    }

    pub fn state_path(&self) -> PathBuf {
        self.dir.join(format!("state-{}.json", self.mac_key))
    }

    /// Atomically writes state-{mac}.json, and optionally the legacy battery/anc files.
    pub fn write(&self, state: &BudsState) -> anyhow::Result<()> {
        let json = serde_json::to_vec_pretty(state)
            .context("failed to serialize BudsState")?;
        atomic_write(&self.state_path(), &json)?;

        if self.legacy {
            self.write_legacy_battery(state)?;
            self.write_legacy_anc(state)?;
        }
        Ok(())
    }

    /// Deletes the state files at the start of a new connection attempt.
    pub fn reset(&self) {
        let _ = fs::remove_file(self.state_path());
        if self.legacy {
            let _ = fs::remove_file(self.dir.join(format!("battery-{}", self.mac_key)));
            let _ = fs::remove_file(self.dir.join(format!("anc-{}", self.mac_key)));
        }
    }

    fn write_legacy_battery(&self, s: &BudsState) -> anyhow::Result<()> {
        let fmt_pct = |v: Option<u8>| v.map(|p| p as i32).unwrap_or(-1);
        let content = format!(
            "left={}\nleft_charging={}\nright={}\nright_charging={}\ncase={}\ncase_charging={}\n",
            fmt_pct(s.left),  s.left_charging,
            fmt_pct(s.right), s.right_charging,
            fmt_pct(s.case),  s.case_charging,
        );
        atomic_write(&self.dir.join(format!("battery-{}", self.mac_key)), content.as_bytes())
    }

    fn write_legacy_anc(&self, s: &BudsState) -> anyhow::Result<()> {
        let supported: Vec<&str> = s.anc_supported.iter().map(|m| m.as_key()).collect();
        let content = format!("{}\n{}", s.anc.as_key(), supported.join(","));
        atomic_write(&self.dir.join(format!("anc-{}", self.mac_key)), content.as_bytes())
    }
}

/// Writes `data` to `path` atomically (write to `.tmp` then rename).
fn atomic_write(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)
            .with_context(|| format!("cannot create {tmp:?}"))?;
        f.write_all(data)
            .with_context(|| format!("cannot write {tmp:?}"))?;
        f.flush()?;
    }
    fs::rename(&tmp, path)
        .with_context(|| format!("cannot rename {tmp:?} → {path:?}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pbcommon::{AncMode, Event};
    use tempfile::TempDir;

    fn writer_in(dir: &Path, legacy: bool) -> StateWriter {
        StateWriter { dir: dir.to_path_buf(), mac_key: "AB_CD_EF_01_23_45".into(), legacy }
    }

    fn sample_state() -> BudsState {
        BudsState {
            schema: 1,
            event: Event::Update,
            left: Some(85), right: Some(87), case: Some(34),
            left_charging: false, right_charging: false, case_charging: true,
            anc: AncMode::NoiseCancellation,
            anc_supported: vec![AncMode::NoiseCancellation, AncMode::Off, AncMode::Transparency],
            mac: Some("AB:CD:EF:01:23:45".into()),
            ..Default::default()
        }
    }

    #[test]
    fn write_state_json_atomic() {
        let dir = TempDir::new().unwrap();
        let w = writer_in(dir.path(), false);
        let s = sample_state();
        w.write(&s).unwrap();

        let json = fs::read_to_string(dir.path().join("state-AB_CD_EF_01_23_45.json")).unwrap();
        let back: BudsState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);

        // with_extension("tmp") replaces the last extension: "…json" → "…tmp"
        assert!(!dir.path().join("state-AB_CD_EF_01_23_45.tmp").exists());
    }

    #[test]
    fn write_legacy_battery_format() {
        let dir = TempDir::new().unwrap();
        let w = writer_in(dir.path(), true);
        let s = sample_state();
        w.write(&s).unwrap();

        let bat = fs::read_to_string(dir.path().join("battery-AB_CD_EF_01_23_45")).unwrap();
        assert!(bat.contains("left=85\n"));
        assert!(bat.contains("right=87\n"));
        assert!(bat.contains("case=34\n"));
        assert!(bat.contains("case_charging=true\n"));
    }

    #[test]
    fn write_legacy_battery_absent_bud() {
        let dir = TempDir::new().unwrap();
        let w = writer_in(dir.path(), true);
        let mut s = sample_state();
        s.left = None;
        w.write(&s).unwrap();

        let bat = fs::read_to_string(dir.path().join("battery-AB_CD_EF_01_23_45")).unwrap();
        assert!(bat.contains("left=-1\n"));
    }

    #[test]
    fn write_legacy_anc_format() {
        let dir = TempDir::new().unwrap();
        let w = writer_in(dir.path(), true);
        w.write(&sample_state()).unwrap();

        let anc = fs::read_to_string(dir.path().join("anc-AB_CD_EF_01_23_45")).unwrap();
        let lines: Vec<&str> = anc.lines().collect();
        assert_eq!(lines[0], "noise_cancellation");
        assert!(lines.get(1).is_some());
    }

    #[test]
    fn write_legacy_anc_unknown_still_written() {
        let dir = TempDir::new().unwrap();
        let w = writer_in(dir.path(), true);
        let mut s = sample_state();
        s.anc = AncMode::Unknown;
        s.anc_supported = vec![];
        w.write(&s).unwrap();

        let anc = fs::read_to_string(dir.path().join("anc-AB_CD_EF_01_23_45")).unwrap();
        assert!(anc.starts_with("unknown"));
    }

    #[test]
    fn reset_removes_files() {
        let dir = TempDir::new().unwrap();
        let w = writer_in(dir.path(), true);
        w.write(&sample_state()).unwrap();
        w.reset();

        assert!(!dir.path().join("state-AB_CD_EF_01_23_45.json").exists());
        assert!(!dir.path().join("battery-AB_CD_EF_01_23_45").exists());
        assert!(!dir.path().join("anc-AB_CD_EF_01_23_45").exists());
    }
}
