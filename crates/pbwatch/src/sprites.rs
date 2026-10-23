use anyhow::Context;

pub struct SpriteSet {
    pub left:         &'static [u8],
    pub right:        &'static [u8],
    pub case:         &'static [u8],
    pub disconnected: &'static [u8],
}

pub static SPRITES_PRO2: SpriteSet = SpriteSet {
    left:         include_bytes!("../assets/sprites/pro2/left.png"),
    right:        include_bytes!("../assets/sprites/pro2/right.png"),
    case:         include_bytes!("../assets/sprites/pro2/case.png"),
    disconnected: include_bytes!("../assets/sprites/pro2/disconnected.png"),
};

pub static SPRITES_PRO: SpriteSet = SpriteSet {
    left:         include_bytes!("../assets/sprites/pro/left.png"),
    right:        include_bytes!("../assets/sprites/pro/right.png"),
    case:         include_bytes!("../assets/sprites/pro/case.png"),
    disconnected: include_bytes!("../assets/sprites/pro/disconnected.png"),
};

pub static SPRITES_A_SERIES: SpriteSet = SpriteSet {
    left:         include_bytes!("../assets/sprites/a-series/left.png"),
    right:        include_bytes!("../assets/sprites/a-series/right.png"),
    case:         include_bytes!("../assets/sprites/a-series/case.png"),
    disconnected: include_bytes!("../assets/sprites/a-series/disconnected.png"),
};

pub static SPRITES_DEFAULT: SpriteSet = SpriteSet {
    left:         include_bytes!("../assets/sprites/default/left.png"),
    right:        include_bytes!("../assets/sprites/default/right.png"),
    case:         include_bytes!("../assets/sprites/default/case.png"),
    disconnected: include_bytes!("../assets/sprites/default/disconnected.png"),
};

pub fn get_set(model_key: &str) -> &'static SpriteSet {
    match model_key {
        "pro2"     => &SPRITES_PRO2,
        "pro"      => &SPRITES_PRO,
        "a-series" => &SPRITES_A_SERIES,
        _          => &SPRITES_DEFAULT,
    }
}

pub fn load_surface(bytes: &[u8]) -> anyhow::Result<cairo::ImageSurface> {
    cairo::ImageSurface::create_from_png(&mut std::io::Cursor::new(bytes))
        .context("failed to decode sprite PNG")
}
