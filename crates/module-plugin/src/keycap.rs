//! Compatibility for orphan keycap SVGs in Twitter Color Emoji SVGinOT.
//! Glyph names are a convention of this font, never a general Unicode mapping.
use std::ffi::c_void;
use windows::{
    Win32::Foundation::{E_INVALIDARG, E_NOTIMPL},
    Win32::Graphics::DirectWrite::*,
    core::{BOOL, ComObject, HSTRING, IUnknown, Interface, PCWSTR, Ref, Result, implement},
};

pub(crate) const BASES: &[u8; 12] = b"0123456789#*";

pub(crate) fn is_keycap(cluster: &str) -> bool {
    compositefont_core::is_emoji_keycap(cluster) || cluster == "🔟"
}

pub(crate) fn index(cluster: &str) -> Option<usize> {
    compositefont_core::is_emoji_keycap(cluster)
        .then(|| BASES.iter().position(|b| *b == cluster.as_bytes()[0]))
        .flatten()
}

/// Resolve names from this face's post table, checking each SVG payload.
/// Zero entries are unavailable and must fall through independently.
/// No hard-coded glyph IDs, system font lookup, font modification or PUA text.
pub(crate) fn svg_glyphs(font: &IDWriteFont) -> Option<[u16; 12]> {
    unsafe {
        let names = font.GetFontFamily().ok()?.GetFamilyNames().ok()?;
        let mut locale = 0;
        let mut exists = BOOL::default();
        names
            .FindLocaleName(&HSTRING::from("en-us"), &mut locale, &mut exists)
            .ok()?;
        if !exists.as_bool() {
            locale = 0;
        }
        let mut name = vec![0u16; names.GetStringLength(locale).ok()? as usize + 1];
        names.GetString(locale, &mut name).ok()?;
        name.pop();
        if !String::from_utf16(&name)
            .ok()?
            .eq_ignore_ascii_case("Twitter Color Emoji")
        {
            return None;
        }
        let face = font.CreateFontFace().ok()?;
        let modern: IDWriteFontFace4 = face.cast().ok()?;
        if !modern
            .GetGlyphImageFormats2()
            .contains(DWRITE_GLYPH_IMAGE_FORMATS_SVG)
        {
            return None;
        }
        // Builds with a working Unicode mapping use normal DirectWrite shaping.
        if BASES
            .iter()
            .all(|c| font.HasCharacter(u32::from(*c)).is_ok_and(|v| v.as_bool()))
            && font.HasCharacter(0x20e3).is_ok_and(|v| v.as_bool())
        {
            return None;
        }
        let mut data = std::ptr::null_mut::<c_void>();
        let mut size = 0;
        let mut context = std::ptr::null_mut();
        face.TryGetFontTable(
            u32::from_le_bytes(*b"post"),
            &mut data,
            &mut size,
            &mut context,
            &mut exists,
        )
        .ok()?;
        if !exists.as_bool() {
            return None;
        }
        // Always release DirectWrite's table context, including malformed tables.
        let result = if !data.is_null() && size <= 4 * 1024 * 1024 {
            named_glyphs(
                std::slice::from_raw_parts(data.cast::<u8>(), size as usize),
                face.GetGlyphCount(),
            )
        } else {
            None
        };
        face.ReleaseFontTable(context);
        let mut glyphs = result?;
        for glyph in &mut glyphs {
            if *glyph != 0
                && !modern
                    .GetGlyphImageFormats(*glyph, 1, 4096)
                    .is_ok_and(|formats| formats.contains(DWRITE_GLYPH_IMAGE_FORMATS_SVG))
            {
                *glyph = 0;
            }
        }
        glyphs.iter().any(|g| *g != 0).then_some(glyphs)
    }
}

