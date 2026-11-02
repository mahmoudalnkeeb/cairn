//! The stack: items parked in Cairn (file references, text and
//! links), kept until removed and encrypted at rest.

use std::fs;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Shell::*;
use windows::core::HSTRING;


use crate::store;

const MAGIC: &[u8; 8] = b"CAIRNST1";
const ICON_PX: i32 = 48;

/// Premultiplied BGRA pixels, ready for Direct2D.
pub struct Thumb {
    pub w: u32,
    pub h: u32,
    pub bgra: Vec<u8>,
}

/// One line of text for a tile: the first 300 characters, whitespace
/// collapsed.
pub fn preview(text: &str) -> String {
    let head: String = text.chars().take(300).collect();
    head.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub enum Kind {
    /// A reference to a file or folder; the shelf never copies or moves it.
    File(PathBuf),
    Text(String),
    Link(String),
}

pub struct ShelfItem {
    pub id: u64,
    pub kind: Kind,
    /// Shell icon or thumbnail, loaded on first draw. `Some(None)` means
    /// loading failed and a glyph is drawn instead.
    pub icon: Option<Option<Thumb>>,
}

pub struct Shelf {
    /// Newest first.
    pub items: Vec<ShelfItem>,
    next_id: u64,
}

pub enum Dropped {
    Paths(Vec<PathBuf>),
    Text(String),
}

fn file() -> Option<PathBuf> {
    Some(store::dir()?.join("stack.bin"))
}

fn looks_like_link(s: &str) -> bool {
    let s = s.trim();
    !s.contains(char::is_whitespace) && (s.starts_with("http://") || s.starts_with("https://") || s.starts_with("www."))
}

impl Shelf {
    pub fn load() -> Shelf {
        let items = file()
            .and_then(|p| fs::read(p).ok())
            .and_then(|enc| store::unprotect(&enc))
            .and_then(|raw| parse(&raw))
            .unwrap_or_default();
        let next_id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        Shelf { items, next_id }
    }

    pub fn save(&self) {
        if let (Some(p), Some(enc)) = (file(), store::protect(&self.encode())) {
            let _ = store::write_atomic(&p, &enc);
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&(self.items.len() as u32).to_le_bytes());
        for item in &self.items {
            let (tag, body) = match &item.kind {
                Kind::File(p) => (0u8, p.to_string_lossy().into_owned()),
                Kind::Text(t) => (1, t.clone()),
                Kind::Link(l) => (2, l.clone()),
            };
            out.extend_from_slice(&item.id.to_le_bytes());
            out.push(tag);
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(body.as_bytes());
        }
        out
    }

    fn contains(&self, kind: &Kind) -> bool {
        self.items.iter().any(|i| match (&i.kind, kind) {
            (Kind::File(a), Kind::File(b)) => a == b,
            (Kind::Text(a), Kind::Text(b)) | (Kind::Link(a), Kind::Link(b)) => a == b,
            _ => false,
        })
    }

    /// Adds dropped content, skipping duplicates. Returns how many were new.
    pub fn add(&mut self, dropped: Dropped) -> usize {
        let kinds: Vec<Kind> = match dropped {
            Dropped::Paths(paths) => paths.into_iter().map(Kind::File).collect(),
            Dropped::Text(t) if t.trim().is_empty() => Vec::new(),
            Dropped::Text(t) if looks_like_link(&t) => vec![Kind::Link(t.trim().to_string())],
            Dropped::Text(t) => vec![Kind::Text(t)],
        };
        let mut added = 0;
        for kind in kinds {
            if !self.contains(&kind) {
                self.items.insert(added, ShelfItem { id: self.next_id, kind, icon: None });
                self.next_id += 1;
                added += 1;
            }
        }
        added
    }

    pub fn remove(&mut self, id: u64) {
        self.items.retain(|i| i.id != id);
    }

    /// Empties the stack, handing back what was in it for undo.
    pub fn clear(&mut self) -> Vec<ShelfItem> {
        std::mem::take(&mut self.items)
    }

    /// Puts cleared items back behind anything added since, skipping ones
    /// that were dropped in again. Returns how many came back.
    pub fn restore(&mut self, items: Vec<ShelfItem>) -> usize {
        let before = self.items.len();
        for item in items {
            if !self.contains(&item.kind) {
                self.items.push(item);
            }
        }
        self.items.len() - before
    }
}

fn parse(raw: &[u8]) -> Option<Vec<ShelfItem>> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = raw.get(at..at + n)?;
        at += n;
        Some(s)
    };
    if take(8)? != MAGIC {
        return None;
    }
    let count = u32::from_le_bytes(take(4)?.try_into().ok()?);
    let mut items = Vec::new();
    for _ in 0..count {
        let id = u64::from_le_bytes(take(8)?.try_into().ok()?);
        let tag = take(1)?[0];
        let len = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
        let body = String::from_utf8(take(len)?.to_vec()).ok()?;
        let kind = match tag {
            0 => Kind::File(PathBuf::from(body)),
            1 => Kind::Text(body),
            2 => Kind::Link(body),
            _ => return None,
        };
        items.push(ShelfItem { id, kind, icon: None });
    }
    Some(items)
}

