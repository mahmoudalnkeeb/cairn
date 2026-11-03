#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Cairn: a drop shelf for Windows. Drag anything to the screen edge to park
//! it, then drag it out into any app.

mod a11y;
mod actions;
mod clipboard;
mod dnd;
mod edgetab;
mod gfx;
mod nearzone;
mod select;
mod settings;
mod shelf;
mod stackview;
mod store;

use std::cell::Cell;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::{OleGetClipboard, OleInitialize};
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
const CLIP_HOTKEY_ID: i32 = 2;
const CLIP_HOTKEY: &str = "Ctrl+Alt+V";
const FULLSCREEN_TIMER: usize = 1;
const PASTE_TIMER: usize = 2;
const UNDO_TIMER: usize = 3;
const UNDO_MS: u32 = 10_000;
const SPRING_TIMER: usize = 4;
const SPRING_MS: u32 = 500;
const DRAG_CLOSE_TIMER: usize = 5;

const CMD_OPEN: u32 = 1;
const CMD_RUN_AT_LOGIN: u32 = 2;
const CMD_QUIT: u32 = 3;
const CMD_CLEAR: u32 = 4;
const CMD_UNDO_CLEAR: u32 = 5;
const CMD_ADD_CLIPBOARD: u32 = 6;
const CMD_DRIVE_COPIES: u32 = 7;
const CMD_SHAKE: u32 = 8;

/// Posted by a worker when a file copy finishes: wParam is the item id,
/// lParam a boxed Option<PathBuf> with the copy.
const WM_COPY_DONE: u32 = WM_APP + 5;
const CMD_EDGE_LEFT: u32 = 10;
const CMD_EDGE_RIGHT: u32 = 11;
const CMD_CENTER_TAB: u32 = 12;
const CMD_HOTKEY_BASE: u32 = 100;
const CMD_MONITOR_BASE: u32 = 200;

pub struct App {
    pub gfx: Gfx,
    pub theme: Theme,
    pub animations: bool,
    pub host: HWND,
    /// One edge tab per enabled monitor.
    pub tabs: Vec<edgetab::EdgeTab>,
    /// The tab the stack opens beside.
    pub active_tab: usize,
    pub stack: shelf::Shelf,
    pub stack_view: stackview::StackView,
    pub near: nearzone::NearZone,
    pub settings: Settings,
    /// What "Clear stack" took, while it can still be undone.
    cleared: Vec<shelf::ShelfItem>,
    hotkey: Option<&'static str>,
    /// Whether Ctrl+Alt+V (add clipboard) could be registered.
    clip_hotkey: bool,
    /// Where the zip being made will be saved.
    pub zip_out: Option<std::path::PathBuf>,
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

/// For COM callbacks (screen readers), which run on the UI thread too.
pub fn app_ref() -> Option<&'static App> {
    app().map(|a| &*a)
}

pub fn app_mut() -> Option<&'static mut App> {
    app()
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
        // Open beside the tab on the monitor the user is working on.
        app.active_tab = edgetab::under_cursor(app);
        let target = unsafe { GetForegroundWindow() };
        stackview::open(app, target);
    }
}

fn tab_index(app: &App, hwnd: HWND) -> Option<usize> {
    app.tabs.iter().position(|t| t.hwnd == hwnd)
}

/// A drag carrying something Cairn accepts is over a tab or the stack, or
/// has left it.
pub fn on_drag_hover(zone: dnd::Zone, over: bool) {
    let Some(app) = app() else { return };
    match zone {
        dnd::Zone::Tab(hwnd) => {
            if let Some(i) = tab_index(app, hwnd) {
                if over {
                    app.active_tab = i;
                }
                edgetab::set_drag_over(app, i, over);
            }
            unsafe {
                if over {
                    // Resting on the tab opens the stack, so the drop can go
                    // into a stack of the user's choice.
                    SetTimer(app.host, SPRING_TIMER, SPRING_MS, None);
                    let _ = KillTimer(app.host, DRAG_CLOSE_TIMER);
                } else {
                    let _ = KillTimer(app.host, SPRING_TIMER);
                    close_drag_stack_soon(app, 400);
                }
            }
        }
        dnd::Zone::Stack => {
            app.stack_view.drop_hover = over;
            stackview::render(app);
            if over {
                unsafe {
                    let _ = KillTimer(app.host, DRAG_CLOSE_TIMER);
                }
            } else {
                close_drag_stack_soon(app, 400);
            }
        }
        dnd::Zone::Near => nearzone::set_hover(app, over),
    }
}

