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
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use winit::platform::windows::EventLoopBuilderExtWindows;

#[path = "egui_font_picker.rs"]
mod font_picker;
use font_picker::FontPicker;
#[path = "font_preview.rs"]
mod font_preview;
#[path = "egui_selection.rs"]
mod selection;
use selection::RowSelection;

// Only the preview source field uses this family; UI labels keep Yu Gothic UI.
fn preview_input_fonts(definitions: &mut egui::FontDefinitions) {
    let mut family = Vec::new();
    let system_fonts = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/Windows"))
        .join("Fonts");
    // Nirmala covers Indic scripts (including Odia). Additional faces cover
    // Arabic/Hebrew, Southeast Asian scripts, African scripts and CJK.
    for file in [
        "Nirmala.ttc",
        "segoeui.ttf",
        "LeelawUI.ttf",
        "ebrima.ttf",
        "gadugi.ttf",
        "sylfaen.ttf",
        "malgun.ttf",
        "msjh.ttc",
        "msyh.ttc",
        "seguisym.ttf",
        "seguiemj.ttf",
        "seguihis.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(system_fonts.join(file)) {
            let name = format!("PreviewInput/{file}");
            definitions
                .font_data
                .insert(name.clone(), Arc::new(egui::FontData::from_owned(bytes)));
            family.push(name);
        }
    }
    family.extend(definitions.families[&egui::FontFamily::Proportional].clone());
    definitions
        .families
        .insert(egui::FontFamily::Name("PreviewInput".into()), family);
}

// Google Material Icons (Apache-2.0; bundled LICENSE.txt).
fn icon_label(icon: char, label: &str) -> egui::WidgetText {
    material_label(icon, label, "MaterialIcons")
}

fn visibility_toggle(ui: &mut egui::Ui, value: &mut bool, label: &str) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(26.0, 28.0), egui::Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            ui.is_enabled(),
            *value,
            format!("{label}の上書き"),
        )
    });
    if ui.is_rect_visible(rect) {
        if response.hovered() || response.has_focus() {
            ui.painter()
                .rect_filled(rect, 3.0, ui.visuals().widgets.hovered.bg_fill);
        }
        let center = rect.center();
        let color = if *value {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_gray(128)
        };
        let stroke = egui::Stroke::new(1.6_f32, color);
        let left = center - egui::vec2(9.0, 0.0);
        let right = center + egui::vec2(9.0, 0.0);
        for control in [
            center - egui::vec2(0.0, 10.0),
            center + egui::vec2(0.0, 10.0),
        ] {
            ui.painter()
                .add(egui::epaint::QuadraticBezierShape::from_points_stroke(
                    [left, control, right],
                    false,
                    egui::Color32::TRANSPARENT,
                    stroke,
                ));
        }
        ui.painter().circle_filled(center, 2.5, color);
        if !*value {
            let slash = [center - egui::vec2(9.0, 8.0), center + egui::vec2(9.0, 8.0)];
            ui.painter()
                .line_segment(slash, egui::Stroke::new(4.5_f32, ui.visuals().panel_fill));
            ui.painter().line_segment(slash, stroke);
        }
    }
    response
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
            &format!("{}  {}", preset.name, preset.range_label()),
        ),
    )
}

fn unicode_preset_list(
    ui: &mut egui::Ui,
    query: &str,
    selected: usize,
    reset_scroll: bool,
) -> egui::scroll_area::ScrollAreaOutput<Option<usize>> {
    let matches: Vec<_> = UNICODE_PRESETS
        .iter()
        .enumerate()
        .filter(|(_, preset)| preset.matches(query))
        .map(|(index, _)| index)
        .collect();
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt("unicode-blocks")
        .max_height(156.0)
        // Popup Areas reuse their previous size. Allow this list to grow again
        // after a search shrinks it; auto_shrink still fits short result sets.
        .min_scrolled_height(156.0);
    if reset_scroll {
        scroll = scroll.vertical_scroll_offset(0.0);
    }
    scroll.show(ui, |ui| {
        let mut chosen = None;
        if matches.is_empty() {
            ui.weak("一致するブロックはありません。");
        }
        let mut section = None;
        for index in matches {
            let frequent = UNICODE_PRESETS[index].frequent;
            if section != Some(frequent) {
                if section.is_some() {
                    ui.separator();
                }
                ui.weak(if frequent {
                    "よく使う範囲"
                } else {
                    "その他のブロック"
                });
                section = Some(frequent);
            }
            if preset_row(ui, index, selected).clicked() {
                chosen = Some(index);
                ui.close();
            }
        }
        chosen
    })
}

fn subrange_details(ui: &mut egui::Ui, subranges: &[[u32; 2]]) {
    ui.label(format!("{}個の範囲をまとめて指定します。", subranges.len()));
    ui.small("絵文字と構成文字（数字・#・*・結合用文字など）が対象です。");
    ui.collapsing("対象コードポイントを表示", |ui| {
        egui::ScrollArea::vertical()
            .max_height(156.0)
            .show(ui, |ui| {
                for &[start, end] in subranges {
                    ui.monospace(format!("U+{start:04X}–U+{end:04X}"));
                }
            });
    });
}

