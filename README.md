# pbwatch

**Your Pixel Buds on Linux, properly.**

[![Latest release](https://img.shields.io/github/v/release/yom/pbwatch)](https://github.com/yom/pbwatch/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Rust ≥ 1.85](https://img.shields.io/badge/rust-%E2%89%A5_1.85-orange)](https://www.rust-lang.org/tools/install)
[![Platform: Linux](https://img.shields.io/badge/platform-Linux-lightgrey)](https://github.com/yom/pbwatch)

Battery and ANC status for Google Pixel Buds on the Linux desktop, shown as a notification whenever something changes. No phone, no app, no cloud.

![Pixel Buds Pro 2 connected, noise cancellation on, left 85%, case 34%, right 87%](images/example.png)

## Why

Android shows battery and ANC state in a system popup. On Linux, BlueZ reports at best a single combined battery number — and no ANC state at all. pbwatch fixes that by speaking Google's Fast Pair protocol directly over Bluetooth, the same way the Android app does.

## How it works

pbwatch connects to your earbuds over a Bluetooth RFCOMM channel and reads the [Fast Pair Message Stream protocol](REFERENCES.md). It draws the widget locally with Cairo and sends a standard freedesktop desktop notification. No network, no Google account, no root.

## Features

- Battery levels for left bud, right bud, and case
- Active ANC mode (Noise Cancellation, Off, Transparency, Adaptive)
- 20 built-in colour themes — or bring your own
- Monitors multiple pairs simultaneously
- Single binary — draws its own widget, no icon themes needed

## Supported devices

| Model | Battery | ANC mode |
|---|---|---|
| Pixel Buds Pro 2 (2024) | ✓ | ✓ |
| Pixel Buds Pro (2022) | ✓ | ✓ |
| Pixel Buds A-Series (2021) | ✓ | — |
| Pixel Buds (2020) | untested | untested |
| Pixel Buds (2017, 1st gen) | ✗ | ✗ |

✓ confirmed · — hardware doesn't support this · ✗ not supported · untested = reports welcome

## Install

### Option A — Debian / Ubuntu package

Download `pbwatch_1.0.0_amd64.deb` from the [latest release](https://github.com/yom/pbwatch/releases/latest), then:

```bash
sudo apt install ./pbwatch_1.0.0_amd64.deb
```

After installing, complete the setup:

**1. Find your Pixel Buds Bluetooth address:**

```bash
bluetoothctl devices
```

**2. Create the config file:**

```bash
mkdir -p ~/.config/pbwatch
echo 'macs = ["AB:CD:EF:01:23:45"]' > ~/.config/pbwatch/config.toml
```

Replace `AB:CD:EF:01:23:45` with your earbuds' address. A fully commented example is at `/usr/share/doc/pbwatch/examples/config.toml`.

**3. Start the service:**

```bash
systemctl --user enable --now pbwatch
```

To update: download the new `.deb` and run `sudo apt install ./pbwatch_X.Y.Z_amd64.deb` again.

---

### Option B — Build from source

For other Linux distributions, or if you want the latest code.

**Requirements:** BlueZ (already installed on most distros), a notification daemon (dunst, mako, SwayNC, GNOME, KDE…).

<details>
<summary>Don't have Rust? Install it first (Rust ≥ 1.85 required)</summary>

```bash
# Debian 13 / Ubuntu 24.04+
sudo apt install rustup && rustup default stable

# Fedora
sudo dnf install rust cargo

# Arch
sudo pacman -S rust

# Any distro — official installer
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Full instructions: [rust-lang.org/tools/install](https://www.rust-lang.org/tools/install)

Check your version: `rustc --version` (needs ≥ 1.85)

</details>

**1. Find your Pixel Buds Bluetooth address:**

```bash
bluetoothctl devices
```

**2. Clone and install:**

```bash
git clone https://github.com/yom/pbwatch.git
cd pbwatch
./install.sh --mac AB:CD:EF:01:23:45
```

The script compiles the binary, installs it, and starts the background service. The MAC address is saved to `~/.config/pbwatch/config.toml`.

To update: `git pull` and run `./install.sh` again — the MAC address is kept from the config.

#### Multiple pairs

Repeat `--mac` for each pair:

```bash
./install.sh --mac AB:CD:EF:01:23:45 --mac 12:34:56:78:9A:BC
```

You can also add pairs later by editing `~/.config/pbwatch/config.toml`:

```toml
macs = ["AB:CD:EF:01:23:45", "12:34:56:78:9A:BC"]
```

Then restart: `systemctl --user restart pbwatch`

#### Audio doesn't switch to the earbuds automatically?

Add `--heal-audio` to the install command:

```bash
./install.sh --mac AB:CD:EF:01:23:45 --heal-audio
```

On each connect, pbwatch switches the Bluetooth card to the A2DP (high-quality audio) profile and sets it as the default audio output. Requires `pactl` and `wpctl`:

```bash
# Debian/Ubuntu
sudo apt install pulseaudio-utils wireplumber

# Fedora
sudo dnf install pulseaudio-utils wireplumber

# Arch
sudo pacman -S libpulse wireplumber
```

## Configuration

All configuration lives in `~/.config/pbwatch/config.toml`. Edit it at any time; restart pbwatch to apply:

```bash
systemctl --user restart pbwatch
```

For `.deb` users, a fully commented example is at `/usr/share/doc/pbwatch/examples/config.toml`. See also `man pbwatch`.

### Example

```toml
macs = ["AB:CD:EF:01:23:45"]
theme = "Nord"
auto_close = 5000
heal_audio = false
```

### General

| Setting | Default | Description |
|---|---|---|
| `macs` | *(required)* | Bluetooth addresses to monitor, e.g. `["AB:CD:EF:01:23:45"]` |
| `device_label` | *(empty)* | Name shown at top of widget (defaults to Bluetooth device name) |
| `heal_audio` | `false` | Switch to A2DP profile on each connect |
| `auto_close` | `5000` | Notification duration in ms (`0` = until dismissed) |
| `show_text` | `false` | Include battery/ANC text alongside the widget image |

### Appearance

| Setting | Default | Description |
|---|---|---|
| `theme` | *(none)* | Named colour theme, e.g. `"Nord"`, `"Dracula"` |
| `bg_color` | `"#1e1e2e"` | Background colour |
| `text_color` | `"#cdd6f4"` | Text colour |
| `highlight_color` | `"#89b4fa"` | Gauge arc (normal charge) and active ANC mode |
| `warn_color` | `"#f9e2af"` | Gauge arc below `battery_warn` threshold |
| `critical_color` | `"#f38ba8"` | Gauge arc below `battery_critical` threshold |
| `ring_color` | `"#ffffff20"` | Gauge track colour (`#RRGGBBAA`, last two digits = alpha) |
| `opacity` | `0.0` | Background opacity: `0.0` = transparent, `1.0` = solid |
| `image_size` | `200` | Widget width in pixels |
| `font_size` | `100` | Font size as a percentage (e.g. `120` = 20% larger) |

### Battery thresholds

| Setting | Default | Description |
|---|---|---|
| `battery_warn` | `50` | Gauge switches to `warn_color` below this % |
| `battery_critical` | `10` | Gauge switches to `critical_color` below this % |

### Advanced

| Setting | Default | Description |
|---|---|---|
| `anc_adaptive` | `"auto"` | Show Adaptive ANC button: `"auto"` (device-reported), `"true"` (always), `"false"` (never) |
| `sprite_set` | *(auto)* | Force sprite sheet: `"default"`, `"pro"`, `"pro2"`, `"a-series"` |

## Themes

Set `theme = "Nord"` (or any name below) in your config to switch colour schemes instantly.

The three battery gauges show the three colour states: left at 80% (`highlight_color`), case at 35% (`warn_color`), right at 5% (`critical_color`).

![Theme board — all 20 built-in themes](images/themes.png)

<details>
<summary>All 20 theme names</summary>

**Dark**

| | | |
|---|---|---|
| Solarized Dark | Gruvbox Dark | Catppuccin Mocha |
| Rosé Pine | Everforest Dark | Nord |
| Dracula | Tokyo Night | Kanagawa |
| SynthWave '84 | | |

**Light**

| | | |
|---|---|---|
| Solarized Light | Gruvbox Light | Catppuccin Latte |
| Rosé Pine Dawn | Everforest Light | Nord Light |
| GitHub Light | Ayu Light | One Light |
| PaperColor Light | | |

</details>

To use your own colours, leave `theme` unset and set the `*_color` fields directly.

## Troubleshooting

**Notifications stopped showing up.**

```bash
systemctl --user status pbwatch
journalctl --user -u pbwatch -n 30
```

Make sure your earbuds are paired and connected — and only to this device. If your phone is also connected, it may be holding the RFCOMM channel.

**The notification shows text but no image, or the image is tiny.**

Most notification daemons need a small config tweak to display the widget at full size.

For **dunst**, add this to `~/.config/dunst/dunstrc`:

```ini
[pbwatch]
    appname = pbwatch
    format = "%b"
    icon_position = left
    min_icon_size = 0
    max_icon_size = 320
```

For mako, SwayNC, and GNOME, see [`docs/notification-daemons/`](docs/notification-daemons/).

**The service doesn't start after login.**

```bash
loginctl enable-linger $USER
```

Needed on minimal desktop setups where no graphical session starts the systemd user instance automatically.

**Debug logging** (useful for compatibility reports):

```bash
RUST_LOG=debug journalctl --user -u pbwatch -f
```

## Uninstall

**If installed via `.deb`:**

```bash
sudo apt remove pbwatch
```

**If installed from source:**

```bash
systemctl --user disable --now pbwatch
sudo rm /usr/local/bin/pbwatch
rm ~/.config/systemd/user/pbwatch.service
```

Your config is kept at `~/.config/pbwatch/`. Remove it too if you want a clean slate:

```bash
rm -rf ~/.config/pbwatch
```

## Contributing

Bug reports, compatibility reports, and feature requests are welcome on the [GitHub Issues](https://github.com/yom/pbwatch/issues) page.

**Compatibility reports** for models listed as *untested* are especially helpful. To include useful diagnostic info, enable debug logging before connecting your earbuds:

```bash
RUST_LOG=debug journalctl --user -u pbwatch -f
```

Then paste the first few lines into your issue.

## License

[MIT](LICENSE) © 2024 Guillaume Blairon
