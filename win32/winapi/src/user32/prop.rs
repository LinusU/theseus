//! Window properties (SetProp/GetProp), a per-window string-keyed table.

use std::collections::HashMap;
use std::sync::Mutex;

use runtime::Context;

use crate::{Ptr, user32::HWND};

static PROPS: Mutex<Option<HashMap<(u32, String), u32>>> = Mutex::new(None);

/// The property name, which may be an atom (a small integer) rather than a
/// string pointer.
fn prop_name(ctx: &Context, lpString: Ptr<u8>) -> String {
    if lpString.addr < 0x10000 {
        format!("#{}", lpString.addr)
    } else {
        ctx.memory.read_str(lpString.addr).to_string()
    }
}

#[win32_derive::dllexport]
pub fn SetPropA(ctx: &mut Context, hWnd: HWND, lpString: Ptr<u8>, hData: u32) -> bool {
    let key = (hWnd.to_raw(), prop_name(ctx, lpString));
    PROPS.lock().unwrap().get_or_insert_with(Default::default).insert(key, hData);
    true
}

#[win32_derive::dllexport]
pub fn GetPropA(ctx: &mut Context, hWnd: HWND, lpString: Ptr<u8>) -> u32 {
    let key = (hWnd.to_raw(), prop_name(ctx, lpString));
    PROPS
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|props| props.get(&key).copied())
        .unwrap_or(0)
}

#[win32_derive::dllexport]
pub fn RemovePropA(ctx: &mut Context, hWnd: HWND, lpString: Ptr<u8>) -> u32 {
    let key = (hWnd.to_raw(), prop_name(ctx, lpString));
    PROPS
        .lock()
        .unwrap()
        .as_mut()
        .and_then(|props| props.remove(&key))
        .unwrap_or(0)
}