const FONT_FIELD_HEIGHT: f32 = 32.0;

fn font_name_input(ui: &mut egui::Ui, font: &mut String, width: f32) -> egui::Response {
    ui.add_sized(
        [width, FONT_FIELD_HEIGHT],
        egui::TextEdit::singleline(font)
            .hint_text("フォント名を入力")
            .desired_width(width)
            .min_size(egui::vec2(width, FONT_FIELD_HEIGHT))
            .vertical_align(egui::Align::Center),
    )
}

const LIGATURE_CHECK: char = '\u{e5ca}'; // Material check
const LIGATURE_HELP: &str = "設定画面のプレビュー専用です。OFFは任意のOpenTypeリガチャを無効化し、言語に必須の結合や結合文字は保持します。AviUtl2標準描画のリガチャ設定を変更する公開APIは未確認のため、動画出力には反映できません。";
const FONT_TABLE_TITLES: [&str; 3] = ["名前 / 分類", "Unicode範囲", "フォント"];

fn font_table_cell_rects(row: egui::Rect) -> [egui::Rect; 3] {
    let edges = [
        row.left(),
        row.left() + row.width() * 0.22,
        row.left() + row.width() * 0.52,
        row.right() - 44.0,
    ];
    std::array::from_fn(|index| {
        egui::Rect::from_min_max(
            egui::pos2(edges[index] + 8.0, row.top() + 8.0),
            egui::pos2(edges[index + 1] - 8.0, row.bottom() - 8.0),
        )
    })
}

fn paint_font_table_cells(ui: &egui::Ui, row: egui::Rect, cells: [&str; 3], color: egui::Color32) {
    let font = egui::TextStyle::Body.resolve(ui.style());
    for (text, rect) in cells.into_iter().zip(font_table_cell_rects(row)) {
        let mut job =
            egui::text::LayoutJob::simple_singleline(text.to_owned(), font.clone(), color);
        job.wrap.max_width = rect.width().max(0.0);
        job.wrap.max_rows = 1;
        job.wrap.overflow_character = Some('…');
        let galley = ui.painter().layout_job(job);
        let pos = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
        ui.painter()
            .with_clip_rect(ui.clip_rect().intersect(rect))
            .galley(pos, galley, color);
    }
}

fn paint_font_table_rules(ui: &egui::Ui, row: egui::Rect, separator: bool) {
    let stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(70, 74, 79));
    for factor in [0.22, 0.52] {
        ui.painter()
            .vline(row.left() + row.width() * factor, row.y_range(), stroke);
    }
    ui.painter()
        .vline(row.right() - 44.0, row.y_range(), stroke);
    if separator {
        ui.painter()
            .hline(row.x_range(), row.bottom() - 0.5, stroke);
    }
}

fn font_table_header(ui: &mut egui::Ui) {
    let height = ui.text_style_height(&egui::TextStyle::Body).max(24.0) + 16.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, 0.0, egui::Color32::from_rgb(60, 64, 69));
    paint_font_table_cells(
        ui,
        rect,
        FONT_TABLE_TITLES,
        ui.visuals().strong_text_color(),
    );
    paint_link(
        ui.painter(),
        egui::Rect::from_center_size(
            egui::pos2(rect.right() - 22.0, rect.center().y),
            egui::vec2(20.0, 20.0),
        ),
        ui.visuals().weak_text_color(),
    );
    paint_font_table_rules(ui, rect, true);
}

fn font_range_row(
    ui: &mut egui::Ui,
    selected: bool,
    cells: [&str; 3],
    separator: bool,
    ligatures_enabled: bool,
) -> egui::Response {
    // Include every cell, padding and rule in one hit target.
    let height = ui.text_style_height(&egui::TextStyle::Body).max(24.0) + 16.0;
    let response = ui.add_sized(
        [ui.available_width(), height],
        egui::Button::selectable(selected, "").corner_radius(0.0),
    );
    let color = if selected {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    };
    paint_font_table_cells(ui, response.rect, cells, color);
    if ligatures_enabled {
        paint_ligature_icon(ui, response.rect, color);
    }
    paint_font_table_rules(ui, response.rect, separator);
    response
}

fn paint_ligature_icon(ui: &egui::Ui, row: egui::Rect, color: egui::Color32) {
    let rect = egui::Rect::from_min_max(
        egui::pos2(row.right() - 44.0, row.top()),
        row.right_bottom(),
    );
    ui.painter()
        .with_clip_rect(ui.clip_rect().intersect(rect))
        .text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            LIGATURE_CHECK,
            egui::FontId::new(20.0, egui::FontFamily::Name("MaterialIcons".into())),
            color,
        );
}

