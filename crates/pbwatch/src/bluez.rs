use std::time::Duration;

use anyhow::Context;
use zbus::Connection;
use zbus::fdo::PropertiesProxy;
use zbus::names::InterfaceName;

// ── One-shot query ────────────────────────────────────────────────────────────

/// Returns true if the device is currently Connected according to BlueZ.
/// Falls back to `false` on any D-Bus error (safe: leads to Retry, not Dormant).
pub async fn is_connected(addr: &bluer::Address) -> bool {
    async {
        let conn = Connection::system().await?;
        let path = resolve_device_path(&conn, addr).await;
        let props = PropertiesProxy::builder(&conn)
            .destination("org.bluez")?
            .path(path.as_str())?
            .build()
            .await?;
        let iface: InterfaceName = "org.bluez.Device1".try_into()?;
        let val = props.get(iface, "Connected").await?;
        anyhow::Ok(matches!(&*val, zbus::zvariant::Value::Bool(true)))
    }
    .await
    .unwrap_or(false)
}

/// Returns the BlueZ Alias for the device (user-editable label), or None on error.
pub async fn device_alias(addr: &bluer::Address) -> Option<String> {
    async {
        let conn = Connection::system().await?;
        let path = resolve_device_path(&conn, addr).await;
        let props = PropertiesProxy::builder(&conn)
            .destination("org.bluez")?
            .path(path.as_str())?
            .build()
            .await?;
        let iface: InterfaceName = "org.bluez.Device1".try_into()?;
        let val = props.get(iface, "Alias").await?;
        match &*val {
            zbus::zvariant::Value::Str(s) => anyhow::Ok(Some(s.to_string())),
            _ => anyhow::Ok(None),
        }
    }
    .await
    .unwrap_or(None)
}

// ── Dormant watcher ───────────────────────────────────────────────────────────

/// Waits until BlueZ reports that the device is Connected=true, then returns.
///
/// Uses a `PropertiesChanged` signal subscription so the reaction is <1s.
/// Falls back to a 5s poll if the signal subscription fails (defensive).
pub async fn wait_until_connected(addr: &bluer::Address) {
    match wait_signal(addr).await {
        Ok(()) => {}
        Err(e) => {
            log::warn!("bluez: signal watch failed ({e}), falling back to 5s poll");
            poll_until_connected(addr).await;
        }
    }
}

async fn wait_signal(addr: &bluer::Address) -> anyhow::Result<()> {
    let conn = Connection::system().await.context("D-Bus system bus")?;
    let path = resolve_device_path(&conn, addr).await;

    // Build a match rule for PropertiesChanged on our device.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.bluez")?
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .path(path.as_str())?
        .build();

    let mut stream = zbus::MessageStream::for_match_rule(rule, &conn, None)
        .await
        .context("PropertiesChanged subscription")?;

    use futures_util::StreamExt;
    while let Some(msg) = stream.next().await {
        let msg = match msg {
            Ok(m)  => m,
            Err(e) => { log::debug!("bluez: signal stream error: {e}"); continue; }
        };

        if let Ok(connected) = extract_connected(&msg) {
            if connected {
                log::debug!("bluez: PropertiesChanged → Connected=true");
                return Ok(());
            }
        }
    }

    // Stream closed without a Connected=true signal — fall back to poll.
    anyhow::bail!("PropertiesChanged stream ended unexpectedly")
}

async fn poll_until_connected(addr: &bluer::Address) {
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        if is_connected(addr).await {
            return;
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Resolves the D-Bus object path for a device by querying BlueZ's managed
/// objects. This handles non-standard adapter names (hci1, hci2, …).
/// Falls back to the hci0 path on any D-Bus error.
async fn resolve_device_path(conn: &Connection, addr: &bluer::Address) -> String {
    let fallback = device_path(addr);
    let mac_key  = crate::state::addr_to_mac_key(addr);
    let suffix   = format!("/dev_{mac_key}");

    let found: anyhow::Result<String> = async {
        let proxy = zbus::Proxy::new(conn, "org.bluez", "/",
            "org.freedesktop.DBus.ObjectManager").await?;

        type Objects = std::collections::HashMap<
            zbus::zvariant::OwnedObjectPath,
            std::collections::HashMap<
                String,
                std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
            >,
        >;
        let objects: Objects = proxy.call("GetManagedObjects", &()).await?;

        for (path, _) in &objects {
            if path.as_str().ends_with(&suffix) {
                return Ok(path.as_str().to_owned());
            }
        }
        anyhow::bail!("device not found in BlueZ managed objects")
    }.await;

    match found {
        Ok(p) => {
            if p != fallback {
                log::debug!("bluez: adapter discovered via D-Bus: {p} (not hci0)");
            }
            p
        }
        Err(e) => {
            log::debug!("bluez: adapter discovery failed ({e}), falling back to hci0");
            fallback
        }
    }
}

/// Returns the hci0-based D-Bus path for a device (used as fallback).
pub fn device_path(addr: &bluer::Address) -> String {
    let mac = crate::state::addr_to_mac_key(addr);
    format!("/org/bluez/hci0/dev_{mac}")
}

/// Extracts the `Connected` field from a `PropertiesChanged` message body.
///
/// The body is `(sa{sv}as)`:
/// - `s`    — interface name ("org.bluez.Device1")
/// - `a{sv}` — changed properties dict
/// - `as`   — invalidated property names (unused)
fn extract_connected(msg: &zbus::Message) -> anyhow::Result<bool> {
    // Deserialize body as (String, HashMap<String, OwnedValue>, Vec<String>)
    type Body = (String, std::collections::HashMap<String, zbus::zvariant::OwnedValue>, Vec<String>);
    let (iface, changed, _): Body = msg.body().deserialize()
        .context("PropertiesChanged body")?;

    if iface != "org.bluez.Device1" {
        anyhow::bail!("not Device1");
    }

    let val = changed.get("Connected")
        .ok_or_else(|| anyhow::anyhow!("Connected not in changed dict"))?;

    match &**val {
        zbus::zvariant::Value::Bool(b) => Ok(*b),
        _ => anyhow::bail!("Connected has unexpected type"),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_path_format() {
        let addr: bluer::Address = "AB:CD:EF:01:23:45".parse().unwrap();
        assert_eq!(device_path(&addr), "/org/bluez/hci0/dev_AB_CD_EF_01_23_45");
    }

    #[test]
    fn device_path_lowercase_input() {
        // bluer::Address is stored as binary; uppercase comes from its Display impl
        let addr: bluer::Address = "ab:cd:ef:01:23:45".parse().unwrap();
        assert_eq!(device_path(&addr), "/org/bluez/hci0/dev_AB_CD_EF_01_23_45");
    }
}
