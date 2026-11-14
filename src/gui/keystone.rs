//! The Keystone mark, drawn from three layers so the keystone can move on its own: the stones
//! for each theme, and the keystone sitting in its seat. design/logo/layers.html renders them.
//!
//! The keystone sits raised by 2.6 units of the 48-unit mark when something needs a look, and
//! drops into its seat when everything is fixed.

use eframe::egui::{self, Color32, ColorImage, Rect, TextureHandle, TextureOptions, Ui};

/// How far the keystone sits above its seat in the logo, in units of the 48-unit mark.
pub const RAISED: f32 = 2.6;

#[derive(Clone)]
struct Layers {
    stones_light: TextureHandle,
    stones_dark: TextureHandle,
    keystone: TextureHandle,
}

fn decode(bytes: &[u8]) -> Result<ColorImage, png::DecodingError> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(png::DecodingError::LimitsExceeded);
    }
    Ok(ColorImage::from_rgba_unmultiplied([info.width as usize, info.height as usize], &buf[..info.buffer_size()]))
}

/// Area-averages a square image down to `px` a side, the way an icon is drawn for one size:
/// every source pixel counts in proportion to how much of each target pixel it covers. Never
/// scales up.
fn shrink(img: &ColorImage, px: usize) -> ColorImage {
    let n = img.size[0];
    if px >= n {
        return img.clone();
    }
    let scale = n as f32 / px as f32;
    // For each target row or column: the source ones it overlaps, and by how much.
    let taps: Vec<Vec<(usize, f32)>> = (0..px)
        .map(|t| {
            let (a, b) = (t as f32 * scale, (t + 1) as f32 * scale);
            (a.floor() as usize..(b.ceil() as usize).min(n)).map(|s| (s, ((s + 1) as f32).min(b) - (s as f32).max(a))).filter(|&(_, w)| w > 0.0).collect()
        })
        .collect();
    let area = scale * scale;
    let mut out = Vec::with_capacity(px * px);
    for ty in &taps {
        for tx in &taps {
            let mut sum = [0.0_f32; 4];
            for &(sy, wy) in ty {
                for &(sx, wx) in tx {
                    // Colour32 is premultiplied, so a plain weighted sum keeps clear pixels clear.
                    let c = img.pixels[sy * n + sx].to_array();
                    for (acc, v) in sum.iter_mut().zip(c) {
                        *acc += v as f32 * wx * wy;
                    }
                }
            }
            let [r, g, b, a] = sum.map(|v| (v / area).round().min(255.0) as u8);
            out.push(Color32::from_rgba_premultiplied(r, g, b, a));
        }
    }
    ColorImage::new([px, px], out)
}

/// The mark's layers drawn for one size in physical pixels, so the screen shows them pixel for
/// pixel instead of blurring a 256 px texture down to 24.
fn layers(ctx: &egui::Context, px: usize) -> Option<Layers> {
    let id = egui::Id::new(("bootmend-keystone", px));
    if let Some(l) = ctx.data(|d| d.get_temp::<Option<Layers>>(id)) {
        return l;
    }
    let load = |name: &str, bytes: &[u8]| match decode(bytes) {
        Ok(img) => Some(ctx.load_texture(format!("{name}-{px}"), shrink(&img, px), TextureOptions::LINEAR)),
        Err(e) => {
            eprintln!("the {name} layer of the logo didn't load, so the mark is hidden: {e}");
            None
        }
    };
    let l = (|| {
        Some(Layers {
            stones_light: load("keystone-stones-light", include_bytes!("../../assets/keystone/stones-light.png"))?,
            stones_dark: load("keystone-stones-dark", include_bytes!("../../assets/keystone/stones-dark.png"))?,
            keystone: load("keystone", include_bytes!("../../assets/keystone/keystone.png"))?,
        })
    })();
    ctx.data_mut(|d| d.insert_temp(id, l.clone()));
    l
}

