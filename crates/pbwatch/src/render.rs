use std::f64::consts::PI;

use anyhow::Context;
use pbcommon::BudsState;

use crate::config::Config;
use crate::icons;
use crate::sprites;

const GAP: f64 = 80.0 * PI / 180.0; // 80° opening at top


// ANC display order; filtered by anc_supported at runtime.
const ANC_MODES: &[(&str, &str)] = &[
    ("noise_cancellation", "Noise cancel."),
    ("off",                "Off"),
    ("adaptive",           "Adaptive"),
    ("transparency",       "Transparency"),
];

pub struct RenderOutput {
    pub surface: cairo::ImageSurface,
    pub width:   i32,
    pub height:  i32,
}

/// Renders the battery + ANC widget for a connected state.
pub fn render(state: &BudsState, cfg: &Config) -> anyhow::Result<RenderOutput> {
    let fs   = cfg.font_size as f64 / 100.0;
    let size = cfg.image_size as i32;
    let cell = size / 3;

    let title = device_title(state, cfg);
    let title_h   = if title.is_empty() { 0 } else { (cell as f64 * 0.35) as i32 };
    let summary_h = (cell as f64 * 0.28) as i32;
    let anc_h     = (cell as f64 * 0.75) as i32;
    let w         = size;
    let h         = title_h + cell + summary_h + anc_h;

    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h)
        .context("create surface")?;
    let ctx = cairo::Context::new(&surface).context("create context")?;

    // Background
    let bg = hex_to_rgba(&cfg.bg_color);
    ctx.set_source_rgba(bg[0], bg[1], bg[2], cfg.opacity);
    ctx.rectangle(0.0, 0.0, w as f64, h as f64);
    ctx.fill().context("fill background")?;

    // Title
    let text_rgba      = hex_to_rgba(&cfg.text_color);
    let highlight_rgba = hex_to_rgba(&cfg.highlight_color);
    let ring_rgba      = hex_to_rgba(&cfg.ring_color);
    let warn_rgba      = hex_to_rgba(&cfg.warn_color);
    let critical_rgba  = hex_to_rgba(&cfg.critical_color);

    if !title.is_empty() {
        ctx.set_source_rgba(text_rgba[0], text_rgba[1], text_rgba[2], text_rgba[3]);
        ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        let mut font_size = title_h as f64 * 0.70 * fs;
        ctx.set_font_size(font_size);
        while ctx.text_extents(&title).map(|e| e.width()).unwrap_or(0.0) > w as f64 * 0.96
            && font_size > 6.0 * fs
        {
            font_size -= 0.5;
            ctx.set_font_size(font_size);
        }
        if let Ok(ext) = ctx.text_extents(&title) {
            ctx.move_to(
                w as f64 / 2.0 - ext.width() / 2.0 - ext.x_bearing(),
                title_h as f64 / 2.0 - ext.height() / 2.0 - ext.y_bearing(),
            );
            let _ = ctx.show_text(&title);
        }
    }

    // Gauge layout
    let col_w     = w as f64 / 3.0;
    let radius    = col_w * 0.34;
    let thickness = (col_w * 0.08).max(4.0);
    let cy        = title_h as f64 + col_w * 0.50;

    // Visible gauges: left, case, right (only those with a value)
    let gauge_defs = [
        ("left",  "L", state.left,  state.left_charging),
        ("case",  "C", state.case,  state.case_charging),
        ("right", "R", state.right, state.right_charging),
    ];
    let visible: Vec<_> = gauge_defs.iter().filter(|g| g.2.is_some()).collect();

    let sprite_key = model_sprite_key(state, cfg);
    let sprite_set = sprites::get_set(&sprite_key);

    if visible.is_empty() {
        // No data — draw grey rings with labels
        for (i, (_, label, _, _)) in gauge_defs.iter().enumerate() {
            let cx = col_w * i as f64 + col_w / 2.0;
            draw_unknown_gauge(&ctx, cx, cy, radius, thickness, label,
                               ring_rgba, text_rgba)?;
        }
    } else {
        let n = visible.len();
        let pos_w = w as f64 / n as f64;
        for (i, (key, label, pct, charging)) in visible.iter().enumerate() {
            let cx = pos_w * i as f64 + pos_w / 2.0;
            let sprite_bytes = match *key {
                "left"  => Some(sprite_set.left),
                "right" => Some(sprite_set.right),
                "case"  => Some(sprite_set.case),
                _       => None,
            };
            draw_gauge(&ctx, cx, cy, radius, thickness, *pct, label,
                       ring_rgba, text_rgba, bg,
                       sprite_bytes, *charging,
                       highlight_rgba, warn_rgba, critical_rgba,
                       cfg.battery_warn, cfg.battery_critical)?;
        }

        // Summary labels
        ctx.set_source_rgba(text_rgba[0], text_rgba[1], text_rgba[2], text_rgba[3]);
        ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        ctx.set_font_size((cell as f64 * 0.14 * fs).max(9.0));
        let summary_y = cy + radius + thickness + radius * 0.20;
        let pos_w = w as f64 / n as f64;
        for (i, (key, _, pct, _)) in visible.iter().enumerate() {
            let pct_val = pct.unwrap_or(0);
            let lbl = match *key {
                "left"  => format!("Left: {pct_val}%"),
                "right" => format!("Right: {pct_val}%"),
                "case"  => format!("Case: {pct_val}%"),
                other   => format!("{other}: {pct_val}%"),
            };
            let cx = pos_w * i as f64 + pos_w / 2.0;
            if let Ok(ext) = ctx.text_extents(&lbl) {
                ctx.move_to(
                    cx - ext.width() / 2.0 - ext.x_bearing(),
                    summary_y - ext.y_bearing(),
                );
                let _ = ctx.show_text(&lbl);
            }
        }
    }

    // Separator line
    let sep_y = (title_h + cell + summary_h) as f64 + anc_h as f64 * 0.04;
    ctx.set_source_rgba(text_rgba[0], text_rgba[1], text_rgba[2], 0.15);
    ctx.set_line_width(1.0);
    ctx.set_dash(&[], 0.0);
    ctx.move_to(w as f64 * 0.05, sep_y);
    ctx.line_to(w as f64 * 0.95, sep_y);
    ctx.stroke().context("sep stroke")?;

    // ANC row
    let anc_y = title_h + cell + summary_h;
    let active_anc = state.anc.as_key();
    let mut supported: Option<Vec<&str>> = if state.anc_supported.is_empty() {
        None
    } else {
        Some(state.anc_supported.iter().map(|m| m.as_key()).collect())
    };
    if let Some(ref mut v) = supported {
        match cfg.anc_adaptive.as_str() {
            "true"  => { if !v.contains(&"adaptive") { v.push("adaptive"); } }
            "false" => { v.retain(|k| *k != "adaptive"); }
            _       => {} // "auto" — keep as reported by device
        }
    }
    draw_anc_row(&ctx, w as f64, anc_y as f64, anc_h as f64,
                 active_anc, highlight_rgba, text_rgba,
                 supported.as_deref(), fs)?;

    Ok(RenderOutput { width: w, height: h, surface })
}