// Lucide / Feather Link (MIT), resource/lucide/LICENSE.txt.
// The two diagonal, interlocking paths use a 24-unit view box.
fn paint_link(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let mut path = vec![egui::pos2(10.0, 13.0)];
    let inner_center = egui::pos2(14.004, 10.005);
    let start_angle = (13.0_f32 - inner_center.y).atan2(10.0 - inner_center.x);
    let end_angle = (13.54_f32 - inner_center.y).atan2(17.54 - inner_center.x);
    for step in 1..=24 {
        let angle = start_angle + (end_angle - start_angle) * step as f32 / 24.0;
        path.push(inner_center + egui::vec2(5.0 * angle.cos(), 5.0 * angle.sin()));
    }
    path.push(egui::pos2(20.54, 10.54));
    for step in 1..=32 {
        let angle = std::f32::consts::FRAC_PI_4 - std::f32::consts::PI * step as f32 / 32.0;
        path.push(egui::pos2(17.005, 7.005) + egui::vec2(5.0 * angle.cos(), 5.0 * angle.sin()));
    }
    path.push(egui::pos2(11.75, 5.18));
    let stroke = egui::Stroke::new(rect.width() / 12.0, color);
    for reflected in [false, true] {
        let points: Vec<_> = path
            .iter()
            .map(|p| {
                let p = if reflected {
                    egui::pos2(24.0 - p.x, 24.0 - p.y)
                } else {
                    *p
                };
                rect.min + egui::vec2(p.x / 24.0 * rect.width(), p.y / 24.0 * rect.height())
            })
            .collect();
        for end in [points[0], *points.last().unwrap()] {
            painter.circle_filled(end, stroke.width / 2.0, color);
        }
        painter.add(egui::epaint::PathShape::line(points, stroke));
    }
}

