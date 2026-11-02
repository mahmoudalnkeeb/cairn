//! The stack flyout (C3 to C6): a grid of parked items beside the edge
//! tab. Select one or several and drag them out into any app; drop more onto
//! the tab or the dashed tile.

use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::System::SystemServices::{MK_CONTROL, MK_SHIFT};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::gfx::Surface;
use crate::select::Selection;
use crate::shelf::{self, Kind};
use crate::settings::Edge;
use crate::{App, clipboard, dnd, dpi_at, edgetab};

const COLS: usize = 4;
const ROWS: usize = 3;
const TILE_W: f32 = 96.0;
const TILE_H: f32 = 88.0;
const GAP: f32 = 8.0;
const M: f32 = 12.0;
const PAD: f32 = 32.0;
const GRID_Y: f32 = M + 28.0 + 8.0;
const GRID_H: f32 = ROWS as f32 * TILE_H + (ROWS - 1) as f32 * GAP;
const FOOTER_Y: f32 = GRID_Y + GRID_H + 12.0;
const CARD_W: f32 = M * 2.0 + COLS as f32 * TILE_W + (COLS - 1) as f32 * GAP;
const CARD_H: f32 = FOOTER_Y + 16.0 + M;
const REMOVE: f32 = 20.0;
const GAP_FROM_TAB: f32 = 8.0;

const GLYPH_TEXT: &str = "\u{E8A5}";
const GLYPH_LINK: &str = "\u{E71B}";
const GLYPH_MISSING: &str = "\u{E7BA}";
const GLYPH_ADD: &str = "\u{E710}";
const GLYPH_REMOVE: &str = "\u{E711}";

pub struct StackView {
    pub hwnd: HWND,
    pub open: bool,
    pub drop_hover: bool,
    /// The keyboard cursor; also where Enter acts.
    focus: usize,
    sel: Selection,
    /// First visible row.
    scroll: usize,
    hover: Option<usize>,
    hover_remove: bool,
    tracked: bool,
    press: Option<(usize, POINT)>,
    last_click: Option<(usize, Instant)>,
    dpi: u32,
    origin: (i32, i32),
    surface: Option<Surface>,
    pub target: HWND,
}

impl StackView {
    pub fn new(hwnd: HWND) -> StackView {
        StackView {
            hwnd,
            open: false,
            drop_hover: false,
            focus: 0,
            sel: Selection::default(),
            scroll: 0,
            hover: None,
            hover_remove: false,
            tracked: false,
            press: None,
            last_click: None,
            dpi: 96,
            origin: (0, 0),
            surface: None,
            target: HWND::default(),
        }
    }
}

/// Grid cells: one per item plus the drop tile at the end.
fn cells(app: &App) -> usize {
    app.stack.items.len() + 1
}

fn ids(app: &App) -> Vec<u64> {
    app.stack.items.iter().map(|i| i.id).collect()
}

fn cell_pos(index: usize, scroll: usize) -> Option<(f32, f32)> {
    let row = index / COLS;
    if row < scroll || row >= scroll + ROWS {
        return None;
    }
    let col = index % COLS;
    Some((M + col as f32 * (TILE_W + GAP), GRID_Y + (row - scroll) as f32 * (TILE_H + GAP)))
}

/// Opens beside the edge tab, on the screen side of it.
fn place(app: &mut App) {
    let (anchor, wa) = edgetab::anchor(app);
    let dpi = dpi_at(anchor);
    let s = dpi as f32 / 96.0;
    let (cw, ch) = ((CARD_W * s) as i32, (CARD_H * s) as i32);
    let gap = (GAP_FROM_TAB * s) as i32;
    let margin = (8.0 * s) as i32;
    let x = match app.settings.edge {
        Edge::Left => anchor.x + gap,
        Edge::Right => anchor.x - gap - cw,
    };
    let x = x.clamp(wa.left + margin, (wa.right - margin - cw).max(wa.left + margin));
    let y = (anchor.y - ch / 2).clamp(wa.top + margin, (wa.bottom - margin - ch).max(wa.top + margin));
    let pad = (PAD * s) as i32;
    let v = &mut app.stack_view;
    v.dpi = dpi;
    v.origin = (x - pad, y - pad);
    let (pw, ph) = (((CARD_W + PAD * 2.0) * s).ceil() as i32, ((CARD_H + PAD * 2.0) * s).ceil() as i32);
    if v.surface.as_ref().is_none_or(|surf| surf.width != pw || surf.height != ph) {
        v.surface = Surface::new(&app.gfx, pw, ph).ok();
    }
}

