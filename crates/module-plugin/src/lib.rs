mod font_coverage;

use aviutl2::module::{IntoScriptModuleReturnValue, ScriptModule, ScriptModuleFunctions};
use compositefont_core::{
    CompositeFontProfile, DEFAULT_PROFILE_NAME, FontAdjustment, PROFILE_SCHEMA_VERSION,
    ProfileDocument, ProfileStore, ResolvedFont, classify_character, codepoint_to_char,
    single_codepoint,
};
#[cfg(test)]
use font_coverage::UnknownGlyphCoverage;
use font_coverage::{FontManagerGlyphCoverage, GlyphCoverage};
use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, SystemTime},
};

const API_VERSION: i32 = 8;
#[cfg(any(debug_assertions, test))]
const INTEGRATION_TEST_PROFILE: &str = "integration-test";

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

#[cfg(debug_assertions)]
fn builtin_profiles() -> Vec<CompositeFontProfile> {
    vec![
        CompositeFontProfile::neutral_default(),
        integration_test_profile(),
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
        profiles.retain(|profile| profile.name != INTEGRATION_TEST_PROFILE);
        profiles.push(integration_test_profile());
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

trait TextRunBackend {
    type Output;

    /// `None` means that this backend cannot represent the resolved runs.
    fn apply(
        &self,
        source: &str,
        runs: &[ResolvedTextRun],
        base_format: BaseTextFormat,
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
        .apply(text, &runs, base_format)
        .unwrap_or_else(|| text.to_owned())
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
    ) -> Option<Self::Output> {
        if runs
            .iter()
            .filter_map(|run| run.adjustment.as_ref())
            .any(|adjustment| !is_safe_adjustment(adjustment))
        {
            return None;
        }

        let mut decorated = String::with_capacity(source.len() + runs.len() * 48);
        for run in runs {
            write_control_tag_run(&mut decorated, source, run, base_format);
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
) {
    let run_text = &source[run.range.utf8.clone()];
    let Some(adjustment) = run.adjustment.as_ref() else {
        output.push_str(run_text);
        return;
    };
    let metrics = adjustment
        .effective_metrics(base_format.font_size)
        .unwrap_or_default();
    let has_font = !adjustment.font_family.is_empty();
    let has_size = !approximately_equal(metrics.size_ratio, 1.0);
    let has_tracking = base_format.font_size.is_some()
        && base_format.char_spacing.is_some()
        && !approximately_equal(metrics.tracking_adjust_em, 0.0);
    let has_horizontal = !approximately_equal(adjustment.horizontal_scale_ratio, 1.0);
    let has_vertical = !approximately_equal(adjustment.vertical_scale_ratio, 1.0);
    let has_baseline =
        base_format.font_size.is_some() && !approximately_equal(metrics.baseline_shift_em, 0.0);

    if has_font {
        output.push_str("<@");
        output.push_str(&adjustment.font_family);
        output.push('>');
    }
    if has_size {
        output.push_str("<s*");
        output.push_str(&format_control_number(metrics.size_ratio));
        output.push('>');
    }
    if has_tracking {
        output.push_str("<gw");
        output.push_str(&format_control_number(
            base_format.char_spacing.unwrap_or_default()
                + base_format.font_size.unwrap_or_default() * metrics.tracking_adjust_em,
        ));
        output.push('>');
    }
    if has_horizontal {
        output.push_str("<tw");
        output.push_str(&format_control_number(adjustment.horizontal_scale_ratio));
        output.push('>');
    }
    if has_vertical {
        output.push_str("<th");
        output.push_str(&format_control_number(adjustment.vertical_scale_ratio));
        output.push('>');
    }
    if has_baseline {
        let shift = -base_format.font_size.unwrap_or_default() * metrics.baseline_shift_em;
        output.push_str("<p+0,");
        if shift >= 0.0 {
            output.push('+');
        }
        output.push_str(&format_control_number(shift));
        output.push('>');
    }

    output.push_str(run_text);

    if has_baseline {
        output.push_str("<p>");
    }
    if has_vertical {
        output.push_str("<th>");
    }
    if has_horizontal {
        output.push_str("<tw>");
    }
    if has_tracking {
        output.push_str("<gw>");
    }
    if has_size {
        output.push_str("<s>");
    }
    if has_font {
        output.push_str("<@>");
    }
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
struct CompositeFontModule {
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
}

aviutl2::register_script_module!(CompositeFontModule);

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct TestCoverage {
        values: HashMap<(String, char), bool>,
    }

    impl GlyphCoverage for TestCoverage {
        fn has_glyph(&mut self, font_family: &str, character: char) -> Option<bool> {
            self.values
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
