mod bluez;
mod convert;
mod heal_audio;
mod hook;
mod notify;
mod reconnect;
mod rfcomm;
mod state;

use pbwatch::{config, protocol, render};

use std::time::Duration;

use clap::{Parser, Subcommand};
use pbcommon::{BudsState, Event};
use protocol::{classify, Msg};
use reconnect::{Failure, Next, Reconnect};
use rfcomm::{FrameReader, SessionEnd};

#[derive(Subcommand)]
enum Command {
    /// Send a desktop notification with the current Pixel Buds state.
    Notify,
}

#[derive(Parser)]
#[command(name = "pbwatch", about = "Pixel Buds RFCOMM monitor", max_term_width = 100)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Bluetooth MAC (repeatable; overrides config). Find it: bluetoothctl devices
    #[arg(long)]
    mac: Vec<String>,

    /// Hook on battery/ANC/placement changes; receives JSON on stdin.
    ///
    /// If omitted, pbwatch renders and notifies internally.
    #[arg(long)]
    on_update: Option<String>,

    /// Hook on disconnect; receives JSON on stdin.
    #[arg(long)]
    on_disconnect: Option<String>,

    /// On connect, switch Bluetooth card to A2DP (high-quality audio) profile.
    ///
    /// Waits for WirePlumber. Enables heal_audio (in addition to config.toml).
    #[arg(long)]
    heal_audio: bool,

    /// Write legacy battery/anc flat files for script compatibility.
    #[arg(long)]
    legacy_state_files: bool,

    /// Disable notifications and hooks; only write state files.
    #[arg(long)]
    no_notify: bool,

    /// Print raw RFCOMM frames to stderr.
    #[arg(long)]
    debug: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let level = if cli.debug { log::LevelFilter::Debug } else { log::LevelFilter::Info };
    env_logger::Builder::new()
        .parse_default_env()
        .filter_level(level)
        .format_timestamp(None)
        .format_target(false)
        .init();

    let cfg = config::Config::load()
        .unwrap_or_else(|e| { log::error!("config: {e}"); config::Config::default() });

    // Collect MACs: CLI takes precedence over config when provided.
    let mac_strs: Vec<String> = if !cli.mac.is_empty() {
        cli.mac.clone()
    } else {
        cfg.macs.clone()
    };

    if mac_strs.is_empty() {
        anyhow::bail!(
            "No Pixel Buds MAC address configured.\n\
             Add to ~/.config/pbwatch/config.toml:\n\
             \n  macs = [\"AB:CD:EF:01:23:45\"]\n\
             \nFind your MAC with: bluetoothctl devices"
        );
    }

    // ── notify subcommand ──────────────────────────────────────────────────────
    if matches!(cli.command, Some(Command::Notify)) {
        if cli.no_notify {
            log::warn!("pbwatch notify: --no-notify suppresses the notification — subcommand has no effect");
        }
        run_notify(&mac_strs, cli.no_notify).await;
        return Ok(());
    }

    let heal_audio    = cli.heal_audio || cfg.heal_audio;
    let on_update: Vec<String> = match cli.on_update.as_deref().map(shlex::split) {
        None            => vec![],
        Some(Some(v))   => v,
        Some(None)      => anyhow::bail!("--on-update: invalid shell quoting"),
    };
    let on_disconnect: Vec<String> = match cli.on_disconnect.as_deref().map(shlex::split) {
        None            => vec![],
        Some(Some(v))   => v,
        Some(None)      => anyhow::bail!("--on-disconnect: invalid shell quoting"),
    };

    // Cairo surfaces are !Send, so we use spawn_local (same thread, no Send bound).
    let local = tokio::task::LocalSet::new();

    for mac_str in &mac_strs {
        let addr = rfcomm::parse_mac(mac_str)?;
        let writer = state::StateWriter::new(cli.legacy_state_files, &addr)?;

        log::info!("pbwatch: monitoring {}{}", mac_str.to_uppercase(),
            if cli.debug { " [DEBUG]" } else { "" });

        let on_update     = on_update.clone();
        let on_disconnect = on_disconnect.clone();
        let no_notify     = cli.no_notify;
        let debug         = cli.debug;

        local.spawn_local(
            run(addr, on_update, on_disconnect, writer, no_notify, heal_audio, debug)
        );
    }

    local.await;
    Ok(())
}

