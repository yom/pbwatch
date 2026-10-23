use std::path::Path;
use std::process::Stdio;

use pbcommon::BudsState;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Spawns a subprocess, writes the JSON state to its stdin, and reaps it
/// asynchronously. Fire-and-forget: failures are logged but never propagate
/// to the caller.
///
/// The subprocess also receives two environment variables for compatibility
/// with legacy scripts that don't read stdin:
/// - `PBWATCH_EVENT`      — "connect", "update", or "disconnect"
/// - `PBWATCH_STATE_FILE` — path to the state file (e.g. `state-AB_CD_EF_01_23_45.json`)
pub fn spawn(cmd: &[String], state: &BudsState, state_path: &Path) {
    if cmd.is_empty() {
        return;
    }

    let json = match serde_json::to_vec(state) {
        Ok(mut v) => { v.push(b'\n'); v }
        Err(e) => { log::error!("hook: failed to serialize state: {e}"); return; }
    };

    let event_str = match state.event {
        pbcommon::Event::Connect    => "connect",
        pbcommon::Event::Update     => "update",
        pbcommon::Event::Disconnect => "disconnect",
    };

    let mut child = match Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("PBWATCH_EVENT",      event_str)
        .env("PBWATCH_STATE_FILE", state_path)
        .kill_on_drop(false)
        .spawn()
    {
        Ok(c)  => c,
        Err(e) => {
            log::error!("hook: failed to spawn {:?}: {e}", cmd[0]);
            return;
        }
    };

    let mut stdin = match child.stdin.take() {
        Some(s) => s,
        None => {
            log::error!("hook: stdin not available");
            return;
        }
    };

    tokio::spawn(async move {
        // Ignore EPIPE — the script may exit without reading stdin.
        let _ = stdin.write_all(&json).await;
        drop(stdin); // signal EOF
        match child.wait().await {
            Ok(status) if !status.success() => {
                log::warn!("hook: exited with {status}");
            }
            Err(e) => log::warn!("hook: wait failed: {e}"),
            _ => {}
        }
    });
}
