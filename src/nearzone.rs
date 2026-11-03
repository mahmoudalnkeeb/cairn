//! Shake to show (opt-in): while the left button is held, shaking the mouse
//! left and right shows a drop zone next to the cursor, so you can drop
//! without travelling to the screen edge.
//!
//! Seeing a shake needs a low-level mouse hook, which only runs while the
//! setting is on. The hook only watches the button and horizontal movement;
//! it never blocks or changes input.

use std::cell::Cell;
use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::gfx::Surface;
use crate::{App, dnd, dpi_at};

const WIN_W: f32 = 120.0;
const WIN_H: f32 = 180.0;
const CARD_W: f32 = 104.0;
const CARD_H: f32 = 164.0;
/// How far right of the cursor the zone opens, in DIPs.
const OFFSET: f32 = 56.0;
const HIDE_TIMER: usize = 1;

/// Posted to the host by the hook: a shake was seen (lParam holds the
/// cursor), or the button was released.
pub const WM_SHAKE: u32 = WM_APP + 3;
pub const WM_SHAKE_END: u32 = WM_APP + 4;

/// Direction changes, each after at least this much travel...
const MIN_SWING: i32 = 24;
/// ...needed within this window to count as a shake.
const REVERSALS: usize = 4;
const WINDOW_MS: u128 = 700;

/// Recognises a shake from a stream of x positions while the button is held.
#[derive(Default)]
pub struct Shake {
    /// Furthest x in the current direction, and the direction (+1 or -1).
    edge: Option<(i32, i32)>,
    turns: Vec<Instant>,
}

impl Shake {
    pub fn reset(&mut self) {
        self.edge = None;
        self.turns.clear();
    }

    /// Feeds one x position; true when this move completes a shake.
    pub fn feed(&mut self, x: i32, now: Instant) -> bool {
        let Some((far, dir)) = self.edge else {
            self.edge = Some((x, 0));
            return false;
        };
        let d = x - far;
        if dir == 0 {
            if d.abs() >= MIN_SWING {
                self.edge = Some((x, d.signum()));
            }
            return false;
        }
        if d.signum() == dir {
            self.edge = Some((x, dir));
        } else if d.abs() >= MIN_SWING {
            // Swung back far enough: a reversal.
            self.edge = Some((x, -dir));
            self.turns.retain(|t| now.duration_since(*t).as_millis() <= WINDOW_MS);
            self.turns.push(now);
            if self.turns.len() >= REVERSALS {
                self.turns.clear();
                return true;
            }
        }
        false
    }
}