/// Reads the persisted state for each MAC and fires a desktop notification.
async fn run_notify(mac_strs: &[String], no_notify: bool) {
    for mac_str in mac_strs {
        let addr = match rfcomm::parse_mac(mac_str) {
            Ok(a)  => a,
            Err(e) => { log::error!("pbwatch notify: {e}"); continue; }
        };
        let writer = match state::StateWriter::new(false, &addr) {
            Ok(w)  => w,
            Err(e) => { log::error!("pbwatch notify: {e}"); continue; }
        };
        let path = writer.state_path();
        let data = match std::fs::read_to_string(&path) {
            Ok(d)  => d,
            Err(e) => { log::warn!("pbwatch notify[{}]: {e}", mac_str.to_uppercase()); continue; }
        };
        let buds: BudsState = match serde_json::from_str(&data) {
            Ok(b)  => b,
            Err(e) => { log::error!("pbwatch notify: parse error: {e}"); continue; }
        };
        fire_notify(&[], &buds, &path, no_notify).await;
    }
}

/// Main reconnect loop for one device. Never returns — runs until the process is killed.
async fn run(
    addr:          bluer::Address,
    on_update:     Vec<String>,
    on_disconnect: Vec<String>,
    writer:        state::StateWriter,
    no_notify:     bool,
    heal_audio:    bool,
    debug:         bool,
) {
    let mac = addr.to_string().to_uppercase();
    let mut machine = Reconnect::default();
    let mut next = Next::Retry(Duration::ZERO);
    // Preserved across reset reconnects so run_session can skip spurious
    // notifications when the device was never truly offline from the user's perspective.
    let mut prior_state: Option<pbcommon::BudsState> = None;

    loop {
        // ── Dormant mode ───────────────────────────────────────────────────────
        while matches!(next, Next::Dormant) {
            log::info!("pbwatch[{mac}]: device offline, waiting for reconnect...");
            bluez::wait_until_connected(&addr).await;
            log::info!("pbwatch[{mac}]: device back online, trying to connect...");
            machine.on_dormant_wake();
            next = Next::Retry(Duration::ZERO);
        }

        // ── Retry delay ────────────────────────────────────────────────────────
        let Next::Retry(delay) = next else { unreachable!() };
        if !delay.is_zero() {
            log::debug!("pbwatch[{mac}]: waiting {}s before retry", delay.as_secs());
            tokio::time::sleep(delay).await;
        }

        // ── Connect attempt ────────────────────────────────────────────────────
        // Only reset state on genuine connects (prior_state is None after a real disconnect
        // or at startup); reset reconnects leave state untouched.
        if prior_state.is_none() {
            writer.reset();
        }
        log::info!("pbwatch[{mac}]: connecting...");

        let stream = match rfcomm::connect(addr).await {
            Ok(s) => s,
            Err(rfcomm::ConnectError::Timeout) => {
                prior_state = None; // connect failed → device may be offline; notify on next connect
                let bt  = bluez::is_connected(&addr).await;
                let dec = machine.on_failure(Failure::ConnectTimeout, bt);
                log::info!("pbwatch[{mac}]: connect timeout (bt_online={})", bt);
                next = dec.next;
                continue;
            }
            Err(rfcomm::ConnectError::Io(e)) => {
                prior_state = None; // connect failed → device may be offline; notify on next connect
                let bt  = bluez::is_connected(&addr).await;
                let dec = machine.on_failure(Failure::Other, bt);
                log::info!("pbwatch[{mac}]: connect error: {e}");
                next = dec.next;
                continue;
            }
        };

        let was_reconnect = machine.on_connected();
        log::info!("pbwatch[{mac}]: connected{}",
            if was_reconnect { " (reconnect)" } else { "" });

        if heal_audio {
            heal_audio::spawn(&addr);
        }

        // ── Session ────────────────────────────────────────────────────────────
        let (session_end, mut buds) =
            run_session(stream, &addr, &writer, &on_update, no_notify, debug,
                        prior_state.take()).await;

        // ── Classify failure ───────────────────────────────────────────────────
        let (failure, bt) = match &session_end {
            SessionEnd::Reset    => (Failure::Reset, true),
            SessionEnd::Other(e) => {
                log::info!("pbwatch[{mac}]: {e}");
                (Failure::Other, bluez::is_connected(&addr).await)
            }
        };

        let dec = machine.on_failure(failure, bt);

        if matches!(session_end, SessionEnd::Reset) {
            log::info!("pbwatch[{mac}]: device reset, reconnecting...");
        }

        // Preserve state across Reset reconnects (bud placed in/out of case) so the next
        // session can suppress spurious connect and ANC notifications when nothing changed.
        prior_state = if matches!(failure, Failure::Reset) {
            Some(buds.clone())
        } else {
            None
        };

        // Reset is transparent to the user — skip writing Disconnect so the state file
        // doesn't briefly show a stale Disconnect during the 500ms gap before reconnect.
        if !matches!(failure, Failure::Reset) {
            buds.event = Event::Disconnect;
            if let Err(e) = writer.write(&buds) {
                log::warn!("state write: {e}");
            }
        }
        if dec.announce_disconnect {
            fire_notify(&on_disconnect, &buds, &writer.state_path(), no_notify).await;
            log::info!("pbwatch[{mac}]: disconnected");
        }

        next = dec.next;
    }
}

