//! DirectPlay (networking). Not supported: creating a DirectPlay object fails
//! and no service providers are enumerated, so games disable multiplayer.

use runtime::Context;

const DPERR_UNAVAILABLE: u32 = 0x8877_0050;

/// dplayx.dll exports DirectPlayCreate as ordinal 1.
#[win32_derive::dllexport]
pub fn ordinal1(_ctx: &mut Context, _lpGUID: u32, _lplpDP: u32, _pUnk: u32) -> u32 {
    log::warn!("DirectPlayCreate: DirectPlay not supported");
    DPERR_UNAVAILABLE
}

/// dplayx.dll exports DirectPlayEnumerateA as ordinal 2.
#[win32_derive::dllexport]
pub fn ordinal2(_ctx: &mut Context, _lpEnumDPCallback: u32, _lpContext: u32) -> u32 {
    0 // DP_OK, with nothing enumerated
}
