use std::collections::BTreeMap;

pub const DEFAULT_PROFILE_NAME: &str = "default";
pub const PROFILE_SCHEMA_VERSION: u32 = 1;

/// 合成フォントが区別する文字分類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pub category: String,
    pub rule_id: String,
}

/// 1分類に適用するフォントと補正値。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontAdjustment {
    /// 空文字列は「現在のフォントを変更しない」を表す。
    pub font_family: String,
    /// 基準フォントサイズに掛ける倍率。`1.0`で変更なし。
    pub size_ratio: f64,
    /// 基準フォントサイズを1emとするベースライン移動量。
    pub baseline_shift_em: f64,
    /// 基準フォントサイズを1emとする字送り補正量。
    pub tracking_adjust_em: f64,
}

impl FontAdjustment {
    pub fn neutral() -> Self {
        Self {
            font_family: String::new(),
            size_ratio: 1.0,
            baseline_shift_em: 0.0,
            tracking_adjust_em: 0.0,
        }
    }
}

/// 各文字分類のフォント設定を持つ合成フォントプロファイル。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositeFontProfile {
    pub name: String,
    pub western: FontAdjustment,
    pub hiragana: FontAdjustment,
    pub katakana: FontAdjustment,
    pub kanji: FontAdjustment,
    pub digit: FontAdjustment,
    pub symbol: FontAdjustment,
    pub other: FontAdjustment,
}

impl CompositeFontProfile {
    pub fn neutral_default() -> Self {
        Self {
            name: DEFAULT_PROFILE_NAME.to_owned(),
            western: FontAdjustment::neutral(),
            hiragana: FontAdjustment::neutral(),
            katakana: FontAdjustment::neutral(),
            kanji: FontAdjustment::neutral(),
            digit: FontAdjustment::neutral(),
            symbol: FontAdjustment::neutral(),
            other: FontAdjustment::neutral(),
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

    pub fn resolve(&self, codepoint: char) -> ResolvedFont {
        let class = classify_character(codepoint);
        let adjustment = self.adjustment_for(class);
        let class_name = class.as_str();

        ResolvedFont {
            font_family: adjustment.font_family.clone(),
            size_ratio: adjustment.size_ratio,
            baseline_shift_em: adjustment.baseline_shift_em,
            tracking_adjust_em: adjustment.tracking_adjust_em,
            category: class_name.to_owned(),
            rule_id: class_name.to_owned(),
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
        let profile_name = profile_name.unwrap_or(DEFAULT_PROFILE_NAME);
        let profile = self
            .profiles
            .get(profile_name)
            .ok_or_else(|| ResolveError::ProfileNotFound(profile_name.to_owned()))?;
        Ok(profile.resolve(codepoint))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn adjustment(font_family: &str) -> FontAdjustment {
        FontAdjustment {
            font_family: font_family.to_owned(),
            size_ratio: 1.0,
            baseline_shift_em: 0.0,
            tracking_adjust_em: 0.0,
        }
    }

    fn test_profile() -> CompositeFontProfile {
        CompositeFontProfile {
            name: "subtitle".to_owned(),
            western: adjustment("Inter"),
            hiragana: adjustment("Hiragana Font"),
            katakana: adjustment("Katakana Font"),
            kanji: adjustment("Kanji Font"),
            digit: adjustment("Digit Font"),
            symbol: adjustment("Symbol Font"),
            other: adjustment("Fallback Font"),
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
}