fn word(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn named_glyphs(post: &[u8], glyph_count: u16) -> Option<[u16; 12]> {
    if post.get(..4)? != [0, 2, 0, 0] || word(post, 32)? != glyph_count {
        return None;
    }
    let count = usize::from(glyph_count);
    let indices = post.get(34..34 + 2 * count)?;
    let max_index = indices
        .chunks_exact(2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .max()?;
    let mut custom = Vec::new();
    let mut cursor = 34 + 2 * count;
    for _ in 258..=max_index {
        let length = usize::from(*post.get(cursor)?);
        cursor += 1;
        custom.push(post.get(cursor..cursor + length)?);
        cursor += length;
    }
    let mut result = [0; 12];
    let targets = BASES.map(|base| format!("{base:x}-20e3"));
    for (gid, bytes) in indices.chunks_exact(2).enumerate().skip(1) {
        let name_index = u16::from_be_bytes([bytes[0], bytes[1]]);
        let Some(name) = name_index
            .checked_sub(258)
            .and_then(|i| custom.get(usize::from(i)))
        else {
            continue;
        };
        for (i, target) in targets.iter().enumerate() {
            if *name == target.as_bytes() {
                if result[i] != 0 {
                    return None;
                }
                result[i] = gid as u16;
            }
        }
    }
    result.iter().any(|g| *g != 0).then_some(result)
}

/// A whole keycap occupies one DirectWrite inline object. Only its glyph
/// selection is overridden; the shared painter still handles SVG, guides,
/// baseline, color and scaling for both the preview and the output bitmap.
pub(crate) fn inline(
    font: &IDWriteFont,
    glyph: u16,
    cluster: &str,
    style: &crate::shaping::TextStyle,
) -> Result<IDWriteInlineObject> {
    let face = unsafe { font.CreateFontFace()? };
    let mut metrics = DWRITE_FONT_METRICS::default();
    let mut glyph_metrics = DWRITE_GLYPH_METRICS::default();
    unsafe {
        face.GetMetrics(&mut metrics);
        face.GetDesignGlyphMetrics(&glyph, 1, &mut glyph_metrics, false)?;
    }
    if metrics.designUnitsPerEm == 0 || glyph_metrics.advanceWidth == 0 || !style.valid() {
        return Err(E_INVALIDARG.into());
    }
    let em = f32::from(metrics.designUnitsPerEm);
    let size = style.size * style.horizontal;
    Ok(ComObject::new(KeycapInline {
        face,
        glyph,
        size,
        advance: glyph_metrics.advanceWidth as f32 / em * size,
        height: (f32::from(metrics.ascent) + f32::from(metrics.descent)) / em
            * style.size
            * style.vertical,
        baseline: f32::from(metrics.ascent) / em * style.size * style.vertical,
        text: cluster.encode_utf16().collect(),
    })
    .into_interface())
}

#[implement(IDWriteInlineObject)]
struct KeycapInline {
    face: IDWriteFontFace,
    glyph: u16,
    size: f32,
    advance: f32,
    height: f32,
    baseline: f32,
    text: Vec<u16>,
}

impl IDWriteInlineObject_Impl for KeycapInline_Impl {
    fn Draw(
        &self,
        context: *const c_void,
        renderer: Ref<IDWriteTextRenderer>,
        x: f32,
        y: f32,
        sideways: BOOL,
        _rtl: BOOL,
        effect: Ref<IUnknown>,
    ) -> Result<()> {
        if sideways.as_bool() {
            return Err(E_NOTIMPL.into());
        }
        let renderer = renderer.ok()?;
        let effect = effect.ok()?;
        let run = DWRITE_GLYPH_RUN {
            fontFace: std::mem::ManuallyDrop::new(Some(self.face.clone())),
            fontEmSize: self.size,
            glyphCount: 1,
            glyphIndices: &self.glyph,
            glyphAdvances: &self.advance,
            ..Default::default()
        };
        let clusters = vec![0u16; self.text.len()];
        let desc = DWRITE_GLYPH_RUN_DESCRIPTION {
            string: PCWSTR(self.text.as_ptr()),
            stringLength: self.text.len() as u32,
            clusterMap: clusters.as_ptr(),
            ..Default::default()
        };
        // DirectWrite supplies the inline object's top-left, not its baseline.
        let result = unsafe {
            renderer.DrawGlyphRun(
                Some(context),
                x,
                y + self.baseline,
                DWRITE_MEASURING_MODE_NATURAL,
                &run,
                &desc,
                effect,
            )
        };
        // DWRITE_GLYPH_RUN has a ManuallyDrop face field; release our clone.
        let mut run = run;
        unsafe {
            std::mem::ManuallyDrop::drop(&mut run.fontFace);
        }
        result
    }
    fn GetMetrics(&self) -> Result<DWRITE_INLINE_OBJECT_METRICS> {
        Ok(DWRITE_INLINE_OBJECT_METRICS {
            width: self.advance,
            height: self.height,
            baseline: self.baseline,
            supportsSideways: false.into(),
        })
    }
    fn GetOverhangMetrics(&self) -> Result<DWRITE_OVERHANG_METRICS> {
        Ok(DWRITE_OVERHANG_METRICS::default())
    }
    fn GetBreakConditions(
        &self,
        before: *mut DWRITE_BREAK_CONDITION,
        after: *mut DWRITE_BREAK_CONDITION,
    ) -> Result<()> {
        if before.is_null() || after.is_null() {
            return Err(E_INVALIDARG.into());
        }
        unsafe {
            *before = DWRITE_BREAK_CONDITION_NEUTRAL;
            *after = DWRITE_BREAK_CONDITION_NEUTRAL;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Vec<u8> {
        let mut post = vec![0; 32];
        post[..4].copy_from_slice(&[0, 2, 0, 0]);
        post.extend_from_slice(&13u16.to_be_bytes());
        post.extend_from_slice(&0u16.to_be_bytes());
        for i in 0..12 {
            post.extend_from_slice(&(258u16 + i).to_be_bytes());
        }
        // Deliberately reorder the names: glyph IDs must come from the file.
        for b in BASES.iter().rev() {
            let name = format!("{b:x}-20e3");
            post.push(name.len() as u8);
            post.extend_from_slice(name.as_bytes());
        }
        post
    }

    #[test]
    fn post_names_resolve_ids_and_reject_truncation() {
        let post = table();
        assert_eq!(
            named_glyphs(&post, 13),
            Some([12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1])
        );
        for end in 0..post.len() {
            assert_eq!(named_glyphs(&post[..end], 13), None);
        }
        assert_eq!(named_glyphs(&post, 12), None);
        let mut wrong_version = post.clone();
        wrong_version[1] = 3;
        assert_eq!(named_glyphs(&wrong_version, 13), None);
        let mut duplicate = post;
        duplicate[36..38].copy_from_slice(&259u16.to_be_bytes());
        assert_eq!(named_glyphs(&duplicate, 13), None);
        let mut partial = table();
        *partial.last_mut().unwrap() = b'x';
        let ids = named_glyphs(&partial, 13).unwrap();
        assert_eq!(ids[0], 0);
        assert_eq!(ids[1], 11);
    }
}
