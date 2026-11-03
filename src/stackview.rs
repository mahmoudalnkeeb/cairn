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
use crate::{App, a11y, actions, clipboard, dnd, dpi_at, edgetab};

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
    /// Opened by a drag resting on the edge tab; closes on its own.
    pub dragging_in: bool,
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
    /// Stack tabs in the header as (x, width) in card DIPs, from the last
    /// render, and the x of the "+" button.
    heads: Vec<(f32, f32)>,
    add_x: f32,
    last_head_click: Option<(usize, Instant)>,
    /// The current stack's name while it is being renamed. `fresh` means the
    /// first key typed replaces it.
    editing: Option<(String, bool)>,
}

#[derive(Clone, Copy, PartialEq)]
enum Head {
    Stack(usize),
    Add,
}

const HEAD_PAD: f32 = 10.0;
const HEAD_GAP: f32 = 4.0;
const ADD_W: f32 = 28.0;
const COUNT_W: f32 = 64.0;

// Tile menu.
const A_OPEN: u32 = 1;
const A_COPY_PATHS: u32 = 2;
const A_COPY_MD: u32 = 3;
const A_ZIP: u32 = 4;
const A_COPY_TO: u32 = 5;
const A_REMOVE: u32 = 6;
const A_MOVE_BASE: u32 = 100;
// Stack tab menu.
const H_RENAME: u32 = 1;
const H_DELETE: u32 = 2;
const H_NEW: u32 = 3;

impl StackView {
    pub fn new(hwnd: HWND) -> StackView {
        StackView {
            hwnd,
            open: false,
            drop_hover: false,
            dragging_in: false,
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
            heads: Vec::new(),
            add_x: 0.0,
            last_head_click: None,
            editing: None,
        }
    }
}

