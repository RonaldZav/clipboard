mod clipboard_monitor;
mod input_utils;
mod ipc;
mod types;
mod ui;

use crate::clipboard_monitor::ClipboardMonitor;
use crate::input_utils::InputUtils;
use crate::ipc::{check_instance, start_listener, InstanceType};
use crate::types::ClipboardItem;
use crate::ui::ClipboardApp;
use eframe::egui;
use std::env;
use std::sync::{mpsc, Arc, Mutex};

fn main() -> eframe::Result<()> {
    // Check arguments
    let args: Vec<String> = env::args().collect();
    let start_hidden = args.contains(&"--start".to_string());

    // Check for existing instance
    match check_instance() {
        InstanceType::Secondary => {
            if !start_hidden {
                println!("Instance already running. Sending signal to show window.");
            } else {
                println!("Instance already running. '--start' ignored.");
            }
            return Ok(());
        }
        InstanceType::Primary(listener) => {
            println!("Starting Primary Instance...");

            // Channel to receive "SHOW" commands from secondary instances
            let (tx, rx) = mpsc::channel();
            start_listener(listener, tx);

            // Shared state for clipboard history
            let history = Arc::new(Mutex::new(Vec::<ClipboardItem>::new()));

            // Start background monitor
            let monitor = ClipboardMonitor::new(history.clone());
            monitor.start();

            // Get initial position
            let (x, y) = InputUtils::get_mouse_position();
            let mouse_pos = egui::pos2(x, y);

            let options = eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_always_on_top()
                    .with_decorations(false) // Frameless
                    .with_inner_size([300.0, 400.0])
                    .with_position(mouse_pos)
                    .with_visible(!start_hidden), // Start visible unless --start is passed
                ..Default::default()
            };

            eframe::run_native(
                "Clipboard Manager",
                options,
                Box::new(move |cc| {
                    // If starting hidden, ensure we send a hide command immediately to be safe
                    if start_hidden {
                        cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    }
                    Box::new(ClipboardApp::new(cc, history, rx, start_hidden))
                }),
            )
        }
    }
}