/// Paints the mark in a square `rect`. Each of `keys` is a keystone to draw: how many units it
/// sits above its seat (negative sinks it a touch, for the landing) and its opacity. A fade
/// between two positions draws two.
pub fn paint(ui: &Ui, rect: Rect, keys: &[(f32, f32)]) {
    // Whole physical pixels, so each texel lands on one screen pixel.
    let ppp = ui.ctx().pixels_per_point();
    let px = (rect.width() * ppp).round().max(1.0) as usize;
    let Some(l) = layers(ui.ctx(), px) else { return };
    let min = (rect.min.to_vec2() * ppp).round() / ppp;
    let rect = Rect::from_min_size(min.to_pos2(), egui::Vec2::splat(px as f32 / ppp));
    let stones = if ui.visuals().dark_mode { &l.stones_dark } else { &l.stones_light };
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let painter = ui.painter();
    painter.image(stones.id(), rect, uv, Color32::WHITE);
    for &(lift, opacity) in keys {
        let up = lift * rect.height() / 48.0;
        // Raised or seated, the keystone sits on whole pixels; in flight it moves smoothly.
        let up = if lift == RAISED || lift == 0.0 { (up * ppp).round() / ppp } else { up };
        painter.image(l.keystone.id(), rect.translate(egui::vec2(0.0, -up)), uv, Color32::WHITE.gamma_multiply(opacity));
    }
}

/// How long the keystone takes to drop into its seat.
pub const DROP_MS: f32 = 360.0;

/// Where the keystone is `t` of the way through its drop (0 to 1), in units above its seat. It
/// falls with gravity, settles a hair below the seat at 78% and comes to rest, with no bounce.
pub fn lift_at(t: f32) -> f32 {
    const SETTLE: f32 = -0.18;
    const LAND: f32 = 0.78;
    let t = t.clamp(0.0, 1.0);
    if t < LAND {
        let k = t / LAND;
        RAISED + (SETTLE - RAISED) * k * k
    } else {
        let k = (t - LAND) / (1.0 - LAND);
        SETTLE * (1.0 - k) * (1.0 - k)
    }
}

/// The logo as it always looks: keystone raised. For the sidebar; it never changes.
pub fn mark(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    paint(ui, rect, &[(RAISED, 1.0)]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layer_decodes_at_full_size() {
        for bytes in [
            &include_bytes!("../../assets/keystone/stones-light.png")[..],
            &include_bytes!("../../assets/keystone/stones-dark.png")[..],
            &include_bytes!("../../assets/keystone/keystone.png")[..],
        ] {
            let img = decode(bytes).expect("an RGBA PNG");
            assert_eq!(img.size, [256, 256]);
            assert!(img.pixels.iter().any(|p| p.a() == 0), "the background must be transparent");
        }
    }

    #[test]
    fn shrinking_averages_each_target_pixel_over_the_area_it_covers() {
        // A 4x4 image, left half opaque white and right half clear, shrunk to 2x2 and to 1x1.
        let px: Vec<Color32> = (0..16).map(|i| if i % 4 < 2 { Color32::WHITE } else { Color32::TRANSPARENT }).collect();
        let img = ColorImage::new([4, 4], px);
        let two = shrink(&img, 2);
        assert_eq!(two.size, [2, 2]);
        assert_eq!(two.pixels, vec![Color32::WHITE, Color32::TRANSPARENT, Color32::WHITE, Color32::TRANSPARENT]);
        let one = shrink(&img, 1);
        assert!((127..=128).contains(&one.pixels[0].a()), "half covered is half opaque, got {}", one.pixels[0].a());
        // 4 to 3 splits source pixels between two targets; nothing is lost or invented.
        let three = shrink(&ColorImage::filled([4, 4], Color32::WHITE), 3);
        assert!(three.pixels.iter().all(|&c| c == Color32::WHITE));
    }

    #[test]
    fn the_logo_is_drawn_from_a_texture_its_exact_size() {
        let img = decode(include_bytes!("../../assets/keystone/keystone.png")).unwrap();
        assert_eq!(shrink(&img, 24).size, [24, 24]);
        assert_eq!(shrink(&img, 999).size, [256, 256], "never scaled up past the artwork");
    }

    #[test]
    fn the_keystone_falls_settles_and_rests_without_bouncing() {
        assert_eq!(lift_at(0.0), RAISED);
        assert!((lift_at(0.78) + 0.18).abs() < 1e-4, "settles a hair below the seat");
        assert_eq!(lift_at(1.0), 0.0);
        let path: Vec<f32> = (0..=78).map(|i| lift_at(i as f32 / 100.0)).collect();
        assert!(path.windows(2).all(|w| w[1] <= w[0]), "only ever falls on the way down");
        let rest: Vec<f32> = (78..=100).map(|i| lift_at(i as f32 / 100.0)).collect();
        assert!(rest.windows(2).all(|w| w[1] >= w[0] && w[1] <= 0.0), "comes up to the seat and no further");
    }
}
