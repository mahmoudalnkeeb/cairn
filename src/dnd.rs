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
use windows::core::*;

use crate::shelf::{Dropped, Kind};

#[derive(Clone, Copy, PartialEq)]
pub enum Zone {
    Tab,
    Stack,
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

fn supported(data: &IDataObject) -> bool {
    has(data, CF_HDROP.0) || has(data, url_format()) || has(data, CF_UNICODETEXT.0)
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

/// Files first, then a link, then plain text.
pub fn read(data: &IDataObject) -> Option<Dropped> {
    if has(data, CF_HDROP.0) {
        return read_paths(data).map(Dropped::Paths);
    }
    read_utf16(data, url_format()).or_else(|| read_utf16(data, CF_UNICODETEXT.0)).map(Dropped::Text)
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

    fn DragOver(&self, _: MODIFIERKEYS_FLAGS, _: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        unsafe {
            *effect = if self.accept.get() { choose_effect(*effect) } else { DROPEFFECT_NONE };
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
