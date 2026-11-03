//! The edge tab: a slim pill on a screen edge of each enabled monitor, each a
//! real OLE drop target. When a supported drag reaches one, Windows calls
//! DragEnter and the pill grows into the drop zone, so Cairn never watches the
//! mouse globally.
//!
//! Each window keeps one size. While idle, only a thin strip along the edge
//! has non-zero alpha; everywhere else clicks and drops fall through to the
//! windows underneath. Dragging the pill moves it along the edge.

use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Ole::RevokeDragDrop;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::gfx::{Color, Surface, bezier};
use crate::settings::Edge;
use crate::{App, dnd, dpi_at};

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

/// A monitor as Cairn sees it: its device name (the key settings use), work
/// area and whether it is the main one.
pub struct Monitor {
    pub handle: HMONITOR,
    pub device: String,
    pub work: RECT,
    pub primary: bool,
}

/// Every connected monitor, main one first, then left to right.
pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn each(mon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        unsafe {
            let list = &mut *(data.0 as *mut Vec<Monitor>);
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(mon, &mut info.monitorInfo).as_bool() {
                let end = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
                list.push(Monitor {
                    handle: mon,
                    device: String::from_utf16_lossy(&info.szDevice[..end]),
                    work: info.monitorInfo.rcWork,
                    primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                });
            }
        }
        TRUE
    }
    let mut list: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut list as *mut _ as isize));
    }
    list.sort_by_key(|m| (!m.primary, m.work.left, m.work.top));
    list
}

pub struct EdgeTab {
    pub hwnd: HWND,
    pub monitor: HMONITOR,
    pub device: String,
    work: RECT,
    pub fullscreen_hidden: bool,
    surface: Option<Surface>,
    dpi: u32,
    origin: (i32, i32),
    drag_over: bool,
    /// Caption shown briefly after a drop ("Added 2").
    flash: Option<String>,
    hover: bool,
    tracked: bool,
    /// Pressed on the pill: cursor y and window y at the press, and whether
    /// it has moved far enough to count as a drag along the edge.
    press: Option<(i32, i32, bool)>,
    /// Expansion: 0 is the idle pill, 1 the full drop zone.
    progress: f32,
    from: f32,
    start: Instant,
}

impl EdgeTab {
    fn expanded(&self) -> bool {
        self.drag_over || self.flash.is_some()
    }
}

/// Where the tab's center sits, as a fraction of the work area's height,
/// kept inside it.
fn center_y(work: &RECT, pos: f32, h: i32) -> i32 {
    let span = work.bottom - work.top;
    let y = work.top + (span as f32 * pos) as i32 - h / 2;
    y.clamp(work.top, (work.bottom - h).max(work.top))
}

/// The fraction `center_y` would turn back into this window top.
fn pos_of(work: &RECT, top: i32, h: i32) -> f32 {
    let span = (work.bottom - work.top).max(1);
    ((top + h / 2 - work.top) as f32 / span as f32).clamp(0.0, 1.0)
}

/// The monitors that get a tab: the ones not turned off in settings, or the
/// main one if that leaves none.
fn enabled(app: &App) -> Vec<Monitor> {
    let all = monitors();
    let on: Vec<Monitor> = all.iter().filter(|m| !app.settings.monitors_off.contains(&m.device)).map(clone).collect();
    if on.is_empty() { all.into_iter().filter(|m| m.primary).collect() } else { on }
}

fn clone(m: &Monitor) -> Monitor {
    Monitor { handle: m.handle, device: m.device.clone(), work: m.work, primary: m.primary }
}

/// Makes one tab per enabled monitor, reusing windows where it can, and
/// puts each against the chosen edge.
pub fn sync(app: &mut App) {
    let want = enabled(app);
    while app.tabs.len() > want.len() {
        let t = app.tabs.pop().unwrap();
        unsafe {
            let _ = RevokeDragDrop(t.hwnd);
            let _ = DestroyWindow(t.hwnd);
        }
    }
    let ex = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    while app.tabs.len() < want.len() {
        let Ok(hwnd) = crate::create_window(w!("CairnTab"), ex) else { break };
        let _ = dnd::register(hwnd, dnd::Zone::Tab(hwnd));
        app.tabs.push(EdgeTab {
            hwnd,
            monitor: HMONITOR::default(),
            device: String::new(),
            work: RECT::default(),
            fullscreen_hidden: false,
            surface: None,
            dpi: 96,
            origin: (0, 0),
            drag_over: false,
            flash: None,
            hover: false,
            tracked: false,
            press: None,
            progress: 0.0,
            from: 0.0,
            start: Instant::now(),
        });
    }
    for (t, m) in app.tabs.iter_mut().zip(want) {
        t.monitor = m.handle;
        t.device = m.device;
        t.work = m.work;
    }
    app.active_tab = app.active_tab.min(app.tabs.len().saturating_sub(1));
    for i in 0..app.tabs.len() {
        position(app, i);
        show(app, i);
    }
}

