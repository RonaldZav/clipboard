use crate::types::{ClipboardContent, ClipboardItem, ImageData};
use arboard::Clipboard;
use gtk4::gdk;
use gtk4::gdk_pixbuf::{Colorspace, InterpType, Pixbuf};
use gtk4::glib::{self, Bytes, ControlFlow, Propagation};
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Box as GtkBox, Button, EventControllerKey, Label, ListBox, ListBoxRow, Orientation, Picture, ScrolledWindow, SelectionMode};
use std::cell::RefCell;
use std::convert::TryFrom;
use std::rc::Rc;
use std::sync::{mpsc::Receiver, Arc, Mutex};
use std::time::Duration;

pub fn run(history: Arc<Mutex<Vec<ClipboardItem>>>, show_rx: Receiver<()>, stop_rx: Receiver<()>, start_hidden: bool) {
    let app = Application::builder()
        .application_id("io.github.ronaldzav.clipboard")
        .build();

    let show_rx = Rc::new(RefCell::new(show_rx));
    let stop_rx = Rc::new(RefCell::new(stop_rx));

    app.connect_activate(move |app| {
        let ui = Rc::new(RefCell::new(ClipboardUi::new(app.clone(), history.clone())));

        if !start_hidden {
            ui.borrow_mut().show();
        }

        let ui_for_tick = ui.clone();
        let show_rx = show_rx.clone();
        let stop_rx = stop_rx.clone();
        let app_for_tick = app.clone();

        glib::timeout_add_local(Duration::from_millis(120), move || {
            while show_rx.borrow_mut().try_recv().is_ok() {
                ui_for_tick.borrow_mut().show();
            }

            if stop_rx.borrow_mut().try_recv().is_ok() {
                app_for_tick.quit();
                return ControlFlow::Break;
            }

            if ui_for_tick.borrow().is_visible() {
                ui_for_tick.borrow_mut().refresh();
            }

            ControlFlow::Continue
        });
    });

    app.run();
}

struct ClipboardUi {
    app: Application,
    history: Arc<Mutex<Vec<ClipboardItem>>>,
    window: Option<ApplicationWindow>,
    list_box: Option<ListBox>,
}

impl ClipboardUi {
    fn new(app: Application, history: Arc<Mutex<Vec<ClipboardItem>>>) -> Self {
        Self {
            app,
            history,
            window: None,
            list_box: None,
        }
    }

    fn is_visible(&self) -> bool {
        self.window.as_ref().is_some_and(|window| window.is_visible())
    }

    fn show(&mut self) {
        if self.window.is_none() {
            self.window = Some(self.build_window());
        }

        self.refresh();

        if let Some(window) = self.window.as_ref() {
            window.present();
            window.set_visible(true);
            window.grab_focus();
        }
    }

    fn refresh(&mut self) {
        let Some(list_box) = self.list_box.as_ref() else {
            return;
        };

        let window = self.window.as_ref().cloned();

        while let Some(child) = list_box.first_child() {
            list_box.remove(&child);
        }

        let items = {
            let history = self.history.lock().unwrap();
            history.clone()
        };

        if items.is_empty() {
            let row = ListBoxRow::new();
            row.set_selectable(false);
            row.set_activatable(false);
            row.set_child(Some(&self.empty_state_label()));
            list_box.append(&row);
            return;
        }

        for item in items {
            let row = self.build_row(item, window.clone());
            list_box.append(&row);
        }
    }

    fn build_window(&mut self) -> ApplicationWindow {
        let window = ApplicationWindow::builder()
            .application(&self.app)
            .title("Clipboard")
            .default_width(420)
            .default_height(560)
            .resizable(true)
            .decorated(false)
            .build();

        let root = GtkBox::new(Orientation::Vertical, 12);
        root.set_margin_top(16);
        root.set_margin_bottom(16);
        root.set_margin_start(16);
        root.set_margin_end(16);

        let title = Label::new(Some("Clipboard History"));
        title.add_css_class("title-1");
        title.set_xalign(0.0);
        root.append(&title);

        let subtitle = Label::new(Some("Press Esc to close. Click an item to copy it back and close."));
        subtitle.add_css_class("dim-label");
        subtitle.set_wrap(true);
        subtitle.set_xalign(0.0);
        root.append(&subtitle);

        let list_box = ListBox::new();
        list_box.set_selection_mode(SelectionMode::None);
        list_box.add_css_class("boxed-list");

        let scroller = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .child(&list_box)
            .build();
        scroller.set_vexpand(true);
        root.append(&scroller);

        window.set_child(Some(&root));

        let window_for_close = window.clone();
        window.connect_close_request(move |_| {
            window_for_close.hide();
            Propagation::Stop
        });

        let window_for_key = window.clone();
        let key_controller = EventControllerKey::new();
        key_controller.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                window_for_key.hide();
                return Propagation::Stop;
            }

