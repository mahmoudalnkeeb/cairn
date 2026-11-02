//! Pasting a text item: put it on the clipboard, then send Ctrl+V to the
//! app the stack was opened from.

use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

/// Replaces the clipboard with plain text.
pub fn write_text(owner: HWND, text: &str) -> bool {
    let units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    unsafe {
        if OpenClipboard(owner).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let mut ok = false;
        if let Ok(global) = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2) {
            let ptr = GlobalLock(global) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(units.as_ptr(), ptr, units.len());
                let _ = GlobalUnlock(global);
                ok = SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(global.0)).is_ok();
            }
            if !ok {
                let _ = GlobalFree(global);
            }
        }
        let _ = CloseClipboard();
        ok
    }
}

/// Sends Ctrl+V to whatever window has focus.
pub fn send_paste() {
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() },
        },
    };
    let inputs = [key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)];
    unsafe {
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }
}
