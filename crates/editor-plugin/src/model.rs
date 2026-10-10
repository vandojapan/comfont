use compositefont_core::{
    CharacterClass, CompositeFontProfile, DEFAULT_PROFILE_NAME, FontAdjustment, MetricUnit,
    ProfileDocument, UnicodeRange,
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
        let mut model = Self {
            document,
            selected_profile: 0,
            selected_category: 0,
            selected_fallback: None,
        };
        model.select_table_row(0);
        model
    }

    pub fn document(&self) -> &ProfileDocument {
        &self.document
    }

    pub fn selected_profile_index(&self) -> usize {
        self.selected_profile
    }

    #[cfg(test)]
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
        static NEUTRAL: std::sync::OnceLock<FontAdjustment> = std::sync::OnceLock::new();
        self.adjustment_for_table_row(self.selected_table_row())
            .unwrap_or_else(|| NEUTRAL.get_or_init(FontAdjustment::neutral))
    }

    pub fn select_profile(&mut self, index: usize) -> bool {
        if index >= self.document.profiles.len() {
            return false;
        }
        self.selected_profile = index;
        self.selected_category = 0;
        self.selected_fallback = None;
        self.select_table_row(0);
        true
    }

    #[cfg(test)]
    pub fn select_category(&mut self, index: usize) -> bool {
        if index >= CATEGORY_ROWS.len() {
            return false;
        }
        self.selected_category = index;
        self.selected_fallback = None;
        true
    }

    pub fn add_selected_adjustment(&mut self) {
        if self.selected_category >= CATEGORY_ROWS.len() {
            let mut range = self.selected_profile().custom_ranges
                [self.selected_category - CATEGORY_ROWS.len()]
            .clone();
            range.name.push_str(" (コピー)");
            self.selected_profile_mut().custom_ranges.push(range);
            self.selected_category =
                CATEGORY_ROWS.len() + self.selected_profile().custom_ranges.len() - 1;
            return;
        }
        let class = CATEGORY_ROWS[self.selected_category].class;
        if !self.selected_profile().class_enabled(class) {
            self.restore_category(self.selected_category);
            return;
        }
        let mut adjustment = self.selected_adjustment().clone();
        adjustment.font_family.clear();
        adjustment.fallback_font_families.clear();
        self.selected_profile_mut()
            .fallbacks_for_mut(class)
            .push(adjustment);
        self.selected_fallback = Some(self.selected_profile().fallbacks_for(class).len() - 1);
    }

    #[cfg(any(test, feature = "egui-ui"))]
    pub fn copy_selected_adjustment(&mut self) -> bool {
        let Some(adjustment) = self
            .adjustment_for_table_row(self.selected_table_row())
            .cloned()
        else {
            return false;
        };
        self.add_selected_adjustment();
        if let Some((category, Some(index))) = self.table_row_target(self.selected_table_row()) {
            let class = CATEGORY_ROWS[category].class;
            self.selected_profile_mut().fallbacks_for_mut(class)[index] = adjustment;
        }
        true
    }

    pub fn remove_adjustments_for_table_rows(&mut self, rows: &[usize]) -> usize {
        let mut targets = rows
            .iter()
            .filter_map(|&row| self.table_row_target(row))
            .collect::<Vec<_>>();
        targets.sort_unstable();
        targets.dedup();
        let removed = targets.len();
        for (category, fallback) in targets.into_iter().rev() {
            if category >= CATEGORY_ROWS.len() {
                self.selected_profile_mut()
                    .custom_ranges
                    .remove(category - CATEGORY_ROWS.len());
                continue;
            }
            let class = CATEGORY_ROWS[category].class;
            let profile = self.selected_profile_mut();
            if let Some(index) = fallback {
                profile.fallbacks_for_mut(class).remove(index);
            } else if !profile.fallbacks_for(class).is_empty() {
                let next = profile.fallbacks_for_mut(class).remove(0);
                *profile.adjustment_for_mut(class) = next;
            } else {
                *profile.adjustment_for_mut(class) = FontAdjustment::neutral();
                profile.disabled_classes.push(class);
            }
        }
        self.selected_category = 0;
        self.selected_fallback = None;
        self.select_table_row(0);
        removed
    }

    pub fn can_move_table_rows(&self, rows: &[usize], direction: RowMoveDirection) -> bool {
        let targets = self.table_targets();
        rows.iter().any(|&row| {
            if row >= targets.len() {
                return false;
            }
            let adjacent = match direction {
                RowMoveDirection::Up => row.checked_sub(1),
                RowMoveDirection::Down => row.checked_add(1),
            };
            adjacent.is_some_and(|next| {
                next < targets.len()
                    && !rows.contains(&next)
                    && Self::same_move_group(targets[row], targets[next])
            })
        })
    }
    fn same_move_group(a: (usize, Option<usize>), b: (usize, Option<usize>)) -> bool {
        a.0 == b.0 || (a.0 >= CATEGORY_ROWS.len() && b.0 >= CATEGORY_ROWS.len())
    }
    fn swap_table_rows(&mut self, a: usize, b: usize) {
        let targets = self.table_targets();
        let (ac, af) = targets[a];
        let (bc, bf) = targets[b];
        if ac >= CATEGORY_ROWS.len() {
            self.selected_profile_mut()
                .custom_ranges
                .swap(ac - CATEGORY_ROWS.len(), bc - CATEGORY_ROWS.len());
            return;
        }
        let class = CATEGORY_ROWS[ac].class;
        let first = self.adjustment_for_table_row(a).unwrap().clone();
        let second = self.adjustment_for_table_row(b).unwrap().clone();
        let profile = self.selected_profile_mut();
        match af {
            None => *profile.adjustment_for_mut(class) = second,
            Some(index) => profile.fallbacks_for_mut(class)[index] = second,
        }
        match bf {
            None => *profile.adjustment_for_mut(class) = first,
            Some(index) => profile.fallbacks_for_mut(class)[index] = first,
        }
    }

    pub fn move_table_rows(&mut self, rows: &[usize], direction: RowMoveDirection) -> Vec<usize> {
        let targets = self.table_targets();
        let mut flags = (0..targets.len())
            .map(|row| rows.contains(&row))
            .collect::<Vec<_>>();
        let indices: Vec<usize> = match direction {
            RowMoveDirection::Up => (1..flags.len()).collect(),
            RowMoveDirection::Down => (0..flags.len().saturating_sub(1)).rev().collect(),
        };
        for index in indices {
            let next = match direction {
                RowMoveDirection::Up => index - 1,
                RowMoveDirection::Down => index + 1,
            };
            if flags[index] && !flags[next] && Self::same_move_group(targets[index], targets[next])
            {
                self.swap_table_rows(index, next);
                flags.swap(index, next);
            }
        }
        let selected = flags
            .iter()
            .enumerate()
            .filter_map(|(index, flag)| flag.then_some(index))
            .collect::<Vec<_>>();
        if let Some(&row) = selected.first() {
            self.select_table_row(row);
        }
        selected
    }

    pub fn selected_table_row(&self) -> usize {
        self.table_targets()
            .iter()
            .position(|&(category, fallback)| {
                category == self.selected_category && fallback == self.selected_fallback
            })
            .unwrap_or(0)
    }

    pub fn select_table_row(&mut self, table_row: usize) -> bool {
        let Some((category, fallback)) = self.table_row_target(table_row) else {
            return false;
        };
        self.selected_category = category;
        self.selected_fallback = fallback;
        true
    }

    pub fn adjustment_for_table_row(&self, table_row: usize) -> Option<&FontAdjustment> {
        let (category, fallback) = self.table_row_target(table_row)?;
        if category >= CATEGORY_ROWS.len() {
            return Some(
                &self.selected_profile().custom_ranges[category - CATEGORY_ROWS.len()].adjustment,
            );
        }
        let class = CATEGORY_ROWS[category].class;
        match fallback {
            Some(index) => self.selected_profile().fallbacks_for(class).get(index),
            None => Some(self.selected_profile().adjustment_for(class)),
        }
    }

    fn table_row_target(&self, table_row: usize) -> Option<(usize, Option<usize>)> {
        self.table_targets().get(table_row).copied()
    }

    fn table_targets(&self) -> Vec<(usize, Option<usize>)> {
        let mut targets = Vec::new();
        for (category, row) in CATEGORY_ROWS.iter().enumerate() {
            if !self.selected_profile().class_enabled(row.class) {
                continue;
            }
            targets.push((category, None));
            targets.extend(
                (0..self.selected_profile().fallbacks_for(row.class).len())
                    .map(|index| (category, Some(index))),
            );
        }
        targets.extend(
            (0..self.selected_profile().custom_ranges.len())
                .map(|index| (CATEGORY_ROWS.len() + index, None)),
        );
        targets
    }

    pub fn table_row_count(&self) -> usize {
        self.table_targets().len()
    }
    pub fn table_row_label(&self, row: usize) -> String {
        let Some((category, _)) = self.table_row_target(row) else {
            return "範囲なし".to_owned();
        };
        if category < CATEGORY_ROWS.len() {
            CATEGORY_ROWS[category].label.to_owned()
        } else {
            let range = &self.selected_profile().custom_ranges[category - CATEGORY_ROWS.len()];
            format!("{} U+{:04X}–{:04X}", range.name, range.start, range.end)
        }
    }
    pub fn selected_range(&self) -> Option<&UnicodeRange> {
        self.selected_category
            .checked_sub(CATEGORY_ROWS.len())
            .and_then(|index| self.selected_profile().custom_ranges.get(index))
    }
    pub fn add_range(&mut self, name: String, start: u32, end: u32) -> Result<(), String> {
        let range = UnicodeRange {
            name,
            start,
            end,
            adjustment: FontAdjustment::neutral(),
        };
        range.validate()?;
        self.selected_profile_mut().custom_ranges.push(range);
        self.selected_category =
            CATEGORY_ROWS.len() + self.selected_profile().custom_ranges.len() - 1;
        self.selected_fallback = None;
        Ok(())
    }
    pub fn update_selected_range(&mut self, start: u32, end: u32) -> Result<(), String> {
        let Some(index) = self.selected_category.checked_sub(CATEGORY_ROWS.len()) else {
            return Ok(());
        };
        let mut range = self.selected_profile().custom_ranges[index].clone();
        range.start = start;
        range.end = end;
        range.validate()?;
        self.selected_profile_mut().custom_ranges[index] = range;
        Ok(())
    }
    pub fn restore_category(&mut self, index: usize) {
        let Some(row) = CATEGORY_ROWS.get(index) else {
            return;
        };
        self.selected_profile_mut()
            .disabled_classes
            .retain(|&class| class != row.class);
        self.selected_category = index;
        self.selected_fallback = None;
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
            if category >= CATEGORY_ROWS.len() {
                self.selected_profile_mut().custom_ranges[category - CATEGORY_ROWS.len()]
                    .adjustment = adjustment.clone();
                continue;
            }
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

    #[test]
    fn copies_fonts_metrics_and_ranges_without_changing_the_source() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        model
            .update_table_rows(
                &[0],
                "Yu Gothic UI".into(),
                MetricUnit::Pixels,
                42.0,
                3.0,
                2.0,
                90.0,
                110.0,
            )
            .unwrap();
        let original = model.selected_adjustment().clone();
        assert!(model.copy_selected_adjustment());
        assert_eq!(model.selected_adjustment(), &original);
        assert_eq!(model.selected_profile().kanji, original);
        model.add_range("Latin".into(), 0x41, 0x5a).unwrap();
        assert!(model.copy_selected_adjustment());
        let ranges = &model.selected_profile().custom_ranges;
        assert_eq!(ranges.len(), 2);
        assert_eq!((ranges[1].start, ranges[1].end), (0x41, 0x5a));
        assert_eq!(ranges[1].adjustment, ranges[0].adjustment);
        model.remove_adjustments_for_table_rows(&(0..model.table_row_count()).collect::<Vec<_>>());
        assert!(!model.copy_selected_adjustment());
        assert_eq!(model.table_row_count(), 0);
    }

    #[test]
    fn deletes_all_fixed_and_custom_rows_and_can_restore_them() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        model.add_range("all".to_owned(), 0, 0x10FFFF).unwrap();
        let count = model.table_row_count();
        assert_eq!(
            model.remove_adjustments_for_table_rows(&(0..count).collect::<Vec<_>>()),
            count
        );
        assert_eq!(model.table_row_count(), 0);
        assert_eq!(model.selected_adjustment(), &FontAdjustment::neutral());
        for ch in ['A', 'あ', '漢', '😀'] {
            assert_eq!(model.selected_profile().resolve(ch).font_family, "");
        }
        let json = serde_json::to_string(model.document()).unwrap();
        let loaded = EditorModel::new(serde_json::from_str(&json).unwrap());
        assert_eq!(loaded.table_row_count(), 0);
        model.restore_category(1);
        assert_eq!(model.table_row_count(), 1);
        assert_eq!(model.selected_table_row(), 0);
        assert!(!model.can_move_table_rows(&[99], RowMoveDirection::Down));
        assert!(!model.can_move_table_rows(&[1], RowMoveDirection::Up));
    }
    #[test]
    fn deletes_primary_promotes_fallback_and_reorders_custom_ranges() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        model.add_selected_adjustment();
        model.selected_profile_mut().kanji_fallbacks[0].font_family = "Fallback".to_owned();
        model.remove_adjustments_for_table_rows(&[0]);
        assert_eq!(model.selected_profile().kanji.font_family, "Fallback");
        model.add_range("first".to_owned(), 0x41, 0x5A).unwrap();
        model.add_range("second".to_owned(), 0x41, 0x5A).unwrap();
        let row = model.selected_table_row();
        assert!(model.can_move_table_rows(&[row], RowMoveDirection::Up));
        model.move_table_rows(&[row], RowMoveDirection::Up);
        assert_eq!(model.selected_profile().custom_ranges[0].name, "second");
    }
}