/// The tab to open the stack from: the one on the monitor under the cursor,
/// or the first.
pub fn under_cursor(app: &App) -> usize {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        app.tabs.iter().position(|t| t.monitor == mon).unwrap_or(0)
    }
}

/// The active tab's inner edge at its vertical center, plus its work area:
/// where the stack flyout opens from.
pub fn anchor(app: &App) -> (POINT, RECT) {
    let Some(t) = app.tabs.get(app.active_tab) else {
        let wa = monitors().into_iter().next().map(|m| m.work).unwrap_or_default();
        return (POINT { x: wa.right, y: (wa.top + wa.bottom) / 2 }, wa);
    };
    let s = t.dpi as f32 / 96.0;
    let hit = (HIT_W * s) as i32;
    let x = match app.settings.edge {
        Edge::Left => t.work.left + hit,
        Edge::Right => t.work.right - hit,
    };
    let h = (WIN_H * s).ceil() as i32;
    (POINT { x, y: t.origin.1 + h / 2 }, t.work)
}

/// Puts a tab against the chosen edge, at the saved height.
pub fn position(app: &mut App, i: usize) {
    let (edge, pos) = (app.settings.edge, app.settings.tab_pos);
    let Some(t) = app.tabs.get_mut(i) else { return };
    let wa = t.work;
    let dpi = dpi_at(POINT { x: (wa.left + wa.right) / 2, y: (wa.top + wa.bottom) / 2 });
    let s = dpi as f32 / 96.0;
    let (w, h) = ((WIN_W * s).ceil() as i32, (WIN_H * s).ceil() as i32);
    let x = match edge {
        Edge::Left => wa.left,
        Edge::Right => wa.right - w,
    };
    t.dpi = dpi;
    t.origin = (x, center_y(&wa, pos, h));
    if t.surface.as_ref().is_none_or(|surf| surf.width != w || surf.height != h) {
        t.surface = Surface::new(&app.gfx, w, h).ok();
    }
    render(app, i);
}

/// Left x of a box `w` wide that sits `gap` DIPs from the screen edge.
fn from_edge(edge: Edge, gap: f32, w: f32) -> f32 {
    match edge {
        Edge::Left => gap,
        Edge::Right => WIN_W - gap - w,
    }
}