/// Runs one RFCOMM session. Returns the reason the session ended and the final BudsState.
///
/// `prior_state` carries the last known state across reset reconnects so that the
/// session can suppress spurious connect and ANC notifications when nothing actually changed.
async fn run_session(
    stream:      bluer::rfcomm::Stream,
    addr:        &bluer::Address,
    writer:      &state::StateWriter,
    on_update:   &[String],
    no_notify:   bool,
    debug:       bool,
    prior_state: Option<pbcommon::BudsState>,
) -> (SessionEnd, BudsState) {
    let mac = addr.to_string().to_uppercase();
    let mut buds = BudsState::default();
    buds.schema = 1;
    buds.mac = Some(mac.clone());
    buds.device_name = bluez::device_alias(addr).await;
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = FrameReader::new(read_half);

    rfcomm::drain_burst(&mut reader, &mut buds, debug).await;

    if let Err(e) = rfcomm::send_get_state(&mut write_half).await {
        log::error!("pbwatch[{mac}]: GET_STATE failed: {e}");
        return (SessionEnd::Other(e), buds);
    }

    // On reset reconnects, seed last_* from the previous session so that the
    // first messages from the buds (which simply echo back unchanged state) don't look
    // like changes and trigger spurious notifications.
    let mut last_placement = prior_state.as_ref()
        .map(|p| (p.left.is_none(), p.right.is_none()))
        .unwrap_or((buds.left.is_none(), buds.right.is_none()));
    let mut last_charging = prior_state.as_ref()
        .map(|p| (p.left_charging, p.right_charging, p.case_charging))
        .unwrap_or((buds.left_charging, buds.right_charging, buds.case_charging));
    let mut last_anc = prior_state.as_ref()
        .map(|p| p.anc)
        .unwrap_or(buds.anc);

    buds.event = Event::Connect;
    if let Err(e) = writer.write(&buds) {
        log::warn!("state write: {e}");
    }
    // Only notify on first connect or after a genuine disconnect — reset
    // reconnects are transparent to the user (device was never truly offline).
    if prior_state.is_none() {
        fire_notify(on_update, &buds, &writer.state_path(), no_notify).await;
    }

    let end = loop {
        let frame = match reader.next().await {
            Ok(f)  => f,
            Err(e) => break SessionEnd::from(e),
        };

        log::debug!(
            "  msg group=0x{:02x} code=0x{:02x} len={}  payload={}",
            frame.group, frame.code, frame.payload.len(),
            frame.payload.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );

        match classify(frame) {
            Msg::Battery(bytes) => {
                protocol::apply_battery(&mut buds, bytes);

                let placement = (buds.left.is_none(), buds.right.is_none());
                let charging  = (buds.left_charging, buds.right_charging, buds.case_charging);

                buds.event = Event::Update;
                if let Err(e) = writer.write(&buds) { log::warn!("state write: {e}"); }

                if placement != last_placement {
                    last_placement = placement;
                    log::info!("pbwatch[{mac}]: placement → left={} right={}",
                        if placement.0 { "in case" } else { "out" },
                        if placement.1 { "in case" } else { "out" });
                    fire_notify(on_update, &buds, &writer.state_path(), no_notify).await;
                } else if charging != last_charging {
                    log::info!("pbwatch[{mac}]: charging → {:?}", charging);
                    fire_notify(on_update, &buds, &writer.state_path(), no_notify).await;
                }
                last_charging = charging;
            }

            Msg::Anc { state: anc_state, supported } => {
                if anc_state != pbcommon::AncMode::Unknown {
                    buds.anc           = anc_state;
                    buds.anc_supported = supported;
                }
                buds.event = Event::Update;
                if let Err(e) = writer.write(&buds) { log::warn!("state write: {e}"); }

                if anc_state != pbcommon::AncMode::Unknown && anc_state != last_anc {
                    last_anc = anc_state;
                    log::info!("pbwatch[{mac}]: ANC → {:?} (supported: {:?})",
                        anc_state, &buds.anc_supported);
                    fire_notify(on_update, &buds, &writer.state_path(), no_notify).await;
                }
            }

            Msg::ModelId(id) => {
                buds.model_id = Some(id);
                if let Some((_, name)) = protocol::model_info(id) {
                    buds.model_name = Some(name.to_string());
                }
            }

            Msg::Other => {}
        }
    };

    (end, buds)
}

