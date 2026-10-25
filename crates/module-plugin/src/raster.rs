//! Small image-producing API shared by the optional SVG aux2 and the preview.
use crate::shaping::{self, Fonts, HostFonts, LayoutOptions, StyledText, TextStyle};
use aviutl2::generic::ReadSection;
use unicode_segmentation::UnicodeSegmentation;

pub struct RasterImage {
    pub width: u32,
    pub height: u32,
    /// Straight (unassociated alpha) BGRA pixels.
    pub bgra: Vec<u8>,
    /// Logical origin and baseline measured from the bitmap's top-left corner.
    pub origin_x: f32,
    pub baseline: f32,
    pub advance: f32,
    pub svg_layers: usize,
}

/// Render SVG XML directly at the requested pixel dimensions, without resizing
/// a previously rasterized image. SVG features follow Windows Direct2D.
pub fn svg(xml: &str, width: u32, height: u32) -> Result<RasterImage, String> {
    if xml.is_empty() || xml.len() > 1024 * 1024 || xml.contains('\0') {
        return Err("SVG XML must contain 1..1048576 bytes and no NUL".into());
    }
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || u64::from(width) * u64::from(height) > 16_777_216
    {
        return Err("Invalid SVG pixel dimensions".into());
    }
    let bgra = shaping::color_glyphs::SvgGlyphRenderer::new()
        .and_then(|mut renderer| renderer.draw_document(xml.as_bytes(), width, height))
        .map_err(|e| e.to_string())?;
    Ok(RasterImage {
        width,
        height,
        bgra,
        origin_x: 0.0,
        baseline: height as f32,
        advance: width as f32,
        svg_layers: 0,
    })
}

/// Shape one grapheme (including flags, skin tones and ZWJ sequences), then
/// rasterize its SVG/COLR font layers. `size` is the em size in pixels, not
/// AviUtl2's UI font size. No text parsing, profile resolution or composition.
pub fn glyph(
    section: &ReadSection,
    text: &str,
    family: &str,
    size: f64,
) -> Result<RasterImage, String> {
    glyph_with_fonts(&mut HostFonts::new(section), text, family, size)
}

pub(crate) fn glyph_with_fonts(
    fonts: &mut dyn Fonts,
    text: &str,
    family: &str,
    size: f64,
) -> Result<RasterImage, String> {
    if text.is_empty()
        || text.len() > 1024
        || text.chars().any(char::is_control)
        || text.graphemes(true).count() != 1
        || family.is_empty()
        || family.len() > 256
        || family.contains('\0')
        || !size.is_finite()
        || !(1.0..=4096.0).contains(&size)
    {
        return Err("Expected one grapheme, a font family and an em size of 1..4096 pixels".into());
    }
    let style = TextStyle {
        family: family.into(),
        size: size as f32,
        horizontal: 1.0,
        vertical: 1.0,
        tracking: 0.0,
        baseline_shift: 0.0,
        color: 0xffffff,
        bold: false,
        italic: false,
        disable_ligatures: false,
    };
    glyph_with_style(fonts, text, &style)
}

pub(crate) fn glyph_with_style(
    fonts: &mut dyn Fonts,
    text: &str,
    style: &TextStyle,
) -> Result<RasterImage, String> {
    let mut plan = StyledText::default();
    plan.push(text, style.clone());
    let image = shaping::rasterize(
        fonts,
        &plan,
        &LayoutOptions {
            base_family: style.family.clone(),
            base_size: style.size,
            line_spacing: 0.0,
            column: 0,
            wrap_width: None,
            glyph_guides: false,
            disable_ligatures: false,
            force_ligatures: false,
            antialias: shaping::AntialiasMethod::Natural,
            color_glyphs: true,
        },
    )?;
    let first = image.guides.first().ok_or("The font produced no glyph")?;
    Ok(RasterImage {
        width: image.bitmap.width as u32,
        height: image.bitmap.height as u32,
        origin_x: first.left,
        baseline: first.baseline,
        advance: image.guides.iter().map(|g| g.right - g.left).sum(),
        bgra: image.bitmap.bgra,
        svg_layers: image.svg_layers,
    })
}
