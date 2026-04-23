use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::thread;

const SOCKET_PATH: &str = "/tmp/clipboard_manager.sock";

pub enum InstanceType {
    Primary(UnixListener),
    Secondary,
}

pub enum IpcCommand {
    Show,
    Stop,
}

pub fn send_command(cmd: IpcCommand) -> bool {
    let socket = Path::new(SOCKET_PATH);
    if let Ok(mut stream) = UnixStream::connect(socket) {
        let msg = match cmd {
            IpcCommand::Show => b"SHOW" as &[u8],
            IpcCommand::Stop => b"STOP" as &[u8],
        };
        let _ = stream.write_all(msg);
        return true;
    }
    false
}

pub fn check_instance() -> InstanceType {
    let socket = Path::new(SOCKET_PATH);

    if socket.exists() {
        if let Ok(mut stream) = UnixStream::connect(socket) {
            let _ = stream.write_all(b"SHOW");
            return InstanceType::Secondary;
        }
        let _ = std::fs::remove_file(socket);
    }

    match UnixListener::bind(socket) {
        Ok(listener) => InstanceType::Primary(listener),
        Err(e) => {
            eprintln!("Failed to bind socket: {}", e);
            InstanceType::Secondary
        }
    }
}

pub fn start_listener(listener: UnixListener, show_tx: Sender<()>, stop_tx: Sender<()>) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(mut s) => {
                    let mut buf = [0u8; 4];
                    let n = s.read(&mut buf).unwrap_or(0);
                    match &buf[..n] {
                        b"STOP" => {
                            let _ = stop_tx.send(());
                            break;
                        }
                        _ => {
                            let _ = show_tx.send(());
                        }
                    }
                }
                Err(e) => eprintln!("Error accepting connection: {}", e),
            }
        }
    });
}