// ── Notification dispatch ─────────────────────────────────────────────────────

async fn fire_notify(
    on_hook:    &[String],
    buds:       &BudsState,
    state_path: &std::path::Path,
    no_notify:  bool,
) {
    if no_notify {
        return;
    }
    if !on_hook.is_empty() {
        hook::spawn(on_hook, buds, state_path);
        return;
    }
    // Reloaded per-notification so color/theme/font changes apply without restart.
    // Fields used at startup only (macs, heal_audio) are not re-read here.
    let cfg  = config::Config::load()
        .unwrap_or_else(|e| { log::error!("config: {e}"); config::Config::default() });
    let disc = matches!(buds.event, Event::Disconnect);
    let mut out = if disc {
        render::render_disconnected(buds, &cfg)
    } else {
        render::render(buds, &cfg)
    };
    // Derive a stable per-device key from the MAC (colons → underscores).
    // Validate the MAC from the state object through bluer's Address parser
    // to prevent path traversal if the state file has been tampered with.
    let device_key = buds.mac.as_deref()
        .and_then(|m| m.parse::<bluer::Address>().ok())
        .map(|a| a.to_string().replace(':', "_"))
        .unwrap_or_else(|| "default".to_string());
    match out {
        Ok(ref mut o) => {
            let summary = if cfg.show_text { notification_summary(buds, &cfg, disc) } else { String::new() };
            let body    = if cfg.show_text { notification_body(buds, disc) } else { String::new() };
            let stack_tag = format!("pbwatch-{device_key}");
            if let Err(e) = notify::send(o, &summary, &body,
                cfg.auto_close as i32, &stack_tag, &device_key).await {
                log::warn!("pbwatch: notification: {e}");
            }
        }
        Err(e) => log::warn!("pbwatch: render: {e}"),
    }
}

fn notification_summary(buds: &BudsState, cfg: &config::Config, disc: bool) -> String {
    let raw = if !cfg.device_label.is_empty() { cfg.device_label.as_str() }
              else { buds.device_name.as_deref().unwrap_or("Pixel Buds") };
    let name = escape_pango(raw);
    if disc { format!("{name} disconnected") } else { name }
}

/// Escapes Pango markup special characters and strips control chars.
/// Some notification daemons (dunst with markup=full) interpret the summary
/// as Pango markup; a device name containing `<` or `&` would corrupt it.
fn escape_pango(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().take(64) {
        match c {
            '&'  => out.push_str("&amp;"),
            '<'  => out.push_str("&lt;"),
            '>'  => out.push_str("&gt;"),
            '\'' => out.push_str("&#39;"),
            '"'  => out.push_str("&quot;"),
            c if c.is_control() => {}
            c    => out.push(c),
        }
    }
    out
}

fn notification_body(buds: &BudsState, disc: bool) -> String {
    if disc { return String::new(); }
    let pct = |v: Option<u8>| v.map_or("—".into(), |p| format!("{p}%"));
    let bat = format!("L {} · C {} · R {}", pct(buds.left), pct(buds.case), pct(buds.right));
    let anc = match buds.anc {
        pbcommon::AncMode::NoiseCancellation => "Noise cancellation",
        pbcommon::AncMode::Transparency      => "Transparency",
        pbcommon::AncMode::Adaptive          => "Adaptive",
        pbcommon::AncMode::Off               => "Off",
        pbcommon::AncMode::Unknown           => "",
    };
    if anc.is_empty() { bat } else { format!("{bat}  —  ANC: {anc}") }
}
