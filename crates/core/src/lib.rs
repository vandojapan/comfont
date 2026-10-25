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

const fn enabled_by_default() -> bool {
    true
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
    /// falseのときフォント名を描画へ渡さず、保存値は保持する。
    #[serde(default = "enabled_by_default")]
    pub font_family_enabled: bool,
    /// 先頭フォントに対象グリフがない場合に順番に試すフォントファミリー。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_font_families: Vec<String>,
    /// 基準フォントサイズに掛ける倍率。`1.0`で変更なし。
    pub size_ratio: f64,
    #[serde(default = "enabled_by_default")]
    pub size_enabled: bool,
    /// 基準フォントサイズを1emとするベースライン移動量。
    pub baseline_shift_em: f64,
    #[serde(default = "enabled_by_default")]
    pub baseline_shift_enabled: bool,
    /// 基準フォントサイズを1emとする字送り補正量。
    pub tracking_adjust_em: f64,
    #[serde(default = "enabled_by_default")]
    pub tracking_adjust_enabled: bool,
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
    #[serde(default = "enabled_by_default")]
    pub vertical_scale_enabled: bool,
    /// Illustratorの水平比率に相当する倍率。`1.0`で変形なし。
    #[serde(default = "neutral_scale_ratio")]
    pub horizontal_scale_ratio: f64,
    #[serde(default = "enabled_by_default")]
    pub horizontal_scale_enabled: bool,
    /// 描画時にこの範囲の合字・必須結合を強制解除する。旧設定はOFF。
    #[serde(default)]
    pub disable_ligatures: bool,
    /// falseのときリガチャ設定を出力せず、AviUtl2標準の字形形成を使う。
    #[serde(default = "enabled_by_default")]
    pub ligature_setting_enabled: bool,
}

impl FontAdjustment {
    pub fn neutral() -> Self {
        Self {
            font_family: String::new(),
            font_family_enabled: true,
            fallback_font_families: Vec::new(),
            size_ratio: 1.0,
            size_enabled: true,
            baseline_shift_em: 0.0,
            baseline_shift_enabled: true,
            tracking_adjust_em: 0.0,
            tracking_adjust_enabled: true,
            metric_unit: MetricUnit::Percent,
            size_px: None,
            baseline_shift_px: None,
            tracking_adjust_px: None,
            vertical_scale_ratio: 1.0,
            vertical_scale_enabled: true,
            horizontal_scale_ratio: 1.0,
            horizontal_scale_enabled: true,
            disable_ligatures: false,
            ligature_setting_enabled: true,
        }
    }

    pub fn effective_font_family(&self) -> &str {
        if self.font_family_enabled {
            &self.font_family
        } else {
            ""
        }
    }

    pub fn effective_vertical_scale_ratio(&self) -> f64 {
        if self.vertical_scale_enabled {
            self.vertical_scale_ratio
        } else {
            1.0
        }
    }

    pub fn effective_horizontal_scale_ratio(&self) -> f64 {
        if self.horizontal_scale_enabled {
            self.horizontal_scale_ratio
        } else {
            1.0
        }
    }

    pub fn effective_disable_ligatures(&self) -> bool {
        self.ligature_setting_enabled && self.disable_ligatures
    }

    pub fn effective_metrics(&self, base_font_size: Option<f64>) -> Option<EffectiveFontMetrics> {
        match self.metric_unit {
            MetricUnit::Percent => Some(EffectiveFontMetrics {
                size_ratio: if self.size_enabled {
                    self.size_ratio
                } else {
                    1.0
                },
                baseline_shift_em: if self.baseline_shift_enabled {
                    self.baseline_shift_em
                } else {
                    0.0
                },
                tracking_adjust_em: if self.tracking_adjust_enabled {
                    self.tracking_adjust_em
                } else {
                    0.0
                },
            }),
            MetricUnit::Pixels => {
                let needs_base = self.size_enabled
                    || self.baseline_shift_enabled
                    || self.tracking_adjust_enabled;
                let base = needs_base
                    .then(|| base_font_size.filter(|value| value.is_finite() && *value > 0.0))
                    .flatten();
                if needs_base && base.is_none() {
                    return None;
                }
                let base = base.unwrap_or(1.0);
                Some(EffectiveFontMetrics {
                    size_ratio: if self.size_enabled {
                        self.size_px? / base
                    } else {
                        1.0
                    },
                    baseline_shift_em: if self.baseline_shift_enabled {
                        self.baseline_shift_px? / base
                    } else {
                        0.0
                    },
                    tracking_adjust_em: if self.tracking_adjust_enabled {
                        self.tracking_adjust_px? / base
                    } else {
                        0.0
                    },
                })
            }
        }
    }
}