/// The drag moved: over the stack, pointing at a stack name switches to it.
pub fn on_drag_move(zone: dnd::Zone, pt: POINT) {
    let Some(app) = app() else { return };
    if zone == dnd::Zone::Stack && app.stack_view.open {
        stackview::drag_over(app, pt);
    }
}

/// A stack opened by a drag closes by itself once the drag is gone.
fn close_drag_stack_soon(app: &App, ms: u32) {
    if app.stack_view.dragging_in {
        unsafe {
            SetTimer(app.host, DRAG_CLOSE_TIMER, ms, None);
        }
    }
}

pub fn on_drop(zone: dnd::Zone, dropped: shelf::Dropped) {
    let Some(app) = app() else { return };
    unsafe {
        let _ = KillTimer(app.host, SPRING_TIMER);
    }
    let added = app.stack.add(dropped);
    if added > 0 {
        app.stack.save();
        copy_from_drives(app, added);
    }
    match zone {
        dnd::Zone::Tab(hwnd) => {
            if let Some(i) = tab_index(app, hwnd) {
                edgetab::dropped(app, i, added);
            }
        }
        dnd::Zone::Near => nearzone::dropped(app, added),
        dnd::Zone::Stack => {}
    }
    if app.stack_view.open {
        stackview::refresh(app);
    }
    // Leave the new item in view for a moment before closing.
    close_drag_stack_soon(app, 900);
}

/// USB sticks, card readers, discs and network shares: files there may be
/// gone the next time the stack is used.
fn on_unreliable_drive(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy();
    if s.starts_with("\\\\") {
        return true;
    }
    let Some(root) = s.get(..3).filter(|r| r.ends_with(":\\")) else { return false };
    let kind = unsafe { windows::Win32::Storage::FileSystem::GetDriveTypeW(&HSTRING::from(root)) };
    matches!(kind, 2 | 4 | 5) // DRIVE_REMOVABLE, DRIVE_REMOTE, DRIVE_CDROM
}

/// With the setting on, copies newly added files from removable and network
/// drives into Cairn's folder in the background, then points the items at
/// the copies.
fn copy_from_drives(app: &App, added: usize) {
    if !app.settings.drive_copies {
        return;
    }
    let host = app.host.0 as isize;
    for item in &app.stack.items[..added.min(app.stack.items.len())] {
        if let shelf::Kind::File(p) = &item.kind
            && p.is_file()
            && on_unreliable_drive(p)
        {
            let (id, src) = (item.id, p.clone());
            std::thread::spawn(move || {
                let copy = Box::new(shelf::copy_in(&src));
                unsafe {
                    let _ = PostMessageW(HWND(host as *mut _), WM_COPY_DONE, WPARAM(id as usize), LPARAM(Box::into_raw(copy) as isize));
                }
            });
        }
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
    offer_undo(app, cleared, "Stack cleared");
}

/// Keeps removed items for 10 seconds: clicking the notification or "Undo"
/// in the tray menu puts them back in the current stack.
pub fn offer_undo(app: &mut App, items: Vec<shelf::ShelfItem>, title: &str) {
    let n = items.len();
    app.cleared = items;
    let what = if n == 1 { "1 item".to_string() } else { format!("{n} items") };
    notify(app, title, &format!("Removed {what}. Click here within 10 seconds to undo."));
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
    let name = &app.stack.names[app.stack.current];
    notify(app, "Undone", &format!("{what} back in {name}."));
}

/// Adds whatever is on the clipboard (files, a link, text or an image) to the
/// current stack, and shows the result on the tab under the cursor.
fn add_clipboard(app: &mut App) {
    let dropped = unsafe { OleGetClipboard() }.ok().and_then(|data| dnd::read(&data));
    let Some(dropped) = dropped else {
        notify(app, "Nothing to add", "The clipboard holds no files, links, text or images.");
        return;
    };
    let added = app.stack.add(dropped);
    if added > 0 {
        app.stack.save();
        copy_from_drives(app, added);
    }
    if app.stack_view.open {
        stackview::refresh(app);
    } else {
        let i = edgetab::under_cursor(app);
        edgetab::dropped(app, i, added);
    }
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
        edgetab::sync(app);
    }
}

