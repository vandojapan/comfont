use super::font_preview::{FontPreviewCatalog, FontVariation};
use eframe::egui::{self, AtomExt as _};
use std::sync::{Arc, Mutex};
use windows::Win32::{
    Foundation::POINT,
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
};

const RESIZE_EDGE_HEIGHT: f32 = 10.0;

#[derive(Default)]
pub(super) struct FontPicker {
    state: Arc<Mutex<PickerState>>,
    anchor: Option<egui::Rect>,
    weight_anchor: Option<egui::Rect>,
    weights: Option<Arc<Vec<FontVariation>>>,
    fonts: Option<Arc<Vec<String>>>,
    previews: FontPreviewCatalog,
    opening: bool,
}
#[derive(Default)]
struct PickerState {
    open: bool,
    weight_mode: bool,
    focus_search: bool,
    had_focus: bool,
    visible: bool,
    search: String,
    current: String,
    chosen: Option<String>,
    anchor_pixels: Option<egui::Rect>,
    search_height_pixels: f32,
    geometry: Option<egui::Rect>,
    height_points: Option<f32>,
    resize_start: Option<(f32, f32, bool)>,
}

// Physical desktop coordinates include monitor origins and exclude the taskbar.
fn desktop_work_area(anchor_pixels: egui::Rect) -> Option<egui::Rect> {
    let center = anchor_pixels.center();
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: info has the required size and remains valid during the call.
    let success = unsafe {
        let monitor = MonitorFromPoint(
            POINT {
                x: center.x.round() as i32,
                y: center.y.round() as i32,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        GetMonitorInfoW(monitor, &mut info).as_bool()
    };
    success.then(|| {
        egui::Rect::from_min_max(
            egui::pos2(info.rcWork.left as f32, info.rcWork.top as f32),
            egui::pos2(info.rcWork.right as f32, info.rcWork.bottom as f32),
        )
    })
}

impl FontPicker {
    pub(super) fn variations(
        &self,
        family: &str,
        host_fonts: &[String],
        ctx: &egui::Context,
    ) -> Option<Vec<FontVariation>> {
        self.previews.variations(family, host_fonts, ctx)
    }

    pub(super) fn begin_frame(&mut self) {
        self.anchor = None;
        self.weight_anchor = None;
    }
    pub(super) fn button(
        &mut self,
        response: &egui::Response,
        anchor: egui::Rect,
        search_height: f32,
    ) {
        self.anchor = Some(anchor);
        let mut state = self.state.lock().unwrap();
        state.search_height_pixels = search_height * response.ctx.pixels_per_point();
        if response.clicked() {
            state.open = !state.open || state.weight_mode;
            state.weight_mode = false;
            state.focus_search = state.open;
            state.had_focus = false;
            state.visible = false;
            state.geometry = None;
            self.opening = state.open;
        }
    }
    pub(super) fn weight_button(&mut self, response: &egui::Response, weights: Vec<FontVariation>) {
        self.weight_anchor = Some(response.rect);
        self.weights = Some(Arc::new(weights));
        if response.clicked() {
            let mut state = self.state.lock().unwrap();
            state.open = !state.open || !state.weight_mode;
            state.weight_mode = true;
            state.focus_search = state.open;
            state.had_focus = false;
            state.visible = false;
            state.geometry = None;
            state.search_height_pixels = response.rect.height() * response.ctx.pixels_per_point();
            self.opening = state.open;
        }
    }
    pub(super) fn show(&mut self, ctx: &egui::Context, fonts: &[String], font: &mut String) {
        let mut state = self.state.lock().unwrap();
        if let Some(chosen) = state.chosen.take() {
            *font = chosen;
            ctx.request_repaint();
        }
        if !state.open {
            return;
        }
        let Some(anchor) = (if state.weight_mode {
            self.weight_anchor
        } else {
            self.anchor
        }) else {
            state.open = false;
            return;
        };
        if !self.opening
            && ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| !anchor.contains(p))
            })
        {
            state.open = false;
            return;
        }
        let Some(origin) = ctx.input(|i| i.viewport().inner_rect.map(|r| r.min)) else {
            state.open = false;
            return;
        };
        // Convert exactly once to physical pixels. Each child converts these with its own DPI.
        let anchor_pixels = anchor.translate(origin.to_vec2()) * ctx.pixels_per_point();
        if state.anchor_pixels != Some(anchor_pixels) {
            ctx.request_repaint_of(egui::ViewportId::from_hash_of("font-picker-desktop"));
        }
        state.anchor_pixels = Some(anchor_pixels);
        state.current = font.clone();
        let initial_position = anchor_pixels.left_bottom() / ctx.pixels_per_point();
        let creating = state.geometry.is_none();
        let visible = state.visible;
        let initial_height = if state.weight_mode {
            1.0
        } else {
            state.height_points.unwrap_or(1.0)
        };
        let weights = state.weight_mode.then(|| self.weights.clone()).flatten();
        drop(state);
        self.opening = false;
        let fonts = Arc::clone(self.fonts.get_or_insert_with(|| Arc::new(fonts.to_vec())));
        let previews = self.previews.clone();
        let state = Arc::clone(&self.state);
        let mut builder = egui::ViewportBuilder::default()
            .with_title(if weights.is_some() {
                "合成フォント — ウェイト選択"
            } else {
                "合成フォント — フォント選択"
            })
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            // Parent repaints must not hide an already presented picker.
            .with_visible(visible);
        if creating {
            builder = builder
                .with_active(false)
                .with_position(initial_position)
                .with_inner_size([
                    anchor_pixels.width() / ctx.pixels_per_point(),
                    initial_height,
                ]);
        }
        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of("font-picker-desktop"),
            builder,
            move |ctx, _class| {
                render_picker(
                    ctx,
                    &fonts,
                    weights.as_deref().map(Vec::as_slice),
                    &state,
                    &previews,
                );
            },
        );
    }
}

