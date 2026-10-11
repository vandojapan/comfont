use super::left_selectable;
use eframe::egui;
use std::sync::{Arc, Mutex};
use windows::Win32::{
    Foundation::POINT,
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
};

#[derive(Default)]
pub(super) struct FontPicker {
    state: Arc<Mutex<PickerState>>,
    anchor: Option<egui::Rect>,
    fonts: Option<Arc<Vec<String>>>,
    opening: bool,
}
#[derive(Default)]
struct PickerState {
    open: bool,
    focus_search: bool,
    had_focus: bool,
    visible: bool,
    search: String,
    current: String,
    chosen: Option<String>,
    anchor_pixels: Option<egui::Rect>,
    search_height_pixels: f32,
    geometry: Option<egui::Rect>,
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
    pub(super) fn begin_frame(&mut self) {
        self.anchor = None;
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
            state.open = !state.open;
            state.focus_search = state.open;
            state.had_focus = false;
            state.visible = false;
            state.geometry = None;
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
        let Some(anchor) = self.anchor else {
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
        let initial_width = anchor_pixels.width() / ctx.pixels_per_point();
        drop(state);
        self.opening = false;
        let fonts = Arc::clone(self.fonts.get_or_insert_with(|| Arc::new(fonts.to_vec())));
        let state = Arc::clone(&self.state);
        let mut builder = egui::ViewportBuilder::default()
            .with_title("合成フォント — フォント選択")
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
                .with_inner_size([initial_width, 1.0]);
        }
        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of("font-picker-desktop"),
            builder,
            move |ctx, _class| {
                render_picker(ctx, &fonts, &state);
            },
        );
    }
}

fn picker_geometry(
    anchor: egui::Rect,
    bounds: egui::Rect,
    total_rows: usize,
    overhead: f32,
    row_height: f32,
    spacing: f32,
) -> (egui::Rect, usize) {
    let rows_height = |rows: usize| (row_height + spacing) * rows as f32 - spacing;
    let desired_height = overhead + rows_height(total_rows.min(5));
    let below = (bounds.bottom() - anchor.bottom()).max(0.0);
    let above = (anchor.top() - bounds.top()).max(0.0);
    let down = desired_height <= below || (desired_height > above && below >= above);
    let available = if down { below } else { above };
    let rows = (((available - overhead + spacing) / (row_height + spacing)).floor() as usize)
        .clamp(1, 5)
        .min(total_rows);
    let height = overhead + rows_height(rows);
    let width = anchor.width().min(bounds.width());
    let position = egui::pos2(
        anchor
            .left()
            .clamp(bounds.left(), (bounds.right() - width).max(bounds.left())),
        (if down {
            anchor.bottom()
        } else {
            anchor.top() - height
        })
        .clamp(bounds.top(), (bounds.bottom() - height).max(bounds.top())),
    );
    (
        egui::Rect::from_min_size(position, egui::vec2(width, height)),
        rows,
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
fn render_picker(ctx: &egui::Context, fonts: &[String], shared: &Mutex<PickerState>) {
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
    let row_height = style.spacing.interact_size.y.max(
        ctx.fonts_mut(|f| f.row_height(&egui::TextStyle::Button.resolve(&style)))
            + 2.0 * style.spacing.button_padding.y,
    );
    let spacing = style.spacing.item_spacing.y;
    let search_height = state.search_height_pixels / ppp;
    let query = state.search.to_lowercase();
    let matches: Vec<_> = fonts
        .iter()
        .filter(|f| query.is_empty() || f.to_lowercase().contains(&query))
        .collect();
    let total_rows = matches.len() + 1;
    let rows_height = |rows: usize| (row_height + spacing) * rows as f32 - spacing;
    let overhead = search_height + spacing + frame.total_margin().sum().y;
    let (geometry, rows) =
        picker_geometry(anchor, bounds, total_rows, overhead, row_height, spacing);
    let changed_geometry = state.geometry != Some(geometry);
    if changed_geometry {
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(geometry.min));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(geometry.size()));
        state.geometry = Some(geometry);
        // Keep the old frame visible until the correctly sized next frame has painted.
        ctx.request_repaint();
    }
    let opening = state.focus_search;
    let mut search_changed = false;
    let mut selected = None;
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        let search = ui.add_sized(
            [ui.available_width(), search_height],
            egui::TextEdit::singleline(&mut state.search).hint_text("フォントを検索"),
        );
        if opening {
            search.request_focus();
        }
        search_changed = search.changed();
        let mut list = egui::ScrollArea::vertical()
            .id_salt("font-search-results")
            .max_height(rows_height(rows))
            .min_scrolled_height(rows_height(rows))
            .auto_shrink([false, false]);
        if opening || search_changed {
            list = list.vertical_scroll_offset(0.0);
        }
        list.show_rows(ui, row_height, total_rows, |ui, rows| {
            for row in rows {
                let choice = if row == 0 {
                    ""
                } else {
                    matches[row - 1].as_str()
                };
                let label = if row == 0 {
                    "（変更なし）"
                } else {
                    choice
                };
                if ui
                    .add_sized(
                        [ui.available_width(), row_height],
                        left_selectable(state.current == choice, label),
                    )
                    .clicked()
                {
                    selected = Some(choice.to_owned());
                }
            }
        });
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
    fn uses_desktop_space_beyond_editor_and_flips_at_work_area_bottom() {
        let work = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1920.0, 1040.0));
        // An editor can end at y=800; desktop space still admits the entire list.
        let field = egui::Rect::from_min_max(egui::pos2(300.0, 700.0), egui::pos2(750.0, 732.0));
        let (below, rows) = picker_geometry(field, work, 200, 50.0, 30.0, 10.0);
        assert_eq!(rows, 5);
        assert_eq!(below.min, field.left_bottom());
        assert_eq!(below.height(), 240.0);
        assert!(below.bottom() > 800.0);
        let field = field.translate(egui::vec2(0.0, 220.0));
        let (above, rows) = picker_geometry(field, work, 200, 50.0, 30.0, 10.0);
        assert_eq!(rows, 5);
        assert_eq!(above.bottom(), field.top());
        assert_eq!(above.left(), field.left());
        assert!(work.contains_rect(above));
        // Search shrink can make downwards placement fit again.
        let (small, rows) = picker_geometry(field, work, 1, 50.0, 30.0, 10.0);
        assert_eq!(rows, 1);
        assert_eq!(small.top(), field.bottom());
    }
    #[test]
    fn negative_monitor_origin_and_short_work_area_keep_whole_rows() {
        let work = egui::Rect::from_min_max(egui::pos2(-1920.0, 0.0), egui::pos2(0.0, 300.0));
        let field = egui::Rect::from_min_max(egui::pos2(-300.0, 100.0), egui::pos2(150.0, 132.0));
        let (popup, rows) = picker_geometry(field, work, 200, 50.0, 30.0, 10.0);
        assert_eq!(rows, 3);
        assert_eq!(popup.height(), 160.0);
        assert_eq!(popup.right(), work.right());
        assert!(work.contains_rect(popup));
    }
}
