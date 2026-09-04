use runtime::Context;

#[win32_derive::dllexport]
pub fn ImmGetContext(_ctx: &mut Context, _hWnd: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn ImmNotifyIME(
    _ctx: &mut Context,
    _hIMC: u32,
    _dwAction: u32,
    _dwIndex: u32,
    _dwValue: u32,
) -> bool {
    false
}

#[win32_derive::dllexport]
pub fn ImmGetDefaultIMEWnd(_ctx: &mut Context, _hWnd: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn ImmSetCompositionWindow(_ctx: &mut Context, _hIMC: u32, _lpCompForm: u32) -> bool {
    false
}

#[win32_derive::dllexport]
pub fn ImmAssociateContext(_ctx: &mut Context, _hWnd: u32, _hIMC: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn ImmDestroyContext(_ctx: &mut Context, _hIMC: u32) -> bool {
    false
}