pub fn render(app: &mut App) {
    if !app.stack_view.open {
        return;
    }
    // Load icons for visible tiles that have none yet.
    let scroll = app.stack_view.scroll;
    for (i, item) in app.stack.items.iter_mut().enumerate() {
        if item.icon.is_none() && cell_pos(i, scroll).is_some() {
            item.icon = Some(match &item.kind {
                Kind::File(p) if p.exists() => shelf::shell_icon(p),
                _ => None,
            });
        }
    }

    let v = &app.stack_view;
    let Some(surf) = &v.surface else { return };
    let (g, t) = (&app.gfx, &app.theme);
    let fmt = |family, size, weight, align| g.text_format_aligned(family, size, weight, align).ok();
    let title = fmt(&g.display_font, 20.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING);
    let name_fmt = fmt(&g.ui_font, 14.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER);
    let caption = fmt(&g.ui_font, 12.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER);
    let caption_lead = fmt(&g.ui_font, 12.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING);
    let caption_end = fmt(&g.ui_font, 12.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING);
    let icon32 = fmt(&g.icon_font, 28.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER);
    let icon12 = fmt(&g.icon_font, 10.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER);
    let items = &app.stack.items;

    let _ = surf.draw(v.dpi, |p| {
        let (cx, cy) = (PAD, PAD);
        p.rect_shadow(cx, cy + 8.0, CARD_W, CARD_H, 8.0, 24.0, 0.55 * t.shadow);
        p.fill_rounded(cx, cy, CARD_W, CARD_H, 8.0, t.surface_flyout);
        p.stroke_rounded(cx + 0.5, cy + 0.5, CARD_W - 1.0, CARD_H - 1.0, 8.0, 1.0, t.outline);
        if let Some(f) = &title {
            p.text("Stack", f, cx + M, cy + M, 200.0, 28.0, t.text);
        }
        if let Some(f) = &caption_end {
            let n = items.len();
            let count = if n == 1 { "1 item".to_string() } else { format!("{n} items") };
            p.text(&count, f, cx + CARD_W - M - 120.0, cy + M, 120.0, 28.0, t.text_secondary);
        }

        for i in 0..items.len() + 1 {
            let Some((tx, ty)) = cell_pos(i, v.scroll) else { continue };
            let (tx, ty) = (cx + tx, cy + ty);
            if i == items.len() {
                // Drop tile.
                if v.drop_hover {
                    p.fill_rounded(tx, ty, TILE_W, TILE_H, 8.0, t.primary_subtle);
                }
                p.stroke_rounded_dashed(tx + 0.75, ty + 0.75, TILE_W - 1.5, TILE_H - 1.5, 8.0, 1.5, t.primary);
                if let Some(f) = &icon32 {
                    p.text(GLYPH_ADD, f, tx, ty + 10.0, TILE_W, 36.0, t.text_secondary);
                }
                if let Some(f) = &caption {
                    p.text("Drop here", f, tx, ty + 52.0, TILE_W, 20.0, t.text_secondary);
                }
                continue;
            }
            let item = &items[i];
            let hovered = v.hover == Some(i);
            p.rect_shadow(tx, ty + 1.0, TILE_W, TILE_H, 8.0, 2.0, 0.4 * t.shadow);
            p.fill_rounded(tx, ty, TILE_W, TILE_H, 8.0, if hovered { t.surface_hover } else { t.surface });
            if v.sel.contains(item.id) {
                p.fill_rounded(tx, ty, TILE_W, TILE_H, 8.0, t.primary_subtle);
            }
            if v.focus == i {
                p.stroke_rounded(tx + 0.5, ty + 0.5, TILE_W - 1.0, TILE_H - 1.0, 8.0, 1.0, t.primary);
            }
            match item.icon.as_ref().and_then(|i| i.as_ref()) {
                Some(icon) => {
                    let scale = (32.0 / icon.w.max(icon.h) as f32).min(1.0);
                    let (iw, ih) = (icon.w as f32 * scale, icon.h as f32 * scale);
                    p.image(&icon.bgra, icon.w, icon.h, tx + (TILE_W - iw) / 2.0, ty + 10.0 + (32.0 - ih) / 2.0, iw, ih);
                }
                None => {
                    let glyph = match &item.kind {
                        Kind::File(_) => GLYPH_MISSING,
                        Kind::Text(_) => GLYPH_TEXT,
                        Kind::Link(_) => GLYPH_LINK,
                    };
                    if let Some(f) = &icon32 {
                        p.text(glyph, f, tx, ty + 8.0, TILE_W, 36.0, t.text);
                    }
                }
            }
            if let Some(f) = &name_fmt {
                p.text(&shelf::name(&item.kind), f, tx + 6.0, ty + 46.0, TILE_W - 12.0, 20.0, t.text);
            }
            if let Some(f) = &caption {
                p.text(shelf::kind_label(&item.kind), f, tx + 6.0, ty + 64.0, TILE_W - 12.0, 16.0, t.text_secondary);
            }
            if hovered {
                let (rx, ry) = (tx + TILE_W - REMOVE - 4.0, ty + 4.0);
                let bg = if v.hover_remove { t.surface } else { t.surface_flyout };
                p.fill_circle(rx + REMOVE / 2.0, ry + REMOVE / 2.0, REMOVE / 2.0, bg);
                p.stroke_circle(rx + REMOVE / 2.0, ry + REMOVE / 2.0, REMOVE / 2.0 - 0.5, 1.0, t.outline_control);
                if let Some(f) = &icon12 {
                    p.text(GLYPH_REMOVE, f, rx, ry, REMOVE, REMOVE, t.text);
                }
            }
        }

        let total_rows = (items.len() + 1).div_ceil(COLS);
        if total_rows > ROWS {
            let thumb = (GRID_H * ROWS as f32 / total_rows as f32).max(16.0);
            let top = (GRID_H - thumb) * v.scroll as f32 / (total_rows - ROWS) as f32;
            p.fill_rounded(cx + CARD_W - 5.0, cy + GRID_Y + top, 2.0, thumb, 1.0, t.text_secondary);
        }
        p.fill_rect(cx, cy + FOOTER_Y - 4.0, CARD_W, 1.0, t.divider);
        if let Some(f) = &caption_lead {
            let hint = match v.sel.len() {
                n if n > 1 => format!("{n} selected     Drag out to use     Del remove     Esc close"),
                _ => "Drag out to use     Enter open     Del remove     Esc close".to_string(),
            };
            p.text(&hint, f, cx + M, cy + FOOTER_Y, CARD_W - M * 2.0, 16.0, t.text_secondary);
        }
    });
    let _ = surf.present(v.hwnd, v.origin.0, v.origin.1, 1.0);
}

