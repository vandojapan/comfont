use std::collections::BTreeMap;

pub const DEFAULT_PROFILE_NAME: &str = "default";
pub const PROFILE_SCHEMA_VERSION: u32 = 1;

/// 合成フォントが区別する文字分類。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CharacterClass {
    /// 半角ASCIIの英字。
    Western,
    Hiragana,
    Katakana,
    Kanji,
    Digit,
    Symbol,
    /// 上記のいずれにも該当しない文字。空白や他言語の文字を含む。
    Other,
}

impl CharacterClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Western => "western",
            Self::Hiragana => "hiragana",
            Self::Katakana => "katakana",
            Self::Kanji => "kanji",
            Self::Digit => "digit",
            Self::Symbol => "symbol",
            Self::Other => "other",
        }
    }
}

/// 1個のUnicodeスカラー値を合成フォント用の文字種へ分類する。
///
/// UnicodeのScriptプロパティでは数字・句読点・長音符などが`Common`になるため、
/// 合成フォント用途に合わせて優先順位と共有記号の扱いを明示している。
pub fn classify_character(value: char) -> CharacterClass {
    let codepoint = value as u32;

    if is_hiragana(codepoint) {
        CharacterClass::Hiragana
    } else if is_katakana(codepoint) {
        CharacterClass::Katakana
    } else if is_kanji(codepoint) {
        CharacterClass::Kanji
    } else if value.is_numeric() {
        CharacterClass::Digit
    } else if value.is_ascii_alphabetic() {
        CharacterClass::Western
    } else if is_symbol(value) {
        CharacterClass::Symbol
    } else {
        CharacterClass::Other
    }
}

const fn is_hiragana(codepoint: u32) -> bool {
    matches!(
        codepoint,
        0x3041
            ..=0x309F // Hiragana block (combining sound marks included)
            | 0x1B001 // Hiragana Letter Archaic Ye
            | 0x1B11F // Hiragana Letter Archaic Wu
            | 0x1B132 // Hiragana Letter Small Ko
    )
}

const fn is_katakana(codepoint: u32) -> bool {
    matches!(
        codepoint,
        0x30A1..=0x30FA // Katakana letters
            | 0x30FC..=0x30FF // prolonged sound mark and iteration/digraph letters
            | 0x31F0..=0x31FF // Katakana Phonetic Extensions
            | 0xFF66..=0xFF9F // halfwidth Katakana and sound marks
            | 0x1B000 // Katakana Letter Archaic E
            | 0x1B100..=0x1B122 // Kana Supplement / Kana Extended-A Katakana
            | 0x1B155 // Katakana Letter Small Ko
            | 0x1B164..=0x1B167 // Katakana small letters
    )
}

const fn is_kanji(codepoint: u32) -> bool {
    matches!(
        codepoint,
        0x2E80..=0x2FDF // CJK radicals
            | 0x3005..=0x3007 // ideographic iteration/closing/zero
            | 0x3021..=0x3029 // Hangzhou numerals
            | 0x3038..=0x303B // ideographic tally/iteration marks
            | 0x31C0..=0x31EF // CJK strokes
            | 0x3400..=0x4DBF // CJK Unified Ideographs Extension A
            | 0x4E00..=0x9FFF // CJK Unified Ideographs
            | 0xF900..=0xFAFF // CJK Compatibility Ideographs
            | 0x16FE2..=0x16FE3 // Old Chinese marks
            | 0x20000..=0x2A6DF // CJK Unified Ideographs Extension B
            | 0x2A700..=0x2EE5F // CJK Unified Ideographs Extensions C through I
            | 0x2F800..=0x2FA1F // CJK Compatibility Ideographs Supplement
            | 0x30000..=0x33479 // CJK Unified Ideographs Extensions G through J
    )
}

fn is_symbol(value: char) -> bool {
    !value.is_alphanumeric() && !value.is_whitespace() && !value.is_control()
}

/// 文字入力またはプロファイル解決に失敗した理由。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ResolveError {
    #[error("character must contain one Unicode scalar value")]
    EmptyCharacter,
    #[error("character must contain exactly one Unicode scalar value")]
    MultipleCharacters,
    #[error("invalid Unicode code point: U+{0:04X}")]
    InvalidCodepoint(u32),
    #[error("composite font profile not found: {0:?}")]
    ProfileNotFound(String),
}

