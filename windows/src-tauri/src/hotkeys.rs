// Global shortcuts from Coucou 0.1.7, adapted for a Polish keyboard.
// Ctrl+Alt is AltGr, so these add Shift and avoid the letters that type Ą Ć Ę Ł.

use std::thread;

use tauri::{AppHandle, Emitter};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

fn mods() -> windows::Win32::UI::Input::KeyboardAndMouse::HOT_KEY_MODIFIERS {
    // Ctrl+Alt is AltGr (ą, ł, …). Ctrl+Alt+Shift is not, and these letters
    // are not the capital Polish ones either (Ą is Shift+AltGr+A).
    MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT
}

/// (id, virtual key, event name sent to the island)
const HOTKEYS: &[(i32, u32, &str)] = &[
    (1, 0x20, "chat"),      // Space
    (2, 0x51, "alert"),     // Q — not a Polish AltGr letter
    (3, 0x54, "terminal"),  // T
    (4, 0xDD, "next"),      // ]
    (5, 0xDB, "prev"),      // [
    (6, 0x4D, "mute"),      // M
    (7, 0x47, "wardrobe"),  // G
    (8, 0x42, "toggle"),    // B
];

pub fn start(app: AppHandle) {
    thread::spawn(move || {
        for (id, vk, name) in HOTKEYS {
            let ok = unsafe { RegisterHotKey(None, *id, mods(), *vk) };
            if ok.is_err() {
                crate::log::line(format!("shortcut {name} is already taken"));
            }
        }
        let mut msg = MSG::default();
        loop {
            let got = unsafe { GetMessageW(std::ptr::addr_of_mut!(msg), None, 0, 0) };
            if !got.as_bool() {
                break;
            }
            if msg.message == WM_HOTKEY {
                let id = msg.wParam.0 as i32;
                if let Some((_, _, name)) = HOTKEYS.iter().find(|(i, _, _)| *i == id) {
                    let _ = app.emit("shortcut", name);
                }
            }
        }
    });
}