pub fn open(app: &mut App, target: HWND) {
    app.stack_view.target = target;
    app.stack_view.open = true;
    app.stack_view.focus = 0;
    let ids = ids(app);
    app.stack_view.sel.only(&ids, 0);
    app.stack_view.scroll = 0;
    app.stack_view.hover = None;
    place(app);
    render(app);
    unsafe {
        let _ = ShowWindow(app.stack_view.hwnd, SW_SHOW);
        let _ = SetForegroundWindow(app.stack_view.hwnd);
    }
}

pub fn close(app: &mut App, restore: bool) {
    if !app.stack_view.open {
        return;
    }
    app.stack_view.open = false;
    app.stack_view.press = None;
    unsafe {
        let _ = ShowWindow(app.stack_view.hwnd, SW_HIDE);
        if restore && !app.stack_view.target.is_invalid() {
            let _ = SetForegroundWindow(app.stack_view.target);
        }
    }
}

/// Called after the stack changes from outside (a drop on the edge tab).
pub fn refresh(app: &mut App) {
    let n = cells(app);
    let ids = ids(app);
    let v = &mut app.stack_view;
    v.sel.retain(&ids);
    v.focus = v.focus.min(n.saturating_sub(2));
    let rows = n.div_ceil(COLS);
    v.scroll = v.scroll.min(rows.saturating_sub(ROWS));
    render(app);
}

