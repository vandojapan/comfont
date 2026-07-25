use std::collections::HashMap;

use aviutl2::generic::ReadSection;
use windows::{Win32::Graphics::DirectWrite::IDWriteFont, core::Interface};

pub trait GlyphCoverage {
    /// `None` means that the font or its coverage could not be queried.
    fn has_glyph(&mut self, font_family: &str, character: char) -> Option<bool>;
}

/// Queries the same fonts that AviUtl2 has registered in its FontManager.
///
/// FontManager may contain private fonts that are absent from the system
/// DirectWrite collection, so coverage must be obtained through `ReadSection`.
pub struct FontManagerGlyphCoverage<'section> {
    section: &'section ReadSection,
    results: HashMap<(String, char), Option<bool>>,
}

impl<'section> FontManagerGlyphCoverage<'section> {
    pub fn new(section: &'section ReadSection) -> Self {
        Self {
            section,
            results: HashMap::new(),
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
