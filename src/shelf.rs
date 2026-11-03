//! The stack: items parked in Cairn (file references, text and
//! links), kept until removed and encrypted at rest.

use std::fs;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Shell::*;
use windows::core::{HSTRING, Interface};


use crate::store;

const MAGIC: &[u8; 8] = b"CAIRNST3";
/// 0.3 to 0.5: named stacks, items with no extra fields.
const MAGIC_V2: &[u8; 8] = b"CAIRNST2";
/// 0.1 and 0.2 kept one unnamed stack.
const MAGIC_V1: &[u8; 8] = b"CAIRNST1";
/// Extra item fields, saved as (key, length, bytes) so unknown keys can be
/// skipped and new ones added.
const FIELD_TRACK: u8 = 1;
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
    /// The shell's link data for a file, saved when it was added, so
    /// Windows can find the file again after it moves on the same drive.
    pub track: Option<Vec<u8>>,
}

impl ShelfItem {
    fn new(id: u64, kind: Kind) -> ShelfItem {
        let track = match &kind {
            Kind::File(p) if p.exists() => track(p),
            _ => None,
        };
        ShelfItem { id, kind, icon: None, track }
    }
}

/// Every named stack. The current one's items live in `items`, so most code
/// only ever sees one stack; switching swaps lists in and out of `others`.
pub struct Shelf {
    /// The current stack's items, newest first.
    pub items: Vec<ShelfItem>,
    pub names: Vec<String>,
    pub current: usize,
    /// The other stacks' items. The slot at `current` is kept empty.
    others: Vec<Vec<ShelfItem>>,
    next_id: u64,
}

pub const DEFAULT_NAME: &str = "Stack";