fn keep_visible(v: &mut StackView) {
    let row = v.focus / COLS;
    if row < v.scroll {
        v.scroll = row;
    } else if row >= v.scroll + ROWS {
        v.scroll = row + 1 - ROWS;
    }
}

/// Files and links open in their default app; text is pasted into the app
/// the stack was opened from.
fn activate(app: &mut App, index: usize) {
    enum Act {
        Missing,
        Open(String),
        Paste(String),
    }
    let act = match app.stack.items.get(index).map(|i| &i.kind) {
        None => return,
        Some(Kind::File(p)) if !p.exists() => Act::Missing,
        Some(Kind::File(p)) => Act::Open(p.to_string_lossy().into_owned()),
        Some(Kind::Link(l)) if l.starts_with("www.") => Act::Open(format!("https://{l}")),
        Some(Kind::Link(l)) => Act::Open(l.clone()),
        Some(Kind::Text(t)) => Act::Paste(t.clone()),
    };
    match act {
        Act::Missing => crate::notify(app, "File not found", "It was moved or deleted. Press Del to remove it from the stack."),
        Act::Open(target) => {
            close(app, false);
            unsafe {
                ShellExecuteW(None, w!("open"), &HSTRING::from(target), None, None, SW_SHOWNORMAL);
            }
        }
        Act::Paste(text) => {
            if clipboard::write_text(app.host, &text) {
                close(app, true);
                if !app.stack_view.target.is_invalid() {
                    crate::schedule_paste(app);
                }
            }
        }
    }
}

fn remove(app: &mut App, gone: &[u64]) {
    if gone.is_empty() {
        return;
    }
    for &id in gone {
        app.stack.remove(id);
    }
    app.stack.save();
    app.stack_view.hover = None;
    refresh(app);
}

/// Del removes the selection, or the focused tile if nothing is selected,
/// then selects whatever moved into the focused spot.
fn remove_selected(app: &mut App) {
    let mut gone = app.stack_view.sel.ordered(&ids(app));
    if gone.is_empty() {
        gone.extend(app.stack.items.get(app.stack_view.focus).map(|i| i.id));
    }
    remove(app, &gone);
    let (ids, focus) = (ids(app), app.stack_view.focus);
    app.stack_view.sel.only(&ids, focus);
    render(app);
}

fn hit(app: &App, lparam: LPARAM) -> Option<(usize, bool)> {
    let v = &app.stack_view;
    let s = v.dpi as f32 / 96.0;
    let x = (lparam.0 & 0xffff) as i16 as f32 / s - PAD;
    let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32 / s - PAD;
    (0..cells(app)).find_map(|i| {
        let (tx, ty) = cell_pos(i, v.scroll)?;
        if x < tx || x >= tx + TILE_W || y < ty || y >= ty + TILE_H {
            return None;
        }
        let on_remove = i < app.stack.items.len() && x >= tx + TILE_W - REMOVE - 4.0 && y < ty + REMOVE + 4.0;
        Some((i, on_remove))
    })
}

fn key_down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(vk.0 as i32) < 0 }
}

/// Arrow keys move the focus and select it; with Shift they extend the
/// selection from the anchor.
fn move_focus(app: &mut App, delta: isize) {
    let n = app.stack.items.len();
    if n == 0 {
        return;
    }
    let ids = ids(app);
    let v = &mut app.stack_view;
    v.focus = (v.focus as isize + delta).clamp(0, n as isize - 1) as usize;
    if key_down(VK_SHIFT) {
        v.sel.extend(&ids, v.focus);
    } else {
        v.sel.only(&ids, v.focus);
    }
    keep_visible(v);
    render(app);
}

/// Pressing on a selected tile drags the whole selection; pressing on any
/// other tile selects it alone and drags just that one.
fn start_drag(app: &mut App, index: usize) {
    app.stack_view.press = None;
    let ids = ids(app);
    let Some(&id) = ids.get(index) else { return };
    if !app.stack_view.sel.contains(id) {
        app.stack_view.sel.only(&ids, index);
        app.stack_view.focus = index;
        render(app);
    }
    let chosen = app.stack_view.sel.ordered(&ids);
    let kinds: Vec<&Kind> = app.stack.items.iter().filter(|i| chosen.contains(&i.id)).map(|i| &i.kind).collect();
    let Some(data) = dnd::data_object_many(&kinds) else { return };
    // Messages keep flowing during the drag; hold no app state across it.
    dnd::drag_out(&data);
}

