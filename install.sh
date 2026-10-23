#!/usr/bin/env bash
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MACS=()
HEAL_AUDIO=false

info()  { echo "[info] $*"; }
warn()  { echo "[warn] $*" >&2; }
done_() { echo "[done] $*"; }
die()   { echo "[fail] $*" >&2; exit 1; }

usage() {
    echo "Usage: $0 [--mac AB:CD:EF:01:23:45] [--mac 12:34:56:78:9A:BC] [--heal-audio]"
    echo ""
    echo "  --mac AB:CD:EF:01:23:45   Pixel Buds Bluetooth address (repeatable for multiple pairs)"
    echo "                            Find yours with: bluetoothctl devices"
    echo "                            Omit to keep the address(es) already in the config."
    echo "  --heal-audio              Switch to A2DP profile on each connect"
    echo "                            (requires pulseaudio-utils + wireplumber)."
    echo "  -h, --help                Show this help."
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --mac)        MACS+=("${2:?'--mac requires an address'}"); shift 2 ;;
        --mac=*)      MACS+=("${1#--mac=}"); shift ;;
        --heal-audio) HEAL_AUDIO=true; shift ;;
        -h|--help)    usage; exit 0 ;;
        *)            echo "Unknown argument: $1" >&2; usage; exit 1 ;;
    esac
done

# ---------------------------------------------------------------------------
# 1. Build binaries
# ---------------------------------------------------------------------------
info "Building pbwatch..."

command -v cargo &>/dev/null || die "cargo not found. Install Rust: https://rustup.rs/"

(cd "$REPO" && cargo build --release 2>&1) || die "cargo build failed."
done_ "Build complete: target/release/pbwatch"

# ---------------------------------------------------------------------------
# 2. Install binary to /usr/local/bin/
# ---------------------------------------------------------------------------
info "Installing pbwatch to /usr/local/bin/ (requires sudo)..."

sudo install -m 0755 "$REPO/target/release/pbwatch" /usr/local/bin/ || die "sudo install failed."
sudo rm -f /usr/local/bin/pbwidget   # remove old binary if present
sudo install -d /usr/share/pbwatch
sudo install -m 0644 "$REPO/crates/pbwatch/assets/themes.json" /usr/share/pbwatch/ \
    || warn "Could not install themes.json to /usr/share/pbwatch/ — embedded themes will be used."
done_ "Binary and themes installed."

# ---------------------------------------------------------------------------
# 3. Config file: write MAC and heal_audio
# ---------------------------------------------------------------------------
CONFIG_DIR="$HOME/.config/pbwatch"
CONFIG_FILE="$CONFIG_DIR/config.toml"

# Migrate from old ~/.config/pbwidget/ location if needed.
# Note: config.rs::load() also handles this migration at runtime; doing it here
# as well means the migrated file is visible immediately after install.
OLD_CONFIG="$HOME/.config/pbwidget/config.toml"
if [[ ! -f "$CONFIG_FILE" ]] && [[ -f "$OLD_CONFIG" ]]; then
    mkdir -p "$CONFIG_DIR"
    cp "$OLD_CONFIG" "$CONFIG_FILE"
    info "Migrated config: $OLD_CONFIG → $CONFIG_FILE"
fi

mkdir -p "$CONFIG_DIR"