            Propagation::Proceed
        });
        window.add_controller(key_controller);

        let window_for_focus = window.clone();
        window.connect_is_active_notify(move |win| {
            if !win.is_active() {
                window_for_focus.hide();
            }
        });

        self.list_box = Some(list_box);
        self.install_css();

        window
    }

    fn build_row(&self, item: ClipboardItem, window: Option<ApplicationWindow>) -> ListBoxRow {
        let row = ListBoxRow::new();
        row.set_activatable(true);

        let button = Button::new();
        button.add_css_class("flat");
        button.set_hexpand(true);
        button.set_halign(gtk4::Align::Fill);

        let content_widget = match &item.content {
            ClipboardContent::Text(text) => {
                let mut preview: String = text.chars().take(120).collect();
                if text.chars().count() > 120 {
                    preview.push_str("...");
                }

                let label = Label::new(Some(&preview));
                label.set_wrap(true);
                label.set_xalign(0.0);
                label.upcast::<gtk4::Widget>()
            }
            ClipboardContent::Image(image) => self.build_image_preview(image),
        };

        button.set_child(Some(&content_widget));

        let history = self.history.clone();
        let item_for_click = item.clone();
        let window_for_click = window.clone();
        button.connect_clicked(move |_| {
            apply_item_to_clipboard(&item_for_click);

            let mut history_guard = history.lock().unwrap();
            if let Some(position) = history_guard.iter().position(|existing| existing == &item_for_click) {
                let selected = history_guard.remove(position);
                history_guard.insert(0, selected);
            }

            if let Some(window) = window_for_click.as_ref() {
                window.hide();
            }
        });

        row.set_child(Some(&button));
        row
    }

    fn build_image_preview(&self, image: &ImageData) -> gtk4::Widget {
        let preview_box = GtkBox::new(Orientation::Horizontal, 12);

        if let Some(picture) = Self::make_picture(image) {
            preview_box.append(&picture);
        }

        let details = GtkBox::new(Orientation::Vertical, 4);
        let title = Label::new(Some("Image"));
        title.set_xalign(0.0);
        title.add_css_class("title-3");

        let size = Label::new(Some(&format!("{} x {}", image.width, image.height)));
        size.set_xalign(0.0);
        size.add_css_class("dim-label");

        details.append(&title);
        details.append(&size);
        preview_box.append(&details);

        preview_box.upcast::<gtk4::Widget>()
    }

    fn make_picture(image: &ImageData) -> Option<Picture> {
        let width = i32::try_from(image.width).ok()?;
        let height = i32::try_from(image.height).ok()?;
        let rowstride = width.checked_mul(4)?;
        let pixel_count = i64::from(width) * i64::from(height);
        let expected_bytes = usize::try_from(pixel_count.checked_mul(4)?).ok()?;

        if image.bytes.len() < expected_bytes {
            return None;
        }

        let bytes = Bytes::from_owned(image.bytes.clone());
        let pixbuf = Pixbuf::from_bytes(&bytes, Colorspace::Rgb, true, 8, width, height, rowstride);
        let thumb = pixbuf.scale_simple(84, 84, InterpType::Bilinear).unwrap_or(pixbuf);

        #[allow(deprecated)]
        let picture = Picture::for_pixbuf(&thumb);
        picture.set_can_shrink(true);
        picture.set_size_request(84, 84);

        Some(picture)
    }

    fn empty_state_label(&self) -> Label {
        let label = Label::new(Some("Clipboard history is empty."));
        label.set_xalign(0.0);
        label.add_css_class("dim-label");
        label
    }

    fn install_css(&self) {
        let provider = gtk4::CssProvider::new();
        let css = r#"
            window {
                background: linear-gradient(180deg, rgba(24, 26, 32, 0.96), rgba(16, 18, 22, 0.98));
            }
            .boxed-list {
                padding: 8px;
                border-radius: 16px;
            }
            button.flat {
                padding: 10px 12px;
                border-radius: 12px;
            }
        "#;

        provider.load_from_data(css);
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
    }
}

fn apply_item_to_clipboard(item: &ClipboardItem) {
    if let Ok(mut clipboard) = Clipboard::new() {
        match &item.content {
            ClipboardContent::Text(text) => {
                let _ = clipboard.set_text(text.clone());
            }
            ClipboardContent::Image(image) => {
                let _ = clipboard.set_image(arboard::ImageData {
                    width: image.width,
                    height: image.height,
                    bytes: std::borrow::Cow::Borrowed(&image.bytes),
                });
            }
        }
    }
}