fn profile_panel() -> egui::SidePanel {
    egui::SidePanel::left("profiles")
        // egui still paints a resize line on hover with show_separator_line(false).
        .resizable(false)
        .default_width(260.0)
        .min_width(220.0)
        .max_width(360.0)
        .show_separator_line(false)
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
    changed_fields: [bool; 8],
    enabled: [bool; 7],
    enabled_changed: [bool; 7],
    ligatures_enabled: bool,
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

fn watch_host_window(
    owner: isize,
    ctx: egui::Context,
    stop: Arc<AtomicBool>,
) -> std::io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("comfont-host-window-watch".to_owned())
        .spawn(move || {
            while !stop.load(Ordering::Acquire) {
                // The configuration callback runs a nested native event loop. When
                // AviUtl2 destroys its main window, close both eframe viewports so
                // the callback can return and the host can finish shutting down.
                if !crate::host_window_exists(owner) {
                    ctx.send_viewport_cmd_to(
                        egui::ViewportId::from_hash_of("font-picker-desktop"),
                        egui::ViewportCommand::Close,
                    );
                    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
                    ctx.request_repaint();
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        })
}

struct HostWindowWatcherGuard {
    stop: Arc<AtomicBool>,
    handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl HostWindowWatcherGuard {
    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::Release);
        let mut handle = self
            .handle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(handle) = handle.take() {
            let _ = handle.join();
        }
    }

    fn restart(&mut self) {
        self.stop_and_join();
        self.stop.store(false, Ordering::Release);
    }
}

impl Drop for HostWindowWatcherGuard {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

enum EframeRunFailure {
    Initialization(String),
    Panic(String),
}

fn run_eframe(
    options: eframe::NativeOptions,
    app_creator: eframe::AppCreator<'static>,
) -> Result<(), EframeRunFailure> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eframe::run_native("合成フォント — β", options, app_creator)
    }))
    .map_err(|payload| {
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("描画バックエンドの初期化中に予期しないエラーが発生しました");
        EframeRunFailure::Panic(format!("egui の実行中に panic が発生しました: {message}"))
    })?
    .map_err(|error| EframeRunFailure::Initialization(error.to_string()))
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
        self.ligatures_enabled = !adjustment.disable_ligatures;
        self.changed_fields = [false; 8];
        self.enabled = [
            adjustment.font_family_enabled,
            adjustment.size_enabled,
            adjustment.baseline_shift_enabled,
            adjustment.tracking_adjust_enabled,
            adjustment.vertical_scale_enabled,
            adjustment.horizontal_scale_enabled,
            adjustment.ligature_setting_enabled,
        ];
        self.enabled_changed = [false; 7];
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
        if self.selection.rows.len() == 1
            && candidate
                .selected_range()
                .is_some_and(|range| range.subranges.is_empty())
        {
            candidate.update_selected_range(
                parse_codepoint(&self.start)?,
                parse_codepoint(&self.end)?,
            )?;
        }
        if self.changed_fields.iter().any(|&changed| changed)
            || self.enabled_changed.iter().any(|&changed| changed)
        {
            let patch = AdjustmentPatch {
                font_family: self.changed_fields[0].then(|| self.font.clone()),
                font_family_enabled: self.enabled_changed[0].then_some(self.enabled[0]),
                metric_unit: self.changed_fields[1].then_some(self.unit),
                disable_ligatures: self.changed_fields[7].then_some(!self.ligatures_enabled),
                values: std::array::from_fn(|index| {
                    (self.changed_fields[index + 2] || (index < 3 && self.changed_fields[1]))
                        .then_some(self.values[index])
                }),
                size_enabled: self.enabled_changed[1].then_some(self.enabled[1]),
                baseline_shift_enabled: self.enabled_changed[2].then_some(self.enabled[2]),
                tracking_adjust_enabled: self.enabled_changed[3].then_some(self.enabled[3]),
                vertical_scale_enabled: self.enabled_changed[4].then_some(self.enabled[4]),
                horizontal_scale_enabled: self.enabled_changed[5].then_some(self.enabled[5]),
                ligature_setting_enabled: self.enabled_changed[6].then_some(self.enabled[6]),
            };
            candidate.patch_table_rows(&self.selection.rows, &patch)?;
        }
        self.preview_dirty |= self.model.document() != candidate.document();
        self.model = candidate;
        self.changed_fields = [false; 8];
        self.enabled_changed = [false; 7];
        Ok(())
    }
    fn save(&mut self) -> Result<(), String> {
        self.apply()?;
        storage::save_document(&self.path, self.model.document())?;
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
                "保存しました。追加フォント{}件はAviUtl2の再起動後に反映されます。",
                outcome.added_file_count
            )
        } else {
            "保存しました。".to_owned()
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
                    .font(egui::FontId::new(
                        17.0,
                        egui::FontFamily::Name("PreviewInput".into()),
                    ))
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
                .explain(
                    guides,
                    "シアン：グリフの実寸、マゼンタ：フォントの字枠、赤：ベースライン。",
                )
                .changed()
            {
                self.preview_dirty = true;
            }
        });
        if self.guides {
            ui.horizontal(|ui| {
                ui.colored_label(egui::Color32::CYAN, "グリフ境界（実寸）");
                ui.colored_label(egui::Color32::MAGENTA, "テキストサイズ（字枠）");
                ui.colored_label(egui::Color32::RED, "ベースライン");
            });
        }
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
                ('\u{e5d8}', RowMoveDirection::Up, "上へ移動。既定分類とカスタム範囲を混在して並べ替えます。分類をまたぐと候補フォントもまとめて移動します。"),
                ('\u{e5db}', RowMoveDirection::Down, "下へ移動。既定分類とカスタム範囲を混在して並べ替えます。分類をまたぐと候補フォントもまとめて移動します。"),
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
                        ui.spacing_mut().item_spacing.y = 0.0;
                        font_table_header(ui);
                        if self.model.table_row_count() == 0 {
                            ui.weak("範囲がありません。「追加」から登録してください。");
                        }
                        for row in 0..self.model.table_row_count() {
                            let adjustment = self.model.adjustment_for_table_row(row).unwrap();
                            let font = if adjustment.font_family.is_empty() {
                                "（変更なし）"
                            } else {
                                &adjustment.font_family
                            };
                            let name = self.model.table_row_name(row);
                            let range = self.model.table_row_range_text(row);
                            let response = font_range_row(
                                ui, self.selection.rows.contains(&row), [name, &range, font],
                                true, !adjustment.disable_ligatures,
                            );
                            self.explain(response.clone(), "カスタム範囲を優先し、重複時は上から最初に一致した範囲を使います。Shiftで範囲選択、Ctrlで追加・解除。通常クリックで複数選択を解除し、その行を編集します。");
                            if response.contains_pointer() && ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| p.x >= response.rect.right() - 44.0) {
                                self.hover_help = Some(LIGATURE_HELP.to_owned());
                            }
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
        if !multiple
            && self
                .model
                .selected_range()
                .is_some_and(|range| !range.subranges.is_empty())
        {
            subrange_details(ui, &self.model.selected_range().unwrap().subranges);
        } else if !multiple && self.model.selected_range().is_some() {
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
            let font_enabled = visibility_toggle(ui, &mut self.enabled[0], "フォント");
            self.enabled_changed[0] |= font_enabled.changed();
            self.explain(font_enabled, "白い目：フォント名を上書き。灰色の斜線付きの目：上書きしません。入力値は保存します。");
            self.explain(ui.add_sized([48.0, 32.0], egui::Label::new("フォント")), "フォント名は直接入力、または右端の一覧から選択できます。空欄は元のフォントを引き継ぎ、他の有効なパラメータを適用します。");
            ui.add_enabled_ui(self.enabled[0], |ui| {
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let width = (ui.available_width() - 140.0).clamp(120.0, 420.0);
                    let input = font_name_input(ui, &mut self.font, width);
                    self.changed_fields[0] |= input.changed();
                    self.explain(input.clone(), "フォント名は直接入力、または右端の一覧から選択できます。空欄は元のフォントを引き継ぎ、他の有効なパラメータを適用します。");
                    let button = ui.add_sized([36.0, 32.0], egui::Button::new(icon_label('', "")));
                    self.explain(button.clone(), "フォント一覧を開きます。検索して選択できます。");
                    self.font_picker.button(&button, input.rect.union(button.rect), input.rect.height());
                    if let Some(variations) = self
                        .font_picker
                        .variations(&self.font, &self.fonts, ui.ctx())
                    {
                        ui.add_space(6.0);
                        let selected = variations
                            .iter()
                            .find(|item| item.family.eq_ignore_ascii_case(self.font.trim()))
                            .map_or("バリエーション", |item| item.label.as_str());
                        let response = ui.add_sized([142.0, 32.0], egui::Button::new((
                            selected.to_owned().atom_shrink(true), egui::Atom::grow(),
                            egui::RichText::new("\u{e5c5}").family(egui::FontFamily::Name("MaterialIcons".into())),
                        )).truncate());
                        self.font_picker.weight_button(&response, variations);
                        self.explain(
                            response,
                            "OpenTypeのウェイト違いを選びます。選択するとフォント名をAviUtl2で認識される名前へ切り替えます。",
                        );
                    }
                });
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
                            ui.horizontal(|ui| {
                                let enabled = visibility_toggle(ui, &mut self.enabled[index + 1], label);
                                self.enabled_changed[index + 1] |= enabled.changed();
                                self.explain(enabled, &format!("白い目：{label}を上書き。灰色の斜線付きの目：上書きしません。入力値は保存します。"));
                                self.explain(ui.label(material_label(icon, label, "MaterialMetrics")), help);
                            });
                            let response = ui.add_enabled_ui(self.enabled[index + 1], |ui| {
                                metric_input(ui, &mut self.values[index], index)
                            }).inner;
                            self.changed_fields[index + 2] |= response.changed();
                            self.explain(response, &format!("{help} ホイールで1ずつ、Shiftで0.1ずつ、Ctrlで10ずつ調整します。"));
                            ui.end_row();
                        }
                        if column == 1 {
                            ui.horizontal(|ui| {
                                let enabled = visibility_toggle(ui, &mut self.enabled[6], "リガチャ");
                                self.enabled_changed[6] |= enabled.changed();
                                self.explain(enabled, "プレビュー専用。白い目：リガチャ設定を上書き。灰色の斜線付きの目：元の設定を使います。動画出力には反映できません。");
                                let label = ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                                    paint_link(ui.painter(), rect, ui.visuals().text_color());
                                    ui.label("リガチャ（プレビュー）");
                                }).response;
                                self.explain(label, LIGATURE_HELP);
                            });
                            let check = ui.add_enabled(self.enabled[6], egui::Checkbox::new(&mut self.ligatures_enabled, "有効"));
                            self.changed_fields[7] |= check.changed();
                            self.explain(check, LIGATURE_HELP);
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
                        chosen = unicode_preset_list(
                            ui,
                            &draft.preset_search,
                            draft.preset,
                            search.changed(),
                        )
                        .inner;
                    });
                if let Some(index) = chosen {
                    draft.preset = index;
                    let preset = &UNICODE_PRESETS[index];
                    draft.start = format!("{:04X}", preset.start);
                    draft.end = format!("{:04X}", preset.end);
                }
            });
            ui.add_space(12.0);
            if ui
                .radio_value(&mut draft.custom, true, "カスタム")
                .clicked()
                && let Some(&[start, end]) = UNICODE_PRESETS[draft.preset].subranges.first()
            {
                draft.start = format!("{start:04X}");
                draft.end = format!("{end:04X}");
            }
            ui.add_enabled_ui(draft.custom, |ui| {
                ui.horizontal(|ui| {
                    ui.label("名前");
                    ui.text_edit_singleline(&mut draft.name);
                });
            });
            let preset = &UNICODE_PRESETS[draft.preset];
            if !draft.custom && !preset.subranges.is_empty() {
                subrange_details(ui, preset.subranges);
            } else {
                ui.horizontal(|ui| {
                    ui.label("開始 U+");
                    let a =
                        ui.add(egui::TextEdit::singleline(&mut draft.start).desired_width(90.0));
                    ui.label("終了 U+");
                    let b = ui.add(egui::TextEdit::singleline(&mut draft.end).desired_width(90.0));
                    if a.changed() || b.changed() {
                        draft.custom = true;
                    }
                });
                ui.small("両端を含む16進数の範囲です。プリセットの値も手動編集できます。");
            }
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
                    let result = if draft.custom {
                        parse_codepoint(&draft.start).and_then(|start| {
                            parse_codepoint(&draft.end)
                                .and_then(|end| self.model.add_range(name.to_owned(), start, end))
                        })
                    } else {
                        self.model.add_preset(&UNICODE_PRESETS[draft.preset])
                    };
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
                ui.weak("β");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self
                        .explain(
                            ui.button(icon_label('\u{e5cd}', "キャンセル")),
                            "保存していない変更を破棄して閉じます。",
                        )
                        .clicked()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if self
                        .explain(
                            ui.button(icon_label('\u{e876}', "保存して閉じる")),
                            "プロファイルを保存して設定画面を閉じます。",
                        )
                        .clicked()
                    {
                        match self.save() {
                            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                            Err(e) => self.status = e,
                        }
                    }
                    if self
                        .explain(
                            ui.button(icon_label('\u{e161}', "保存")),
                            "プロファイルを保存します。描画時に約1秒間隔で設定を読み直します。",
                        )
                        .clicked()
                        && let Err(e) = self.save()
                    {
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
        profile_panel().show(ctx, |ui| {
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
        if self.font != old_font {
            self.changed_fields[0] = true;
        }
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
        changed_fields: [false; 8],
        enabled: [true; 7],
        enabled_changed: [false; 7],
        ligatures_enabled: true,
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
    let app_slot = Arc::new(Mutex::new(Some(app)));
    let app_started = Arc::new(AtomicBool::new(false));
    let monitor_stop = Arc::new(AtomicBool::new(false));
    let monitor_handle = Arc::new(Mutex::new(None::<JoinHandle<()>>));
    let mut watcher_guard = HostWindowWatcherGuard {
        stop: Arc::clone(&monitor_stop),
        handle: Arc::clone(&monitor_handle),
    };
    let owner_hwnd = owner.hwnd.get();
    let _dpi_scope = ThreadDpiScope::enter();
    let surface_error_reported = Arc::new(AtomicBool::new(false));
    let native_options = |renderer, backends| {
        let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut wgpu_options.wgpu_setup {
            setup.instance_descriptor.backends = backends;
            // The editor is a light workload: prefer an integrated/low-power
            // adapter when both integrated and discrete GPUs are available.
            setup.power_preference = wgpu::PowerPreference::LowPower;
            // Ask for egui's downlevel limits and let each adapter provide its
            // own texture dimensions instead of requiring an 8192px texture.
            setup.device_descriptor = Arc::new(|adapter| wgpu::DeviceDescriptor {
                label: Some("comfont egui device"),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            });
        }
        let surface_error_reported = Arc::clone(&surface_error_reported);
        wgpu_options.on_surface_error = Arc::new(move |error| {
            let message = error.to_string();
            match error {
                wgpu::SurfaceError::Outdated => {
                    eframe::egui_wgpu::SurfaceErrorAction::RecreateSurface
                }
                wgpu::SurfaceError::Lost => {
                    if !surface_error_reported.swap(true, Ordering::AcqRel) {
                        let _ = aviutl2::logger::write_error_log(&format!(
                            "Comfont UI wgpu surface was lost; recreating it: {message}"
                        ));
                    }
                    eframe::egui_wgpu::SurfaceErrorAction::RecreateSurface
                }
                wgpu::SurfaceError::Timeout => eframe::egui_wgpu::SurfaceErrorAction::SkipFrame,
                wgpu::SurfaceError::OutOfMemory | wgpu::SurfaceError::Other => {
                    if !surface_error_reported.swap(true, Ordering::AcqRel) {
                        let _ = aviutl2::logger::write_error_log(&format!(
                            "Comfont UI wgpu could not acquire a frame: {message}"
                        ));
                    }
                    eframe::egui_wgpu::SurfaceErrorAction::SkipFrame
                }
            }
        });
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                // eframe 0.33 sets its default icon on GetActiveWindow(), which can
                // still be the host during startup. The empty icon explicitly disables
                // AppTitleIconSetter so it never sends WM_SETICON to AviUtl2.
                .with_icon(egui::IconData::default())
                .with_inner_size([1280.0, 960.0])
                .with_min_inner_size([960.0, 720.0]),
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_any_thread(true).with_dpi_aware(false);
            })),
            renderer,
            wgpu_options,
            run_and_return: true,
            dithering: false,
            ..Default::default()
        }
    };
    let app_creator = || {
        let app_slot = Arc::clone(&app_slot);
        let app_started = Arc::clone(&app_started);
        let monitor_stop = Arc::clone(&monitor_stop);
        let monitor_handle = Arc::clone(&monitor_handle);
        Box::new(move |cc: &eframe::CreationContext<'_>| {
            let watcher = watch_host_window(owner_hwnd, cc.egui_ctx.clone(), monitor_stop)?;
            *monitor_handle
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(watcher);
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
            preview_input_fonts(&mut definitions);
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
            let app = app_slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
                .ok_or_else(|| std::io::Error::other("Editor application was already created"))?;
            app_started.store(true, Ordering::Release);
            Ok(Box::new(app) as Box<dyn eframe::App>)
        }) as eframe::AppCreator<'static>
    };

    let mut startup_failures = Vec::new();
    let mut run_result = None;
    for (index, (name, renderer, backends)) in [
        (
            "wgpu / Direct3D 12",
            eframe::Renderer::Wgpu,
            wgpu::Backends::DX12,
        ),
        (
            "wgpu / Vulkan",
            eframe::Renderer::Wgpu,
            wgpu::Backends::VULKAN,
        ),
        (
            "egui_glow / OpenGL",
            eframe::Renderer::Glow,
            wgpu::Backends::empty(),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let result = run_eframe(native_options(renderer, backends), app_creator());
        match result {
            Ok(()) => {
                run_result = Some(Ok(()));
                break;
            }
            Err(EframeRunFailure::Panic(error)) => {
                run_result = Some(Err(error));
                break;
            }
            Err(EframeRunFailure::Initialization(error)) if app_started.load(Ordering::Acquire) => {
                run_result = Some(Err(format!(
                    "{name} の実行中に描画エラーが発生しました: {error}"
                )));
                break;
            }
            Err(EframeRunFailure::Initialization(error)) => {
                startup_failures.push(format!("{name}: {error}"));
                let _ = aviutl2::logger::write_info_log(&format!(
                    "Comfont UI renderer startup failed ({name}); trying the next backend: {error}"
                ));
                watcher_guard.restart();
                if index == 2 {
                    run_result = Some(Err(format!(
                        "合成フォント設定画面の初期化に失敗しました。試行した描画方式: {}",
                        startup_failures.join("; ")
                    )));
                }
            }
        }
    }
    run_result
        .unwrap_or_else(|| Err("合成フォント設定画面の初期化を完了できませんでした".to_owned()))
        .inspect_err(|error| {
            let _ = aviutl2::logger::write_error_log(error);
        })?;
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
    fn host_window_watcher_closes_editor_when_owner_is_gone() {
        let ctx = egui::Context::default();
        let watcher = watch_host_window(1, ctx.clone(), Arc::new(AtomicBool::new(false)))
            .expect("host-window watcher should start");
        watcher.join().expect("host-window watcher should exit");

        let output = ctx.run(egui::RawInput::default(), |_| {});
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::Close)
        );
    }

    #[test]
    fn numeric_wheel_adjusts_hovered_field_and_consumes_parent_scroll() {
        for (index, initial, delta, modifiers, expected) in [
            (0, 100.0, 1.0, egui::Modifiers::NONE, 101.0),
            (1, 0.0, -1.0, egui::Modifiers::SHIFT, -0.1),
            (2, 0.0, 2.0, egui::Modifiers::CTRL, 20.0),
            (3, 0.1, -1.0, egui::Modifiers::NONE, 0.1),
        ] {
            let ctx = egui::Context::default();
            let mut value = initial;
            let mut rect = egui::Rect::NOTHING;
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    rect = metric_input(ui, &mut value, index).rect;
                });
            });
            let _ = ctx.run(
                egui::RawInput {
                    modifiers,
                    events: vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Line,
                            delta: egui::vec2(0.0, delta),
                            modifiers,
                        },
                    ],
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        metric_input(ui, &mut value, index);
                        assert_eq!(ui.input(|i| i.raw_scroll_delta.y), 0.0);
                        assert_eq!(ui.input(|i| i.smooth_scroll_delta.y), 0.0);
                        assert!(!ui.input(|i| {
                            i.events
                                .iter()
                                .any(|e| matches!(e, egui::Event::MouseWheel { .. }))
                        }));
                    });
                },
            );
            assert_eq!(value, expected);
        }
    }

    #[test]
    fn preset_popup_grows_again_after_codepoint_search_is_broadened() {
        let ctx = egui::Context::default();
        for _ in 0..2 {
            for query in ["U+0B33", "U+10FFFF", "", "Latin", "no matching block", ""] {
                let mut height = 0.0;
                for _ in 0..3 {
                    let _ = ctx.run(egui::RawInput::default(), |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let button = ui.add_sized([420.0, 32.0], egui::Button::new("preset"));
                            egui::Popup::menu(&button)
                                .id(egui::Id::new("preset-search-regression"))
                                .width(button.rect.width())
                                .open(true)
                                .show(|ui| {
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    ui.add_sized(
                                        [ui.available_width(), 28.0],
                                        egui::TextEdit::singleline(&mut query.to_owned()),
                                    );
                                    let list = unicode_preset_list(ui, query, 0, true);
                                    height = list.inner_rect.height().min(list.content_size.y);
                                });
                        });
                    });
                }
                if matches!(query, "Latin" | "") {
                    assert!(
                        (height - 156.0).abs() < 1.0,
                        "query={query:?}, height={height}"
                    );
                } else {
                    assert!(height < 100.0, "query={query:?}, height={height}");
                }
            }
        }
    }

    #[test]
    fn font_name_input_matches_dropdown_height_and_edges() {
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let input = font_name_input(ui, &mut "Meiryo UI".into(), 240.0);
                        let button =
                            ui.add_sized([36.0, FONT_FIELD_HEIGHT], egui::Button::new("v"));
                        assert_eq!(input.rect.width(), 240.0);
                        assert_eq!(input.rect.height(), FONT_FIELD_HEIGHT);
                        assert_eq!(input.rect.top(), button.rect.top());
                        assert_eq!(input.rect.bottom(), button.rect.bottom());
                        assert_eq!(input.rect.right(), button.rect.left());
                    });
                });
            });
        }
    }

    #[test]
    fn clicking_the_far_edge_and_rule_of_a_row_clears_multi_selection() {
        let ctx = egui::Context::default();
        let mut selection = RowSelection::default();
        selection.replace(vec![0, 1]);
        let mut row_rects = Vec::new();
        {
            let mut draw = |ctx: &egui::Context| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    row_rects.clear();
                    for row in 0..3 {
                        let response = font_range_row(
                            ui,
                            selection.rows.contains(&row),
                            ["name", "", "font"],
                            true,
                            false,
                        );
                        row_rects.push(response.rect);
                        if response.clicked() {
                            let mods = ui.input(|i| i.modifiers);
                            selection.click(row, 3, mods.shift, mods.ctrl);
                        }
                    }
                });
            };
            let _ = ctx.run(egui::RawInput::default(), &mut draw);
        }
        let target = egui::pos2(row_rects[2].right() - 1.0, row_rects[2].bottom() - 1.0);
        for pressed in [true, false] {
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![
                        egui::Event::PointerMoved(target),
                        egui::Event::PointerButton {
                            pos: target,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for row in 0..3 {
                            let response = font_range_row(
                                ui,
                                selection.rows.contains(&row),
                                ["name", "", "font"],
                                true,
                                false,
                            );
                            if response.clicked() {
                                let mods = ui.input(|i| i.modifiers);
                                selection.click(row, 3, mods.shift, mods.ctrl);
                            }
                        }
                    });
                },
            );
        }
        assert_eq!(selection.rows, vec![2]);
    }

    #[test]
    fn table_text_stays_between_rules_and_columns_at_different_sizes() {
        for (width, font_size) in [(360.0, 17.0), (800.0, 24.0), (1200.0, 32.0)] {
            let ctx = egui::Context::default();
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "MaterialIcons".into(),
                Arc::new(egui::FontData::from_static(include_bytes!(
                    "../../../resource/material-icons/MaterialIcons-Regular.ttf"
                ))),
            );
            fonts.families.insert(
                egui::FontFamily::Name("MaterialIcons".into()),
                vec!["MaterialIcons".into()],
            );
            ctx.set_fonts(fonts);
            ctx.style_mut(|style| {
                style
                    .text_styles
                    .insert(egui::TextStyle::Body, egui::FontId::proportional(font_size));
            });
            let mut rows = Vec::new();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 400.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        font_table_header(ui);
                        rows.push(
                            font_range_row(
                                ui,
                                false,
                                [
                                    "Kanji",
                                    "",
                                    "A very long font family name that must stay in its column",
                                ],
                                true,
                                false,
                            )
                            .rect,
                        );
                        rows.push(
                            font_range_row(
                                ui,
                                true,
                                ["Custom range", "U+10000–U+10FFFF", "Yu Gothic UI"],
                                true,
                                true,
                            )
                            .rect,
                        );
                    });
                },
            );
            assert_eq!(rows[0].bottom(), rows[1].top());
            assert_eq!(rows[0].x_range(), rows[1].x_range());
            let mut texts = 0;
            let mut checks = Vec::new();
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(text) = shape.shape {
                    if text.galley.size().x == 0.0 {
                        continue;
                    }
                    let bounds = egui::Rect::from_min_size(text.pos, text.galley.size());
                    if let Some(row) = rows.iter().find(|row| row.contains(bounds.center())) {
                        if text.galley.job.text == LIGATURE_CHECK.to_string() {
                            checks.push(bounds.center());
                            assert!(bounds.left() > row.right() - 44.0);
                            assert!(bounds.right() < row.right());
                            continue;
                        }
                        texts += 1;
                        assert!(bounds.top() >= row.top() + 7.0, "{bounds:?} {row:?}");
                        assert!(bounds.bottom() <= row.bottom() - 7.0, "{bounds:?} {row:?}");
                        for factor in [0.22, 0.52] {
                            let line = row.left() + row.width() * factor;
                            assert!(
                                shape.clip_rect.right() <= line - 7.0
                                    || shape.clip_rect.left() >= line + 7.0,
                                "{:?} line={line}",
                                shape.clip_rect
                            );
                        }
                    }
                }
            }
            assert_eq!(texts, 5);
            assert_eq!(checks.len(), 1);
            assert!(rows[1].contains(checks[0]));
            assert!(!rows[0].contains(checks[0]));
        }
    }

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