/// 1文字に対する解決結果。
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedFont {
    pub font_family: String,
    pub size_ratio: f64,
    pub baseline_shift_em: f64,
    pub tracking_adjust_em: f64,
    pub vertical_scale_ratio: f64,
    pub horizontal_scale_ratio: f64,
    pub category: String,
    pub rule_id: String,
}

const fn neutral_scale_ratio() -> f64 {
    1.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetricUnit {
    #[default]
    Percent,
    #[serde(rename = "px")]
    Pixels,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectiveFontMetrics {
    pub size_ratio: f64,
    pub baseline_shift_em: f64,
    pub tracking_adjust_em: f64,
}

impl Default for EffectiveFontMetrics {
    fn default() -> Self {
        Self {
            size_ratio: 1.0,
            baseline_shift_em: 0.0,
            tracking_adjust_em: 0.0,
        }
    }
}

/// 1分類に適用するフォントと補正値。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontAdjustment {
    /// 空文字列は「現在のフォントを変更しない」を表す。
    pub font_family: String,
    /// 先頭フォントに対象グリフがない場合に順番に試すフォントファミリー。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_font_families: Vec<String>,
    /// 基準フォントサイズに掛ける倍率。`1.0`で変更なし。
    pub size_ratio: f64,
    /// 基準フォントサイズを1emとするベースライン移動量。
    pub baseline_shift_em: f64,
    /// 基準フォントサイズを1emとする字送り補正量。
    pub tracking_adjust_em: f64,
    /// サイズ・ベースライン・字送りを絶対pxで保持する場合の単位指定。
    #[serde(default)]
    pub metric_unit: MetricUnit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_px: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_shift_px: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking_adjust_px: Option<f64>,
    /// Illustratorの垂直比率に相当する倍率。`1.0`で変形なし。
    #[serde(default = "neutral_scale_ratio")]
    pub vertical_scale_ratio: f64,
    /// Illustratorの水平比率に相当する倍率。`1.0`で変形なし。
    #[serde(default = "neutral_scale_ratio")]
    pub horizontal_scale_ratio: f64,
}

impl FontAdjustment {
    pub fn neutral() -> Self {
        Self {
            font_family: String::new(),
            fallback_font_families: Vec::new(),
            size_ratio: 1.0,
            baseline_shift_em: 0.0,
            tracking_adjust_em: 0.0,
            metric_unit: MetricUnit::Percent,
            size_px: None,
            baseline_shift_px: None,
            tracking_adjust_px: None,
            vertical_scale_ratio: 1.0,
            horizontal_scale_ratio: 1.0,
        }
    }

    pub fn effective_metrics(&self, base_font_size: Option<f64>) -> Option<EffectiveFontMetrics> {
        match self.metric_unit {
            MetricUnit::Percent => Some(EffectiveFontMetrics {
                size_ratio: self.size_ratio,
                baseline_shift_em: self.baseline_shift_em,
                tracking_adjust_em: self.tracking_adjust_em,
            }),
            MetricUnit::Pixels => {
                let base = base_font_size.filter(|value| value.is_finite() && *value > 0.0)?;
                Some(EffectiveFontMetrics {
                    size_ratio: self.size_px? / base,
                    baseline_shift_em: self.baseline_shift_px? / base,
                    tracking_adjust_em: self.tracking_adjust_px? / base,
                })
            }
        }
    }
}

/// Inclusive Unicode range. Endpoints must be scalar values; a range may span
/// the surrogate gap, which cannot match a Rust char.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnicodeRange {
    pub name: String,
    pub start: u32,
    pub end: u32,
    pub adjustment: FontAdjustment,
}
impl UnicodeRange {
    pub fn validate(&self) -> Result<(), String> {
        if char::from_u32(self.start).is_none()
            || char::from_u32(self.end).is_none()
            || self.start > self.end
        {
            return Err("範囲の開始・終了には U+0000～U+10FFFF のUnicodeスカラー値を昇順で指定してください。".to_owned());
        }
        Ok(())
    }
    pub fn contains(&self, character: char) -> bool {
        self.validate().is_ok() && (self.start..=self.end).contains(&(character as u32))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct UnicodePreset {
    pub name: &'static str,
    pub start: u32,
    pub end: u32,
}
pub const UNICODE_PRESETS: &[UnicodePreset] = &[
    UnicodePreset {
        name: "Basic Latin",
        start: 0x0000,
        end: 0x007F,
    },
    UnicodePreset {
        name: "Latin-1 Supplement",
        start: 0x0080,
        end: 0x00FF,
    },
    UnicodePreset {
        name: "ひらがな",
        start: 0x3040,
        end: 0x309F,
    },
    UnicodePreset {
        name: "カタカナ",
        start: 0x30A0,
        end: 0x30FF,
    },
    UnicodePreset {
        name: "CJK統合漢字",
        start: 0x4E00,
        end: 0x9FFF,
    },
    UnicodePreset {
        name: "CJK統合漢字拡張A",
        start: 0x3400,
        end: 0x4DBF,
    },
    UnicodePreset {
        name: "CJK統合漢字拡張B",
        start: 0x20000,
        end: 0x2A6DF,
    },
    UnicodePreset {
        name: "CJK記号・句読点",
        start: 0x3000,
        end: 0x303F,
    },
    UnicodePreset {
        name: "半角・全角形",
        start: 0xFF00,
        end: 0xFFEF,
    },
    UnicodePreset {
        name: "絵文字 (Emoticons)",
        start: 0x1F600,
        end: 0x1F64F,
    },
];

/// Accept hexadecimal values with optional U+ or 0x prefix.
pub fn parse_codepoint(value: &str) -> Result<u32, String> {
    let value = value.trim();
    let value = value
        .strip_prefix("U+")
        .or_else(|| value.strip_prefix("u+"))
        .or_else(|| value.strip_prefix("0x"))
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    let codepoint = u32::from_str_radix(value, 16)
        .map_err(|_| "コードポイントは16進数で入力してください。".to_owned())?;
    char::from_u32(codepoint).ok_or_else(|| "Unicodeスカラー値ではありません。".to_owned())?;
    Ok(codepoint)
}

/// 各文字分類のフォント設定を持つ合成フォントプロファイル。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositeFontProfile {
    /// Custom rules win over legacy classification; first matching range wins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_ranges: Vec<UnicodeRange>,
    /// Deleted legacy classes remain disabled across save/load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_classes: Vec<CharacterClass>,
    pub name: String,
    pub western: FontAdjustment,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub western_fallbacks: Vec<FontAdjustment>,
    pub hiragana: FontAdjustment,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hiragana_fallbacks: Vec<FontAdjustment>,
    pub katakana: FontAdjustment,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub katakana_fallbacks: Vec<FontAdjustment>,
    pub kanji: FontAdjustment,
    /// 漢字用の追加調整セット。上から順にグリフの有無を確認する。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kanji_fallbacks: Vec<FontAdjustment>,
    pub digit: FontAdjustment,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub digit_fallbacks: Vec<FontAdjustment>,
    pub symbol: FontAdjustment,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbol_fallbacks: Vec<FontAdjustment>,
    pub other: FontAdjustment,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_fallbacks: Vec<FontAdjustment>,
}

impl CompositeFontProfile {
    pub fn neutral_default() -> Self {
        Self {
            name: DEFAULT_PROFILE_NAME.to_owned(),
            custom_ranges: Vec::new(),
            disabled_classes: Vec::new(),
            western: FontAdjustment::neutral(),
            western_fallbacks: Vec::new(),
            hiragana: FontAdjustment::neutral(),
            hiragana_fallbacks: Vec::new(),
            katakana: FontAdjustment::neutral(),
            katakana_fallbacks: Vec::new(),
            kanji: FontAdjustment::neutral(),
            kanji_fallbacks: Vec::new(),
            digit: FontAdjustment::neutral(),
            digit_fallbacks: Vec::new(),
            symbol: FontAdjustment::neutral(),
            symbol_fallbacks: Vec::new(),
            other: FontAdjustment::neutral(),
            other_fallbacks: Vec::new(),
        }
    }

