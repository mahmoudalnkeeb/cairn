//! Screen reader support for the stack: an MSAA `IAccessible` for the
//! flyout with one list item per tile, so Narrator and NVDA can read names,
//! kinds, selection and focus, and open items.

use std::ffi::c_void;

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::shelf::{self, Kind};
use crate::stackview;

const CHILDID_SELF: i32 = 0;
// oleacc.h values; this crate only exposes them under another feature.
const STATE_FOCUSABLE: u32 = 0x0010_0000;
const STATE_OFFSCREEN: u32 = 0x0001_0000;
const STATE_INVISIBLE: u32 = 0x0000_8000;

#[implement(IAccessible)]
struct StackAccessible {
    hwnd: HWND,
}

/// The item a child id names: child ids are 1-based, 0 is the list itself.
fn item(v: &VARIANT) -> Result<Option<usize>> {
    let id = i32::try_from(v).map_err(|_| Error::from(E_INVALIDARG))?;
    let n = crate::app_ref().map_or(0, |a| a.stack.items.len()) as i32;
    match id {
        CHILDID_SELF => Ok(None),
        k if (1..=n).contains(&k) => Ok(Some(k as usize - 1)),
        _ => Err(E_INVALIDARG.into()),
    }
}

fn not_here<T>() -> Result<T> {
    Err(Error::from(S_FALSE))
}

impl IDispatch_Impl for StackAccessible_Impl {
    fn GetTypeInfoCount(&self) -> Result<u32> {
        Ok(0)
    }
    fn GetTypeInfo(&self, _: u32, _: u32) -> Result<ITypeInfo> {
        Err(E_NOTIMPL.into())
    }
    fn GetIDsOfNames(&self, _: *const GUID, _: *const PCWSTR, _: u32, _: u32, _: *mut i32) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn Invoke(&self, _: i32, _: *const GUID, _: u32, _: DISPATCH_FLAGS, _: *const DISPPARAMS, _: *mut VARIANT, _: *mut EXCEPINFO, _: *mut u32) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}

impl IAccessible_Impl for StackAccessible_Impl {
    fn accParent(&self) -> Result<IDispatch> {
        let mut out: *mut c_void = std::ptr::null_mut();
        unsafe {
            CreateStdAccessibleObject(self.hwnd, OBJID_WINDOW.0, &IDispatch::IID, &mut out)?;
            Ok(IDispatch::from_raw(out))
        }
    }

    fn accChildCount(&self) -> Result<i32> {
        Ok(crate::app_ref().map_or(0, |a| a.stack.items.len()) as i32)
    }

    fn get_accChild(&self, _: &VARIANT) -> Result<IDispatch> {
        // Tiles are simple elements, not objects of their own.
        not_here()
    }

    fn get_accName(&self, v: &VARIANT) -> Result<BSTR> {
        let app = crate::app_ref().ok_or(Error::from(E_FAIL))?;
        Ok(match item(v)? {
            None => {
                let (n, name) = (app.stack.items.len(), &app.stack.names[app.stack.current]);
                BSTR::from(if n == 1 { format!("{name}, 1 item") } else { format!("{name}, {n} items") })
            }
            Some(i) => {
                let kind = &app.stack.items[i].kind;
                BSTR::from(format!("{}, {}", shelf::name(kind), shelf::kind_label(kind)))
            }
        })
    }

    fn get_accValue(&self, v: &VARIANT) -> Result<BSTR> {
        let app = crate::app_ref().ok_or(Error::from(E_FAIL))?;
        match item(v)?.map(|i| &app.stack.items[i].kind) {
            Some(Kind::File(p)) => Ok(BSTR::from(p.to_string_lossy().as_ref())),
            Some(Kind::Link(l)) => Ok(BSTR::from(l.as_str())),
            _ => not_here(),
        }
    }

    fn get_accDescription(&self, _: &VARIANT) -> Result<BSTR> {
        not_here()
    }

    fn get_accRole(&self, v: &VARIANT) -> Result<VARIANT> {
        let role = if item(v)?.is_some() { ROLE_SYSTEM_LISTITEM } else { ROLE_SYSTEM_LIST };
        Ok(VARIANT::from(role as i32))
    }

    fn get_accState(&self, v: &VARIANT) -> Result<VARIANT> {
        let app = crate::app_ref().ok_or(Error::from(E_FAIL))?;
        let focused = unsafe { GetFocus() } == self.hwnd;
        let state = match item(v)? {
            None => STATE_FOCUSABLE | if focused && app.stack.items.is_empty() { STATE_SYSTEM_FOCUSED } else { 0 },
            Some(i) => {
                let mut s = STATE_FOCUSABLE | STATE_SYSTEM_SELECTABLE | STATE_SYSTEM_MULTISELECTABLE;
                if stackview::is_selected(app, i) {
                    s |= STATE_SYSTEM_SELECTED;
                }
                if focused && stackview::focus_index(app) == i {
                    s |= STATE_SYSTEM_FOCUSED;
                }
                if stackview::tile_screen_rect(app, i).is_none() {
                    s |= STATE_OFFSCREEN | STATE_INVISIBLE;
                }
                s
            }
        };
        Ok(VARIANT::from(state as i32))
    }

