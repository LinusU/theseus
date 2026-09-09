use std::collections::HashMap;

use runtime::{Context, Memory};

use crate::{
    Ptr, stub,
    user32::{HINSTANCE, HWND, state},
};

/// A dialog created via CreateDialogIndirectParamA (or a DLGTEMPLATE's child
/// control). We don't run real control window procedures or draw anything
/// for these -- there's no host-backed surface behind them -- this only
/// tracks enough (mainly, control ids) for GetDlgItem and friends to find
/// their target instead of always getting the null HWND that made every
/// dialog interaction throw CNotSupportedException-and-worse.
pub enum DialogWindow {
    Dialog { controls: HashMap<i32, HWND> },
    Control { id: i32, class: String },
}

/// DS_SETFONT: the DLGTEMPLATE header is followed by a point size and
/// typeface name.
const DS_SETFONT: u32 = 0x40;

/// Reads one of a DLGTEMPLATE's "sz_Or_Ord" fields (menu/class/title names):
/// 0x0000 for none, 0xFFFF followed by a WORD ordinal (formatted as "#N"),
/// or a NUL-terminated UTF-16 string. Advances `*addr` past the field.
fn read_sz_or_ord(memory: &Memory, addr: &mut u32) -> String {
    match memory.read::<u16>(*addr) {
        0x0000 => {
            *addr += 2;
            String::new()
        }
        0xffff => {
            let ord = memory.read::<u16>(*addr + 2);
            *addr += 4;
            format!("#{ord}")
        }
        _ => {
            let s = memory.read_wstr(*addr);
            *addr += (s.len() as u32 + 1) * 2;
            s.to_string_lossy()
        }
    }
}

/// Parses a DLGTEMPLATE at `addr` -- the format CreateDialogIndirectParamA
/// (and DialogBoxIndirectParamA) receive: a plain, already-resolved pointer
/// to an in-memory template, not a resource name -- and registers a modeless
/// dialog "window" plus one entry per child control (from its
/// DLGITEMTEMPLATE array), keyed by control id.
///
/// Also delivers HCBT_CREATEWND for the new dialog, same as CreateWindowExA:
/// MFC's CDialog::CreateDlgIndirect relies entirely on that notification to
/// attach its CWnd wrapper's m_hWnd to the new HWND (there's no other path --
/// the raw HWND this function returns typically isn't the one the game's own
/// code goes on to call GetDlgItem with; it calls through the MFC CWnd, whose
/// m_hWnd only gets set from inside the hook).
pub fn create_dialog(ctx: &mut Context, addr: u32, hInstance: u32, hWndParent: HWND) -> HWND {
    let memory = &ctx.memory;

    // DLGTEMPLATEEX starts with wDlgVer=1, wSignature=0xFFFF, which can't
    // occur as the low/high words of a plain DLGTEMPLATE's `style` (no real
    // window/dialog style sets every bit of the upper word); that's also how
    // Windows itself tells the two formats apart. We only handle the plain
    // format, which is what older dialog editors (Visual C++ 4/5/6) emit.
    if memory.read::<u16>(addr) == 1 && memory.read::<u16>(addr + 2) == 0xffff {
        log::warn!("CreateDialogIndirectParamA: DLGTEMPLATEEX not supported");
        return HWND::null();
    }

    let style = memory.read::<u32>(addr);
    let mut cursor = addr + 8; // style, dwExtendedStyle
    let cdit = memory.read::<u16>(cursor);
    cursor += 2;
    let x = memory.read::<i16>(cursor) as i32;
    let y = memory.read::<i16>(cursor + 2) as i32;
    let cx = memory.read::<i16>(cursor + 4) as i32;
    let cy = memory.read::<i16>(cursor + 6) as i32;
    cursor += 8;

    read_sz_or_ord(memory, &mut cursor); // menu
    read_sz_or_ord(memory, &mut cursor); // class
    read_sz_or_ord(memory, &mut cursor); // title
    if style & DS_SETFONT != 0 {
        cursor += 2; // pointsize
        read_sz_or_ord(memory, &mut cursor); // typeface
    }

    let mut controls = HashMap::new();
    for _ in 0..cdit {
        cursor = cursor.next_multiple_of(4); // each item is DWORD-aligned
        cursor += 8 + 8; // style, dwExtendedStyle, then x/y/cx/cy
        let id = memory.read::<u16>(cursor) as i32;
        cursor += 2;
        let class = read_sz_or_ord(memory, &mut cursor);
        read_sz_or_ord(memory, &mut cursor); // title
        let extra_count = memory.read::<u16>(cursor);
        cursor += 2 + extra_count as u32;

        let hwnd = state()
            .dialog_windows
            .borrow_mut()
            .add(DialogWindow::Control { id, class });
        controls.insert(id, hwnd);
    }

    let hwnd = state()
        .dialog_windows
        .borrow_mut()
        .add(DialogWindow::Dialog { controls });

    let cs = super::CREATESTRUCTA {
        lpCreateParams: 0,
        hInstance,
        hMenu: 0,
        hwndParent: hWndParent.to_raw(),
        cy,
        cx,
        y,
        x,
        style,
        lpszName: 0,
        lpszClass: 0,
        dwExStyle: 0,
    };
    super::cbt_create_wnd(ctx, hwnd, &cs);

    hwnd
}

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

