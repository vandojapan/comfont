//! Non-destructive anm2 rendering. Only the image buffer is replaced.
use crate::{
    control_text::{ControlTextDefaults, ParsedSpanKind, parse_control_text},
    font_coverage::{FontManagerGlyphCoverage, GlyphCoverage},
    resolved_adjustment_for_format,
};
use aviutl2::generic::ReadSection;
use compositefont_core::CompositeFontProfile;
use std::sync::Arc;
use windows::{Win32::Graphics::DirectWrite::*, core::Interface};

#[derive(Clone, PartialEq)]
pub(crate) struct RenderRequest {
    pub text: String,
    pub profile: CompositeFontProfile,
    pub font: String,
    pub size: f64,
    pub spacing: f64,
    pub line_spacing: f64,
    pub color: String,
    pub align: i32,
    pub bold: bool,
    pub italic: bool,
}

pub(crate) struct Bitmap {
    pub width: i32,
    pub height: i32,
    pub bgra: Vec<u8>,
}
pub(crate) struct RenderedText {
    pub bitmap: Arc<Bitmap>,
}
#[aviutl2::module::metatable]
impl RenderedText {
    #[direct]
    fn index(&self, params: &mut aviutl2::module::ScriptModuleCallHandle) {
        // Lua __index receives (userdata, key); the Rust receiver comes from
        // the callback context and does not consume the Lua userdata argument.
        match params.get_param_str(1).ok().as_deref() {
            Some("data") => params.push_result_data(self.bitmap.bgra.as_ptr()),
            Some("width") => params.push_result_int(self.bitmap.width),
            Some("height") => params.push_result_int(self.bitmap.height),
            _ => {}
        }
    }
}

pub(crate) fn decode_alias(text: &str) -> String {
    // obj.getvalue returns the alias representation. Decode once, preserving
    // escaped backslashes, rather than successively replacing sequences.
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            result.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => result.push('\n'),
            Some('r') => result.push('\r'),
            Some('t') => result.push('\t'),
            Some('\\') => result.push('\\'),
            Some(other) => {
                result.push('\\');
                result.push(other);
            }
            None => result.push('\\'),
        }
    }
    result
}

