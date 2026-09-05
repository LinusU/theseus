use runtime::Context;

use crate::{
    Ptr, stub,
    user32::{HINSTANCE, HWND},
};

#[win32_derive::dllexport]
pub fn DialogBoxParamA(
    _ctx: &mut Context,
    _hInstance: HINSTANCE,
    _lpTemplateName: Ptr<u8>,
    _hWndParent: HWND,
    _lpDialogFunc: Ptr<()>, /* DLGPROC */
    _dwInitParam: u32,
) -> i32 {
    stub!(1) // return value from dialog proc
}

// The emulated model has no dialogs: DialogBoxParam returns without
// creating one, so every dialog-item accessor and mutator fails.

#[win32_derive::dllexport]
pub fn DialogBoxParamW(
    _ctx: &mut Context,
    _hInstance: HINSTANCE,
    _lpTemplateName: Ptr<u16>, /* WSTR */
    _hWndParent: HWND,
    _lpDialogFunc: Ptr<()>, /* DLGPROC */
    _dwInitParam: u32,
) -> i32 {
    stub!(1) // same as DialogBoxParamA: no dialog is ever created
}

#[win32_derive::dllexport]
pub fn CheckDlgButton(
    _ctx: &mut Context,
    _hDlg: HWND,
    _nIDButton: i32,
    _uCheck: u32, /* DLG_BUTTON_CHECK_STATE */
) -> bool {
    false
}

#[win32_derive::dllexport]
pub fn EndDialog(_ctx: &mut Context, _hDlg: HWND, _nResult: i32) -> bool {
    false
}

#[win32_derive::dllexport]
pub fn SendDlgItemMessageA(
    _ctx: &mut Context,
    _hDlg: HWND,
    _nIDDlgItem: i32,
    _Msg: u32,
    _wParam: u32,
    _lParam: u32,
) -> u32 {
    crate::stub!(0)
}

#[win32_derive::dllexport]
pub fn IsDlgButtonChecked(_ctx: &mut Context, _hDlg: HWND, _nIDButton: i32) -> u32 {
    0 // BST_UNCHECKED
}

#[win32_derive::dllexport]
pub fn GetDlgItem(_ctx: &mut Context, _hDlg: HWND, _nIDDlgItem: i32) -> HWND {
    HWND::null()
}

#[win32_derive::dllexport]
pub fn GetDlgItemInt(
    ctx: &mut Context,
    _hDlg: HWND,
    _nIDDlgItem: i32,
    lpTranslated: Ptr<u32>,
    _bSigned: bool,
) -> u32 {
    if lpTranslated.addr != 0 {
        ctx.memory.write::<u32>(lpTranslated.addr, 0);
    }
    0
}

#[win32_derive::dllexport]
pub fn GetDlgItemTextW(
    ctx: &mut Context,
    _hDlg: HWND,
    _nIDDlgItem: i32,
    lpString: Ptr<u16>, /* WSTR */
    cchMax: i32,
) -> u32 {
    if lpString.addr != 0 && cchMax > 0 {
        ctx.memory.write::<u16>(lpString.addr, 0);
    }
    0
}

#[win32_derive::dllexport]
pub fn SetDlgItemInt(
    _ctx: &mut Context,
    _hDlg: HWND,
    _nIDDlgItem: i32,
    _uValue: u32,
    _bSigned: bool,
) -> bool {
    false
}

#[win32_derive::dllexport]
pub fn SetDlgItemTextW(
    _ctx: &mut Context,
    _hDlg: HWND,
    _nIDDlgItem: i32,
    _lpString: Ptr<u16>, /* WSTR */
) -> bool {
    false
}
