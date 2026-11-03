//! OLE drag and drop: the edge tab and the stack accept files, text and
//! links; items can be dragged out into any app.

use std::cell::{Cell, RefCell};
use std::mem::ManuallyDrop;
use std::path::PathBuf;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::DeleteObject;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
use windows::Win32::System::Com::StructuredStorage::StgCreateDocfile;
use windows::Win32::System::Com::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::Shell::Common;
use windows::Win32::UI::WindowsAndMessaging::{IDC_ARROW, LoadCursorW, SetCursor};
use windows::core::*;

use crate::shelf::{Dropped, Kind};

#[derive(Clone, Copy, PartialEq)]
pub enum Zone {
    /// The edge tab window that took the drag.
    Tab(HWND),
    Stack,
    /// The shake-to-show zone near the cursor.
    Near,
}

fn registered(name: PCWSTR) -> u16 {
    unsafe { windows::Win32::System::DataExchange::RegisterClipboardFormatW(name) as u16 }
}

fn url_format() -> u16 {
    registered(w!("UniformResourceLocatorW"))
}

fn format(cf: u16) -> FORMATETC {
    FORMATETC { cfFormat: cf, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 }
}

fn has(data: &IDataObject, cf: u16) -> bool {
    unsafe { data.QueryGetData(&format(cf)) == S_OK }
}

fn png_format() -> u16 {
    registered(w!("PNG"))
}

fn descriptor_format() -> u16 {
    registered(w!("FileGroupDescriptorW"))
}

fn contents_format() -> u16 {
    registered(w!("FileContents"))
}

fn supported(data: &IDataObject) -> bool {
    [CF_HDROP.0, descriptor_format(), url_format(), CF_UNICODETEXT.0, png_format(), CF_DIB.0].into_iter().any(|cf| has(data, cf))
}

/// A file or folder a source offers with no path on disk, such as an email
/// attachment or a file inside a zip.
struct Virtual {
    /// May hold a relative path ("folder\file.txt") for items inside a folder.
    name: String,
    dir: bool,
    written: Option<FILETIME>,
}

/// The FILEGROUPDESCRIPTORW a source puts on the data object.
fn parse_descriptors(bytes: &[u8]) -> Option<Vec<Virtual>> {
    let count = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    let size = size_of::<FILEDESCRIPTORW>();
    (0..count)
        .map(|i| {
            let raw = bytes.get(4 + i * size..4 + (i + 1) * size)?;
            let d: FILEDESCRIPTORW = unsafe { std::ptr::read_unaligned(raw.as_ptr() as *const _) };
            let (flags, attrs, units) = (d.dwFlags, d.dwFileAttributes, d.cFileName);
            let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
            Some(Virtual {
                name: String::from_utf16_lossy(&units[..end]),
                dir: flags & FD_ATTRIBUTES.0 as u32 != 0 && attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
                written: (flags & FD_WRITESTIME.0 as u32 != 0).then_some(d.ftLastWriteTime),
            })
        })
        .collect()
}

/// The parts of a name a source gave, safe to use as a path under Cairn's
/// folder: no "..", no drive or root, no characters Windows refuses.
fn safe_parts(name: &str) -> Option<Vec<String>> {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6",
        "LPT7", "LPT8", "LPT9",
    ];
    let parts: Vec<String> = name
        .split(['\\', '/'])
        .map(|part| {
            let clean: String = part.chars().map(|c| if c < ' ' || "<>:\"|?*".contains(c) { '_' } else { c }).collect();
            let clean = clean.trim().trim_end_matches('.').to_string();
            let stem = clean.split('.').next().unwrap_or_default().to_ascii_uppercase();
            if RESERVED.contains(&stem.as_str()) { format!("_{clean}") } else { clean }
        })
        .filter(|p| !p.is_empty())
        .collect();
    (!parts.is_empty()).then_some(parts)
}

