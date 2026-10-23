// ── Bundled SVG assets ─────────────────────────────────────────────────────────

mod bundled {
    pub fn battery(name: &str) -> Option<&'static [u8]> {
        Some(match name {
            "battery-level-0-symbolic"   => include_bytes!("../assets/icons/battery-level-0-symbolic.svg"),
            "battery-level-10-symbolic"  => include_bytes!("../assets/icons/battery-level-10-symbolic.svg"),
            "battery-level-20-symbolic"  => include_bytes!("../assets/icons/battery-level-20-symbolic.svg"),
            "battery-level-30-symbolic"  => include_bytes!("../assets/icons/battery-level-30-symbolic.svg"),
            "battery-level-40-symbolic"  => include_bytes!("../assets/icons/battery-level-40-symbolic.svg"),
            "battery-level-50-symbolic"  => include_bytes!("../assets/icons/battery-level-50-symbolic.svg"),
            "battery-level-60-symbolic"  => include_bytes!("../assets/icons/battery-level-60-symbolic.svg"),
            "battery-level-70-symbolic"  => include_bytes!("../assets/icons/battery-level-70-symbolic.svg"),
            "battery-level-80-symbolic"  => include_bytes!("../assets/icons/battery-level-80-symbolic.svg"),
            "battery-level-90-symbolic"  => include_bytes!("../assets/icons/battery-level-90-symbolic.svg"),
            "battery-level-100-symbolic" => include_bytes!("../assets/icons/battery-level-100-symbolic.svg"),
            "battery-level-0-charging-symbolic"  => include_bytes!("../assets/icons/battery-level-0-charging-symbolic.svg"),
            "battery-level-10-charging-symbolic" => include_bytes!("../assets/icons/battery-level-10-charging-symbolic.svg"),
            "battery-level-20-charging-symbolic" => include_bytes!("../assets/icons/battery-level-20-charging-symbolic.svg"),
            "battery-level-30-charging-symbolic" => include_bytes!("../assets/icons/battery-level-30-charging-symbolic.svg"),
            "battery-level-40-charging-symbolic" => include_bytes!("../assets/icons/battery-level-40-charging-symbolic.svg"),
            "battery-level-50-charging-symbolic" => include_bytes!("../assets/icons/battery-level-50-charging-symbolic.svg"),
            "battery-level-60-charging-symbolic" => include_bytes!("../assets/icons/battery-level-60-charging-symbolic.svg"),
            "battery-level-70-charging-symbolic" => include_bytes!("../assets/icons/battery-level-70-charging-symbolic.svg"),
            "battery-level-80-charging-symbolic" => include_bytes!("../assets/icons/battery-level-80-charging-symbolic.svg"),
            "battery-level-90-charging-symbolic" => include_bytes!("../assets/icons/battery-level-90-charging-symbolic.svg"),
            "battery-missing-symbolic"           => include_bytes!("../assets/icons/battery-missing-symbolic.svg"),
            _ => return None,
        })
    }

    pub fn ear() -> &'static [u8] {
        include_bytes!("../assets/icons/hearing-symbolic.svg")
    }
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Returns the icon name for a battery percentage.
/// Uses banker's rounding (round_ties_even) to snap to multiples of 10.
/// `pct=None` → `"battery-missing-symbolic"`.
pub fn battery_icon_name(pct: Option<u8>, charging: bool) -> String {
    let Some(p) = pct else {
        return "battery-missing-symbolic".into();
    };
    let level = (p as f64 / 10.0).round_ties_even() as u64 * 10;
    let level = level.min(100);
    if charging {
        format!("battery-level-{level}-charging-symbolic")
    } else {
        format!("battery-level-{level}-symbolic")
    }
}

/// Returns SVG bytes for a bundled battery icon by name, or `None` if unknown.
pub fn find_icon_bytes(name: &str) -> Option<Vec<u8>> {
    bundled::battery(name).map(|b| b.to_vec())
}

/// Returns SVG bytes for the ear/ANC icon (hearing-symbolic, always bundled).
pub fn find_ear_icon_bytes() -> Vec<u8> {
    bundled::ear().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_name_banker_rounding() {
        assert_eq!(battery_icon_name(Some(5),  false), "battery-level-0-symbolic");
        assert_eq!(battery_icon_name(Some(15), false), "battery-level-20-symbolic");
        assert_eq!(battery_icon_name(Some(25), false), "battery-level-20-symbolic");
        assert_eq!(battery_icon_name(Some(35), false), "battery-level-40-symbolic");
        assert_eq!(battery_icon_name(Some(45), false), "battery-level-40-symbolic");
        assert_eq!(battery_icon_name(Some(95), false), "battery-level-100-symbolic");
        assert_eq!(battery_icon_name(Some(85), false), "battery-level-80-symbolic");
        assert_eq!(battery_icon_name(Some(75), false), "battery-level-80-symbolic");
    }

    #[test]
    fn battery_name_none() {
        assert_eq!(battery_icon_name(None, false), "battery-missing-symbolic");
        assert_eq!(battery_icon_name(None, true),  "battery-missing-symbolic");
    }

    #[test]
    fn battery_name_charging() {
        assert_eq!(battery_icon_name(Some(80), true), "battery-level-80-charging-symbolic");
        assert_eq!(battery_icon_name(Some(50), true), "battery-level-50-charging-symbolic");
    }

    #[test]
    fn battery_name_exact_values() {
        assert_eq!(battery_icon_name(Some(0),   false), "battery-level-0-symbolic");
        assert_eq!(battery_icon_name(Some(100), false), "battery-level-100-symbolic");
        assert_eq!(battery_icon_name(Some(50),  false), "battery-level-50-symbolic");
    }

    #[test]
    fn bundled_battery_icons_all_present() {
        for name in [
            "battery-level-0-symbolic",   "battery-level-10-symbolic",
            "battery-level-20-symbolic",  "battery-level-30-symbolic",
            "battery-level-40-symbolic",  "battery-level-50-symbolic",
            "battery-level-60-symbolic",  "battery-level-70-symbolic",
            "battery-level-80-symbolic",  "battery-level-90-symbolic",
            "battery-level-100-symbolic",
            "battery-level-0-charging-symbolic",  "battery-level-10-charging-symbolic",
            "battery-level-20-charging-symbolic", "battery-level-30-charging-symbolic",
            "battery-level-40-charging-symbolic", "battery-level-50-charging-symbolic",
            "battery-level-60-charging-symbolic", "battery-level-70-charging-symbolic",
            "battery-level-80-charging-symbolic", "battery-level-90-charging-symbolic",
            "battery-missing-symbolic",
        ] {
            assert!(find_icon_bytes(name).is_some(), "missing bundled icon: {name}");
        }
    }

    #[test]
    fn bundled_battery_unknown_returns_none() {
        assert!(find_icon_bytes("battery-level-100-charging-symbolic").is_none());
        assert!(find_icon_bytes("nonexistent-icon").is_none());
    }

    #[test]
    fn bundled_ear_icon_is_svg() {
        let data = find_ear_icon_bytes();
        assert!(!data.is_empty());
        assert!(data.windows(4).any(|w| w == b"<svg"),
            "hearing-symbolic.svg should contain <svg");
    }
}