# On update with no --mac: try to read existing MACs from config, then fall back
# to the old service file for one-time migration.
if [[ ${#MACS[@]} -eq 0 ]]; then
    if [[ -f "$CONFIG_FILE" ]] && grep -q '^macs\s*=' "$CONFIG_FILE" 2>/dev/null; then
        info "Keeping existing MAC address(es) from $CONFIG_FILE"
    else
        SERVICE_DEST="$HOME/.config/systemd/user/pbwatch.service"
        if [[ -f "$SERVICE_DEST" ]]; then
            EXISTING_MAC="$(grep -oP '(?<=--mac )[0-9A-Fa-f:]{17}' "$SERVICE_DEST" || true)"
            if [[ -n "$EXISTING_MAC" ]]; then
                MACS+=("$EXISTING_MAC")
                info "Migrating MAC $EXISTING_MAC from old service file to $CONFIG_FILE"
            fi
            if grep -q -- '--heal-audio' "$SERVICE_DEST" 2>/dev/null; then
                HEAL_AUDIO=true
                info "Migrating --heal-audio from old service file to $CONFIG_FILE"
            fi
        fi
    fi
fi

# Validate MAC format (loose check)
for mac in "${MACS[@]}"; do
    if ! [[ "$mac" =~ ^([0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}$ ]]; then
        die "Invalid MAC address format: $mac  (expected AB:CD:EF:01:23:45)"
    fi
done

# Write MACs to config.toml — prepend at top so they never land inside a [section].
if [[ ${#MACS[@]} -gt 0 ]]; then
    macs_toml="macs = [$(printf '"%s", ' "${MACS[@]}" | sed 's/, $//')]"
    if [[ -f "$CONFIG_FILE" ]]; then
        { printf '%s\n' "$macs_toml"; grep -v '^macs\s*=' "$CONFIG_FILE"; } \
            > "$CONFIG_FILE.tmp" && mv "$CONFIG_FILE.tmp" "$CONFIG_FILE"
    else
        printf '%s\n' "$macs_toml" > "$CONFIG_FILE"
    fi
    info "MAC address(es) saved to $CONFIG_FILE"
fi

# Write heal_audio to config.toml if requested — prepend at top.
if [[ "$HEAL_AUDIO" == true ]]; then
    if [[ -f "$CONFIG_FILE" ]]; then
        { printf 'heal_audio = true\n'; grep -v '^heal_audio\s*=' "$CONFIG_FILE"; } \
            > "$CONFIG_FILE.tmp" && mv "$CONFIG_FILE.tmp" "$CONFIG_FILE"
    else
        printf 'heal_audio = true\n' > "$CONFIG_FILE"
    fi
    info "heal_audio = true saved to $CONFIG_FILE"
fi

HAS_MACS=false
if [[ -f "$CONFIG_FILE" ]] && grep -q '^macs\s*=' "$CONFIG_FILE" 2>/dev/null; then
    HAS_MACS=true
fi


# ---------------------------------------------------------------------------
# 4. Install systemd user service
# ---------------------------------------------------------------------------
info "Installing systemd user service..."

SERVICE_DEST="$HOME/.config/systemd/user/pbwatch.service"
mkdir -p "$HOME/.config/systemd/user"

# Remove old bt-pixel-buds.service if present.
if systemctl --user is-enabled bt-pixel-buds.service &>/dev/null; then
    systemctl --user disable --now bt-pixel-buds.service 2>/dev/null || true
    rm -f "$HOME/.config/systemd/user/bt-pixel-buds.service"
    info "Removed old bt-pixel-buds.service."
fi

install -m 0644 "$REPO/systemd/pbwatch.service" "$SERVICE_DEST"

systemctl --user daemon-reload
systemctl --user enable --now pbwatch.service \
    && done_ "systemd user service enabled and started." \
    || warn "systemctl enable/start failed. Run manually: systemctl --user enable --now pbwatch"

# ---------------------------------------------------------------------------
# 5. Patch dunstrc (optional — only if dunst is in use)
# ---------------------------------------------------------------------------
DUNSTRC="$HOME/.config/dunst/dunstrc"

DUNST_BLOCK='# pbwatch — display battery/ANC widget
[pbwatch]
    appname = pbwatch
    # Use "%s\n%b" here if you enable show_text = true in config.toml.
    format = "%b"
    icon_position = left
    min_icon_size = 0
    max_icon_size = 320'

if [[ -f "$DUNSTRC" ]]; then
    if grep -q '\[pbwatch\]' "$DUNSTRC"; then
        info "dunstrc already contains [pbwatch] rule — skipping."
    else
        printf '\n%s\n' "$DUNST_BLOCK" >> "$DUNSTRC"
        dunstctl reload 2>/dev/null || true
        done_ "dunstrc patched and dunst reloaded."
    fi
else
    info "~/.config/dunst/dunstrc not found — dunst rule not added."
    info "For dunst, mako, or SwayNC setup see: docs/notification-daemons/"
fi

# ---------------------------------------------------------------------------
# 6. Notify optional heal-audio deps
# ---------------------------------------------------------------------------
if [[ "$HEAL_AUDIO" == true ]]; then
    MISSING_OPT=()
    for pkg in pulseaudio-utils wireplumber; do
        dpkg -l "$pkg" 2>/dev/null | grep -q '^ii' || MISSING_OPT+=("$pkg")
    done
    if [[ ${#MISSING_OPT[@]} -gt 0 ]]; then
        warn "--heal-audio requires: ${MISSING_OPT[*]}"
        warn "Install with: sudo apt install ${MISSING_OPT[*]}"
    fi
fi

# ---------------------------------------------------------------------------
# 7. Summary
# ---------------------------------------------------------------------------
echo ""
echo "----------------------------------------------------------------------"
echo " pbutils — install complete"
echo "----------------------------------------------------------------------"
echo " [x] built from source (cargo build --release)"
echo " [x] binary -> /usr/local/bin/pbwatch"
echo " [x] systemd user service: pbwatch.service"
echo " [x] config: $CONFIG_FILE"
if [[ "$HAS_MACS" == false ]]; then
echo ""
echo " ACTION REQUIRED: add your Pixel Buds MAC address to:"
echo "   $CONFIG_FILE"
echo ""
echo "   macs = [\"AB:CD:EF:01:23:45\"]"
echo ""
echo "   Find your MAC with:  bluetoothctl devices"
echo "   Then restart:        systemctl --user restart pbwatch"
else
echo ""
echo " pbwatch is running. Check status:"
echo "   systemctl --user status pbwatch"
echo "   journalctl --user -u pbwatch -n 20"
fi
echo "----------------------------------------------------------------------"
