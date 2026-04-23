mod clipboard_monitor;
mod input_utils;
mod ipc;
mod types;
mod ui;
mod window;

use crate::clipboard_monitor::ClipboardMonitor;
use crate::ipc::{check_instance, send_command, start_listener, InstanceType, IpcCommand};
use crate::types::ClipboardItem;
use std::env;
use std::sync::{mpsc, Arc, Mutex};

fn main() {
    let args: Vec<String> = env::args().collect();
    let start_hidden = args.contains(&"--start".to_string());
    let stop = args.contains(&"stop".to_string());

    if stop {
        if send_command(IpcCommand::Stop) {
            println!("Clipboard manager stopped.");
        } else {
            eprintln!("No running instance found.");
        }
        return;
    }

    match check_instance() {
        InstanceType::Secondary => {
            if !start_hidden {
                println!("Instance already running. Sending signal to show window.");
            } else {
                println!("Instance already running. '--start' ignored.");
            }
        }
        InstanceType::Primary(listener) => {
            println!("Starting Primary Instance...");

            let (show_tx, show_rx) = mpsc::channel();
            let (stop_tx, stop_rx) = mpsc::channel();
            start_listener(listener, show_tx, stop_tx);

            let history = Arc::new(Mutex::new(Vec::<ClipboardItem>::new()));

            let monitor = ClipboardMonitor::new(history.clone());
            monitor.start();

            window::run(history, show_rx, stop_rx, start_hidden);
        }
    }
}
