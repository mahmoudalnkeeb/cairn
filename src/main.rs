#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Cairn: a drop shelf for Windows. Drag anything to the screen edge to park
//! it, then drag it out into any app. See PRD.md.

mod clipboard;
mod dnd;
mod edgetab;
mod gfx;
mod select;
mod settings;
mod shelf;
mod stackview;
mod store;

use std::cell::Cell;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::OleInitialize;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use gfx::{Color, Gfx, Surface, Theme};
use settings::{Edge, Settings};

const WM_TRAY: u32 = WM_APP + 1;
const HOTKEY_ID: i32 = 1;
const FULLSCREEN_TIMER: usize = 1;
const PASTE_TIMER: usize = 2;
const UNDO_TIMER: usize = 3;
const UNDO_MS: u32 = 10_000;

const CMD_OPEN: u32 = 1;
const CMD_RUN_AT_LOGIN: u32 = 2;
const CMD_QUIT: u32 = 3;
const CMD_CLEAR: u32 = 4;
const CMD_UNDO_CLEAR: u32 = 5;
const CMD_EDGE_LEFT: u32 = 10;
const CMD_EDGE_RIGHT: u32 = 11;
const CMD_HOTKEY_BASE: u32 = 100;

pub struct App {
    pub gfx: Gfx,
    pub theme: Theme,
    pub animations: bool,
    pub host: HWND,
    pub tab: edgetab::EdgeTab,
    pub stack: shelf::Shelf,
    pub stack_view: stackview::StackView,
    pub settings: Settings,
    /// What "Clear stack" took, while it can still be undone.
    cleared: Vec<shelf::ShelfItem>,
    hotkey: Option<&'static str>,
    tray_icon: HICON,
    taskbar_created: u32,
}

thread_local! {
    static APP: Cell<*mut App> = const { Cell::new(std::ptr::null_mut()) };
}

/// The app lives for the whole message loop on the UI thread.
fn app() -> Option<&'static mut App> {
    let p = APP.with(|a| a.get());
    unsafe { p.as_mut() }
}

pub fn dpi_at(pt: POINT) -> u32 {
    unsafe {
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let (mut x, mut y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut x, &mut y);
        x
    }
}

fn animations_enabled() -> bool {
    let mut on = TRUE;
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut on as *mut BOOL as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    on.as_bool()
}

fn wide<const N: usize>(s: &str) -> [u16; N] {
    let mut out = [0u16; N];
    for (o, c) in out.iter_mut().zip(s.encode_utf16().take(N - 1)) {
        *o = c;
    }
    out
}

fn tray_data(app: &App) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: app.host, uID: 1, ..Default::default() }
}

fn tray_tip(app: &App) -> [u16; 128] {
    wide(&match app.hotkey {
        Some(k) => format!("Cairn ({k})"),
        None => "Cairn".to_string(),
    })
}