fn picker_geometry(
    anchor: egui::Rect,
    bounds: egui::Rect,
    preferred_height: Option<f32>,
    overhead: f32,
    row_height: f32,
    spacing: f32,
) -> (egui::Rect, usize, bool) {
    let rows_height = |rows: usize| (row_height + spacing) * rows as f32 - spacing;
    let default_height = overhead + rows_height(5);
    let desired_height = preferred_height
        .unwrap_or(default_height)
        .max(overhead + row_height);
    let below = (bounds.bottom() - anchor.bottom()).max(0.0);
    let above = (anchor.top() - bounds.top()).max(0.0);
    let down = desired_height <= below || (desired_height > above && below >= above);
    let available = if down { below } else { above };
    let height = desired_height
        .min(available.max(1.0))
        .min(bounds.height().max(1.0));
    let list_height = (height - overhead).max(1.0);
    let visible_rows = (((list_height + spacing) / (row_height + spacing)).floor() as usize).max(1);
    let left = anchor.left().clamp(bounds.left(), bounds.right());
    let available_width = (bounds.right() - left).max(1.0);
    let width = anchor.width().min(available_width);
    let position = egui::pos2(
        left,
        (if down {
            anchor.bottom()
        } else {
            anchor.top() - height
        })
        .clamp(bounds.top(), (bounds.bottom() - height).max(bounds.top())),
    );
    (
        egui::Rect::from_min_size(position, egui::vec2(width, height)),
        visible_rows,
        down,
    )
}

fn selected_font_scroll_offset(
    selected_row: Option<usize>,
    total_rows: usize,
    visible_rows: usize,
    row_height: f32,
    spacing: f32,
) -> f32 {
    let Some(selected_row) = selected_row else {
        return 0.0;
    };
    let first_visible = selected_row
        .saturating_sub(visible_rows / 2)
        .min(total_rows.saturating_sub(visible_rows));
    first_visible as f32 * (row_height + spacing)
}