/// Inclusive Unicode range, optionally restricted to disjoint subranges.
/// Endpoints must be scalar values; a range may span the surrogate gap.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnicodeRange {
    pub name: String,
    pub start: u32,
    pub end: u32,
    /// When nonempty, only these inclusive ranges match. start/end are the envelope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subranges: Vec<[u32; 2]>,
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
        if !self.subranges.is_empty()
            && (self.subranges.first().unwrap()[0] != self.start
                || self.subranges.last().unwrap()[1] != self.end
                || self.subranges.iter().any(|&[start, end]| {
                    char::from_u32(start).is_none() || char::from_u32(end).is_none() || start > end
                })
                || self
                    .subranges
                    .windows(2)
                    .any(|pair| pair[0][1] >= pair[1][0]))
        {
            return Err("複数範囲は開始・終了の内側で、重複のない昇順のUnicodeスカラー値として指定してください。".to_owned());
        }
        Ok(())
    }
    pub fn contains(&self, character: char) -> bool {
        let codepoint = character as u32;
        self.validate().is_ok()
            && (self.start..=self.end).contains(&codepoint)
            && (self.subranges.is_empty()
                || self
                    .subranges
                    .iter()
                    .any(|&[start, end]| (start..=end).contains(&codepoint)))
    }
    /// Identify saved/copied Emoji presets by their set, independent of the row name.
    pub fn is_emoji_preset(&self) -> bool {
        self.subranges == EMOJI_RANGES
    }

    /// Match a grapheme while retaining the official scalar set in `contains`.
    pub fn matches_cluster(&self, cluster: &str) -> bool {
        let Some(character) = cluster.chars().next() else {
            return false;
        };
        self.contains(character)
            && (!self.is_emoji_preset() || !is_keycap_base(character) || is_emoji_keycap(cluster))
    }

    pub fn range_label(&self) -> String {
        range_label(self.start, self.end, &self.subranges)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct UnicodePreset {
    pub name: &'static str,
    /// Official Unicode block name or English range label, also used for searching.
    pub block_name: &'static str,
    pub start: u32,
    pub end: u32,
    pub frequent: bool,
    /// Empty for a contiguous block; otherwise the exact inclusive ranges.
    pub subranges: &'static [[u32; 2]],
}
impl UnicodePreset {
    pub fn to_range(&self, adjustment: FontAdjustment) -> UnicodeRange {
        UnicodeRange {
            name: self.name.to_owned(),
            start: self.start,
            end: self.end,
            subranges: self.subranges.to_vec(),
            adjustment,
        }
    }
    pub fn range_label(&self) -> String {
        range_label(self.start, self.end, self.subranges)
    }
    /// Search Japanese/English names or a code point within this range.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim();
        if query.is_empty() {
            return true;
        }
        if let Ok(codepoint) = parse_codepoint(query) {
            return (self.start..=self.end).contains(&codepoint)
                && (self.subranges.is_empty()
                    || self
                        .subranges
                        .iter()
                        .any(|&[start, end]| (start..=end).contains(&codepoint)));
        }
        let names = format!("{} {}", self.name, self.block_name).to_lowercase();
        query
            .to_lowercase()
            .split_whitespace()
            .all(|word| names.contains(word))
    }
}
fn range_label(start: u32, end: u32, subranges: &[[u32; 2]]) -> String {
    if subranges.is_empty() {
        format!("U+{start:04X}–U+{end:04X}")
    } else {
        format!("{}範囲", subranges.len())
    }
}
mod emoji_ranges;
pub use emoji_ranges::EMOJI_RANGES;
mod unicode_presets;
pub use unicode_presets::UNICODE_PRESETS;