/// Short display name for a tile.
pub fn name(kind: &Kind) -> String {
    match kind {
        Kind::File(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string_lossy().into_owned()),
        Kind::Text(t) => preview(t),
        Kind::Link(l) => l.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.").to_string(),
    }
}

/// The kind word shown under the name.
pub fn kind_label(kind: &Kind) -> &'static str {
    match kind {
        Kind::File(p) if p.is_dir() => "Folder",
        Kind::File(p) if p.exists() => "File",
        Kind::File(_) => "Missing",
        Kind::Text(_) => "Text",
        Kind::Link(_) => "Link",
    }
}

/// The shell's icon or thumbnail for a path, as premultiplied BGRA.
pub fn shell_icon(path: &Path) -> Option<Thumb> {
    unsafe {
        let name = HSTRING::from(path.to_string_lossy().as_ref());
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&name, None).ok()?;
        let bitmap = factory.GetImage(SIZE { cx: ICON_PX, cy: ICON_PX }, SIIGBF_BIGGERSIZEOK).ok()?;
        let mut bm = BITMAP::default();
        GetObjectW(bitmap, size_of::<BITMAP>() as i32, Some(&mut bm as *mut BITMAP as *mut _));
        let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(None);
        let lines = GetDIBits(dc, bitmap, 0, h as u32, Some(bgra.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(bitmap);
        if lines == 0 || w <= 0 {
            return None;
        }
        // Some thumbnails come without alpha; show them opaque.
        if bgra.chunks_exact(4).all(|p| p[3] == 0) {
            bgra.chunks_exact_mut(4).for_each(|p| p[3] = 255);
        }
        Some(Thumb { w: w as u32, h: h as u32, bgra })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shelf() -> Shelf {
        Shelf { items: Vec::new(), next_id: 1 }
    }

    #[test]
    fn classifies_links_and_text() {
        let mut s = shelf();
        s.add(Dropped::Text("https://example.com/a".into()));
        s.add(Dropped::Text("two words".into()));
        s.add(Dropped::Text("   ".into()));
        assert!(matches!(s.items[1].kind, Kind::Link(_)));
        assert!(matches!(s.items[0].kind, Kind::Text(_)));
        assert_eq!(s.items.len(), 2);
    }

    #[test]
    fn newest_first_and_no_duplicates() {
        let mut s = shelf();
        assert_eq!(s.add(Dropped::Paths(vec!["C:\\a.txt".into(), "C:\\b.txt".into()])), 2);
        assert_eq!(s.add(Dropped::Paths(vec!["C:\\b.txt".into(), "C:\\c.txt".into()])), 1);
        let names: Vec<String> = s.items.iter().map(|i| name(&i.kind)).collect();
        assert_eq!(names, ["c.txt", "a.txt", "b.txt"]);
    }

    #[test]
    fn storage_round_trips() {
        let mut s = shelf();
        s.add(Dropped::Paths(vec!["C:\\x\\ü.txt".into()]));
        s.add(Dropped::Text("note\nline".into()));
        s.add(Dropped::Text("https://example.com".into()));
        assert!(parse(&shelf().encode()).unwrap().is_empty());
        let back = parse(&s.encode()).unwrap();
        assert_eq!(back.len(), 3);
        assert!(matches!(&back[2].kind, Kind::File(p) if p.to_string_lossy() == "C:\\x\\ü.txt"));
        assert!(matches!(&back[0].kind, Kind::Link(l) if l == "https://example.com"));
    }

    #[test]
    fn clear_and_restore() {
        let mut s = shelf();
        s.add(Dropped::Paths(vec!["C:\\a.txt".into(), "C:\\b.txt".into()]));
        let cleared = s.clear();
        assert!(s.items.is_empty());
        // Dropped again while the undo was pending: kept once, in front.
        s.add(Dropped::Paths(vec!["C:\\b.txt".into(), "C:\\n.txt".into()]));
        assert_eq!(s.restore(cleared), 1);
        let names: Vec<String> = s.items.iter().map(|i| name(&i.kind)).collect();
        assert_eq!(names, ["b.txt", "n.txt", "a.txt"]);
    }

    #[test]
    fn display_names() {
        assert_eq!(name(&Kind::Link("https://www.example.com/x".into())), "example.com/x");
        assert_eq!(kind_label(&Kind::File("Z:\\definitely\\missing.bin".into())), "Missing");
    }
}