/// Copies a stream to a file in 64 KB pieces, so large files never sit in
/// memory whole.
fn write_stream(stream: &IStream, target: &std::path::Path) -> Option<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(target).ok()?;
    let mut buf = vec![0u8; 64 * 1024];
    unsafe {
        let _ = stream.Seek(0, STREAM_SEEK_SET, None);
        loop {
            let mut read = 0u32;
            stream.Read(buf.as_mut_ptr() as *mut _, buf.len() as u32, Some(&mut read)).ok().ok()?;
            if read == 0 {
                return Some(());
            }
            file.write_all(&buf[..read as usize]).ok()?;
        }
    }
}

/// Writes the contents of virtual item `index` to `target`. Sources hand it
/// over as memory, a stream, or (Outlook messages) a structured storage.
fn save_contents(data: &IDataObject, index: usize, target: &std::path::Path) -> Option<()> {
    let fmt = FORMATETC {
        cfFormat: contents_format(),
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: index as i32,
        tymed: (TYMED_HGLOBAL.0 | TYMED_ISTREAM.0 | TYMED_ISTORAGE.0) as u32,
    };
    unsafe {
        let mut medium = data.GetData(&fmt).ok()?;
        let saved = match TYMED(medium.tymed as i32) {
            TYMED_ISTREAM => medium.u.pstm.as_ref().and_then(|s| write_stream(s, target)),
            TYMED_ISTORAGE => medium.u.pstg.as_ref().and_then(|src| {
                let dest = StgCreateDocfile(&HSTRING::from(target.as_os_str()), STGM_CREATE | STGM_READWRITE | STGM_SHARE_EXCLUSIVE, 0).ok()?;
                src.CopyTo(None, None, &dest).ok()?;
                dest.Commit(0).ok()
            }),
            TYMED_HGLOBAL => {
                let global = medium.u.hGlobal;
                let ptr = GlobalLock(global) as *const u8;
                let ok = !ptr.is_null() && std::fs::write(target, std::slice::from_raw_parts(ptr, GlobalSize(global))).is_ok();
                let _ = GlobalUnlock(global);
                ok.then_some(())
            }
            _ => None,
        };
        ReleaseStgMedium(&mut medium);
        saved
    }
}

/// FILETIME (100 ns steps since 1601) as a SystemTime.
fn system_time(ft: FILETIME) -> Option<std::time::SystemTime> {
    let ticks = ((ft.dwHighDateTime as u64) << 32 | ft.dwLowDateTime as u64).checked_sub(116_444_736_000_000_000)?;
    std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_nanos(ticks.checked_mul(100)?))
}

/// Saves the files and folders a source offers without paths into Cairn's
/// folder and returns the top-level ones. None when there are none, or when
/// they are only link shortcuts (browsers offer a .url file with every
/// dragged link), so the link itself is kept instead.
fn read_virtual(data: &IDataObject) -> Option<Vec<PathBuf>> {
    let items = parse_descriptors(&read_bytes(data, descriptor_format())?)?;
    if items.iter().all(|v| v.name.to_ascii_lowercase().ends_with(".url")) {
        return None;
    }
    let dir = crate::shelf::owned_dir()?;
    // Each top-level name, and where it was saved.
    let mut tops: Vec<(String, PathBuf)> = Vec::new();
    for (i, v) in items.iter().enumerate() {
        let Some(parts) = safe_parts(&v.name) else { continue };
        let top = match tops.iter().find(|(n, _)| *n == parts[0]) {
            Some((_, p)) => p.clone(),
            None => {
                let p = crate::shelf::free_path(&dir, &parts[0]);
                tops.push((parts[0].clone(), p.clone()));
                p
            }
        };
        let target = parts[1..].iter().fold(top, |p, part| p.join(part));
        if v.dir {
            let _ = std::fs::create_dir_all(&target);
            continue;
        }
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if save_contents(data, i, &target).is_none() {
            let _ = std::fs::remove_file(&target);
            continue;
        }
        if let Some(t) = v.written.and_then(system_time)
            && let Ok(f) = std::fs::File::options().write(true).open(&target)
        {
            let _ = f.set_modified(t);
        }
    }
    let saved: Vec<PathBuf> = tops.into_iter().map(|(_, p)| p).filter(|p| p.exists()).collect();
    (!saved.is_empty()).then_some(saved)
}