fn is_keycap_base(character: char) -> bool {
    character.is_ascii_digit() || matches!(character, '#' | '*')
}

/// Fully- and minimally-qualified emoji keycaps, kept as a single grapheme.
pub fn is_emoji_keycap(cluster: &str) -> bool {
    let mut characters = cluster.chars();
    if !characters.next().is_some_and(is_keycap_base) {
        return false;
    }
    let mark = match characters.next() {
        Some('\u{FE0F}') => characters.next(),
        next => next,
    };
    mark == Some('\u{20E3}') && characters.next().is_none()
}

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

/// Display entries mix a legacy class (including its candidates) with a Unicode range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileEntry {
    Class(CharacterClass),
    Range(usize),
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
    /// Display order of legacy classes. It does not change character classification.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub class_order: Vec<CharacterClass>,
    /// Mixed display order. Custom-range precedence is stored in custom_ranges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entry_order: Vec<ProfileEntry>,
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
    /// Keep old class order when absent, ignoring duplicate and stale range entries.
    pub fn ordered_entries(&self) -> Vec<ProfileEntry> {
        let mut order = Vec::with_capacity(7 + self.custom_ranges.len());
        let mut seen_classes = Vec::with_capacity(7);
        let mut seen_ranges = vec![false; self.custom_ranges.len()];
        for entry in self
            .entry_order
            .iter()
            .copied()
            .chain(self.ordered_classes().into_iter().map(ProfileEntry::Class))
            .chain((0..self.custom_ranges.len()).map(ProfileEntry::Range))
        {
            let duplicate = match entry {
                ProfileEntry::Class(class) => {
                    if seen_classes.contains(&class) {
                        true
                    } else {
                        seen_classes.push(class);
                        false
                    }
                }
                ProfileEntry::Range(index) => seen_ranges
                    .get_mut(index)
                    .map(|seen| std::mem::replace(seen, true))
                    .unwrap_or(true),
            };
            if !duplicate {
                order.push(entry);
            }
        }
        order
    }

    /// Missing classes are appended in the legacy order; duplicates are ignored.
    pub fn ordered_classes(&self) -> Vec<CharacterClass> {
        let mut order = Vec::with_capacity(7);
        for class in self.class_order.iter().copied().chain([
            CharacterClass::Kanji,
            CharacterClass::Hiragana,
            CharacterClass::Katakana,
            CharacterClass::Symbol,
            CharacterClass::Western,
            CharacterClass::Digit,
            CharacterClass::Other,
        ]) {
            if !order.contains(&class) {
                order.push(class);
            }
        }
        order
    }

    pub fn neutral_default() -> Self {
        Self {
            name: DEFAULT_PROFILE_NAME.to_owned(),
            custom_ranges: Vec::new(),
            disabled_classes: Vec::new(),
            class_order: Vec::new(),
            entry_order: Vec::new(),
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
        let mut encoded = [0u8; 4];
        self.matching_range_for_cluster(character.encode_utf8(&mut encoded))
    }

    pub fn matching_range_for_cluster(&self, cluster: &str) -> Option<(usize, &UnicodeRange)> {
        self.custom_ranges
            .iter()
            .enumerate()
            .find(|(_, range)| range.matches_cluster(cluster))
    }

    pub fn class_enabled(&self, class: CharacterClass) -> bool {
        !self.disabled_classes.contains(&class)
    }

    pub fn adjustment_for_character(&self, character: char) -> Option<&FontAdjustment> {
        let mut encoded = [0u8; 4];
        self.adjustment_for_cluster(character.encode_utf8(&mut encoded))
    }

    pub fn adjustment_for_cluster(&self, cluster: &str) -> Option<&FontAdjustment> {
        if let Some((_, range)) = self.matching_range_for_cluster(cluster) {
            return Some(&range.adjustment);
        }
        let class = classify_character(cluster.chars().next()?);
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
            font_family: adjustment.effective_font_family().to_owned(),
            size_ratio: metrics.size_ratio,
            baseline_shift_em: metrics.baseline_shift_em,
            tracking_adjust_em: metrics.tracking_adjust_em,
            vertical_scale_ratio: adjustment.effective_vertical_scale_ratio(),
            horizontal_scale_ratio: adjustment.effective_horizontal_scale_ratio(),
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
            class_order: Vec::new(),
            entry_order: Vec::new(),
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
    fn disabled_parameters_keep_saved_values_but_resolve_to_no_override() {
        let mut adjustment = FontAdjustment {
            font_family: "Saved font".into(),
            size_ratio: 1.35,
            baseline_shift_em: 0.2,
            tracking_adjust_em: -0.1,
            vertical_scale_ratio: 0.8,
            horizontal_scale_ratio: 1.2,
            disable_ligatures: true,
            ..FontAdjustment::neutral()
        };
        adjustment.font_family_enabled = false;
        adjustment.size_enabled = false;
        adjustment.baseline_shift_enabled = false;
        adjustment.tracking_adjust_enabled = false;
        adjustment.vertical_scale_enabled = false;
        adjustment.horizontal_scale_enabled = false;
        adjustment.ligature_setting_enabled = false;

        assert_eq!(adjustment.font_family, "Saved font");
        assert_eq!(adjustment.effective_font_family(), "");
        assert_eq!(
            adjustment.effective_metrics(None),
            Some(EffectiveFontMetrics::default())
        );
        assert_eq!(adjustment.effective_vertical_scale_ratio(), 1.0);
        assert_eq!(adjustment.effective_horizontal_scale_ratio(), 1.0);
        assert!(!adjustment.effective_disable_ligatures());
    }

    #[test]
    fn class_display_order_defaults_and_normalizes_partial_orders() {
        let mut profile = CompositeFontProfile::neutral_default();
        assert_eq!(
            profile.ordered_classes(),
            vec![
                CharacterClass::Kanji,
                CharacterClass::Hiragana,
                CharacterClass::Katakana,
                CharacterClass::Symbol,
                CharacterClass::Western,
                CharacterClass::Digit,
                CharacterClass::Other,
            ]
        );
        profile.class_order = vec![
            CharacterClass::Digit,
            CharacterClass::Digit,
            CharacterClass::Kanji,
        ];
        assert_eq!(
            profile.ordered_classes(),
            vec![
                CharacterClass::Digit,
                CharacterClass::Kanji,
                CharacterClass::Hiragana,
                CharacterClass::Katakana,
                CharacterClass::Symbol,
                CharacterClass::Western,
                CharacterClass::Other,
            ]
        );
    }

    #[test]
    fn mixed_entry_order_preserves_legacy_order_and_normalizes_stale_references() {
        let mut profile = test_profile();
        profile.class_order = vec![CharacterClass::Western];
        profile.custom_ranges.push(UnicodeRange {
            name: "Latin".into(),
            start: 0x41,
            end: 0x5a,
            subranges: Vec::new(),
            adjustment: adjustment("Range"),
        });
        let loaded = profile.clone();
        assert_eq!(
            loaded.ordered_entries()[0],
            ProfileEntry::Class(CharacterClass::Western)
        );
        assert_eq!(
            loaded.ordered_entries().last(),
            Some(&ProfileEntry::Range(0))
        );
        profile.entry_order = vec![
            ProfileEntry::Range(0),
            ProfileEntry::Range(99),
            ProfileEntry::Class(CharacterClass::Digit),
            ProfileEntry::Range(0),
        ];
        let entries = profile.ordered_entries();
        assert_eq!(entries.len(), 8);
        assert_eq!(
            &entries[..2],
            &[
                ProfileEntry::Range(0),
                ProfileEntry::Class(CharacterClass::Digit)
            ]
        );
        assert_eq!(profile.resolve('A').font_family, "Range");
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
                subranges: Vec::new(),
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
            subranges: Vec::new(),
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
                preset
                    .to_range(FontAdjustment::neutral())
                    .validate()
                    .is_ok()
            );
        }
    }
}