/// Grid cells: one per item.
fn cells(app: &App) -> usize {
    app.stack.items.len()
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

/// A stack tab's text: its name, or the name being typed with a caret.
fn head_label(app: &App, i: usize, name: &str) -> String {
    match &app.stack_view.editing {
        Some((text, _)) if i == app.stack.current => format!("{text}|"),
        _ => name.to_string(),
    }
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

    // Lay out the stack tabs: each name in a pill, then "+", leaving room
    // for the item count. Tabs that do not fit are reached with Ctrl+Tab.
    let head_fmt = app.gfx.text_format_aligned(&app.gfx.ui_font, 14.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER).ok();
    // A tab that does not fit gets zero width, so indexes match names.
    let mut heads = Vec::new();
    let mut x = M;
    let room = CARD_W - M - COUNT_W - ADD_W - HEAD_GAP;
    for (i, name) in app.stack.names.iter().enumerate() {
        let w = head_fmt.as_ref().map_or(60.0, |f| app.gfx.text_width(&head_label(app, i, name), f)).min(160.0) + HEAD_PAD * 2.0;
        if x + w > room && i != app.stack.current {
            heads.push((0.0, 0.0));
            continue;
        }
        heads.push((x, w));
        x += w + HEAD_GAP;
    }
    app.stack_view.heads = heads;
    app.stack_view.add_x = x;

    let v = &app.stack_view;
    let Some(surf) = &v.surface else { return };
    let (g, t) = (&app.gfx, &app.theme);
    let fmt = |family, size, weight, align| g.text_format_aligned(family, size, weight, align).ok();
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
        if v.drop_hover {
            // The whole stack is the drop target. An opaque base keeps the
            // window behind from showing through the tint.
            p.fill_rounded(cx, cy, CARD_W, CARD_H, 8.0, t.surface);
            p.fill_rounded(cx, cy, CARD_W, CARD_H, 8.0, t.primary_subtle);
            p.stroke_rounded_dashed(cx + 0.75, cy + 0.75, CARD_W - 1.5, CARD_H - 1.5, 8.0, 1.5, t.primary);
        } else {
            p.stroke_rounded(cx + 0.5, cy + 0.5, CARD_W - 1.0, CARD_H - 1.0, 8.0, 1.0, t.outline);
        }
        if items.is_empty() {
            if let Some(f) = &name_fmt {
                p.text("Nothing here yet", f, cx, cy + GRID_Y + GRID_H / 2.0 - 22.0, CARD_W, 20.0, t.text);
            }
            if let Some(f) = &caption {
                p.text("Drag files, text or links onto the edge tab or here", f, cx, cy + GRID_Y + GRID_H / 2.0, CARD_W, 16.0, t.text_secondary);
            }
        }
        for (i, &(hx, hw)) in v.heads.iter().enumerate().filter(|(_, h)| h.1 > 0.0) {
            let current = i == app.stack.current;
            if current {
                p.fill_rounded(cx + hx, cy + M, hw, 28.0, 6.0, t.surface);
            }
            if current && v.editing.is_some() {
                p.stroke_rounded(cx + hx + 0.5, cy + M + 0.5, hw - 1.0, 27.0, 6.0, 1.0, t.primary);
            }
            if let Some(f) = &head_fmt {
                let color = if current { t.text } else { t.text_secondary };
                p.text(&head_label(app, i, &app.stack.names[i]), f, cx + hx + HEAD_PAD, cy + M + 4.0, hw - HEAD_PAD * 2.0, 20.0, color);
            }
        }
        if let Some(f) = &icon12 {
            p.text(GLYPH_ADD, f, cx + v.add_x, cy + M + 8.0, ADD_W, 14.0, t.text_secondary);
        }
        if let Some(f) = &caption_end {
            let n = items.len();
            let count = if n == 1 { "1 item".to_string() } else { format!("{n} items") };
            p.text(&count, f, cx + CARD_W - M - COUNT_W, cy + M, COUNT_W, 28.0, t.text_secondary);
        }

        for i in 0..items.len() {
            let Some((tx, ty)) = cell_pos(i, v.scroll) else { continue };
            let (tx, ty) = (cx + tx, cy + ty);
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

        let total_rows = items.len().div_ceil(COLS);
        if total_rows > ROWS {
            let thumb = (GRID_H * ROWS as f32 / total_rows as f32).max(16.0);
            let top = (GRID_H - thumb) * v.scroll as f32 / (total_rows - ROWS) as f32;
            p.fill_rounded(cx + CARD_W - 5.0, cy + GRID_Y + top, 2.0, thumb, 1.0, t.text_secondary);
        }
        p.fill_rect(cx, cy + FOOTER_Y - 4.0, CARD_W, 1.0, t.divider);
        if let Some(f) = &caption_lead {
            let hint = match v.sel.len() {
                _ if v.drop_hover && app.stack.names.len() > 1 => format!("Drop to add to {}     Point at another name to switch", app.stack.names[app.stack.current]),
                _ if v.drop_hover => format!("Drop to add to {}", app.stack.names[app.stack.current]),
                _ if v.dragging_in => "Move onto the stack to drop here".to_string(),
                n if n > 1 => format!("{n} selected     Drag out to use     Del remove     Right-click for more"),
                _ => "Drag out to use     Enter open     Del remove     Right-click for more".to_string(),
            };
            p.text(&hint, f, cx + M, cy + FOOTER_Y, CARD_W - M * 2.0, 16.0, t.text_secondary);
        }
    });
    let _ = surf.present(v.hwnd, v.origin.0, v.origin.1, 1.0);
}

pub fn open(app: &mut App, target: HWND) {
    app.stack_view.target = target;
    app.stack_view.editing = None;
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
    announce(app);
}

/// Back to the first tile, selected, after the stack's contents change.
fn reset_view(app: &mut App) {
    let ids = ids(app);
    let v = &mut app.stack_view;
    v.focus = 0;
    v.sel.only(&ids, 0);
    v.scroll = 0;
    v.hover = None;
}

pub fn switch_stack(app: &mut App, stack: usize) {
    if stack == app.stack.current || stack >= app.stack.names.len() {
        return;
    }
    finish_rename(app, true);
    app.stack.switch(stack);
    app.stack.save();
    reset_view(app);
    render(app);
    announce(app);
}

fn cycle_stack(app: &mut App, delta: isize) {
    let n = app.stack.names.len() as isize;
    let next = (app.stack.current as isize + delta).rem_euclid(n) as usize;
    switch_stack(app, next);
}

fn new_stack(app: &mut App) {
    finish_rename(app, true);
    app.stack.new_stack();
    app.stack.save();
    reset_view(app);
    start_rename(app);
}

fn start_rename(app: &mut App) {
    let name = app.stack.names[app.stack.current].clone();
    app.stack_view.editing = Some((name, true));
    render(app);
}

fn finish_rename(app: &mut App, commit: bool) {
    if let Some((text, _)) = app.stack_view.editing.take() {
        if commit {
            app.stack.rename(app.stack.current, &text);
            app.stack.save();
        }
        render(app);
    }
}