fn read_bytes(data: &IDataObject, cf: u16) -> Option<Vec<u8>> {
    unsafe {
        let mut medium = data.GetData(&format(cf)).ok()?;
        let global = medium.u.hGlobal;
        let size = GlobalSize(global);
        let ptr = GlobalLock(global) as *const u8;
        let bytes = (!ptr.is_null() && size > 0).then(|| std::slice::from_raw_parts(ptr, size).to_vec());
        let _ = GlobalUnlock(global);
        ReleaseStgMedium(&mut medium);
        bytes
    }
}

/// A .bmp file for CF_DIB data: the 14-byte file header, then the DIB as is.
pub fn bmp_from_dib(dib: &[u8]) -> Option<Vec<u8>> {
    let u32_at = |at: usize| Some(u32::from_le_bytes(dib.get(at..at + 4)?.try_into().ok()?));
    let header = u32_at(0)? as usize;
    let bits = u16::from_le_bytes(dib.get(14..16)?.try_into().ok()?) as u32;
    let compression = u32_at(16)?;
    let used = u32_at(32)?;
    let colors = if used > 0 { used } else if bits <= 8 { 1 << bits } else { 0 };
    // A 40-byte header with BI_BITFIELDS is followed by three colour masks.
    let masks = if compression == 3 && header == 40 { 12 } else { 0 };
    let offset = 14 + header + masks + colors as usize * 4;
    if header < 40 || offset > 14 + dib.len() {
        return None;
    }
    let mut out = Vec::with_capacity(14 + dib.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((14 + dib.len()) as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(dib);
    Some(out)
}

fn read_utf16(data: &IDataObject, cf: u16) -> Option<String> {
    unsafe {
        let mut medium = data.GetData(&format(cf)).ok()?;
        let global = medium.u.hGlobal;
        let size = GlobalSize(global) / 2;
        let ptr = GlobalLock(global) as *const u16;
        let text = (!ptr.is_null()).then(|| {
            let units = std::slice::from_raw_parts(ptr, size);
            let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
            String::from_utf16_lossy(&units[..end])
        });
        let _ = GlobalUnlock(global);
        ReleaseStgMedium(&mut medium);
        text
    }
}

fn read_paths(data: &IDataObject) -> Option<Vec<PathBuf>> {
    unsafe {
        let mut medium = data.GetData(&format(CF_HDROP.0)).ok()?;
        let drop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(drop, u32::MAX, None);
        let paths = (0..count)
            .map(|i| {
                let mut buf = vec![0u16; DragQueryFileW(drop, i, None) as usize + 1];
                let n = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
                PathBuf::from(String::from_utf16_lossy(&buf[..n]))
            })
            .collect();
        ReleaseStgMedium(&mut medium);
        Some(paths)
    }
}

/// Files first, then files with no path (saved into Cairn), then a link,
/// then plain text, then an image. Images come last because apps like Excel
/// offer a picture of copied cells next to the text.
pub fn read(data: &IDataObject) -> Option<Dropped> {
    if has(data, CF_HDROP.0) {
        return read_paths(data).map(Dropped::Paths);
    }
    if has(data, descriptor_format())
        && let Some(paths) = read_virtual(data)
    {
        return Some(Dropped::Paths(paths));
    }
    if let Some(text) = read_utf16(data, url_format()).or_else(|| read_utf16(data, CF_UNICODETEXT.0)) {
        // An image copied or dragged from a browser comes with its address.
        // Keep the picture: the address may stop working once the page closes.
        if crate::shelf::looks_like_link(&text)
            && let Some(image) = read_image(data)
        {
            return Some(image);
        }
        return Some(Dropped::Text(text));
    }
    read_image(data)
}

fn read_image(data: &IDataObject) -> Option<Dropped> {
    if let Some(png) = read_bytes(data, png_format()) {
        return Some(Dropped::Image { bytes: png, ext: "png" });
    }
    read_bytes(data, CF_DIB.0).and_then(|dib| bmp_from_dib(&dib)).map(|bmp| Dropped::Image { bytes: bmp, ext: "bmp" })
}

/// Cairn keeps references, so ask the source to copy, never move.
fn choose_effect(allowed: DROPEFFECT) -> DROPEFFECT {
    if allowed.0 & DROPEFFECT_COPY.0 != 0 {
        DROPEFFECT_COPY
    } else if allowed.0 & DROPEFFECT_LINK.0 != 0 {
        DROPEFFECT_LINK
    } else {
        DROPEFFECT_NONE
    }
}

/// The label under the source's drag image ("Add to Stack"). Shown only by
/// sources that allow it, such as Explorer and Cairn itself.
fn describe(data: &IDataObject, kind: DROPIMAGETYPE, message: &str, insert: &str) {
    let text = |s: &str| {
        let mut out = [0u16; 260];
        for (o, c) in out.iter_mut().zip(s.encode_utf16().take(259)) {
            *o = c;
        }
        out
    };
    let d = DROPDESCRIPTION { r#type: kind, szMessage: text(message), szInsert: text(insert) };
    let bytes = unsafe { std::slice::from_raw_parts(&d as *const _ as *const u8, size_of::<DROPDESCRIPTION>()) };
    let _ = set_global(data, registered(w!("DropDescription")), bytes);
}

/// Whether the shell is drawing the drag image and its effect badge, in
/// which case the source shows a plain arrow.
fn showing_layered(data: &IDataObject) -> bool {
    let cf = registered(w!("IsShowingLayered"));
    has(data, cf) && read_bytes(data, cf).is_some_and(|b| b.iter().take(4).any(|&x| x != 0))
}

#[implement(IDropTarget)]
struct DropTarget {
    zone: Zone,
    hwnd: HWND,
    accept: Cell<bool>,
    /// Keeps the source's drag image drawn over Cairn's windows.
    helper: Option<IDropTargetHelper>,
    data: RefCell<Option<IDataObject>>,
    /// The stack name last put in the drop label.
    label: RefCell<String>,
}

impl DropTarget {
    /// Names the stack the drop will land in, when that changed.
    fn update_label(&self) {
        let name = crate::drop_label();
        if *self.label.borrow() == name {
            return;
        }
        if let Some(data) = &*self.data.borrow() {
            describe(data, DROPIMAGE_COPY, "Add to %1", &name);
        }
        *self.label.borrow_mut() = name;
    }

    fn clear_label(&self) {
        if let Some(data) = &*self.data.borrow()
            && !self.label.borrow().is_empty()
        {
            describe(data, DROPIMAGE_INVALID, "", "");
        }
        self.label.borrow_mut().clear();
    }
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(&self, data: Option<&IDataObject>, _: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let ok = data.is_some_and(supported);
        self.accept.set(ok);
        *self.data.borrow_mut() = data.cloned();
        self.label.borrow_mut().clear();
        unsafe {
            *effect = if ok { choose_effect(*effect) } else { DROPEFFECT_NONE };
        }
        crate::on_drag_hover(self.zone, ok);
        if ok {
            self.update_label();
        }
        if let (Some(h), Some(d)) = (&self.helper, data) {
            unsafe {
                let _ = h.DragEnter(self.hwnd, d, &POINT { x: pt.x, y: pt.y }, *effect);
            }
        }
        Ok(())
    }

    fn DragOver(&self, _: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        unsafe {
            *effect = if self.accept.get() { choose_effect(*effect) } else { DROPEFFECT_NONE };
        }
        if self.accept.get() {
            crate::on_drag_move(self.zone, POINT { x: pt.x, y: pt.y });
            // Pointing at another stack's name switches to it.
            self.update_label();
        }
        if let Some(h) = &self.helper {
            unsafe {
                let _ = h.DragOver(&POINT { x: pt.x, y: pt.y }, *effect);
            }
        }
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        self.clear_label();
        if let Some(h) = &self.helper {
            unsafe {
                let _ = h.DragLeave();
            }
        }
        *self.data.borrow_mut() = None;
        crate::on_drag_hover(self.zone, false);
        Ok(())
    }

    fn Drop(&self, data: Option<&IDataObject>, _: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        self.clear_label();
        *self.data.borrow_mut() = None;
        crate::on_drag_hover(self.zone, false);
        let dropped = data.and_then(read);
        unsafe {
            *effect = if dropped.is_some() { choose_effect(*effect) } else { DROPEFFECT_NONE };
        }
        if let (Some(h), Some(d)) = (&self.helper, data) {
            unsafe {
                let _ = h.Drop(d, &POINT { x: pt.x, y: pt.y }, *effect);
            }
        }
        if let Some(d) = dropped {
            crate::on_drop(self.zone, d);
        }
        Ok(())
    }
}

pub fn register(hwnd: HWND, zone: Zone) -> Result<()> {
    let helper = unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER).ok() };
    let target: IDropTarget = DropTarget {
        zone,
        hwnd,
        accept: Cell::new(false),
        helper,
        data: RefCell::new(None),
        label: RefCell::new(String::new()),
    }
    .into();
    unsafe { RegisterDragDrop(hwnd, &target) }
}

#[implement(IDropSource)]
struct DropSource {
    data: IDataObject,
}

impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if keys.0 & MK_LBUTTON.0 == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _: DROPEFFECT) -> HRESULT {
        // The shell draws the effect beside the drag image; a "+" on the
        // cursor as well would show it twice.
        if showing_layered(&self.data) {
            unsafe {
                if let Ok(arrow) = LoadCursorW(None, IDC_ARROW) {
                    SetCursor(arrow);
                }
            }
            return S_OK;
        }
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

fn set_utf16(obj: &IDataObject, cf: u16, text: &str) -> Option<()> {
    let units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let bytes = unsafe { std::slice::from_raw_parts(units.as_ptr() as *const u8, units.len() * 2) };
    set_global(obj, cf, bytes)
}

fn set_global(obj: &IDataObject, cf: u16, bytes: &[u8]) -> Option<()> {
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).ok()?;
        let ptr = GlobalLock(global) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(global);
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(global);
        let medium = STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: global },
            pUnkForRelease: ManuallyDrop::new(None),
        };
        // With fRelease = TRUE the data object owns the memory on success.
        if obj.SetData(&format(cf), &medium, true).is_err() {
            let _ = GlobalFree(global);
            return None;
        }
    }
    Some(())
}

