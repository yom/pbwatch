use std::collections::HashMap;

use anyhow::Context;
use zbus::Connection;
use zbus::zvariant::{StructureBuilder, Value};

use pbwatch::render::RenderOutput;

// ── D-Bus proxy ───────────────────────────────────────────────────────────────

#[zbus::proxy(
    interface  = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path    = "/org/freedesktop/Notifications",
)]
trait Notifications {
    fn notify(
        &self,
        app_name:       &str,
        replaces_id:    u32,
        app_icon:       &str,
        summary:        &str,
        body:           &str,
        actions:        &[&str],
        hints:          HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    fn get_server_information(&self) -> zbus::Result<(String, String, String, String)>;

    fn get_capabilities(&self) -> zbus::Result<Vec<String>>;
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Sends or replaces a desktop notification with the rendered widget image.
///
/// `device_key` identifies the device (MAC with colons replaced by underscores).
/// Each device keeps its own notification slot so multiple devices don't clobber
/// each other's notification.
///
/// Takes `&mut RenderOutput` because `cairo::ImageSurface::data()` requires `&mut self`.
pub async fn send(
    output:       &mut RenderOutput,
    summary_text: &str,
    body_text:    &str,
    expire_ms:    i32,
    stack_tag:    &str,
    device_key:   &str,
) -> anyhow::Result<u32> {
    let conn    = Connection::session().await.context("D-Bus session bus")?;
    let proxy   = NotificationsProxy::new(&conn).await.context("Notifications proxy")?;

    // Detect server and pick the correct image hint name.
    let spec_version = detect_server(&proxy).await;
    let image_hint   = image_hint_name(&spec_version);

    let replaces_id = load_notification_id(device_key);

    // Build image-data hint: (width, height, rowstride, has_alpha, bps, channels, pixels)
    let image_value = build_image_hint(output).context("build image hint")?;

    let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
    hints.insert(image_hint,                    image_value);
    hints.insert("x-dunst-stack-tag",           Value::Str(stack_tag.into()));
    hints.insert("x-canonical-private-synchronous", Value::Str(stack_tag.into()));
    hints.insert("transient",                   Value::Bool(false));

    let id = proxy.notify(
        "pbwatch",
        replaces_id,
        "audio-headphones",   // app icon — standard, not our PNG (which goes in image-data)
        summary_text,
        body_text,
        &[],
        hints,
        expire_ms,
    ).await.context("Notify call failed")?;

    save_notification_id(device_key, id);
    Ok(id)
}

// ── Server detection ──────────────────────────────────────────────────────────

async fn detect_server(proxy: &NotificationsProxy<'_>) -> String {
    match proxy.get_server_information().await {
        Ok((name, _vendor, version, spec)) => {
            log::debug!("pbwatch: notification daemon: {name} {version} (spec {spec})");
            spec
        }
        Err(e) => {
            log::warn!("pbwatch: GetServerInformation failed: {e} — assuming spec 1.2");
            "1.2".into()
        }
    }
}

/// Returns the correct hint name for the image based on spec version.
fn image_hint_name(spec: &str) -> &'static str {
    // spec "1.0" used "icon_data", 1.1 used "image_data", ≥1.2 uses "image-data".
    let major: u32 = spec.split('.').next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let minor: u32 = spec.split('.').nth(1).and_then(|s| s.parse().ok()).unwrap_or(2);
    match (major, minor) {
        (1, 0)          => "icon_data",
        (1, 1)          => "image_data",
        _               => "image-data",
    }
}

// ── Image hint construction ───────────────────────────────────────────────────

/// Builds the FDO `image-data` hint value `(iiibiiay)` from a RenderOutput.
///
/// Cairo ARGB32 → RGBA non-premultiplied via `convert::cairo_argb32_to_fdo_rgba`.
/// Takes `&mut` because `cairo::ImageSurface::data()` requires `&mut self`.
fn build_image_hint(output: &mut RenderOutput) -> anyhow::Result<Value<'static>> {
    output.surface.flush();

    let width   = output.width  as usize;
    let height  = output.height as usize;
    let stride  = output.surface.stride() as usize;

    // Borrow, convert, then drop the borrow so we own `rgba: Vec<u8>`.
    let rgba: Vec<u8> = {
        let data = output.surface.data()
            .map_err(|e| anyhow::anyhow!("surface data: {e:?}"))?;
        crate::convert::cairo_argb32_to_fdo_rgba(&data, width, height, stride)
    };

    // FDO image-data: (width, height, rowstride, has_alpha, bps, channels, pixels)
    // rowstride = width * 4 (RGBA tightly packed, no Cairo stride padding).
    let rowstride = (width * 4) as i32;

    let structure = StructureBuilder::new()
        .add_field(output.width)   // i32
        .add_field(output.height)  // i32
        .add_field(rowstride)      // i32
        .add_field(true)           // bool has_alpha
        .add_field(8i32)           // i32 bits-per-sample
        .add_field(4i32)           // i32 channels
        .add_field(rgba)           // Vec<u8> ay
        .build()
        .context("build image-data structure")?;

    Ok(Value::Structure(structure))
}

// ── ID persistence ────────────────────────────────────────────────────────────

fn notification_id_path(device_key: &str) -> std::path::PathBuf {
    // device_key is already MAC-with-underscores (e.g. "AB_CD_EF_01_23_45")
    pbcommon::runtime_dir("pbwatch").join(format!("notification_id-{device_key}"))
}

fn load_notification_id(device_key: &str) -> u32 {
    std::fs::read_to_string(notification_id_path(device_key))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn save_notification_id(device_key: &str, id: u32) {
    let path = notification_id_path(device_key);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, id.to_string());
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_hint_name_by_spec() {
        assert_eq!(image_hint_name("1.0"), "icon_data");
        assert_eq!(image_hint_name("1.1"), "image_data");
        assert_eq!(image_hint_name("1.2"), "image-data");
        assert_eq!(image_hint_name("1.3"), "image-data");
        assert_eq!(image_hint_name("2.0"), "image-data");
        assert_eq!(image_hint_name("unknown"), "image-data");  // fallback
    }
}
