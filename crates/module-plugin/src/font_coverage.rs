use std::collections::HashMap;

use aviutl2::generic::ReadSection;
use windows::{
    Win32::Graphics::DirectWrite::{
        DWRITE_FONT_METRICS, DWRITE_GLYPH_METRICS, IDWriteFont, IDWriteFont1,
    },
    core::Interface,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontVerticalMetrics {
    pub ascent_em: f64,
    pub descent_em: f64,
    pub line_gap_em: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct GlyphLayoutMetrics {
    top_em: f64,
    advance_em: f64,
}

pub trait GlyphCoverage {
    /// `None` means that the font or its coverage could not be queried.
    fn has_glyph(&mut self, font_family: &str, character: char) -> Option<bool>;

    /// Returns normalized DirectWrite vertical metrics for the resolved font.
    fn vertical_metrics(&mut self, _font_family: &str) -> Option<FontVerticalMetrics> {
        None
    }

    /// Reports whether DirectWrite classifies the resolved font as monospaced.
    fn is_monospaced_font(&mut self, _font_family: &str) -> Option<bool> {
        None
    }

    /// Returns the glyph ink's top extent above the horizontal baseline in em.
    fn glyph_top_em(&mut self, _font_family: &str, _character: char) -> Option<f64> {
        None
    }

    /// Returns the glyph's horizontal advance in em.
    fn glyph_advance_em(&mut self, _font_family: &str, _character: char) -> Option<f64> {
        None
    }
}

/// Queries the same fonts that AviUtl2 has registered in its FontManager.
///
/// FontManager may contain private fonts that are absent from the system
/// DirectWrite collection, so coverage must be obtained through `ReadSection`.
pub struct FontManagerGlyphCoverage<'section> {
    section: &'section ReadSection,
    results: HashMap<(String, char), Option<bool>>,
    vertical_metrics: HashMap<String, Option<FontVerticalMetrics>>,
    monospaced_fonts: HashMap<String, Option<bool>>,
    glyph_layout_metrics: HashMap<(String, char), Option<GlyphLayoutMetrics>>,
}

impl<'section> FontManagerGlyphCoverage<'section> {
    pub fn new(section: &'section ReadSection) -> Self {
        Self {
            section,
            results: HashMap::new(),
            vertical_metrics: HashMap::new(),
            monospaced_fonts: HashMap::new(),
            glyph_layout_metrics: HashMap::new(),
        }
    }

    fn query(&self, font_family: &str, character: char) -> Option<bool> {
        let raw = self.section.get_font(font_family).ok()?;
        // SAFETY: `ReadSection::get_font` returns an AviUtl2-owned IDWriteFont.
        // The pointer stays valid for this module call and must not be released.
        let font = unsafe { IDWriteFont::from_raw_borrowed(&raw) }?;
        unsafe {
            font.HasCharacter(character as u32)
                .ok()
                .map(|value| value.as_bool())
        }
    }

    fn query_vertical_metrics(&self, font_family: &str) -> Option<FontVerticalMetrics> {
        let raw = self.section.get_font(font_family).ok()?;
        // SAFETY: `ReadSection::get_font` returns an AviUtl2-owned IDWriteFont.
        // The pointer stays valid for this module call and must not be released.
        let font = unsafe { IDWriteFont::from_raw_borrowed(&raw) }?;
        let mut metrics = DWRITE_FONT_METRICS::default();
        unsafe { font.GetMetrics(&mut metrics) };
        let em = f64::from(metrics.designUnitsPerEm);
        (em > 0.0).then_some(FontVerticalMetrics {
            ascent_em: f64::from(metrics.ascent) / em,
            descent_em: f64::from(metrics.descent) / em,
            line_gap_em: f64::from(metrics.lineGap) / em,
        })
    }

    fn query_is_monospaced_font(&self, font_family: &str) -> Option<bool> {
        let raw = self.section.get_font(font_family).ok()?;
        // SAFETY: `ReadSection::get_font` returns an AviUtl2-owned IDWriteFont.
        // QueryInterface creates the temporary owned interface used here.
        let font = unsafe { IDWriteFont::from_raw_borrowed(&raw) }?;
        let font = font.cast::<IDWriteFont1>().ok()?;
        Some(unsafe { font.IsMonospacedFont() }.as_bool())
    }

    fn query_glyph_layout_metrics(
        &self,
        font_family: &str,
        character: char,
    ) -> Option<GlyphLayoutMetrics> {
        let raw = self.section.get_font(font_family).ok()?;
        // SAFETY: `ReadSection::get_font` returns an AviUtl2-owned IDWriteFont.
        // The pointer stays valid for this module call and must not be released.
        let font = unsafe { IDWriteFont::from_raw_borrowed(&raw) }?;
        let mut font_metrics = DWRITE_FONT_METRICS::default();
        unsafe { font.GetMetrics(&mut font_metrics) };
        let em = f64::from(font_metrics.designUnitsPerEm);
        if em <= 0.0 {
            return None;
        }

        let face = unsafe { font.CreateFontFace().ok()? };
        let codepoint = character as u32;
        let mut glyph_index = 0_u16;
        unsafe {
            face.GetGlyphIndices(&codepoint, 1, &mut glyph_index).ok()?;
        }
        if glyph_index == 0 {
            return None;
        }
        let mut glyph_metrics = DWRITE_GLYPH_METRICS::default();
        unsafe {
            face.GetDesignGlyphMetrics(&glyph_index, 1, &mut glyph_metrics, false)
                .ok()?;
        }
        let top = glyph_metrics.verticalOriginY - glyph_metrics.topSideBearing;
        Some(GlyphLayoutMetrics {
            top_em: f64::from(top.max(0)) / em,
            advance_em: f64::from(glyph_metrics.advanceWidth) / em,
        })
    }

    fn cached_glyph_layout_metrics(
        &mut self,
        font_family: &str,
        character: char,
    ) -> Option<GlyphLayoutMetrics> {
        let key = (font_family.to_owned(), character);
        if let Some(metrics) = self.glyph_layout_metrics.get(&key) {
            return *metrics;
        }
        let metrics = self.query_glyph_layout_metrics(font_family, character);
        self.glyph_layout_metrics.insert(key, metrics);
        metrics
    }
}

impl GlyphCoverage for FontManagerGlyphCoverage<'_> {
    fn has_glyph(&mut self, font_family: &str, character: char) -> Option<bool> {
        let key = (font_family.to_owned(), character);
        if let Some(result) = self.results.get(&key) {
            return *result;
        }
        let result = self.query(font_family, character);
        self.results.insert(key, result);
        result
    }

    fn vertical_metrics(&mut self, font_family: &str) -> Option<FontVerticalMetrics> {
        if let Some(metrics) = self.vertical_metrics.get(font_family) {
            return *metrics;
        }
        let metrics = self.query_vertical_metrics(font_family);
        self.vertical_metrics
            .insert(font_family.to_owned(), metrics);
        metrics
    }

    fn is_monospaced_font(&mut self, font_family: &str) -> Option<bool> {
        if let Some(result) = self.monospaced_fonts.get(font_family) {
            return *result;
        }
        let result = self.query_is_monospaced_font(font_family);
        self.monospaced_fonts.insert(font_family.to_owned(), result);
        result
    }

    fn glyph_top_em(&mut self, font_family: &str, character: char) -> Option<f64> {
        self.cached_glyph_layout_metrics(font_family, character)
            .map(|metrics| metrics.top_em)
    }

    fn glyph_advance_em(&mut self, font_family: &str, character: char) -> Option<f64> {
        self.cached_glyph_layout_metrics(font_family, character)
            .map(|metrics| metrics.advance_em)
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct UnknownGlyphCoverage;

#[cfg(test)]
impl GlyphCoverage for UnknownGlyphCoverage {
    fn has_glyph(&mut self, _font_family: &str, _character: char) -> Option<bool> {
        None
    }
}
