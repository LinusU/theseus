use runtime::Context;

use crate::{
    Ptr,
    user32::{HICON, HINSTANCE, HWND},
};

#[win32_derive::dllexport]
pub fn ShellExecuteA(
    ctx: &mut Context,
    _hWnd: HWND,
    lpOperation: Ptr<u8>,
    lpFile: Ptr<u8>,
    lpParameters: Ptr<u8>,
    lpDirectory: Ptr<u8>,
    _nShowCmd: i32,
) -> HINSTANCE {
    const SE_ERR_FNF: HINSTANCE = 2;
    const SE_ERR_NOASSOC: HINSTANCE = 31;

    let Some(file) = (lpFile.addr != 0).then(|| ctx.memory.read_str(lpFile.addr)) else {
        return SE_ERR_FNF;
    };
    let operation = (lpOperation.addr != 0).then(|| ctx.memory.read_str(lpOperation.addr));
    let parameters = (lpParameters.addr != 0).then(|| ctx.memory.read_str(lpParameters.addr));
    let directory = (lpDirectory.addr != 0).then(|| ctx.memory.read_str(lpDirectory.addr));
    log::warn!(
        "ShellExecuteA is unavailable: operation={operation:?}, file={file:?}, parameters={parameters:?}, directory={directory:?}"
    );
    SE_ERR_NOASSOC
}

#[win32_derive::dllexport]
pub fn ShellAboutW(
    _ctx: &mut Context,
    _hWnd: HWND,
    _szApp: u32,        /* WSTR */
    _szOtherStuff: u32, /* WSTR */
    _hIcon: HICON,
) -> i32 {
    todo!()
}