#[cfg(test)]
mod unicode_preset_tests {
    use super::*;

    #[test]
    fn presets_cover_official_unicode_blocks_without_surrogates_or_duplicates() {
        let mut official: Vec<_> = include_str!("../../../resource/unicode/Blocks-17.0.0.txt")
            .lines()
            .filter_map(|line| {
                let (range, name) = line.split_once("; ")?;
                let (start, end) = range.split_once("..")?;
                let start = u32::from_str_radix(start, 16).ok()?;
                let end = u32::from_str_radix(end, 16).ok()?;
                if (0xD800..=0xDFFF).contains(&start) {
                    return None;
                }
                Some((start, end, name))
            })
            .collect();
        let mut actual: Vec<_> = UNICODE_PRESETS
            .iter()
            .map(|preset| (preset.start, preset.end, preset.block_name))
            .collect();
        // The combined emoji preset is an extra range, not an official block.
        let emoji_index = actual
            .iter()
            .position(|preset| preset.2 == "Emoji")
            .unwrap();
        assert_eq!(actual.remove(emoji_index), (0x0023, 0xE007F, "Emoji"));
        official.sort_unstable();
        actual.sort_unstable();
        assert_eq!(actual, official);
        assert_eq!(actual.len(), 343);
        assert!(actual.windows(2).all(|pair| pair[0].1 < pair[1].0));
    }

