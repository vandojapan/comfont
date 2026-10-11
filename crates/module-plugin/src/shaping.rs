//! Shared DirectWrite text shaping and rasterization for the object and preview.
//! Layout, including script analysis, clusters, kerning and bidi, stays in
//! DirectWrite. We only paint the glyph runs returned by its text renderer.
use crate::render::Bitmap;
use aviutl2::generic::ReadSection;
use icu_properties::{CodePointMapData, props::BidiClass};
use std::{collections::HashMap, ffi::c_void, ops::Range, sync::Mutex};
use windows::{
    Win32::{
        Foundation::{E_INVALIDARG, E_NOTIMPL, RECT},
        Graphics::DirectWrite::*,
    },
    core::{BOOL, ComObject, HSTRING, IUnknown, Interface, Ref, Result, implement},
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextStyle {
    pub family: String,
    pub size: f32,
    pub horizontal: f32,
    pub vertical: f32,
    pub tracking: f32,
    pub baseline_shift: f32,
    pub color: u32,
    pub bold: bool,
    pub italic: bool,
    pub disable_ligatures: bool,
}
impl TextStyle {
    pub fn valid(&self) -> bool {
        !self.family.is_empty()
            && self.size.is_finite()
            && (0.01..=1024.0).contains(&self.size)
            && self.horizontal.is_finite()
            && (0.01..=10.0).contains(&self.horizontal)
            && self.vertical.is_finite()
            && (0.01..=10.0).contains(&self.vertical)
            && self.tracking.is_finite()
            && self.tracking.abs() <= 4096.0
            && self.baseline_shift.is_finite()
            && self.baseline_shift.abs() <= 4096.0
            && self.color <= 0xffffff
    }
}
#[derive(Clone, Debug)]
pub(crate) struct StyledRange {
    pub utf16: Range<usize>,
    pub style: TextStyle,
}
#[derive(Default)]
pub(crate) struct StyledText {
    pub utf16: Vec<u16>,
    pub ranges: Vec<StyledRange>,
    pub modified: bool,
}
impl StyledText {
    pub fn push(&mut self, text: &str, style: TextStyle) {
        let start = self.utf16.len();
        // Canonically equivalent combining sequences (including Hangul jamo)
        // must use the same glyphs, even in fonts containing only NFC forms.
        // This changes the transient layout string, never the saved source.
        let normalized = icu_normalizer::ComposingNormalizer::new_nfc().normalize(text);
        self.utf16.extend(normalized.encode_utf16());
        if start == self.utf16.len() {
            return;
        }
        if let Some(last) = self.ranges.last_mut()
            && last.utf16.end == start
            && last.style == style
        {
            last.utf16.end = self.utf16.len();
        } else {
            self.ranges.push(StyledRange {
                utf16: start..self.utf16.len(),
                style,
            });
        }
    }
}
// Required script ligatures can ignore OpenType feature overrides. Force mode
// inserts ZWNJ only into the transient layout text, including inside clusters.
// Font/style ownership follows the original scalar and saved text is untouched.
fn non_joining_text(text: &StyledText, force_all: bool) -> std::result::Result<StyledText, String> {
    let source = String::from_utf16(&text.utf16).map_err(|e| e.to_string())?;
    let mut characters = source.chars().peekable();
    let mut result = StyledText {
        modified: text.modified,
        ..Default::default()
    };
    let mut source_offset = 0;
    let mut range_index = 0;
    while let Some(character) = characters.next() {
        while text
            .ranges
            .get(range_index)
            .is_some_and(|r| r.utf16.end <= source_offset)
        {
            range_index += 1;
        }
        let range = text.ranges.get(range_index).ok_or("Missing text style")?;
        let mut segment = character.to_string();
        if (force_all || range.style.disable_ligatures)
            && !character.is_control()
            && !character.is_whitespace()
            && characters
                .peek()
                .is_some_and(|next| !next.is_control() && !next.is_whitespace())
        {
            segment.push('\u{200c}');
        }
        result.push(&segment, range.style.clone());
        source_offset += character.len_utf16();
    }
    Ok(result)
}

pub(crate) trait Fonts {
    fn get(&mut self, family: &str) -> std::result::Result<IDWriteFont, String>;
}
pub(crate) struct HostFonts<'a> {
    section: &'a ReadSection,
    cache: HashMap<String, IDWriteFont>,
}
impl<'a> HostFonts<'a> {
    pub fn new(section: &'a ReadSection) -> Self {
        Self {
            section,
            cache: HashMap::new(),
        }
    }
}
impl Fonts for HostFonts<'_> {
    fn get(&mut self, family: &str) -> std::result::Result<IDWriteFont, String> {
        if let Some(font) = self.cache.get(family) {
            return Ok(font.clone());
        }
        let raw = self.section.get_font(family).map_err(|e| e.to_string())?;
        // The host owns this pointer. AddRef before putting it in our cache.
        let font = unsafe { IDWriteFont::from_raw_borrowed(&raw) }
            .ok_or("FontManager returned no font")?
            .clone();
        self.cache.insert(family.to_owned(), font.clone());
        Ok(font)
    }
}
pub(crate) struct LayoutOptions {
    pub base_family: String,
    pub base_size: f32,
    pub line_spacing: f32,
    pub column: i32,
    pub wrap_width: Option<f32>,
    pub glyph_guides: bool,
    pub disable_ligatures: bool,
}
pub(crate) struct Guide {
    pub bounds: RECT,
    pub layout_bounds: RECT,
    pub baseline: f32,
    pub left: f32,
    pub right: f32,
}
pub(crate) struct ShapedBitmap {
    pub bitmap: Bitmap,
    pub guides: Vec<Guide>,
    #[cfg(test)]
    pub runs: Vec<RunInfo>,
}
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct RunInfo {
    pub text: Vec<u16>,
    pub clusters: Vec<u16>,
    pub glyphs: Vec<u16>,
    pub offsets: Vec<DWRITE_GLYPH_OFFSET>,
    pub bidi: u32,
    pub x: f32,
    pub advance: f32,
}
#[derive(Clone, Copy)]
struct Paint {
    vertical: f32,
    baseline_shift: f32,
    color: u32,
}
#[implement(IDWritePixelSnapping)]
struct PaintEffect {
    paint: Paint,
}
impl IDWritePixelSnapping_Impl for PaintEffect_Impl {
    fn IsPixelSnappingDisabled(&self, _: *const c_void) -> Result<BOOL> {
        Ok(true.into())
    }
    fn GetCurrentTransform(&self, _: *const c_void, transform: *mut DWRITE_MATRIX) -> Result<()> {
        identity(transform)
    }
    fn GetPixelsPerDip(&self, _: *const c_void) -> Result<f32> {
        Ok(1.0)
    }
}
fn identity(transform: *mut DWRITE_MATRIX) -> Result<()> {
    if transform.is_null() {
        return Err(E_INVALIDARG.into());
    }
    // SAFETY: DirectWrite passes a writable output matrix to this callback.
    unsafe {
        *transform = DWRITE_MATRIX {
            m11: 1.0,
            m22: 1.0,
            ..Default::default()
        };
    }
    Ok(())
}
// Logical character cells use the selected face's ascent/descent and the
// native shaped advances; outline/mark overhangs belong only to ink bounds.
fn layout_bounds(
    run: &DWRITE_GLYPH_RUN,
    vertical: f32,
    baseline: f32,
    left: f32,
    right: f32,
) -> Result<RECT> {
    let face = run.fontFace.as_ref().ok_or(E_INVALIDARG)?;
    let mut metrics = DWRITE_FONT_METRICS::default();
    unsafe { face.GetMetrics(&mut metrics) };
    if metrics.designUnitsPerEm == 0 {
        return Err(E_INVALIDARG.into());
    }
    let scale = run.fontEmSize * vertical / f32::from(metrics.designUnitsPerEm);
    Ok(RECT {
        left: left.floor() as i32,
        right: right.ceil() as i32,
        top: (baseline - f32::from(metrics.ascent) * scale).floor() as i32,
        bottom: (baseline + f32::from(metrics.descent) * scale).ceil() as i32,
    })
}