    fn get_accHelp(&self, _: &VARIANT) -> Result<BSTR> {
        not_here()
    }

    fn get_accHelpTopic(&self, _: *mut BSTR, _: &VARIANT) -> Result<i32> {
        Err(E_NOTIMPL.into())
    }

    fn get_accKeyboardShortcut(&self, _: &VARIANT) -> Result<BSTR> {
        not_here()
    }

    fn accFocus(&self) -> Result<VARIANT> {
        let app = crate::app_ref().ok_or(Error::from(E_FAIL))?;
        if unsafe { GetFocus() } != self.hwnd {
            return Ok(VARIANT::default());
        }
        if app.stack.items.is_empty() {
            return Ok(VARIANT::from(CHILDID_SELF));
        }
        Ok(VARIANT::from(stackview::focus_index(app) as i32 + 1))
    }

    fn accSelection(&self) -> Result<VARIANT> {
        let app = crate::app_ref().ok_or(Error::from(E_FAIL))?;
        // One selected tile is reported directly; for several, readers fall
        // back to each tile's selected state.
        let selected: Vec<usize> = (0..app.stack.items.len()).filter(|&i| stackview::is_selected(app, i)).collect();
        Ok(match selected[..] {
            [one] => VARIANT::from(one as i32 + 1),
            _ => VARIANT::default(),
        })
    }

    fn get_accDefaultAction(&self, v: &VARIANT) -> Result<BSTR> {
        let app = crate::app_ref().ok_or(Error::from(E_FAIL))?;
        match item(v)?.map(|i| &app.stack.items[i].kind) {
            Some(Kind::Text(_)) => Ok(BSTR::from("Paste")),
            Some(_) => Ok(BSTR::from("Open")),
            None => not_here(),
        }
    }

    fn accSelect(&self, flags: i32, v: &VARIANT) -> Result<()> {
        let app = crate::app_mut().ok_or(Error::from(E_FAIL))?;
        let Some(i) = item(v)? else { return Err(E_INVALIDARG.into()) };
        if flags & (SELFLAG_TAKEFOCUS | SELFLAG_TAKESELECTION) as i32 != 0 {
            stackview::take_focus(app, i);
        }
        Ok(())
    }

    fn accLocation(&self, left: *mut i32, top: *mut i32, width: *mut i32, height: *mut i32, v: &VARIANT) -> Result<()> {
        let app = crate::app_ref().ok_or(Error::from(E_FAIL))?;
        let r = match item(v)? {
            Some(i) => stackview::tile_screen_rect(app, i),
            None => {
                let mut r = RECT::default();
                unsafe { GetWindowRect(self.hwnd, &mut r).ok().map(|_| r) }
            }
        };
        let Some(r) = r else { return not_here() };
        unsafe {
            *left = r.left;
            *top = r.top;
            *width = r.right - r.left;
            *height = r.bottom - r.top;
        }
        Ok(())
    }

    fn accNavigate(&self, _: i32, _: &VARIANT) -> Result<VARIANT> {
        Err(E_NOTIMPL.into())
    }

    fn accHitTest(&self, x: i32, y: i32) -> Result<VARIANT> {
        let app = crate::app_ref().ok_or(Error::from(E_FAIL))?;
        Ok(match stackview::hit_screen(app, POINT { x, y }) {
            Some(i) => VARIANT::from(i as i32 + 1),
            None => VARIANT::from(CHILDID_SELF),
        })
    }

    fn accDoDefaultAction(&self, v: &VARIANT) -> Result<()> {
        let app = crate::app_mut().ok_or(Error::from(E_FAIL))?;
        let Some(i) = item(v)? else { return Err(E_INVALIDARG.into()) };
        stackview::activate(app, i);
        Ok(())
    }

    fn put_accName(&self, _: &VARIANT, _: &BSTR) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn put_accValue(&self, _: &VARIANT, _: &BSTR) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}

/// Answers WM_GETOBJECT for the stack window's client area.
pub fn get_object(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    if lparam.0 as i32 != OBJID_CLIENT.0 {
        return None;
    }
    let acc: IAccessible = StackAccessible { hwnd }.into();
    Some(unsafe { LresultFromObject(&IAccessible::IID, wparam, &acc) })
}

/// Tells screen readers the keyboard focus moved to tile `focus`.
pub fn focus_changed(hwnd: HWND, focus: Option<usize>) {
    let child = focus.map_or(CHILDID_SELF, |i| i as i32 + 1);
    unsafe {
        NotifyWinEvent(EVENT_OBJECT_FOCUS, hwnd, OBJID_CLIENT.0, child);
    }
}
