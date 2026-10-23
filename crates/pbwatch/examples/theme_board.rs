/// Renders a visual board of all available themes.
///
/// For each theme, three gauges are shown:
///   Left  80% — highlight_color (ok state)
///   Case  35% — warn_color      (below battery_warn default of 50%)
///   Right  5% — critical_color  (below battery_critical default of 10%)
///
/// Output: /tmp/pbwatch-theme-board.png
use pbcommon::{AncMode, BudsState, Event};
use pbwatch::{config::Config, render::render};

fn main() -> anyhow::Result<()> {
    let themes = [
        "Solarized Dark", "Gruvbox Dark", "Catppuccin Mocha", "Rosé Pine",
        "Everforest Dark", "Nord", "Dracula", "Tokyo Night", "Kanagawa", "SynthWave '84",
        "Solarized Light", "Gruvbox Light", "Catppuccin Latte", "Rosé Pine Dawn",
        "Everforest Light", "Nord Light", "GitHub Light", "Ayu Light",
        "One Light", "PaperColor Light",
    ];

    let state = BudsState {
        schema:        1,
        event:         Event::Update,
        left:          Some(80),   // ok → highlight_color
        case:          Some(35),   // warn → warn_color
        right:         Some(5),    // critical → critical_color
        anc:           AncMode::NoiseCancellation,
        anc_supported: vec![AncMode::NoiseCancellation, AncMode::Off, AncMode::Transparency],
        mac:           Some("AB:CD:EF:01:23:45".into()),
        device_name:   Some("Paul's Pixel Buds Pro 2".into()),
        ..Default::default()
    };

    let mut row_surfaces: Vec<cairo::ImageSurface> = Vec::new();

    for theme_name in &themes {
        let mut cfg = Config::default();
        cfg.theme = Some(theme_name.to_string());
        cfg.sprite_set = Some("pro2".to_string());
        cfg.opacity = 1.0;
        cfg.validate_and_clamp();

        let out = render(&state, &cfg)?;

        // Label strip above the widget
        let label_h = 22_i32;
        let total_h = label_h + out.height;
        let strip = cairo::ImageSurface::create(cairo::Format::ARgb32, out.width, total_h)?;
        let ctx  = cairo::Context::new(&strip)?;

        // Background for label (slightly darker than widget bg)
        let bg = pbwatch::render::hex_to_rgba(&cfg.bg_color);
        ctx.set_source_rgba(bg[0] * 0.85, bg[1] * 0.85, bg[2] * 0.85, 1.0);
        ctx.rectangle(0.0, 0.0, out.width as f64, label_h as f64);
        ctx.fill()?;

        // Theme name
        let text = pbwatch::render::hex_to_rgba(&cfg.text_color);
        ctx.set_source_rgba(text[0], text[1], text[2], 1.0);
        ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        ctx.set_font_size(11.0);
        if let Ok(ext) = ctx.text_extents(theme_name) {
            ctx.move_to(
                out.width as f64 / 2.0 - ext.width() / 2.0 - ext.x_bearing(),
                label_h as f64 / 2.0 - ext.height() / 2.0 - ext.y_bearing(),
            );
            let _ = ctx.show_text(theme_name);
        }

        // Widget below label
        ctx.set_source_surface(&out.surface, 0.0, label_h as f64)?;
        ctx.paint()?;

        row_surfaces.push(strip);
    }

    // Arrange in a 2-column grid (10 rows × 2 cols = 20 themes)
    let cols      = 2_i32;
    let cell_w    = row_surfaces[0].width();
    let cell_h    = row_surfaces[0].height();
    let gap       = 6_i32;
    let pad       = 12_i32;
    let board_w   = pad * 2 + cols * cell_w + (cols - 1) * gap;
    let rows      = (themes.len() as i32 + cols - 1) / cols;
    let board_h   = pad * 2 + rows * cell_h + (rows - 1) * gap;

    let board = cairo::ImageSurface::create(cairo::Format::ARgb32, board_w, board_h)?;
    let ctx   = cairo::Context::new(&board)?;

    // Dark neutral background for the board itself
    ctx.set_source_rgb(0.12, 0.12, 0.12);
    ctx.paint()?;

    for (i, surf) in row_surfaces.iter().enumerate() {
        let col = (i as i32) % cols;
        let row = (i as i32) / cols;
        let x   = pad + col * (cell_w + gap);
        let y   = pad + row * (cell_h + gap);
        ctx.set_source_surface(surf, x as f64, y as f64)?;
        ctx.paint()?;
    }

    let out_path = "/tmp/pbwatch-theme-board.png";
    let mut f = std::fs::File::create(out_path)?;
    board.write_to_png(&mut f)?;
    println!("{out_path}");
    Ok(())
}
