/// Converts a Cairo ARGB32 buffer to FDO RGBA (non-premultiplied).
///
/// Cairo ARGB32 stores pixels as [B, G, R, A] in memory (little-endian).
/// FDO image-data expects [R, G, B, A] non-premultiplied bytes.
///
/// `data`   — raw surface bytes (stride × height bytes total)
/// `width`  — image width in pixels
/// `height` — image height in pixels
/// `stride` — Cairo stride in bytes (may be wider than width × 4)
///
/// Returns exactly `width × height × 4` bytes.
pub fn cairo_argb32_to_fdo_rgba(
    data:   &[u8],
    width:  usize,
    height: usize,
    stride: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(width * height * 4);

    for row in 0..height {
        let row_base = row * stride;
        for col in 0..width {
            let off = row_base + col * 4;
            let b = data[off];
            let g = data[off + 1];
            let r = data[off + 2];
            let a = data[off + 3];

            let (rr, gg, bb) = if a == 0 {
                (0u8, 0u8, 0u8)
            } else if a == 255 {
                (r, g, b)
            } else {
                let a32 = a as u32;
                let unp = |c: u8| -> u8 {
                    ((c as u32 * 255 + a32 / 2) / a32).min(255) as u8
                };
                (unp(r), unp(g), unp(b))
            };

            out.extend_from_slice(&[rr, gg, bb, a]);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_red_pixel() {
        // Cairo ARGB32 LE: [B=0, G=0, R=255, A=255] → FDO [R=255, G=0, B=0, A=255]
        let data = [0u8, 0, 255, 255];
        let out = cairo_argb32_to_fdo_rgba(&data, 1, 1, 4);
        assert_eq!(out, [255, 0, 0, 255]);
    }

    #[test]
    fn transparent_pixel() {
        // Alpha=0 → [0, 0, 0, 0]
        let data = [128u8, 64, 200, 0];
        let out = cairo_argb32_to_fdo_rgba(&data, 1, 1, 4);
        assert_eq!(out, [0, 0, 0, 0]);
    }

    #[test]
    fn half_alpha_unpremultiply() {
        // Cairo BGRA: [B=0, G=0, R=128, A=128] — red at 50% alpha, premultiplied R=128
        // Unpremultiplied R = (128*255 + 64) / 128 = (32640 + 64) / 128 = 32704 / 128 = 255
        let data = [0u8, 0, 128, 128];
        let out = cairo_argb32_to_fdo_rgba(&data, 1, 1, 4);
        assert_eq!(out[0], 255); // R
        assert_eq!(out[1], 0);   // G
        assert_eq!(out[2], 0);   // B
        assert_eq!(out[3], 128); // A unchanged
    }

    #[test]
    fn stride_padding_ignored() {
        // 2×1 image with stride=16 (8 bytes of padding after the 2 pixels)
        let mut data = vec![0u8; 16];
        // Pixel 0: opaque red [B=0, G=0, R=255, A=255]
        data[0] = 0; data[1] = 0; data[2] = 255; data[3] = 255;
        // Pixel 1: opaque blue [B=255, G=0, R=0, A=255]
        data[4] = 255; data[5] = 0; data[6] = 0; data[7] = 255;
        // bytes 8-15: stride padding (garbage)
        data[8..16].fill(0xAA);

        let out = cairo_argb32_to_fdo_rgba(&data, 2, 1, 16);
        assert_eq!(out.len(), 8);
        assert_eq!(&out[0..4], &[255, 0, 0, 255]); // red
        assert_eq!(&out[4..8], &[0, 0, 255, 255]);  // blue
    }
}