pub enum Dropped {
    Paths(Vec<PathBuf>),
    Text(String),
    /// Image data with no file behind it, such as a screenshot.
    Image { bytes: Vec<u8>, ext: &'static str },
}

fn file() -> Option<PathBuf> {
    Some(store::dir()?.join("stack.bin"))
}

/// Where Cairn keeps its own copies: images that had no file, and files
/// copied from removable or network drives.
pub fn owned_dir() -> Option<PathBuf> {
    let d = store::dir()?.join("files");
    fs::create_dir_all(&d).ok()?;
    Some(d)
}

/// "Image 2026-11-03 142501.png", or with " (2)" and so on if taken.
fn free_name(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let dot = if ext.is_empty() { String::new() } else { format!(".{ext}") };
    (1..)
        .map(|n| if n == 1 { dir.join(format!("{stem}{dot}")) } else { dir.join(format!("{stem} ({n}){dot}")) })
        .find(|p| !p.exists())
        .unwrap()
}

/// A free path in `dir` for a file or folder name, keeping its extension.
pub fn free_path(dir: &Path, name: &str) -> PathBuf {
    let p = Path::new(name);
    let stem = p.file_stem().map_or(name.into(), |s| s.to_string_lossy());
    let ext = p.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    free_name(dir, &stem, &ext)
}

fn stamp() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02} {:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

fn save_image(dir: &Path, bytes: &[u8], ext: &str, stamp: &str) -> Option<PathBuf> {
    let path = free_name(dir, &format!("Image {stamp}"), ext);
    fs::write(&path, bytes).ok()?;
    Some(path)
}

/// Copies a file into Cairn's folder, keeping its name.
pub fn copy_in(src: &Path) -> Option<PathBuf> {
    let dir = owned_dir()?;
    let stem = src.file_stem()?.to_string_lossy().into_owned();
    let ext = src.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    let dest = free_name(&dir, &stem, &ext);
    fs::copy(src, &dest).ok()?;
    Some(dest)
}

fn looks_like_link(s: &str) -> bool {
    let s = s.trim();
    !s.contains(char::is_whitespace) && (s.starts_with("http://") || s.starts_with("https://") || s.starts_with("www."))
}

impl Shelf {
    pub fn load() -> Shelf {
        let saved = file()
            .and_then(|p| fs::read(p).ok())
            .and_then(|enc| store::unprotect(&enc))
            .and_then(|raw| parse(&raw));
        Shelf::from_stacks(saved.unwrap_or_default())
    }

    fn from_stacks(saved: Saved) -> Shelf {
        let (mut stacks, current) = (saved.stacks, saved.current);
        if stacks.is_empty() {
            stacks.push((DEFAULT_NAME.to_string(), Vec::new()));
        }
        let current = current.min(stacks.len() - 1);
        let next_id = stacks.iter().flat_map(|(_, items)| items).map(|i| i.id).max().unwrap_or(0) + 1;
        let (names, mut others): (Vec<String>, Vec<Vec<ShelfItem>>) = stacks.into_iter().unzip();
        let items = std::mem::take(&mut others[current]);
        Shelf { items, names, current, others, next_id }
    }

    pub fn save(&self) {
        if let (Some(p), Some(enc)) = (file(), store::protect(&self.encode())) {
            let _ = store::write_atomic(&p, &enc);
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&(self.names.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.current as u32).to_le_bytes());
        for (i, name) in self.names.iter().enumerate() {
            put_str(&mut out, name);
            let items = if i == self.current { &self.items } else { &self.others[i] };
            out.extend_from_slice(&(items.len() as u32).to_le_bytes());
            for item in items {
                let (tag, body) = match &item.kind {
                    Kind::File(p) => (0u8, p.to_string_lossy().into_owned()),
                    Kind::Text(t) => (1, t.clone()),
                    Kind::Link(l) => (2, l.clone()),
                };
                out.extend_from_slice(&item.id.to_le_bytes());
                out.push(tag);
                put_str(&mut out, &body);
                let fields: Vec<(u8, &[u8])> = item.track.iter().map(|t| (FIELD_TRACK, t.as_slice())).collect();
                out.push(fields.len() as u8);
                for (key, bytes) in fields {
                    out.push(key);
                    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                    out.extend_from_slice(bytes);
                }
            }
        }
        out
    }

    /// How many items a stack holds.
    pub fn count(&self, stack: usize) -> usize {
        if stack == self.current { self.items.len() } else { self.others.get(stack).map_or(0, Vec::len) }
    }

    /// Makes another stack the current one.
    pub fn switch(&mut self, stack: usize) {
        if stack < self.names.len() && stack != self.current {
            std::mem::swap(&mut self.items, &mut self.others[self.current]);
            std::mem::swap(&mut self.items, &mut self.others[stack]);
            self.current = stack;
        }
    }

    /// Adds an empty stack with a free "Stack N" name and switches to it.
    pub fn new_stack(&mut self) -> usize {
        let name = (2..).map(|n| format!("{DEFAULT_NAME} {n}")).find(|n| !self.names.contains(n)).unwrap();
        self.names.push(name);
        self.others.push(Vec::new());
        let i = self.names.len() - 1;
        self.switch(i);
        i
    }

    pub fn rename(&mut self, stack: usize, name: &str) {
        let name = name.trim();
        if let Some(n) = self.names.get_mut(stack)
            && !name.is_empty()
        {
            *n = name.chars().take(40).collect();
        }
    }

    /// Points an item, in whichever stack holds it, at a new path.
    pub fn replace_path(&mut self, id: u64, path: PathBuf) -> bool {
        let all = self.items.iter_mut().chain(self.others.iter_mut().flatten());
        match all.into_iter().find(|i| i.id == id) {
            Some(item) => {
                item.track = track(&path);
                item.kind = Kind::File(path);
                item.icon = None;
                true
            }
            None => false,
        }
    }

    /// Asks Windows where a missing file in the current stack went, using
    /// the link data saved when it was added, and points the item there.
    pub fn find_again(&mut self, id: u64) -> Option<PathBuf> {
        let item = self.items.iter().find(|i| i.id == id)?;
        let found = resolve(item.track.as_deref()?)?;
        self.replace_path(id, found.clone());
        Some(found)
    }

    /// Deletes Cairn's own copies that no stack refers to any more. Items in
    /// `keep` (waiting for undo) count as referred to.
    pub fn prune_owned(&self, keep: &[ShelfItem]) {
        let Some(dir) = owned_dir() else { return };
        prune(&dir, self.items.iter().chain(self.others.iter().flatten()).chain(keep));
    }

    /// Moves items from the current stack to the front of another, skipping
    /// any it already holds. Returns how many moved.
    pub fn move_to(&mut self, ids: &[u64], stack: usize) -> usize {
        if stack == self.current || stack >= self.names.len() {
            return 0;
        }
        let (moving, keep): (Vec<ShelfItem>, Vec<ShelfItem>) = std::mem::take(&mut self.items).into_iter().partition(|i| ids.contains(&i.id));
        self.items = keep;
        let dest = &mut self.others[stack];
        let mut moved = 0;
        for item in moving {
            if !dest.iter().any(|d| same(&d.kind, &item.kind)) {
                dest.insert(moved, item);
                moved += 1;
            }
        }
        moved
    }

    /// Deletes a stack and returns its items, unless it is the last one.
    pub fn delete_stack(&mut self, stack: usize) -> Option<Vec<ShelfItem>> {
        if self.names.len() < 2 || stack >= self.names.len() {
            return None;
        }
        if stack == self.current {
            self.switch(if stack == 0 { 1 } else { stack - 1 });
        }
        self.names.remove(stack);
        let items = self.others.remove(stack);
        if self.current > stack {
            self.current -= 1;
        }
        Some(items)
    }

    fn contains(&self, kind: &Kind) -> bool {
        self.items.iter().any(|i| same(&i.kind, kind))
    }

    /// Adds dropped content, skipping duplicates. Returns how many were new.
    pub fn add(&mut self, dropped: Dropped) -> usize {
        let kinds: Vec<Kind> = match dropped {
            Dropped::Paths(paths) => paths.into_iter().map(Kind::File).collect(),
            Dropped::Image { bytes, ext } => owned_dir().and_then(|d| save_image(&d, &bytes, ext, &stamp())).map(Kind::File).into_iter().collect(),
            Dropped::Text(t) if t.trim().is_empty() => Vec::new(),
            Dropped::Text(t) if looks_like_link(&t) => vec![Kind::Link(t.trim().to_string())],
            Dropped::Text(t) => vec![Kind::Text(t)],
        };
        let mut added = 0;
        for kind in kinds {
            if !self.contains(&kind) {
                self.items.insert(added, ShelfItem::new(self.next_id, kind));
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

fn prune<'a>(dir: &Path, items: impl Iterator<Item = &'a ShelfItem>) {
    let used: Vec<&Path> = items.filter_map(|i| if let Kind::File(p) = &i.kind { Some(p.as_path()) } else { None }).collect();
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if used.contains(&p.as_path()) {
            continue;
        }
        // Folders come from dropped folders that had no path, like one
        // dragged out of a zip.
        if p.is_dir() {
            let _ = fs::remove_dir_all(p);
        } else {
            let _ = fs::remove_file(p);
        }
    }
}

fn same(a: &Kind, b: &Kind) -> bool {
    match (a, b) {
        (Kind::File(a), Kind::File(b)) => a == b,
        (Kind::Text(a), Kind::Text(b)) | (Kind::Link(a), Kind::Link(b)) => a == b,
        _ => false,
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// What the stack file holds: named stacks and which one is current.
#[derive(Default)]
struct Saved {
    stacks: Vec<(String, Vec<ShelfItem>)>,
    current: usize,
}

/// Reads the current format, or the single-stack format of 0.1 and 0.2.
fn parse(raw: &[u8]) -> Option<Saved> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = raw.get(at..at + n)?;
        at += n;
        Some(s)
    };
    let magic = take(8)?;
    let fields = magic == MAGIC;
    let named = fields || magic == MAGIC_V2;
    if !named && magic != MAGIC_V1 {
        return None;
    }
    let u32_at = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap());
    let (stack_count, current) = if named { (u32_at(take(4)?), u32_at(take(4)?) as usize) } else { (1, 0) };
    let mut saved = Saved { stacks: Vec::new(), current };
    for _ in 0..stack_count {
        let name = if named {
            let len = u32_at(take(4)?) as usize;
            String::from_utf8(take(len)?.to_vec()).ok()?
        } else {
            DEFAULT_NAME.to_string()
        };
        let count = u32_at(take(4)?);
        let mut items = Vec::new();
        for _ in 0..count {
            let id = u64::from_le_bytes(take(8)?.try_into().ok()?);
            let tag = take(1)?[0];
            let len = u32_at(take(4)?) as usize;
            let body = String::from_utf8(take(len)?.to_vec()).ok()?;
            let kind = match tag {
                0 => Kind::File(PathBuf::from(body)),
                1 => Kind::Text(body),
                2 => Kind::Link(body),
                _ => return None,
            };
            let mut item = ShelfItem { id, kind, icon: None, track: None };
            for _ in 0..if fields { take(1)?[0] } else { 0 } {
                let key = take(1)?[0];
                let len = u32_at(take(4)?) as usize;
                let bytes = take(len)?.to_vec();
                if key == FIELD_TRACK {
                    item.track = Some(bytes);
                }
            }
            items.push(item);
        }
        saved.stacks.push((name, items));
    }
    Some(saved)
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

/// Shell link data for a file: what a .lnk file would hold, including the
/// IDs Windows uses to track a file moved within its drive.
fn track(path: &Path) -> Option<Vec<u8>> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        link.SetPath(&HSTRING::from(path.as_os_str())).ok()?;
        let stream = SHCreateMemStream(None)?;
        link.cast::<IPersistStream>().ok()?.Save(&stream, true).ok()?;
        let mut len = 0u64;
        stream.Seek(0, STREAM_SEEK_CUR, Some(&mut len)).ok()?;
        stream.Seek(0, STREAM_SEEK_SET, None).ok()?;
        let mut out = vec![0u8; len as usize];
        let mut read = 0u32;
        stream.Read(out.as_mut_ptr() as *mut _, len as u32, Some(&mut read)).ok().ok()?;
        out.truncate(read as usize);
        Some(out)
    }
}

/// Where tracked link data points now, if that file exists. Never shows UI
/// and never searches folders by name; gives up after half a second.
fn resolve(data: &[u8]) -> Option<PathBuf> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = SHCreateMemStream(Some(data))?;
        link.cast::<IPersistStream>().ok()?.Load(&stream).ok()?;
        let flags = (SLR_NO_UI.0 | SLR_NOSEARCH.0) as u32 | (500 << 16);
        link.Resolve(None, flags).ok()?;
        let mut buf = vec![0u16; 32768];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let end = buf.iter().position(|&u| u == 0).unwrap_or(buf.len());
        let path = PathBuf::from(String::from_utf16_lossy(&buf[..end]));
        path.exists().then_some(path)
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
        Shelf::from_stacks(Saved::default())
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
        assert!(parse(&shelf().encode()).unwrap().stacks[0].1.is_empty());
        s.new_stack();
        s.add(Dropped::Text("in the second".into()));
        s.rename(1, "Invoices");
        let back = Shelf::from_stacks(parse(&s.encode()).unwrap());
        assert_eq!(back.names, ["Stack", "Invoices"]);
        assert_eq!((back.current, back.count(0), back.count(1)), (1, 3, 1));
        let first = &back.others[0];
        assert!(matches!(&first[2].kind, Kind::File(p) if p.to_string_lossy() == "C:\\x\\ü.txt"));
        assert!(matches!(&first[0].kind, Kind::Link(l) if l == "https://example.com"));
    }

    #[test]
    fn reads_the_single_stack_format() {
        let mut v1 = MAGIC_V1.to_vec();
        v1.extend_from_slice(&1u32.to_le_bytes());
        v1.extend_from_slice(&7u64.to_le_bytes());
        v1.push(1);
        put_str(&mut v1, "kept");
        let s = Shelf::from_stacks(parse(&v1).unwrap());
        assert_eq!(s.names, [DEFAULT_NAME]);
        assert!(matches!(&s.items[0].kind, Kind::Text(t) if t == "kept"));
        // New items never reuse an old id.
        assert_eq!(s.next_id, 8);
    }

    #[test]
    fn stacks_switch_and_delete() {
        let mut s = shelf();
        s.add(Dropped::Text("one".into()));
        assert_eq!(s.new_stack(), 1);
        assert_eq!(s.names[1], "Stack 2");
        assert!(s.items.is_empty());
        s.add(Dropped::Text("two".into()));
        s.switch(0);
        assert!(matches!(&s.items[0].kind, Kind::Text(t) if t == "one"));
        // Deleting the current stack moves to a neighbour.
        assert_eq!(s.delete_stack(0).unwrap().len(), 1);
        assert_eq!((s.current, s.names.len()), (0, 1));
        assert!(matches!(&s.items[0].kind, Kind::Text(t) if t == "two"));
        assert!(s.delete_stack(0).is_none());
    }

    #[test]
    fn images_get_free_names_and_unused_copies_go() {
        let dir = std::env::temp_dir().join("cairn-owned-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let a = save_image(&dir, b"one", "png", "2026-11-03 120000").unwrap();
        let b = save_image(&dir, b"two", "png", "2026-11-03 120000").unwrap();
        assert_eq!(a.file_name().unwrap(), "Image 2026-11-03 120000.png");
        assert_eq!(b.file_name().unwrap(), "Image 2026-11-03 120000 (2).png");
        assert_eq!(free_path(&dir, "Image 2026-11-03 120000.png").file_name().unwrap(), "Image 2026-11-03 120000 (3).png");
        fs::create_dir_all(dir.join("unzipped").join("inner")).unwrap();
        let kept = [ShelfItem { id: 1, kind: Kind::File(a.clone()), icon: None, track: None }];
        prune(&dir, kept.iter());
        assert!(a.exists() && !b.exists() && !dir.join("unzipped").exists());
    }

    #[test]
    fn replaces_a_path_in_any_stack() {
        let mut s = shelf();
        s.add(Dropped::Paths(vec!["E:\\usb.txt".into()]));
        let id = s.items[0].id;
        s.new_stack();
        assert!(s.replace_path(id, "C:\\copy\\usb.txt".into()));
        s.switch(0);
        assert!(matches!(&s.items[0].kind, Kind::File(p) if p.to_string_lossy() == "C:\\copy\\usb.txt"));
    }

    #[test]
    fn moves_items_between_stacks() {
        let mut s = shelf();
        s.add(Dropped::Paths(vec!["C:\\a.txt".into(), "C:\\b.txt".into(), "C:\\c.txt".into()]));
        s.new_stack();
        s.add(Dropped::Paths(vec!["C:\\b.txt".into()]));
        s.switch(0);
        let ids: Vec<u64> = s.items[..2].iter().map(|i| i.id).collect();
        // b.txt is already in the other stack, so only a.txt moves; both
        // leave this one.
        assert_eq!(s.move_to(&ids, 1), 1);
        assert_eq!(s.items.len(), 1);
        s.switch(1);
        let names: Vec<String> = s.items.iter().map(|i| name(&i.kind)).collect();
        assert_eq!(names, ["a.txt", "b.txt"]);
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
    fn reads_the_format_before_extra_fields() {
        let mut v2 = MAGIC_V2.to_vec();
        v2.extend_from_slice(&1u32.to_le_bytes());
        v2.extend_from_slice(&0u32.to_le_bytes());
        put_str(&mut v2, "Old");
        v2.extend_from_slice(&1u32.to_le_bytes());
        v2.extend_from_slice(&3u64.to_le_bytes());
        v2.push(2);
        put_str(&mut v2, "https://example.com");
        let s = Shelf::from_stacks(parse(&v2).unwrap());
        assert_eq!(s.names, ["Old"]);
        assert!(matches!(&s.items[0].kind, Kind::Link(l) if l == "https://example.com"));
    }

    #[test]
    fn finds_a_file_moved_on_the_same_drive() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let dir = std::env::temp_dir().join("cairn-track-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("later")).unwrap();
        let (before, after) = (dir.join("report.txt"), dir.join("later").join("report.txt"));
        fs::write(&before, "r").unwrap();
        let mut s = shelf();
        s.add(Dropped::Paths(vec![before.clone()]));
        // The link data survives a save and load.
        let mut s = Shelf::from_stacks(parse(&s.encode()).unwrap());
        assert!(s.items[0].track.is_some());
        fs::rename(&before, &after).unwrap();
        let found = s.find_again(s.items[0].id).expect("found again");
        let real = |p: &Path| fs::canonicalize(p).unwrap();
        assert_eq!(real(&found), real(&after));
        assert!(matches!(&s.items[0].kind, Kind::File(p) if real(p) == real(&after)));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn display_names() {
        assert_eq!(name(&Kind::Link("https://www.example.com/x".into())), "example.com/x");
        assert_eq!(kind_label(&Kind::File("Z:\\definitely\\missing.bin".into())), "Missing");
    }
}
