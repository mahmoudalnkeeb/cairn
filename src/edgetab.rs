//! C1: the edge tab. A slim pill on a screen edge that is a real OLE drop
//! target. When a supported drag reaches it, Windows calls DragEnter and the
//! pill grows into the drop zone, so Cairn never watches the mouse globally.
//!
//! The window keeps one size. While idle, only a thin strip along the edge
//! has non-zero alpha; everywhere else clicks and drops fall through to the
//! windows underneath.

use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::gfx::{Color, Surface, bezier};
use crate::settings::Edge;
use crate::{App, dpi_at};

const WIN_W: f32 = 120.0;
const WIN_H: f32 = 200.0;
const CARD_W: f32 = 88.0;
const CARD_H: f32 = 160.0;
const CARD_GAP: f32 = 6.0;
const HIT_W: f32 = 10.0;
const HIT_H: f32 = 120.0;
const PILL_W: f32 = 4.0;
const PILL_H: f32 = 96.0;
const PILL_GAP: f32 = 3.0;
const OPEN_MS: f32 = 140.0;
const CLOSE_MS: f32 = 120.0;
const FLASH_MS: u32 = 700;
const ANIM_TIMER: usize = 1;
const FLASH_TIMER: usize = 2;

pub struct EdgeTab {
    pub hwnd: HWND,
    pub fullscreen_hidden: bool,
    surface: Option<Surface>,
    dpi: u32,
    origin: (i32, i32),
    drag_over: bool,
    /// Caption shown briefly after a drop ("Added 2").
    flash: Option<String>,
    hover: bool,
    tracked: bool,
    /// Expansion: 0 is the idle pill, 1 the full drop zone.
    progress: f32,
    from: f32,
    start: Instant,
}

impl EdgeTab {
    pub fn new(hwnd: HWND) -> EdgeTab {
        EdgeTab {
            hwnd,
            fullscreen_hidden: false,
            surface: None,
            dpi: 96,
            origin: (0, 0),
            drag_over: false,
            flash: None,
            hover: false,
            tracked: false,
            progress: 0.0,
            from: 0.0,
            start: Instant::now(),
        }
    }

    fn expanded(&self) -> bool {
        self.drag_over || self.flash.is_some()
    }
}

pub fn work_area(pt: POINT) -> RECT {
    unsafe {
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut info);
        info.rcWork
    }
}

/// The primary monitor's work area. Other monitors arrive in M3.
fn home() -> RECT {
    work_area(POINT { x: 0, y: 0 })
}

/// The tab's inner edge at its vertical center, plus the work area: where
/// the stack flyout opens from.
pub fn anchor(app: &App) -> (POINT, RECT) {
    let wa = home();
    let s = dpi_at(POINT { x: wa.left, y: wa.top }) as f32 / 96.0;
    let hit = (HIT_W * s) as i32;
    let x = match app.settings.edge {
        Edge::Left => wa.left + hit,
        Edge::Right => wa.right - hit,
    };
    (POINT { x, y: (wa.top + wa.bottom) / 2 }, wa)
}

/// Puts the window against the chosen edge, vertically centered.
pub fn position(app: &mut App) {
    let wa = home();
    let dpi = dpi_at(POINT { x: wa.left, y: wa.top });
    let s = dpi as f32 / 96.0;
    let (w, h) = ((WIN_W * s).ceil() as i32, (WIN_H * s).ceil() as i32);
    let x = match app.settings.edge {
        Edge::Left => wa.left,
        Edge::Right => wa.right - w,
    };
    let t = &mut app.tab;
    t.dpi = dpi;
    t.origin = (x, (wa.top + wa.bottom - h) / 2);
    if t.surface.as_ref().is_none_or(|surf| surf.width != w || surf.height != h) {
        t.surface = Surface::new(&app.gfx, w, h).ok();
    }
    render(app);
}

/// Left x of a box `w` wide that sits `gap` DIPs from the screen edge.
fn from_edge(edge: Edge, gap: f32, w: f32) -> f32 {
    match edge {
        Edge::Left => gap,
        Edge::Right => WIN_W - gap - w,
    }
}

