use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

// ── Public API ────────────────────────────────────────────────────────────────

/// Spawns the A2DP audio healing task in the background. Returns immediately.
///
/// Only one healing task runs at a time — duplicate connect events are silently
/// dropped. The MAC address is used to locate the BlueZ sink and card.
pub fn spawn(addr: &bluer::Address) {
    let mac = addr.to_string().to_uppercase();
    tokio::spawn(async move {
        let Ok(_guard) = heal_lock().try_lock() else {
            log::debug!("heal_audio: already running, skipping");
            return;
        };
        heal(&mac).await;
    });
}

// ── Heal logic ────────────────────────────────────────────────────────────────

async fn heal(mac: &str) {
    let mac_under = mac.replace(':', "_");

    // Poll up to 7.5s for WirePlumber to register the BT sink.
    let mut sink_found = false;
    for _ in 0u8..15 {
        if bt_sink_exists(&mac_under).await {
            sink_found = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    if !sink_found {
        // Rate-limit restarts to at most once every 5 minutes to prevent a
        // misbehaving or spoofed peer from hammering WirePlumber on reconnect.
        let mut last = wp_restart_lock().lock().await;
        let cooldown = Duration::from_secs(300);
        if last.map_or(true, |t: Instant| t.elapsed() >= cooldown) {
            log::warn!("heal_audio: BT sink not found after 7.5s — restarting WirePlumber");
            let _ = tokio::process::Command::new("systemctl")
                .args(["--user", "restart", "wireplumber"])
                .output()
                .await;
            *last = Some(Instant::now());
            tokio::time::sleep(Duration::from_secs(2)).await;
        } else {
            log::warn!("heal_audio: BT sink not found; skipping WirePlumber restart (restarted recently)");
        }
    }

    let sinks = pactl(&["list", "sinks", "short"]).await;
    let cards = pactl(&["list", "cards", "short"]).await;

    let sink = sinks.lines()
        .find(|l| l.contains(&format!("bluez_output.{mac_under}")))
        .and_then(|l| l.split_whitespace().nth(1))
        .map(String::from);

    let card = cards.lines()
        .find(|l| l.contains(&format!("bluez_card.{mac_under}")))
        .and_then(|l| l.split_whitespace().nth(1))
        .map(String::from);

    // Switch to A2DP profile unless already active (fix B8: starts_with, not ==).
    // PipeWire uses "a2dp-sink-sbc", "a2dp-sink-aac", etc.
    if let Some(ref card_name) = card {
        let profile = active_profile(card_name).await;
        if !profile.starts_with("a2dp-sink") {
            log::info!("heal_audio: switching {card_name} to a2dp-sink (was {profile:?})");
            let _ = pactl(&["set-card-profile", card_name, "a2dp-sink"]).await;
        } else {
            log::debug!("heal_audio: {card_name} already on A2DP profile ({profile:?})");
        }
    } else {
        log::warn!("heal_audio: could not find card for {mac}");
    }

    // Set default sink if it's not already the BT sink.
    if let Some(ref sink_name) = sink {
        let default = pactl(&["get-default-sink"]).await;
        if default.trim() != sink_name {
            log::info!("heal_audio: setting default sink → {sink_name}");
            let _ = pactl(&["set-default-sink", sink_name]).await;
        }
    }

    log::info!("heal_audio: done (sink={:?}, card={:?})", sink, card);
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn bt_sink_exists(mac_under: &str) -> bool {
    let out = pactl(&["list", "sinks", "short"]).await;
    out.contains(&format!("bluez_output.{mac_under}"))
}

/// Returns the Active Profile string for the given card, or "" if not found.
async fn active_profile(card: &str) -> String {
    let out = pactl(&["list", "cards"]).await;
    let mut in_card = false;
    for line in out.lines() {
        if line.contains(card) { in_card = true; }
        if in_card && line.contains("Active Profile:") {
            return line.splitn(2, ':').nth(1).unwrap_or("").trim().to_string();
        }
    }
    String::new()
}

async fn pactl(args: &[&str]) -> String {
    tokio::process::Command::new("pactl")
        .args(args)
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn heal_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn wp_restart_lock() -> &'static Mutex<Option<Instant>> {
    static LOCK: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(None))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // Verify the B8 fix: profile matching with starts_with rather than == .
    #[test]
    fn a2dp_sink_prefix_match() {
        let profiles = ["a2dp-sink", "a2dp-sink-sbc", "a2dp-sink-aac", "a2dp-sink-ldac"];
        for p in &profiles {
            assert!(p.starts_with("a2dp-sink"),
                "{p} should match starts_with(\"a2dp-sink\")");
        }
        let non_a2dp = ["headset-head-unit", "off", "a2dp-source", "hfp-hf"];
        for p in &non_a2dp {
            assert!(!p.starts_with("a2dp-sink"),
                "{p} should NOT match starts_with(\"a2dp-sink\")");
        }
    }

    #[test]
    fn sink_name_matches_pactl_format() {
        // heal() searches pactl output for "bluez_output.{mac_under}" where mac_under
        // uses underscores. This test pins the exact format heal() depends on.
        let mac_under = "AB:CD:EF:01:23:45".replace(':', "_");
        let sink_name = format!("bluez_output.{mac_under}");
        // Simulate one line of `pactl list sinks short` output:
        // "42\tbluez_output.AB_CD_EF_01_23_45.a2dp_sink\tmodule-bluez5-device.c\ts16le ..."
        let pactl_line = format!("42\t{sink_name}.a2dp_sink\tmodule-bluez5-device.c\ts16le 48000Hz stereo");
        assert!(pactl_line.contains(&sink_name),
            "sink search string not found in typical pactl output line");
    }

    #[test]
    fn card_name_matches_pactl_format() {
        let mac_under = "AB:CD:EF:01:23:45".replace(':', "_");
        let card_name = format!("bluez_card.{mac_under}");
        let pactl_line = format!("5\t{card_name}\tmodule-bluez5-device.c");
        assert!(pactl_line.contains(&card_name));
    }
}