pub fn render(app: &mut App, i: usize) {
    let Some(t) = app.tabs.get(i) else { return };
    let Some(surf) = &t.surface else { return };
    let (g, th) = (&app.gfx, &app.theme);
    let edge = app.settings.edge;
    let p = t.progress;
    let lerp = |a: f32, b: f32| a + (b - a) * p;
    let caption = g.text_format(&g.ui_font, 12.0, DWRITE_FONT_WEIGHT_NORMAL).ok();
    let flash = t.flash.clone();
    let hover = t.hover || t.press.is_some_and(|(_, _, moved)| moved);
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

pub fn show(app: &App, i: usize) {
    let Some(t) = app.tabs.get(i) else { return };
    unsafe {
        let _ = ShowWindow(t.hwnd, if t.fullscreen_hidden { SW_HIDE } else { SW_SHOWNOACTIVATE });
    }
}

/// Re-asserts topmost, since other topmost windows can cover the tabs.
pub fn raise(app: &App) {
    for t in app.tabs.iter().filter(|t| !t.fullscreen_hidden) {
        unsafe {
            let _ = SetWindowPos(t.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

fn animate(app: &mut App, i: usize) {
    let Some(t) = app.tabs.get_mut(i) else { return };
    t.from = t.progress;
    t.start = Instant::now();
    unsafe {
        SetTimer(t.hwnd, ANIM_TIMER, 10, None);
    }
    tick(app, i);
}

fn tick(app: &mut App, i: usize) {
    let animations = app.animations;
    let Some(t) = app.tabs.get_mut(i) else { return };
    let target = if t.expanded() { 1.0 } else { 0.0 };
    let dur = if target > t.from { OPEN_MS } else { CLOSE_MS };
    let k = if animations { (t.start.elapsed().as_secs_f32() * 1000.0 / dur).min(1.0) } else { 1.0 };
    t.progress = t.from + (target - t.from) * bezier(0.0, 0.0, 0.0, 1.0, k);
    if k >= 1.0 {
        unsafe {
            let _ = KillTimer(t.hwnd, ANIM_TIMER);
        }
    }
    render(app, i);
}

pub fn set_drag_over(app: &mut App, i: usize, over: bool) {
    if let Some(t) = app.tabs.get_mut(i)
        && t.drag_over != over
    {
        t.drag_over = over;
        animate(app, i);
    }
}

/// Shows what the drop did, then folds back to the pill.
pub fn dropped(app: &mut App, i: usize, added: usize) {
    let Some(t) = app.tabs.get_mut(i) else { return };
    t.flash = Some(match added {
        0 => "Already here".to_string(),
        n => format!("Added {n}"),
    });
    unsafe {
        SetTimer(t.hwnd, FLASH_TIMER, FLASH_MS, None);
    }
    animate(app, i);
}

fn cursor_y() -> i32 {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    pt.y
}

/// Saves where a dragged tab ended up and moves the other tabs to match.
fn finish_move(app: &mut App, i: usize) {
    let t = &app.tabs[i];
    let h = (WIN_H * t.dpi as f32 / 96.0).ceil() as i32;
    app.settings.tab_pos = pos_of(&t.work, t.origin.1, h);
    app.settings.save();
    for j in (0..app.tabs.len()).filter(|&j| j != i) {
        position(app, j);
    }
}

pub fn handle(app: &mut App, i: usize, hwnd: HWND, msg: u32, wparam: WPARAM, _lparam: LPARAM) -> Option<LRESULT> {
    match msg {
        WM_TIMER if wparam.0 == ANIM_TIMER => tick(app, i),
        WM_TIMER if wparam.0 == FLASH_TIMER => {
            unsafe {
                let _ = KillTimer(hwnd, FLASH_TIMER);
            }
            app.tabs[i].flash = None;
            animate(app, i);
        }
        WM_LBUTTONDOWN => {
            let t = &mut app.tabs[i];
            t.press = Some((cursor_y(), t.origin.1, false));
            unsafe {
                SetCapture(hwnd);
            }
        }
        WM_MOUSEMOVE => {
            let t = &mut app.tabs[i];
            if let Some((from_y, from_top, moved)) = t.press {
                let dy = cursor_y() - from_y;
                let moved = moved || dy.abs() > unsafe { GetSystemMetrics(SM_CYDRAG) };
                if moved {
                    let h = (WIN_H * t.dpi as f32 / 96.0).ceil() as i32;
                    t.origin.1 = (from_top + dy).clamp(t.work.top, (t.work.bottom - h).max(t.work.top));
                    t.press = Some((from_y, from_top, true));
                    render(app, i);
                }
            } else if !t.tracked {
                t.tracked = true;
                t.hover = true;
                let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                unsafe {
                    let _ = TrackMouseEvent(&mut tme);
                }
                render(app, i);
            }
        }
        WM_MOUSELEAVE => {
            let t = &mut app.tabs[i];
            t.tracked = false;
            t.hover = false;
            render(app, i);
        }
        WM_LBUTTONUP => {
            let press = app.tabs[i].press.take();
            unsafe {
                let _ = ReleaseCapture();
            }
            match press {
                Some((_, _, true)) => finish_move(app, i),
                Some(_) => {
                    app.active_tab = i;
                    crate::toggle_stack(app);
                }
                None => {}
            }
        }
        WM_CAPTURECHANGED => {
            if app.tabs[i].press.take().is_some_and(|(_, _, moved)| moved) {
                finish_move(app, i);
            }
        }
        WM_RBUTTONUP => crate::show_context_menu(app),
        WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
        WM_DPICHANGED => position(app, i),
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

    #[test]
    fn tab_height_round_trips_and_stays_on_screen() {
        let work = RECT { left: 0, top: 40, right: 1920, bottom: 1040 };
        let top = center_y(&work, 0.3, 200);
        assert_eq!(top, 40 + 300 - 100);
        assert!((pos_of(&work, top, 200) - 0.3).abs() < 0.01);
        // Saved near an end, the window still fits in the work area.
        assert_eq!(center_y(&work, 0.0, 200), 40);
        assert_eq!(center_y(&work, 1.0, 200), 1040 - 200);
    }
}