/// Renders the disconnected state screen.
pub fn render_disconnected(state: &BudsState, cfg: &Config) -> anyhow::Result<RenderOutput> {
    let fs   = cfg.font_size as f64 / 100.0;
    let size = cfg.image_size as i32;
    let w = size;
    let h = size / 2;

    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h)
        .context("create surface")?;
    let ctx = cairo::Context::new(&surface).context("create context")?;

    let bg = hex_to_rgba(&cfg.bg_color);
    ctx.set_source_rgba(bg[0], bg[1], bg[2], cfg.opacity);
    ctx.rectangle(0.0, 0.0, w as f64, h as f64);
    ctx.fill().context("fill background")?;

    // Disconnected sprite centered
    let sprite_key = model_sprite_key(state, cfg);
    let sprite_set = sprites::get_set(&sprite_key);
    let sprite_h = h as f64 * 0.65;
    if let Ok(surf) = sprites::load_surface(sprite_set.disconnected) {
        let scale = sprite_h / surf.height() as f64;
        draw_sprite(&ctx, &surf, w as f64 / 2.0, h as f64 * 0.46, scale)?;
    }

    // "X disconnected." label
    let text_rgba = hex_to_rgba(&cfg.text_color);
    ctx.set_source_rgba(text_rgba[0], text_rgba[1], text_rgba[2], text_rgba[3]);
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    let device = device_title(state, cfg);
    let device = if device.is_empty() { "Pixel Buds".to_owned() } else { device };
    let label = format!("{device} disconnected.");
    let mut font_size = h as f64 * 0.14 * fs;
    ctx.set_font_size(font_size);
    while ctx.text_extents(&label).map(|e| e.width()).unwrap_or(0.0) > w as f64 * 0.92
        && font_size > 6.0 * fs
    {
        font_size -= 0.5;
        ctx.set_font_size(font_size);
    }
    if let Ok(ext) = ctx.text_extents(&label) {
        ctx.move_to(
            w as f64 / 2.0 - ext.width() / 2.0 - ext.x_bearing(),
            h as f64 * 0.90 - ext.y_bearing(),
        );
        let _ = ctx.show_text(&label);
    }

    Ok(RenderOutput { width: w, height: h, surface })
}

