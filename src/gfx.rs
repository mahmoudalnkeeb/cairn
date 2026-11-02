//! Direct2D/DirectWrite rendering into per-pixel-alpha layered windows,
//! plus the brand color tokens.

use std::ffi::c_void;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

#[derive(Clone, Copy)]
pub struct Color(pub f32, pub f32, pub f32, pub f32);

impl Color {
    pub const fn hex(rgb: u32, a: u8) -> Color {
        Color(
            ((rgb >> 16) & 0xff) as f32 / 255.0,
            ((rgb >> 8) & 0xff) as f32 / 255.0,
            (rgb & 0xff) as f32 / 255.0,
            a as f32 / 255.0,
        )
    }
    pub fn alpha(self, a: f32) -> Color {
        Color(self.0, self.1, self.2, self.3 * a)
    }
    fn d2d(self) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r: self.0, g: self.1, b: self.2, a: self.3 }
    }
}

/// Brand color tokens.
pub struct Theme {
    pub surface: Color,
    pub surface_hover: Color,
    pub surface_sunken: Color,
    pub divider: Color,
    pub surface_flyout: Color,
    pub outline: Color,
    pub outline_control: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub stone: Color,
    pub primary: Color,
    pub primary_subtle: Color,
    pub focus_ring: Color,
    pub shadow: f32,
}

impl Theme {
    pub fn dark() -> Theme {
        Theme {
            surface: Color::hex(0x1b1e22, 0xff),
            surface_hover: Color::hex(0x23272c, 0xff),
            surface_sunken: Color::hex(0x0d0f11, 0xff),
            divider: Color::hex(0x2a2f34, 0xff),
            surface_flyout: Color::hex(0x181b1f, 0xf2),
            outline: Color::hex(0x30353b, 0xff),
            outline_control: Color::hex(0x767d79, 0xff),
            text: Color::hex(0xeef0ec, 0xff),
            text_secondary: Color::hex(0x9ea4a0, 0xff),
            stone: Color::hex(0xb9b4aa, 0xff),
            primary: Color::hex(0xc6e36b, 0xff),
            primary_subtle: Color::hex(0xc6e36b, 0x26),
            focus_ring: Color::hex(0xeef0ec, 0xff),
            shadow: 1.0,
        }
    }

    pub fn light() -> Theme {
        Theme {
            surface: Color::hex(0xffffff, 0xff),
            surface_hover: Color::hex(0xefece6, 0xff),
            surface_sunken: Color::hex(0xebe8e2, 0xff),
            divider: Color::hex(0xddd8d1, 0xff),
            surface_flyout: Color::hex(0xffffff, 0xf2),
            outline: Color::hex(0xd4cec6, 0xff),
            outline_control: Color::hex(0x767676, 0xff),
            text: Color::hex(0x191b1d, 0xff),
            text_secondary: Color::hex(0x5a605c, 0xff),
            stone: Color::hex(0x6e675c, 0xff),
            primary: Color::hex(0x4a6410, 0xff),
            primary_subtle: Color::hex(0x4a6410, 0x24),
            focus_ring: Color::hex(0x191b1d, 0xff),
            shadow: 0.33,
        }
    }

    /// Light when the system app theme is light, dark otherwise.
    pub fn system() -> Theme {
        if personalize_flag(w!("AppsUseLightTheme")) { Theme::light() } else { Theme::dark() }
    }
}

/// True when the taskbar (and tray) is light, so the tray icon must be dark.
pub fn taskbar_is_light() -> bool {
    personalize_flag(w!("SystemUsesLightTheme"))
}

fn personalize_flag(name: PCWSTR) -> bool {
    let mut value: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            name,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    err == ERROR_SUCCESS && value == 1
}

pub struct Gfx {
    pub d2d: ID2D1Factory,
    pub dwrite: IDWriteFactory,
    pub ui_font: HSTRING,
    pub display_font: HSTRING,
    pub icon_font: HSTRING,
    pub code_font: HSTRING,
}

