use crate::input_utils::InputUtils;
use crate::types::{ClipboardContent, ClipboardItem};
use arboard::Clipboard;
use eframe::egui;
use std::sync::{mpsc::Receiver, Arc, Mutex};
use std::time::{Duration, Instant};

pub struct ClipboardApp {
    history: Arc<Mutex<Vec<ClipboardItem>>>,
    show_signal: Receiver<()>,
    visible: bool,
    last_focus_check: Instant,
    texture_cache: std::collections::HashMap<usize, egui::TextureHandle>,
}

impl ClipboardApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        history: Arc<Mutex<Vec<ClipboardItem>>>,
        show_signal: Receiver<()>,
        start_hidden: bool,
    ) -> Self {
        Self {
            history,
            show_signal,
            visible: !start_hidden,
            last_focus_check: Instant::now(),
            texture_cache: std::collections::HashMap::new(),
        }
    }
}

impl eframe::App for ClipboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for external show signals
        if let Ok(_) = self.show_signal.try_recv() {
            self.visible = true;
            self.last_focus_check = Instant::now();

            let (x, y) = InputUtils::get_mouse_position();

            // Restore window properties when showing
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([350.0, 450.0].into()));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([x, y].into()));
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        if !self.visible {
            std::thread::sleep(Duration::from_millis(100));
            ctx.request_repaint();
            return;
        }

        if self.last_focus_check.elapsed() > Duration::from_millis(500) {
            if !ctx.input(|i| i.focused) {
                println!("Lost focus, hiding...");
                self.visible = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Clipboard History");
            ui.separator();

            let mut history = self.history.lock().unwrap();
            let mut selected_idx = None;

            if history.is_empty() {
                ui.label("Clipboard history is empty.");
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.set_width(ui.available_width());

                for (i, item) in history.iter().enumerate() {
                    let clicked = ui.push_id(i, |ui| {
                        ui.vertical_centered_justified(|ui| {
                            match &item.content {
                                ClipboardContent::Text(text) => {
                                    let display_text = if text.len() > 100 {
                                        format!("{}...", &text[..100])
                                    } else {
                                        text.clone()
                                    };
                                    if ui.button(display_text).clicked() {
                                        return true;
                                    }
                                }
                                ClipboardContent::Image(img_data) => {
                                    let texture = self.texture_cache.entry(i).or_insert_with(|| {
                                        let image = egui::ColorImage::from_rgba_unmultiplied(
                                            [img_data.width, img_data.height],
                                            &img_data.bytes,
                                        );
                                        ctx.load_texture(
                                            format!("img_{}", i),
                                            image,
                                            egui::TextureOptions::LINEAR,
                                        )
                                    });

                                    let max_height = 150.0;
                                    let aspect = img_data.width as f32 / img_data.height as f32;
                                    let width = ui.available_width();
                                    let height = (width / aspect).min(max_height);
                                    let size = egui::vec2(width * 0.9, height);

                                    let image = egui::Image::new(&*texture)
                                        .fit_to_exact_size(size);

                                    if ui.add(egui::Button::image(image)).clicked() {
                                        return true;
                                    }
                                }
                            }
                            false
                        }).inner
                    }).inner;

                    if clicked {
                        selected_idx = Some(i);
                    }

                    ui.separator();
                }
            });

            if let Some(idx) = selected_idx {
                let item = history[idx].clone();

                history.remove(idx);
                history.insert(0, item.clone());

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

                println!("Item selected, pasting and hiding...");
                self.visible = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));

                InputUtils::paste_content();
            }
        });
    }
}
