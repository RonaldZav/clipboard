use crate::types::{ClipboardContent, ClipboardItem, ImageData};
use arboard::Clipboard;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct ClipboardMonitor {
    history: Arc<Mutex<Vec<ClipboardItem>>>,
}

impl ClipboardMonitor {
    pub fn new(history: Arc<Mutex<Vec<ClipboardItem>>>) -> Self {
        Self { history }
    }

    pub fn start(&self) {
        let history = self.history.clone();

        thread::spawn(move || {
            let mut clipboard = match Clipboard::new() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to initialize clipboard: {}", e);
                    return;
                }
            };
            
            let mut last_inserted_item: Option<ClipboardItem> = None;

            loop {
                let mut new_item: Option<ClipboardItem> = None;

                // Check for Text
                if let Ok(text) = clipboard.get_text() {
                    if !text.trim().is_empty() {
                        // Check if it matches the last thing we inserted
                        let is_same_as_last = if let Some(last) = &last_inserted_item {
                            match &last.content {
                                ClipboardContent::Text(t) => t == &text,
                                _ => false,
                            }
                        } else {
                            false
                        };

                        if !is_same_as_last {
                             new_item = Some(ClipboardItem {
                                content: ClipboardContent::Text(text),
                            });
                        }
                    }
                }

                // Check for Image (only if no text change detected)
                if new_item.is_none() {
                    if let Ok(image) = clipboard.get_image() {
                        let is_same_as_last = if let Some(last) = &last_inserted_item {
                            match &last.content {
                                ClipboardContent::Image(img) => {
                                    img.width == image.width
                                    && img.height == image.height
                                    && img.bytes == image.bytes.as_ref()
                                },
                                _ => false,
                            }
                        } else {
                            false
                        };

                        if !is_same_as_last {
                            new_item = Some(ClipboardItem {
                                content: ClipboardContent::Image(ImageData {
                                    width: image.width,
                                    height: image.height,
                                    bytes: image.bytes.into_owned(),
                                }),
                            });
                        }
                    }
                }

                if let Some(item) = new_item {
                    let mut history_guard = history.lock().unwrap();

                    // Double check against the very top of the history too (in case it was modified elsewhere)
                    let is_duplicate_top = if let Some(top) = history_guard.first() {
                        top == &item
                    } else {
                        false
                    };

                    if !is_duplicate_top {
                        // If it exists elsewhere in history, move it to top
                        if let Some(pos) = history_guard.iter().position(|x| x == &item) {
                            history_guard.remove(pos);
                        }

                        history_guard.insert(0, item.clone());

                        // Limit history size to 30
                        if history_guard.len() > 30 {
                            history_guard.pop();
                        }

                        last_inserted_item = Some(item);
                    }
                }

                thread::sleep(Duration::from_millis(500));
            }
        });
    }
}
