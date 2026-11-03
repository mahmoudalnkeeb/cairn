//! OLE drag and drop: the edge tab and the stack accept files, text and
//! links; items can be dragged out into any app.

use std::cell::Cell;
use std::mem::ManuallyDrop;
use std::path::PathBuf;

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::Shell::Common;
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

fn url_format() -> u16 {
    unsafe { windows::Win32::System::DataExchange::RegisterClipboardFormatW(w!("UniformResourceLocatorW")) as u16 }
}

fn format(cf: u16) -> FORMATETC {
    FORMATETC { cfFormat: cf, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 }
}

fn has(data: &IDataObject, cf: u16) -> bool {
    unsafe { data.QueryGetData(&format(cf)) == S_OK }
}

fn png_format() -> u16 {
    unsafe { windows::Win32::System::DataExchange::RegisterClipboardFormatW(w!("PNG")) as u16 }
}

fn supported(data: &IDataObject) -> bool {
    has(data, CF_HDROP.0) || has(data, url_format()) || has(data, CF_UNICODETEXT.0) || has(data, png_format()) || has(data, CF_DIB.0)
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

/// Files first, then a link, then plain text, then an image. Images come
/// last because apps like Excel offer a picture of copied cells next to the
/// text.
pub fn read(data: &IDataObject) -> Option<Dropped> {
    if has(data, CF_HDROP.0) {
        return read_paths(data).map(Dropped::Paths);
    }
    if let Some(text) = read_utf16(data, url_format()).or_else(|| read_utf16(data, CF_UNICODETEXT.0)) {
        return Some(Dropped::Text(text));
    }
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

#[implement(IDropTarget)]
struct DropTarget {
    zone: Zone,
    accept: Cell<bool>,
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(&self, data: Option<&IDataObject>, _: MODIFIERKEYS_FLAGS, _: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let ok = data.is_some_and(supported);
        self.accept.set(ok);
        unsafe {
            *effect = if ok { choose_effect(*effect) } else { DROPEFFECT_NONE };
        }
        crate::on_drag_hover(self.zone, ok);
        Ok(())
    }

    fn DragOver(&self, _: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        unsafe {
            *effect = if self.accept.get() { choose_effect(*effect) } else { DROPEFFECT_NONE };
        }
        if self.accept.get() {
            crate::on_drag_move(self.zone, POINT { x: pt.x, y: pt.y });
        }
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        crate::on_drag_hover(self.zone, false);
        Ok(())
    }

    fn Drop(&self, data: Option<&IDataObject>, _: MODIFIERKEYS_FLAGS, _: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        crate::on_drag_hover(self.zone, false);
        let dropped = data.and_then(read);
        unsafe {
            *effect = if dropped.is_some() { choose_effect(*effect) } else { DROPEFFECT_NONE };
        }
        if let Some(d) = dropped {
            crate::on_drop(self.zone, d);
        }
        Ok(())
    }
}

pub fn register(hwnd: HWND, zone: Zone) -> Result<()> {
    let target: IDropTarget = DropTarget { zone, accept: Cell::new(false) }.into();
    unsafe { RegisterDragDrop(hwnd, &target) }
}

#[implement(IDropSource)]
struct DropSource;

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
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

fn set_utf16(obj: &IDataObject, cf: u16, text: &str) -> Option<()> {
    let units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2).ok()?;
        let ptr = GlobalLock(global) as *mut u16;
        if ptr.is_null() {
            let _ = GlobalFree(global);
            return None;
        }
        std::ptr::copy_nonoverlapping(units.as_ptr(), ptr, units.len());
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

/// Runs the modal drag loop. Callers must not hold app state across it,
/// since window messages keep flowing while it runs.
pub fn drag_out(data: &IDataObject) -> DROPEFFECT {
    let source: IDropSource = DropSource.into();
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
        assert_eq!(paths, [a, b]);
    }

    #[test]
    fn several_texts_drag_out_as_lines() {
        let (t, l) = (Kind::Text("one".into()), Kind::Link("https://example.com".into()));
        let obj = data_object_many(&[&t, &l]).expect("data object");
        assert!(matches!(read(&obj), Some(Dropped::Text(s)) if s == "one\r\nhttps://example.com"));
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
