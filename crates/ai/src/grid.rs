//! A labelled reference grid drawn over an image before a model reads it,
//! so the model can say *where* it sees something ("flooding in D5") and
//! the answer can be put back on the map.
//!
//! Columns A-H run west to east, rows 1-8 north to south, like a map sheet.

use base64::Engine;

/// Cells per side.
pub const GRID: usize = 8;

const GLYPHS: [(char, [u8; 7]); 16] = [
    ('A', [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11]),
    ('B', [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e]),
    ('C', [0x0e, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0e]),
    ('D', [0x1e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1e]),
    ('E', [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f]),
    ('F', [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10]),
    ('G', [0x0e, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0f]),
    ('H', [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11]),
    ('1', [0x04, 0x0c, 0x04, 0x04, 0x04, 0x04, 0x0e]),
    ('2', [0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f]),
    ('3', [0x1f, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0e]),
    ('4', [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02]),
    ('5', [0x1f, 0x10, 0x1e, 0x01, 0x01, 0x11, 0x0e]),
    ('6', [0x06, 0x08, 0x10, 0x1e, 0x11, 0x11, 0x0e]),
    ('7', [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08]),
    ('8', [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e]),
];

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Canvas {
    fn set(&mut self, x: usize, y: usize, c: [u8; 4], alpha: f32) {
        if x >= self.w || y >= self.h {
            return;
        }
        let o = (y * self.w + x) * 4;
        for (dst, src) in self.px[o..o + 3].iter_mut().zip(c) {
            *dst = (f32::from(*dst) * (1.0 - alpha) + f32::from(src) * alpha).round() as u8;
        }
        self.px[o + 3] = 255;
    }

    fn text(&mut self, x: usize, y: usize, s: &str, scale: usize) {
        let w = s.chars().count() * 6 * scale + scale;
        for yy in y..y + 9 * scale {
            for xx in x..x + w {
                self.set(xx, yy, [0, 0, 0, 255], 0.65);
            }
        }
        for (i, ch) in s.chars().enumerate() {
            let Some((_, rows)) = GLYPHS.iter().find(|(c, _)| *c == ch) else {
                continue;
            };
            for (r, bits) in rows.iter().enumerate() {
                for col in 0..5 {
                    if bits >> (4 - col) & 1 == 1 {
                        for dy in 0..scale {
                            for dx in 0..scale {
                                self.set(
                                    x + scale + (i * 6 + col) * scale + dx,
                                    y + scale + r * scale + dy,
                                    [255, 255, 255, 255],
                                    1.0,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

fn label(col: usize, row: usize) -> String {
    format!("{}{}", (b'A' + col as u8) as char, row + 1)
}

/// Draw the grid over a PNG (base64) and return the new PNG (base64).
/// Transparent pixels (outside the scene) become black.
pub fn overlay_grid(png_base64: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png_base64)
        .map_err(|e| e.to_string())?;
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let ch = info.color_type.samples();
    let mut c = Canvas {
        w,
        h,
        px: vec![0; w * h * 4],
    };
    for i in 0..w * h {
        let s = &buf[i * ch..(i + 1) * ch];
        let (rgb, a) = match ch {
            1 => ([s[0]; 3], 255),
            2 => ([s[0]; 3], s[1]),
            3 => ([s[0], s[1], s[2]], 255),
            _ => ([s[0], s[1], s[2]], s[3]),
        };
        let a = f32::from(a) / 255.0;
        for (dst, src) in c.px[i * 4..i * 4 + 3].iter_mut().zip(rgb) {
            *dst = (f32::from(src) * a).round() as u8;
        }
        c.px[i * 4 + 3] = 255;
    }
    let scale = if w >= 600 { 2 } else { 1 };
    for k in 1..GRID {
        let x = k * w / GRID;
        let y = k * h / GRID;
        for t in 0..h {
            c.set(x, t, [255, 255, 255, 255], 0.55);
        }
        for t in 0..w {
            c.set(t, y, [255, 255, 255, 255], 0.55);
        }
    }
    for row in 0..GRID {
        for col in 0..GRID {
            c.text(
                col * w / GRID + 2,
                row * h / GRID + 2,
                &label(col, row),
                scale,
            );
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&c.px).map_err(|e| e.to_string())?;
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(out))
}

/// Centre of a grid cell such as `"C4"` in the coordinates of `bounds`
/// (west, south, east, north).
pub fn cell_center(cell: &str, bounds: [f64; 4]) -> Option<[f64; 2]> {
    let cell = cell.trim().to_ascii_uppercase();
    let mut chars = cell.chars();
    let col = chars.next()? as i64 - 'A' as i64;
    let row: i64 = chars.as_str().parse().ok()?;
    if !(0..GRID as i64).contains(&col) || !(1..=GRID as i64).contains(&row) {
        return None;
    }
    let [w, s, e, n] = bounds;
    let size = |a: f64, b: f64| (b - a) / GRID as f64;
    Some([
        w + (col as f64 + 0.5) * size(w, e),
        n - (row as f64 - 0.5) * size(s, n),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_map_to_the_right_place() {
        let b = [-800.0, -800.0, 800.0, 800.0];
        assert_eq!(cell_center("A1", b), Some([-700.0, 700.0]));
        assert_eq!(cell_center("h8", b), Some([700.0, -700.0]));
        assert_eq!(cell_center("I1", b), None);
        assert_eq!(cell_center("A9", b), None);
        assert_eq!(cell_center("", b), None);
    }

    #[test]
    fn overlay_keeps_size_and_draws() {
        let (w, h) = (64usize, 64usize);
        let mut raw = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut raw, w as u32, h as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut wr = enc.write_header().unwrap();
            wr.write_image_data(&vec![40u8; w * h * 4]).unwrap();
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(raw);
        let out = overlay_grid(&b64).unwrap();
        assert_ne!(out, b64);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(out)
            .unwrap();
        let dec = png::Decoder::new(std::io::Cursor::new(bytes));
        let r = dec.read_info().unwrap();
        assert_eq!((r.info().width, r.info().height), (64, 64));
    }
}
