use compositefont_core::{
    CharacterClass, CompositeFontProfile, DEFAULT_PROFILE_NAME, FontAdjustment, MetricUnit,
    ProfileDocument,
};

pub const CATEGORY_ROWS: [CategoryRow; 7] = [
    CategoryRow::new(CharacterClass::Kanji, "漢字", "永国漢字髙﨑"),
    CategoryRow::new(CharacterClass::Hiragana, "かな", "あがぱゃゅょっ"),
    CategoryRow::new(CharacterClass::Katakana, "カナ", "アガパャュョッー"),
    CategoryRow::new(CharacterClass::Symbol, "記号", "。、・！？「」"),
    CategoryRow::new(CharacterClass::Western, "欧文", "TypeAaGgQq"),
    CategoryRow::new(CharacterClass::Digit, "数字", "0123456789"),
    CategoryRow::new(CharacterClass::Other, "その他", "ＡéЖΩ"),
];

pub const MIXED_PREVIEW_SAMPLES: [PreviewSample; 21] = [
    PreviewSample::new(CharacterClass::Kanji, "国"),
    PreviewSample::new(CharacterClass::Western, "A"),
    PreviewSample::new(CharacterClass::Hiragana, "あ"),
    PreviewSample::new(CharacterClass::Digit, "1"),
    PreviewSample::new(CharacterClass::Symbol, "、"),
    PreviewSample::new(CharacterClass::Katakana, "カ"),
    PreviewSample::new(CharacterClass::Other, "é"),
    PreviewSample::new(CharacterClass::Kanji, "漢"),
    PreviewSample::new(CharacterClass::Western, "B"),
    PreviewSample::new(CharacterClass::Hiragana, "い"),
    PreviewSample::new(CharacterClass::Digit, "2"),
    PreviewSample::new(CharacterClass::Symbol, "。"),
    PreviewSample::new(CharacterClass::Katakana, "ナ"),
    PreviewSample::new(CharacterClass::Other, "Ж"),
    PreviewSample::new(CharacterClass::Kanji, "字"),
    PreviewSample::new(CharacterClass::Western, "C"),
    PreviewSample::new(CharacterClass::Hiragana, "う"),
    PreviewSample::new(CharacterClass::Digit, "3"),
    PreviewSample::new(CharacterClass::Symbol, "！"),
    PreviewSample::new(CharacterClass::Katakana, "タ"),
    PreviewSample::new(CharacterClass::Other, "Ω"),
];

