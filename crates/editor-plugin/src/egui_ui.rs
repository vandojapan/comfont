use crate::{
    font_collection::FontRegistrationState,
    model::{AdjustmentPatch, CATEGORY_ROWS, EditorModel, RowMoveDirection, adjustment_values},
    storage,
};
use aviutl2::generic::EditHandle;
use compositefont_core::{MetricUnit, ProfileDocument, UNICODE_PRESETS, parse_codepoint};
use eframe::egui::{self, AtomExt as _};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use winit::platform::windows::EventLoopBuilderExtWindows;

#[path = "egui_font_picker.rs"]
mod font_picker;
use font_picker::FontPicker;
#[path = "egui_selection.rs"]
mod selection;
use selection::RowSelection;

// Google Material Icons (Apache-2.0; bundled LICENSE.txt).
fn icon_label(icon: char, label: &str) -> egui::WidgetText {
    material_label(icon, label, "MaterialIcons")
}

fn material_label(icon: char, label: &str, family: &'static str) -> egui::WidgetText {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &icon.to_string(),
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(20.0, egui::FontFamily::Name(family.into())),
            color: egui::Color32::from_rgb(224, 228, 234),
            ..Default::default()
        },
    );
    if !label.is_empty() {
        job.append(
            &format!("  {label}"),
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(17.0),
                color: egui::Color32::from_rgb(224, 228, 234),
                ..Default::default()
            },
        );
    }
    job.into()
}

fn left_selectable(selected: bool, label: &str) -> egui::Button<'_> {
    egui::Button::selectable(selected, (label.atom_shrink(true), egui::Atom::grow())).truncate()
}

fn section_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(60, 64, 69))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .corner_radius(4.0)
}

fn section_title(ui: &mut egui::Ui, title: &str) -> egui::Response {
    section_frame()
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.heading(title);
        })
        .response
}

/// Keep the header's child layouts within one row instead of the whole sidebar.
fn profile_header(ui: &mut egui::Ui) -> egui::InnerResponse<egui::Response> {
    section_frame().show(ui, |ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 32.0),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                let add = ui.add_sized([32.0, 32.0], egui::Button::new(icon_label('\u{e145}', "")));
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.heading("プロファイル");
                });
                add
            },
        )
        .inner
    })
}

fn preset_row(ui: &mut egui::Ui, index: usize, selected: usize) -> egui::Response {
    let preset = &UNICODE_PRESETS[index];
    ui.add_sized(
        [ui.available_width(), 28.0],
        left_selectable(
            index == selected,
            &format!("{}  U+{:04X}–{:04X}", preset.name, preset.start, preset.end),
        ),
    )
}

fn metric_input(ui: &mut egui::Ui, value: &mut f64, index: usize) -> egui::Response {
    let positive = matches!(index, 0 | 3 | 4);
    let mut widget = egui::DragValue::new(value).speed(0.1);
    if positive {
        widget = widget.range(0.1..=f64::MAX);
    }
    let mut response = ui.add_sized([72.0, 28.0], widget);
    if response.hovered() {
        let delta = ui.input_mut(|i| {
            // Consume raw wheel once, before the enclosing ScrollArea processes it.
            let mut delta = 0.0;
            i.events.retain(|event| {
                if let egui::Event::MouseWheel {
                    unit, delta: wheel, ..
                } = event
                {
                    delta += match unit {
                        egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => wheel.y,
                        egui::MouseWheelUnit::Point => wheel.y / 40.0,
                    };
                    false
                } else {
                    true
                }
            });
            i.raw_scroll_delta.y = 0.0;
            i.smooth_scroll_delta.y = 0.0;
            delta
        });
        if delta != 0.0 {
            ui.memory_mut(|m| m.surrender_focus(response.id));
            let step = ui.input(|i| {
                if i.modifiers.shift {
                    0.1
                } else if i.modifiers.ctrl {
                    10.0
                } else {
                    1.0
                }
            });
            *value = ((*value + f64::from(delta) * step) * 10.0).round() / 10.0;
            if positive {
                *value = value.max(0.1);
            }
            response.mark_changed();
            ui.ctx().request_repaint();
        }
    }
    response
}

// Scope DPI awareness to this UI thread; never change the host process setting.
struct ThreadDpiScope(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT);
impl ThreadDpiScope {
    fn enter() -> Self {
        use windows::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
        };
        // SAFETY: this changes only the calling UI thread and is restored by Drop.
        Self(unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) })
    }
}
impl Drop for ThreadDpiScope {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            // SAFETY: restore the context returned for this same thread.
            unsafe {
                windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(self.0);
            }
        }
    }
}

