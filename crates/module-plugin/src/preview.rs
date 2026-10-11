//! Preview and anm2 use the same shaping, font resolution and rasterization.
use crate::{
    font_coverage::FontManagerGlyphCoverage,
    render::{RenderRequest, layout_options, prepare},
    shaping::{HostFonts, rasterize},
};
use aviutl2::generic::ReadSection;
use compositefont_core::CompositeFontProfile;

pub struct PreviewBitmap {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}
pub struct PreviewOptions {
    pub size: f64,
    pub width: u32,
    pub height: u32,
    pub baseline_guide: bool,
    pub bounds_guide: bool,
}
pub fn render_preview(
    section: &ReadSection,
    profile: &CompositeFontProfile,
    text: &str,
    options: PreviewOptions,
) -> Result<PreviewBitmap, String> {
    let PreviewOptions {
        size,
        width,
        height,
        baseline_guide,
        bounds_guide,
    } = options;
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || !size.is_finite()
        || !(1.0..=256.0).contains(&size)
    {
        return Err("プレビューの寸法が不正です。".into());
    }
    let request = RenderRequest {
        text: text.into(),
        profile: Some(profile.clone()),
        font: "Yu Gothic UI".into(),
        size,
        spacing: 0.0,
        line_spacing: 0.0,
        color: "f4f4f4".into(),
        align: 0,
        bold: false,
        italic: false,
        disable_ligatures: false,
    };
    let mut coverage = FontManagerGlyphCoverage::new(section);
    // Preview text is already logical text, not the SDK's escaped alias.
    let plan = prepare(&request, text, &mut coverage)?
        .ok_or("プレビューに未対応の書式または補正値です。")?;
    let mut layout = layout_options(&request, Some(width.saturating_sub(32).max(1) as f32));
    // Per-glyph analyses are needed only for the settings preview guides.
    layout.glyph_guides = bounds_guide;
    let shaped = rasterize(&mut HostFonts::new(section), &plan, &layout)?;
    let mut bitmap = PreviewBitmap {
        width,
        height,
        bgra: [0x1b, 0x19, 0x18, 255].repeat((width * height) as usize),
    };
    for y in 0..shaped.bitmap.height.min(height.saturating_sub(16) as i32) {
        for x in 0..shaped.bitmap.width.min(width.saturating_sub(16) as i32) {
            let src = ((y * shaped.bitmap.width + x) * 4) as usize;
            let dst = (((y + 16) as u32 * width + (x + 16) as u32) * 4) as usize;
            let alpha = u32::from(shaped.bitmap.bgra[src + 3]);
            for c in 0..3 {
                bitmap.bgra[dst + c] = ((u32::from(bitmap.bgra[dst + c]) * (255 - alpha)
                    + u32::from(shaped.bitmap.bgra[src + c]) * alpha)
                    / 255) as u8;
            }
        }
    }
    // The logical cell and painted outline are separate. Native glyph indices
    // keep a ligature as one glyph and retain positioned combining marks.
    if bounds_guide {
        for guide in &shaped.guides {
            outline(&mut bitmap, guide.layout_bounds, [255, 0, 255, 255]);
        }
        for guide in &shaped.guides {
            outline(&mut bitmap, guide.bounds, [255, 255, 0, 255]);
        }
    }
    if baseline_guide {
        for guide in &shaped.guides {
            for x in guide.left.floor() as i32..guide.right.ceil() as i32 {
                put(
                    &mut bitmap,
                    x + 16,
                    guide.baseline.round() as i32 + 16,
                    [0, 0, 255, 255],
                );
            }
        }
    }
    Ok(bitmap)
}
fn outline(bitmap: &mut PreviewBitmap, bounds: windows::Win32::Foundation::RECT, color: [u8; 4]) {
    if bounds.right <= bounds.left || bounds.bottom <= bounds.top {
        return;
    }
    for x in bounds.left..bounds.right {
        put(bitmap, x + 16, bounds.top + 16, color);
        put(bitmap, x + 16, bounds.bottom + 15, color);
    }
    for y in bounds.top..bounds.bottom {
        put(bitmap, bounds.left + 16, y + 16, color);
        put(bitmap, bounds.right + 15, y + 16, color);
    }
}
fn put(bitmap: &mut PreviewBitmap, x: i32, y: i32, color: [u8; 4]) {
    if x >= 0 && y >= 0 && x < bitmap.width as i32 && y < bitmap.height as i32 {
        let index = (y as usize * bitmap.width as usize + x as usize) * 4;
        bitmap.bgra[index..index + 4].copy_from_slice(&color);
    }
}
