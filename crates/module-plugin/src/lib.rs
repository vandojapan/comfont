mod font_coverage;

use aviutl2::module::{IntoScriptModuleReturnValue, ScriptModule, ScriptModuleFunctions};
use compositefont_core::{
    CompositeFontProfile, DEFAULT_PROFILE_NAME, FontAdjustment, PROFILE_SCHEMA_VERSION,
    ProfileDocument, ProfileStore, ResolvedFont, classify_character, codepoint_to_char,
    single_codepoint,
};
use font_coverage::{FontManagerGlyphCoverage, GlyphCoverage};
#[cfg(test)]
use font_coverage::{FontVerticalMetrics, UnknownGlyphCoverage};
use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, SystemTime},
};

const API_VERSION: i32 = 9;
// AviUtl2 rasterizes text size at twice the value reported by obj.getfont(),
// while gw and the resulting object bounds use pixel units.
const TEXT_RASTER_SIZE_SCALE: f64 = 2.0;
#[cfg(any(debug_assertions, test))]
const INTEGRATION_TEST_PROFILE: &str = "integration-test";
#[cfg(any(debug_assertions, test))]
const CLIPPING_TEST_PROFILE: &str = "clipping-test";

#[cfg(any(debug_assertions, test))]
fn integration_test_profile() -> CompositeFontProfile {
    let western = FontAdjustment {
        font_family: "Arial Black".to_owned(),
        fallback_font_families: Vec::new(),
        size_ratio: 1.0,
        baseline_shift_em: 0.0,
        tracking_adjust_em: 0.05,
        ..FontAdjustment::neutral()
    };
    let mut western = western;
    western.vertical_scale_ratio = 1.1;
    western.horizontal_scale_ratio = 0.9;
    let japanese = FontAdjustment {
        font_family: "游明朝".to_owned(),
        fallback_font_families: Vec::new(),
        size_ratio: 1.0,
        baseline_shift_em: 0.0,
        tracking_adjust_em: 0.1,
        ..FontAdjustment::neutral()
    };
    let mut japanese = japanese;
    japanese.vertical_scale_ratio = 0.9;
    japanese.horizontal_scale_ratio = 1.1;
    let mut kanji = japanese.clone();
    kanji.font_family = "Arial".to_owned();
    let mut adobe_kanji = japanese.clone();
    adobe_kanji.font_family = "A P-OTF UD新ゴ Pr6N B".to_owned();

    CompositeFontProfile {
        name: INTEGRATION_TEST_PROFILE.to_owned(),
        western: western.clone(),
        western_fallbacks: Vec::new(),
        hiragana: japanese.clone(),
        hiragana_fallbacks: Vec::new(),
        katakana: japanese.clone(),
        katakana_fallbacks: Vec::new(),
        kanji,
        kanji_fallbacks: vec![adobe_kanji, japanese.clone()],
        digit: western,
        digit_fallbacks: Vec::new(),
        symbol: japanese.clone(),
        symbol_fallbacks: Vec::new(),
        other: japanese,
        other_fallbacks: Vec::new(),
    }
}

#[cfg(any(debug_assertions, test))]
fn clipping_test_profile() -> CompositeFontProfile {
    let ocrb = FontAdjustment {
        font_family: "OCRB".to_owned(),
        ..FontAdjustment::neutral()
    };
    let biz_ud_gothic = FontAdjustment {
        font_family: "BIZ UDゴシック".to_owned(),
        ..FontAdjustment::neutral()
    };
    CompositeFontProfile {
        name: CLIPPING_TEST_PROFILE.to_owned(),
        western: ocrb.clone(),
        western_fallbacks: Vec::new(),
        hiragana: biz_ud_gothic.clone(),
        hiragana_fallbacks: Vec::new(),
        katakana: biz_ud_gothic.clone(),
        katakana_fallbacks: Vec::new(),
        kanji: biz_ud_gothic,
        kanji_fallbacks: Vec::new(),
        digit: ocrb,
        digit_fallbacks: Vec::new(),
        symbol: FontAdjustment::neutral(),
        symbol_fallbacks: Vec::new(),
        other: FontAdjustment::neutral(),
        other_fallbacks: Vec::new(),
    }
}

#[cfg(debug_assertions)]
fn builtin_profiles() -> Vec<CompositeFontProfile> {
    vec![
        CompositeFontProfile::neutral_default(),
        integration_test_profile(),
        clipping_test_profile(),
    ]
}

#[cfg(not(debug_assertions))]
fn builtin_profiles() -> Vec<CompositeFontProfile> {
    vec![CompositeFontProfile::neutral_default()]
}

fn profile_path() -> PathBuf {
    aviutl2::config::app_data_path()
        .join("compositefont")
        .join("profiles.json")
}

