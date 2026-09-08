//! Menus. Windows here have no menu bar, so every menu query reports "no such
//! item"; programs building menus at startup carry on without one.

use runtime::Context;

use crate::{
    Ptr,
    user32::{HMENU, HWND},
};

#[win32_derive::dllexport]
pub fn GetMenu(_ctx: &mut Context, _hWnd: HWND) -> HMENU {
    0
}

#[win32_derive::dllexport]
pub fn GetSubMenu(_ctx: &mut Context, _hMenu: HMENU, _nPos: i32) -> HMENU {
    0
}

#[win32_derive::dllexport]
pub fn GetMenuItemCount(_ctx: &mut Context, _hMenu: HMENU) -> i32 {
    0
}

#[win32_derive::dllexport]
pub fn GetMenuItemID(_ctx: &mut Context, _hMenu: HMENU, _nPos: i32) -> u32 {
    0xffff_ffff
}

#[win32_derive::dllexport]
pub fn GetMenuState(_ctx: &mut Context, _hMenu: HMENU, _uId: u32, _uFlags: u32) -> u32 {
    0xffff_ffff
}

#[win32_derive::dllexport]
pub fn GetMenuStringA(
    _ctx: &mut Context,
    _hMenu: HMENU,
    _uIDItem: u32,
    _lpString: Ptr<u8>,
    _cchMax: i32,
    _flags: u32,
) -> i32 {
    0
}

#[win32_derive::dllexport]
pub fn InsertMenuA(
    _ctx: &mut Context,
    _hMenu: HMENU,
    _uPosition: u32,
    _uFlags: u32,
    _uIDNewItem: u32,
    _lpNewItem: Ptr<u8>,
) -> bool {
    false
}

#[win32_derive::dllexport]
pub fn ModifyMenuA(
    _ctx: &mut Context,
    _hMnu: HMENU,
    _uPosition: u32,
    _uFlags: u32,
    _uIDNewItem: u32,
    _lpNewItem: Ptr<u8>,
) -> bool {
    false
}

#[win32_derive::dllexport]
pub fn DeleteMenu(_ctx: &mut Context, _hMenu: HMENU, _uPosition: u32, _uFlags: u32) -> bool {
    false
}

#[win32_derive::dllexport]
pub fn DestroyMenu(_ctx: &mut Context, _hMenu: HMENU) -> bool {
    true
}

#[win32_derive::dllexport]
pub fn EnableMenuItem(_ctx: &mut Context, _hMenu: HMENU, _uIDEnableItem: u32, _uEnable: u32) -> u32 {
    0xffff_ffff // item does not exist
}

#[win32_derive::dllexport]
pub fn SetMenuItemBitmaps(
    _ctx: &mut Context,
    _hMenu: HMENU,
    _uPosition: u32,
    _uFlags: u32,
    _hBitmapUnchecked: u32,
    _hBitmapChecked: u32,
) -> bool {
    false
}

#[win32_derive::dllexport]
pub fn GetMenuCheckMarkDimensions(_ctx: &mut Context) -> u32 {
    (13 << 16) | 13
}
