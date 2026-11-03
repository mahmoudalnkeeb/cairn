//! Peek: Space on a tile shows the item larger, in a card beside the stack,
//! without opening another app. Space or Esc closes it; the arrow keys move
//! the selection and the peek follows.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::detect::{self, Form};
use crate::gfx::{Color, Surface, Tween};
use crate::settings::Edge;
use crate::shelf::{self, Kind, Thumb};
use crate::{App, edgetab, stackview};

const W: f32 = 480.0;
const H: f32 = 360.0;
/// Room around the card for its shadow.
const PAD: f32 = 32.0;
const M: f32 = 16.0;
const GAP: f32 = 8.0;
/// Thumbnails are asked for at this size, in pixels.
const THUMB_PX: i32 = 512;
const BIG_SWATCH: f32 = 96.0;

pub struct Peek {
    pub hwnd: HWND,
    pub open: bool,
    /// 0 hidden, 1 shown.
    pub show: Tween,
    surface: Option<Surface>,
    dpi: u32,
    origin: (i32, i32),
    /// The large thumbnail, and the item it belongs to.
    thumb: Option<(u64, Option<Thumb>)>,
}

impl Peek {
    pub fn new(hwnd: HWND) -> Peek {
        Peek { hwnd, open: false, show: Tween::new(0.0), surface: None, dpi: 96, origin: (0, 0), thumb: None }
    }
}

pub fn toggle(app: &mut App) {
    if app.peek.open { close(app) } else { open(app) }
}