fn tray_add(app: &App) {
    let mut nid = tray_data(app);
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = app.tray_icon;
    nid.szTip = tray_tip(app);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

fn tray_update(app: &App) {
    let mut nid = tray_data(app);
    nid.uFlags = NIF_ICON | NIF_TIP;
    nid.hIcon = app.tray_icon;
    nid.szTip = tray_tip(app);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

pub fn notify(app: &App, title: &str, text: &str) {
    let mut nid = tray_data(app);
    nid.uFlags = NIF_INFO;
    nid.szInfoTitle = wide(title);
    nid.szInfo = wide(text);
    nid.dwInfoFlags = NIIF_NONE | NIIF_NOSOUND;
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

/// The one-color mark (brand/cairn-mono.svg), white on a dark taskbar and
/// near-black on a light one.
fn make_tray_icon(gfx: &Gfx) -> HICON {
    let dpi = dpi_at(POINT::default());
    let side = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) };
    let Ok(surf) = Surface::new(gfx, side, side) else { return HICON::default() };
    let ink = if gfx::taskbar_is_light() { Color::hex(0x191b1d, 0xff) } else { Color::hex(0xffffff, 0xff) };
    let k = side as f32 / 16.0;
    let _ = surf.draw(96, |p| {
        for (x, y, w) in [(1.0, 11.0, 14.0), (2.0, 6.0, 10.0), (7.0, 1.0, 6.0)] {
            p.fill_rounded(x * k, y * k, w * k, 4.0 * k, 2.0 * k, ink);
        }
    });
    surf.to_icon().unwrap_or_default()
}

pub fn toggle_stack(app: &mut App) {
    if app.stack_view.open {
        stackview::close(app, true);
    } else {
        let target = unsafe { GetForegroundWindow() };
        stackview::open(app, target);
    }
}

/// A drag carrying something Cairn accepts is over the tab or the stack, or
/// has left it.
pub fn on_drag_hover(zone: dnd::Zone, over: bool) {
    let Some(app) = app() else { return };
    match zone {
        dnd::Zone::Tab => edgetab::set_drag_over(app, over),
        dnd::Zone::Stack => {
            app.stack_view.drop_hover = over;
            stackview::render(app);
        }
    }
}

pub fn on_drop(zone: dnd::Zone, dropped: shelf::Dropped) {
    let Some(app) = app() else { return };
    let added = app.stack.add(dropped);
    if added > 0 {
        app.stack.save();
    }
    match zone {
        dnd::Zone::Tab => {
            edgetab::dropped(app, added);
            if app.stack_view.open {
                stackview::refresh(app);
            }
        }
        dnd::Zone::Stack => stackview::refresh(app),
    }
}

/// Sends Ctrl+V once focus has settled back in the target app.
pub fn schedule_paste(app: &App) {
    unsafe {
        SetTimer(app.host, PASTE_TIMER, 60, None);
    }
}

/// Empties the stack and offers undo for 10 seconds: click the notification
/// or choose "Undo clear" from the tray menu.
fn clear_stack(app: &mut App) {
    let cleared = app.stack.clear();
    if cleared.is_empty() {
        return;
    }
    app.stack.save();
    stackview::close(app, false);
    let n = cleared.len();
    app.cleared = cleared;
    let what = if n == 1 { "1 item".to_string() } else { format!("{n} items") };
    notify(app, "Stack cleared", &format!("Removed {what}. Click here within 10 seconds to undo."));
    unsafe {
        SetTimer(app.host, UNDO_TIMER, UNDO_MS, None);
    }
}

fn undo_clear(app: &mut App) {
    unsafe {
        let _ = KillTimer(app.host, UNDO_TIMER);
    }
    if app.cleared.is_empty() {
        return;
    }
    let back = app.stack.restore(std::mem::take(&mut app.cleared));
    app.stack.save();
    if app.stack_view.open {
        stackview::refresh(app);
    }
    let what = if back == 1 { "1 item is".to_string() } else { format!("{back} items are") };
    notify(app, "Clear undone", &format!("{what} back in the stack."));
}

fn register_hotkey(host: HWND, i: usize) -> bool {
    let h = &settings::HOTKEYS[i];
    unsafe { RegisterHotKey(host, HOTKEY_ID, h.mods | MOD_NOREPEAT, h.vk.0 as u32).is_ok() }
}

/// Switches the global hotkey, keeping the old one if the new one is taken.
fn choose_hotkey(app: &mut App, i: usize) {
    let label = settings::HOTKEYS[i].label;
    if app.hotkey == Some(label) {
        return;
    }
    unsafe {
        let _ = UnregisterHotKey(app.host, HOTKEY_ID);
    }
    if register_hotkey(app.host, i) {
        app.hotkey = Some(label);
        app.settings.hotkey = i;
        app.settings.save();
        tray_update(app);
    } else {
        if let Some(o) = app.hotkey.and_then(|l| settings::HOTKEYS.iter().position(|h| h.label == l)) {
            register_hotkey(app.host, o);
        }
        notify(app, "Hotkey unavailable", &format!("{label} is in use by another app. Cairn kept {}.", app.hotkey.unwrap_or("no hotkey")));
    }
}

fn set_edge(app: &mut App, edge: Edge) {
    if app.settings.edge != edge {
        app.settings.edge = edge;
        app.settings.save();
        stackview::close(app, false);
        edgetab::position(app);
    }
}

pub fn show_context_menu(app: &mut App) {
    stackview::close(app, false);
    unsafe {
        let Ok(popup) = CreatePopupMenu() else { return };
        let open = HSTRING::from(format!("Open stack\t{}", app.hotkey.unwrap_or_default()));
        let _ = AppendMenuW(popup, MF_STRING, CMD_OPEN as usize, &open);
        if app.cleared.is_empty() {
            let empty = if app.stack.items.is_empty() { MF_GRAYED } else { MF_ENABLED };
            let _ = AppendMenuW(popup, MF_STRING | empty, CMD_CLEAR as usize, w!("Clear stack"));
        } else {
            let _ = AppendMenuW(popup, MF_STRING, CMD_UNDO_CLEAR as usize, w!("Undo clear"));
        }
        let _ = AppendMenuW(popup, MF_SEPARATOR, 0, None);
        if let Ok(keys) = CreatePopupMenu() {
            for (i, h) in settings::HOTKEYS.iter().enumerate() {
                let checked = if app.hotkey == Some(h.label) { MF_CHECKED } else { MF_UNCHECKED };
                let _ = AppendMenuW(keys, MF_STRING | checked, CMD_HOTKEY_BASE as usize + i, &HSTRING::from(h.label));
            }
            let _ = AppendMenuW(popup, MF_POPUP, keys.0 as usize, w!("Hotkey"));
        }
        if let Ok(edges) = CreatePopupMenu() {
            let check = |e: Edge| if app.settings.edge == e { MF_CHECKED } else { MF_UNCHECKED };
            let _ = AppendMenuW(edges, MF_STRING | check(Edge::Left), CMD_EDGE_LEFT as usize, w!("Left"));
            let _ = AppendMenuW(edges, MF_STRING | check(Edge::Right), CMD_EDGE_RIGHT as usize, w!("Right"));
            let _ = AppendMenuW(popup, MF_POPUP, edges.0 as usize, w!("Screen edge"));
        }
        let login = if settings::run_at_login() { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(popup, MF_STRING | login, CMD_RUN_AT_LOGIN as usize, w!("Run at login"));
        let _ = AppendMenuW(popup, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(popup, MF_STRING, CMD_QUIT as usize, w!("Quit Cairn"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(app.host);
        let cmd = TrackPopupMenu(popup, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, 0, app.host, None);
        let _ = PostMessageW(app.host, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(popup);
        match cmd.0 as u32 {
            CMD_OPEN => toggle_stack(app),
            CMD_CLEAR => clear_stack(app),
            CMD_UNDO_CLEAR => undo_clear(app),
            CMD_EDGE_LEFT => set_edge(app, Edge::Left),
            CMD_EDGE_RIGHT => set_edge(app, Edge::Right),
            CMD_RUN_AT_LOGIN => {
                let on = !settings::run_at_login();
                if !settings::set_run_at_login(on) {
                    notify(app, "Run at login", "Windows did not accept the change. Try again.");
                }
            }
            c if (CMD_HOTKEY_BASE..CMD_HOTKEY_BASE + settings::HOTKEYS.len() as u32).contains(&c) => {
                choose_hotkey(app, (c - CMD_HOTKEY_BASE) as usize)
            }
            CMD_QUIT => quit(app),
            _ => {}
        }
    }
}

fn quit(app: &mut App) {
    unsafe {
        let _ = UnregisterHotKey(app.host, HOTKEY_ID);
        let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(app));
        PostQuitMessage(0);
    }
}

fn foreground_is_fullscreen() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == GetShellWindow() || fg == GetDesktopWindow() {
            return false;
        }
        // Click-through overlays (recorders, assistants) are not apps.
        if GetWindowLongW(fg, GWL_EXSTYLE) as u32 & WS_EX_TRANSPARENT.0 != 0 {
            return false;
        }
        let mut rect = RECT::default();
        if GetWindowRect(fg, &mut rect).is_err() {
            return false;
        }
        let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut info);
        let m = info.rcMonitor;
        rect.left <= m.left && rect.top <= m.top && rect.right >= m.right && rect.bottom >= m.bottom
    }
}

/// Hides the tab while a fullscreen app, game or presentation is up, and
/// keeps it above other topmost windows otherwise.
fn poll_fullscreen(app: &mut App) {
    let busy = match unsafe { SHQueryUserNotificationState() } {
        Ok(QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE) => true,
        // QUNS_BUSY also fires for click-through overlays, so require the
        // foreground window itself to cover its monitor.
        Ok(QUNS_BUSY) => foreground_is_fullscreen(),
        _ => false,
    };
    if busy != app.tab.fullscreen_hidden {
        app.tab.fullscreen_hidden = busy;
        if busy {
            stackview::close(app, false);
        }
        edgetab::show(app);
    }
    edgetab::raise(app);
}

fn host_handle(app: &mut App, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    match msg {
        WM_HOTKEY if wparam.0 as i32 == HOTKEY_ID => toggle_stack(app),
        WM_TRAY => match lparam.0 as u32 {
            WM_LBUTTONUP => toggle_stack(app),
            WM_RBUTTONUP => show_context_menu(app),
            NIN_BALLOONUSERCLICK if !app.cleared.is_empty() => undo_clear(app),
            _ => {}
        },
        WM_TIMER if wparam.0 == UNDO_TIMER => {
            unsafe {
                let _ = KillTimer(app.host, UNDO_TIMER);
            }
            app.cleared.clear();
        }
        WM_TIMER if wparam.0 == FULLSCREEN_TIMER => poll_fullscreen(app),
        WM_TIMER if wparam.0 == PASTE_TIMER => {
            unsafe {
                let _ = KillTimer(app.host, PASTE_TIMER);
            }
            clipboard::send_paste();
        }
        WM_SETTINGCHANGE => {
            app.theme = Theme::system();
            app.animations = animations_enabled();
            let old = app.tray_icon;
            app.tray_icon = make_tray_icon(&app.gfx);
            tray_update(app);
            unsafe {
                let _ = DestroyIcon(old);
            }
            edgetab::render(app);
        }
        WM_DISPLAYCHANGE => edgetab::position(app),
        m if m == app.taskbar_created && m != 0 => tray_add(app),
        _ => return None,
    }
    Some(LRESULT(0))
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(app) = app() {
        let handled = if hwnd == app.tab.hwnd {
            edgetab::handle(app, hwnd, msg, wparam, lparam)
        } else if hwnd == app.stack_view.hwnd {
            stackview::handle(app, hwnd, msg, wparam, lparam)
        } else if hwnd == app.host {
            host_handle(app, msg, wparam, lparam)
        } else {
            None
        };
        if let Some(r) = handled {
            return r;
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn create_window(class: PCWSTR, ex: WINDOW_EX_STYLE) -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        RegisterClassW(&wc);
        CreateWindowExW(ex, class, w!("Cairn"), WS_POPUP, 0, 0, 0, 0, None, None, instance, None)
    }
}

fn main() -> Result<()> {
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\Cairn.SingleInstance"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // Drag and drop needs OLE on this (the UI) thread.
        OleInitialize(None)?;

        let floating = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW;
        let host = create_window(w!("CairnHost"), WS_EX_TOOLWINDOW)?;
        let tab_hwnd = create_window(w!("CairnTab"), floating | WS_EX_NOACTIVATE)?;
        let stack_hwnd = create_window(w!("CairnStack"), floating)?;

        let gfx = Gfx::new()?;
        let tray_icon = make_tray_icon(&gfx);
        let app = Box::leak(Box::new(App {
            gfx,
            theme: Theme::system(),
            animations: animations_enabled(),
            host,
            tab: edgetab::EdgeTab::new(tab_hwnd),
            stack: shelf::Shelf::load(),
            stack_view: stackview::StackView::new(stack_hwnd),
            settings: Settings::load(),
            cleared: Vec::new(),
            hotkey: None,
            tray_icon,
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
        }));
        APP.with(|a| a.set(app as *mut App));

        // The saved hotkey first, then the others. A fallback becomes the
        // saved choice so the notice shows only once.
        let preferred = app.settings.hotkey;
        let chosen = std::iter::once(preferred)
            .chain((0..settings::HOTKEYS.len()).filter(|&i| i != preferred))
            .find(|&i| register_hotkey(host, i));
        app.hotkey = chosen.map(|i| settings::HOTKEYS[i].label);

        tray_add(app);
        edgetab::position(app);
        edgetab::show(app);
        let _ = dnd::register(tab_hwnd, dnd::Zone::Tab);
        let _ = dnd::register(stack_hwnd, dnd::Zone::Stack);
        SetTimer(host, FULLSCREEN_TIMER, 1000, None);
        match chosen {
            None => notify(app, "Hotkey unavailable", "Every Cairn hotkey is in use by another app. Use the edge tab or the tray icon."),
            Some(i) if i != preferred => {
                let (was, now) = (settings::HOTKEYS[preferred].label, settings::HOTKEYS[i].label);
                notify(app, "Hotkey changed", &format!("{was} is in use by another app, so Cairn uses {now}. Change it from the tray icon."));
                app.settings.hotkey = i;
                app.settings.save();
            }
            Some(_) => {}
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