#[derive(Clone, Copy, Debug)]
pub struct CategoryRow {
    pub class: CharacterClass,
    pub label: &'static str,
    pub sample: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowMoveDirection {
    Up,
    Down,
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

#[derive(Clone, Copy, Debug)]
pub struct PreviewSample {
    pub class: CharacterClass,
    pub text: &'static str,
}

impl PreviewSample {
    const fn new(class: CharacterClass, text: &'static str) -> Self {
        Self { class, text }
    }
}

#[derive(Clone, Debug)]
pub struct EditorModel {
    document: ProfileDocument,
    selected_profile: usize,
    selected_category: usize,
    selected_fallback: Option<usize>,
}

impl EditorModel {
    pub fn new(mut document: ProfileDocument) -> Self {
        if document.profiles.is_empty() {
            document = ProfileDocument::with_builtin_default();
        }
        for profile in &mut document.profiles {
            migrate_legacy_kanji_fallbacks(profile);
        }
        Self {
            document,
            selected_profile: 0,
            selected_category: 0,
            selected_fallback: None,
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

    #[cfg(test)]
    pub fn selected_fallback_index(&self) -> Option<usize> {
        self.selected_fallback
    }

    pub fn selected_profile(&self) -> &CompositeFontProfile {
        &self.document.profiles[self.selected_profile]
    }

    pub fn selected_profile_mut(&mut self) -> &mut CompositeFontProfile {
        &mut self.document.profiles[self.selected_profile]
    }

    pub fn selected_adjustment(&self) -> &FontAdjustment {
        let class = CATEGORY_ROWS[self.selected_category].class;
        if let Some(index) = self.selected_fallback {
            &self.selected_profile().fallbacks_for(class)[index]
        } else {
            self.selected_profile().adjustment_for(class)
        }
    }

    pub fn select_profile(&mut self, index: usize) -> bool {
        if index >= self.document.profiles.len() {
            return false;
        }
        self.selected_profile = index;
        self.selected_fallback = None;
        true
    }

    pub fn select_category(&mut self, index: usize) -> bool {
        if index >= CATEGORY_ROWS.len() {
            return false;
        }
        self.selected_category = index;
        self.selected_fallback = None;
        true
    }

    pub fn select_fallback(&mut self, category: usize, index: usize) -> bool {
        let Some(row) = CATEGORY_ROWS.get(category) else {
            return false;
        };
        if index >= self.selected_profile().fallbacks_for(row.class).len() {
            return false;
        }
        self.selected_category = category;
        self.selected_fallback = Some(index);
        true
    }

    pub fn add_selected_adjustment(&mut self) {
        let category = self.selected_category;
        let class = CATEGORY_ROWS[category].class;
        let mut adjustment = self.selected_adjustment().clone();
        adjustment.font_family.clear();
        adjustment.fallback_font_families.clear();
        self.selected_profile_mut()
            .fallbacks_for_mut(class)
            .push(adjustment);
        self.selected_category = category;
        self.selected_fallback = Some(self.selected_profile().fallbacks_for(class).len() - 1);
    }

    pub fn remove_adjustments_for_table_rows(&mut self, rows: &[usize]) -> usize {
        let mut targets = rows
            .iter()
            .filter_map(|&row| self.table_row_target(row))
            .filter_map(|(category, fallback)| fallback.map(|index| (category, index)))
            .collect::<Vec<_>>();
        targets.sort_unstable();
        targets.dedup();
        let removed = targets.len();
        let selected_category = targets
            .first()
            .map(|(category, _)| *category)
            .unwrap_or(self.selected_category);
        for (category, index) in targets.into_iter().rev() {
            let class = CATEGORY_ROWS[category].class;
            self.selected_profile_mut()
                .fallbacks_for_mut(class)
                .remove(index);
        }
        if removed > 0 {
            self.selected_fallback = None;
            self.selected_category = selected_category;
        }
        removed
    }

    pub fn can_move_table_rows(&self, rows: &[usize], direction: RowMoveDirection) -> bool {
        self.selected_row_flags(rows)
            .into_iter()
            .any(|selected| match direction {
                RowMoveDirection::Up => {
                    (1..selected.len()).any(|index| selected[index] && !selected[index - 1])
                }
                RowMoveDirection::Down => (0..selected.len().saturating_sub(1))
                    .any(|index| selected[index] && !selected[index + 1]),
            })
    }

    pub fn move_table_rows(&mut self, rows: &[usize], direction: RowMoveDirection) -> Vec<usize> {
        let mut selected_by_category = self.selected_row_flags(rows);
        let mut moved = false;

        for (category, selected) in selected_by_category.iter_mut().enumerate() {
            let class = CATEGORY_ROWS[category].class;
            let mut adjustments = std::iter::once(self.selected_profile().adjustment_for(class))
                .chain(self.selected_profile().fallbacks_for(class))
                .cloned()
                .collect::<Vec<_>>();
            let mut group_moved = false;
            match direction {
                RowMoveDirection::Up => {
                    for index in 1..selected.len() {
                        if selected[index] && !selected[index - 1] {
                            adjustments.swap(index, index - 1);
                            selected.swap(index, index - 1);
                            group_moved = true;
                        }
                    }
                }
                RowMoveDirection::Down => {
                    for index in (0..selected.len().saturating_sub(1)).rev() {
                        if selected[index] && !selected[index + 1] {
                            adjustments.swap(index, index + 1);
                            selected.swap(index, index + 1);
                            group_moved = true;
                        }
                    }
                }
            }
            if group_moved {
                let profile = self.selected_profile_mut();
                *profile.adjustment_for_mut(class) = adjustments.remove(0);
                *profile.fallbacks_for_mut(class) = adjustments;
                moved = true;
            }
        }

        if !moved {
            return rows.to_vec();
        }

        let mut moved_rows = Vec::new();
        let mut first_row = 0;
        for (category, selected) in selected_by_category.iter().enumerate() {
            moved_rows.extend(
                selected
                    .iter()
                    .enumerate()
                    .filter(|(_, selected)| **selected)
                    .map(|(index, _)| first_row + index),
            );
            first_row += 1 + self
                .selected_profile()
                .fallbacks_for(CATEGORY_ROWS[category].class)
                .len();
        }
        if let Some(&first) = moved_rows.first() {
            self.select_table_row(first);
        }
        moved_rows
    }

    pub fn selected_table_row(&self) -> usize {
        let preceding_rows = CATEGORY_ROWS[..self.selected_category]
            .iter()
            .map(|row| 1 + self.selected_profile().fallbacks_for(row.class).len())
            .sum::<usize>();
        preceding_rows + self.selected_fallback.map_or(0, |index| index + 1)
    }

    pub fn select_table_row(&mut self, table_row: usize) -> bool {
        let Some((category, fallback)) = self.table_row_target(table_row) else {
            return false;
        };
        match fallback {
            Some(index) => self.select_fallback(category, index),
            None => self.select_category(category),
        }
    }

    pub fn adjustment_for_table_row(&self, table_row: usize) -> Option<&FontAdjustment> {
        let (category, fallback) = self.table_row_target(table_row)?;
        let class = CATEGORY_ROWS[category].class;
        match fallback {
            Some(index) => self.selected_profile().fallbacks_for(class).get(index),
            None => Some(self.selected_profile().adjustment_for(class)),
        }
    }

    pub fn is_additional_table_row(&self, table_row: usize) -> bool {
        self.table_row_target(table_row)
            .is_some_and(|(_, fallback)| fallback.is_some())
    }

    fn table_row_target(&self, table_row: usize) -> Option<(usize, Option<usize>)> {
        let mut first_row = 0;
        for (category, row) in CATEGORY_ROWS.iter().enumerate() {
            let fallback_count = self.selected_profile().fallbacks_for(row.class).len();
            if table_row == first_row {
                return Some((category, None));
            }
            if table_row <= first_row + fallback_count {
                return Some((category, Some(table_row - first_row - 1)));
            }
            first_row += 1 + fallback_count;
        }
        None
    }

    fn selected_row_flags(&self, rows: &[usize]) -> Vec<Vec<bool>> {
        let mut selected = CATEGORY_ROWS
            .iter()
            .map(|row| vec![false; 1 + self.selected_profile().fallbacks_for(row.class).len()])
            .collect::<Vec<_>>();
        for &row in rows {
            if let Some((category, fallback)) = self.table_row_target(row) {
                selected[category][fallback.map_or(0, |index| index + 1)] = true;
            }
        }
        selected
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
        self.selected_fallback = None;
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
        self.selected_fallback = None;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_table_rows(
        &mut self,
        rows: &[usize],
        font_family: String,
        metric_unit: MetricUnit,
        size_percent: f64,
        baseline_percent: f64,
        tracking_percent: f64,
        vertical_scale_percent: f64,
        horizontal_scale_percent: f64,
    ) -> Result<(), String> {
        let adjustment = make_adjustment(
            font_family,
            metric_unit,
            size_percent,
            baseline_percent,
            tracking_percent,
            vertical_scale_percent,
            horizontal_scale_percent,
        )?;
        for &row in rows {
            let Some((category, fallback)) = self.table_row_target(row) else {
                continue;
            };
            let class = CATEGORY_ROWS[category].class;
            let profile = self.selected_profile_mut();
            let target = match fallback {
                Some(index) => profile.fallbacks_for_mut(class).get_mut(index),
                None => Some(profile.adjustment_for_mut(class)),
            };
            if let Some(target) = target {
                *target = adjustment.clone();
            }
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn make_adjustment(
    font_family: String,
    metric_unit: MetricUnit,
    size_percent: f64,
    baseline_percent: f64,
    tracking_percent: f64,
    vertical_scale_percent: f64,
    horizontal_scale_percent: f64,
) -> Result<FontAdjustment, String> {
    for (label, value) in [
        ("サイズ", size_percent),
        ("ベースライン", baseline_percent),
        ("字送り", tracking_percent),
        ("垂直比率", vertical_scale_percent),
        ("水平比率", horizontal_scale_percent),
    ] {
        if !value.is_finite() {
            return Err(format!("{label}には有限の数値を入力してください。"));
        }
    }
    if size_percent <= 0.0 {
        return Err("サイズは0より大きい値にしてください。".to_owned());
    }
    if vertical_scale_percent <= 0.0 || horizontal_scale_percent <= 0.0 {
        return Err("垂直比率と水平比率は0より大きい値にしてください。".to_owned());
    }

    let (
        size_ratio,
        baseline_shift_em,
        tracking_adjust_em,
        size_px,
        baseline_shift_px,
        tracking_adjust_px,
    ) = match metric_unit {
        MetricUnit::Percent => (
            size_percent / 100.0,
            baseline_percent / 100.0,
            tracking_percent / 100.0,
            None,
            None,
            None,
        ),
        MetricUnit::Pixels => (
            1.0,
            0.0,
            0.0,
            Some(size_percent),
            Some(baseline_percent),
            Some(tracking_percent),
        ),
    };

    Ok(FontAdjustment {
        font_family: font_family.trim().to_owned(),
        fallback_font_families: Vec::new(),
        size_ratio,
        baseline_shift_em,
        tracking_adjust_em,
        metric_unit,
        size_px,
        baseline_shift_px,
        tracking_adjust_px,
        vertical_scale_ratio: vertical_scale_percent / 100.0,
        horizontal_scale_ratio: horizontal_scale_percent / 100.0,
    })
}

fn migrate_legacy_kanji_fallbacks(profile: &mut CompositeFontProfile) {
    let legacy = std::mem::take(&mut profile.kanji.fallback_font_families);
    for family in legacy {
        if family.is_empty()
            || family == profile.kanji.font_family
            || profile
                .kanji_fallbacks
                .iter()
                .any(|adjustment| adjustment.font_family == family)
        {
            continue;
        }
        let mut adjustment = profile.kanji.clone();
        adjustment.font_family = family;
        profile.kanji_fallbacks.push(adjustment);
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
            .update_table_rows(
                &[4],
                "DIN 2014".to_owned(),
                MetricUnit::Percent,
                115.0,
                -2.0,
                3.5,
                92.0,
                108.0,
            )
            .unwrap();

        let western = &model.selected_profile().western;
        assert_eq!(western.font_family, "DIN 2014");
        assert_eq!(western.size_ratio, 1.15);
        assert_eq!(western.baseline_shift_em, -0.02);
        assert_eq!(western.tracking_adjust_em, 0.035);
        assert_eq!(western.vertical_scale_ratio, 0.92);
        assert_eq!(western.horizontal_scale_ratio, 1.08);
    }

    #[test]
    fn stores_pixel_metrics_without_changing_fixed_scale_ratios() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        model
            .update_table_rows(
                &[0],
                "Pixel Font".to_owned(),
                MetricUnit::Pixels,
                48.0,
                3.0,
                -2.0,
                90.0,
                110.0,
            )
            .unwrap();

        let adjustment = &model.selected_profile().kanji;
        assert_eq!(adjustment.metric_unit, MetricUnit::Pixels);
        assert_eq!(adjustment.size_px, Some(48.0));
        assert_eq!(adjustment.baseline_shift_px, Some(3.0));
        assert_eq!(adjustment.tracking_adjust_px, Some(-2.0));
        assert_eq!(adjustment.vertical_scale_ratio, 0.9);
        assert_eq!(adjustment.horizontal_scale_ratio, 1.1);
    }

    #[test]
    fn adds_updates_and_removes_ordered_kanji_rows() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        model
            .update_table_rows(
                &[0],
                "A-OTF Gothic Std".to_owned(),
                MetricUnit::Percent,
                100.0,
                0.0,
                0.0,
                100.0,
                100.0,
            )
            .unwrap();
        model.add_selected_adjustment();
        model
            .update_table_rows(
                &[1],
                "A-OTF Gothic Pr6".to_owned(),
                MetricUnit::Percent,
                105.0,
                1.0,
                2.0,
                98.0,
                102.0,
            )
            .unwrap();
        assert_eq!(
            model.selected_profile().kanji.font_family,
            "A-OTF Gothic Std"
        );
        assert_eq!(model.selected_profile().kanji_fallbacks.len(), 1);
        assert_eq!(
            model.selected_profile().kanji_fallbacks[0].font_family,
            "A-OTF Gothic Pr6"
        );
        assert_eq!(model.selected_profile().kanji_fallbacks[0].size_ratio, 1.05);
        assert_eq!(model.remove_adjustments_for_table_rows(&[1]), 1);
        assert!(model.selected_profile().kanji_fallbacks.is_empty());
    }

    #[test]
    fn adds_the_same_character_class_as_the_selected_row() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        assert!(model.select_category(1));

        model.add_selected_adjustment();

        assert_eq!(model.selected_category_index(), 1);
        assert_eq!(model.selected_fallback_index(), Some(0));
        assert_eq!(model.selected_profile().hiragana_fallbacks.len(), 1);
        assert!(model.selected_profile().kanji_fallbacks.is_empty());
        assert_eq!(model.selected_table_row(), 2);
    }

    #[test]
    fn moves_multiple_fallback_rows_as_an_ordered_block() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        for name in ["A", "B", "C"] {
            model.add_selected_adjustment();
            model
                .selected_profile_mut()
                .kanji_fallbacks
                .last_mut()
                .unwrap()
                .font_family = name.to_owned();
        }

        assert!(model.can_move_table_rows(&[2, 3], RowMoveDirection::Up));
        assert_eq!(
            model.move_table_rows(&[2, 3], RowMoveDirection::Up),
            vec![1, 2]
        );
        assert_eq!(
            model
                .selected_profile()
                .kanji_fallbacks
                .iter()
                .map(|adjustment| adjustment.font_family.as_str())
                .collect::<Vec<_>>(),
            vec!["B", "C", "A"]
        );

        assert!(model.can_move_table_rows(&[1, 2], RowMoveDirection::Down));
        assert_eq!(
            model.move_table_rows(&[1, 2], RowMoveDirection::Down),
            vec![2, 3]
        );
        assert_eq!(
            model
                .selected_profile()
                .kanji_fallbacks
                .iter()
                .map(|adjustment| adjustment.font_family.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "B", "C"]
        );
    }

    #[test]
    fn moves_the_primary_row_and_stays_inside_group_edges() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        model.selected_profile_mut().kanji.font_family = "Primary".to_owned();
        model.add_selected_adjustment();
        model
            .selected_profile_mut()
            .kanji_fallbacks
            .last_mut()
            .unwrap()
            .font_family = "A".to_owned();
        model.add_selected_adjustment();
        model
            .selected_profile_mut()
            .kanji_fallbacks
            .last_mut()
            .unwrap()
            .font_family = "B".to_owned();

        assert!(!model.can_move_table_rows(&[0], RowMoveDirection::Up));
        assert!(model.can_move_table_rows(&[0], RowMoveDirection::Down));
        assert_eq!(model.move_table_rows(&[0], RowMoveDirection::Down), vec![1]);
        assert_eq!(model.selected_profile().kanji.font_family, "A");
        assert_eq!(
            model.selected_profile().kanji_fallbacks[0].font_family,
            "Primary"
        );
        assert!(model.can_move_table_rows(&[1], RowMoveDirection::Up));
        assert!(!model.can_move_table_rows(&[2], RowMoveDirection::Down));
        assert_eq!(model.move_table_rows(&[1], RowMoveDirection::Up), vec![0]);
        assert_eq!(model.selected_profile().kanji.font_family, "Primary");
    }

    #[test]
    fn migrates_legacy_font_name_fallbacks_to_kanji_rows() {
        let mut document = ProfileDocument::with_builtin_default();
        document.profiles[0].kanji.font_family = "Std".to_owned();
        document.profiles[0].kanji.fallback_font_families =
            vec!["Pr6".to_owned(), "Pr6N".to_owned()];

        let model = EditorModel::new(document);

        assert!(
            model
                .selected_profile()
                .kanji
                .fallback_font_families
                .is_empty()
        );
        assert_eq!(
            model
                .selected_profile()
                .kanji_fallbacks
                .iter()
                .map(|adjustment| adjustment.font_family.as_str())
                .collect::<Vec<_>>(),
            vec!["Pr6", "Pr6N"]
        );
    }

    #[test]
    fn updates_multiple_table_rows_with_one_adjustment() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        model.add_selected_adjustment();
        model
            .update_table_rows(
                &[1, 5],
                "Shared Font".to_owned(),
                MetricUnit::Percent,
                110.0,
                -1.0,
                2.0,
                95.0,
                105.0,
            )
            .unwrap();

        let fallback = &model.selected_profile().kanji_fallbacks[0];
        let western = &model.selected_profile().western;
        for adjustment in [fallback, western] {
            assert_eq!(adjustment.font_family, "Shared Font");
            assert_eq!(adjustment.size_ratio, 1.1);
            assert_eq!(adjustment.tracking_adjust_em, 0.02);
        }
    }

    #[test]
    fn keeps_at_least_one_profile() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        assert!(model.delete_selected_profile().is_err());
    }

    #[test]
    fn preview_samples_only_contain_their_assigned_character_class() {
        for row in CATEGORY_ROWS {
            for character in row.sample.chars() {
                assert_eq!(
                    compositefont_core::classify_character(character),
                    row.class,
                    "{character:?} in the {} preview sample",
                    row.label
                );
            }
        }
    }

    #[test]
    fn mixed_preview_alternates_valid_character_classes() {
        for (index, sample) in MIXED_PREVIEW_SAMPLES.iter().enumerate() {
            assert_eq!(sample.text.chars().count(), 1);
            assert_eq!(
                compositefont_core::classify_character(sample.text.chars().next().unwrap()),
                sample.class
            );
            if let Some(previous) = index.checked_sub(1) {
                assert_ne!(MIXED_PREVIEW_SAMPLES[previous].class, sample.class);
            }
        }
    }
}