    #[test]
    fn common_blocks_come_first_and_search_accepts_both_names_and_codepoints() {
        let names: Vec<_> = UNICODE_PRESETS
            .iter()
            .take_while(|p| p.frequent)
            .map(|p| p.block_name)
            .collect();
        assert_eq!(
            names,
            [
                "CJK Unified Ideographs",
                "Hiragana",
                "Katakana",
                "Basic Latin",
                "Halfwidth and Fullwidth Forms",
                "Emoji"
            ]
        );
        assert!(UNICODE_PRESETS[names.len()..].iter().all(|p| !p.frequent));
        assert!(
            UNICODE_PRESETS[names.len()..]
                .windows(2)
                .all(|pair| pair[0].end < pair[1].start)
        );
        let hiragana = &UNICODE_PRESETS[1];
        for query in ["ひらがな", " HIRAGANA ", "U+3042", "0x3042", "3042", ""] {
            assert!(hiragana.matches(query), "{query}");
        }
        assert!(!hiragana.matches("U+30A2"));
        assert!(!hiragana.matches("Katakana"));
        let emoji = &UNICODE_PRESETS[names.len() - 1];
        for query in [
            "絵文字",
            " EMOJI ",
            "U+1F300",
            "0x1FAFA",
            "U+2764",
            "1F1EF",
            "0031",
            "E007F",
        ] {
            assert!(emoji.matches(query), "{query}");
        }
        for query in ["U+3042", "0041", "1F3010", "1FAFF"] {
            assert!(!emoji.matches(query), "{query}");
        }
        let extension_j = UNICODE_PRESETS
            .iter()
            .find(|p| p.block_name.ends_with("Ideographs Extension J"))
            .unwrap();
        assert_eq!((extension_j.start, extension_j.end), (0x323B0, 0x3347F));
        assert!(extension_j.matches("漢字 拡張J"));
        assert!(extension_j.matches("CJK Extension J"));
    }

