//! Text parsing and layout helpers used by the editor preview.
use crate::{
    control_text::{ControlTextDefaults, parse_control_text, parse_decorated_control_text},
    font_coverage::GlyphCoverage,
};
use compositefont_core::CompositeFontProfile;

#[derive(Clone, PartialEq)]
pub(crate) struct RenderRequest {
    pub text: String,
    pub profile: Option<CompositeFontProfile>,
    pub font: String,
    pub size: f64,
    pub spacing: f64,
    pub line_spacing: f64,
    pub color: String,
    pub align: i32,
    pub bold: bool,
    pub italic: bool,
    pub disable_ligatures: bool,
}

pub(crate) struct Bitmap {
    pub width: i32,
    pub height: i32,
    pub bgra: Vec<u8>,
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

pub(crate) fn layout_options(
    request: &RenderRequest,
    wrap_width: Option<f32>,
) -> crate::shaping::LayoutOptions {
    crate::shaping::LayoutOptions {
        base_family: request.font.clone(),
        base_size: request.size as f32,
        line_spacing: request.line_spacing as f32,
        column: request.align % 3,
        wrap_width,
        glyph_guides: false,
        disable_ligatures: request.disable_ligatures,
        force_ligatures: false,
        antialias: crate::shaping::AntialiasMethod::Natural,
        color_glyphs: false,
    }
}

/// Opt-in full-object DirectWrite rendering. Only the transient host image is
/// replaced; the saved text and standard path remain untouched.
pub(crate) fn render_ligature_opt_in(
    section: &aviutl2::generic::ReadSection,
    request: &RenderRequest,
    antialias: crate::shaping::AntialiasMethod,
) -> Result<Option<Bitmap>, String> {
    let mut coverage = crate::font_coverage::FontManagerGlyphCoverage::new(section);
    let Some(plan) = prepare(request, &request.text, &mut coverage)? else {
        return Ok(None);
    };
    let mut options = layout_options(request, None);
    options.force_ligatures = true;
    options.antialias = antialias;
    let mut fonts = crate::shaping::HostFonts::new(section);
    crate::shaping::rasterize(&mut fonts, &plan, &options).map(|image| Some(image.bitmap))
}

/// Tags are removed before script analysis. UTF-16 ranges refer to the visible
/// string; adjacent equivalent styles merge even when tags separate them.
pub(crate) fn prepare(
    request: &RenderRequest,
    text: &str,
    coverage: &mut dyn GlyphCoverage,
) -> Result<Option<crate::shaping::StyledText>, String> {
    prepare_with_units(request, text, coverage, 2.0)
}

/// Native alias sizes and <sN> controls are already measured in pixels.
pub(crate) fn prepare_native(
    request: &RenderRequest,
    text: &str,
    coverage: &mut dyn GlyphCoverage,
) -> Result<Option<crate::shaping::StyledText>, String> {
    prepare_with_units(request, text, coverage, 1.0)
}

fn prepare_with_units(
    request: &RenderRequest,
    text: &str,
    coverage: &mut dyn GlyphCoverage,
    pixels_per_unit: f64,
) -> Result<Option<crate::shaping::StyledText>, String> {
    use crate::shaping::{StyledText, TextStyle};
    use unicode_segmentation::UnicodeSegmentation;
    if !(1.0..=4096.0).contains(&request.size)
        || !request.spacing.is_finite()
        || !request.line_spacing.is_finite()
        || !(0..=8).contains(&request.align)
        || request.text.len() > 64 * 1024
        || text.len() > 64 * 1024
        || text.contains("<?")
        || text.contains('\t')
    {
        return Ok(None);
    }
    let base_size = request.size / pixels_per_unit;
    let parse = if request.profile.is_some() {
        parse_control_text
    } else {
        parse_decorated_control_text
    };
    let Ok(parsed) = parse(
        text,
        ControlTextDefaults {
            font_size: Some(base_size),
            font_family: Some(request.font.clone()),
        },
    ) else {
        return Ok(None);
    };
    let Ok(base_color) = u32::from_str_radix(request.color.trim_start_matches('#'), 16) else {
        return Ok(None);
    };
    if base_color > 0xffffff {
        return Ok(None);
    }
    let mut plan = StyledText {
        modified: request.profile.is_none(),
        ..Default::default()
    };
    for span in &parsed.spans {
        if span.format.at_decoration_override().is_some_and(|d| d != 0)
            || span.format.s_outline_override().is_some()
            || span.format.at_style_override().is_some_and(|s| s.strike)
            || span.format.s_style_override().is_some_and(|s| s.strike)
        {
            return Ok(None);
        }
        for cluster in text[span.range.clone()].graphemes(true) {
            let adjustment = request.profile.as_ref().map_or_else(
                compositefont_core::FontAdjustment::neutral,
                |profile| {
                    crate::resolved_adjustment_for_cluster(profile, cluster, &span.format, coverage)
                },
            );
            if !crate::is_safe_adjustment(&adjustment) {
                return Ok(None);
            }
            let size = span.format.effective_font_size().unwrap_or(base_size);
            let metrics = adjustment
                .effective_metrics(Some(size))
                .ok_or("Invalid text metrics")?;
            plan.modified |= !adjustment.effective_font_family().is_empty()
                || adjustment.effective_disable_ligatures()
                || !crate::approximately_equal(metrics.size_ratio, 1.0)
                || !crate::approximately_equal(metrics.baseline_shift_em, 0.0)
                || !crate::approximately_equal(metrics.tracking_adjust_em, 0.0)
                || !crate::approximately_equal(adjustment.effective_vertical_scale_ratio(), 1.0)
                || !crate::approximately_equal(adjustment.effective_horizontal_scale_ratio(), 1.0);
            let family = if adjustment.effective_font_family().is_empty()
                || !span.format.profile_font_allowed()
            {
                span.format.effective_font_family().unwrap_or(&request.font)
            } else {
                adjustment.effective_font_family()
            };
            let flags = span
                .format
                .s_style_override()
                .or_else(|| span.format.at_style_override());
            let color = if let Some(value) = &span.format.text_color.current_value {
                match u32::from_str_radix(value, 16) {
                    Ok(c) if c <= 0xffffff => c,
                    _ => return Ok(None),
                }
            } else {
                base_color
            };
            let style = TextStyle {
                family: family.to_owned(),
                size: (size
                    * pixels_per_unit
                    * if span.format.profile_size_allowed() {
                        metrics.size_ratio
                    } else {
                        1.0
                    }) as f32,
                horizontal: (adjustment.effective_horizontal_scale_ratio()
                    * span.format.render_layout.horizontal) as f32,
                vertical: (adjustment.effective_vertical_scale_ratio()
                    * span.format.render_layout.vertical) as f32,
                tracking: (span
                    .format
                    .render_layout
                    .tracking
                    .unwrap_or(request.spacing)
                    + metrics.tracking_adjust_em * size) as f32,
                baseline_shift: (-metrics.baseline_shift_em * size
                    + span.format.render_layout.offset_y) as f32,
                color,
                bold: flags.map_or(request.bold, |s| s.bold),
                italic: flags.map_or(request.italic, |s| s.italic),
                disable_ligatures: adjustment.effective_disable_ligatures()
                    || span.format.render_layout.disable_ligatures,
            };
            if !style.valid() {
                return Ok(None);
            }
            plan.push(cluster, style);
        }
    }
    Ok(Some(plan))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font_coverage::UnknownGlyphCoverage;
    fn request(text: &str) -> RenderRequest {
        RenderRequest {
            text: text.into(),
            profile: None,
            font: "Yu Gothic UI".into(),
            size: 64.0,
            spacing: 0.0,
            line_spacing: 0.0,
            color: "ffffff".into(),
            align: 0,
            bold: false,
            italic: false,
            disable_ligatures: false,
        }
    }
    #[test]
    fn native_sizes_use_pixels_for_defaults_and_inline_controls() {
        let req = request("A<s32>😀<s>B");
        let plan = prepare_native(&req, &req.text, &mut UnknownGlyphCoverage)
            .unwrap()
            .unwrap();
        assert_eq!(
            plan.ranges.iter().map(|r| r.style.size).collect::<Vec<_>>(),
            [64.0, 32.0, 64.0]
        );
        let max = RenderRequest {
            size: 4096.0,
            ..request("😀")
        };
        assert_eq!(
            prepare_native(&max, &max.text, &mut UnknownGlyphCoverage)
                .unwrap()
                .unwrap()
                .ranges[0]
                .style
                .size,
            4096.0
        );
    }

    #[test]
    fn ligature_switch_changes_cache_key_even_without_decoration() {
        let normal = RenderRequest {
            profile: Some(CompositeFontProfile::neutral_default()),
            ..request("ffi")
        };
        let forced = RenderRequest {
            disable_ligatures: true,
            ..normal.clone()
        };
        assert!(normal != forced);
        let plan = prepare(&forced, "ffi", &mut UnknownGlyphCoverage)
            .unwrap()
            .unwrap();
        assert!(!plan.modified);
        assert!(layout_options(&forced, None).disable_ligatures);
        assert!(!layout_options(&normal, None).disable_ligatures);
        assert!(!layout_options(&normal, None).color_glyphs);
        assert_eq!(plan.utf16, "ffi".encode_utf16().collect::<Vec<_>>());
    }

    #[test]
    fn preview_respects_each_profile_parameter_toggle() {
        let mut profile = CompositeFontProfile::neutral_default();
        profile.western.size_ratio = 1.5;
        profile.western.size_enabled = false;
        let disabled_request = RenderRequest {
            profile: Some(profile.clone()),
            ..request("A")
        };
        let plan = prepare(&disabled_request, "A", &mut UnknownGlyphCoverage)
            .unwrap()
            .unwrap();
        assert!(!plan.modified);

        profile.western.size_enabled = true;
        let enabled_request = RenderRequest {
            profile: Some(profile),
            ..request("A")
        };
        let plan = prepare(&enabled_request, "A", &mut UnknownGlyphCoverage)
            .unwrap()
            .unwrap();
        assert!(plan.modified);
    }

    #[test]
    fn preview_inherits_the_object_font_and_applies_enabled_metrics() {
        let mut profile = CompositeFontProfile::neutral_default();
        profile.western.size_ratio = 1.5;
        profile.western.baseline_shift_em = 0.25;
        profile.western.tracking_adjust_em = 0.125;
        profile.western.vertical_scale_ratio = 0.8;
        profile.western.horizontal_scale_ratio = 1.2;
        profile.western_fallbacks = vec![compositefont_core::FontAdjustment {
            font_family: "Fallback".into(),
            ..compositefont_core::FontAdjustment::neutral()
        }];
        for enabled in [false, true] {
            profile.western.size_enabled = enabled;
            let request = RenderRequest {
                profile: Some(profile.clone()),
                ..request("A")
            };
            let plan = prepare(&request, "A", &mut UnknownGlyphCoverage)
                .unwrap()
                .unwrap();
            assert!(plan.modified);
            let style = &plan.ranges[0].style;
            assert_eq!(style.family, request.font);
            assert_eq!(style.size, if enabled { 96.0 } else { 64.0 });
            assert_eq!(style.baseline_shift, -8.0);
            assert_eq!(style.tracking, 4.0);
            assert_eq!(style.vertical, 0.8);
            assert_eq!(style.horizontal, 1.2);
        }
    }

    #[test]
    fn alias_decode_preserves_literal_backslashes() {
        assert_eq!(decode_alias(r"日本語\nA\\nB\tC"), "日本語\nA\\nB\tC");
    }
    #[test]
    fn scripts_and_clusters_reach_the_text_layout_without_scalar_filtering() {
        for text in [
            "ଳା",
            "ଳି",
            "କ୍ଳ",
            "कि",
            "क्ष",
            "لا",
            "العربية 123",
            "กิ",
            "កា",
            "e\u{301}",
            "か\u{3099}",
            "한",
            "👩\u{200d}💻",
        ] {
            let plan = prepare(&request(text), text, &mut UnknownGlyphCoverage)
                .unwrap()
                .unwrap();
            assert_eq!(
                String::from_utf16(&plan.utf16).unwrap(),
                icu_normalizer::ComposingNormalizer::new_nfc().normalize(text)
            );
            assert_eq!(plan.ranges.len(), 1, "{text}");
        }
    }
    #[test]
    fn opt_in_layout_accepts_object_sizes_above_512() {
        for size in [512.0, 513.0, 768.0, 1024.0, 2048.0] {
            let input = RenderRequest {
                size,
                ..request("ffi")
            };
            let plan = prepare(&input, "ffi", &mut UnknownGlyphCoverage)
                .unwrap()
                .unwrap();
            assert_eq!(plan.ranges.len(), 1, "size {size}");
            assert_eq!(plan.ranges[0].style.size, size as f32);
            assert!(plan.ranges[0].style.valid());
        }
        let mut profile = CompositeFontProfile::neutral_default();
        profile.western.size_ratio = 1.2;
        let input = RenderRequest {
            size: 1024.0,
            profile: Some(profile),
            ..request("ffi")
        };
        let plan = prepare(&input, "ffi", &mut UnknownGlyphCoverage)
            .unwrap()
            .unwrap();
        assert!(plan.ranges[0].style.size > 1024.0);
        assert!(plan.ranges[0].style.valid());
    }
    #[test]
    fn equivalent_tagged_ranges_merge_and_use_visible_utf16_offsets() {
        let text = "<@Segoe UI>e\u{301}<@><@Segoe UI>𠮷fi<@>";
        let plan = prepare(&request(text), text, &mut UnknownGlyphCoverage)
            .unwrap()
            .unwrap();
        assert_eq!(String::from_utf16(&plan.utf16).unwrap(), "é𠮷fi");
        assert_eq!(plan.ranges.len(), 1);
        assert_eq!(plan.ranges[0].utf16, 0..5);
    }
    #[test]
    fn unsupported_object_controls_still_keep_host_rendering() {
        for text in ["<?print('x')?>A", "A\tB", "<unknown>A"] {
            assert!(
                prepare(&request(text), text, &mut UnknownGlyphCoverage)
                    .unwrap()
                    .is_none()
            );
        }
    }
}
