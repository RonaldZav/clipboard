use crate::types::{ClipboardContent, ClipboardItem};
use arboard::Clipboard;
use egui;
use std::collections::HashMap;

pub fn draw_ui(
    ctx: &egui::Context,
    history: &mut Vec<ClipboardItem>,
    popup_origin: egui::Pos2,
    texture_cache: &mut HashMap<usize, egui::TextureHandle>,
) -> Option<ClipboardItem> {
    let mut selected_item: Option<ClipboardItem> = None;

    egui::Window::new("clipboard_popup")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .fixed_pos(popup_origin)
        .fixed_size([350.0, 450.0])
        .show(ctx, |ui| {
            ui.heading("Clipboard History");
            ui.separator();

            if history.is_empty() {
                ui.label("Clipboard history is empty.");
                return;
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.set_width(ui.available_width());

                for (i, item) in history.iter().enumerate() {
                    let clicked = ui.push_id(i, |ui| {
                        ui.vertical_centered_justified(|ui| {
                            match &item.content {
                                ClipboardContent::Text(text) => {
                                    let display_text = if text.chars().count() > 100 {
                                        let truncated: String = text.chars().take(100).collect();
                                        format!("{}...", truncated)
                                    } else {
                                        text.clone()
                                    };
                                    ui.button(display_text).clicked()
                                }
                                ClipboardContent::Image(img_data) => {
                                    let texture =
                                        texture_cache.entry(i).or_insert_with(|| {
                                            let image =
                                                egui::ColorImage::from_rgba_unmultiplied(
                                                    [img_data.width, img_data.height],
                                                    &img_data.bytes,
                                                );
                                            ctx.load_texture(
                                                format!("img_{}", i),
                                                image,
                                                egui::TextureOptions::LINEAR,
                                            )
                                        });

                                    let max_height = 150.0_f32;
                                    let aspect =
                                        img_data.width as f32 / img_data.height as f32;
                                    let width = ui.available_width();
                                    let height = (width / aspect).min(max_height);
                                    let size = egui::vec2(width * 0.9, height);

                                    ui.add(egui::Button::image(
                                        egui::Image::new(&*texture).fit_to_exact_size(size),
                                    ))
                                    .clicked()
                                }
                            }
                        })
                        .inner
                    })
                    .inner;

                    if clicked {
                        selected_item = Some(history[i].clone());
                    }

                    ui.separator();
                }
            });
        });

    if let Some(ref item) = selected_item {
        // Move selected item to front
        if let Some(pos) = history.iter().position(|x| x == item) {
            let removed = history.remove(pos);
            history.insert(0, removed);
        }

        // Put it on the system clipboard
        if let Ok(mut clipboard) = Clipboard::new() {
            match &item.content {
                ClipboardContent::Text(text) => {
                    let _ = clipboard.set_text(text.clone());
                }
                ClipboardContent::Image(img) => {
                    let _ = clipboard.set_image(arboard::ImageData {
                        width: img.width,
                        height: img.height,
                        bytes: std::borrow::Cow::Borrowed(&img.bytes),
                    });
                }
            }
        }
    }

    selected_item
}
