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

fn layers(ctx: &egui::Context) -> Option<Layers> {
    let id = egui::Id::new("bootmend-keystone");
    if let Some(l) = ctx.data(|d| d.get_temp::<Option<Layers>>(id)) {
        return l;
    }
    // Mipmaps keep the mark clean at sidebar size, a tenth of the drawn size.
    let opts = TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..TextureOptions::LINEAR };
    let load = |name: &str, bytes: &[u8]| match decode(bytes) {
        Ok(img) => Some(ctx.load_texture(name, img, opts)),
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
    let Some(l) = layers(ui.ctx()) else { return };
    let stones = if ui.visuals().dark_mode { &l.stones_dark } else { &l.stones_light };
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let painter = ui.painter();
    painter.image(stones.id(), rect, uv, Color32::WHITE);
    for &(lift, opacity) in keys {
        let up = lift * rect.height() / 48.0;
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