thread_local! {
    static HOOK: Cell<HHOOK> = const { Cell::new(HHOOK(std::ptr::null_mut())) };
    static HOST: Cell<isize> = const { Cell::new(0) };
    static DOWN: Cell<bool> = const { Cell::new(false) };
    static SHAKE: std::cell::RefCell<Shake> = std::cell::RefCell::new(Shake::default());
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let host = HWND(HOST.get() as *mut _);
        match wparam.0 as u32 {
            WM_LBUTTONDOWN => {
                DOWN.set(true);
                SHAKE.with_borrow_mut(Shake::reset);
            }
            WM_LBUTTONUP => {
                DOWN.set(false);
                unsafe {
                    let _ = PostMessageW(host, WM_SHAKE_END, WPARAM(0), LPARAM(0));
                }
            }
            WM_MOUSEMOVE if DOWN.get() => {
                if SHAKE.with_borrow_mut(|s| s.feed(info.pt.x, Instant::now())) {
                    let packed = ((info.pt.y as u32 as isize) << 32) | info.pt.x as u32 as isize;
                    unsafe {
                        let _ = PostMessageW(host, WM_SHAKE, WPARAM(0), LPARAM(packed));
                    }
                }
            }
            _ => {}
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

pub struct NearZone {
    pub hwnd: HWND,
    surface: Option<Surface>,
    dpi: u32,
    origin: (i32, i32),
    pub shown: bool,
    hover: bool,
    flash: Option<String>,
}

impl NearZone {
    pub fn new(hwnd: HWND) -> NearZone {
        NearZone { hwnd, surface: None, dpi: 96, origin: (0, 0), shown: false, hover: false, flash: None }
    }
}

/// Turns the hook on or off to match the setting.
pub fn set_enabled(app: &App, on: bool) {
    HOST.set(app.host.0 as isize);
    let current = HOOK.get();
    unsafe {
        if on && current.is_invalid() {
            let module = GetModuleHandleW(None).unwrap_or_default();
            if let Ok(h) = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0) {
                HOOK.set(h);
            }
        } else if !on && !current.is_invalid() {
            let _ = UnhookWindowsHookEx(current);
            HOOK.set(HHOOK(std::ptr::null_mut()));
        }
    }
}

/// Opens the zone beside the cursor, inside the work area.
pub fn show_at(app: &mut App, pt: POINT) {
    let dpi = dpi_at(pt);
    let s = dpi as f32 / 96.0;
    let (w, h) = ((WIN_W * s).ceil() as i32, (WIN_H * s).ceil() as i32);
    let wa = unsafe {
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST), &mut info);
        info.rcWork
    };
    let mut x = pt.x + (OFFSET * s) as i32;
    if x + w > wa.right {
        // No room on the right: open on the left of the cursor.
        x = pt.x - (OFFSET * s) as i32 - w;
    }
    let y = (pt.y - h / 2).clamp(wa.top, (wa.bottom - h).max(wa.top));
    let z = &mut app.near;
    z.dpi = dpi;
    z.origin = (x, y);
    z.shown = true;
    z.flash = None;
    z.hover = false;
    if z.surface.as_ref().is_none_or(|surf| surf.width != w || surf.height != h) {
        z.surface = Surface::new(&app.gfx, w, h).ok();
    }
    unsafe {
        let _ = KillTimer(app.near.hwnd, HIDE_TIMER);
    }
    render(app);
    unsafe {
        let _ = ShowWindow(app.near.hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(app.near.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Hides the zone after `ms`, leaving time for a drop that is under way.
pub fn hide_soon(app: &App, ms: u32) {
    if app.near.shown {
        unsafe {
            SetTimer(app.near.hwnd, HIDE_TIMER, ms, None);
        }
    }
}

fn hide(app: &mut App) {
    app.near.shown = false;
    unsafe {
        let _ = KillTimer(app.near.hwnd, HIDE_TIMER);
        let _ = ShowWindow(app.near.hwnd, SW_HIDE);
    }
}

pub fn set_hover(app: &mut App, over: bool) {
    app.near.hover = over;
    render(app);
}

/// Shows what a drop did, then hides.
pub fn dropped(app: &mut App, added: usize) {
    app.near.flash = Some(match added {
        0 => "Already here".to_string(),
        n => format!("Added {n}"),
    });
    app.near.hover = false;
    render(app);
    hide_soon(app, 900);
}

fn render(app: &mut App) {
    let z = &app.near;
    let Some(surf) = &z.surface else { return };
    let (g, t) = (&app.gfx, &app.theme);
    let caption = g.text_format(&g.ui_font, 12.0, DWRITE_FONT_WEIGHT_NORMAL).ok();
    let (x, y) = ((WIN_W - CARD_W) / 2.0, (WIN_H - CARD_H) / 2.0);
    let _ = surf.draw(z.dpi, |d| {
        d.rect_shadow(x, y + 6.0, CARD_W, CARD_H, 8.0, 12.0, 0.45 * t.shadow);
        d.fill_rounded(x, y, CARD_W, CARD_H, 8.0, t.surface_flyout);
        let fill = if z.hover { t.primary.alpha(0.28) } else { t.primary_subtle };
        d.fill_rounded(x, y, CARD_W, CARD_H, 8.0, fill);
        d.stroke_rounded_dashed(x + 0.75, y + 0.75, CARD_W - 1.5, CARD_H - 1.5, 8.0, 1.5, t.primary);
        d.cairn_mark(x + (CARD_W - 32.0) / 2.0, y + 46.0, 32.0, t.stone, t.primary);
        if let Some(f) = &caption {
            let (text, color) = match &z.flash {
                Some(s) => (s.as_str(), t.text),
                None => ("Drop here", t.text_secondary),
            };
            d.text(text, f, x, y + 96.0, CARD_W, 20.0, color);
        }
    });
    let _ = surf.present(z.hwnd, z.origin.0, z.origin.1, 1.0);
}

pub fn handle(app: &mut App, msg: u32, wparam: WPARAM) -> Option<LRESULT> {
    match msg {
        WM_TIMER if wparam.0 == HIDE_TIMER => hide(app),
        WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
        _ => return None,
    }
    Some(LRESULT(0))
}

/// Registers the zone window as a drop target.
pub fn register(hwnd: HWND) {
    let _ = dnd::register(hwnd, dnd::Zone::Near);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn feed_all(s: &mut Shake, xs: &[i32], step_ms: u64) -> bool {
        let start = Instant::now();
        xs.iter().enumerate().any(|(i, &x)| s.feed(x, start + Duration::from_millis(i as u64 * step_ms)))
    }

    #[test]
    fn quick_back_and_forth_is_a_shake() {
        let mut s = Shake::default();
        assert!(feed_all(&mut s, &[0, 40, 80, 40, 0, 40, 80, 40, 0, 40], 30));
    }

    #[test]
    fn a_straight_drag_or_jitter_is_not() {
        let mut s = Shake::default();
        assert!(!feed_all(&mut s, &(0..40).map(|i| i * 20).collect::<Vec<_>>(), 10));
        s.reset();
        // Small wobbles under the swing size never count.
        assert!(!feed_all(&mut s, &[0, 10, 0, 10, 0, 10, 0, 10, 0, 10, 0], 20));
    }

    #[test]
    fn slow_swings_are_not_a_shake() {
        let mut s = Shake::default();
        assert!(!feed_all(&mut s, &[0, 80, 0, 80, 0, 80, 0], 400));
    }
}
