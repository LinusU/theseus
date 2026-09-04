use runtime::Context;

const E_NOTIMPL: u32 = 0x8000_4001;

#[win32_derive::dllexport]
pub fn ordinal1(_ctx: &mut Context, _lpGuid: u32, _lplpDirectPlay: u32, _pUnkOuter: u32) -> u32 {
    E_NOTIMPL
}
