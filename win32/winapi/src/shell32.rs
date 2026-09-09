use runtime::Context;

use crate::user32::{HICON, HWND};

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

#[win32_derive::dllexport]
pub fn ShellExecuteA(
    _ctx: &mut Context,
    _hwnd: HWND,
    _lpOperation: u32,
    _lpFile: u32,
    _lpParameters: u32,
    _lpDirectory: u32,
    _nShowCmd: i32,
) -> u32 {
    33
}