impl Gfx {
    pub fn new() -> Result<Gfx> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let mut fonts: Option<IDWriteFontCollection> = None;
            dwrite.GetSystemFontCollection(&mut fonts, false)?;
            let fonts = fonts.unwrap();
            let pick = |names: &[&str]| -> HSTRING {
                for name in names {
                    let h = HSTRING::from(*name);
                    let (mut index, mut exists) = (0u32, BOOL(0));
                    if fonts.FindFamilyName(&h, &mut index, &mut exists).is_ok() && exists.as_bool() {
                        return h;
                    }
                }
                HSTRING::from(*names.last().unwrap())
            };
            Ok(Gfx {
                ui_font: pick(&["Segoe UI Variable Text", "Segoe UI"]),
                display_font: pick(&["Segoe UI Variable Display", "Segoe UI"]),
                icon_font: pick(&["Segoe Fluent Icons", "Segoe MDL2 Assets"]),
                code_font: pick(&["Cascadia Mono", "Consolas"]),
                d2d,
                dwrite,
            })
        }
    }

    /// A one-line format aligned to the leading or trailing edge that
    /// ends overflowing text with an ellipsis.
    pub fn text_format_aligned(
        &self,
        family: &HSTRING,
        size: f32,
        weight: DWRITE_FONT_WEIGHT,
        align: DWRITE_TEXT_ALIGNMENT,
    ) -> Result<IDWriteTextFormat> {
        let f = self.text_format(family, size, weight)?;
        unsafe {
            f.SetTextAlignment(align)?;
            let sign = self.dwrite.CreateEllipsisTrimmingSign(&f)?;
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            f.SetTrimming(&trimming, &sign)?;
        }
        Ok(f)
    }

    pub fn text_format(&self, family: &HSTRING, size: f32, weight: DWRITE_FONT_WEIGHT) -> Result<IDWriteTextFormat> {
        unsafe {
            let f = self.dwrite.CreateTextFormat(
                family,
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("en-us"),
            )?;
            f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            Ok(f)
        }
    }

    /// Width in DIPs of a single line of text.
    pub fn text_width(&self, text: &str, format: &IDWriteTextFormat) -> f32 {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let Ok(layout) = self.dwrite.CreateTextLayout(&wide, format, 10_000.0, 100.0) else { return 0.0 };
            let mut m = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut m).is_ok() { m.width } else { 0.0 }
        }
    }
}

/// A 32bpp premultiplied DIB that Direct2D draws into and
/// `UpdateLayeredWindow` presents.
pub struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    pub width: i32,
    pub height: i32,
    target: ID2D1DCRenderTarget,
}

impl Surface {
    pub fn new(gfx: &Gfx, width: i32, height: i32) -> Result<Surface> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let old = SelectObject(dc, bitmap);
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                ..Default::default()
            };
            let target = gfx.d2d.CreateDCRenderTarget(&props)?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            Ok(Surface { dc, bitmap, old, width, height, target })
        }
    }

    /// Draws with coordinates in DIPs at the given DPI.
    pub fn draw(&self, dpi: u32, f: impl FnOnce(&Painter)) -> Result<()> {
        unsafe {
            let rect = RECT { left: 0, top: 0, right: self.width, bottom: self.height };
            self.target.BindDC(self.dc, &rect)?;
            self.target.SetDpi(dpi as f32, dpi as f32);
            self.target.BeginDraw();
            self.target.Clear(Some(&Color(0.0, 0.0, 0.0, 0.0).d2d()));
            f(&Painter { rt: &self.target });
            self.target.EndDraw(None, None)
        }
    }

    pub fn present(&self, hwnd: HWND, x: i32, y: i32, opacity: f32) -> Result<()> {
        unsafe {
            let screen = GetDC(None);
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: (opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let r = UpdateLayeredWindow(
                hwnd,
                screen,
                Some(&POINT { x, y }),
                Some(&SIZE { cx: self.width, cy: self.height }),
                self.dc,
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            ReleaseDC(None, screen);
            r
        }
    }

    /// Builds an icon (for the tray) from the current pixels.
    pub fn to_icon(&self) -> Result<HICON> {
        unsafe {
            let mask = CreateBitmap(self.width, self.height, 1, 1, None);
            let info = ICONINFO { fIcon: TRUE, hbmColor: self.bitmap, hbmMask: mask, ..Default::default() };
            let icon = CreateIconIndirect(&info);
            let _ = DeleteObject(mask);
            icon
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.dc);
        }
    }
}

pub struct Painter<'a> {
    rt: &'a ID2D1RenderTarget,
}

fn ellipse(x: f32, y: f32, r: f32) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE { point: D2D_POINT_2F { x, y }, radiusX: r, radiusY: r }
}