fn center_tab(app: &mut App) {
    app.settings.tab_pos = 0.5;
    app.settings.save();
    stackview::close(app, false);
    edgetab::sync(app);
}

/// Turns the tab on or off for one monitor, keeping at least one on.
fn toggle_monitor(app: &mut App, device: String) {
    let off = &mut app.settings.monitors_off;
    if let Some(at) = off.iter().position(|d| *d == device) {
        off.remove(at);
    } else {
        let on = edgetab::monitors().iter().filter(|m| !off.contains(&m.device)).count();
        if on <= 1 {
            notify(app, "Monitors", "Cairn needs the tab on at least one monitor.");
            return;
        }
        off.push(device);
    }
    app.settings.save();
    stackview::close(app, false);
    edgetab::sync(app);
}

pub fn show_context_menu(app: &mut App) {
    stackview::close(app, false);
    unsafe {
        let Ok(popup) = CreatePopupMenu() else { return };
        let open = HSTRING::from(format!("Open stack\t{}", app.hotkey.unwrap_or_default()));
        let _ = AppendMenuW(popup, MF_STRING, CMD_OPEN as usize, &open);
        let clip = HSTRING::from(match app.clip_hotkey {
            true => format!("Add clipboard to stack\t{CLIP_HOTKEY}"),
            false => "Add clipboard to stack".to_string(),
        });
        let _ = AppendMenuW(popup, MF_STRING, CMD_ADD_CLIPBOARD as usize, &clip);
        if app.cleared.is_empty() {
            let empty = if app.stack.items.is_empty() { MF_GRAYED } else { MF_ENABLED };
            let _ = AppendMenuW(popup, MF_STRING | empty, CMD_CLEAR as usize, w!("Clear stack"));
        } else {
            let _ = AppendMenuW(popup, MF_STRING, CMD_UNDO_CLEAR as usize, w!("Undo"));
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
            let _ = AppendMenuW(edges, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(edges, MF_STRING, CMD_CENTER_TAB as usize, w!("Center the tab"));
            let _ = AppendMenuW(popup, MF_POPUP, edges.0 as usize, w!("Screen edge"));
        }
        let monitors = edgetab::monitors();
        if monitors.len() > 1
            && let Ok(mons) = CreatePopupMenu()
        {
            for (i, m) in monitors.iter().enumerate() {
                let checked = if app.settings.monitors_off.contains(&m.device) { MF_UNCHECKED } else { MF_CHECKED };
                let (w, h) = (m.work.right - m.work.left, m.work.bottom - m.work.top);
                let main = if m.primary { ", main" } else { "" };
                let label = HSTRING::from(format!("Display {} ({w} × {h}{main})", i + 1));
                let _ = AppendMenuW(mons, MF_STRING | checked, CMD_MONITOR_BASE as usize + i, &label);
            }
            let _ = AppendMenuW(popup, MF_POPUP, mons.0 as usize, w!("Show tab on"));
        }
        let login = if settings::run_at_login() { MF_CHECKED } else { MF_UNCHECKED };
        let check = |on: bool| if on { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(popup, MF_STRING | check(app.settings.shake), CMD_SHAKE as usize, w!("Shake while dragging to show a drop zone"));
        let _ = AppendMenuW(popup, MF_STRING | check(app.settings.drive_copies), CMD_DRIVE_COPIES as usize, w!("Keep copies of files from USB and network drives"));
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
            CMD_ADD_CLIPBOARD => add_clipboard(app),
            CMD_CLEAR => clear_stack(app),
            CMD_UNDO_CLEAR => undo_clear(app),
            CMD_EDGE_LEFT => set_edge(app, Edge::Left),
            CMD_EDGE_RIGHT => set_edge(app, Edge::Right),
            CMD_CENTER_TAB => center_tab(app),
            c if (CMD_MONITOR_BASE..CMD_MONITOR_BASE + monitors.len() as u32).contains(&c) => {
                toggle_monitor(app, monitors[(c - CMD_MONITOR_BASE) as usize].device.clone())
            }
            CMD_DRIVE_COPIES => {
                app.settings.drive_copies = !app.settings.drive_copies;
                app.settings.save();
            }
            CMD_SHAKE => {
                app.settings.shake = !app.settings.shake;
                app.settings.save();
                nearzone::set_enabled(app, app.settings.shake);
            }
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
        let _ = UnregisterHotKey(app.host, CLIP_HOTKEY_ID);
        let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(app));
        app.stack.prune_owned(&[]);
        PostQuitMessage(0);
    }
}

/// The monitor of the foreground window, if that window is a real app.
fn foreground_monitor() -> Option<(HWND, HMONITOR)> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == GetShellWindow() || fg == GetDesktopWindow() {
            return None;
        }
        // Click-through overlays (recorders, assistants) are not apps.
        if GetWindowLongW(fg, GWL_EXSTYLE) as u32 & WS_EX_TRANSPARENT.0 != 0 {
            return None;
        }
        Some((fg, MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST)))
    }
}

