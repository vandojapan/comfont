//! Preview uses the rendering module's rule/fallback resolver and the host's
//! FontManager (including private fonts), rather than a system-font substitute.
use crate::{
    font_coverage::{FontManagerGlyphCoverage, GlyphCoverage},
    resolved_adjustment,
};
use aviutl2::generic::ReadSection;
use compositefont_core::CompositeFontProfile;
use windows::{Win32::Graphics::DirectWrite::*, core::Interface};

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
        return Err("プレビューの寸法が不正です。".to_owned());
    }
    let factory: IDWriteFactory =
        unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.map_err(|e| e.to_string())?;
    let mut bitmap = PreviewBitmap {
        width,
        height,
        bgra: [0x1b, 0x19, 0x18, 255].repeat((width * height) as usize),
    };
    let mut coverage = FontManagerGlyphCoverage::new(section);
    let mut x = 16.0f32;
    let mut baseline = (size * 1.7) as f32;
    let mut faces = std::collections::HashMap::new();
    for character in text.chars() {
        if character == '\r' {
            continue;
        }
        if character == '\n' {
            x = 16.0;
            baseline += (size * 2.0) as f32;
            continue;
        }
        let adjustment = resolved_adjustment(profile, character, &mut coverage);
        let metrics = adjustment.effective_metrics(Some(size)).unwrap_or_default();
        if !crate::is_safe_adjustment(&adjustment) {
            return Err("プレビューの補正値が不正です。".to_owned());
        }
        let family = if adjustment.font_family.is_empty() {
            "Yu Gothic UI"
        } else {
            &adjustment.font_family
        };
        if !faces.contains_key(family) {
            let raw = section.get_font(family).map_err(|e| e.to_string())?;
            let font = unsafe { IDWriteFont::from_raw_borrowed(&raw) }
                .ok_or("フォントを取得できません。")?;
            let face = unsafe { font.CreateFontFace() }.map_err(|e| e.to_string())?;
            faces.insert(family.to_owned(), face);
        }
        let face = &faces[family];
        let glyph_size = (size * metrics.size_ratio).clamp(1.0, 512.0) as f32;
        let horizontal = adjustment.horizontal_scale_ratio.clamp(0.01, 10.0) as f32;
        let vertical = adjustment.vertical_scale_ratio.clamp(0.01, 10.0) as f32;
        let mut glyph = 0u16;
        let point = character as u32;
        unsafe { face.GetGlyphIndices(&point, 1, &mut glyph) }.map_err(|e| e.to_string())?;
        let advance =
            coverage.glyph_advance_em(family, character).unwrap_or(1.0) as f32 * glyph_size;
        let tracking = (metrics.tracking_adjust_em * size) as f32;
        if x + advance * horizontal > width as f32 - 16.0 {
            x = 16.0;
            baseline += (size * 2.0) as f32;
        }
        if baseline > height as f32 + size as f32 {
            break;
        }
        let glyph_baseline = baseline - (metrics.baseline_shift_em * size) as f32;
        let transform = DWRITE_MATRIX {
            m11: horizontal,
            m22: vertical,
            dx: x,
            dy: glyph_baseline,
            ..Default::default()
        };
        // DWRITE_GLYPH_RUN has a raw interface field. Explicitly release our
        // clone after creating the analysis, including the error path.
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
        let mut face_ref = run.fontFace;
        unsafe {
            std::mem::ManuallyDrop::drop(&mut face_ref);
        }
        let analysis = analysis.map_err(|e| e.to_string())?;
        let mut bounds = unsafe { analysis.GetAlphaTextureBounds(DWRITE_TEXTURE_CLEARTYPE_3x1) }
            .map_err(|e| e.to_string())?;
        bounds.left = bounds.left.clamp(0, width as i32);
        bounds.right = bounds.right.clamp(0, width as i32);
        bounds.top = bounds.top.clamp(0, height as i32);
        bounds.bottom = bounds.bottom.clamp(0, height as i32);
        if bounds.right > bounds.left && bounds.bottom > bounds.top {
            let bw = (bounds.right - bounds.left) as usize;
            let bh = (bounds.bottom - bounds.top) as usize;
            let mut alpha = vec![0u8; bw * bh * 3];
            unsafe {
                analysis.CreateAlphaTexture(DWRITE_TEXTURE_CLEARTYPE_3x1, &bounds, &mut alpha)
            }
            .map_err(|e| e.to_string())?;
            for gy in 0..bh {
                for gx in 0..bw {
                    let src = (gy * bw + gx) * 3;
                    let dst =
                        ((gy + bounds.top as usize) * width as usize + gx + bounds.left as usize)
                            * 4;
                    for channel in 0..3 {
                        let a = alpha[src + 2 - channel] as u32;
                        bitmap.bgra[dst + channel] =
                            ((bitmap.bgra[dst + channel] as u32 * (255 - a) + 244 * a) / 255) as u8;
                    }
                }
            }
            if bounds_guide {
                for px in bounds.left..bounds.right {
                    put(&mut bitmap, px, bounds.top, [192, 192, 0, 255]);
                    put(&mut bitmap, px, bounds.bottom - 1, [192, 192, 0, 255]);
                }
                for py in bounds.top..bounds.bottom {
                    put(&mut bitmap, bounds.left, py, [192, 192, 0, 255]);
                    put(&mut bitmap, bounds.right - 1, py, [192, 192, 0, 255]);
                }
            }
        }
        if baseline_guide {
            for px in x as i32..(x + advance * horizontal).ceil() as i32 {
                put(&mut bitmap, px, glyph_baseline as i32, [0, 64, 224, 255]);
            }
        }
        x += (advance + tracking) * horizontal;
    }
    Ok(bitmap)
}
fn put(bitmap: &mut PreviewBitmap, x: i32, y: i32, color: [u8; 4]) {
    if x >= 0 && y >= 0 && x < bitmap.width as i32 && y < bitmap.height as i32 {
        let index = (y as usize * bitmap.width as usize + x as usize) * 4;
        bitmap.bgra[index..index + 4].copy_from_slice(&color);
    }
}
