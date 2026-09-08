//! Common controls. Only the entry points a framework touches at startup.

use runtime::Context;

/// comctl32.dll exports InitCommonControls as ordinal 17.
#[win32_derive::dllexport]
pub fn ordinal17(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn ImageList_Destroy(_ctx: &mut Context, _himl: u32) -> bool {
    false
}