impl Painter<'_> {
    fn brush(&self, c: Color) -> Option<ID2D1SolidColorBrush> {
        unsafe { self.rt.CreateSolidColorBrush(&c.d2d(), None).ok() }
    }

    pub fn fill_circle(&self, x: f32, y: f32, r: f32, c: Color) {
        if let Some(b) = self.brush(c) {
            unsafe { self.rt.FillEllipse(&ellipse(x, y, r), &b) }
        }
    }

    pub fn stroke_circle(&self, x: f32, y: f32, r: f32, width: f32, c: Color) {
        if let Some(b) = self.brush(c) {
            unsafe { self.rt.DrawEllipse(&ellipse(x, y, r), &b, width, None) }
        }
    }

    fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> D2D1_ROUNDED_RECT {
        D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h },
            radiusX: r,
            radiusY: r,
        }
    }

    pub fn fill_rounded(&self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
        if let Some(b) = self.brush(c) {
            unsafe { self.rt.FillRoundedRectangle(&Self::rounded(x, y, w, h, r), &b) }
        }
    }

    pub fn stroke_rounded(&self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, c: Color) {
        if let Some(b) = self.brush(c) {
            unsafe { self.rt.DrawRoundedRectangle(&Self::rounded(x, y, w, h, r), &b, width, None) }
        }
    }

    pub fn stroke_rounded_dashed(&self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, c: Color) {
        let props = D2D1_STROKE_STYLE_PROPERTIES {
            dashStyle: D2D1_DASH_STYLE_CUSTOM,
            dashCap: D2D1_CAP_STYLE_FLAT,
            ..Default::default()
        };
        unsafe {
            let Ok(factory) = self.rt.GetFactory() else { return };
            let Ok(style) = factory.CreateStrokeStyle(&props, Some(&[3.0, 2.0])) else { return };
            if let Some(b) = self.brush(c) {
                self.rt.DrawRoundedRectangle(&Self::rounded(x, y, w, h, r), &b, width, &style);
            }
        }
    }

    /// The Cairn mark (brand/cairn-mark.svg) in a `size` square at x, y.
    pub fn cairn_mark(&self, x: f32, y: f32, size: f32, base: Color, top: Color) {
        let k = size / 64.0;
        let stones = [(8.0, 43.0, 48.0, 13.0, base), (11.0, 28.0, 35.0, 12.0, base), (26.0, 14.0, 19.0, 11.0, top)];
        for (sx, sy, w, h, c) in stones {
            self.fill_rounded(x + sx * k, y + sy * k, w * k, h * k, h * k / 2.0, c);
        }
    }

    pub fn fill_rect(&self, x: f32, y: f32, w: f32, h: f32, c: Color) {
        if let Some(b) = self.brush(c) {
            unsafe { self.rt.FillRectangle(&D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }, &b) }
        }
    }

    /// Soft shadow under a rounded card, built from stacked translucent
    /// rounded rectangles that grow outward.
    pub fn rect_shadow(&self, x: f32, y: f32, w: f32, h: f32, r: f32, blur: f32, alpha: f32) {
        const STEPS: usize = 10;
        for i in 0..STEPS {
            let grow = blur * (i as f32 + 1.0) / STEPS as f32;
            let a = alpha * 2.0 / STEPS as f32 * (1.0 - i as f32 / STEPS as f32);
            self.fill_rounded(x - grow, y - grow, w + grow * 2.0, h + grow * 2.0, r + grow, Color(0.0, 0.0, 0.0, a));
        }
    }

    /// Draws premultiplied BGRA pixels into the given box.
    pub fn image(&self, bgra: &[u8], w: u32, h: u32, x: f32, y: f32, dw: f32, dh: f32) {
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        unsafe {
            let size = D2D_SIZE_U { width: w, height: h };
            let Ok(bitmap) = self.rt.CreateBitmap(size, Some(bgra.as_ptr() as *const _), w * 4, &props) else { return };
            let dest = D2D_RECT_F { left: x, top: y, right: x + dw, bottom: y + dh };
            self.rt.DrawBitmap(&bitmap, Some(&dest), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
        }
    }

    /// Single line of text centered in the given box.
    pub fn text(&self, s: &str, format: &IDWriteTextFormat, x: f32, y: f32, w: f32, h: f32, c: Color) {
        let wide: Vec<u16> = s.encode_utf16().collect();
        if let Some(b) = self.brush(c) {
            unsafe {
                self.rt.DrawText(
                    &wide,
                    format,
                    &D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h },
                    &b,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                )
            }
        }
    }
}

/// CSS-style cubic-bezier easing.
pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let curve = |a: f32, b: f32, u: f32| 3.0 * a * u * (1.0 - u).powi(2) + 3.0 * b * u * u * (1.0 - u) + u.powi(3);
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if curve(x1, x2, mid) < t { lo = mid } else { hi = mid }
    }
    curve(y1, y2, (lo + hi) / 2.0)
}