#[win32_derive::dllexport]
pub fn DialogBoxParamW(
    _ctx: &mut Context,
    _hInstance: HINSTANCE,
    _lpTemplateName: Ptr<u16>, /* WSTR */
    _hWndParent: HWND,
    _lpDialogFunc: Ptr<()>, /* DLGPROC */
    _dwInitParam: u32,
) -> i32 {
    todo!()
}

#[win32_derive::dllexport]
pub fn CheckDlgButton(
    _ctx: &mut Context,
    _hDlg: HWND,
    _nIDButton: i32,
    _uCheck: u32, /* DLG_BUTTON_CHECK_STATE */
) -> bool {
    todo!()
}

#[win32_derive::dllexport]
pub fn EndDialog(_ctx: &mut Context, hDlg: HWND, _nResult: i32) -> bool {
    // CreateDialogIndirectParamA's dialogs are modeless; EndDialog is really
    // for DialogBoxParamA-style modal ones, but a modeless dialog closing
    // itself this way isn't unheard of, so accept it rather than todo!()ing.
    state().dialog_windows.borrow_mut().remove(hDlg).is_some()
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
    todo!()
}

#[win32_derive::dllexport]
pub fn GetDlgItem(_ctx: &mut Context, hDlg: HWND, nIDDlgItem: i32) -> HWND {
    let dialogs = state().dialog_windows.borrow();
    if let Some(DialogWindow::Dialog { controls }) = dialogs.get(hDlg) {
        if let Some(&hwnd) = controls.get(&nIDDlgItem) {
            return hwnd;
        }
    }
    log::warn!("GetDlgItem({hDlg:?}, {nIDDlgItem}): not found");
    HWND::null()
}

#[win32_derive::dllexport]
pub fn GetDlgItemInt(
    _ctx: &mut Context,
    _hDlg: HWND,
    _nIDDlgItem: i32,
    _lpTranslated: Ptr<u32>,
    _bSigned: bool,
) -> u32 {
    todo!()
}

#[win32_derive::dllexport]
pub fn GetDlgItemTextW(
    _ctx: &mut Context,
    _hDlg: HWND,
    _nIDDlgItem: i32,
    _lpString: Ptr<u16>, /* WSTR */
    _cchMax: i32,
) -> u32 {
    todo!()
}

#[win32_derive::dllexport]
pub fn SetDlgItemInt(
    _ctx: &mut Context,
    _hDlg: HWND,
    _nIDDlgItem: i32,
    _uValue: u32,
    _bSigned: bool,
) -> bool {
    todo!()
}

#[win32_derive::dllexport]
pub fn SetDlgItemTextW(
    _ctx: &mut Context,
    _hDlg: HWND,
    _nIDDlgItem: i32,
    _lpString: Ptr<u16>, /* WSTR */
) -> bool {
    todo!()
}