struct EditorApp {
    model: EditorModel,
    path: PathBuf,
    saved: Arc<Mutex<ProfileDocument>>,
    handle: Arc<EditHandle>,
    fonts: Vec<String>,
    registration: Arc<Mutex<FontRegistrationState>>,
    profile_name: String,
    font: String,
    font_picker: FontPicker,
    unit: MetricUnit,
    values: [f64; 5],
    selection: RowSelection,
    changed_fields: [bool; 7],
    editing_profile: bool,
    focus_profile_editor: bool,
    range_dialog: Option<RangeDialog>,
    start: String,
    end: String,
    sample: String,
    preview: Option<egui::TextureHandle>,
    preview_dirty: bool,
    scroll_selected_row: bool,
    guides: bool,
    status: String,
    hover_help: Option<String>,
}
impl EditorApp {
    fn explain(&mut self, response: egui::Response, text: &str) -> egui::Response {
        if response.contains_pointer() {
            self.hover_help = Some(text.to_owned());
        }
        response
    }
    fn load_fields(&mut self) {
        self.profile_name = self.model.selected_profile().name.clone();
        let adjustment = self.model.selected_adjustment();
        self.font = adjustment.font_family.clone();
        self.unit = adjustment.metric_unit;
        self.values = adjustment_values(adjustment);
        self.changed_fields = [false; 7];
        if let Some(range) = self.model.selected_range() {
            self.start = format!("{:04X}", range.start);
            self.end = format!("{:04X}", range.end);
        }
        self.scroll_selected_row = true;
    }
    fn reset_selection(&mut self) {
        self.selection.single(
            self.model.selected_table_row(),
            self.model.table_row_count(),
        );
        self.load_fields();
        self.preview_dirty = true;
    }
    fn apply(&mut self) -> Result<(), String> {
        // Validate all editor fields before committing any changes.
        let mut candidate = self.model.clone();
        candidate.rename_selected_profile(&self.profile_name)?;
        if self.selection.rows.len() == 1 && candidate.selected_range().is_some() {
            candidate.update_selected_range(
                parse_codepoint(&self.start)?,
                parse_codepoint(&self.end)?,
            )?;
        }
        if self.changed_fields.iter().any(|&changed| changed) {
            let patch = AdjustmentPatch {
                font_family: self.changed_fields[0].then(|| self.font.clone()),
                metric_unit: self.changed_fields[1].then_some(self.unit),
                values: std::array::from_fn(|index| {
                    (self.changed_fields[index + 2] || (index < 3 && self.changed_fields[1]))
                        .then_some(self.values[index])
                }),
            };
            candidate.patch_table_rows(&self.selection.rows, &patch)?;
        }
        self.preview_dirty |= self.model.document() != candidate.document();
        self.model = candidate;
        self.changed_fields = [false; 7];
        Ok(())
    }
    fn save(&mut self) -> Result<(), String> {
        self.apply()?;
        storage::save_document_with_selector(&self.path, self.model.document())?;
        *self
            .saved
            .lock()
            .map_err(|_| "保存状態を更新できません。")? = self.model.document().clone();
        let outcome = self
            .registration
            .lock()
            .map_err(|_| "フォント状態を取得できません。")?
            .stage_fonts_for_next_launch()?;
        self.status = if outcome.added_file_count > 0 {
            format!(
                "保存しました。追加フォント{}件とanm2の選択肢はAviUtl2の再起動後に反映されます。",
                outcome.added_file_count
            )
        } else {
            "保存しました。anm2の選択肢は再起動後に反映されます。".to_owned()
        };
        Ok(())
    }
    fn update_preview(&mut self, ctx: &egui::Context) {
        if !self.preview_dirty {
            return;
        }
        self.preview_dirty = false;
        let profile = self.model.selected_profile().clone();
        let text = self.sample.clone();
        let guides = self.guides;
        match self.handle.call_read_section(move |section| {
            compositefont_module::preview::render_preview(
                section,
                &profile,
                &text,
                compositefont_module::preview::PreviewOptions {
                    size: 32.0,
                    width: 880,
                    height: 110,
                    baseline_guide: guides,
                    bounds_guide: guides,
                },
            )
        }) {
            Ok(Ok(bitmap)) => {
                let mut rgba = bitmap.bgra;
                for pixel in rgba.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [bitmap.width as usize, bitmap.height as usize],
                    &rgba,
                );
                if let Some(texture) = &mut self.preview {
                    texture.set(image, egui::TextureOptions::LINEAR);
                } else {
                    self.preview = Some(ctx.load_texture(
                        "comfont-preview",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            result => {
                self.preview = None;
                self.status = match result {
                    Err(e) => e.to_string(),
                    Ok(Err(e)) => e,
                    _ => unreachable!(),
                };
            }
        }
    }
}
#[derive(Clone)]
struct RangeDialog {
    custom: bool,
    preset: usize,
    preset_search: String,
    focus_preset_search: bool,
    name: String,
    start: String,
    end: String,
    error: String,
}
impl Default for RangeDialog {
    fn default() -> Self {
        Self {
            custom: false,
            preset: 0,
            preset_search: String::new(),
            focus_preset_search: true,
            name: "カスタム範囲".into(),
            start: format!("{:04X}", UNICODE_PRESETS[0].start),
            end: format!("{:04X}", UNICODE_PRESETS[0].end),
            error: String::new(),
        }
    }
}
impl EditorApp {
    fn select_profile(&mut self, index: usize) {
        if index == self.model.selected_profile_index() {
            return;
        }
        match self.apply() {
            Ok(()) => {
                self.model.select_profile(index);
                self.editing_profile = false;
                self.reset_selection();
            }
            Err(e) => self.status = e,
        }
    }
    fn delete_profile(&mut self) {
        match self.model.delete_selected_profile() {
            Ok(()) => {
                self.editing_profile = false;
                self.reset_selection();
            }
            Err(e) => self.status = e,
        }
    }
    fn commit_profile_name(&mut self) {
        match self.model.rename_selected_profile(&self.profile_name) {
            Ok(()) => {
                self.profile_name = self.model.selected_profile().name.clone();
                self.editing_profile = false;
            }
            Err(e) => self.status = e,
        }
    }
    fn profiles_ui(&mut self, ui: &mut egui::Ui) {
        if self
            .explain(
                profile_header(ui).inner,
                "新規プロファイルを作成し、名前を編集します。",
            )
            .clicked()
        {
            match self.apply() {
                Ok(()) => {
                    self.model.create_profile();
                    self.editing_profile = true;
                    self.focus_profile_editor = true;
                    self.reset_selection();
                }
                Err(e) => self.status = e,
            }
        }
        ui.add_space(12.0);
        let names: Vec<_> = self
            .model
            .document()
            .profiles
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let list_height = ui.available_height().max(32.0);
        egui::ScrollArea::vertical()
            .id_salt("profile-list")
            .max_height(list_height)
            .show(ui, |ui| {
                for (index, name) in names.iter().enumerate() {
                    let selected = index == self.model.selected_profile_index();
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 88.0).max(80.0);
                        if selected && self.editing_profile {
                            let response = ui.add_sized(
                                [width, 32.0],
                                egui::TextEdit::singleline(&mut self.profile_name)
                                    .desired_width(width)
                                    .background_color(egui::Color32::WHITE)
                                    .text_color(egui::Color32::BLACK),
                            );
                            self.explain(response.clone(), "プロファイル名を編集します。Enterで確定、Escapeで取り消します。");
                            if self.focus_profile_editor {
                                response.request_focus();
                                self.focus_profile_editor = false;
                            }
                            if response.has_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Escape))
                            {
                                self.profile_name = self.model.selected_profile().name.clone();
                                self.editing_profile = false;
                            } else if response.lost_focus()
                                || (response.has_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                            {
                                self.commit_profile_name();
                            }
                        } else {
                            let response =
                                ui.add_sized([width, 32.0], left_selectable(selected, name));
                            self.explain(response.clone(), "プロファイルを選択します。名前はダブルクリックで編集できます。defaultの名前は固定です。");
                            if response.clicked() {
                                self.select_profile(index);
                            }
                            if response.double_clicked()
                                && name != "default"
                                && self.model.selected_profile_index() == index
                            {
                                self.editing_profile = true;
                                self.focus_profile_editor = true;
                            }
                        }
                        if selected {
                            if self.explain(ui.add_enabled(name != "default", egui::Button::new(icon_label('\u{e3c9}', ""))), "プロファイル名を編集します。defaultの名前は固定です。").clicked()
                            {
                                self.editing_profile = true;
                                self.focus_profile_editor = true;
                            }
                            if self.explain(ui.add_enabled(name != "default", egui::Button::new(icon_label('\u{e92e}', ""))), "選択したプロファイルを削除します。defaultは削除できません。").clicked()
                            {
                                self.delete_profile();
                            }
                        }
                    });
                }
            });
    }
    fn preview_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            self.explain(
                ui.label("プレビュー本文"),
                "プレビュー専用の本文です。テキストオブジェクトの本文は変更しません。",
            );
            let sample = ui.add(
                egui::TextEdit::singleline(&mut self.sample)
                    .desired_width((ui.available_width() - 120.0).min(460.0)),
            );
            if self
                .explain(
                    sample,
                    "プレビュー専用の本文です。テキストオブジェクトの本文は変更しません。",
                )
                .changed()
            {
                self.preview_dirty = true;
            }
            let guides = ui.checkbox(&mut self.guides, "ガイド");
            if self
                .explain(guides, "文字の境界とベースラインのガイドを表示します。")
                .changed()
            {
                self.preview_dirty = true;
            }
        });
        self.update_preview(ctx);
        if let Some(texture) = &self.preview {
            ui.add(egui::Image::new(texture).max_width(ui.available_width()));
        }
        ui.add_space(8.0);
    }
    fn fonts_ui(&mut self, ui: &mut egui::Ui) {
        self.explain(section_title(ui, "フォント"), "カスタム範囲を優先し、重複時は上から最初に一致した範囲を使います。全範囲を削除すると無装飾です。");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if self
                .explain(
                    ui.button(icon_label('\u{e145}', "追加")),
                    "Unicodeプリセットまたはカスタム範囲を追加します。削除した分類も復元できます。",
                )
                .clicked()
            {
                match self.apply() {
                    Ok(()) => self.range_dialog = Some(RangeDialog::default()),
                    Err(e) => self.status = e,
                }
            }
            if self
                .explain(
                    ui.add_enabled(
                        !self.selection.rows.is_empty(),
                        egui::Button::new(icon_label('\u{e92e}', "削除")),
                    ),
                    "選択した範囲・分類を削除します。全範囲を削除すると無装飾です。",
                )
                .clicked()
            {
                self.model
                    .remove_adjustments_for_table_rows(&self.selection.rows);
                self.reset_selection();
            }
            if self
                .explain(
                    ui.add_enabled(
                        !self.selection.rows.is_empty(),
                        egui::Button::new(icon_label('\u{e14d}', "コピー")),
                    ),
                    "選択行のフォントと補正値をコピーします。",
                )
                .clicked()
            {
                match self.apply() {
                    Ok(()) => {
                        let rows = self.model.copy_table_rows(&self.selection.rows);
                        self.selection.replace(rows);
                        self.load_fields();
                        self.preview_dirty = true;
                    }
                    Err(e) => self.status = e,
                }
            }
            for (icon, direction, tooltip) in [
                ('\u{e5d8}', RowMoveDirection::Up, "上へ移動"),
                ('\u{e5db}', RowMoveDirection::Down, "下へ移動"),
            ] {
                if self
                    .explain(
                        ui.add_enabled(
                            self.model
                                .can_move_table_rows(&self.selection.rows, direction),
                            egui::Button::new(icon_label(icon, "")),
                        ),
                        tooltip,
                    )
                    .clicked()
                {
                    match self.apply() {
                        Ok(()) => {
                            let rows = self.model.move_table_rows(&self.selection.rows, direction);
                            self.selection.replace(rows);
                            self.load_fields();
                            self.preview_dirty = true;
                        }
                        Err(e) => self.status = e,
                    }
                }
            }
        });
        let height = (ui.available_height() * 0.40).clamp(170.0, 280.0);
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(43, 46, 50))
            .inner_margin(10.0)
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt("font-ranges")
                    .max_height(height)
                    .min_scrolled_height(height)
                    .show(ui, |ui| {
                        if self.model.table_row_count() == 0 {
                            ui.weak("範囲がありません。「追加」から登録してください。");
                        }
                        ui.spacing_mut().item_spacing.y = 4.0;
                        for row in 0..self.model.table_row_count() {
                            if row > 0 {
                                ui.add(egui::Separator::default().spacing(1.0));
                            }
                            let adjustment = self.model.adjustment_for_table_row(row).unwrap();
                            let font = if adjustment.font_family.is_empty() {
                                "（変更なし）"
                            } else {
                                &adjustment.font_family
                            };
                            let label = format!("{}    {}", self.model.table_row_label(row), font);
                            let response = ui.add_sized(
                                [ui.available_width(), 32.0],
                                left_selectable(self.selection.rows.contains(&row), &label),
                            );
                            self.explain(response.clone(), "カスタム範囲を優先し、重複時は上から最初に一致した範囲を使います。Shiftで範囲選択、Ctrlで追加・解除。通常クリックで複数選択を解除し、その行を編集します。");
                            if self.scroll_selected_row && row == self.model.selected_table_row() {
                                response.scroll_to_me(Some(egui::Align::Center));
                                self.scroll_selected_row = false;
                            }
                            if response.clicked() {
                                match self.apply() {
                                    Ok(()) => {
                                        let modifiers = ui.input(|i| i.modifiers);
                                        self.selection.click(row, self.model.table_row_count(), modifiers.shift, modifiers.ctrl);
                                        if let Some(active) = self.selection.rows.iter().copied().find(|&r| r == row)
                                            .or_else(|| self.selection.rows.first().copied()) {
                                            self.model.select_table_row(active);
                                        }
                                        self.load_fields();
                                        self.scroll_selected_row = false;
                                    }
                                    Err(e) => self.status = e,
                                }
                            }
                        }
                    });
            });
        ui.add_space(16.0);
        if self.selection.rows.is_empty() {
            return;
        }
        let multiple = self.selection.rows.len() > 1;
        let title = if multiple {
            format!("({}つ選択)", self.selection.rows.len())
        } else {
            self.model.table_row_label(self.model.selected_table_row())
        };
        section_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.heading(title);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.explain(ui.button(icon_label('\u{e876}', "適用")), "編集中の値を一覧とプレビューに反映します。ファイルへの保存は上部の保存ボタンで行います。").clicked() {
                        match self.apply() {
                            Ok(()) => self.status = "適用しました。保存でファイルに反映します。".into(),
                            Err(e) => self.status = e,
                        }
                    }
                });
            });
        });
        if !multiple && self.model.selected_range().is_some() {
            ui.horizontal(|ui| {
                self.explain(
                    ui.label("開始 U+"),
                    "範囲の開始コードポイントを16進数で指定します。両端を含みます。",
                );
                let start = ui.add(egui::TextEdit::singleline(&mut self.start).desired_width(80.0));
                self.explain(
                    start,
                    "範囲の開始コードポイントを16進数で指定します。両端を含みます。",
                );
                self.explain(
                    ui.label("終了 U+"),
                    "範囲の終了コードポイントを16進数で指定します。両端を含みます。",
                );
                let end = ui.add(egui::TextEdit::singleline(&mut self.end).desired_width(80.0));
                self.explain(
                    end,
                    "範囲の終了コードポイントを16進数で指定します。両端を含みます。",
                );
            });
        }
        ui.add_space(8.0);
        if multiple {
            ui.weak("変更した項目だけを選択行へ適用します。表示値は最後に選んだ行の値です。");
        }
        ui.horizontal_top(|ui| {
            ui.spacing_mut().interact_size.y = 32.0;
            self.explain(ui.add_sized([48.0, 32.0], egui::Label::new("フォント")), "フォント名は直接入力、または右端の一覧から選択できます。空欄はフォントを変更しません。");
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let width = (ui.available_width() - 140.0).clamp(160.0, 420.0);
                let input = ui.add_sized([width, 32.0],
                    egui::TextEdit::singleline(&mut self.font).hint_text("フォント名を入力"));
                self.changed_fields[0] |= input.changed();
                self.explain(input.clone(), "フォント名は直接入力、または右端の一覧から選択できます。空欄はフォントを変更しません。");
                let button = ui.add_sized([36.0, 32.0], egui::Button::new(icon_label('', "")));
                self.explain(button.clone(), "フォント一覧を開きます。検索して選択できます。");
                self.font_picker.button(&button, input.rect.union(button.rect), input.rect.height());
            });
            let old_unit = self.unit;
            let unit = egui::ComboBox::from_id_salt("unit")
                .width(50.0)
                .selected_text(if self.unit == MetricUnit::Percent {
                    "%"
                } else {
                    "px"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.unit, MetricUnit::Percent, "%");
                    ui.selectable_value(&mut self.unit, MetricUnit::Pixels, "px");
                });
            self.changed_fields[1] |= self.unit != old_unit;
            self.explain(unit.response, "サイズ・ベースライン・字送りの単位を切り替えます。%は元のサイズに対する比率、pxは絶対値です。");
        });
        ui.add_space(12.0);
        ui.columns(2, |columns| {
            for (column, indices) in [&[0, 1, 2][..], &[3, 4][..]].iter().enumerate() {
                egui::Grid::new(("metrics", column))
                    .num_columns(2)
                    .spacing([16.0, 12.0])
                    .show(&mut columns[column], |ui| {
                        for &index in *indices {
                            let (icon, label, help) = [
                                ('\u{e245}', "サイズ", "文字サイズを指定します。%は元の文字サイズに対する比率、pxは絶対サイズです。"),
                                ('\u{e8d5}', "ベースライン", "文字のベースライン位置を補正します。%またはpxで指定します。"),
                                ('\u{f773}', "字送り", "文字の送り幅を補正します。%またはpxで指定します。"),
                                ('\u{ea16}', "垂直 (%)", "文字を縦方向に拡大・縮小します。100%で元の高さです。"),
                                ('\u{f69b}', "水平 (%)", "文字を横方向に拡大・縮小します。100%で元の幅です。"),
                            ][index];
                            self.explain(ui.label(material_label(icon, label, "MaterialMetrics")), help);
                            let response = metric_input(ui, &mut self.values[index], index);
                            self.changed_fields[index + 2] |= response.changed();
                            self.explain(response, &format!("{help} ホイールで1ずつ、Shiftで0.1ずつ、Ctrlで10ずつ調整します。"));
                            ui.end_row();
                        }
                    });
            }
        });
    }
    fn range_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.range_dialog.take() else {
            return;
        };
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("add-range-dialog")).show(ctx, |ui| {
            ui.set_min_width(420.0);
            section_title(ui, "範囲を追加");
            ui.add_space(12.0);
            if ui
                .radio_value(&mut draft.custom, false, "プリセット")
                .clicked()
            {
                draft.start = format!("{:04X}", UNICODE_PRESETS[draft.preset].start);
                draft.end = format!("{:04X}", UNICODE_PRESETS[draft.preset].end);
            }
            ui.add_enabled_ui(!draft.custom, |ui| {
                let selected_name = UNICODE_PRESETS[draft.preset].name;
                let button = ui.add_sized(
                    [420.0, 32.0],
                    egui::Button::new((
                        selected_name.atom_shrink(true),
                        egui::Atom::grow(),
                        icon_label('\u{e5c5}', ""),
                    )),
                );
                if button.clicked() {
                    draft.focus_preset_search = true;
                }
                let mut chosen = None;
                egui::Popup::menu(&button)
                    .id(egui::Id::new("new-range-preset"))
                    .width(button.rect.width())
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        // The common blocks stay above the independently scrolling full list.
                        ui.spacing_mut().item_spacing.y = 4.0;
                        let search = ui.add_sized(
                            [ui.available_width(), 28.0],
                            egui::TextEdit::singleline(&mut draft.preset_search)
                                .hint_text("ブロック名 / U+コードポイントで検索"),
                        );
                        if draft.focus_preset_search {
                            search.request_focus();
                            draft.focus_preset_search = false;
                        }
                        ui.weak("よく使う範囲");
                        for (index, _) in UNICODE_PRESETS
                            .iter()
                            .enumerate()
                            .take_while(|(_, preset)| preset.frequent)
                        {
                            if preset_row(ui, index, draft.preset).clicked() {
                                chosen = Some(index);
                                ui.close();
                            }
                        }
                        ui.separator();
                        let matches: Vec<_> = UNICODE_PRESETS
                            .iter()
                            .enumerate()
                            .filter(|(_, preset)| {
                                !preset.frequent && preset.matches(&draft.preset_search)
                            })
                            .map(|(index, _)| index)
                            .collect();
                        ui.weak(format!("その他のブロック · {}件", matches.len()));
                        let mut scroll = egui::ScrollArea::vertical()
                            .id_salt("unicode-blocks")
                            .max_height(156.0);
                        if search.changed() {
                            scroll = scroll.vertical_scroll_offset(0.0);
                        }
                        scroll.show(ui, |ui| {
                            if matches.is_empty() {
                                ui.weak("一致するブロックはありません。");
                            }
                            for index in matches {
                                if preset_row(ui, index, draft.preset).clicked() {
                                    chosen = Some(index);
                                    ui.close();
                                }
                            }
                        });
                    });
                if let Some(index) = chosen {
                    draft.preset = index;
                    let preset = &UNICODE_PRESETS[index];
                    draft.start = format!("{:04X}", preset.start);
                    draft.end = format!("{:04X}", preset.end);
                }
            });
            ui.add_space(12.0);
            ui.radio_value(&mut draft.custom, true, "カスタム");
            ui.add_enabled_ui(draft.custom, |ui| {
                ui.horizontal(|ui| {
                    ui.label("名前");
                    ui.text_edit_singleline(&mut draft.name);
                });
            });
            ui.horizontal(|ui| {
                ui.label("開始 U+");
                let a = ui.add(egui::TextEdit::singleline(&mut draft.start).desired_width(90.0));
                ui.label("終了 U+");
                let b = ui.add(egui::TextEdit::singleline(&mut draft.end).desired_width(90.0));
                if a.changed() || b.changed() {
                    draft.custom = true;
                }
            });
            ui.small("両端を含む16進数の範囲です。プリセットの値も手動編集できます。");
            if !draft.error.is_empty() {
                ui.colored_label(egui::Color32::LIGHT_RED, &draft.error);
            }
            let missing: Vec<_> = CATEGORY_ROWS
                .iter()
                .enumerate()
                .filter(|(_, row)| !self.model.selected_profile().class_enabled(row.class))
                .collect();
            if !missing.is_empty() {
                ui.separator();
                ui.label("削除した分類を復元");
                ui.horizontal_wrapped(|ui| {
                    for (index, row) in missing {
                        if ui.button(row.label).clicked() {
                            self.model.restore_category(index);
                            self.reset_selection();
                            close = true;
                        }
                    }
                });
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button(icon_label('\u{e145}', "追加")).clicked() {
                    let name = if draft.custom {
                        if draft.name.trim().is_empty() {
                            "カスタム範囲"
                        } else {
                            draft.name.trim()
                        }
                    } else {
                        UNICODE_PRESETS[draft.preset].name
                    };
                    let result = parse_codepoint(&draft.start).and_then(|start| {
                        parse_codepoint(&draft.end)
                            .and_then(|end| self.model.add_range(name.to_owned(), start, end))
                    });
                    match result {
                        Ok(()) => {
                            self.reset_selection();
                            close = true;
                        }
                        Err(e) => draft.error = e,
                    }
                }
                if ui.button(icon_label('\u{e5cd}', "キャンセル")).clicked() {
                    close = true;
                }
            });
        });
        if !close && !response.should_close() {
            self.range_dialog = Some(draft);
        }
    }
}
impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // No timer or unconditional repaint: video processing never drives the UI.
        self.hover_help = None;
        self.font_picker.begin_frame();
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading("合成フォント");
                ui.weak("egui 試験版");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.explain(ui.button(icon_label('\u{e5cd}', "キャンセル")), "保存していない変更を破棄して閉じます。").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if self.explain(ui.button(icon_label('\u{e876}', "保存して閉じる")), "プロファイルを保存して設定画面を閉じます。").clicked() {
                        match self.save() {
                            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                            Err(e) => self.status = e,
                        }
                    }
                    if self.explain(ui.button(icon_label('\u{e161}', "保存")), "プロファイルを保存します。anm2の選択肢の変更はAviUtl2の再起動後に反映されます。").clicked()
                        && let Err(e) = self.save() {
                        self.status = e;
                    }
                });
            });
            ui.add_space(8.0);
        });
        // Reserve the footer before the panels, then fill it after collecting this frame's hover.
        let mut status_ui = egui::TopBottomPanel::bottom("status")
            .show(ctx, |ui| {
                let (_, rect) = ui.allocate_space(egui::vec2(ui.available_width(), 22.0));
                ui.new_child(egui::UiBuilder::new().max_rect(rect))
            })
            .inner;
        egui::SidePanel::left("profiles")
            .resizable(true)
            .default_width(260.0)
            .min_width(220.0)
            .max_width(360.0)
            .show_separator_line(false)
            .show(ctx, |ui| {
                self.profiles_ui(ui);
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.preview_ui(ui, ctx);
            egui::ScrollArea::vertical()
                .id_salt("font-editor")
                .show(ui, |ui| {
                    self.fonts_ui(ui);
                });
        });
        let old_font = self.font.clone();
        self.font_picker.show(ctx, &self.fonts, &mut self.font);
        self.changed_fields[0] |= self.font != old_font;
        self.range_dialog_ui(ctx);
        status_ui
            .add(egui::Label::new(self.hover_help.as_deref().unwrap_or(&self.status)).truncate());
        // Anchor Windows IME candidates to the caret, including inline names.
        ctx.output_mut(|output| {
            if let Some(ime) = &mut output.ime {
                ime.rect = ime.cursor_rect;
            }
        });
        if self.preview_dirty {
            ctx.request_repaint();
        }
    }
}

