use runtime::Context;

use crate::kernel32::HMODULE;

#[win32_derive::dllexport]
pub fn PlaySoundW(
    _ctx: &mut Context,
    _pszSound: u32, /* WSTR */
    _hmod: HMODULE,
    _fdwSound: u32, /* SND_FLAGS */
) -> bool {
    todo!()
}

#[win32_derive::dllexport]
pub fn mciGetErrorStringA(_ctx: &mut Context, _mcierr: u32, _pszText: u32, _cchText: u32) -> bool {
    false
}
