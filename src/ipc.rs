use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::thread;

const SOCKET_PATH: &str = "/tmp/clipboard_manager.sock";

pub enum InstanceType {
    Primary(UnixListener),
    Secondary,
}

pub fn check_instance() -> InstanceType {
    let socket = Path::new(SOCKET_PATH);

    // Try to connect to existing socket
    if let Ok(mut stream) = UnixStream::connect(socket) {
        // If successful, we are a secondary instance
        let _ = stream.write_all(b"SHOW");
        return InstanceType::Secondary;
    }

    // If connect failed, maybe the socket file exists but is dead. Remove it.
    if socket.exists() {
        let _ = std::fs::remove_file(socket);
    }

    // Bind new socket
    match UnixListener::bind(socket) {
        Ok(listener) => InstanceType::Primary(listener),
        Err(e) => {
            eprintln!("Failed to bind socket: {}", e);
            // Fallback: treat as secondary to avoid crashing, or panic
            InstanceType::Secondary
        }
    }
}

pub fn start_listener(listener: UnixListener, tx: Sender<()>) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(_) => {
                    // Received a connection (signal to show window)
                    let _ = tx.send(());
                }
                Err(e) => eprintln!("Error accepting connection: {}", e),
            }
        }
    });
}
