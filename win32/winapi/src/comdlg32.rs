//! Common dialogs. No dialogs are shown; every request is reported cancelled.

use runtime::Context;

#[win32_derive::dllexport]
pub fn GetOpenFileNameA(_ctx: &mut Context, _lpofn: u32) -> bool {
    log::warn!("GetOpenFileNameA: dialogs not supported");
    false
}

#[win32_derive::dllexport]
pub fn GetSaveFileNameA(_ctx: &mut Context, _lpofn: u32) -> bool {
    log::warn!("GetSaveFileNameA: dialogs not supported");
    false
}

#[win32_derive::dllexport]
pub fn GetFileTitleA(_ctx: &mut Context, _lpszFile: u32, _lpszTitle: u32, _cbBuf: u32) -> i32 {
    -1
}