fn load_profile_store(path: &Path) -> Result<ProfileStore, String> {
    let profiles = match std::fs::read(path) {
        Ok(bytes) => {
            let document: ProfileDocument = serde_json::from_slice(&bytes)
                .map_err(|error| format!("{}の形式が不正です: {error}", path.display()))?;
            if document.schema_version != PROFILE_SCHEMA_VERSION {
                return Err(format!(
                    "{}のschema_version {}には対応していません",
                    path.display(),
                    document.schema_version
                ));
            }
            if document.profiles.is_empty() {
                return Err(format!("{}にプロファイルがありません", path.display()));
            }
            if !document
                .profiles
                .iter()
                .any(|profile| profile.name == DEFAULT_PROFILE_NAME)
            {
                return Err(format!(
                    "{}にdefaultプロファイルがありません",
                    path.display()
                ));
            }
            document.profiles
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => builtin_profiles(),
        Err(error) => return Err(format!("{}を読み込めません: {error}", path.display())),
    };

    #[cfg(debug_assertions)]
    let profiles = {
        let mut profiles = profiles;
        profiles.retain(|profile| {
            !matches!(
                profile.name.as_str(),
                INTEGRATION_TEST_PROFILE | CLIPPING_TEST_PROFILE
            )
        });
        profiles.push(integration_test_profile());
        profiles.push(clipping_test_profile());
        profiles
    };
    Ok(ProfileStore::from_profiles(profiles))
}

struct ReloadableProfiles {
    store: ProfileStore,
    path: PathBuf,
    observed_modified: Option<SystemTime>,
    last_checked: Instant,
}

impl ReloadableProfiles {
    fn new(path: PathBuf) -> Self {
        let observed_modified = modified_time(&path);
        let store = load_profile_store(&path).unwrap_or_else(|error| {
            aviutl2::logger::write_error_log(&error).ok();
            ProfileStore::from_profiles(builtin_profiles())
        });
        Self {
            store,
            path,
            observed_modified,
            last_checked: Instant::now(),
        }
    }

    fn resolve(
        &mut self,
        codepoint: char,
        profile_name: Option<&str>,
        base_font_size: Option<f64>,
        coverage: &mut dyn GlyphCoverage,
    ) -> Result<ResolvedFont, compositefont_core::ResolveError> {
        self.refresh_if_changed();
        let profile = self.store.profile(profile_name)?;
        let mut resolved = profile.resolve(codepoint);
        let (adjustment, candidate_index) =
            resolved_adjustment_with_index(profile, codepoint, coverage);
        let metrics = adjustment
            .effective_metrics(base_font_size)
            .unwrap_or_default();
        resolved.font_family = adjustment.font_family;
        resolved.size_ratio = metrics.size_ratio;
        resolved.baseline_shift_em = metrics.baseline_shift_em;
        resolved.tracking_adjust_em = metrics.tracking_adjust_em;
        resolved.vertical_scale_ratio = adjustment.vertical_scale_ratio;
        resolved.horizontal_scale_ratio = adjustment.horizontal_scale_ratio;
        if candidate_index > 0 {
            resolved.rule_id = format!("{}:fallback:{candidate_index}", resolved.rule_id);
        }
        Ok(resolved)
    }

    fn decorate(
        &mut self,
        text: &str,
        profile_name: Option<&str>,
        base_font_size: Option<f64>,
        base_char_spacing: Option<f64>,
        coverage: &mut dyn GlyphCoverage,
    ) -> String {
        self.refresh_if_changed();
        let Ok(profile) = self.store.profile(profile_name) else {
            return text.to_owned();
        };
        decorate_plain_text_with_coverage(
            text,
            profile,
            base_font_size,
            base_char_spacing,
            coverage,
        )
    }

    fn decorate_layout(
        &mut self,
        text: &str,
        profile_name: Option<&str>,
        base_font_size: Option<f64>,
        base_char_spacing: Option<f64>,
        base_font_family: Option<&str>,
        coverage: &mut dyn GlyphCoverage,
    ) -> DecoratedTextLayout {
        self.refresh_if_changed();
        let Ok(profile) = self.store.profile(profile_name) else {
            return DecoratedTextLayout::unchanged(text);
        };
        decorate_plain_text_layout_with_coverage(
            text,
            profile,
            base_font_size,
            base_char_spacing,
            base_font_family,
            coverage,
        )
    }

    fn refresh_if_changed(&mut self) {
        if self.last_checked.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_checked = Instant::now();
        let modified = modified_time(&self.path);
        if modified == self.observed_modified {
            return;
        }
        self.observed_modified = modified;
        match load_profile_store(&self.path) {
            Ok(store) => self.store = store,
            Err(error) => {
                aviutl2::logger::write_error_log(&error).ok();
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextRunRange {
    utf8: Range<usize>,
    utf16: Range<usize>,
}

#[derive(Clone, Debug, PartialEq)]
struct ResolvedTextRun {
    range: TextRunRange,
    adjustment: Option<FontAdjustment>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct BaseTextFormat {
    font_size: Option<f64>,
    char_spacing: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
struct LayoutGlyphOverride {
    utf8: Range<usize>,
    size_factor: f64,
    tracking_px: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
struct DecoratedTextLayout {
    text: String,
    object_offset_y: f64,
}

impl DecoratedTextLayout {
    fn unchanged(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            object_offset_y: 0.0,
        }
    }
}

trait TextRunBackend {
    type Output;

    /// `None` means that this backend cannot represent the resolved runs.
    fn apply(
        &self,
        source: &str,
        runs: &[ResolvedTextRun],
        base_format: BaseTextFormat,
        layout_overrides: &[LayoutGlyphOverride],
    ) -> Option<Self::Output>;
}

struct ControlTagBackend;

#[cfg(test)]
fn decorate_plain_text(
    text: &str,
    profile: &CompositeFontProfile,
    base_font_size: Option<f64>,
    base_char_spacing: Option<f64>,
) -> String {
    let mut coverage = UnknownGlyphCoverage;
    decorate_plain_text_with_coverage(
        text,
        profile,
        base_font_size,
        base_char_spacing,
        &mut coverage,
    )
}

fn decorate_plain_text_with_coverage(
    text: &str,
    profile: &CompositeFontProfile,
    base_font_size: Option<f64>,
    base_char_spacing: Option<f64>,
    coverage: &mut dyn GlyphCoverage,
) -> String {
    // PSDToolKit2のmodifierで字幕を壊さないことを優先する初期実装。
    // 制御文字、ルビ、コメント、スクリプト、未知タグを区別せずfail-openする。
    if text.contains('<') || text.is_empty() {
        return text.to_owned();
    }

    let runs = resolve_text_runs(text, profile, coverage);
    let base_format = BaseTextFormat {
        font_size: base_font_size.filter(|value| value.is_finite() && *value > 0.0),
        char_spacing: base_char_spacing.filter(|value| value.is_finite()),
    };
    ControlTagBackend
        .apply(text, &runs, base_format, &[])
        .unwrap_or_else(|| text.to_owned())
}

fn decorate_plain_text_layout_with_coverage(
    text: &str,
    profile: &CompositeFontProfile,
    base_font_size: Option<f64>,
    base_char_spacing: Option<f64>,
    base_font_family: Option<&str>,
    coverage: &mut dyn GlyphCoverage,
) -> DecoratedTextLayout {
    // Existing control text is opaque to this backend, so both decoration and
    // the corresponding object-coordinate correction must fail open together.
    if text.contains('<') || text.is_empty() {
        return DecoratedTextLayout::unchanged(text);
    }

    let runs = resolve_text_runs(text, profile, coverage);
    let font_size = base_font_size.filter(|value| value.is_finite() && *value > 0.0);
    let char_spacing = base_char_spacing.filter(|value| value.is_finite());
    let layout_overrides = layout_glyph_overrides(
        text,
        &runs,
        font_size,
        char_spacing,
        base_font_family.filter(|family| !family.is_empty()),
        coverage,
    );
    let base_format = BaseTextFormat {
        font_size,
        char_spacing,
    };
    let Some(text) = ControlTagBackend.apply(text, &runs, base_format, &layout_overrides) else {
        return DecoratedTextLayout::unchanged(text);
    };
    DecoratedTextLayout {
        text,
        object_offset_y: 0.0,
    }
}

fn resolve_text_runs(
    text: &str,
    profile: &CompositeFontProfile,
    coverage: &mut dyn GlyphCoverage,
) -> Vec<ResolvedTextRun> {
    let mut runs = Vec::new();
    let mut current_utf8_start = 0;
    let mut current_utf16_start = 0;
    let mut utf16_index = 0;
    let mut current_adjustment: Option<FontAdjustment> = None;

    for (index, character) in text.char_indices() {
        let utf16_end = utf16_index + character.len_utf16();
        if matches!(character, '\r' | '\n') {
            if let Some(adjustment) = current_adjustment.take() {
                runs.push(ResolvedTextRun {
                    range: TextRunRange {
                        utf8: current_utf8_start..index,
                        utf16: current_utf16_start..utf16_index,
                    },
                    adjustment: Some(adjustment),
                });
            }
            let end = index + character.len_utf8();
            runs.push(ResolvedTextRun {
                range: TextRunRange {
                    utf8: index..end,
                    utf16: utf16_index..utf16_end,
                },
                adjustment: None,
            });
            current_utf8_start = end;
            current_utf16_start = utf16_end;
            utf16_index = utf16_end;
            continue;
        }

        let adjustment = resolved_adjustment(profile, character, coverage);

        match current_adjustment {
            Some(ref current) if current != &adjustment => {
                runs.push(ResolvedTextRun {
                    range: TextRunRange {
                        utf8: current_utf8_start..index,
                        utf16: current_utf16_start..utf16_index,
                    },
                    adjustment: current_adjustment.take(),
                });
                current_utf8_start = index;
                current_utf16_start = utf16_index;
                current_adjustment = Some(adjustment);
            }
            None => current_adjustment = Some(adjustment),
            Some(_) => {}
        }
        utf16_index = utf16_end;
    }

    if let Some(adjustment) = current_adjustment {
        runs.push(ResolvedTextRun {
            range: TextRunRange {
                utf8: current_utf8_start..text.len(),
                utf16: current_utf16_start..utf16_index,
            },
            adjustment: Some(adjustment),
        });
    }

    runs
}

#[derive(Clone, Debug)]
struct LineLayoutCharacter<'run> {
    utf8: Range<usize>,
    character: char,
    adjustment: &'run FontAdjustment,
}

fn layout_glyph_overrides(
    source: &str,
    runs: &[ResolvedTextRun],
    base_font_size: Option<f64>,
    base_char_spacing: Option<f64>,
    base_font_family: Option<&str>,
    coverage: &mut dyn GlyphCoverage,
) -> Vec<LayoutGlyphOverride> {
    let (Some(base_font_size), Some(base_char_spacing)) = (base_font_size, base_char_spacing)
    else {
        return Vec::new();
    };
    let raster_font_size = base_font_size * TEXT_RASTER_SIZE_SCALE;
    if !raster_font_size.is_finite() {
        return Vec::new();
    }

    let mut overrides = Vec::new();
    let mut line = Vec::new();

    for run in runs {
        let Some(adjustment) = run.adjustment.as_ref() else {
            append_line_layout_overrides(
                &line,
                base_font_size,
                raster_font_size,
                base_char_spacing,
                base_font_family,
                coverage,
                &mut overrides,
            );
            line.clear();
            continue;
        };
        for (relative_start, character) in source[run.range.utf8.clone()].char_indices() {
            if character.is_control() {
                continue;
            }
            let start = run.range.utf8.start + relative_start;
            line.push(LineLayoutCharacter {
                utf8: start..start + character.len_utf8(),
                character,
                adjustment,
            });
        }
    }
    append_line_layout_overrides(
        &line,
        base_font_size,
        raster_font_size,
        base_char_spacing,
        base_font_family,
        coverage,
        &mut overrides,
    );

    overrides
}

fn append_line_layout_overrides(
    line: &[LineLayoutCharacter<'_>],
    base_font_size: f64,
    raster_font_size: f64,
    base_char_spacing: f64,
    base_font_family: Option<&str>,
    coverage: &mut dyn GlyphCoverage,
    output: &mut Vec<LayoutGlyphOverride>,
) {
    let (Some(first), Some(second)) = (line.first(), line.get(1)) else {
        return;
    };
    let Some(line_top_capacity) = run_line_top_capacity(
        first.adjustment,
        base_font_size,
        raster_font_size,
        base_font_family,
        coverage,
    ) else {
        return;
    };
    if line_top_capacity <= 0.0 {
        return;
    }

    let maximum_top_extent = line
        .iter()
        .filter(|item| is_visible_layout_character(item.character))
        .filter_map(|item| {
            glyph_top_extent(
                item.character,
                item.adjustment,
                base_font_size,
                raster_font_size,
                base_font_family,
                coverage,
            )
        })
        .fold(0.0_f64, f64::max);
    let overflow = maximum_top_extent - line_top_capacity;
    if !overflow.is_finite() || overflow <= 0.0 {
        return;
    }

    // AviUtl2 rounds text positions while rasterizing. Reserve two whole
    // pixels beyond the rounded-up ink overflow so antialiasing cannot touch
    // the inclusive top edge.
    let padding = overflow.ceil() + 2.0;
    let size_factor = 1.0 + padding / line_top_capacity;
    if !size_factor.is_finite() || size_factor <= 1.0 {
        return;
    }

    let Some(first_font_family) = adjustment_font_family(first.adjustment, base_font_family) else {
        return;
    };
    // Fragmenting a proportional run can change kerning, ligatures, or script
    // shaping. Limit this workaround to fonts whose advances are independent.
    if coverage.is_monospaced_font(first_font_family) != Some(true) {
        return;
    }
    let Some(first_advance_em) = coverage.glyph_advance_em(first_font_family, first.character)
    else {
        return;
    };
    let Some(second_font_family) = adjustment_font_family(second.adjustment, base_font_family)
    else {
        return;
    };
    let Some(second_advance_em) = coverage.glyph_advance_em(second_font_family, second.character)
    else {
        return;
    };
    if first_advance_em <= 0.0 || second_advance_em <= 0.0 {
        return;
    }
    let Some(first_metrics) = first.adjustment.effective_metrics(Some(base_font_size)) else {
        return;
    };
    let Some(second_metrics) = second.adjustment.effective_metrics(Some(base_font_size)) else {
        return;
    };
    let extra_advance =
        first_advance_em * raster_font_size * first_metrics.size_ratio * (size_factor - 1.0);
    let tracking_px =
        base_char_spacing + base_font_size * second_metrics.tracking_adjust_em - extra_advance;
    if !extra_advance.is_finite() || !tracking_px.is_finite() {
        return;
    }

    // Only the first glyph supplies the taller line metrics. Inverse tw/th
    // scaling preserves its visible size, and the next glyph's gw value
    // absorbs the one enlarged advance without moving the remaining text.
    output.push(LayoutGlyphOverride {
        utf8: first.utf8.clone(),
        size_factor,
        tracking_px: None,
    });
    output.push(LayoutGlyphOverride {
        utf8: second.utf8.clone(),
        size_factor: 1.0,
        tracking_px: Some(tracking_px),
    });
}

fn run_line_top_capacity(
    adjustment: &FontAdjustment,
    base_font_size: f64,
    raster_font_size: f64,
    base_font_family: Option<&str>,
    coverage: &mut dyn GlyphCoverage,
) -> Option<f64> {
    let font_family = if adjustment.font_family.is_empty() {
        base_font_family?
    } else {
        &adjustment.font_family
    };
    let font_metrics = coverage.vertical_metrics(font_family)?;
    let metrics = adjustment.effective_metrics(Some(base_font_size))?;
    // AviUtl2 derives the line box from the selected font size. tw/th and p
    // alter glyph rendering, but do not add pre-rasterization line padding.
    let top_extent = font_metrics.ascent_em * raster_font_size * metrics.size_ratio;
    top_extent.is_finite().then_some(top_extent)
}

fn glyph_top_extent(
    character: char,
    adjustment: &FontAdjustment,
    base_font_size: f64,
    raster_font_size: f64,
    base_font_family: Option<&str>,
    coverage: &mut dyn GlyphCoverage,
) -> Option<f64> {
    let font_family = if adjustment.font_family.is_empty() {
        base_font_family?
    } else {
        &adjustment.font_family
    };
    let metrics = adjustment.effective_metrics(Some(base_font_size))?;
    let glyph_top_em = coverage.glyph_top_em(font_family, character)?;
    let scaled_top =
        glyph_top_em * raster_font_size * metrics.size_ratio * adjustment.vertical_scale_ratio;
    let top_extent = scaled_top + base_font_size * metrics.baseline_shift_em;
    top_extent.is_finite().then_some(top_extent)
}

fn adjustment_font_family<'a>(
    adjustment: &'a FontAdjustment,
    base_font_family: Option<&'a str>,
) -> Option<&'a str> {
    if adjustment.font_family.is_empty() {
        base_font_family
    } else {
        Some(&adjustment.font_family)
    }
}

fn is_visible_layout_character(character: char) -> bool {
    !character.is_whitespace() && !character.is_control()
}

fn resolved_adjustment(
    profile: &CompositeFontProfile,
    character: char,
    coverage: &mut dyn GlyphCoverage,
) -> FontAdjustment {
    resolved_adjustment_with_index(profile, character, coverage).0
}

fn resolved_adjustment_with_index(
    profile: &CompositeFontProfile,
    character: char,
    coverage: &mut dyn GlyphCoverage,
) -> (FontAdjustment, usize) {
    let class = classify_character(character);

    let mut last = (FontAdjustment::neutral(), 0);
    let mut first_unknown = None;
    for (index, adjustment) in adjustments_for_class(profile, class)
        .into_iter()
        .enumerate()
    {
        if adjustment.font_family.is_empty() {
            continue;
        }
        let result = coverage.has_glyph(&adjustment.font_family, character);
        last = (adjustment, index);
        match result {
            Some(true) => return last,
            Some(false) => {}
            None if first_unknown.is_none() => first_unknown = Some(last.clone()),
            None => {}
        }
    }
    first_unknown.unwrap_or(last)
}

fn adjustments_for_class(
    profile: &CompositeFontProfile,
    class: compositefont_core::CharacterClass,
) -> Vec<FontAdjustment> {
    let mut primary = profile.adjustment_for(class).clone();
    let legacy = std::mem::take(&mut primary.fallback_font_families);
    let fallbacks = profile.fallbacks_for(class);
    let mut candidates = Vec::with_capacity(1 + fallbacks.len() + legacy.len());
    candidates.push(primary.clone());
    candidates.extend(fallbacks.iter().cloned().map(|mut adjustment| {
        adjustment.fallback_font_families.clear();
        adjustment
    }));
    candidates.extend(legacy.into_iter().map(|family| {
        let mut adjustment = primary.clone();
        adjustment.font_family = family;
        adjustment
    }));
    candidates
}

impl TextRunBackend for ControlTagBackend {
    type Output = String;

    fn apply(
        &self,
        source: &str,
        runs: &[ResolvedTextRun],
        base_format: BaseTextFormat,
        layout_overrides: &[LayoutGlyphOverride],
    ) -> Option<Self::Output> {
        if runs
            .iter()
            .filter_map(|run| run.adjustment.as_ref())
            .any(|adjustment| !is_safe_adjustment(adjustment))
        {
            return None;
        }
        let mut previous_end = 0;
        for item in layout_overrides {
            if item.utf8.start < previous_end
                || item.utf8.start >= item.utf8.end
                || item.utf8.end > source.len()
                || !source.is_char_boundary(item.utf8.start)
                || !source.is_char_boundary(item.utf8.end)
                || !item.size_factor.is_finite()
                || item.size_factor <= 0.0
                || item.tracking_px.is_some_and(|value| !value.is_finite())
            {
                return None;
            }
            previous_end = item.utf8.end;
        }

        let mut decorated = String::with_capacity(source.len() + runs.len() * 48);
        for run in runs {
            if run.adjustment.is_none() {
                decorated.push_str(&source[run.range.utf8.clone()]);
                continue;
            }
            write_control_tag_run(&mut decorated, source, run, base_format, layout_overrides);
        }
        Some(decorated)
    }
}

fn is_safe_adjustment(adjustment: &FontAdjustment) -> bool {
    let metric_values_are_safe = match adjustment.metric_unit {
        compositefont_core::MetricUnit::Percent => {
            adjustment.size_ratio.is_finite()
                && adjustment.size_ratio > 0.0
                && adjustment.baseline_shift_em.is_finite()
                && adjustment.tracking_adjust_em.is_finite()
        }
        compositefont_core::MetricUnit::Pixels => {
            adjustment
                .size_px
                .is_some_and(|value| value.is_finite() && value > 0.0)
                && adjustment.baseline_shift_px.is_some_and(f64::is_finite)
                && adjustment.tracking_adjust_px.is_some_and(f64::is_finite)
        }
    };
    !adjustment
        .font_family
        .chars()
        .any(|character| matches!(character, '<' | '>' | ',') || character.is_control())
        && metric_values_are_safe
        && adjustment.vertical_scale_ratio.is_finite()
        && adjustment.vertical_scale_ratio > 0.0
        && adjustment.horizontal_scale_ratio.is_finite()
        && adjustment.horizontal_scale_ratio > 0.0
}

fn write_control_tag_run(
    output: &mut String,
    source: &str,
    run: &ResolvedTextRun,
    base_format: BaseTextFormat,
    layout_overrides: &[LayoutGlyphOverride],
) {
    let Some(adjustment) = run.adjustment.as_ref() else {
        output.push_str(&source[run.range.utf8.clone()]);
        return;
    };
    let has_font = !adjustment.font_family.is_empty();
    if has_font {
        output.push_str("<@");
        output.push_str(&adjustment.font_family);
        output.push('>');
    }

    let mut cursor = run.range.utf8.start;
    for item in layout_overrides.iter().filter(|item| {
        item.utf8.start >= run.range.utf8.start && item.utf8.end <= run.range.utf8.end
    }) {
        if cursor < item.utf8.start {
            write_control_tag_fragment(
                output,
                source,
                cursor..item.utf8.start,
                adjustment,
                base_format,
                None,
            );
        }
        write_control_tag_fragment(
            output,
            source,
            item.utf8.clone(),
            adjustment,
            base_format,
            Some(item),
        );
        cursor = item.utf8.end;
    }
    if cursor < run.range.utf8.end {
        write_control_tag_fragment(
            output,
            source,
            cursor..run.range.utf8.end,
            adjustment,
            base_format,
            None,
        );
    }

    if has_font {
        output.push_str("<@>");
    }
}

fn write_control_tag_fragment(
    output: &mut String,
    source: &str,
    range: Range<usize>,
    adjustment: &FontAdjustment,
    base_format: BaseTextFormat,
    layout_override: Option<&LayoutGlyphOverride>,
) {
    if range.is_empty() {
        return;
    }
    let metrics = adjustment
        .effective_metrics(base_format.font_size)
        .unwrap_or_default();
    let size_factor = layout_override.map_or(1.0, |item| item.size_factor);
    let size_ratio = metrics.size_ratio * size_factor;
    let horizontal_scale_ratio = adjustment.horizontal_scale_ratio / size_factor;
    let vertical_scale_ratio = adjustment.vertical_scale_ratio / size_factor;
    let has_size = !approximately_equal(size_ratio, 1.0);
    let tracking_px = layout_override
        .and_then(|item| item.tracking_px)
        .or_else(|| {
            (base_format.font_size.is_some()
                && base_format.char_spacing.is_some()
                && !approximately_equal(metrics.tracking_adjust_em, 0.0))
            .then(|| {
                base_format.char_spacing.unwrap_or_default()
                    + base_format.font_size.unwrap_or_default() * metrics.tracking_adjust_em
            })
        });
    let has_horizontal = !approximately_equal(horizontal_scale_ratio, 1.0);
    let has_vertical = !approximately_equal(vertical_scale_ratio, 1.0);
    let baseline_shift = base_format.font_size.unwrap_or_default() * metrics.baseline_shift_em;
    let has_baseline =
        base_format.font_size.is_some() && !approximately_equal(metrics.baseline_shift_em, 0.0);

    if has_size {
        output.push_str("<s*");
        output.push_str(&format_control_number(size_ratio));
        output.push('>');
    }
    if let Some(tracking_px) = tracking_px {
        output.push_str("<gw");
        output.push_str(&format_control_number(tracking_px));
        output.push('>');
    }
    if has_horizontal {
        output.push_str("<tw");
        output.push_str(&format_control_number(horizontal_scale_ratio));
        output.push('>');
    }
    if has_vertical {
        output.push_str("<th");
        output.push_str(&format_control_number(vertical_scale_ratio));
        output.push('>');
    }
    if has_baseline {
        write_relative_position_tag(output, -baseline_shift);
    }

    output.push_str(&source[range]);

    if has_baseline {
        output.push_str("<p>");
    }
    if has_vertical {
        output.push_str("<th>");
    }
    if has_horizontal {
        output.push_str("<tw>");
    }
    if tracking_px.is_some() {
        output.push_str("<gw>");
    }
    if has_size {
        output.push_str("<s>");
    }
}

fn write_relative_position_tag(output: &mut String, shift_y: f64) {
    output.push_str("<p+0,");
    if shift_y > 0.0 {
        output.push('+');
    }
    output.push_str(&format_control_number(shift_y));
    output.push('>');
}

fn approximately_equal(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-9
}

fn format_control_number(value: f64) -> String {
    let value = if value.abs() < 0.000_000_5 {
        0.0
    } else {
        value
    };
    let mut formatted = format!("{value:.6}");
    while formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
}

fn modified_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

/// Luaへ公開する多値返却。aviutl2依存をcoreへ持ち込まないための境界型。
struct ScriptResolvedFont(ResolvedFont);

impl From<ResolvedFont> for ScriptResolvedFont {
    fn from(value: ResolvedFont) -> Self {
        Self(value)
    }
}

impl IntoScriptModuleReturnValue for ScriptResolvedFont {
    type Err = anyhow::Error;

    fn into_return_values(
        self,
    ) -> Result<Vec<aviutl2::module::ScriptModuleReturnValue>, Self::Err> {
        let value = self.0;
        (
            value.font_family,
            value.size_ratio,
            value.baseline_shift_em,
            value.tracking_adjust_em,
            value.vertical_scale_ratio,
            value.horizontal_scale_ratio,
            value.category,
            value.rule_id,
        )
            .into_return_values()
    }
}

/// Luaから`compositefont`として参照されるスクリプトモジュール。
#[aviutl2::plugin(ScriptModule)]
pub struct CompositeFontModule {
    profiles: Mutex<ReloadableProfiles>,
}

impl ScriptModule for CompositeFontModule {
    fn new(_info: aviutl2::AviUtl2Info) -> aviutl2::AnyResult<Self> {
        Ok(Self {
            profiles: Mutex::new(ReloadableProfiles::new(profile_path())),
        })
    }

    fn plugin_info(&self) -> aviutl2::module::ScriptModuleTable {
        aviutl2::module::ScriptModuleTable {
            information: format!(
                "Composite Font script module for AviUtl2 / v{} / API v{API_VERSION}",
                env!("CARGO_PKG_VERSION")
            ),
            functions: Self::functions(),
        }
    }
}

#[aviutl2::module::functions]
impl CompositeFontModule {
    fn api_version() -> i32 {
        API_VERSION
    }

    fn resolve(
        &self,
        character: String,
        profile: Option<String>,
        base_font_size: Option<f64>,
        section: &aviutl2::generic::ReadSection,
    ) -> aviutl2::AnyResult<ScriptResolvedFont> {
        let codepoint = single_codepoint(&character)?;
        let mut coverage = FontManagerGlyphCoverage::new(section);
        let mut profiles = self
            .profiles
            .lock()
            .map_err(|_| anyhow::anyhow!("composite font profile lock was poisoned"))?;
        Ok(profiles
            .resolve(codepoint, profile.as_deref(), base_font_size, &mut coverage)?
            .into())
    }

    fn resolve_codepoint(
        &self,
        codepoint: u32,
        profile: Option<String>,
        base_font_size: Option<f64>,
        section: &aviutl2::generic::ReadSection,
    ) -> aviutl2::AnyResult<ScriptResolvedFont> {
        let codepoint = codepoint_to_char(codepoint)?;
        let mut coverage = FontManagerGlyphCoverage::new(section);
        let mut profiles = self
            .profiles
            .lock()
            .map_err(|_| anyhow::anyhow!("composite font profile lock was poisoned"))?;
        Ok(profiles
            .resolve(codepoint, profile.as_deref(), base_font_size, &mut coverage)?
            .into())
    }

    fn decorate(
        &self,
        text: String,
        profile: Option<String>,
        base_font_size: Option<f64>,
        base_char_spacing: Option<f64>,
        section: &aviutl2::generic::ReadSection,
    ) -> String {
        let mut coverage = FontManagerGlyphCoverage::new(section);
        let Ok(mut profiles) = self.profiles.lock() else {
            return text;
        };
        profiles.decorate(
            &text,
            profile.as_deref(),
            base_font_size,
            base_char_spacing,
            &mut coverage,
        )
    }

    fn decorate_layout(
        &self,
        text: String,
        profile: Option<String>,
        base_font_size: Option<f64>,
        base_char_spacing: Option<f64>,
        base_font_family: Option<String>,
        section: &aviutl2::generic::ReadSection,
    ) -> (String, f64) {
        let mut coverage = FontManagerGlyphCoverage::new(section);
        let Ok(mut profiles) = self.profiles.lock() else {
            return (text, 0.0);
        };
        let layout = profiles.decorate_layout(
            &text,
            profile.as_deref(),
            base_font_size,
            base_char_spacing,
            base_font_family.as_deref(),
            &mut coverage,
        );
        (layout.text, layout.object_offset_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct TestCoverage {
        values: HashMap<(String, char), bool>,
        vertical_metrics: HashMap<String, FontVerticalMetrics>,
        monospaced_fonts: HashMap<String, bool>,
        glyph_top_extents: HashMap<(String, char), f64>,
        glyph_advances: HashMap<(String, char), f64>,
    }

    impl GlyphCoverage for TestCoverage {
        fn has_glyph(&mut self, font_family: &str, character: char) -> Option<bool> {
            self.values
                .get(&(font_family.to_owned(), character))
                .copied()
        }

        fn vertical_metrics(&mut self, font_family: &str) -> Option<FontVerticalMetrics> {
            self.vertical_metrics.get(font_family).copied()
        }

        fn is_monospaced_font(&mut self, font_family: &str) -> Option<bool> {
            self.monospaced_fonts.get(font_family).copied()
        }

        fn glyph_top_em(&mut self, font_family: &str, character: char) -> Option<f64> {
            self.glyph_top_extents
                .get(&(font_family.to_owned(), character))
                .copied()
        }

        fn glyph_advance_em(&mut self, font_family: &str, character: char) -> Option<f64> {
            self.glyph_advances
                .get(&(font_family.to_owned(), character))
                .copied()
        }
    }

    fn adjustment(
        font_family: &str,
        size_ratio: f64,
        baseline_shift_em: f64,
        tracking_adjust_em: f64,
        vertical_scale_ratio: f64,
        horizontal_scale_ratio: f64,
    ) -> FontAdjustment {
        FontAdjustment {
            font_family: font_family.to_owned(),
            fallback_font_families: Vec::new(),
            size_ratio,
            baseline_shift_em,
            tracking_adjust_em,
            vertical_scale_ratio,
            horizontal_scale_ratio,
            ..FontAdjustment::neutral()
        }
    }

    fn decoration_profile() -> CompositeFontProfile {
        let latin = adjustment("Latin", 1.15, 0.02, 0.05, 0.9, 1.1);
        let japanese = adjustment("Japanese", 1.0, 0.0, 0.0, 1.0, 1.0);
        CompositeFontProfile {
            name: "subtitle".to_owned(),
            western: latin.clone(),
            western_fallbacks: Vec::new(),
            hiragana: japanese.clone(),
            hiragana_fallbacks: Vec::new(),
            katakana: japanese.clone(),
            katakana_fallbacks: Vec::new(),
            kanji: japanese.clone(),
            kanji_fallbacks: Vec::new(),
            digit: latin,
            digit_fallbacks: Vec::new(),
            symbol: japanese.clone(),
            symbol_fallbacks: Vec::new(),
            other: japanese,
            other_fallbacks: Vec::new(),
        }
    }

    fn font_metrics(ascent: u16, descent: u16, line_gap: i16) -> FontVerticalMetrics {
        FontVerticalMetrics {
            ascent_em: f64::from(ascent) / 2048.0,
            descent_em: f64::from(descent) / 2048.0,
            line_gap_em: f64::from(line_gap) / 2048.0,
        }
    }

    fn temporary_profile_path() -> PathBuf {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "compositefont-module-{}-{}.json",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn result_is_exported_as_eight_lua_values() {
        let resolved = ResolvedFont {
            font_family: "Inter".to_owned(),
            size_ratio: 0.95,
            baseline_shift_em: 0.02,
            tracking_adjust_em: -0.01,
            vertical_scale_ratio: 0.9,
            horizontal_scale_ratio: 1.1,
            category: "latin".to_owned(),
            rule_id: "latin-uppercase".to_owned(),
        };
        let values = ScriptResolvedFont(resolved).into_return_values().unwrap();

        assert_eq!(values.len(), 8);
        assert!(matches!(
            &values[0],
            aviutl2::module::ScriptModuleReturnValue::String(value) if value == "Inter"
        ));
        assert!(matches!(
            values[1],
            aviutl2::module::ScriptModuleReturnValue::Float(value) if value == 0.95
        ));
        assert!(matches!(
            values[4],
            aviutl2::module::ScriptModuleReturnValue::Float(value) if value == 0.9
        ));
        assert!(matches!(
            values[5],
            aviutl2::module::ScriptModuleReturnValue::Float(value) if value == 1.1
        ));
    }

    #[test]
    fn selects_first_font_with_the_requested_glyph() {
        let mut profile = decoration_profile();
        profile.kanji = adjustment("Std", 1.0, 0.0, 0.0, 1.0, 1.0);
        profile.kanji_fallbacks = vec![
            adjustment("Pr6", 1.05, 0.01, 0.02, 0.98, 1.02),
            adjustment("Last", 1.0, 0.0, 0.0, 1.0, 1.0),
        ];
        let mut coverage = TestCoverage::default();
        coverage.values.insert(("Std".to_owned(), '𠮷'), false);
        coverage.values.insert(("Pr6".to_owned(), '𠮷'), true);

        let (selected, index) = resolved_adjustment_with_index(&profile, '𠮷', &mut coverage);
        assert_eq!(selected.font_family, "Pr6");
        assert_eq!(selected.size_ratio, 1.05);
        assert_eq!(index, 1);
    }

    #[test]
    fn selects_fallbacks_for_non_kanji_character_classes() {
        let mut profile = decoration_profile();
        profile.hiragana = adjustment("Kana Std", 1.0, 0.0, 0.0, 1.0, 1.0);
        profile.hiragana_fallbacks =
            vec![adjustment("Kana Extended", 1.08, 0.01, 0.02, 0.95, 1.05)];
        let mut coverage = TestCoverage::default();
        coverage.values.insert(("Kana Std".to_owned(), 'あ'), false);
        coverage
            .values
            .insert(("Kana Extended".to_owned(), 'あ'), true);

        let (selected, index) = resolved_adjustment_with_index(&profile, 'あ', &mut coverage);
        assert_eq!(selected.font_family, "Kana Extended");
        assert_eq!(selected.size_ratio, 1.08);
        assert_eq!(selected.horizontal_scale_ratio, 1.05);
        assert_eq!(index, 1);
    }

    #[test]
    fn continues_after_an_unknown_font_when_a_later_font_has_the_glyph() {
        let mut profile = decoration_profile();
        profile.kanji = adjustment("Missing", 1.0, 0.0, 0.0, 1.0, 1.0);
        profile.kanji_fallbacks = vec![adjustment("Pr6", 1.0, 0.0, 0.0, 1.0, 1.0)];
        let mut coverage = TestCoverage::default();
        coverage.values.insert(("Pr6".to_owned(), '𠮷'), true);

        let (selected, index) = resolved_adjustment_with_index(&profile, '𠮷', &mut coverage);
        assert_eq!(selected.font_family, "Pr6");
        assert_eq!(index, 1);
    }

    #[test]
    fn decoration_splits_kanji_runs_at_fallback_boundaries() {
        let mut profile = decoration_profile();
        profile.kanji.font_family = "Std".to_owned();
        profile.kanji_fallbacks = vec![adjustment("Pr6", 1.0, 0.0, 0.0, 1.0, 1.0)];
        let mut coverage = TestCoverage::default();
        coverage.values.insert(("Std".to_owned(), '国'), true);
        coverage.values.insert(("Std".to_owned(), '𠮷'), false);
        coverage.values.insert(("Pr6".to_owned(), '𠮷'), true);

        assert_eq!(
            decorate_plain_text_with_coverage(
                "国𠮷国",
                &profile,
                Some(100.0),
                Some(0.0),
                &mut coverage,
            ),
            "<@Std>国<@><@Pr6>𠮷<@><@Std>国<@>"
        );
    }

    #[test]
    fn integration_profile_resolves_all_categories() {
        let profile = integration_test_profile();

        assert_eq!(profile.resolve('A').font_family, "Arial Black");
        assert_eq!(profile.resolve('A').tracking_adjust_em, 0.05);
        assert_eq!(profile.resolve('A').vertical_scale_ratio, 1.1);
        assert_eq!(profile.resolve('A').horizontal_scale_ratio, 0.9);
        assert_eq!(profile.resolve('あ').font_family, "游明朝");
        assert_eq!(profile.resolve('あ').tracking_adjust_em, 0.1);
        assert_eq!(profile.resolve('あ').vertical_scale_ratio, 0.9);
        assert_eq!(profile.resolve('あ').horizontal_scale_ratio, 1.1);
        assert_eq!(profile.resolve('カ').font_family, "游明朝");
        assert_eq!(profile.resolve('日').font_family, "Arial");
        assert_eq!(profile.kanji_fallbacks.len(), 2);
        assert_eq!(
            profile.kanji_fallbacks[0].font_family,
            "A P-OTF UD新ゴ Pr6N B"
        );
        assert_eq!(profile.resolve('1').font_family, "Arial Black");
        assert_eq!(profile.resolve('。').font_family, "游明朝");
        assert_eq!(profile.resolve('Ж').font_family, "游明朝");
    }

    #[test]
    fn loads_profiles_saved_by_the_editor() {
        let path = temporary_profile_path();
        let mut document = ProfileDocument::with_builtin_default();
        document.profiles[0].kanji.font_family = "Arial Black".to_owned();
        std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();

        let store = load_profile_store(&path).unwrap();
        assert_eq!(
            store.resolve('漢', Some("default")).unwrap().font_family,
            "Arial Black"
        );

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn empty_and_neutral_text_are_unchanged() {
        assert_eq!(
            decorate_plain_text("", &decoration_profile(), Some(100.0), Some(0.0)),
            ""
        );
        assert_eq!(
            decorate_plain_text(
                "国LINE123",
                &CompositeFontProfile::neutral_default(),
                Some(100.0),
                Some(0.0),
            ),
            "国LINE123"
        );
    }

    #[test]
    fn decorates_and_coalesces_adjacent_visual_runs() {
        let decorated =
            decorate_plain_text("国LINE123国", &decoration_profile(), Some(100.0), Some(3.0));

        assert_eq!(
            decorated,
            concat!(
                "<@Japanese>国<@>",
                "<@Latin><s*1.15><gw8><tw1.1><th0.9><p+0,-2>",
                "LINE123",
                "<p><th><tw><gw><s><@>",
                "<@Japanese>国<@>"
            )
        );
    }

    #[test]
    fn layout_decoration_prevents_a_taller_later_font_from_clipping() {
        let mut profile = CompositeFontProfile::neutral_default();
        profile.western.font_family = "OCRB".to_owned();
        profile.digit.font_family = "OCRB".to_owned();
        for adjustment in [
            &mut profile.hiragana,
            &mut profile.katakana,
            &mut profile.kanji,
        ] {
            adjustment.font_family = "BIZ UDゴシック".to_owned();
        }
        let mut coverage = TestCoverage::default();
        coverage
            .vertical_metrics
            .insert("OCRB".to_owned(), font_metrics(1446, 337, 1137));
        coverage.monospaced_fonts.insert("OCRB".to_owned(), true);
        coverage
            .vertical_metrics
            .insert("BIZ UDゴシック".to_owned(), font_metrics(1802, 246, 0));
        coverage
            .vertical_metrics
            .insert("Yu Gothic UI".to_owned(), font_metrics(2210, 514, 0));
        for character in "年月日はてんき晴れ".chars() {
            coverage
                .glyph_top_extents
                .insert(("BIZ UDゴシック".to_owned(), character), 1760.0 / 2048.0);
        }
        for character in "202679".chars() {
            coverage
                .glyph_top_extents
                .insert(("OCRB".to_owned(), character), 1446.0 / 2048.0);
        }
        coverage
            .glyph_advances
            .insert(("OCRB".to_owned(), '2'), 0.6);
        coverage
            .glyph_advances
            .insert(("OCRB".to_owned(), '0'), 0.6);
        coverage
            .glyph_top_extents
            .insert(("Yu Gothic UI".to_owned(), '、'), 500.0 / 2048.0);

        let layout = decorate_plain_text_layout_with_coverage(
            "2026年7月29日、てんきは晴れ",
            &profile,
            Some(40.0),
            Some(0.0),
            Some("Yu Gothic UI"),
            &mut coverage,
        );

        assert_eq!(layout.object_offset_y, 0.0);
        assert!(!layout.text.contains("<p"));
        assert!(layout.text.starts_with(concat!(
            "<@OCRB><s*1.26556><tw0.790164><th0.790164>",
            "2<th><tw><s><gw-12.746888>0<gw>26<@>"
        )));
        assert!(layout.text.contains("<@BIZ UDゴシック>年<@>"));
        assert!(layout.text.contains("<@>、<@BIZ UDゴシック>"));
    }

    #[test]
    fn layout_decoration_does_not_shift_when_the_tallest_font_is_first() {
        let mut profile = CompositeFontProfile::neutral_default();
        profile.kanji.font_family = "Tall".to_owned();
        profile.digit.font_family = "Short".to_owned();
        let mut coverage = TestCoverage::default();
        coverage
            .vertical_metrics
            .insert("Tall".to_owned(), font_metrics(1802, 246, 0));
        coverage
            .vertical_metrics
            .insert("Short".to_owned(), font_metrics(1446, 337, 1137));
        coverage
            .glyph_top_extents
            .insert(("Tall".to_owned(), '年'), 1802.0 / 2048.0);
        for character in "2026".chars() {
            coverage
                .glyph_top_extents
                .insert(("Short".to_owned(), character), 1446.0 / 2048.0);
        }

        let layout = decorate_plain_text_layout_with_coverage(
            "年2026",
            &profile,
            Some(81.0),
            Some(0.0),
            Some("Yu Gothic UI"),
            &mut coverage,
        );

        assert_eq!(layout.object_offset_y, 0.0);
        assert!(!layout.text.contains("<p"));
    }

    #[test]
    fn layout_decoration_does_not_fragment_a_proportional_first_font() {
        let mut profile = CompositeFontProfile::neutral_default();
        profile.western.font_family = "Proportional".to_owned();
        profile.kanji.font_family = "Tall".to_owned();
        let mut coverage = TestCoverage::default();
        for (font, ascent) in [("Proportional", 0.7), ("Tall", 0.9)] {
            coverage.vertical_metrics.insert(
                font.to_owned(),
                FontVerticalMetrics {
                    ascent_em: ascent,
                    descent_em: 0.2,
                    line_gap_em: 0.0,
                },
            );
        }
        coverage
            .monospaced_fonts
            .insert("Proportional".to_owned(), false);
        coverage
            .glyph_top_extents
            .insert(("Proportional".to_owned(), 'A'), 0.7);
        coverage
            .glyph_top_extents
            .insert(("Tall".to_owned(), '年'), 0.9);

        let layout = decorate_plain_text_layout_with_coverage(
            "A年",
            &profile,
            Some(100.0),
            Some(0.0),
            Some("Base"),
            &mut coverage,
        );

        assert_eq!(layout.object_offset_y, 0.0);
        assert_eq!(layout.text, "<@Proportional>A<@><@Tall>年<@>");
    }

    #[test]
    fn layout_decoration_keeps_run_baseline_shift_local() {
        let mut profile = CompositeFontProfile::neutral_default();
        profile.western = adjustment("Short", 1.0, 0.0, 0.0, 1.0, 1.0);
        profile.kanji = adjustment("Tall", 1.0, 0.05, 0.0, 1.0, 1.0);
        let mut coverage = TestCoverage::default();
        coverage.vertical_metrics.insert(
            "Short".to_owned(),
            FontVerticalMetrics {
                ascent_em: 0.7,
                descent_em: 0.2,
                line_gap_em: 0.0,
            },
        );
        coverage
            .glyph_top_extents
            .insert(("Short".to_owned(), 'A'), 0.7);
        coverage
            .glyph_top_extents
            .insert(("Tall".to_owned(), '年'), 0.9);
        coverage
            .glyph_advances
            .insert(("Short".to_owned(), 'A'), 0.5);
        coverage
            .glyph_advances
            .insert(("Tall".to_owned(), '年'), 1.0);
        coverage.monospaced_fonts.insert("Short".to_owned(), true);

        let layout = decorate_plain_text_layout_with_coverage(
            "A年",
            &profile,
            Some(100.0),
            Some(0.0),
            Some("Base"),
            &mut coverage,
        );

        assert_eq!(layout.object_offset_y, 0.0);
        assert!(
            layout
                .text
                .starts_with("<@Short><s*1.335714><tw0.748663><th0.748663>A<th><tw><s><@>")
        );
        assert!(
            layout
                .text
                .ends_with("<@Tall><gw-33.571429><p+0,-5>年<p><gw><@>")
        );
        assert_eq!(layout.text.matches("<p").count(), 2);
    }

    #[test]
    fn layout_decoration_adjusts_only_lines_that_need_more_top_space() {
        let mut profile = CompositeFontProfile::neutral_default();
        profile.kanji.font_family = "Tall".to_owned();
        let mut coverage = TestCoverage::default();
        for (font, ascent) in [("Base", 0.7), ("Tall", 0.9)] {
            coverage.vertical_metrics.insert(
                font.to_owned(),
                FontVerticalMetrics {
                    ascent_em: ascent,
                    descent_em: 0.2,
                    line_gap_em: 0.0,
                },
            );
        }
        coverage
            .glyph_top_extents
            .insert(("Base".to_owned(), 'A'), 0.7);
        coverage
            .glyph_top_extents
            .insert(("Tall".to_owned(), '年'), 0.9);
        coverage
            .glyph_advances
            .insert(("Base".to_owned(), 'A'), 0.5);
        coverage
            .glyph_advances
            .insert(("Tall".to_owned(), '年'), 1.0);
        coverage.monospaced_fonts.insert("Base".to_owned(), true);

        let layout = decorate_plain_text_layout_with_coverage(
            "A年\r\n年A",
            &profile,
            Some(100.0),
            Some(0.0),
            Some("Base"),
            &mut coverage,
        );

        assert_eq!(layout.object_offset_y, 0.0);
        assert!(layout.text.starts_with(concat!(
            "<s*1.3><tw0.769231><th0.769231>A<th><tw><s>",
            "<@Tall><gw-30>年<gw><@>\r\n<@Tall>年<@>A"
        )));
        assert!(!layout.text.contains("<p"));
    }

    #[test]
    fn layout_decoration_fails_open_with_zero_offset_for_control_text() {
        let mut coverage = TestCoverage::default();
        let layout = decorate_plain_text_layout_with_coverage(
            "<#red>2026<#>",
            &decoration_profile(),
            Some(81.0),
            Some(0.0),
            Some("Yu Gothic UI"),
            &mut coverage,
        );

        assert_eq!(layout, DecoratedTextLayout::unchanged("<#red>2026<#>"));
    }

    #[test]
    fn decorates_pixel_metrics_as_absolute_values() {
        let mut profile = CompositeFontProfile::neutral_default();
        profile.western = FontAdjustment {
            font_family: "Pixel Font".to_owned(),
            metric_unit: compositefont_core::MetricUnit::Pixels,
            size_px: Some(48.0),
            baseline_shift_px: Some(3.0),
            tracking_adjust_px: Some(-2.0),
            ..FontAdjustment::neutral()
        };

        assert_eq!(
            decorate_plain_text("A", &profile, Some(64.0), Some(3.0)),
            "<@Pixel Font><s*0.75><gw1><p+0,-3>A<p><gw><s><@>"
        );
    }

    #[test]
    fn omits_absolute_adjustments_without_base_metrics() {
        assert_eq!(
            decorate_plain_text("A", &decoration_profile(), None, None),
            "<@Latin><s*1.15><tw1.1><th0.9>A<th><tw><s><@>"
        );
    }

    #[test]
    fn closes_runs_around_line_breaks() {
        assert_eq!(
            decorate_plain_text("A\r\n国\nB", &decoration_profile(), Some(100.0), Some(0.0),),
            concat!(
                "<@Latin><s*1.15><gw5><tw1.1><th0.9><p+0,-2>A",
                "<p><th><tw><gw><s><@>\r\n",
                "<@Japanese>国<@>\n",
                "<@Latin><s*1.15><gw5><tw1.1><th0.9><p+0,-2>B",
                "<p><th><tw><gw><s><@>"
            )
        );
    }

    #[test]
    fn resolved_runs_expose_utf8_and_utf16_ranges_for_sdk_adapter() {
        let text = "A𠮷\nあ";
        let mut coverage = UnknownGlyphCoverage;
        let runs = resolve_text_runs(
            text,
            &CompositeFontProfile::neutral_default(),
            &mut coverage,
        );

        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].range.utf8, 0..5);
        assert_eq!(runs[0].range.utf16, 0..3);
        assert_eq!(&text[runs[0].range.utf8.clone()], "A𠮷");
        assert_eq!(runs[1].range.utf8, 5..6);
        assert_eq!(runs[1].range.utf16, 3..4);
        assert_eq!(runs[1].adjustment, None);
        assert_eq!(runs[2].range.utf8, 6..9);
        assert_eq!(runs[2].range.utf16, 4..5);
        assert_eq!(&text[runs[2].range.utf8.clone()], "あ");
    }

    #[test]
    fn possible_control_text_is_returned_byte_for_byte() {
        let profile = decoration_profile();
        for text in [
            "<#ff0000>国LINE<#>",
            "<@User Font>LINE<@>",
            "<s120>国<s>",
            "</>漢字<!>かんじ</>",
            "<// comment > remains opaque //>国",
            "<?obj.rz=obj.time*360?>国",
            "<?=string.format(\"%d\", obj.frame)?>",
            "<future-control>国",
            "閉じていない<",
            "1 < 2",
        ] {
            assert_eq!(
                decorate_plain_text(text, &profile, Some(100.0), Some(0.0)),
                text
            );
        }
    }

    #[test]
    fn generated_text_is_idempotent() {
        let profile = decoration_profile();
        let once = decorate_plain_text("国LINE", &profile, Some(100.0), Some(0.0));
        let twice = decorate_plain_text(&once, &profile, Some(100.0), Some(0.0));
        assert_eq!(twice, once);
    }

    #[test]
    fn unsafe_used_adjustment_fails_open() {
        let mut profile = decoration_profile();
        profile.western.font_family = "Unsafe,Font".to_owned();
        assert_eq!(
            decorate_plain_text("ABC", &profile, Some(100.0), Some(0.0)),
            "ABC"
        );
        profile.western.font_family = "Unsafe\nFont".to_owned();
        assert_eq!(
            decorate_plain_text("ABC", &profile, Some(100.0), Some(0.0)),
            "ABC"
        );
    }

    #[test]
    fn greater_than_and_fullwidth_angle_brackets_are_plain_text() {
        let mut profile = decoration_profile();
        profile.symbol = profile.western.clone();
        let decorated = decorate_plain_text("A>B＜C＞", &profile, None, None);
        assert!(decorated.contains("A>B＜C＞"));
        assert_ne!(decorated, "A>B＜C＞");
    }

    #[test]
    fn missing_profile_fails_open() {
        let mut profiles = ReloadableProfiles {
            store: ProfileStore::from_profiles([decoration_profile()]),
            path: PathBuf::from("missing-profile-file.json"),
            observed_modified: None,
            last_checked: Instant::now(),
        };
        let mut coverage = UnknownGlyphCoverage;
        assert_eq!(
            profiles.decorate(
                "国LINE",
                Some("missing"),
                Some(100.0),
                Some(0.0),
                &mut coverage,
            ),
            "国LINE"
        );
    }
}