/// Typing while a stack is being renamed. Returns false for keys it leaves
/// alone.
fn edit_key(app: &mut App, vk: VIRTUAL_KEY) -> bool {
    match vk {
        VK_RETURN => finish_rename(app, true),
        VK_ESCAPE => finish_rename(app, false),
        VK_BACK => {
            if let Some((text, fresh)) = &mut app.stack_view.editing {
                if *fresh {
                    text.clear();
                } else {
                    text.pop();
                }
                *fresh = false;
            }
            render(app);
        }
        _ => return false,
    }
    true
}

fn edit_char(app: &mut App, c: char) {
    if let Some((text, fresh)) = &mut app.stack_view.editing
        && !c.is_control()
    {
        if *fresh {
            text.clear();
            *fresh = false;
        }
        if text.chars().count() < 40 {
            text.push(c);
        }
        render(app);
    }
}

/// Deletes a stack after the user picked it from the menu. Its items can be
/// brought back for 10 seconds, into whichever stack is current then.
fn delete_stack(app: &mut App, stack: usize) {
    finish_rename(app, false);
    let name = app.stack.names[stack].clone();
    let Some(items) = app.stack.delete_stack(stack) else { return };
    app.stack.save();
    reset_view(app);
    render(app);
    announce(app);
    if !items.is_empty() {
        crate::offer_undo(app, items, &format!("Deleted \"{name}\""));
    }
}

/// The selected items, or the focused one if nothing is selected.
fn chosen(app: &App) -> Vec<u64> {
    let mut ids = app.stack_view.sel.ordered(&ids(app));
    if ids.is_empty() {
        ids.extend(app.stack.items.get(app.stack_view.focus).map(|i| i.id));
    }
    ids
}

fn popup(owner: HWND, menu: HMENU, at: POINT) -> u32 {
    unsafe {
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, at.x, at.y, 0, owner, None);
        let _ = DestroyMenu(menu);
        cmd.0 as u32
    }
}

/// The actions menu for the selection.
fn tile_menu(app: &mut App, at: POINT) {
    let ids = chosen(app);
    if ids.is_empty() {
        return;
    }
    // Copy what the actions need now: the menu and dialogs below can close
    // the flyout or change the stack.
    let (text_paths, text_md, files, refs, first_is_text) = {
        let kinds: Vec<&Kind> = app.stack.items.iter().filter(|i| ids.contains(&i.id)).map(|i| &i.kind).collect();
        let refs = kinds.iter().any(|k| !matches!(k, Kind::Text(_)));
        (actions::paths_text(&kinds), actions::markdown(&kinds), actions::existing_files(&kinds), refs, matches!(kinds[0], Kind::Text(_)))
    };
    let one = ids.len() == 1;
    let has_files = !files.is_empty();
    let menu = unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let on = |b: bool| if b { MF_ENABLED } else { MF_GRAYED };
        if one {
            let open = if first_is_text { w!("Paste") } else { w!("Open") };
            let _ = AppendMenuW(menu, MF_STRING, A_OPEN as usize, open);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        }
        let _ = AppendMenuW(menu, MF_STRING | on(refs), A_COPY_PATHS as usize, if one { w!("Copy path") } else { w!("Copy paths") });
        let _ = AppendMenuW(menu, MF_STRING | on(refs), A_COPY_MD as usize, w!("Copy as Markdown link"));
        let _ = AppendMenuW(menu, MF_STRING | on(has_files), A_ZIP as usize, w!("Save as zip…"));
        let _ = AppendMenuW(menu, MF_STRING | on(has_files), A_COPY_TO as usize, w!("Copy to folder…"));
        if app.stack.names.len() > 1
            && let Ok(stacks) = CreatePopupMenu()
        {
            for (i, name) in app.stack.names.iter().enumerate().filter(|&(i, _)| i != app.stack.current) {
                let _ = AppendMenuW(stacks, MF_STRING, A_MOVE_BASE as usize + i, &HSTRING::from(name.as_str()));
            }
            let _ = AppendMenuW(menu, MF_POPUP, stacks.0 as usize, w!("Move to stack"));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, A_REMOVE as usize, w!("Remove"));
        menu
    };
    match popup(app.stack_view.hwnd, menu, at) {
        A_OPEN => activate(app, app.stack.items.iter().position(|i| i.id == ids[0]).unwrap_or(0)),
        A_COPY_PATHS => copy_text(app, &text_paths, if one { "Path copied" } else { "Paths copied" }),
        A_COPY_MD => copy_text(app, &text_md, "Markdown copied"),
        A_ZIP => {
            close(app, false);
            if let Some(out) = actions::pick_zip(HWND::default(), &files[0]) {
                app.zip_out = Some(out.clone());
                actions::zip(files, out, app.host);
            }
        }
        A_COPY_TO => {
            close(app, false);
            if let Some(folder) = actions::pick_folder(HWND::default()) {
                actions::copy_to(HWND::default(), &files, &folder);
            }
        }
        A_REMOVE => remove_selected(app),
        c if c >= A_MOVE_BASE && ((c - A_MOVE_BASE) as usize) < app.stack.names.len() => {
            let dest = (c - A_MOVE_BASE) as usize;
            let moved = app.stack.move_to(&ids, dest);
            app.stack.save();
            let n = ids.len();
            refresh(app);
            reset_view(app);
            render(app);
            announce(app);
            let name = &app.stack.names[dest];
            let note = match (moved, n) {
                (1, 1) => format!("Moved to {name}"),
                (m, n) if m == n => format!("Moved {m} items to {name}"),
                (m, _) => format!("Moved {m} to {name}; the rest were already there"),
            };
            crate::notify(app, "Cairn", &note);
        }
        _ => {}
    }
}