fn glyph_guides(
    factory: &IDWriteFactory,
    run: &DWRITE_GLYPH_RUN,
    mut transform: DWRITE_MATRIX,
    mode: DWRITE_MEASURING_MODE,
    baseline: f32,
    cell: RECT,
) -> Result<Vec<Guide>> {
    let mut guides = Vec::with_capacity(run.glyphCount as usize);
    let rtl = run.bidiLevel & 1 != 0;
    for index in 0..run.glyphCount as usize {
        // This temporary view borrows the callback's ManuallyDrop fontFace and
        // arrays, so dropping it neither releases nor takes ownership of them.
        let mut glyph = unsafe { std::ptr::read(run) };
        glyph.glyphCount = 1;
        unsafe {
            glyph.glyphIndices = run.glyphIndices.add(index);
            glyph.glyphAdvances = run.glyphAdvances.add(index);
            if !run.glyphOffsets.is_null() {
                glyph.glyphOffsets = run.glyphOffsets.add(index);
            }
        }
        let advance = unsafe { *glyph.glyphAdvances };
        let next = transform.dx + if rtl { -advance } else { advance };
        let left = transform.dx.min(next);
        let right = transform.dx.max(next);
        let analysis = unsafe {
            factory.CreateGlyphRunAnalysis(
                &glyph,
                1.0,
                Some(&transform),
                DWRITE_RENDERING_MODE_NATURAL,
                mode,
                0.0,
                0.0,
            )
        }?;
        let bounds = unsafe { analysis.GetAlphaTextureBounds(DWRITE_TEXTURE_CLEARTYPE_3x1) }?;
        guides.push(Guide {
            bounds,
            layout_bounds: RECT {
                left: left.floor() as i32,
                right: right.ceil() as i32,
                ..cell
            },
            baseline,
            left,
            right,
        });
        transform.dx = next;
    }
    Ok(guides)
}

