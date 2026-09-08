//! Joystick API. No joysticks are reported.

use runtime::Context;

const JOYERR_PARMS: u32 = 165;
const JOYERR_UNPLUGGED: u32 = 167;

#[win32_derive::dllexport]
pub fn joyGetNumDevs(_ctx: &mut Context) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn joyGetDevCapsA(_ctx: &mut Context, _uJoyID: u32, _pjc: u32, _cbjc: u32) -> u32 {
    JOYERR_PARMS
}

#[win32_derive::dllexport]
pub fn joyGetPosEx(_ctx: &mut Context, _uJoyID: u32, _pji: u32) -> u32 {
    JOYERR_UNPLUGGED
}
