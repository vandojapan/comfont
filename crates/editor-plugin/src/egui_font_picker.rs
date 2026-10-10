use super::left_selectable;
use eframe::egui;
use windows::Win32::{
    Foundation::POINT,
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
};

#[derive(Default)]
pub(super) struct FontPicker {
    open: bool,
    opening: bool,
    had_focus: bool,
    anchor: Option<egui::Rect>,
    search_height: f32,
    search: String,
    pixels_per_point: Option<f32>,
}

// Coordinates stay in egui points until the Win32 monitor lookup. rcWork includes
// the monitor origin (also for monitors left of the primary) and excludes taskbars.
fn desktop_work_area(anchor: egui::Rect, pixels_per_point: f32) -> Option<egui::Rect> {
    let center = anchor.center() * pixels_per_point;
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: info has the required size and remains valid during the synchronous call.
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
            egui::pos2(info.rcWork.left as f32, info.rcWork.top as f32) / pixels_per_point,
            egui::pos2(info.rcWork.right as f32, info.rcWork.bottom as f32) / pixels_per_point,
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
        self.search_height = search_height;
        if response.clicked() {
            self.open = !self.open;
            self.opening = self.open;
            self.had_focus = false;
            self.pixels_per_point = None;
        }
    }

    pub(super) fn show(&mut self, ctx: &egui::Context, fonts: &[String], font: &mut String) {
        if !self.open {
            return;
        }
        let Some(local_anchor) = self.anchor else {
            self.open = false;
            return;
        };
        // A click in the editor outside the field dismisses the separate viewport.
        if !self.opening
            && ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| !local_anchor.contains(p))
            })
        {
            self.open = false;
            return;
        }
        let Some(origin) = ctx.input(|i| i.viewport().inner_rect.map(|r| r.min)) else {
            self.open = false;
            return;
        };
        let screen_anchor = local_anchor.translate(origin.to_vec2());
        let popup_scale = self.pixels_per_point.unwrap_or(ctx.pixels_per_point());
        let scale = ctx.pixels_per_point() / popup_scale;
        let anchor = egui::Rect::from_min_max(screen_anchor.min * scale, screen_anchor.max * scale);
        let Some(bounds) = desktop_work_area(anchor, popup_scale) else {
            self.open = false;
            return;
        };
        let style = ctx.style();
        let frame = egui::Frame::popup(&style);
        let row_height = style.spacing.interact_size.y.max(
            ctx.fonts_mut(|f| f.row_height(&egui::TextStyle::Button.resolve(&style)))
                + 2.0 * style.spacing.button_padding.y,
        );
        let spacing = style.spacing.item_spacing.y;
        let rows_height = |rows: usize| (row_height + spacing) * rows as f32 - spacing;
        let query = self.search.to_lowercase();
        let desired_rows = (fonts
            .iter()
            .filter(|f| query.is_empty() || f.to_lowercase().contains(&query))
            .count()
            + 1)
        .min(5);
        let overhead = self.search_height + spacing + frame.total_margin().sum().y;
        let desired_height = overhead + rows_height(desired_rows);
        let below = (bounds.bottom() - anchor.bottom()).max(0.0);
        let above = (anchor.top() - bounds.top()).max(0.0);
        let open_below = desired_height <= below || (desired_height > above && below >= above);
        let available = if open_below { below } else { above };
        let fitting_rows = (((available - overhead + spacing) / (row_height + spacing)).floor()
            as usize)
            .clamp(1, 5);
        let height = overhead + rows_height(desired_rows.min(fitting_rows));
        let width = anchor.width().min(bounds.width());
        let position = egui::pos2(
            anchor
                .left()
                .clamp(bounds.left(), (bounds.right() - width).max(bounds.left())),
            if open_below {
                anchor.bottom()
            } else {
                anchor.top() - height
            },
        );
        let opening = self.opening;
        let mut restore_focus = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("font-picker-desktop"),
            egui::ViewportBuilder::default()
                .with_title("合成フォント — フォント選択")
                .with_decorations(false)
                .with_resizable(false)
                .with_taskbar(false)
                .with_window_level(egui::WindowLevel::AlwaysOnTop)
                .with_active(true)
                .with_position(position)
                .with_inner_size([width, height]),
            |popup_ctx, _class| {
                // A newly created native viewport may use a different monitor DPI.
                // Rebuild its position in its own point scale, preserving physical alignment.
                let actual_scale = popup_ctx.pixels_per_point();
                if (actual_scale - popup_scale).abs() > 0.001 {
                    self.pixels_per_point = Some(actual_scale);
                    popup_ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
                let (focused, close) = popup_ctx.input(|i| {
                    (
                        i.viewport().focused.unwrap_or(false),
                        i.viewport().close_requested() || i.key_pressed(egui::Key::Escape),
                    )
                });
                if focused {
                    self.had_focus = true;
                }
                if close || (self.had_focus && !focused) {
                    self.open = false;
                    restore_focus = close;
                }
                egui::CentralPanel::default()
                    .frame(frame)
                    .show(popup_ctx, |ui| {
                        let search = ui.add_sized(
                            [ui.available_width(), self.search_height],
                            egui::TextEdit::singleline(&mut self.search)
                                .hint_text("フォントを検索"),
                        );
                        if opening {
                            search.request_focus();
                            popup_ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        }
                        let query = self.search.to_lowercase();
                        let matches: Vec<_> = fonts
                            .iter()
                            .filter(|f| query.is_empty() || f.to_lowercase().contains(&query))
                            .collect();
                        let total_rows = matches.len() + 1;
                        let list_height = rows_height(total_rows.min(fitting_rows));
                        let mut list = egui::ScrollArea::vertical()
                            .id_salt("font-search-results")
                            .max_height(list_height)
                            .min_scrolled_height(list_height)
                            .auto_shrink([false, false]);
                        if opening || search.changed() {
                            list = list.vertical_scroll_offset(0.0);
                        }
                        if search.changed() {
                            popup_ctx.request_repaint();
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
                                        left_selectable(font == choice, label),
                                    )
                                    .clicked()
                                {
                                    *font = choice.to_owned();
                                    self.open = false;
                                    restore_focus = true;
                                }
                            }
                        });
                    });
                popup_ctx.output_mut(|output| {
                    if let Some(ime) = &mut output.ime {
                        ime.rect = ime.cursor_rect;
                    }
                });
            },
        );
        self.opening = false;
        if !self.open {
            ctx.request_repaint();
            if restore_focus {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
    }
}
