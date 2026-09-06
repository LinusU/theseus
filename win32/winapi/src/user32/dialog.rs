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
    // lpTranslated receives FALSE on failure; an unusable pointer is ignored.
    if crate::ddraw::guest_range(ctx, lpTranslated.addr, std::mem::size_of::<u32>() as u32) {
        lpTranslated.write(&mut ctx.memory, 0);
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
    // On failure the buffer receives an empty string; an unusable pointer is
    // ignored rather than panicking the host.
    if cchMax > 0
        && crate::ddraw::guest_range(ctx, lpString.addr, std::mem::size_of::<u16>() as u32)
    {
        lpString.write(&mut ctx.memory, 0);
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

#[cfg(test)]
mod tests {
    use super::{GetDlgItemInt, GetDlgItemTextW};
    use crate::{Ptr, user32::HWND};
    use runtime::{BlockCache, CPU, Context, Memory};

    fn context() -> Context {
        Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x4000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    #[test]
    fn dialog_out_pointers_tolerate_bad_addresses() {
        let mut ctx = context();
        // An out-of-range optional out-pointer is ignored, not a panic.
        assert_eq!(
            GetDlgItemInt(&mut ctx, HWND::null(), 0, Ptr::new(0xffff_ff00), false),
            0
        );
        // A low out-pointer is also ignored.
        assert_eq!(
            GetDlgItemInt(&mut ctx, HWND::null(), 0, Ptr::new(0x500), false),
            0
        );
        // A valid out-pointer still receives FALSE.
        assert_eq!(
            GetDlgItemInt(&mut ctx, HWND::null(), 0, Ptr::new(0x2000), false),
            0
        );
        assert_eq!(ctx.memory.read::<u32>(0x2000), 0);

        // Same for the text out-buffer: bad addresses are ignored.
        assert_eq!(
            GetDlgItemTextW(&mut ctx, HWND::null(), 0, Ptr::new(0xffff_ff00), 16),
            0
        );
        assert_eq!(
            GetDlgItemTextW(&mut ctx, HWND::null(), 0, Ptr::new(0x500), 16),
            0
        );
        assert_eq!(
            GetDlgItemTextW(&mut ctx, HWND::null(), 0, Ptr::new(0x2000), 16),
            0
        );
        assert_eq!(ctx.memory.read::<u16>(0x2000), 0);
    }
}