pub(crate) fn render(
    section: &ReadSection,
    request: &RenderRequest,
) -> Result<Option<Bitmap>, String> {
    if !(1.0..=512.0).contains(&request.size)
        || !request.spacing.is_finite()
        || !request.line_spacing.is_finite()
        || !(0..=8).contains(&request.align)
        || request.text.len() > 64 * 1024
    {
        return Ok(None);
    }
    let text = decode_alias(&request.text);
    // A second execution of embedded Lua or unsupported layout controls would
    // change the semantics of the original object. Leave its rendered image.
    if text.contains("<?") || text.contains('\t') || text.chars().any(requires_shaping) {
        return Ok(None);
    }
    let base_size = request.size / 2.0; // obj.getfont units vs text's size slider
    let parsed = match parse_control_text(
        &text,
        ControlTextDefaults {
            font_size: Some(base_size),
            font_family: Some(request.font.clone()),
        },
    ) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(None),
    };
    let base_color = match u32::from_str_radix(request.color.trim_start_matches('#'), 16) {
        Ok(color) if color <= 0xffffff => color,
        _ => return Ok(None),
    };
    let mut coverage = FontManagerGlyphCoverage::new(section);
    let factory: IDWriteFactory =
        unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.map_err(|e| e.to_string())?;
    let mut faces = std::collections::HashMap::new();
    struct Glyph {
        analysis: IDWriteGlyphRunAnalysis,
        bounds: windows::Win32::Foundation::RECT,
        color: u32,
        line: usize,
        x: f32,
    }
    let mut glyphs = Vec::new();
    let mut line_widths = vec![0.0f32];
    let mut x = 0.0f32;
    let mut line = 0usize;
    let base_metrics = coverage.vertical_metrics(&request.font);
    let ascent = base_metrics.map_or(0.9, |m| m.ascent_em) * request.size;
    let line_step = base_metrics.map_or(1.2, |m| m.ascent_em + m.descent_em + m.line_gap_em)
        * request.size
        + request.line_spacing;
    if !line_step.is_finite() || line_step <= 0.0 {
        return Ok(None);
    }
    let mut modified = false;
    for span in &parsed.spans {
        if span.kind == ParsedSpanKind::Newline {
            line_widths[line] = x;
            x = 0.0;
            line += 1;
            line_widths.push(0.0);
            continue;
        }
        if span.format.at_decoration_override().is_some_and(|d| d != 0)
            || span.format.s_outline_override().is_some()
            || span.format.at_style_override().is_some_and(|s| s.strike)
            || span.format.s_style_override().is_some_and(|s| s.strike)
        {
            return Ok(None);
        }
        for character in text[span.range.clone()].chars() {
            let adjustment = resolved_adjustment_for_format(
                &request.profile,
                character,
                &span.format,
                &mut coverage,
            );
            modified |= !adjustment.font_family.is_empty();
            let size = span.format.effective_font_size().unwrap_or(base_size);
            let metrics = adjustment
                .effective_metrics(Some(size))
                .ok_or("Invalid text metrics")?;
            let family = if adjustment.font_family.is_empty() {
                span.format.effective_font_family().unwrap_or(&request.font)
            } else {
                &adjustment.font_family
            };
            let style = span
                .format
                .s_style_override()
                .or_else(|| span.format.at_style_override());
            let bold = style.map_or(request.bold, |s| s.bold);
            let italic = style.map_or(request.italic, |s| s.italic);
            let key = (family.to_owned(), bold, italic);
            if !faces.contains_key(&key) {
                let raw = section.get_font(family).map_err(|e| e.to_string())?;
                let font = unsafe { IDWriteFont::from_raw_borrowed(&raw) }
                    .ok_or("FontManager returned no font")?;
                let face = if bold || italic {
                    let family = unsafe { font.GetFontFamily() }.map_err(|e| e.to_string())?;
                    let weight = if bold {
                        DWRITE_FONT_WEIGHT_BOLD
                    } else {
                        unsafe { font.GetWeight() }
                    };
                    let style = if italic {
                        DWRITE_FONT_STYLE_ITALIC
                    } else {
                        unsafe { font.GetStyle() }
                    };
                    let matching =
                        unsafe { family.GetFirstMatchingFont(weight, font.GetStretch(), style) }
                            .map_err(|e| e.to_string())?;
                    unsafe { matching.CreateFontFace() }
                } else {
                    unsafe { font.CreateFontFace() }
                }
                .map_err(|e| e.to_string())?;
                faces.insert(key.clone(), face);
            }
            let face = &faces[&key];
            let glyph_size = (size * 2.0 * metrics.size_ratio) as f32;
            let horizontal = adjustment.horizontal_scale_ratio as f32;
            let vertical = adjustment.vertical_scale_ratio as f32;
            if !glyph_size.is_finite()
                || !(0.01..=1024.0).contains(&glyph_size)
                || !horizontal.is_finite()
                || !(0.01..=10.0).contains(&horizontal)
                || !vertical.is_finite()
                || !(0.01..=10.0).contains(&vertical)
            {
                return Ok(None);
            }
            let mut glyph = 0u16;
            unsafe { face.GetGlyphIndices(&(character as u32), 1, &mut glyph) }
                .map_err(|e| e.to_string())?;
            if glyph == 0 && !character.is_whitespace() {
                return Ok(None);
            }
            let mut fm = DWRITE_FONT_METRICS::default();
            unsafe { face.GetMetrics(&mut fm) };
            let mut gm = DWRITE_GLYPH_METRICS::default();
            unsafe { face.GetDesignGlyphMetrics(&glyph, 1, &mut gm, false) }
                .map_err(|e| e.to_string())?;
            let advance = gm.advanceWidth as f32 / fm.designUnitsPerEm as f32 * glyph_size;
            let baseline =
                ascent + line as f64 * line_step - metrics.baseline_shift_em * size * 2.0;
            if !baseline.is_finite() || baseline.abs() > 4096.0 {
                return Ok(None);
            }
            let transform = DWRITE_MATRIX {
                m11: horizontal,
                m22: vertical,
                dx: 0.0,
                dy: baseline as f32,
                ..Default::default()
            };
            let run = DWRITE_GLYPH_RUN {
                fontFace: std::mem::ManuallyDrop::new(Some(face.clone())),
                fontEmSize: glyph_size,
                glyphCount: 1,
                glyphIndices: &glyph,
                glyphAdvances: &advance,
                ..Default::default()
            };
            let analysis = unsafe {
                factory.CreateGlyphRunAnalysis(
                    &run,
                    1.0,
                    Some(&transform),
                    DWRITE_RENDERING_MODE_NATURAL,
                    DWRITE_MEASURING_MODE_NATURAL,
                    0.0,
                    0.0,
                )
            };
            let mut owned = run.fontFace;
            unsafe { std::mem::ManuallyDrop::drop(&mut owned) };
            let analysis = analysis.map_err(|e| e.to_string())?;
            let bounds = unsafe { analysis.GetAlphaTextureBounds(DWRITE_TEXTURE_CLEARTYPE_3x1) }
                .map_err(|e| e.to_string())?;
            if [bounds.left, bounds.right, bounds.top, bounds.bottom]
                .iter()
                .any(|v| v.abs_diff(0) > 8192)
            {
                return Ok(None);
            }
            let color = if let Some(value) = &span.format.text_color.current_value {
                match u32::from_str_radix(value, 16) {
                    Ok(c) if c <= 0xffffff => c,
                    _ => return Ok(None),
                }
            } else {
                base_color
            };
            glyphs.push(Glyph {
                analysis,
                bounds,
                color,
                line,
                x,
            });
            x += advance * horizontal
                + request.spacing as f32
                + (metrics.tracking_adjust_em * size * 2.0) as f32;
            if !x.is_finite() || x.abs() > 4096.0 || line as f64 * line_step > 4096.0 {
                return Ok(None);
            }
        }
    }
    if !modified {
        return Ok(None);
    }
    line_widths[line] = x;
    let max_width = line_widths.iter().copied().fold(0.0f32, f32::max);
    let column = request.align % 3;
    let mut left = 0i32;
    let mut right = max_width.ceil() as i32;
    let mut top = 0i32;
    let Some(mut bottom) = layout_height(line + 1, line_step, request.line_spacing) else {
        return Ok(None);
    };
    let mut offsets = Vec::with_capacity(glyphs.len());
    for glyph in &glyphs {
        let offset =
            (glyph.x + (max_width - line_widths[glyph.line]) * column as f32 / 2.0).round() as i32;
        left = left.min(glyph.bounds.left + offset);
        right = right.max(glyph.bounds.right + offset);
        top = top.min(glyph.bounds.top);
        bottom = bottom.max(glyph.bounds.bottom);
        offsets.push(offset);
    }
    let width = right - left;
    let height = bottom - top;
    if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
        return Ok(None);
    }
    let mut bitmap = Bitmap {
        width,
        height,
        bgra: vec![0; width as usize * height as usize * 4],
    };
    for (glyph, offset) in glyphs.into_iter().zip(offsets) {
        let bounds = glyph.bounds;
        let gw = (bounds.right - bounds.left) as usize;
        let gh = (bounds.bottom - bounds.top) as usize;
        if gw == 0 || gh == 0 {
            continue;
        }
        if gw > 4096 || gh > 4096 {
            return Ok(None);
        }
        let mut texture = vec![0u8; gw * gh * 3];
        unsafe {
            glyph
                .analysis
                .CreateAlphaTexture(DWRITE_TEXTURE_CLEARTYPE_3x1, &bounds, &mut texture)
        }
        .map_err(|e| e.to_string())?;
        for y in 0..gh {
            for x in 0..gw {
                let alpha = ((texture[(y * gw + x) * 3] as u32
                    + texture[(y * gw + x) * 3 + 1] as u32
                    + texture[(y * gw + x) * 3 + 2] as u32)
                    / 3) as u8;
                let dx = bounds.left + offset - left + x as i32;
                let dy = bounds.top - top + y as i32;
                blend(
                    &mut bitmap.bgra[((dy * width + dx) * 4) as usize..][..4],
                    glyph.color,
                    alpha,
                );
            }
        }
    }
    Ok(Some(bitmap))
}
fn layout_height(lines: usize, line_step: f64, spacing: f64) -> Option<i32> {
    let height = lines as f64 * line_step - spacing;
    (height.is_finite() && (0.0..=4096.0).contains(&height)).then_some(height.ceil() as i32)
}
fn requires_shaping(c: char) -> bool {
    // Scalar glyph rendering cannot preserve combining marks, bidi, complex
    // shaping, or emoji sequences. Keep AviUtl's image for these inputs.
    matches!(c as u32, 0x0300..=0x036f | 0x0483..=0x0489 | 0x0590..=0x109f | 0x1100..=0x11ff | 0x1780..=0x18af | 0x1900..=0x1aaf | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0xa800..=0xabff | 0xfb1d..=0xfdff | 0xfe70..=0xfeff | 0xff9e..=0xff9f | 0x11000..=0x11fff | 0x200c..=0x200f | 0x202a..=0x202e | 0x2066..=0x2069 | 0x20d0..=0x20ff | 0x3099..=0x309a | 0xfe00..=0xfe0f | 0xfe20..=0xfe2f | 0x1f000..=0x1faff | 0xe0100..=0xe01ef)
}
fn blend(dst: &mut [u8], color: u32, alpha: u8) {
    let a = u32::from(alpha);
    let old = u32::from(dst[3]);
    let total = a * 255 + old * (255 - a);
    if total == 0 {
        return;
    }
    for (channel, dst) in dst[..3].iter_mut().enumerate() {
        let source = (color >> (channel * 8)) & 255;
        *dst = ((source * a * 255 + u32::from(*dst) * old * (255 - a)) / total) as u8;
    }
    dst[3] = (total / 255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alias_decode_preserves_literal_backslashes() {
        assert_eq!(decode_alias(r"日本語\nA\\nB\tC"), "日本語\nA\\nB\tC");
    }
    #[test]
    fn trailing_newlines_and_extreme_line_spacing_cannot_overflow_bitmap_bounds() {
        assert_eq!(layout_height(1000, 1e300, 1e300), None);
        assert_eq!(layout_height(2, f64::MAX, 0.0), None);
        assert_eq!(layout_height(2, 64.0, 0.0), Some(128));
    }
    #[test]
    fn complex_sequences_keep_host_rendering() {
        for c in [
            '\u{0301}',
            '\u{3099}',
            '\u{200d}',
            '\u{fe0f}',
            '\u{1f600}',
            '\u{0627}',
        ] {
            assert!(requires_shaping(c));
        }
        for c in "日本語ABC123".chars() {
            assert!(!requires_shaping(c));
        }
    }
    #[test]
    fn alpha_blending_returns_straight_bgra() {
        let mut pixel = [0; 4];
        blend(&mut pixel, 0xff0000, 128);
        assert_eq!(pixel, [0, 0, 255, 128]);
        blend(&mut pixel, 0x0000ff, 255);
        assert_eq!(pixel, [255, 0, 0, 255]);
    }
}