    pub fn adjustment_for(&self, class: CharacterClass) -> &FontAdjustment {
        match class {
            CharacterClass::Western => &self.western,
            CharacterClass::Hiragana => &self.hiragana,
            CharacterClass::Katakana => &self.katakana,
            CharacterClass::Kanji => &self.kanji,
            CharacterClass::Digit => &self.digit,
            CharacterClass::Symbol => &self.symbol,
            CharacterClass::Other => &self.other,
        }
    }

    pub fn adjustment_for_mut(&mut self, class: CharacterClass) -> &mut FontAdjustment {
        match class {
            CharacterClass::Western => &mut self.western,
            CharacterClass::Hiragana => &mut self.hiragana,
            CharacterClass::Katakana => &mut self.katakana,
            CharacterClass::Kanji => &mut self.kanji,
            CharacterClass::Digit => &mut self.digit,
            CharacterClass::Symbol => &mut self.symbol,
            CharacterClass::Other => &mut self.other,
        }
    }

    pub fn fallbacks_for(&self, class: CharacterClass) -> &[FontAdjustment] {
        match class {
            CharacterClass::Western => &self.western_fallbacks,
            CharacterClass::Hiragana => &self.hiragana_fallbacks,
            CharacterClass::Katakana => &self.katakana_fallbacks,
            CharacterClass::Kanji => &self.kanji_fallbacks,
            CharacterClass::Digit => &self.digit_fallbacks,
            CharacterClass::Symbol => &self.symbol_fallbacks,
            CharacterClass::Other => &self.other_fallbacks,
        }
    }