fn copy_text(app: &mut App, text: &str, done: &str) {
    if clipboard::write_text(app.host, text) {
        crate::notify(app, done, &shelf::preview(text));
    }
}

/// The menu for a stack's tab.
fn head_menu(app: &mut App, stack: usize, at: POINT) {
    switch_stack(app, stack);
    let n = app.stack.count(stack);
    let menu = unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING, H_RENAME as usize, w!("Rename\tF2"));
        let _ = AppendMenuW(menu, MF_STRING, H_NEW as usize, w!("New stack\tCtrl+N"));
        let delete = match n {
            0 => "Delete stack".to_string(),
            1 => "Delete stack and its item".to_string(),
            n => format!("Delete stack and its {n} items"),
        };
        let last = if app.stack.names.len() < 2 { MF_GRAYED } else { MF_ENABLED };
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING | last, H_DELETE as usize, &HSTRING::from(delete));
        menu
    };
    match popup(app.stack_view.hwnd, menu, at) {
        H_RENAME => start_rename(app),
        H_NEW => new_stack(app),
        H_DELETE => delete_stack(app, stack),
        _ => {}
    }
}

fn hit_head(app: &App, lparam: LPARAM) -> Option<Head> {
    let s = app.stack_view.dpi as f32 / 96.0;
    let x = (lparam.0 & 0xffff) as i16 as f32 / s - PAD;
    let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32 / s - PAD;
    head_at(app, x, y)
}

/// The header control at a point in card DIPs.
fn head_at(app: &App, x: f32, y: f32) -> Option<Head> {
    let v = &app.stack_view;
    if !(M..M + 28.0).contains(&y) {
        return None;
    }
    if (v.add_x..v.add_x + ADD_W).contains(&x) {
        return Some(Head::Add);
    }
    v.heads.iter().position(|&(hx, hw)| hw > 0.0 && (hx..hx + hw).contains(&x)).map(Head::Stack)
}

