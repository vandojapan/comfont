use compositefont_core::{
    CharacterClass, CompositeFontProfile, DEFAULT_PROFILE_NAME, FontAdjustment, ProfileDocument,
};

pub const CATEGORY_ROWS: [CategoryRow; 7] = [
    CategoryRow::new(CharacterClass::Kanji, "漢字", "国漢字"),
    CategoryRow::new(CharacterClass::Hiragana, "かな", "あいう"),
    CategoryRow::new(CharacterClass::Katakana, "カナ", "アイウ"),
    CategoryRow::new(CharacterClass::Symbol, "記号", "。・！？★"),
    CategoryRow::new(CharacterClass::Western, "欧文", "LINE Word"),
    CategoryRow::new(CharacterClass::Digit, "数字", "123456"),
    CategoryRow::new(CharacterClass::Other, "その他", "é Ж Ａ"),
];

#[derive(Clone, Copy, Debug)]
pub struct CategoryRow {
    pub class: CharacterClass,
    pub label: &'static str,
    pub sample: &'static str,
}

impl CategoryRow {
    const fn new(class: CharacterClass, label: &'static str, sample: &'static str) -> Self {
        Self {
            class,
            label,
            sample,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EditorModel {
    document: ProfileDocument,
    selected_profile: usize,
    selected_category: usize,
}

impl EditorModel {
    pub fn new(mut document: ProfileDocument) -> Self {
        if document.profiles.is_empty() {
            document = ProfileDocument::with_builtin_default();
        }
        Self {
            document,
            selected_profile: 0,
            selected_category: 0,
        }
    }

    pub fn document(&self) -> &ProfileDocument {
        &self.document
    }

    pub fn selected_profile_index(&self) -> usize {
        self.selected_profile
    }

    pub fn selected_category_index(&self) -> usize {
        self.selected_category
    }

    pub fn selected_profile(&self) -> &CompositeFontProfile {
        &self.document.profiles[self.selected_profile]
    }

    pub fn selected_profile_mut(&mut self) -> &mut CompositeFontProfile {
        &mut self.document.profiles[self.selected_profile]
    }

    pub fn selected_adjustment(&self) -> &FontAdjustment {
        self.selected_profile()
            .adjustment_for(CATEGORY_ROWS[self.selected_category].class)
    }

    pub fn select_profile(&mut self, index: usize) -> bool {
        if index >= self.document.profiles.len() {
            return false;
        }
        self.selected_profile = index;
        true
    }

    pub fn select_category(&mut self, index: usize) -> bool {
        if index >= CATEGORY_ROWS.len() {
            return false;
        }
        self.selected_category = index;
        true
    }

    pub fn create_profile(&mut self) {
        let mut suffix = 1;
        let name = loop {
            let candidate = format!("新規プロファイル {suffix}");
            if !self
                .document
                .profiles
                .iter()
                .any(|profile| profile.name == candidate)
            {
                break candidate;
            }
            suffix += 1;
        };

        let mut profile = CompositeFontProfile::neutral_default();
        profile.name = name;
        self.document.profiles.push(profile);
        self.selected_profile = self.document.profiles.len() - 1;
        self.selected_category = 0;
    }

    pub fn rename_selected_profile(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("プロファイル名を入力してください。".to_owned());
        }
        if self
            .document
            .profiles
            .iter()
            .enumerate()
            .any(|(index, profile)| index != self.selected_profile && profile.name == name)
        {
            return Err(format!("プロファイル「{name}」は既に存在します。"));
        }
        if self.selected_profile().name == DEFAULT_PROFILE_NAME && name != DEFAULT_PROFILE_NAME {
            return Err("defaultプロファイルの名前は変更できません。".to_owned());
        }
        self.selected_profile_mut().name = name.to_owned();
        Ok(())
    }

    pub fn delete_selected_profile(&mut self) -> Result<(), String> {
        if self.selected_profile().name == DEFAULT_PROFILE_NAME {
            return Err("defaultプロファイルは削除できません。".to_owned());
        }
        if self.document.profiles.len() == 1 {
            return Err("最後のプロファイルは削除できません。".to_owned());
        }
        self.document.profiles.remove(self.selected_profile);
        self.selected_profile = self
            .selected_profile
            .min(self.document.profiles.len().saturating_sub(1));
        self.selected_category = 0;
        Ok(())
    }

    pub fn update_selected_adjustment(
        &mut self,
        font_family: String,
        size_percent: f64,
        baseline_percent: f64,
        tracking_percent: f64,
    ) -> Result<(), String> {
        for (label, value) in [
            ("サイズ", size_percent),
            ("ベースライン", baseline_percent),
            ("字送り", tracking_percent),
        ] {
            if !value.is_finite() {
                return Err(format!("{label}には有限の数値を入力してください。"));
            }
        }
        if size_percent <= 0.0 {
            return Err("サイズは0より大きい値にしてください。".to_owned());
        }

        let class = CATEGORY_ROWS[self.selected_category].class;
        *self.selected_profile_mut().adjustment_for_mut(class) = FontAdjustment {
            font_family: font_family.trim().to_owned(),
            size_ratio: size_percent / 100.0,
            baseline_shift_em: baseline_percent / 100.0,
            tracking_adjust_em: tracking_percent / 100.0,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_renames_and_deletes_profiles() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        model.create_profile();
        assert_eq!(model.selected_profile().name, "新規プロファイル 1");

        model.rename_selected_profile("字幕").unwrap();
        assert_eq!(model.selected_profile().name, "字幕");

        model.delete_selected_profile().unwrap();
        assert_eq!(model.document().profiles.len(), 1);
        assert_eq!(model.selected_profile().name, "default");
    }

    #[test]
    fn rejects_duplicate_or_empty_profile_names() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        model.create_profile();
        assert!(model.rename_selected_profile("default").is_err());
        assert!(model.rename_selected_profile("  ").is_err());
    }

    #[test]
    fn keeps_the_default_profile_name() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        assert!(model.rename_selected_profile("字幕").is_err());
        assert!(model.delete_selected_profile().is_err());
    }

    #[test]
    fn updates_each_category_as_percent_values() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        model.select_category(4);
        model
            .update_selected_adjustment("DIN 2014".to_owned(), 115.0, -2.0, 3.5)
            .unwrap();

        let western = &model.selected_profile().western;
        assert_eq!(western.font_family, "DIN 2014");
        assert_eq!(western.size_ratio, 1.15);
        assert_eq!(western.baseline_shift_em, -0.02);
        assert_eq!(western.tracking_adjust_em, 0.035);
    }

    #[test]
    fn keeps_at_least_one_profile() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        assert!(model.delete_selected_profile().is_err());
    }
}