fn weight_picker_geometry(
    anchor: egui::Rect,
    bounds: egui::Rect,
    desired_height: f32,
) -> egui::Rect {
    let height = desired_height.min(bounds.height()).max(1.0);
    let width = anchor.width().min(bounds.width()).max(1.0);
    let below = bounds.bottom() - anchor.bottom();
    let above = anchor.top() - bounds.top();
    let down = height <= below || (height > above && below >= above);
    let top = if down {
        anchor.bottom()
    } else {
        anchor.top() - height
    };
    egui::Rect::from_min_size(
        egui::pos2(
            anchor.left().clamp(bounds.left(), bounds.right() - width),
            top.clamp(bounds.top(), bounds.bottom() - height),
        ),
        egui::vec2(width, height),
    )
}

fn dismiss(ctx: &egui::Context, state: &mut PickerState, restore_focus: bool) {
    state.open = false;
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    ctx.request_repaint_of(egui::ViewportId::ROOT);
    if restore_focus {
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
    }
}
fn render_picker(
    ctx: &egui::Context,
    fonts: &[String],
    weights: Option<&[FontVariation]>,
    shared: &Mutex<PickerState>,
    previews: &FontPreviewCatalog,
) {
    let mut state = shared.lock().unwrap();
    if !state.open {
        return;
    }
    let (focused, close) = ctx.input(|i| {
        (
            i.viewport().focused.unwrap_or(false),
            i.viewport().close_requested() || i.key_pressed(egui::Key::Escape),
        )
    });
    if focused {
        state.had_focus = true;
    }
    if close || (state.had_focus && !focused) {
        dismiss(ctx, &mut state, close);
        return;
    }
    let Some(anchor_pixels) = state.anchor_pixels else {
        return;
    };
    let Some(bounds_pixels) = desktop_work_area(anchor_pixels) else {
        dismiss(ctx, &mut state, false);
        return;
    };
    let ppp = ctx.pixels_per_point();
    let anchor = anchor_pixels / ppp;
    let bounds = bounds_pixels / ppp;
    let style = ctx.style();
    let frame = egui::Frame::popup(&style);
    let mut row_height = style.spacing.interact_size.y.max(
        ctx.fonts_mut(|f| f.row_height(&egui::TextStyle::Button.resolve(&style)))
            + 2.0 * style.spacing.button_padding.y,
    );
    // Keep the picker geometry and row height fixed while letting each font
    // sample use almost all of the row's existing content height.
    let preview_font_size = (row_height - 2.0 * style.spacing.button_padding.y).max(1.0);
    let spacing = style.spacing.item_spacing.y;
    let search_height = state.search_height_pixels / ppp;
    if weights.is_some() {
        row_height = row_height.max(search_height);
    }
    let query = state.search.to_lowercase();
    let matches: Vec<_> = fonts
        .iter()
        .filter(|f| query.is_empty() || f.to_lowercase().contains(&query))
        .collect();
    let total_rows = weights.map_or(matches.len() + 1, <[FontVariation]>::len);
    let overhead = frame.total_margin().sum().y
        + if weights.is_some() {
            0.0
        } else {
            search_height + spacing * 2.0 + RESIZE_EDGE_HEIGHT
        };
    let (geometry, rows, opens_down) = if weights.is_some() {
        let desired_height = overhead + (row_height + spacing) * total_rows as f32 - spacing;
        let geometry = weight_picker_geometry(anchor, bounds, desired_height);
        let rows = (((geometry.height() - overhead + spacing) / (row_height + spacing)).floor()
            as usize)
            .max(1);
        (geometry, rows, geometry.top() >= anchor.bottom())
    } else {
        picker_geometry(
            anchor,
            bounds,
            state.height_points,
            overhead,
            row_height,
            spacing,
        )
    };
    let list_height = (geometry.height() - overhead).max(1.0);
    let changed_geometry = state.geometry != Some(geometry);
    if changed_geometry {
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(geometry.min));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(geometry.size()));
        state.geometry = Some(geometry);
        // Keep the old frame visible until the correctly sized next frame has painted.
        ctx.request_repaint();
    }
    let opening = state.focus_search;
    let selected_row = if let Some(weights) = weights {
        weights
            .iter()
            .position(|weight| weight.family == state.current)
    } else if state.current.is_empty() {
        Some(0)
    } else {
        matches
            .iter()
            .position(|font| font.as_str() == state.current)
            .map(|index| index + 1)
    };
    let mut search_changed = false;
    let mut selected = None;
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        if weights.is_none() {
            let search = ui.add_sized(
                [ui.available_width(), search_height],
                egui::TextEdit::singleline(&mut state.search).hint_text("フォントを検索"),
            );
            if opening {
                search.request_focus();
            }
            search_changed = search.changed();
        }
        let mut list = egui::ScrollArea::vertical()
            .id_salt("font-search-results")
            .max_height(list_height)
            .min_scrolled_height(list_height)
            .auto_shrink([false, false]);
        if opening {
            list = list.vertical_scroll_offset(selected_font_scroll_offset(
                selected_row,
                total_rows,
                rows,
                row_height,
                spacing,
            ));
        } else if search_changed {
            list = list.vertical_scroll_offset(0.0);
        }
        list.show_rows(ui, row_height, total_rows, |ui, rows| {
            for row in rows {
                let choice = if let Some(weights) = weights {
                    weights[row].family.as_str()
                } else if row == 0 {
                    ""
                } else {
                    matches[row - 1].as_str()
                };
                let label = if let Some(weights) = weights {
                    weights[row].label.as_str()
                } else if row == 0 {
                    "（変更なし）"
                } else {
                    choice
                };
                let (font_family, texture) = if weights.is_some() || row == 0 {
                    (None, None)
                } else {
                    previews.preview(choice, preview_font_size, ctx)
                };
                let text = if texture.is_some() { "" } else { label };
                let response = ui.add_sized(
                    [ui.available_width(), row_height],
                    font_selectable(
                        state.current == choice,
                        text,
                        font_family,
                        if weights.is_some() {
                            egui::TextStyle::Button.resolve(&style).size
                        } else if row == 0 {
                            16.0
                        } else {
                            preview_font_size
                        },
                    ),
                );
                if let Some(texture) = texture {
                    let ppp = ctx.pixels_per_point();
                    let size = texture.size_vec2() / ppp;
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(
                            response.rect.left() + 2.0,
                            response.rect.center().y - size.y / 2.0,
                        ),
                        size,
                    );
                    ui.painter()
                        .with_clip_rect(response.rect.shrink(2.0))
                        .image(
                            texture.id(),
                            rect,
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            ui.visuals().text_color(),
                        );
                }
                if response.clicked() {
                    selected = Some(choice.to_owned());
                }
            }
        });
        if weights.is_none() {
            ui.add_space(spacing);
            let (resize_edge, response) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), RESIZE_EDGE_HEIGHT),
                egui::Sense::drag(),
            );
            if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            let stroke =
                egui::Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color);
            ui.painter()
                .line_segment([resize_edge.left_top(), resize_edge.right_top()], stroke);
            let grip_center = resize_edge.center();
            for offset in [-3.0, 0.0, 3.0] {
                let y = grip_center.y + offset;
                ui.painter().line_segment(
                    [
                        egui::pos2(grip_center.x - 5.0, y),
                        egui::pos2(grip_center.x + 5.0, y),
                    ],
                    stroke,
                );
            }
            if response.drag_started() {
                let pointer_y = response
                    .interact_pointer_pos()
                    .map_or(resize_edge.center().y, |position| position.y);
                state.resize_start = Some((geometry.height(), pointer_y, opens_down));
            }
            if response.dragged()
                && let (Some((start_height, start_y, down)), Some(pointer)) =
                    (state.resize_start, response.interact_pointer_pos())
            {
                let max_height = if down {
                    (bounds.bottom() - anchor.bottom()).max(1.0)
                } else {
                    (anchor.top() - bounds.top()).max(1.0)
                };
                let minimum_height = (overhead + row_height).min(max_height);
                let delta = (pointer.y - start_y) * if down { 1.0 } else { -1.0 };
                state.height_points =
                    Some((start_height + delta).clamp(minimum_height, max_height));
                ctx.request_repaint();
            }
            if response.drag_stopped() {
                state.resize_start = None;
            }
            response.on_hover_text("下辺を上下にドラッグして表示領域を変更");
        }
    });
    state.focus_search = false;
    if search_changed {
        ctx.request_repaint();
    }
    if let Some(selected) = selected {
        state.chosen = Some(selected);
        dismiss(ctx, &mut state, true);
    } else if !state.visible && !changed_geometry {
        // Show only after a full frame at the final geometry has been presented.
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        state.visible = true;
    }
    ctx.output_mut(|output| {
        if let Some(ime) = &mut output.ime {
            ime.rect = ime.cursor_rect;
        }
    });
}