/// Opens the stack while something is being dragged onto the edge tab,
/// without taking focus from the app the drag came from.
pub fn open_for_drag(app: &mut App) {
    if app.stack_view.open {
        return;
    }
    app.stack_view.target = unsafe { GetForegroundWindow() };
    app.stack_view.editing = None;
    app.stack_view.open = true;
    app.stack_view.dragging_in = true;
    reset_view(app);
    place(app);
    render(app);
    unsafe {
        let _ = ShowWindow(app.stack_view.hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(app.stack_view.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// A drag moving over the stack: pointing at another stack's name switches
/// to it, so the drop lands there.
pub fn drag_over(app: &mut App, pt: POINT) {
    let v = &app.stack_view;
    let s = v.dpi as f32 / 96.0;
    let (x, y) = ((pt.x - v.origin.0) as f32 / s - PAD, (pt.y - v.origin.1) as f32 / s - PAD);
    if let Some(Head::Stack(i)) = head_at(app, x, y) {
        switch_stack(app, i);
    }
}

/// The actions menu from the keyboard, at the focused tile.
fn focused_menu(app: &mut App) {
    let focus = app.stack_view.focus;
    if let Some(r) = tile_screen_rect(app, focus) {
        if !is_selected(app, focus) {
            take_focus(app, focus);
        }
        tile_menu(app, POINT { x: r.left + 16, y: r.bottom - 16 });
    }
}

fn cursor() -> POINT {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    pt
}

/// Tells screen readers which tile has the keyboard focus.
fn announce(app: &App) {
    let v = &app.stack_view;
    a11y::focus_changed(v.hwnd, (!app.stack.items.is_empty()).then_some(v.focus));
}

pub fn focus_index(app: &App) -> usize {
    app.stack_view.focus
}

pub fn is_selected(app: &App, index: usize) -> bool {
    app.stack.items.get(index).is_some_and(|i| app.stack_view.sel.contains(i.id))
}

/// A tile's rectangle in screen pixels, if it is scrolled into view.
pub fn tile_screen_rect(app: &App, index: usize) -> Option<RECT> {
    let v = &app.stack_view;
    let (tx, ty) = cell_pos(index, v.scroll)?;
    let s = v.dpi as f32 / 96.0;
    let (x, y) = (v.origin.0 + ((PAD + tx) * s) as i32, v.origin.1 + ((PAD + ty) * s) as i32);
    Some(RECT { left: x, top: y, right: x + (TILE_W * s) as i32, bottom: y + (TILE_H * s) as i32 })
}

/// The tile under a screen point.
pub fn hit_screen(app: &App, pt: POINT) -> Option<usize> {
    (0..app.stack.items.len()).find(|&i| {
        tile_screen_rect(app, i).is_some_and(|r| pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom)
    })
}

/// Moves the focus to a tile and selects only it, as a screen reader asks.
pub fn take_focus(app: &mut App, index: usize) {
    if index >= app.stack.items.len() {
        return;
    }
    let ids = ids(app);
    let v = &mut app.stack_view;
    v.focus = index;
    v.sel.only(&ids, index);
    keep_visible(v);
    render(app);
    announce(app);
}

pub fn close(app: &mut App, restore: bool) {
    if !app.stack_view.open {
        return;
    }
    // Clicking away keeps a name that was being typed.
    finish_rename(app, true);
    app.stack_view.open = false;
    app.stack_view.dragging_in = false;
    app.stack_view.drop_hover = false;
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
    v.focus = v.focus.min(n.saturating_sub(1));
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
pub fn activate(app: &mut App, index: usize) {
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
    announce(app);
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
    announce(app);
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
    if msg == WM_GETOBJECT {
        return a11y::get_object(hwnd, wparam, lparam);
    }
    if !app.stack_view.open {
        return None;
    }
    let vk = VIRTUAL_KEY(wparam.0 as u16);
    if msg == WM_KEYDOWN && app.stack_view.editing.is_some() && edit_key(app, vk) {
        return Some(LRESULT(0));
    }
    match msg {
        WM_CHAR => {
            if let Some(c) = char::from_u32(wparam.0 as u32) {
                edit_char(app, c);
            }
        }
        WM_KEYDOWN if app.stack_view.editing.is_some() => {}
        WM_KEYDOWN => match vk {
            VK_TAB if key_down(VK_CONTROL) => cycle_stack(app, if key_down(VK_SHIFT) { -1 } else { 1 }),
            VK_N if key_down(VK_CONTROL) => new_stack(app),
            VK_F2 => start_rename(app),
            VK_APPS => focused_menu(app),
            VK_F10 if key_down(VK_SHIFT) => focused_menu(app),
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
                announce(app);
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
        WM_RBUTTONUP => match (hit_head(app, lparam), hit(app, lparam)) {
            (Some(Head::Stack(i)), _) => head_menu(app, i, cursor()),
            (_, Some((i, _))) if i < app.stack.items.len() => {
                // Right-clicking outside the selection acts on that tile alone.
                if !is_selected(app, i) {
                    take_focus(app, i);
                }
                tile_menu(app, cursor());
            }
            _ => {}
        },
        WM_LBUTTONUP if hit_head(app, lparam).is_some() => {
            app.stack_view.press = None;
            match hit_head(app, lparam) {
                Some(Head::Add) => new_stack(app),
                Some(Head::Stack(i)) => {
                    let double = app.stack_view.last_head_click.is_some_and(|(j, at)| {
                        j == i && at.elapsed().as_millis() <= unsafe { GetDoubleClickTime() } as u128
                    });
                    app.stack_view.last_head_click = Some((i, Instant::now()));
                    if double && i == app.stack.current {
                        start_rename(app);
                    } else {
                        switch_stack(app, i);
                    }
                }
                None => {}
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
                        announce(app);
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
