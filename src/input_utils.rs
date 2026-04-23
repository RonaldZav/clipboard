use enigo::{Enigo, Key, Keyboard, Mouse, Settings};
use std::thread;
use std::time::Duration;

fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
}

pub struct InputUtils;

impl InputUtils {
    pub fn get_mouse_position() -> (f32, f32) {
        if is_wayland() {
            // Wayland doesn't allow global cursor position queries
            return (100.0, 100.0);
        }
        match Enigo::new(&Settings::default()) {
            Ok(enigo) => match enigo.location() {
                Ok((x, y)) => (x as f32, y as f32),
                Err(_) => (100.0, 100.0),
            },
            Err(_) => (100.0, 100.0),
        }
    }

    pub fn paste_content() {
        thread::spawn(|| {
            thread::sleep(Duration::from_millis(200));

            if is_wayland() {
                // Try wtype (wlroots-based compositors: sway, hyprland, etc.)
                if std::process::Command::new("wtype")
                    .args(["-M", "ctrl", "-k", "v", "-m", "ctrl"])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
                {
                    return;
                }
                // Fallback: ydotool (requires ydotoold daemon running)
                let _ = std::process::Command::new("ydotool")
                    .args(["key", "29:1", "47:1", "47:0", "29:0"])
                    .status();
            } else {
                if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
                    let _ = enigo.key(Key::Control, enigo::Direction::Press);
                    let _ = enigo.key(Key::Unicode('v'), enigo::Direction::Click);
                    let _ = enigo.key(Key::Control, enigo::Direction::Release);
                }
            }
        });
    }
}