// ── Layout helpers ─────────────────────────────────────────────────────────────

fn device_title(state: &BudsState, cfg: &Config) -> String {
    if !cfg.device_label.is_empty() {
        return cfg.device_label.clone();
    }
    state.device_name.clone()
        .or_else(|| state.model_name.clone())
        .unwrap_or_else(|| "Pixel Buds".into())
}

fn model_sprite_key(state: &BudsState, cfg: &Config) -> String {
    if let Some(ref key) = cfg.sprite_set {
        return key.clone();
    }
    crate::protocol::sprite_set_for(state.model_id).to_owned()
}


/// Parses `#RRGGBB` or `#RRGGBBAA` into `[r, g, b, a]` (0..1 range).
pub fn hex_to_rgba(h: &str) -> [f64; 4] {
    let h = h.trim_start_matches('#');
    // Guard against multi-byte UTF-8: byte slicing is only safe on ASCII.
    if !h.is_ascii() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let parse = |s: &str| u8::from_str_radix(s, 16).unwrap_or(0) as f64 / 255.0;
    if h.len() == 8 {
        [parse(&h[0..2]), parse(&h[2..4]), parse(&h[4..6]), parse(&h[6..8])]
    } else if h.len() == 6 {
        [parse(&h[0..2]), parse(&h[2..4]), parse(&h[4..6]), 1.0]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

// ── Drawing primitives ─────────────────────────────────────────────────────────

fn arc_color(pct: u8, battery_warn: u8, battery_critical: u8,
             ok: [f64; 4], warn: [f64; 4], critical: [f64; 4]) -> [f64; 4] {
    if pct < battery_critical { critical }
    else if pct < battery_warn { warn }
    else { ok }
}

fn draw_unknown_gauge(
    ctx: &cairo::Context,
    cx: f64, cy: f64, radius: f64, thickness: f64,
    label: &str,
    ring: [f64; 4], text: [f64; 4],
) -> anyhow::Result<()> {
    ctx.set_line_cap(cairo::LineCap::Round);
    ctx.set_line_width(thickness);
    let ring_start = -PI / 2.0 + GAP / 2.0;
    let ring_end   = -PI / 2.0 - GAP / 2.0 + 2.0 * PI;
    ctx.set_source_rgba(ring[0], ring[1], ring[2], ring[3]);
    ctx.new_sub_path();
    ctx.arc(cx, cy, radius, ring_start, ring_end);
    ctx.stroke().context("unknown gauge ring")?;

    ctx.set_source_rgba(text[0], text[1], text[2], text[3]);
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    ctx.set_font_size(radius * 0.60);
    if let Ok(ext) = ctx.text_extents(label) {
        ctx.move_to(cx - ext.width() / 2.0 - ext.x_bearing(),
                    cy - ext.height() / 2.0 - ext.y_bearing());
        let _ = ctx.show_text(label);
    }
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn draw_gauge(
    ctx: &cairo::Context,
    cx: f64, cy: f64, radius: f64, thickness: f64,
    pct: Option<u8>, label: &str,
    ring: [f64; 4], text: [f64; 4], bg: [f64; 4],
    sprite_bytes: Option<&'static [u8]>,
    charging: bool,
    ok: [f64; 4], warn: [f64; 4], critical: [f64; 4],
    battery_warn: u8, battery_critical: u8,
) -> anyhow::Result<()> {
    let Some(p) = pct else { return Ok(()); };

    let arc_col = arc_color(p, battery_warn, battery_critical, ok, warn, critical);

    ctx.set_line_cap(cairo::LineCap::Round);
    ctx.set_line_width(thickness);

    // Background ring
    let ring_start = -PI / 2.0 + GAP / 2.0;
    let ring_end   = -PI / 2.0 - GAP / 2.0 + 2.0 * PI;
    ctx.set_source_rgba(ring[0], ring[1], ring[2], ring[3]);
    ctx.new_sub_path();
    ctx.arc(cx, cy, radius, ring_start, ring_end);
    ctx.stroke().context("gauge ring")?;

    // Filled arc
    if p > 0 {
        let arc_span = 2.0 * PI - GAP;
        let end = ring_start + (p as f64 / 100.0) * arc_span;
        ctx.set_source_rgba(arc_col[0], arc_col[1], arc_col[2], arc_col[3]);
        ctx.new_sub_path();
        ctx.arc(cx, cy, radius, ring_start, end);
        ctx.stroke().context("gauge arc")?;
    }

    // Sprite or text label inside the ring
    if let Some(bytes) = sprite_bytes {
        if let Ok(surf) = sprites::load_surface(bytes) {
            let inner = (radius - thickness / 2.0) * 2.0;
            let scale = (inner * 0.75) / (surf.width().max(surf.height()) as f64);
            draw_sprite(ctx, &surf, cx, cy, scale)?;
        }
    } else {
        ctx.set_source_rgba(text[0], text[1], text[2], text[3]);
        ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        ctx.set_font_size(radius * 0.60);
        if let Ok(ext) = ctx.text_extents(label) {
            ctx.move_to(cx - ext.width() / 2.0 - ext.x_bearing(),
                        cy - ext.height() / 2.0 - ext.y_bearing());
            let _ = ctx.show_text(label);
        }
    }

    // Battery icon above ring
    draw_battery_icon(ctx, cx, cy, radius, Some(p), charging, arc_col, bg)?;
    Ok(())
}

fn draw_battery_icon(
    ctx: &cairo::Context,
    cx: f64, cy: f64, radius: f64,
    pct: Option<u8>, charging: bool,
    arc_color: [f64; 4], bg_rgba: [f64; 4],
) -> anyhow::Result<()> {
    let name = icons::battery_icon_name(pct, charging);
    let Some(svg_data) = icons::find_icon_bytes(&name) else {
        return Ok(());
    };

    let opt = resvg::usvg::Options::default();
    let Ok(tree) = resvg::usvg::Tree::from_data(&svg_data, &opt) else {
        return Ok(());
    };
    let svg_w = tree.size().width()  as f64;
    let svg_h = tree.size().height() as f64;

    let gap_chord  = 2.0 * radius * (GAP / 2.0).sin();
    let icon_size  = gap_chord * 0.36;
    let circle_r   = icon_size * 0.72;

    let icon_cx = cx;
    let icon_cy = cy - radius;

    // Draw filled circle behind icon
    ctx.save().ok();
    ctx.set_source_rgba(arc_color[0], arc_color[1], arc_color[2], arc_color[3]);
    ctx.arc(icon_cx, icon_cy, circle_r, 0.0, 2.0 * PI);
    ctx.fill().ok();
    ctx.restore().ok();

    // Render SVG to pixmap, then use as mask
    let px = ((icon_size * svg_w / svg_h) as u32).max(1);
    let py = (icon_size as u32).max(1);

    let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(px, py) else {
        return Ok(());
    };
    resvg::render(&tree,
        resvg::tiny_skia::Transform::from_scale(px as f32 / svg_w as f32, py as f32 / svg_h as f32),
        &mut pixmap.as_mut());

    // Convert tiny_skia RGBA premultiplied → Cairo BGRA premultiplied
    let pix_data = pixmap.data();
    let mut cairo_data: Vec<u8> = Vec::with_capacity(pix_data.len());
    for chunk in pix_data.chunks_exact(4) {
        cairo_data.push(chunk[2]); // B
        cairo_data.push(chunk[1]); // G
        cairo_data.push(chunk[0]); // R
        cairo_data.push(chunk[3]); // A
    }
    let stride = cairo::Format::ARgb32.stride_for_width(px).unwrap_or(px as i32 * 4);
    let Ok(icon_surf) = cairo::ImageSurface::create_for_data(
        cairo_data, cairo::Format::ARgb32, px as i32, py as i32, stride,
    ) else {
        return Ok(());
    };

    let off_x = icon_cx - px as f64 / 2.0;
    let off_y = icon_cy - py as f64 / 2.0;
    ctx.save().ok();
    ctx.set_source_rgba(bg_rgba[0], bg_rgba[1], bg_rgba[2], bg_rgba[3]);
    ctx.mask_surface(&icon_surf, off_x, off_y).ok();
    ctx.restore().ok();
    Ok(())
}

fn draw_sprite(
    ctx: &cairo::Context,
    surf: &cairo::ImageSurface,
    cx: f64, cy: f64, scale: f64,
) -> anyhow::Result<()> {
    let dw = surf.width()  as f64 * scale;
    let dh = surf.height() as f64 * scale;
    ctx.save().ok();
    ctx.translate(cx - dw / 2.0, cy - dh / 2.0);
    ctx.scale(scale, scale);
    ctx.set_source_surface(surf, 0.0, 0.0).context("set_source_surface")?;
    ctx.rectangle(0.0, 0.0, surf.width() as f64, surf.height() as f64);
    ctx.fill().context("sprite fill")?;
    ctx.restore().ok();
    Ok(())
}

fn draw_anc_row(
    ctx: &cairo::Context,
    w: f64, anc_y: f64, anc_h: f64,
    active_key: &str,
    highlight: [f64; 4], text: [f64; 4],
    supported: Option<&[&str]>,
    font_scale: f64,
) -> anyhow::Result<()> {
    let modes: Vec<(&str, &str)> = ANC_MODES
        .iter()
        .filter(|(k, _)| supported.map_or(true, |s| s.contains(k)))
        .copied()
        .collect();
    let n = modes.len();
    if n == 0 {
        return Ok(());
    }

    let cell   = w / n as f64;
    let icon_r = anc_h * 0.28;
    let icon_cy = anc_y + anc_h * 0.42;
    let max_w  = cell * 0.96;

    let mut label_font = (anc_h * 0.20 * font_scale).max(7.0);
    let longest = modes.iter().map(|(_, l)| l).max_by_key(|s| s.len()).unwrap_or(&"");
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    ctx.set_font_size(label_font);
    while ctx.text_extents(longest).map(|e| e.width()).unwrap_or(0.0) > max_w
        && label_font > 7.0 * font_scale
    {
        label_font -= 0.5;
        ctx.set_font_size(label_font);
    }

    for (i, (mode_key, mode_label)) in modes.iter().enumerate() {
        let cx = cell * i as f64 + cell / 2.0;
        draw_anc_icon(ctx, cx, icon_cy, icon_r, mode_key, active_key,
                      highlight, text)?;

        let is_active = *mode_key == active_key;
        let color = if is_active {
            highlight
        } else {
            [text[0], text[1], text[2], 0.30]
        };
        ctx.set_source_rgba(color[0], color[1], color[2], color[3]);
        if is_active {
            ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        } else {
            ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        }
        ctx.set_font_size(label_font);

        let label_y = icon_cy + icon_r + anc_h * 0.10;
        if let Ok(ext) = ctx.text_extents(mode_label) {
            ctx.move_to(
                cx - ext.width() / 2.0 - ext.x_bearing(),
                label_y - ext.y_bearing(),
            );
            let _ = ctx.show_text(mode_label);
        }
    }
    Ok(())
}

fn draw_anc_icon(
    ctx: &cairo::Context,
    cx: f64, cy: f64, r: f64,
    mode_key: &str, active_key: &str,
    highlight: [f64; 4], text: [f64; 4],
) -> anyhow::Result<()> {
    let is_active = mode_key == active_key;
    let color = if is_active {
        highlight
    } else {
        [text[0], text[1], text[2], 0.30]
    };

    let lw = (r * 0.10).max(1.5);
    ctx.set_line_width(lw);
    ctx.set_line_cap(cairo::LineCap::Round);
    ctx.set_source_rgba(color[0], color[1], color[2], color[3]);

    match mode_key {
        "noise_cancellation" => {
            ctx.set_dash(&[], 0.0);
            ctx.new_sub_path();
            ctx.arc(cx, cy, r, 0.0, 2.0 * PI);
            ctx.stroke().ok();
        }
        "off" => {
            let unit = r * 0.35;
            ctx.set_dash(&[unit, unit * 0.65], 0.0);
            ctx.new_sub_path();
            ctx.arc(cx, cy, r, 0.0, 2.0 * PI);
            ctx.stroke().ok();
            ctx.set_dash(&[], 0.0);
        }
        "transparency" => {
            let unit = r * 0.18;
            ctx.set_dash(&[unit, unit * 0.9], 0.0);
            ctx.new_sub_path();
            ctx.arc(cx, cy, r, 0.0, 2.0 * PI);
            ctx.stroke().ok();
            ctx.set_dash(&[], 0.0);
        }
        "adaptive" => {
            ctx.set_dash(&[], 0.0);
            ctx.new_sub_path();
            ctx.arc(cx, cy, r, PI, 2.0 * PI);
            ctx.stroke().ok();
            let unit = r * 0.30;
            ctx.set_dash(&[unit, unit * 0.65], 0.0);
            ctx.new_sub_path();
            ctx.arc(cx, cy, r, 0.0, PI);
            ctx.stroke().ok();
            ctx.set_dash(&[], 0.0);

            // Sparkle star
            let star_cx = cx + r * 0.72;
            let star_cy = cy - r * 0.72;
            let sr  = r * 0.46;
            let sw  = sr * 0.30;
            ctx.set_line_width(0.0);
            for &angle in &[0.0f64, PI / 2.0] {
                ctx.save().ok();
                ctx.translate(star_cx, star_cy);
                ctx.rotate(angle);
                ctx.new_path();
                ctx.move_to(0.0, -sr);
                ctx.curve_to( sw, -sw,  sw,  sw, 0.0,  sr);
                ctx.curve_to(-sw,  sw, -sw, -sw, 0.0, -sr);
                ctx.close_path();
                ctx.fill().ok();
                ctx.restore().ok();
            }
            ctx.set_source_rgba(color[0], color[1], color[2], color[3]);
            ctx.set_line_width(lw * 0.9);
        }
        _ => {}
    }

    // Ear icon centered in the ring (hearing-symbolic or audio-headphones-symbolic)
    draw_ear(ctx, cx, cy, r, color);

    Ok(())
}

/// Renders the ear/headphone SVG icon centered at (cx, cy), tinted with `color`.
/// Falls back silently if the icon cannot be found or loaded.
fn draw_ear(
    ctx: &cairo::Context,
    cx: f64, cy: f64, r: f64,
    color: [f64; 4],
) {
    let svg_data = icons::find_ear_icon_bytes();
    let opt  = resvg::usvg::Options::default();
    let Ok(tree) = resvg::usvg::Tree::from_data(&svg_data, &opt) else { return; };

    let icon_size = r * 1.4;
    let px = (icon_size as u32).max(1);

    let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(px, px) else { return; };
    let svg_w = tree.size().width();
    let svg_h = tree.size().height();
    let scale_x = px as f32 / svg_w;
    let scale_y = px as f32 / svg_h;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale_x, scale_y),
        &mut pixmap.as_mut(),
    );

    // Convert tiny_skia RGBA premultiplied → Cairo BGRA premultiplied
    let pix_data = pixmap.data();
    let mut cairo_data: Vec<u8> = Vec::with_capacity(pix_data.len());
    for chunk in pix_data.chunks_exact(4) {
        cairo_data.push(chunk[2]); // B
        cairo_data.push(chunk[1]); // G
        cairo_data.push(chunk[0]); // R
        cairo_data.push(chunk[3]); // A
    }
    let stride = cairo::Format::ARgb32.stride_for_width(px).unwrap_or(px as i32 * 4);
    let Ok(icon_surf) = cairo::ImageSurface::create_for_data(
        cairo_data, cairo::Format::ARgb32, px as i32, px as i32, stride,
    ) else { return; };

    let off_x = cx - icon_size / 2.0;
    let off_y = cy - icon_size / 2.0;
    ctx.save().ok();
    ctx.set_source_rgba(color[0], color[1], color[2], color[3]);
    ctx.mask_surface(&icon_surf, off_x, off_y).ok();
    ctx.restore().ok();
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use pbcommon::{AncMode, BudsState, Event};

    fn sample_state() -> BudsState {
        BudsState {
            schema: 1,
            event: Event::Update,
            left: Some(85), right: Some(87), case: Some(34),
            anc: AncMode::NoiseCancellation,
            anc_supported: vec![AncMode::NoiseCancellation, AncMode::Off, AncMode::Transparency],
            mac: Some("AB:CD:EF:01:23:45".into()),
            ..Default::default()
        }
    }

    #[test]
    fn render_connected_smoke() {
        let state = sample_state();
        let cfg = Config::default();
        let out = render(&state, &cfg).expect("render() must succeed with default config");
        assert_eq!(out.width, cfg.image_size as i32);
        assert!(out.height > 0);
    }

    #[test]
    fn render_disconnected_smoke() {
        let mut state = sample_state();
        state.event = Event::Disconnect;
        let cfg = Config::default();
        let out = render_disconnected(&state, &cfg)
            .expect("render_disconnected() must succeed with default config");
        assert_eq!(out.width, cfg.image_size as i32);
        assert!(out.height > 0);
    }

    #[test]
    fn render_respects_image_size() {
        let state = sample_state();
        let mut cfg = Config::default();
        cfg.image_size = 300;
        let out = render(&state, &cfg).expect("render with image_size=300");
        assert_eq!(out.width, 300);
    }

    #[test]
    fn hex_to_rgba_rgb() {
        let [r, g, b, a] = hex_to_rgba("#ff8000");
        assert!((r - 1.0).abs() < 0.01);
        assert!((g - (128.0 / 255.0)).abs() < 0.01);
        assert_eq!(b, 0.0);
        assert_eq!(a, 1.0);
    }

    #[test]
    fn hex_to_rgba_rgba() {
        let [_, _, _, a] = hex_to_rgba("#ff000080");
        assert!((a - (128.0 / 255.0)).abs() < 0.01);
    }
}