/// A data object other apps understand: the shell's own for files, plain
/// text (plus a URL format for links) otherwise.
pub fn data_object(kind: &Kind) -> Option<IDataObject> {
    unsafe {
        match kind {
            Kind::File(p) => {
                let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(p.to_string_lossy().as_ref()), None).ok()?;
                item.BindToHandler(None, &BHID_DataObject).ok()
            }
            Kind::Text(t) => {
                let obj: IDataObject = SHCreateDataObject(None, None, None).ok()?;
                set_utf16(&obj, CF_UNICODETEXT.0, t)?;
                Some(obj)
            }
            Kind::Link(l) => {
                let obj: IDataObject = SHCreateDataObject(None, None, None).ok()?;
                set_utf16(&obj, CF_UNICODETEXT.0, l)?;
                set_utf16(&obj, url_format(), l)?;
                Some(obj)
            }
        }
    }
}

/// One data object for several items. Files win: if any are selected, only
/// the files go, as one shell item array, so Explorer and mail clients see a
/// normal multi-file drag. Otherwise texts and links go as lines of text.
pub fn data_object_many(kinds: &[&Kind]) -> Option<IDataObject> {
    if let [one] = kinds {
        return data_object(one);
    }
    let paths: Vec<&PathBuf> = kinds.iter().filter_map(|k| if let Kind::File(p) = k { Some(p) } else { None }).collect();
    if paths.is_empty() {
        let lines: Vec<&str> = kinds
            .iter()
            .filter_map(|k| match k {
                Kind::Text(t) | Kind::Link(t) => Some(t.as_str()),
                Kind::File(_) => None,
            })
            .collect();
        let obj: IDataObject = unsafe { SHCreateDataObject(None, None, None).ok()? };
        set_utf16(&obj, CF_UNICODETEXT.0, &lines.join("\r\n"))?;
        return Some(obj);
    }
    unsafe {
        let pidls: Vec<*const Common::ITEMIDLIST> = paths
            .iter()
            .map(|p| ILCreateFromPathW(&HSTRING::from(p.to_string_lossy().as_ref())) as *const Common::ITEMIDLIST)
            .filter(|p| !p.is_null())
            .collect();
        let data = (!pidls.is_empty())
            .then(|| SHCreateShellItemArrayFromIDLists(&pidls).ok())
            .flatten()
            .and_then(|array| array.BindToHandler(None, &BHID_DataObject).ok());
        for p in pidls {
            ILFree(Some(p));
        }
        data
    }
}