struct InkRun {
    analysis: IDWriteGlyphRunAnalysis,
    guide: Guide,
    glyph_guides: Vec<Guide>,
    color: u32,
    #[cfg(test)]
    info: RunInfo,
}
#[implement(IDWriteTextRenderer)]
struct Renderer {
    factory: IDWriteFactory,
    ink: Mutex<Vec<InkRun>>,
    glyph_guides: bool,
}
impl IDWritePixelSnapping_Impl for Renderer_Impl {
    fn IsPixelSnappingDisabled(&self, _: *const c_void) -> Result<BOOL> {
        Ok(true.into())
    }
    fn GetCurrentTransform(&self, _: *const c_void, transform: *mut DWRITE_MATRIX) -> Result<()> {
        identity(transform)
    }
    fn GetPixelsPerDip(&self, _: *const c_void) -> Result<f32> {
        Ok(1.0)
    }
}
impl IDWriteTextRenderer_Impl for Renderer_Impl {
    fn DrawGlyphRun(
        &self,
        _: *const c_void,
        x: f32,
        y: f32,
        mode: DWRITE_MEASURING_MODE,
        run: *const DWRITE_GLYPH_RUN,
        description: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        effect: Ref<IUnknown>,
    ) -> Result<()> {
        if run.is_null() || description.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let paint = effect.ok()?.cast_object_ref::<PaintEffect>()?.paint;
        // Borrow the callback run: its font face and arrays belong to DirectWrite.
        let run = unsafe { &*run };
        if run.glyphCount == 0 {
            return Ok(());
        }
        // This BGRA path paints outline glyphs. Keep the host image for color
        // bitmap/SVG/COLR fonts rather than silently discard their palettes.
        if let Some(face) = run.fontFace.as_ref()
            && let Ok(color_face) = face.cast::<IDWriteFontFace2>()
            && unsafe { color_face.IsColorFont() }.as_bool()
        {
            return Err(E_NOTIMPL.into());
        }
        if run.glyphCount > 65536
            || run.glyphIndices.is_null()
            || run.glyphAdvances.is_null()
            || !x.is_finite()
            || !y.is_finite()
        {
            return Err(E_INVALIDARG.into());
        }
        let advances =
            unsafe { std::slice::from_raw_parts(run.glyphAdvances, run.glyphCount as usize) };
        let indices =
            unsafe { std::slice::from_raw_parts(run.glyphIndices, run.glyphCount as usize) };
        // A missing visible glyph must not replace a usable host image with
        // tofu. Zero-width controls may legitimately have glyph index zero.
        if indices
            .iter()
            .zip(advances)
            .any(|(g, a)| *g == 0 && *a > 0.0)
        {
            return Err(E_INVALIDARG.into());
        }
        let advance: f32 = advances.iter().sum();
        if !advance.is_finite() || advance.abs() > 8192.0 {
            return Err(E_INVALIDARG.into());
        }
        let baseline = y + paint.baseline_shift;
        // Font size in the layout includes horizontal scaling, so the native
        // advances remain valid for shaping and bidi. Restore vertical size here.
        let transform = DWRITE_MATRIX {
            m11: 1.0,
            m22: paint.vertical,
            dx: x,
            dy: baseline,
            ..Default::default()
        };
        let analysis = unsafe {
            self.factory.CreateGlyphRunAnalysis(
                run,
                1.0,
                Some(&transform),
                DWRITE_RENDERING_MODE_NATURAL,
                mode,
                0.0,
                0.0,
            )
        }?;
        let bounds = unsafe { analysis.GetAlphaTextureBounds(DWRITE_TEXTURE_CLEARTYPE_3x1) }?;
        if [bounds.left, bounds.top, bounds.right, bounds.bottom]
            .iter()
            .any(|v| v.abs_diff(0) > 8192)
        {
            return Err(E_INVALIDARG.into());
        }
        let (left, right) = if run.bidiLevel & 1 == 0 {
            (x, x + advance)
        } else {
            (x - advance, x)
        };
        let layout_bounds = layout_bounds(run, paint.vertical, baseline, left, right)?;
        let glyph_guides = if self.glyph_guides {
            glyph_guides(&self.factory, run, transform, mode, baseline, layout_bounds)?
        } else {
            Vec::new()
        };
        #[cfg(test)]
        let info = {
            let desc = unsafe { &*description };
            let n = desc.stringLength as usize;
            RunInfo {
                text: if desc.string.is_null() {
                    Vec::new()
                } else {
                    unsafe { std::slice::from_raw_parts(desc.string.0, n).to_vec() }
                },
                clusters: if desc.clusterMap.is_null() {
                    Vec::new()
                } else {
                    unsafe { std::slice::from_raw_parts(desc.clusterMap, n).to_vec() }
                },
                glyphs: unsafe {
                    std::slice::from_raw_parts(run.glyphIndices, run.glyphCount as usize).to_vec()
                },
                offsets: if run.glyphOffsets.is_null() {
                    Vec::new()
                } else {
                    unsafe {
                        std::slice::from_raw_parts(run.glyphOffsets, run.glyphCount as usize)
                            .to_vec()
                    }
                },
                bidi: run.bidiLevel,
                x,
                advance,
            }
        };
        self.ink
            .lock()
            .map_err(|_| windows::core::Error::from(E_INVALIDARG))?
            .push(InkRun {
                analysis,
                guide: Guide {
                    bounds,
                    layout_bounds,
                    baseline,
                    left,
                    right,
                },
                glyph_guides,
                color: paint.color,
                #[cfg(test)]
                info,
            });
        Ok(())
    }
    fn DrawUnderline(
        &self,
        _: *const c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_UNDERLINE,
        _: Ref<IUnknown>,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn DrawStrikethrough(
        &self,
        _: *const c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_STRIKETHROUGH,
        _: Ref<IUnknown>,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn DrawInlineObject(
        &self,
        _: *const c_void,
        _: f32,
        _: f32,
        _: Ref<IDWriteInlineObject>,
        _: BOOL,
        _: BOOL,
        _: Ref<IUnknown>,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}
fn paragraph_ranges(text: &[u16]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < text.len() {
        if matches!(text[i], 10 | 13 | 0x2029) {
            out.push(start..i);
            if text[i] == 13 && text.get(i + 1) == Some(&10) {
                i += 1;
            }
            start = i + 1;
        }
        i += 1;
    }
    out.push(start..text.len());
    out
}
fn is_rtl(text: &[u16]) -> bool {
    let bidi = CodePointMapData::<BidiClass>::new();
    let mut isolates = 0usize;
    for c in char::decode_utf16(text.iter().copied()).flatten() {
        // UAX #9 P2: strong characters inside isolates do not determine the
        // surrounding paragraph direction. DirectWrite handles their content.
        match bidi.get(c) {
            BidiClass::LeftToRightIsolate
            | BidiClass::RightToLeftIsolate
            | BidiClass::FirstStrongIsolate => {
                isolates += 1;
                continue;
            }
            BidiClass::PopDirectionalIsolate => {
                isolates = isolates.saturating_sub(1);
                continue;
            }
            _ if isolates > 0 => continue,
            BidiClass::LeftToRight => return false,
            BidiClass::RightToLeft | BidiClass::ArabicLetter => return true,
            _ => {}
        }
    }
    false
}
fn family_name(font: &IDWriteFont) -> Result<(HSTRING, IDWriteFontCollection)> {
    unsafe {
        let family = font.GetFontFamily()?;
        let names = family.GetFamilyNames()?;
        let mut index = 0;
        let mut exists = BOOL::default();
        names.FindLocaleName(&HSTRING::from("en-us"), &mut index, &mut exists)?;
        if !exists.as_bool() {
            index = 0;
        }
        let mut name = vec![0u16; names.GetStringLength(index)? as usize + 1];
        names.GetString(index, &mut name)?;
        name.pop();
        Ok((HSTRING::from_wide(&name), family.GetFontCollection()?))
    }
}
struct ResolvedFont {
    font: IDWriteFont,
    family: HSTRING,
    collection: IDWriteFontCollection,
}
fn shaped_layout(
    fonts: &mut dyn Fonts,
    factory: &IDWriteFactory,
    text: &StyledText,
    options: &LayoutOptions,
) -> std::result::Result<ShapedBitmap, String> {
    let base = fonts.get(&options.base_family)?;
    let (family, collection) = family_name(&base).map_err(|e| e.to_string())?;
    let mut metrics = DWRITE_FONT_METRICS::default();
    unsafe {
        base.GetMetrics(&mut metrics);
    }
    let units = f32::from(metrics.designUnitsPerEm);
    if units == 0.0 {
        return Err("Invalid font metrics".into());
    }
    let baseline = f32::from(metrics.ascent) / units * options.base_size;
    let nominal_height =
        (f32::from(metrics.ascent) + f32::from(metrics.descent)) / units * options.base_size;
    let line_step =
        (f32::from(metrics.ascent) + f32::from(metrics.descent) + f32::from(metrics.lineGap))
            / units
            * options.base_size
            + options.line_spacing;
    if !line_step.is_finite() || !(0.01..=4096.0).contains(&line_step) {
        return Err("Invalid line spacing".into());
    }
    let format = unsafe {
        factory.CreateTextFormat(
            &family,
            &collection,
            base.GetWeight(),
            base.GetStyle(),
            base.GetStretch(),
            options.base_size,
            &HSTRING::from(""),
        )
    }
    .map_err(|e| e.to_string())?;
    unsafe {
        format
            .SetWordWrapping(if options.wrap_width.is_some() {
                DWRITE_WORD_WRAPPING_WRAP
            } else {
                DWRITE_WORD_WRAPPING_NO_WRAP
            })
            .map_err(|e| e.to_string())?;
        format
            .SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, line_step, baseline)
            .map_err(|e| e.to_string())?;
    }
    let mut resolved = HashMap::<String, ResolvedFont>::new();
    let mut effects = Vec::<(TextStyle, IUnknown)>::new();
    let mut layouts = Vec::new();
    let mut max_width = 0.0f32;
    let mut total_lines = 0usize;
    for paragraph in paragraph_ranges(&text.utf16) {
        let contents = &text.utf16[paragraph.clone()];
        let layout = unsafe {
            factory.CreateTextLayout(
                contents,
                &format,
                options.wrap_width.unwrap_or(4096.0),
                4096.0,
            )
        }
        .map_err(|e| e.to_string())?;
        if options.disable_ligatures || text.ranges.iter().any(|r| r.style.disable_ligatures) {
            let typography = unsafe { factory.CreateTypography() }.map_err(|e| e.to_string())?;
            for tag in [*b"liga", *b"clig", *b"dlig", *b"hlig", *b"rlig"] {
                unsafe {
                    typography
                        .AddFontFeature(DWRITE_FONT_FEATURE {
                            nameTag: DWRITE_FONT_FEATURE_TAG(u32::from_le_bytes(tag)),
                            parameter: 0,
                        })
                        .map_err(|e| e.to_string())?;
                }
            }
            for range in &text.ranges {
                let start = paragraph.start.max(range.utf16.start);
                let end = paragraph.end.min(range.utf16.end);
                if start < end && (options.disable_ligatures || range.style.disable_ligatures) {
                    unsafe {
                        layout
                            .SetTypography(
                                &typography,
                                DWRITE_TEXT_RANGE {
                                    startPosition: (start - paragraph.start) as u32,
                                    length: (end - start) as u32,
                                },
                            )
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        let rtl = is_rtl(contents);
        unsafe {
            layout
                .SetReadingDirection(if rtl {
                    DWRITE_READING_DIRECTION_RIGHT_TO_LEFT
                } else {
                    DWRITE_READING_DIRECTION_LEFT_TO_RIGHT
                })
                .map_err(|e| e.to_string())?;
        }
        let spacing: IDWriteTextLayout1 = layout.cast().map_err(|e| e.to_string())?;
        for range in &text.ranges {
            let start = paragraph.start.max(range.utf16.start);
            let end = paragraph.end.min(range.utf16.end);
            if start >= end {
                continue;
            }
            let style = &range.style;
            if !style.valid() {
                return Err("Invalid text style".into());
            }
            if !resolved.contains_key(&style.family) {
                let font = fonts.get(&style.family)?;
                let (family, collection) = family_name(&font).map_err(|e| e.to_string())?;
                resolved.insert(
                    style.family.clone(),
                    ResolvedFont {
                        font,
                        family,
                        collection,
                    },
                );
            }
            let font = &resolved[&style.family];
            let native_range = DWRITE_TEXT_RANGE {
                startPosition: (start - paragraph.start) as u32,
                length: (end - start) as u32,
            };
            let effect = if let Some((_, effect)) = effects.iter().find(|(s, _)| s == style) {
                effect.clone()
            } else {
                let effect = ComObject::new(PaintEffect {
                    paint: Paint {
                        vertical: style.vertical / style.horizontal,
                        baseline_shift: style.baseline_shift,
                        color: style.color,
                    },
                })
                .into_interface::<IUnknown>();
                effects.push((style.clone(), effect.clone()));
                effect
            };
            unsafe {
                layout
                    .SetFontCollection(&font.collection, native_range)
                    .map_err(|e| e.to_string())?;
                layout
                    .SetFontFamilyName(&font.family, native_range)
                    .map_err(|e| e.to_string())?;
                layout
                    .SetFontWeight(
                        if style.bold {
                            DWRITE_FONT_WEIGHT_BOLD
                        } else {
                            font.font.GetWeight()
                        },
                        native_range,
                    )
                    .map_err(|e| e.to_string())?;
                layout
                    .SetFontStyle(
                        if style.italic {
                            DWRITE_FONT_STYLE_ITALIC
                        } else {
                            font.font.GetStyle()
                        },
                        native_range,
                    )
                    .map_err(|e| e.to_string())?;
                layout
                    .SetFontStretch(font.font.GetStretch(), native_range)
                    .map_err(|e| e.to_string())?;
                layout
                    .SetFontSize(style.size * style.horizontal, native_range)
                    .map_err(|e| e.to_string())?;
                spacing
                    .SetPairKerning(true, native_range)
                    .map_err(|e| e.to_string())?;
                spacing
                    .SetCharacterSpacing(0.0, style.tracking, 0.0, native_range)
                    .map_err(|e| e.to_string())?;
                layout
                    .SetDrawingEffect(&effect, native_range)
                    .map_err(|e| e.to_string())?;
            }
        }
        let mut tm = DWRITE_TEXT_METRICS::default();
        unsafe {
            layout.GetMetrics(&mut tm).map_err(|e| e.to_string())?;
        }
        if !tm.widthIncludingTrailingWhitespace.is_finite()
            || tm.widthIncludingTrailingWhitespace > 4096.0
        {
            return Err("Text image exceeds 4096 pixels".into());
        }
        max_width = max_width.max(tm.widthIncludingTrailingWhitespace);
        // DirectWrite reports one line for an empty layout on Windows. Keep a
        // defensive minimum so empty and trailing paragraphs preserve height.
        let count = tm.lineCount.max(1) as usize;
        layouts.push((layout, rtl, total_lines));
        total_lines += count;
        if total_lines as f32 * line_step > 8192.0 {
            return Err("Too many lines".into());
        }
    }
    let width = options.wrap_width.unwrap_or(max_width.max(1.0));
    let renderer = ComObject::new(Renderer {
        factory: factory.clone(),
        ink: Mutex::new(Vec::new()),
        glyph_guides: options.glyph_guides,
    });
    let interface = renderer.to_interface::<IDWriteTextRenderer>();
    for (layout, rtl, line) in layouts {
        let alignment = match (options.column, rtl) {
            (1, _) => DWRITE_TEXT_ALIGNMENT_CENTER,
            (2, false) | (0, true) => DWRITE_TEXT_ALIGNMENT_TRAILING,
            _ => DWRITE_TEXT_ALIGNMENT_LEADING,
        };
        unsafe {
            layout.SetMaxWidth(width).map_err(|e| e.to_string())?;
            layout
                .SetTextAlignment(alignment)
                .map_err(|e| e.to_string())?;
            layout
                .Draw(None, &interface, 0.0, line as f32 * line_step)
                .map_err(|e| e.to_string())?;
        }
    }
    let ink = std::mem::take(&mut *renderer.ink.lock().map_err(|_| "Renderer lock failed")?);
    let mut left = 0;
    let mut top = 0;
    let mut right = width.ceil().max(1.0) as i32;
    let logical_height = (total_lines.saturating_sub(1) as f32 * line_step + nominal_height).ceil();
    if !logical_height.is_finite() || logical_height > 4096.0 {
        return Err("Text image exceeds 4096 pixels".into());
    }
    let mut bottom = logical_height.max(1.0) as i32;
    for run in &ink {
        left = left.min(run.guide.bounds.left);
        top = top.min(run.guide.bounds.top);
        right = right.max(run.guide.bounds.right);
        bottom = bottom.max(run.guide.bounds.bottom);
        if options.glyph_guides {
            for guide in &run.glyph_guides {
                left = left.min(guide.layout_bounds.left);
                top = top.min(guide.layout_bounds.top);
                right = right.max(guide.layout_bounds.right);
                bottom = bottom.max(guide.layout_bounds.bottom);
            }
        }
    }
    let width = right - left;
    let height = bottom - top;
    if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
        return Err("Text image exceeds 4096 pixels".into());
    }
    let mut bitmap = Bitmap {
        width,
        height,
        bgra: vec![0; width as usize * height as usize * 4],
    };
    let mut guides = Vec::new();
    #[cfg(test)]
    let mut runs = Vec::new();
    for run in ink {
        let bounds = run.guide.bounds;
        let gw = (bounds.right - bounds.left).max(0) as usize;
        let gh = (bounds.bottom - bounds.top).max(0) as usize;
        if gw != 0 && gh != 0 {
            let mut alpha = vec![0u8; gw * gh * 3];
            unsafe {
                run.analysis
                    .CreateAlphaTexture(DWRITE_TEXTURE_CLEARTYPE_3x1, &bounds, &mut alpha)
                    .map_err(|e| e.to_string())?;
            }
            for y in 0..gh {
                for x in 0..gw {
                    let source = (y * gw + x) * 3;
                    let a = ((u32::from(alpha[source])
                        + u32::from(alpha[source + 1])
                        + u32::from(alpha[source + 2]))
                        / 3) as u8;
                    let dx = bounds.left - left + x as i32;
                    let dy = bounds.top - top + y as i32;
                    blend(
                        &mut bitmap.bgra[((dy * width + dx) * 4) as usize..][..4],
                        run.color,
                        a,
                    );
                }
            }
        }
        let run_guides = if options.glyph_guides {
            run.glyph_guides
        } else {
            vec![run.guide]
        };
        for guide in run_guides {
            let shift = |r: RECT| RECT {
                left: r.left - left,
                right: r.right - left,
                top: r.top - top,
                bottom: r.bottom - top,
            };
            guides.push(Guide {
                bounds: shift(guide.bounds),
                layout_bounds: shift(guide.layout_bounds),
                baseline: guide.baseline - top as f32,
                left: guide.left - left as f32,
                right: guide.right - left as f32,
            });
        }
        #[cfg(test)]
        runs.push(run.info);
    }
    Ok(ShapedBitmap {
        bitmap,
        guides,
        #[cfg(test)]
        runs,
    })
}
pub(crate) fn rasterize(
    fonts: &mut dyn Fonts,
    text: &StyledText,
    options: &LayoutOptions,
) -> std::result::Result<ShapedBitmap, String> {
    if text.utf16.len() > 65536
        || !options.base_size.is_finite()
        || !(0.01..=1024.0).contains(&options.base_size)
        || !(0..=2).contains(&options.column)
        || options
            .wrap_width
            .is_some_and(|v| !v.is_finite() || !(1.0..=4096.0).contains(&v))
    {
        return Err("Invalid text layout".into());
    }
    let factory: IDWriteFactory =
        unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.map_err(|e| e.to_string())?;
    if options.disable_ligatures || text.ranges.iter().any(|r| r.style.disable_ligatures) {
        let separated = non_joining_text(text, options.disable_ligatures)?;
        // The extra controls have no ink or advance; the same resource bound
        // applies to the source, allowing at most one control per scalar.
        shaped_layout(fonts, &factory, &separated, options)
    } else {
        shaped_layout(fonts, &factory, text, options)
    }
}
// Keep straight BGRA, matching obj.putpixeldata rather than premultiplied D2D.
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
    struct TestFonts {
        system: IDWriteFontCollection,
        private: Option<IDWriteFontCollection>,
    }
    impl TestFonts {
        fn new() -> Self {
            let factory: IDWriteFactory =
                unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.unwrap();
            let mut collection = None;
            unsafe {
                factory
                    .GetSystemFontCollection(&mut collection, false)
                    .unwrap();
            }
            Self {
                system: collection.unwrap(),
                private: None,
            }
        }
        fn private_file(file: &std::path::Path) -> Self {
            let factory: IDWriteFactory =
                unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.unwrap();
            let modern: IDWriteFactory5 = factory.cast().unwrap();
            let builder = unsafe { modern.CreateFontSetBuilder() }.unwrap();
            let file = unsafe {
                factory.CreateFontFileReference(&HSTRING::from(file.to_str().unwrap()), None)
            }
            .unwrap();
            unsafe {
                builder.AddFontFile(&file).unwrap();
            }
            let set = unsafe { builder.CreateFontSet() }.unwrap();
            let factory3: IDWriteFactory3 = factory.cast().unwrap();
            let collection = unsafe { factory3.CreateFontCollectionFromFontSet(&set) }
                .unwrap()
                .cast()
                .unwrap();
            Self {
                private: Some(collection),
                ..Self::new()
            }
        }
    }
    impl Fonts for TestFonts {
        fn get(&mut self, family: &str) -> std::result::Result<IDWriteFont, String> {
            for collection in self.private.iter().chain(std::iter::once(&self.system)) {
                let mut index = 0;
                let mut exists = BOOL::default();
                unsafe {
                    collection
                        .FindFamilyName(&HSTRING::from(family), &mut index, &mut exists)
                        .unwrap();
                }
                if exists.as_bool() {
                    return Ok(unsafe {
                        collection
                            .GetFontFamily(index)
                            .unwrap()
                            .GetFirstMatchingFont(
                                DWRITE_FONT_WEIGHT_NORMAL,
                                DWRITE_FONT_STRETCH_NORMAL,
                                DWRITE_FONT_STYLE_NORMAL,
                            )
                    }
                    .unwrap());
                }
            }
            Err(format!("Test font missing: {family}"))
        }
    }
    fn style(family: &str) -> TextStyle {
        TextStyle {
            family: family.into(),
            size: 48.0,
            horizontal: 1.0,
            vertical: 1.0,
            tracking: 0.0,
            baseline_shift: 0.0,
            color: 0xffffff,
            bold: false,
            italic: false,
            disable_ligatures: false,
        }
    }
    fn options() -> LayoutOptions {
        LayoutOptions {
            base_family: "Segoe UI".into(),
            base_size: 48.0,
            line_spacing: 0.0,
            column: 0,
            wrap_width: None,
            glyph_guides: false,
            disable_ligatures: false,
        }
    }
    fn draw(fonts: &mut TestFonts, text: &str, style: TextStyle) -> ShapedBitmap {
        let mut plan = StyledText::default();
        plan.push(text, style);
        rasterize(fonts, &plan, &options()).unwrap()
    }
    fn ink(image: &ShapedBitmap) -> usize {
        image
            .bitmap
            .bgra
            .chunks_exact(4)
            .filter(|p| p[3] != 0)
            .count()
    }
    fn check_shape(image: &ShapedBitmap, text: &str) {
        assert!(ink(image) > 0, "{text}: no ink");
        assert!(!image.runs.is_empty(), "{text}: no glyph runs");
        for run in &image.runs {
            assert!(
                run.glyphs.iter().all(|g| *g != 0),
                "{text}: missing glyphs {:?}",
                run.glyphs
            );
            assert_eq!(run.clusters.len(), run.text.len(), "{text}");
            assert!(
                run.offsets
                    .iter()
                    .all(|o| o.advanceOffset.is_finite() && o.ascenderOffset.is_finite())
            );
        }
    }
    #[test]
    fn native_text_shaping_supports_indic_marks_and_conjuncts() {
        let mut fonts = TestFonts::new();
        for text in [
            "ଳି",
            "କ୍ଳ",
            "କ୍ଷ",
            "कि",
            "क्ष",
            "বাংলা",
            "ক্ষ",
            "ਪੰਜਾਬੀ",
            "ગુજરાતી",
            "தமிழ்",
            "తెలుగు",
            "ಕನ್ನಡ",
            "മലയാളം",
            "සිංහල",
        ] {
            let image = draw(&mut fonts, text, style("Nirmala UI"));
            check_shape(&image, text);
            assert!(
                image
                    .runs
                    .iter()
                    .any(|r| r.clusters.windows(2).any(|p| p[0] == p[1])),
                "{text}: no shaped cluster"
            );
        }
    }
    #[test]
    fn native_arabic_ligature_and_mixed_bidi_are_positioned_by_directwrite() {
        let mut fonts = TestFonts::new();
        let image = draw(&mut fonts, "لا", style("Arial"));
        check_shape(&image, "لا");
        assert!(image.runs.iter().any(|r| r.bidi & 1 == 1));
        assert!(
            image.runs.iter().map(|r| r.glyphs.len()).sum::<usize>() < 2,
            "lam alef ligature"
        );
        let mixed = draw(&mut fonts, "سلام 123 ABC", style("Segoe UI"));
        check_shape(&mixed, "سلام 123 ABC");
        let arabic = mixed.runs.iter().find(|r| r.bidi & 1 == 1).unwrap();
        let latin = mixed
            .runs
            .iter()
            .find(|r| String::from_utf16_lossy(&r.text).contains("ABC"))
            .unwrap();
        assert!(
            arabic.x - arabic.advance > latin.x,
            "Arabic should be on the right of the LTR suffix"
        );
        assert!(is_rtl(&"123 سلام".encode_utf16().collect::<Vec<_>>()));
        assert!(!is_rtl(&"ABC שלום".encode_utf16().collect::<Vec<_>>()));
    }
    #[test]
    fn native_combining_kerning_and_latin_ligatures_use_cluster_glyphs() {
        let mut fonts = TestFonts::new();
        for (text, family) in [
            ("e\u{301}", "Segoe UI"),
            ("か\u{3099}", "Yu Gothic UI"),
            ("한", "Malgun Gothic"),
            ("กิ", "Leelawadee UI"),
            ("កា", "Leelawadee UI"),
            ("ກິ", "Leelawadee UI"),
            ("שָ", "Segoe UI"),
        ] {
            let image = draw(&mut fonts, text, style(family));
            check_shape(&image, text);
            assert!(
                image
                    .runs
                    .iter()
                    .any(|r| r.clusters.windows(2).any(|p| p[0] == p[1]))
                    || image.runs.iter().map(|r| r.glyphs.len()).sum::<usize>()
                        < text.chars().count(),
                "{text}: {:?}",
                image.runs
            );
        }
        let ffi = draw(&mut fonts, "ffi", style("Calibri"));
        check_shape(&ffi, "ffi");
        assert!(
            ffi.runs.iter().map(|r| r.glyphs.len()).sum::<usize>() < 3,
            "ffi ligature"
        );
        let av = draw(&mut fonts, "AV", style("Times New Roman"));
        let a = draw(&mut fonts, "A", style("Times New Roman"));
        let v = draw(&mut fonts, "V", style("Times New Roman"));
        let advance = |image: &ShapedBitmap| image.runs.iter().map(|r| r.advance).sum::<f32>();
        assert!(advance(&av) < advance(&a) + advance(&v), "native kerning");
    }
    #[test]
    fn non_joining_layout_preserves_source_styles_surrogates_and_paragraphs() {
        let mut text = StyledText::default();
        text.push("A𐐀", style("Segoe UI"));
        text.push("\r\nଳି\u{2029}B", style("Nirmala UI"));
        let original = text.utf16.clone();
        let separated = non_joining_text(&text, true).unwrap();
        let visible = String::from_utf16(&separated.utf16).unwrap();
        assert_eq!(visible, "A\u{200c}𐐀\r\nଳ\u{200c}ି\u{2029}B");
        assert_eq!(text.utf16, original);
        assert_eq!(separated.ranges.len(), 2);
        assert_eq!(separated.ranges[0].utf16, 0..4);
        assert_eq!(separated.ranges[0].style.family, "Segoe UI");
        assert_eq!(separated.ranges[1].utf16, 4..11);
        assert_eq!(separated.ranges[1].style.family, "Nirmala UI");
    }

    #[test]
    fn forced_ligature_disable_separates_latin_arabic_and_indic_glyphs() {
        let mut fonts = TestFonts::new();
        for (text, family) in [
            ("ffi", "Calibri"),
            ("لا", "Arial"),
            ("କ୍ଷ", "Nirmala UI"),
            ("क्ष", "Nirmala UI"),
        ] {
            let mut plan = StyledText::default();
            plan.push(text, style(family));
            let normal = rasterize(
                &mut fonts,
                &plan,
                &LayoutOptions {
                    glyph_guides: true,
                    ..options()
                },
            )
            .unwrap();
            let separated = rasterize(
                &mut fonts,
                &plan,
                &LayoutOptions {
                    disable_ligatures: true,
                    glyph_guides: true,
                    ..options()
                },
            )
            .unwrap();
            let count = |image: &ShapedBitmap| {
                image
                    .guides
                    .iter()
                    .filter(|g| g.bounds.right > g.bounds.left && g.bounds.bottom > g.bounds.top)
                    .count()
            };
            eprintln!("{text}: {} -> {}", count(&normal), count(&separated));
            assert!(
                count(&separated) > count(&normal),
                "ligature was not separated: {text}"
            );
            assert_eq!(plan.utf16, text.encode_utf16().collect::<Vec<_>>());
            assert!(ink(&separated) > 0);
        }
    }

    #[test]
    fn profile_ligature_switch_is_local_and_matches_decorated_rendering() {
        use crate::render::{RenderRequest, layout_options, prepare};
        use compositefont_core::{CompositeFontProfile, FontAdjustment, UnicodeRange};
        let mut fonts = TestFonts::new();
        let source = "ffi لا କ୍ଷ";
        for family in ["", "Calibri"] {
            let mut profile = CompositeFontProfile::neutral_default();
            profile.western.font_family = family.into();
            profile.western.disable_ligatures = true;
            profile.other.font_family = "Arial".into();
            profile.custom_ranges.push(UnicodeRange {
                name: "Oriya".into(),
                start: 0x0b00,
                end: 0x0b7f,
                adjustment: FontAdjustment {
                    font_family: "Nirmala UI".into(),
                    disable_ligatures: true,
                    ..FontAdjustment::neutral()
                },
            });
            let request = RenderRequest {
                text: source.into(),
                profile: Some(profile.clone()),
                font: "Calibri".into(),
                size: 48.0,
                spacing: 0.0,
                line_spacing: 0.0,
                color: "ffffff".into(),
                align: 0,
                bold: false,
                italic: false,
                disable_ligatures: false,
            };
            let decorated = crate::decorate_plain_text_with_coverage(
                source,
                &profile,
                Some(24.0),
                Some(0.0),
                &mut Coverage(&mut fonts),
            );
            assert!(decorated.contains("<cf_liga0>"));
            let generated_request = RenderRequest {
                text: decorated.clone(),
                profile: None,
                ..request.clone()
            };
            let direct = prepare(&request, source, &mut Coverage(&mut fonts))
                .unwrap()
                .unwrap();
            let generated = prepare(&generated_request, &decorated, &mut Coverage(&mut fonts))
                .unwrap()
                .unwrap();
            assert_eq!(direct.utf16, source.encode_utf16().collect::<Vec<_>>());
            assert_eq!(direct.utf16, generated.utf16);
            assert_eq!(direct.ranges.len(), generated.ranges.len());
            for (a, b) in direct.ranges.iter().zip(&generated.ranges) {
                assert_eq!(a.utf16, b.utf16);
                assert_eq!(a.style, b.style);
            }
            let mut layout = layout_options(&request, None);
            layout.glyph_guides = true;
            let a = rasterize(&mut fonts, &direct, &layout).unwrap();
            let b = rasterize(&mut fonts, &generated, &layout).unwrap();
            assert_eq!(
                (a.bitmap.width, a.bitmap.height),
                (b.bitmap.width, b.bitmap.height)
            );
            assert_eq!(a.bitmap.bgra, b.bitmap.bgra);
            let count = |image: &ShapedBitmap| {
                image
                    .guides
                    .iter()
                    .filter(|g| g.bounds.right > g.bounds.left && g.bounds.bottom > g.bounds.top)
                    .count()
            };
            assert_eq!(count(&a), 7, "only Latin and Oriya must separate");
            layout.disable_ligatures = true;
            let all = rasterize(&mut fonts, &generated, &layout).unwrap();
            assert_eq!(count(&all), 8, "object force also separates Arabic");
            profile.western.disable_ligatures = false;
            profile.custom_ranges[0].adjustment.disable_ligatures = false;
            let restored = prepare(
                &RenderRequest {
                    profile: Some(profile),
                    ..request.clone()
                },
                source,
                &mut Coverage(&mut fonts),
            )
            .unwrap()
            .unwrap();
            layout.disable_ligatures = false;
            let normal = rasterize(&mut fonts, &restored, &layout).unwrap();
            assert_eq!(count(&normal), 3);
        }
    }

    #[test]
    fn preview_guides_follow_shaped_glyphs_and_separate_ink_from_logical_cells() {
        let mut fonts = TestFonts::new();
        for (text, family) in [
            (" i ", "Segoe UI"),
            ("ffi", "Calibri"),
            ("لا", "Arial"),
            ("שָ", "Segoe UI"),
            ("କ୍ଷ", "Nirmala UI"),
        ] {
            let mut plan = StyledText::default();
            let mut adjusted = style(family);
            adjusted.horizontal = 1.25;
            adjusted.vertical = 0.75;
            adjusted.baseline_shift = -12.0;
            plan.push(text, adjusted);
            let plain = rasterize(&mut fonts, &plan, &options()).unwrap();
            let image = rasterize(
                &mut fonts,
                &plan,
                &LayoutOptions {
                    glyph_guides: true,
                    ..options()
                },
            )
            .unwrap();
            assert_eq!(
                ink(&plain),
                ink(&image),
                "guides changed painted text: {text}"
            );
            assert_eq!(
                image.guides.len(),
                image.runs.iter().map(|r| r.glyphs.len()).sum::<usize>()
            );
            for guide in &image.guides {
                let cell = guide.layout_bounds;
                assert!(cell.top >= 0 && cell.bottom <= image.bitmap.height);
                assert!(cell.left >= 0 && cell.right <= image.bitmap.width);
                assert!(cell.top as f32 <= guide.baseline && guide.baseline <= cell.bottom as f32);
            }
            // Includes bidi pen direction and native combining-mark offsets:
            // every painted pixel must lie in a native individual-glyph box.
            for (index, pixel) in image.bitmap.bgra.chunks_exact(4).enumerate() {
                if pixel[3] == 0 {
                    continue;
                }
                let x = index as i32 % image.bitmap.width;
                let y = index as i32 / image.bitmap.width;
                assert!(
                    image.guides.iter().any(|g| x >= g.bounds.left
                        && x < g.bounds.right
                        && y >= g.bounds.top
                        && y < g.bounds.bottom),
                    "{text}: uncovered ink {x},{y}"
                );
            }
            if text == " i " {
                assert_eq!(image.guides.len(), 3);
                let space = &image.guides[0];
                assert_eq!(space.bounds.left, space.bounds.right);
                assert!(space.layout_bounds.right > space.layout_bounds.left);
                let i = &image.guides[1];
                assert!(
                    i.layout_bounds.bottom - i.layout_bounds.top > i.bounds.bottom - i.bounds.top
                );
            } else if matches!(text, "ffi" | "لا") {
                assert!(
                    image.guides.len() < text.chars().count(),
                    "ligature was split: {text}"
                );
            }
        }
    }
    #[test]
    fn scaling_tracking_colors_and_baseline_preserve_arabic_ligatures() {
        let mut fonts = TestFonts::new();
        let plain = draw(&mut fonts, "لا", style("Arial"));
        let mut custom = style("Arial");
        custom.horizontal = 1.5;
        custom.vertical = 0.75;
        custom.tracking = 3.0;
        custom.baseline_shift = -6.0;
        custom.color = 0xff4000;
        let changed = draw(&mut fonts, "لا", custom);
        check_shape(&changed, "لا");
        assert_eq!(plain.runs[0].glyphs, changed.runs[0].glyphs);
        assert!(changed.runs[0].advance > plain.runs[0].advance * 1.45);
        assert!(changed.bitmap.width > plain.bitmap.width);
        let plain_ink_height = plain.guides[0].bounds.bottom - plain.guides[0].bounds.top;
        let changed_ink_height = changed.guides[0].bounds.bottom - changed.guides[0].bounds.top;
        assert!(changed_ink_height < plain_ink_height);
        assert_eq!(changed.guides[0].baseline, plain.guides[0].baseline - 6.0);
        assert!(
            changed
                .bitmap
                .bgra
                .chunks_exact(4)
                .filter(|p| p[3] > 0)
                .all(|p| p[..3] == [0, 64, 255])
        );
    }
    #[test]
    fn multiline_alignment_wrapping_and_trailing_empty_lines_are_bounded() {
        let mut fonts = TestFonts::new();
        let text = "ABC\r\nسلام\n\n";
        let plan = {
            let mut plan = StyledText::default();
            plan.push(text, style("Segoe UI"));
            plan
        };
        assert_eq!(paragraph_ranges(&plan.utf16).len(), 4);
        for column in 0..=2 {
            let image = rasterize(
                &mut fonts,
                &plan,
                &LayoutOptions {
                    column,
                    ..options()
                },
            )
            .unwrap();
            assert!(image.bitmap.width < 300);
            assert!(image.bitmap.height > 200 && image.bitmap.height < 400);
        }
        let mut long = StyledText::default();
        long.push(
            "one two three four five six seven eight nine",
            style("Segoe UI"),
        );
        let wrapped = rasterize(
            &mut fonts,
            &long,
            &LayoutOptions {
                wrap_width: Some(180.0),
                ..options()
            },
        )
        .unwrap();
        assert!(wrapped.bitmap.width <= 185 && wrapped.bitmap.height > 100);
        assert!(wrapped.guides.iter().any(|g| g.baseline > 80.0));
        let mut huge = StyledText::default();
        huge.push(&"A\n".repeat(1000), style("Segoe UI"));
        assert!(rasterize(&mut fonts, &huge, &options()).is_err());
        assert!(
            rasterize(
                &mut fonts,
                &plan,
                &LayoutOptions {
                    line_spacing: f32::MAX,
                    ..options()
                }
            )
            .is_err()
        );
    }
    #[test]
    fn straight_bgra_blending_preserves_object_pixel_contract() {
        let mut pixel = [0; 4];
        blend(&mut pixel, 0xff0000, 128);
        assert_eq!(pixel, [0, 0, 255, 128]);
        blend(&mut pixel, 0x0000ff, 255);
        assert_eq!(pixel, [255, 0, 0, 255]);
    }
    #[test]
    #[ignore = "Requires the separately downloaded official Noto Sans Oriya verification font in target"]
    fn official_noto_oriya_private_font_shapes_conjuncts_and_vowels() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/NotoSansOriya-Regular-verification.ttf");
        assert!(
            file.exists(),
            "Download the official Noto Sans Oriya fixture before running this test"
        );
        let mut fonts = TestFonts::private_file(&file);
        verify_decorate_pipeline(&mut fonts);
        for text in ["ଳ", "ଳା", "ଳି", "କ୍ଳ", "କ୍ଷ", "ଓଡ଼ିଆ"] {
            let image = draw(&mut fonts, text, style("Noto Sans Oriya"));
            check_shape(&image, text);
            if text == "ଳ" {
                assert_eq!(image.runs[0].glyphs, vec![48]);
            }
            if text == "କ୍ଳ" || text == "କ୍ଷ" {
                assert!(
                    image
                        .runs
                        .iter()
                        .any(|r| r.clusters.windows(2).any(|p| p[0] == p[1]))
                );
                assert!(
                    image.runs.iter().map(|r| r.glyphs.len()).sum::<usize>() < text.chars().count()
                );
            }
            println!(
                "{text}: glyphs={}, bitmap={}x{}, ink={}",
                image.runs.iter().map(|r| r.glyphs.len()).sum::<usize>(),
                image.bitmap.width,
                image.bitmap.height,
                ink(&image)
            );
        }
    }
    struct Coverage<'a>(&'a mut TestFonts);
    impl crate::font_coverage::GlyphCoverage for Coverage<'_> {
        fn has_glyph(&mut self, family: &str, character: char) -> Option<bool> {
            unsafe {
                self.0
                    .get(family)
                    .ok()?
                    .HasCharacter(character as u32)
                    .ok()
                    .map(|v| v.as_bool())
            }
        }
    }
    fn verify_decorate_pipeline(fonts: &mut TestFonts) {
        use crate::render::{RenderRequest, layout_options, prepare};
        use compositefont_core::{CompositeFontProfile, FontAdjustment, UnicodeRange};
        let mut profile = CompositeFontProfile::neutral_default();
        profile.custom_ranges.push(UnicodeRange {
            name: "Oriya".into(),
            start: 0x0b00,
            end: 0x0b7f,
            adjustment: FontAdjustment {
                font_family: "Noto Sans Oriya".into(),
                size_ratio: 1.25,
                baseline_shift_em: 0.125,
                tracking_adjust_em: 0.125,
                horizontal_scale_ratio: 1.25,
                vertical_scale_ratio: 0.75,
                ..FontAdjustment::neutral()
            },
        });
        for source in ["ଳି କ୍ଳ\nକ୍ଷ", "<#ff00ff>ଳା କ୍ଷ<#>", "<s36>ଳି କ୍ଷ<s>"]
        {
            let request = RenderRequest {
                text: source.into(),
                profile: Some(profile.clone()),
                font: "Segoe UI".into(),
                size: 48.0,
                spacing: 2.0,
                line_spacing: 1.0,
                color: "ffffff".into(),
                align: 0,
                bold: false,
                italic: false,
                disable_ligatures: false,
            };
            let decorated = crate::decorate_plain_text_with_coverage(
                source,
                &profile,
                Some(24.0),
                Some(2.0),
                &mut Coverage(fonts),
            );
            assert_ne!(decorated, source);
            let direct = prepare(&request, source, &mut Coverage(fonts))
                .unwrap()
                .unwrap();
            let generated_request = RenderRequest {
                text: decorated.clone(),
                profile: None,
                ..request.clone()
            };
            let generated = prepare(&generated_request, &decorated, &mut Coverage(fonts))
                .unwrap()
                .unwrap();
            assert_eq!(direct.utf16, generated.utf16);
            assert_eq!(direct.ranges.len(), generated.ranges.len(), "{source}");
            for (a, b) in direct.ranges.iter().zip(&generated.ranges) {
                assert_eq!(a.utf16, b.utf16);
                assert_eq!(a.style, b.style, "{source}");
            }
            let a = rasterize(fonts, &direct, &layout_options(&request, None)).unwrap();
            let b =
                rasterize(fonts, &generated, &layout_options(&generated_request, None)).unwrap();
            assert_eq!(
                (a.bitmap.width, a.bitmap.height),
                (b.bitmap.width, b.bitmap.height)
            );
            assert_eq!(
                a.bitmap.bgra, b.bitmap.bgra,
                "decorate must not double-apply metrics"
            );
            check_shape(&b, source);
            let forced_request = RenderRequest {
                disable_ligatures: true,
                ..request.clone()
            };
            let forced_generated = RenderRequest {
                disable_ligatures: true,
                ..generated_request.clone()
            };
            let forced_a =
                rasterize(fonts, &direct, &layout_options(&forced_request, None)).unwrap();
            let forced_b =
                rasterize(fonts, &generated, &layout_options(&forced_generated, None)).unwrap();
            assert_eq!(
                (forced_a.bitmap.width, forced_a.bitmap.height),
                (forced_b.bitmap.width, forced_b.bitmap.height)
            );
            assert_eq!(
                forced_a.bitmap.bgra, forced_b.bitmap.bgra,
                "forced decorate path differs"
            );
            assert!(forced_b.bitmap.bgra != b.bitmap.bgra);
            let restored =
                rasterize(fonts, &generated, &layout_options(&generated_request, None)).unwrap();
            assert_eq!(
                restored.bitmap.bgra, b.bitmap.bgra,
                "switching off did not restore shaping"
            );
        }
    }
    #[test]
    fn bidi_isolates_do_not_determine_the_surrounding_paragraph_direction() {
        let utf16 = |s: &str| s.encode_utf16().collect::<Vec<_>>();
        assert!(!is_rtl(&utf16("\u{2067}سلام\u{2069} ABC")));
        assert!(is_rtl(&utf16("\u{2066}ABC\u{2069} سلام")));
        assert!(!is_rtl(&utf16(
            "\u{2068}سلام\u{2066}XYZ\u{2069}\u{2069} 123"
        )));
        assert_eq!(paragraph_ranges(&utf16("ABC\u{2029}سلام")).len(), 2);
    }
}