fn open(app: &mut App) {
    if app.stack.items.is_empty() {
        return;
    }
    app.peek.open = true;
    let d = stackview::ms(app, 140.0);
    app.peek.show.set(1.0, d);
    render(app);
    unsafe {
        let _ = ShowWindow(app.peek.hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(app.peek.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
    stackview::animate(app);
}

pub fn close(app: &mut App) {
    if !app.peek.open {
        return;
    }
    app.peek.open = false;
    app.peek.thumb = None;
    let d = stackview::ms(app, 120.0);
    app.peek.show.set(0.0, d);
    stackview::animate(app);
}

/// Hides the window once a close has faded out.
pub fn tick(app: &mut App) {
    if !app.peek.open && app.peek.show.done() {
        unsafe {
            let _ = ShowWindow(app.peek.hwnd, SW_HIDE);
        }
    } else {
        render(app);
    }
}

/// Beside the stack, on the side away from the screen edge, top-aligned
/// with it and kept on the work area.
fn place(app: &mut App) {
    let (card, dpi) = stackview::card_rect(app);
    let wa = edgetab::anchor(app).1;
    let s = dpi as f32 / 96.0;
    let (w, h, pad, gap) = ((W * s) as i32, (H * s) as i32, (PAD * s) as i32, (GAP * s) as i32);
    let x = match app.settings.edge {
        Edge::Right => card.left - gap - w,
        Edge::Left => card.right + gap,
    };
    let x = x.clamp(wa.left, (wa.right - w).max(wa.left));
    let y = card.top.clamp(wa.top, (wa.bottom - h).max(wa.top));
    let p = &mut app.peek;
    p.dpi = dpi;
    p.origin = (x - pad, y - pad);
    let (pw, ph) = (((W + PAD * 2.0) * s).ceil() as i32, ((H + PAD * 2.0) * s).ceil() as i32);
    if p.surface.as_ref().is_none_or(|surf| surf.width != pw || surf.height != ph) {
        p.surface = Surface::new(&app.gfx, pw, ph).ok();
    }
}

pub fn render(app: &mut App) {
    if !app.peek.open && app.peek.show.done() {
        return;
    }
    let index = stackview::focus_index(app);
    let Some(id) = app.stack.items.get(index).map(|i| i.id) else {
        close(app);
        return;
    };
    // A file's large thumbnail, loaded once per item.
    if app.peek.thumb.as_ref().is_none_or(|(t, _)| *t != id) {
        let thumb = match &app.stack.items[index].kind {
            Kind::File(p) if p.exists() => shelf::shell_image(p, THUMB_PX),
            _ => None,
        };
        app.peek.thumb = Some((id, thumb));
    }
    place(app);
    let item = &app.stack.items[index];
    let (g, t, p) = (&app.gfx, &app.theme, &app.peek);
    let Some(surf) = &p.surface else { return };
    let lead = |family, size, weight| {
        let f = g.text_format_aligned(family, size, weight, DWRITE_TEXT_ALIGNMENT_LEADING).ok()?;
        unsafe {
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR).ok()?;
        }
        Some(f)
    };
    let wrapped = |family, size| {
        let f = lead(family, size, DWRITE_FONT_WEIGHT_NORMAL)?;
        unsafe {
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP).ok()?;
        }
        Some(f)
    };
    let title = lead(&g.ui_font, 14.0, DWRITE_FONT_WEIGHT_SEMI_BOLD);
    let mono = lead(&g.code_font, 12.0, DWRITE_FONT_WEIGHT_NORMAL);
    let form = shelf::form(item);
    let sub = match &item.kind {
        Kind::File(p) => p.to_string_lossy().into_owned(),
        Kind::Link(l) => l.clone(),
        Kind::Text(_) => shelf::kind_label(item).to_string(),
    };
    let (bx, by, bw, bh) = (PAD + M, PAD + M + 48.0, W - M * 2.0, H - M * 3.0 - 48.0);
    let _ = surf.draw(p.dpi, |pt| {
        pt.rect_shadow(PAD, PAD + 8.0, W, H, 8.0, 24.0, 0.55 * t.shadow);
        pt.fill_rounded(PAD, PAD, W, H, 8.0, t.surface_flyout);
        pt.stroke_rounded(PAD + 0.5, PAD + 0.5, W - 1.0, H - 1.0, 8.0, 1.0, t.outline);
        if let Some(f) = &title {
            pt.text(&shelf::label(item), f, PAD + M, PAD + M, W - M * 2.0, 20.0, t.text);
        }
        if let Some(f) = &mono {
            pt.text(&sub, f, PAD + M, PAD + M + 22.0, W - M * 2.0, 16.0, t.text_secondary);
        }
        pt.fill_rect(PAD, by - 8.0, W, 1.0, t.divider);
        match (&item.kind, form) {
            (Kind::File(_), _) => match p.thumb.as_ref().and_then(|(_, th)| th.as_ref()) {
                Some(th) => {
                    let scale = (bw / th.w as f32).min(bh / th.h as f32).min(1.0);
                    let (iw, ih) = (th.w as f32 * scale, th.h as f32 * scale);
                    pt.image(&th.bgra, th.w, th.h, bx + (bw - iw) / 2.0, by + (bh - ih) / 2.0, iw, ih);
                }
                None => {
                    if let Some(f) = &wrapped(&g.ui_font, 14.0) {
                        pt.text("File not found. It was moved or deleted. Right-click it to find it.", f, bx, by, bw, bh, t.text_secondary);
                    }
                }
            },
            (_, Some(Form::Color(c))) => {
                let [r, gr, b, a] = c;
                let color = Color(r as f32 / 255.0, gr as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0);
                pt.fill_rounded(bx, by, BIG_SWATCH, BIG_SWATCH, 4.0, color);
                pt.stroke_rounded(bx + 0.5, by + 0.5, BIG_SWATCH - 1.0, BIG_SWATCH - 1.0, 4.0, 1.0, t.outline);
                if let Some(f) = &lead(&g.code_font, 13.0, DWRITE_FONT_WEIGHT_NORMAL) {
                    for (i, line) in [detect::hex(c), detect::rgb(c), detect::hsl(c)].iter().enumerate() {
                        pt.text(line, f, bx + BIG_SWATCH + M, by + 8.0 + i as f32 * 28.0, bw - BIG_SWATCH - M, 20.0, t.text);
                    }
                }
            }
            (Kind::Text(text), _) => {
                let family = if form == Some(Form::Code) { &g.code_font } else { &g.ui_font };
                if let Some(f) = &wrapped(family, if form == Some(Form::Code) { 13.0 } else { 14.0 }) {
                    // A long text shows its start; the rest ends in an ellipsis.
                    let head: String = text.chars().take(4000).collect();
                    pt.text(&head, f, bx, by, bw, bh, t.text);
                }
            }
            (Kind::Link(url), _) => {
                if let Some(f) = &wrapped(&g.code_font, 13.0) {
                    pt.text(url, f, bx, by, bw, bh, t.text);
                }
            }
        }
    });
    let shown = p.show.value();
    let _ = surf.present(p.hwnd, p.origin.0, p.origin.1, shown);
}