/// Runs the modal drag loop with `image` under the cursor. Callers must not
/// hold app state across it, since window messages keep flowing while it
/// runs. The shell owns the image's bitmap once it takes it.
pub fn drag_out(data: &IDataObject, image: Option<SHDRAGIMAGE>) -> DROPEFFECT {
    if let Some(img) = image {
        let helper: Option<IDragSourceHelper2> = unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER).ok() };
        let taken = helper.is_some_and(|h| unsafe {
            let _ = h.SetFlags(DSH_ALLOWDROPDESCRIPTIONTEXT.0 as u32);
            h.InitializeFromBitmap(&img, data).is_ok()
        });
        if !taken {
            unsafe {
                let _ = DeleteObject(img.hbmpDragImage);
            }
        }
    }
    let source: IDropSource = DropSource { data: data.clone() }.into();
    let mut effect = DROPEFFECT_NONE;
    unsafe {
        let _ = DoDragDrop(data, &source, DROPEFFECT_COPY | DROPEFFECT_LINK, &mut effect);
    }
    effect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_files_drag_out_as_one_file_drop() {
        unsafe {
            let _ = OleInitialize(None);
        }
        let dir = std::env::temp_dir().join("cairn-dnd-test");
        let _ = std::fs::create_dir_all(dir.join("sub"));
        let (a, b) = (dir.join("a.txt"), dir.join("sub").join("b.txt"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let note = Kind::Text("skipped when files are present".into());
        let missing = Kind::File(dir.join("gone.txt"));
        let (ka, kb) = (Kind::File(a.clone()), Kind::File(b.clone()));
        let obj = data_object_many(&[&ka, &note, &missing, &kb]).expect("data object");
        let Some(Dropped::Paths(paths)) = read(&obj) else { panic!("no file drop") };
        // The shell gives back long names, and TEMP can be a short 8.3 path (as on CI runners).
        let real = |p: &PathBuf| std::fs::canonicalize(p).unwrap();
        assert_eq!(paths.iter().map(real).collect::<Vec<_>>(), [real(&a), real(&b)]);
    }

    #[test]
    fn several_texts_drag_out_as_lines() {
        let (t, l) = (Kind::Text("one".into()), Kind::Link("https://example.com".into()));
        let obj = data_object_many(&[&t, &l]).expect("data object");
        assert!(matches!(read(&obj), Some(Dropped::Text(s)) if s == "one\r\nhttps://example.com"));
    }

    #[test]
    fn a_browser_image_keeps_the_picture_not_its_address() {
        let with = |text: &str| {
            let obj: IDataObject = unsafe { SHCreateDataObject(None, None, None) }.unwrap();
            set_utf16(&obj, CF_UNICODETEXT.0, text).unwrap();
            set_global(&obj, png_format(), b"\x89PNG fake").unwrap();
            read(&obj)
        };
        assert!(matches!(with("https://example.com/cat.png"), Some(Dropped::Image { ext: "png", .. })));
        // Copied cells from Excel come with a picture too; the text wins.
        assert!(matches!(with("Q3\t1200"), Some(Dropped::Text(t)) if t == "Q3\t1200"));
    }

    #[test]
    fn reads_file_descriptors() {
        let entry = |name: &str, flags: u32, attrs: u32| {
            let mut d: FILEDESCRIPTORW = unsafe { std::mem::zeroed() };
            d.dwFlags = flags;
            d.dwFileAttributes = attrs;
            d.ftLastWriteTime = FILETIME { dwLowDateTime: 1, dwHighDateTime: 2 };
            let mut units = [0u16; 260];
            for (u, c) in units.iter_mut().zip(name.encode_utf16()) {
                *u = c;
            }
            d.cFileName = units;
            unsafe { std::slice::from_raw_parts(&d as *const _ as *const u8, size_of::<FILEDESCRIPTORW>()) }.to_vec()
        };
        let mut bytes = 2u32.to_le_bytes().to_vec();
        bytes.extend(entry("Photos", FD_ATTRIBUTES.0 as u32, FILE_ATTRIBUTE_DIRECTORY.0));
        bytes.extend(entry("Photos\\ü.jpg", FD_WRITESTIME.0 as u32, 0));
        let items = parse_descriptors(&bytes).unwrap();
        assert_eq!((items[0].name.as_str(), items[0].dir), ("Photos", true));
        assert_eq!((items[1].name.as_str(), items[1].dir), ("Photos\\ü.jpg", false));
        assert!(items[0].written.is_none() && items[1].written.is_some());
        // A count larger than the data is refused.
        bytes[0] = 3;
        assert!(parse_descriptors(&bytes).is_none());
    }

    #[test]
    fn names_from_sources_stay_inside_cairns_folder() {
        assert_eq!(safe_parts("a\\b/c.txt").unwrap(), ["a", "b", "c.txt"]);
        assert_eq!(safe_parts("..\\..\\Windows\\x.dll").unwrap(), ["Windows", "x.dll"]);
        assert_eq!(safe_parts("C:\\x").unwrap(), ["C_", "x"]);
        assert_eq!(safe_parts("re: why?.msg").unwrap(), ["re_ why_.msg"]);
        assert_eq!(safe_parts("con.txt").unwrap(), ["_con.txt"]);
        assert!(safe_parts("\\..\\.").is_none());
    }

    #[test]
    fn streams_are_copied_to_files() {
        let dir = std::env::temp_dir().join("cairn-stream-test");
        let _ = std::fs::create_dir_all(&dir);
        let data: Vec<u8> = (0..200_000u32).map(|i| i as u8).collect();
        let stream = unsafe { SHCreateMemStream(Some(&data)) }.unwrap();
        let target = dir.join("big.bin");
        write_stream(&stream, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), data);
    }

    #[test]
    fn dib_becomes_a_bmp_file() {
        // A 2×1 24-bit DIB: 40-byte header, no palette, one padded row.
        let mut dib = Vec::new();
        for v in [40u32, 2, 1] {
            dib.extend_from_slice(&v.to_le_bytes());
        }
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&24u16.to_le_bytes());
        dib.extend_from_slice(&[0; 24]);
        dib.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0]);
        let bmp = bmp_from_dib(&dib).unwrap();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(u32::from_le_bytes(bmp[2..6].try_into().unwrap()) as usize, bmp.len());
        // Pixels start right after the two headers.
        assert_eq!(u32::from_le_bytes(bmp[10..14].try_into().unwrap()), 54);
        // An 8-bit DIB has a 256-colour palette in between.
        dib[14] = 8;
        dib.extend_from_slice(&[0; 1024]);
        assert_eq!(u32::from_le_bytes(bmp_from_dib(&dib).unwrap()[10..14].try_into().unwrap()), 54 + 1024);
    }
}

