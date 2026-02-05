use enigo::{Enigo, Key, Keyboard, Mouse, Settings};
use std::thread;
use std::time::Duration;

pub struct InputUtils;

impl InputUtils {
    pub fn get_mouse_position() -> (f32, f32) {
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
            // Small delay to ensure window focus has switched back
            thread::sleep(Duration::from_millis(200));

            if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
                let _ = enigo.key(Key::Control, enigo::Direction::Press);
                let _ = enigo.key(Key::Unicode('v'), enigo::Direction::Click);
                let _ = enigo.key(Key::Control, enigo::Direction::Release);
            }
        });
    }
}
