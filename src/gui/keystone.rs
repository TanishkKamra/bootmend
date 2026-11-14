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

/// Paints the mark in a square `rect`, with the keystone `lift` units above its seat (negative
/// sinks it a touch, for the landing) and drawn at `opacity`.
pub fn paint(ui: &Ui, rect: Rect, lift: f32, opacity: f32) {
    let Some(l) = layers(ui.ctx()) else { return };
    let stones = if ui.visuals().dark_mode { &l.stones_dark } else { &l.stones_light };
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let painter = ui.painter();
    painter.image(stones.id(), rect, uv, Color32::WHITE);
    let up = lift * rect.height() / 48.0;
    painter.image(l.keystone.id(), rect.translate(egui::vec2(0.0, -up)), uv, Color32::WHITE.gamma_multiply(opacity));
}

/// The logo as it always looks: keystone raised. For the sidebar; it never changes.
pub fn mark(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    paint(ui, rect, RAISED, 1.0);
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
}