fn covers_monitor(fg: HWND, mon: HMONITOR) -> bool {
    unsafe {
        let mut rect = RECT::default();
        if GetWindowRect(fg, &mut rect).is_err() {
            return false;
        }
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut info);
        let m = info.rcMonitor;
        rect.left <= m.left && rect.top <= m.top && rect.right >= m.right && rect.bottom >= m.bottom
    }
}

/// Hides the tab on the monitor where a fullscreen app, game or
/// presentation is up, and keeps the tabs above other topmost windows.
fn poll_fullscreen(app: &mut App) {
    let fg = foreground_monitor();
    let busy = match unsafe { SHQueryUserNotificationState() } {
        Ok(QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE) => fg.map(|(_, m)| m),
        // QUNS_BUSY also fires for click-through overlays, so require the
        // foreground window itself to cover its monitor.
        Ok(QUNS_BUSY) => fg.filter(|&(w, m)| covers_monitor(w, m)).map(|(_, m)| m),
        _ => None,
    };
    for i in 0..app.tabs.len() {
        let hide = busy == Some(app.tabs[i].monitor);
        if hide != app.tabs[i].fullscreen_hidden {
            app.tabs[i].fullscreen_hidden = hide;
            if hide && app.active_tab == i {
                stackview::close(app, false);
            }
            edgetab::show(app, i);
        }
    }
    edgetab::raise(app);
}

