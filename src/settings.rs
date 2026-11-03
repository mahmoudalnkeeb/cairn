//! User choices: the global hotkey, which screen edge holds the tab, which
//! monitors show it and how high, and whether Cairn starts at login. Stored
//! in %APPDATA%\Cairn\settings.txt, except run-at-login, which lives in the
//! registry Run key.

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
    /// Device names (`\\.\DISPLAY2`) of monitors with the tab turned off.
    pub monitors_off: Vec<String>,
    /// Where the tab's center sits along the edge: 0 top, 1 bottom.
    pub tab_pos: f32,
    /// Keep Cairn's own copy of files dropped from USB and network drives.
    pub drive_copies: bool,
    /// Shaking the mouse during a drag shows a drop zone near the cursor.
    pub shake: bool,
}

impl Settings {
    pub fn load() -> Settings {
        let text = store::dir().and_then(|d| fs::read_to_string(d.join("settings.txt")).ok()).unwrap_or_default();
        Settings::parse(&text)
    }

    fn parse(text: &str) -> Settings {
        let mut s = Settings { hotkey: 0, edge: Edge::Right, monitors_off: Vec::new(), tab_pos: 0.5, drive_copies: false, shake: false };
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
                ("monitors_off", v) => s.monitors_off = v.split(';').filter(|d| !d.is_empty()).map(String::from).collect(),
                ("tab_pos", v) => {
                    if let Ok(p) = v.parse::<f32>() {
                        s.tab_pos = p.clamp(0.0, 1.0);
                    }
                }
                ("drive_copies", v) => s.drive_copies = v == "on",
                ("shake", v) => s.shake = v == "on",
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
        let on = |b: bool| if b { "on" } else { "off" };
        format!(
            "hotkey={}\nedge={edge}\nmonitors_off={}\ntab_pos={:.3}\ndrive_copies={}\nshake={}\n",
            HOTKEYS[self.hotkey].label,
            self.monitors_off.join(";"),
            self.tab_pos,
            on(self.drive_copies),
            on(self.shake)
        )
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
        let s = Settings::parse("hotkey=Ctrl+Alt+K\nedge=left\nmonitors_off=\\\\.\\DISPLAY2;\\\\.\\DISPLAY3\ntab_pos=0.25\ndrive_copies=on\nnonsense\n");
        assert_eq!(HOTKEYS[s.hotkey].label, "Ctrl+Alt+K");
        assert_eq!(s.edge, Edge::Left);
        assert_eq!(s.monitors_off, ["\\\\.\\DISPLAY2", "\\\\.\\DISPLAY3"]);
        let back = Settings::parse(&s.serialize());
        assert_eq!((back.hotkey, back.edge, &back.monitors_off), (s.hotkey, s.edge, &s.monitors_off));
        assert!((back.tab_pos - 0.25).abs() < 0.001);
        assert!(back.drive_copies && !back.shake);
    }

    #[test]
    fn defaults_to_right_edge_and_first_hotkey() {
        let s = Settings::parse("hotkey=F13\nedge=top\ntab_pos=7");
        assert_eq!((s.hotkey, s.edge), (0, Edge::Right));
        assert!(s.monitors_off.is_empty());
        assert_eq!(s.tab_pos, 1.0);
        assert!(!s.drive_copies && !s.shake);
    }
}