pub fn show_editor(
    owner: aviutl2::Win32WindowHandle,
    document: ProfileDocument,
    profile_path: PathBuf,
    edit_handle: Arc<EditHandle>,
    font_registration: Arc<Mutex<FontRegistrationState>>,
) -> Result<ProfileDocument, String> {
    let saved = Arc::new(Mutex::new(document.clone()));
    let result = Arc::clone(&saved);
    let mut fonts = edit_handle.get_font_names();
    fonts.sort();
    fonts.dedup();
    let mut app = EditorApp {
        model: EditorModel::new(document),
        path: profile_path,
        saved,
        handle: edit_handle,
        fonts,
        registration: font_registration,
        profile_name: String::new(),
        font: String::new(),
        font_picker: FontPicker::default(),
        unit: MetricUnit::Percent,
        values: [100.0, 0.0, 0.0, 100.0, 100.0],
        selection: RowSelection::default(),
        changed_fields: [false; 7],
        editing_profile: false,
        focus_profile_editor: false,
        range_dialog: None,
        start: "0000".to_owned(),
        end: "007F".to_owned(),
        sample: "漢字 ひらがな カタカナ ABC123 😀".to_owned(),
        preview: None,
        preview_dirty: true,
        scroll_selected_row: true,
        guides: true,
        status: "編集中の変更は「適用」でプレビューに反映します。".to_owned(),
        hover_help: None,
    };
    app.reset_selection();
    let _ = owner;
    let _dpi_scope = ThreadDpiScope::enter();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 960.0])
            .with_min_inner_size([960.0, 720.0]),
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_any_thread(true).with_dpi_aware(false);
        })),
        run_and_return: true,
        dithering: false,
        ..Default::default()
    };
    eframe::run_native(
        "合成フォント — egui 試験版",
        options,
        Box::new(move |cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.override_text_color = Some(egui::Color32::from_rgb(224, 228, 234));
            cc.egui_ctx.set_visuals(visuals);
            cc.egui_ctx.style_mut(|style| {
                style.animation_time = 0.0;
                style.spacing.item_spacing = egui::vec2(8.0, 10.0);
                style.spacing.button_padding = egui::vec2(8.0, 5.0);
                style
                    .text_styles
                    .insert(egui::TextStyle::Body, egui::FontId::proportional(17.0));
                style
                    .text_styles
                    .insert(egui::TextStyle::Button, egui::FontId::proportional(17.0));
                style
                    .text_styles
                    .insert(egui::TextStyle::Heading, egui::FontId::proportional(24.0));
                style
                    .text_styles
                    .insert(egui::TextStyle::Small, egui::FontId::proportional(14.0));
            });
            let mut definitions = egui::FontDefinitions::default();
            for (file, index) in [
                ("C:/Windows/Fonts/YuGothM.ttc", 1), // Yu Gothic UI Regular
                ("C:/Windows/Fonts/meiryo.ttc", 2),  // Meiryo UI Regular
            ] {
                if let Ok(bytes) = std::fs::read(file) {
                    definitions.font_data.insert(
                        "Japanese".to_owned(),
                        Arc::new(egui::FontData {
                            index,
                            ..egui::FontData::from_owned(bytes)
                        }),
                    );
                    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                        definitions
                            .families
                            .get_mut(&family)
                            .unwrap()
                            .insert(0, "Japanese".to_owned());
                    }
                    break;
                }
            }
            definitions.font_data.insert(
                "MaterialIcons".to_owned(),
                Arc::new(egui::FontData::from_static(include_bytes!(
                    "../../../resource/material-icons/MaterialIcons-Regular.ttf"
                ))),
            );
            // Keep icon glyphs in a dedicated family: Japanese TTC PUA glyphs
            // must not mask MaterialIcons codepoints.
            definitions.families.insert(
                egui::FontFamily::Name("MaterialIcons".into()),
                vec!["MaterialIcons".to_owned()],
            );
            definitions.font_data.insert(
                "MaterialMetrics".to_owned(),
                Arc::new(egui::FontData::from_static(include_bytes!(
                    "../../../resource/material-icons/MaterialSymbols-Metrics.ttf"
                ))),
            );
            definitions.families.insert(
                egui::FontFamily::Name("MaterialMetrics".into()),
                vec!["MaterialMetrics".to_owned()],
            );
            cc.egui_ctx.set_fonts(definitions);
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| error.to_string())?;
    let document = result
        .lock()
        .map_err(|_| "保存状態を取得できません。")?
        .clone();
    Ok(document)
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn profile_header_leaves_visible_space_for_rows_at_every_sidebar_width() {
        for width in [220.0, 260.0, 360.0] {
            let ctx = egui::Context::default();
            let mut fonts = egui::FontDefinitions::default();
            fonts.families.insert(
                egui::FontFamily::Name("MaterialIcons".into()),
                fonts.families[&egui::FontFamily::Proportional].clone(),
            );
            ctx.set_fonts(fonts);
            ctx.style_mut(|style| {
                style.spacing.item_spacing = egui::vec2(8.0, 10.0);
                style
                    .text_styles
                    .insert(egui::TextStyle::Heading, egui::FontId::proportional(24.0));
            });
            for _ in 0..2 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(960.0, 720.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::SidePanel::left("profiles")
                            .exact_width(width)
                            .show(ctx, |ui| {
                                let header = profile_header(ui);
                                assert!(
                                    header.response.rect.height() <= 48.0,
                                    "{:?}",
                                    header.response.rect
                                );
                                ui.add_space(12.0);
                                let space = ui.available_height();
                                assert!(space > 600.0, "{space}");
                                egui::ScrollArea::vertical()
                                    .max_height(space)
                                    .show(ui, |ui| {
                                        for name in ["default", "sample"] {
                                            let row = ui.add_sized(
                                                [ui.available_width(), 32.0],
                                                left_selectable(false, name),
                                            );
                                            assert!(row.rect.bottom() < 150.0, "{:?}", row.rect);
                                            assert!(ui.clip_rect().contains_rect(row.rect));
                                        }
                                    });
                            });
                    },
                );
            }
        }
    }
}