    pub fn fallbacks_for_mut(&mut self, class: CharacterClass) -> &mut Vec<FontAdjustment> {
        match class {
            CharacterClass::Western => &mut self.western_fallbacks,
            CharacterClass::Hiragana => &mut self.hiragana_fallbacks,
            CharacterClass::Katakana => &mut self.katakana_fallbacks,
            CharacterClass::Kanji => &mut self.kanji_fallbacks,
            CharacterClass::Digit => &mut self.digit_fallbacks,
            CharacterClass::Symbol => &mut self.symbol_fallbacks,
            CharacterClass::Other => &mut self.other_fallbacks,
        }
    }

    pub fn matching_range(&self, character: char) -> Option<(usize, &UnicodeRange)> {
        self.custom_ranges
            .iter()
            .enumerate()
            .find(|(_, range)| range.contains(character))
    }

    pub fn class_enabled(&self, class: CharacterClass) -> bool {
        !self.disabled_classes.contains(&class)
    }

    pub fn adjustment_for_character(&self, character: char) -> Option<&FontAdjustment> {
        if let Some((_, range)) = self.matching_range(character) {
            return Some(&range.adjustment);
        }
        let class = classify_character(character);
        self.class_enabled(class)
            .then(|| self.adjustment_for(class))
    }

    pub fn validate_ranges(&self) -> Result<(), String> {
        for range in &self.custom_ranges {
            range.validate()?;
        }
        Ok(())
    }

    pub fn resolve(&self, codepoint: char) -> ResolvedFont {
        let class = classify_character(codepoint);
        let neutral = FontAdjustment::neutral();
        let adjustment = self.adjustment_for_character(codepoint).unwrap_or(&neutral);
        let metrics = adjustment.effective_metrics(None).unwrap_or_default();
        let class_name = class.as_str();

        ResolvedFont {
            font_family: adjustment.font_family.clone(),
            size_ratio: metrics.size_ratio,
            baseline_shift_em: metrics.baseline_shift_em,
            tracking_adjust_em: metrics.tracking_adjust_em,
            vertical_scale_ratio: adjustment.vertical_scale_ratio,
            horizontal_scale_ratio: adjustment.horizontal_scale_ratio,
            category: class_name.to_owned(),
            rule_id: self.matching_range(codepoint).map_or_else(
                || class_name.to_owned(),
                |(index, _)| format!("unicode:{index}"),
            ),
        }
    }
}

/// editor-pluginとmodule-pluginが共有する永続化文書。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileDocument {
    pub schema_version: u32,
    pub profiles: Vec<CompositeFontProfile>,
}

impl ProfileDocument {
    pub fn with_builtin_default() -> Self {
        Self {
            schema_version: PROFILE_SCHEMA_VERSION,
            profiles: vec![CompositeFontProfile::neutral_default()],
        }
    }

    pub fn from_profiles(profiles: Vec<CompositeFontProfile>) -> Self {
        Self {
            schema_version: PROFILE_SCHEMA_VERSION,
            profiles,
        }
    }
}

#[derive(Debug)]
pub struct ProfileStore {
    profiles: BTreeMap<String, CompositeFontProfile>,
}

impl ProfileStore {
    pub fn profiles(&self) -> impl Iterator<Item = &CompositeFontProfile> {
        self.profiles.values()
    }

    pub fn with_builtin_default() -> Self {
        Self::from_profiles([CompositeFontProfile::neutral_default()])
    }

    pub fn from_profiles(profiles: impl IntoIterator<Item = CompositeFontProfile>) -> Self {
        Self {
            profiles: profiles
                .into_iter()
                .map(|profile| (profile.name.clone(), profile))
                .collect(),
        }
    }