fn font_selectable(
    selected: bool,
    label: &str,
    family: Option<String>,
    font_size: f32,
) -> egui::Button<'static> {
    let font_id = egui::FontId::new(
        font_size,
        family.map_or(egui::FontFamily::Proportional, |name| {
            egui::FontFamily::Name(name.into())
        }),
    );
    let job = egui::text::LayoutJob::simple_singleline(
        label.to_owned(),
        font_id,
        egui::Color32::PLACEHOLDER,
    );
    egui::Button::selectable(selected, (job.atom_shrink(true), egui::Atom::grow())).truncate()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parent_repaints_keep_the_presented_picker_visible_and_do_not_refocus_it() {
        let ctx = egui::Context::default();
        ctx.set_embed_viewports(false);
        let field = egui::Rect::from_min_size(egui::pos2(100.0, 200.0), egui::vec2(420.0, 32.0));
        let mut picker = FontPicker::default();
        {
            let mut state = picker.state.lock().unwrap();
            state.open = true;
            state.visible = true;
            state.geometry = Some(egui::Rect::from_min_size(
                field.left_bottom(),
                egui::vec2(420.0, 240.0),
            ));
        }
        for _ in 0..3 {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960.0, 720.0),
                )),
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .inner_rect = Some(egui::Rect::from_min_size(
                egui::pos2(80.0, 80.0),
                egui::vec2(960.0, 720.0),
            ));
            let output = ctx.run(input, |ctx| {
                picker.begin_frame();
                picker.anchor = Some(field);
                picker.show(ctx, &["Meiryo UI".into()], &mut "Meiryo UI".into());
            });
            let viewport =
                &output.viewport_output[&egui::ViewportId::from_hash_of("font-picker-desktop")];
            assert_eq!(viewport.builder.visible, Some(true));
            assert_eq!(viewport.builder.active, None);
            assert_eq!(viewport.builder.position, None);
            assert!(picker.state.lock().unwrap().open);
        }
    }

    #[test]
    fn uses_fixed_default_viewport_and_flips_at_work_area_bottom() {
        let work = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1920.0, 1040.0));
        // An editor can end at y=800; desktop space still admits the entire list.
        let field = egui::Rect::from_min_max(egui::pos2(300.0, 700.0), egui::pos2(750.0, 732.0));
        let (below, rows, opens_down) = picker_geometry(field, work, None, 50.0, 30.0, 10.0);
        assert_eq!(rows, 5);
        assert!(opens_down);
        assert_eq!(below.min, field.left_bottom());
        assert_eq!(below.height(), 240.0);
        assert!(below.bottom() > 800.0);
        let field = field.translate(egui::vec2(0.0, 220.0));
        let (above, rows, opens_down) = picker_geometry(field, work, None, 50.0, 30.0, 10.0);
        assert_eq!(rows, 5);
        assert!(!opens_down);
        assert_eq!(above.bottom(), field.top());
        assert_eq!(above.left(), field.left());
        assert!(work.contains_rect(above));
    }

    #[test]
    fn weight_menu_fits_all_rows_using_the_desktop_instead_of_editor_height() {
        let work = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1920.0, 1040.0));
        let field = egui::Rect::from_min_size(egui::pos2(300.0, 500.0), egui::vec2(142.0, 32.0));
        let popup = weight_picker_geometry(field, work, 324.0);
        assert_eq!(popup.min, field.left_bottom());
        assert_eq!(popup.height(), 324.0);
        // Even if neither side has enough room, show the full menu by shifting
        // it within the desktop, rather than imposing the parent window limit.
        let tall = weight_picker_geometry(field, work, 900.0);
        assert_eq!(tall.height(), 900.0);
        assert!(work.contains_rect(tall));
    }

    #[test]
    fn oversized_weight_menu_clamps_to_monitor_work_area() {
        let work = egui::Rect::from_min_max(egui::pos2(-1920.0, -200.0), egui::pos2(0.0, 840.0));
        let field = egui::Rect::from_min_size(egui::pos2(-100.0, 700.0), egui::vec2(142.0, 32.0));
        let popup = weight_picker_geometry(field, work, 2000.0);
        assert_eq!(popup.height(), work.height());
        assert_eq!(popup.width(), field.width());
        assert!(work.contains_rect(popup));
        let shorter = weight_picker_geometry(field, work, 324.0);
        assert_eq!(shorter.bottom(), field.top());
        assert!(work.contains_rect(shorter));
    }

    #[test]
    fn selected_font_opens_near_the_middle_and_clamps_at_list_ends() {
        let row_height = 30.0;
        let spacing = 4.0;
        assert_eq!(
            selected_font_scroll_offset(Some(50), 101, 5, row_height, spacing),
            48.0 * (row_height + spacing)
        );
        assert_eq!(
            selected_font_scroll_offset(Some(100), 101, 5, row_height, spacing),
            96.0 * (row_height + spacing)
        );
        assert_eq!(
            selected_font_scroll_offset(Some(1), 101, 5, row_height, spacing),
            0.0
        );
        assert_eq!(
            selected_font_scroll_offset(None, 101, 5, row_height, spacing),
            0.0
        );
    }

    #[test]
    fn negative_monitor_origin_and_short_work_area_keep_popup_inside_bounds() {
        let work = egui::Rect::from_min_max(egui::pos2(-1920.0, 0.0), egui::pos2(0.0, 300.0));
        let field = egui::Rect::from_min_max(egui::pos2(-300.0, 100.0), egui::pos2(150.0, 132.0));
        let (popup, rows, _) = picker_geometry(field, work, None, 50.0, 30.0, 10.0);
        assert_eq!(rows, 3);
        assert_eq!(popup.height(), 168.0);
        assert_eq!(popup.right(), work.right());
        assert!(work.contains_rect(popup));
    }

    #[test]
    fn resized_height_is_remembered_and_clamped_to_desktop_space() {
        let work = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 800.0));
        let field = egui::Rect::from_min_size(egui::pos2(200.0, 100.0), egui::vec2(320.0, 32.0));
        let (expanded, rows, opens_down) =
            picker_geometry(field, work, Some(400.0), 80.0, 30.0, 4.0);
        assert!(opens_down);
        assert!(rows > 5);
        assert_eq!(expanded.height(), 400.0);
        assert_eq!(expanded.width(), field.width());

        let short_work = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 300.0));
        let near_middle =
            egui::Rect::from_min_size(egui::pos2(200.0, 130.0), egui::vec2(320.0, 32.0));
        let (shrunk, visible_rows, opens_down) =
            picker_geometry(near_middle, short_work, Some(400.0), 80.0, 30.0, 4.0);
        assert!(opens_down);
        assert_eq!(visible_rows, 1);
        assert_eq!(shrunk.height(), 138.0);
        assert!(short_work.contains_rect(shrunk));
    }
}
