mod clipboard_monitor;
mod gtk_ui;
mod ipc;
mod types;

use crate::clipboard_monitor::ClipboardMonitor;
use crate::ipc::{check_instance, send_command, start_listener, start_stdin_listener, InstanceType, IpcCommand};
use crate::types::ClipboardItem;
use std::env;
use std::sync::{mpsc, Arc, Mutex};

fn main() {
    let args: Vec<String> = env::args().collect();
    let start_hidden = args.contains(&"--start".to_string());
    let show = args.iter().any(|arg| arg == "show");
    let stop = args.iter().any(|arg| arg == "stop");

    if show {
        if send_command(IpcCommand::Show) {
            println!("Clipboard window shown.");
            return;
        }
        println!("No running instance found. Starting clipboard manager.");
    }

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
            println!("Type 'show' to open the window or 'stop' to exit.");

            let (show_tx, show_rx) = mpsc::channel();
            let (stop_tx, stop_rx) = mpsc::channel();
            start_listener(listener, show_tx, stop_tx);

            let history = Arc::new(Mutex::new(Vec::<ClipboardItem>::new()));

            let monitor = ClipboardMonitor::new(history.clone());
            monitor.start();

            let (stdin_show_tx, stdin_show_rx) = mpsc::channel();
            let (stdin_stop_tx, stdin_stop_rx) = mpsc::channel();
            start_stdin_listener(stdin_show_tx, stdin_stop_tx);

            let merged_show_rx = merge_receivers(show_rx, stdin_show_rx);
            let merged_stop_rx = merge_receivers(stop_rx, stdin_stop_rx);

            gtk_ui::run(history, merged_show_rx, merged_stop_rx, start_hidden);
        }
    }
}

fn merge_receivers(
    primary_rx: mpsc::Receiver<()>,
    secondary_rx: mpsc::Receiver<()>,
) -> mpsc::Receiver<()> {
    let (merged_tx, merged_rx) = mpsc::channel();
    let primary_tx = merged_tx.clone();
    let secondary_tx = merged_tx;

    std::thread::spawn(move || {
        loop {
            match primary_rx.recv() {
                Ok(()) => {
                    let _ = primary_tx.send(());
                }
                Err(_) => break,
            }
        }
    });

    std::thread::spawn(move || {
        loop {
            match secondary_rx.recv() {
                Ok(()) => {
                    let _ = secondary_tx.send(());
                }
                Err(_) => break,
            }
        }
    });

    merged_rx
}
