//! User choices: the global hotkey, which screen edge holds the tab, and
//! whether Cairn starts at login. Stored in %APPDATA%\Cairn\settings.txt,
//! except run-at-login, which lives in the registry Run key.

use std::fs;

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::core::*;

use crate::store;

pub struct Hotkey {
    pub mods: HOT_KEY_MODIFIERS,
    pub vk: VIRTUAL_KEY,
    pub label: &'static str,
}

const CTRL_ALT: HOT_KEY_MODIFIERS = HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0);

/// Offered in the tray menu. The first is the default.
pub const HOTKEYS: [Hotkey; 3] = [
    Hotkey { mods: CTRL_ALT, vk: VK_C, label: "Ctrl+Alt+C" },
    Hotkey { mods: CTRL_ALT, vk: VK_K, label: "Ctrl+Alt+K" },
    Hotkey { mods: HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0 | MOD_SHIFT.0), vk: VK_C, label: "Ctrl+Alt+Shift+C" },
];

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Cairn");

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Edge {
    Left,
    Right,
}

pub struct Settings {
    /// Index into HOTKEYS the user picked.
    pub hotkey: usize,
    pub edge: Edge,
}

impl Settings {
    pub fn load() -> Settings {
        let text = store::dir().and_then(|d| fs::read_to_string(d.join("settings.txt")).ok()).unwrap_or_default();
        Settings::parse(&text)
    }

    fn parse(text: &str) -> Settings {
        let mut s = Settings { hotkey: 0, edge: Edge::Right };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            match (key.trim(), value.trim()) {
                ("hotkey", v) => {
                    if let Some(i) = HOTKEYS.iter().position(|h| h.label == v) {
                        s.hotkey = i;
                    }
                }
                ("edge", "left") => s.edge = Edge::Left,
                ("edge", "right") => s.edge = Edge::Right,
                _ => {}
            }
        }
        s
    }

    fn serialize(&self) -> String {
        let edge = match self.edge {
            Edge::Left => "left",
            Edge::Right => "right",
        };
        format!("hotkey={}\nedge={edge}\n", HOTKEYS[self.hotkey].label)
    }

    pub fn save(&self) {
        if let Some(d) = store::dir() {
            let _ = store::write_atomic(&d.join("settings.txt"), self.serialize().as_bytes());
        }
    }
}

pub fn run_at_login() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE, RRF_RT_REG_SZ, None, None, None) == ERROR_SUCCESS }
}

pub fn set_run_at_login(on: bool) -> bool {
    unsafe {
        if !on {
            let r = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE);
            return r == ERROR_SUCCESS || !run_at_login();
        }
        let Ok(exe) = std::env::current_exe() else { return false };
        let value: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain(Some(0)).collect();
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
            REG_SZ.0,
            Some(value.as_ptr() as *const _),
            (value.len() * 2) as u32,
        ) == ERROR_SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_ignores_junk() {
        let s = Settings::parse("hotkey=Ctrl+Alt+K\nedge=left\nnonsense\n");
        assert_eq!(HOTKEYS[s.hotkey].label, "Ctrl+Alt+K");
        assert_eq!(s.edge, Edge::Left);
        let back = Settings::parse(&s.serialize());
        assert_eq!((back.hotkey, back.edge), (s.hotkey, s.edge));
    }

    #[test]
    fn defaults_to_right_edge_and_first_hotkey() {
        let s = Settings::parse("hotkey=F13\nedge=top");
        assert_eq!((s.hotkey, s.edge), (0, Edge::Right));
    }
}