pub fn render(app: &mut App) {
    let t = &app.tab;
    let Some(surf) = &t.surface else { return };
    let (g, th) = (&app.gfx, &app.theme);
    let edge = app.settings.edge;
    let p = t.progress;
    let lerp = |a: f32, b: f32| a + (b - a) * p;
    let caption = g.text_format(&g.ui_font, 12.0, DWRITE_FONT_WEIGHT_NORMAL).ok();
    let flash = t.flash.clone();
    let hover = t.hover;
    let _ = surf.draw(t.dpi, |d| {
        // The hit strip: alpha 1/255, invisible but it catches drags.
        let cy = WIN_H / 2.0;
        d.fill_rect(from_edge(edge, 0.0, HIT_W), cy - HIT_H / 2.0, HIT_W, HIT_H, Color(0.0, 0.0, 0.0, 1.0 / 255.0));
        if p < 0.02 {
            let alpha = if hover { 0.9 } else { 0.4 };
            let x = from_edge(edge, PILL_GAP, PILL_W);
            d.fill_rounded(x, cy - PILL_H / 2.0, PILL_W, PILL_H, PILL_W / 2.0, th.outline_control.alpha(alpha));
            return;
        }
        let (w, h) = (lerp(PILL_W, CARD_W), lerp(PILL_H, CARD_H));
        let x = from_edge(edge, lerp(PILL_GAP, CARD_GAP), w);
        let y = cy - h / 2.0;
        let r = lerp(PILL_W / 2.0, 8.0);
        d.rect_shadow(x, y + 6.0, w, h, r, 12.0 * p, 0.45 * th.shadow * p);
        d.fill_rounded(x, y, w, h, r, th.surface_flyout.alpha(p));
        d.fill_rounded(x, y, w, h, r, th.primary_subtle.alpha(p));
        d.stroke_rounded_dashed(x + 0.75, y + 0.75, w - 1.5, h - 1.5, r, 1.5, th.primary.alpha(p));
        if p > 0.6 {
            let a = (p - 0.6) / 0.4;
            d.cairn_mark(x + (w - 32.0) / 2.0, y + 44.0, 32.0, th.stone.alpha(a), th.primary.alpha(a));
            if let Some(f) = &caption {
                let (text, color) = match &flash {
                    Some(s) => (s.as_str(), th.text),
                    None => ("Drop here", th.text_secondary),
                };
                d.text(text, f, x, y + 92.0, w, 20.0, color.alpha(a));
            }
        }
    });
    let _ = surf.present(t.hwnd, t.origin.0, t.origin.1, 1.0);
}

pub fn show(app: &mut App) {
    unsafe {
        let _ = ShowWindow(app.tab.hwnd, if app.tab.fullscreen_hidden { SW_HIDE } else { SW_SHOWNOACTIVATE });
    }
}

/// Re-asserts topmost, since other topmost windows can cover the tab.
pub fn raise(app: &App) {
    if !app.tab.fullscreen_hidden {
        unsafe {
            let _ = SetWindowPos(app.tab.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

fn animate(app: &mut App) {
    let t = &mut app.tab;
    t.from = t.progress;
    t.start = Instant::now();
    unsafe {
        SetTimer(t.hwnd, ANIM_TIMER, 10, None);
    }
    tick(app);
}

fn tick(app: &mut App) {
    let t = &mut app.tab;
    let target = if t.expanded() { 1.0 } else { 0.0 };
    let dur = if target > t.from { OPEN_MS } else { CLOSE_MS };
    let k = if app.animations { (t.start.elapsed().as_secs_f32() * 1000.0 / dur).min(1.0) } else { 1.0 };
    t.progress = t.from + (target - t.from) * bezier(0.0, 0.0, 0.0, 1.0, k);
    if k >= 1.0 {
        unsafe {
            let _ = KillTimer(t.hwnd, ANIM_TIMER);
        }
    }
    render(app);
}

pub fn set_drag_over(app: &mut App, over: bool) {
    if app.tab.drag_over != over {
        app.tab.drag_over = over;
        animate(app);
    }
}

/// Shows what the drop did, then folds back to the pill.
pub fn dropped(app: &mut App, added: usize) {
    app.tab.flash = Some(match added {
        0 => "Already here".to_string(),
        n => format!("Added {n}"),
    });
    animate(app);
    unsafe {
        SetTimer(app.tab.hwnd, FLASH_TIMER, FLASH_MS, None);
    }
}

pub fn handle(app: &mut App, hwnd: HWND, msg: u32, wparam: WPARAM, _lparam: LPARAM) -> Option<LRESULT> {
    match msg {
        WM_TIMER if wparam.0 == ANIM_TIMER => tick(app),
        WM_TIMER if wparam.0 == FLASH_TIMER => {
            unsafe {
                let _ = KillTimer(hwnd, FLASH_TIMER);
            }
            app.tab.flash = None;
            animate(app);
        }
        WM_MOUSEMOVE => {
            if !app.tab.tracked {
                app.tab.tracked = true;
                app.tab.hover = true;
                let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                unsafe {
                    let _ = TrackMouseEvent(&mut tme);
                }
                render(app);
            }
        }
        WM_MOUSELEAVE => {
            app.tab.tracked = false;
            app.tab.hover = false;
            render(app);
        }
        WM_LBUTTONUP => crate::toggle_stack(app),
        WM_RBUTTONUP => crate::show_context_menu(app),
        WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
        WM_DISPLAYCHANGE | WM_DPICHANGED => position(app),
        _ => return None,
    }
    Some(LRESULT(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxes_hug_the_chosen_edge() {
        assert_eq!(from_edge(Edge::Left, 3.0, 4.0), 3.0);
        assert_eq!(from_edge(Edge::Right, 3.0, 4.0), WIN_W - 7.0);
        // The expanded card stays inside the window on both edges.
        assert!(from_edge(Edge::Right, CARD_GAP, CARD_W) >= 0.0);
        assert!(from_edge(Edge::Left, CARD_GAP, CARD_W) + CARD_W <= WIN_W);
    }
}