    #[test]
    fn emoji_ranges_match_official_properties_and_cover_every_test_sequence() {
        use std::collections::BTreeSet;
        let mut official = BTreeSet::new();
        for line in include_str!("../../../resource/unicode/emoji-data-18.0.0.txt").lines() {
            let data = line.split('#').next().unwrap();
            let Some((points, property)) = data.split_once(';') else {
                continue;
            };
            if !matches!(property.trim(), "Emoji" | "Emoji_Component") {
                continue;
            }
            let (start, end) = points
                .trim()
                .split_once("..")
                .unwrap_or((points.trim(), points.trim()));
            official.extend(
                u32::from_str_radix(start, 16).unwrap()..=u32::from_str_radix(end, 16).unwrap(),
            );
        }
        let actual: BTreeSet<_> = EMOJI_RANGES
            .iter()
            .flat_map(|&[start, end]| start..=end)
            .collect();
        assert_eq!(actual, official);
        assert_eq!(actual.len(), 1546);
        assert_eq!(EMOJI_RANGES.len(), 154);
        let preset = UNICODE_PRESETS
            .iter()
            .find(|preset| preset.block_name == "Emoji")
            .unwrap();
        assert_eq!(preset.subranges, EMOJI_RANGES);
        let range = preset.to_range(FontAdjustment::neutral());
        assert!(range.validate().is_ok());
        let mut sequences = 0;
        for line in include_str!("../../../resource/unicode/emoji-test-18.0.0.txt").lines() {
            let Some((points, _)) = line.split('#').next().unwrap().split_once(';') else {
                continue;
            };
            sequences += 1;
            let mut cluster = String::new();
            for point in points.split_whitespace() {
                let character = char::from_u32(u32::from_str_radix(point, 16).unwrap()).unwrap();
                assert!(range.contains(character), "U+{point} in {line}");
                cluster.push(character);
            }
            assert!(range.matches_cluster(&cluster), "{line}");
        }
        assert!(sequences > 5000);
        for character in ['A', '漢', 'あ', ' ', '\u{1F100}', '\u{1FAFF}'] {
            assert!(!range.contains(character));
        }
    }

    #[test]
    fn emoji_keycaps_match_as_sequences_and_leave_bare_bases_to_lower_rules() {
        let mut profile = CompositeFontProfile::neutral_default();
        let preset = UNICODE_PRESETS
            .iter()
            .find(|p| p.block_name == "Emoji")
            .unwrap();
        let mut emoji = preset.to_range(FontAdjustment::neutral());
        emoji.name = "Renamed emoji".into();
        emoji.adjustment.font_family = "Emoji".into();
        profile.custom_ranges.push(emoji);
        profile.custom_ranges.push(UnicodeRange {
            name: "Digits and symbols".into(),
            start: 0x23,
            end: 0x39,
            subranges: Vec::new(),
            adjustment: FontAdjustment {
                font_family: "Numbers".into(),
                ..FontAdjustment::neutral()
            },
        });
        for character in "0123456789#*".chars() {
            assert!(profile.custom_ranges[0].contains(character));
            assert_eq!(profile.resolve(character).font_family, "Numbers");
            assert_eq!(profile.resolve(character).rule_id, "unicode:1");
            for cluster in [
                format!("{character}\u{20E3}"),
                format!("{character}\u{FE0F}\u{20E3}"),
            ] {
                assert!(is_emoji_keycap(&cluster));
                assert_eq!(profile.matching_range_for_cluster(&cluster).unwrap().0, 0);
            }
        }
        for malformed in [
            "",
            "1",
            "1\u{FE0F}",
            "1\u{FE0E}\u{20E3}",
            "A\u{20E3}",
            "1\u{20E3}\u{301}",
        ] {
            assert!(!is_emoji_keycap(malformed));
        }
        profile.custom_ranges.pop();
        profile.digit.font_family = "Digit class".into();
        assert_eq!(profile.resolve('1').font_family, "Digit class");
        assert_eq!(profile.resolve('1').rule_id, "digit");
    }

    #[test]
    fn rejects_malformed_subranges_including_invalid_envelopes() {
        let mut range = UnicodeRange {
            name: "test".into(),
            start: 0x23,
            end: 0x39,
            subranges: vec![[0x23, 0x23], [0x30, 0x39]],
            adjustment: FontAdjustment::neutral(),
        };
        assert!(range.validate().is_ok());
        assert!(!range.contains('$'));
        for subranges in [
            vec![[0x23, 0x30], [0x30, 0x39]],
            vec![[0x30, 0x39], [0x23, 0x23]],
            vec![[0x23, 0x23], [0x39, 0x30]],
            vec![[0x23, 0x23], [0xD800, 0xDFFF], [0x39, 0x39]],
            vec![[0x24, 0x39]],
            vec![[0x23, 0x38]],
        ] {
            range.subranges = subranges;
            assert!(range.validate().is_err());
            assert!(!range.contains('#'));
        }
    }
}