pub fn handle(app: &mut App, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    if !app.stack_view.open {
        return None;
    }
    match msg {
        WM_KEYDOWN => match VIRTUAL_KEY(wparam.0 as u16) {
            VK_ESCAPE => close(app, true),
            VK_RIGHT => move_focus(app, 1),
            VK_LEFT => move_focus(app, -1),
            VK_DOWN => move_focus(app, COLS as isize),
            VK_UP => move_focus(app, -(COLS as isize)),
            VK_RETURN => activate(app, app.stack_view.focus),
            VK_DELETE => remove_selected(app),
            VK_A if key_down(VK_CONTROL) => {
                let ids = ids(app);
                app.stack_view.sel.all(&ids);
                render(app);
            }
            _ => {}
        },
        WM_MOUSEMOVE => {
            if !app.stack_view.tracked {
                app.stack_view.tracked = true;
                let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                unsafe {
                    let _ = TrackMouseEvent(&mut tme);
                }
            }
            if let Some((index, from)) = app.stack_view.press {
                let mut now = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut now);
                }
                let (tx, ty) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
                if (now.x - from.x).abs() > tx || (now.y - from.y).abs() > ty {
                    start_drag(app, index);
                    return Some(LRESULT(0));
                }
            }
            let h = hit(app, lparam).filter(|&(i, _)| i < app.stack.items.len());
            let (hover, rm) = (h.map(|(i, _)| i), h.is_some_and(|(_, r)| r));
            if hover != app.stack_view.hover || rm != app.stack_view.hover_remove {
                app.stack_view.hover = hover;
                app.stack_view.hover_remove = rm;
                render(app);
            }
        }
        WM_MOUSELEAVE => {
            app.stack_view.tracked = false;
            app.stack_view.hover = None;
            render(app);
        }
        WM_LBUTTONDOWN => {
            if let Some((i, false)) = hit(app, lparam).filter(|&(i, _)| i < app.stack.items.len()) {
                let mut pt = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut pt);
                }
                app.stack_view.press = Some((i, pt));
            }
        }
        WM_LBUTTONUP => {
            app.stack_view.press = None;
            match hit(app, lparam) {
                Some((i, true)) => {
                    let id = app.stack.items[i].id;
                    remove(app, &[id]);
                }
                Some((i, false)) if i < app.stack.items.len() => {
                    let (ctrl, shift) = (wparam.0 as u32 & MK_CONTROL.0 != 0, wparam.0 as u32 & MK_SHIFT.0 != 0);
                    let double = !ctrl && !shift && app.stack_view.last_click.is_some_and(|(j, at)| {
                        j == i && at.elapsed().as_millis() <= unsafe { GetDoubleClickTime() } as u128
                    });
                    let ids = ids(app);
                    let v = &mut app.stack_view;
                    v.focus = i;
                    match (ctrl, shift) {
                        (_, true) => v.sel.extend(&ids, i),
                        (true, false) => v.sel.toggle(&ids, i),
                        _ => v.sel.only(&ids, i),
                    }
                    v.last_click = Some((i, Instant::now()));
                    if double {
                        app.stack_view.last_click = None;
                        activate(app, i);
                    } else {
                        render(app);
                    }
                }
                _ => {}
            }
        }
        WM_MOUSEWHEEL => {
            let delta = (wparam.0 >> 16) as i16 as isize;
            let rows = cells(app).div_ceil(COLS);
            let v = &mut app.stack_view;
            v.scroll = (v.scroll as isize - delta.signum()).clamp(0, rows.saturating_sub(ROWS) as isize) as usize;
            v.hover = None;
            render(app);
        }
        WM_ACTIVATE => {
            if (wparam.0 & 0xffff) as u32 == WA_INACTIVE && app.stack_view.press.is_none() {
                close(app, false);
            }
        }
        _ => return None,
    }
    Some(LRESULT(0))
}