fn host_handle(app: &mut App, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    match msg {
        WM_HOTKEY if wparam.0 as i32 == HOTKEY_ID => toggle_stack(app),
        WM_HOTKEY if wparam.0 as i32 == CLIP_HOTKEY_ID => add_clipboard(app),
        nearzone::WM_SHAKE if !app.near.shown && !app.stack_view.open => {
            let pt = POINT { x: (lparam.0 & 0xffff_ffff) as u32 as i32, y: (lparam.0 >> 32) as i32 };
            nearzone::show_at(app, pt);
        }
        nearzone::WM_SHAKE_END => nearzone::hide_soon(app, 500),
        WM_COPY_DONE => {
            let copy = unsafe { Box::from_raw(lparam.0 as *mut Option<std::path::PathBuf>) };
            match *copy {
                Some(path) if app.stack.replace_path(wparam.0 as u64, path.clone()) => {
                    app.stack.save();
                    if app.stack_view.open {
                        stackview::refresh(app);
                    }
                }
                // The item was removed while copying.
                Some(path) => {
                    let _ = std::fs::remove_file(path);
                }
                None => notify(app, "Copy failed", "Cairn could not keep a copy of a file from a removable or network drive. The item still points at the drive."),
            }
        }
        actions::WM_ZIP_DONE => match wparam.0 {
            1 => {
                let note = app.zip_out.take().map_or("The zip is ready.".to_string(), |p| format!("Saved {}", p.display()));
                notify(app, "Zip saved", &note)
            }
            _ => notify(app, "Zip failed", "Windows could not create the zip. Check the folder can be written to."),
        },
        WM_TRAY => match lparam.0 as u32 {
            WM_LBUTTONUP => toggle_stack(app),
            WM_RBUTTONUP => show_context_menu(app),
            NIN_BALLOONUSERCLICK if !app.cleared.is_empty() => undo_clear(app),
            _ => {}
        },
        WM_TIMER if wparam.0 == SPRING_TIMER => {
            unsafe {
                let _ = KillTimer(app.host, SPRING_TIMER);
            }
            if edgetab::is_drag_over(app, app.active_tab) {
                stackview::open_for_drag(app);
            }
        }
        WM_TIMER if wparam.0 == DRAG_CLOSE_TIMER => {
            unsafe {
                let _ = KillTimer(app.host, DRAG_CLOSE_TIMER);
            }
            let still_dragging = app.stack_view.drop_hover || (0..app.tabs.len()).any(|i| edgetab::is_drag_over(app, i));
            if app.stack_view.dragging_in && !still_dragging {
                stackview::close(app, false);
            }
        }
        WM_TIMER if wparam.0 == UNDO_TIMER => {
            unsafe {
                let _ = KillTimer(app.host, UNDO_TIMER);
            }
            app.cleared.clear();
            app.stack.prune_owned(&[]);
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
            // The taskbar may have moved, changing the work areas.
            edgetab::sync(app);
        }
        WM_DISPLAYCHANGE => {
            stackview::close(app, false);
            edgetab::sync(app);
        }
        m if m == app.taskbar_created && m != 0 => tray_add(app),
        _ => return None,
    }
    Some(LRESULT(0))
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(app) = app() {
        let handled = if let Some(i) = tab_index(app, hwnd) {
            edgetab::handle(app, i, hwnd, msg, wparam, lparam)
        } else if hwnd == app.near.hwnd {
            nearzone::handle(app, msg, wparam)
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

pub fn create_window(class: PCWSTR, ex: WINDOW_EX_STYLE) -> Result<HWND> {
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
        let stack_hwnd = create_window(w!("CairnStack"), floating)?;
        let near_hwnd = create_window(w!("CairnNear"), floating | WS_EX_NOACTIVATE)?;

        let gfx = Gfx::new()?;
        let tray_icon = make_tray_icon(&gfx);
        let app = Box::leak(Box::new(App {
            gfx,
            theme: Theme::system(),
            animations: animations_enabled(),
            host,
            tabs: Vec::new(),
            active_tab: 0,
            stack: shelf::Shelf::load(),
            stack_view: stackview::StackView::new(stack_hwnd),
            near: nearzone::NearZone::new(near_hwnd),
            settings: Settings::load(),
            cleared: Vec::new(),
            hotkey: None,
            clip_hotkey: false,
            zip_out: None,
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
        app.clip_hotkey = RegisterHotKey(host, CLIP_HOTKEY_ID, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, VK_V.0 as u32).is_ok();

        tray_add(app);
        edgetab::sync(app);
        let _ = dnd::register(stack_hwnd, dnd::Zone::Stack);
        nearzone::register(near_hwnd);
        nearzone::set_enabled(app, app.settings.shake);
        // Copies nothing refers to any more, from removals since the last run.
        app.stack.prune_owned(&[]);
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