    pub fn resolve(
        &self,
        codepoint: char,
        profile_name: Option<&str>,
    ) -> Result<ResolvedFont, ResolveError> {
        let profile = self.profile(profile_name)?;
        Ok(profile.resolve(codepoint))
    }

    pub fn profile(
        &self,
        profile_name: Option<&str>,
    ) -> Result<&CompositeFontProfile, ResolveError> {
        let profile_name = profile_name.unwrap_or(DEFAULT_PROFILE_NAME);
        self.profiles
            .get(profile_name)
            .ok_or_else(|| ResolveError::ProfileNotFound(profile_name.to_owned()))
    }
}

pub fn single_codepoint(value: &str) -> Result<char, ResolveError> {
    let mut chars = value.chars();
    let first = chars.next().ok_or(ResolveError::EmptyCharacter)?;
    if chars.next().is_some() {
        return Err(ResolveError::MultipleCharacters);
    }
    Ok(first)
}

pub fn codepoint_to_char(value: u32) -> Result<char, ResolveError> {
    char::from_u32(value).ok_or(ResolveError::InvalidCodepoint(value))
}

/// Stable anm2 selector value. Zero belongs to the built-in default, -1 to off.
/// Names are hashed rather than indexed so reordering cannot change selection.
/// Display-safe label for AviUtl's static select directive and saved alias.
pub fn profile_selection_label(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            ',' => '，',
            '=' => '＝',
            ':' => '：',
            '\\' => '＼',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect()
}
pub fn profile_selection_id(name: &str) -> i32 {
    if name == DEFAULT_PROFILE_NAME {
        return 0;
    }
    let hash = name.as_bytes().iter().fold(2166136261u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(16777619)
    });
    ((hash & 0x7fffffff).max(1)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adjustment(font_family: &str) -> FontAdjustment {
        FontAdjustment {
            font_family: font_family.to_owned(),
            fallback_font_families: Vec::new(),
            size_ratio: 1.0,
            baseline_shift_em: 0.0,
            tracking_adjust_em: 0.0,
            vertical_scale_ratio: 1.0,
            horizontal_scale_ratio: 1.0,
            ..FontAdjustment::neutral()
        }
    }

    fn test_profile() -> CompositeFontProfile {
        CompositeFontProfile {
            custom_ranges: Vec::new(),
            disabled_classes: Vec::new(),
            name: "subtitle".to_owned(),
            western: adjustment("Inter"),
            western_fallbacks: Vec::new(),
            hiragana: adjustment("Hiragana Font"),
            hiragana_fallbacks: Vec::new(),
            katakana: adjustment("Katakana Font"),
            katakana_fallbacks: Vec::new(),
            kanji: adjustment("Kanji Font"),
            kanji_fallbacks: Vec::new(),
            digit: adjustment("Digit Font"),
            digit_fallbacks: Vec::new(),
            symbol: adjustment("Symbol Font"),
            symbol_fallbacks: Vec::new(),
            other: adjustment("Fallback Font"),
            other_fallbacks: Vec::new(),
        }
    }

    #[test]
    fn classifies_ascii_letters_as_western() {
        for character in ['A', 'z'] {
            assert_eq!(classify_character(character), CharacterClass::Western);
        }
    }

    #[test]
    fn classifies_hiragana() {
        for character in ['あ', 'ん', 'ゝ', '\u{3099}'] {
            assert_eq!(classify_character(character), CharacterClass::Hiragana);
        }
    }

    #[test]
    fn classifies_katakana() {
        for character in ['ア', 'ン', 'ヿ', 'ー', 'ｶ', 'ﾞ'] {
            assert_eq!(classify_character(character), CharacterClass::Katakana);
        }
    }

    #[test]
    fn classifies_kanji() {
        for character in ['漢', '字', '々', '𠮷'] {
            assert_eq!(classify_character(character), CharacterClass::Kanji);
        }
    }

    #[test]
    fn classifies_digits() {
        for character in ['0', '9', '０', '９', '٥'] {
            assert_eq!(classify_character(character), CharacterClass::Digit);
        }
    }

    #[test]
    fn classifies_symbols() {
        for character in ['!', '?', '。', '・', '★', '😀'] {
            assert_eq!(classify_character(character), CharacterClass::Symbol);
        }
    }

    #[test]
    fn classifies_unmatched_characters_as_other() {
        for character in ['é', 'Ж', 'Ａ', ' '] {
            assert_eq!(classify_character(character), CharacterClass::Other);
        }
    }

    #[test]
    fn profile_selects_adjustment_for_each_class() {
        let profile = test_profile();

        for (character, font, category) in [
            ('A', "Inter", "western"),
            ('あ', "Hiragana Font", "hiragana"),
            ('カ', "Katakana Font", "katakana"),
            ('漢', "Kanji Font", "kanji"),
            ('1', "Digit Font", "digit"),
            ('!', "Symbol Font", "symbol"),
            ('Ж', "Fallback Font", "other"),
        ] {
            let resolved = profile.resolve(character);
            assert_eq!(resolved.font_family, font);
            assert_eq!(resolved.category, category);
            assert_eq!(resolved.rule_id, category);
        }
    }

    #[test]
    fn pixel_metrics_are_resolved_against_the_base_font_size() {
        let adjustment = FontAdjustment {
            metric_unit: MetricUnit::Pixels,
            size_px: Some(48.0),
            baseline_shift_px: Some(3.0),
            tracking_adjust_px: Some(-2.0),
            ..FontAdjustment::neutral()
        };

        assert_eq!(
            adjustment.effective_metrics(Some(24.0)),
            Some(EffectiveFontMetrics {
                size_ratio: 2.0,
                baseline_shift_em: 0.125,
                tracking_adjust_em: -2.0 / 24.0,
            })
        );
        assert_eq!(adjustment.effective_metrics(None), None);
    }

    #[test]
    fn accepts_exactly_one_unicode_scalar() {
        assert_eq!(single_codepoint("漢").unwrap(), '漢');
        assert_eq!(single_codepoint(""), Err(ResolveError::EmptyCharacter));
        assert_eq!(
            single_codepoint("ab"),
            Err(ResolveError::MultipleCharacters)
        );
    }

    #[test]
    fn validates_integer_codepoints() {
        assert_eq!(codepoint_to_char(0x6f22).unwrap(), '漢');
        assert_eq!(
            codepoint_to_char(0xd800),
            Err(ResolveError::InvalidCodepoint(0xd800))
        );
    }

    #[test]
    fn rejects_unknown_profile() {
        let store = ProfileStore::with_builtin_default();
        let error = store.resolve('A', Some("missing")).unwrap_err();
        assert_eq!(error, ResolveError::ProfileNotFound("missing".to_owned()));
    }

    #[test]
    fn custom_ranges_win_and_use_first_match_including_supplementary_planes() {
        let mut profile = test_profile();
        for (name, start, end, font) in [
            ("first", 0x41, 0x5A, "First"),
            ("overlap", 0x41, 0x7F, "Second"),
            ("emoji", 0x1F600, 0x1F64F, "Emoji"),
        ] {
            profile.custom_ranges.push(UnicodeRange {
                name: name.to_owned(),
                start,
                end,
                adjustment: adjustment(font),
            });
        }
        assert_eq!(profile.resolve('A').font_family, "First");
        assert_eq!(profile.resolve('A').rule_id, "unicode:0");
        assert_eq!(profile.resolve('😀').font_family, "Emoji");
        assert_eq!(profile.resolve('z').font_family, "Second");
        assert_eq!(profile.resolve('あ').font_family, "Hiragana Font");
        profile.custom_ranges[0].adjustment = FontAdjustment::neutral();
        assert_eq!(profile.resolve('A').font_family, ""); // Matching neutral range blocks lower rules.
    }
    #[test]
    fn validates_range_endpoints_and_hex_input() {
        for value in ["D800", "DFFF", "110000", "-1", "xyz"] {
            assert!(parse_codepoint(value).is_err());
        }
        assert_eq!(parse_codepoint(" U+1F600 ").unwrap(), 0x1F600);
        assert_eq!(parse_codepoint("0x10FFFF").unwrap(), 0x10FFFF);
        let mut range = UnicodeRange {
            name: "test".to_owned(),
            start: 0,
            end: 0x10FFFF,
            adjustment: FontAdjustment::neutral(),
        };
        assert!(range.validate().is_ok());
        range.start = 0xD800;
        assert!(range.validate().is_err());
        range.start = 10;
        range.end = 9;
        assert!(range.validate().is_err());
        for preset in UNICODE_PRESETS {
            assert!(
                UnicodeRange {
                    name: preset.name.to_owned(),
                    start: preset.start,
                    end: preset.end,
                    adjustment: FontAdjustment::neutral()
                }
                .validate()
                .is_ok()
            );
        }
    }
}
