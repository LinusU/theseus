//! Video for Windows (MSVFW32) codec manager.
//!
//! The game loads this DLL to play the intro movie. There are no codecs
//! installed, so every open/locate/decompress call reports failure, which lets
//! the game fall back to its existing no-movie path.

use runtime::Context;

const ICERR_OK: u32 = 0;

#[win32_derive::dllexport]
pub fn MCIWndCreate(
    _ctx: &mut Context,
    _hwndParent: u32,
    _hInstance: u32,
    _dwStyle: u32,
    _szFileName: u32,
) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn ICOpen(_ctx: &mut Context, _fccType: u32, _fccHandler: u32, _wMode: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn ICClose(_ctx: &mut Context, _hic: u32) -> u32 {
    ICERR_OK
}

#[win32_derive::dllexport]
pub fn ICImageDecompress(
    _ctx: &mut Context,
    _hic: u32,
    _uiFlags: u32,
    _lpbiIn: u32,
    _lpBits: u32,
    _lpbiOut: u32,
) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn ICImageCompress(
    _ctx: &mut Context,
    _hic: u32,
    _dwFlags: u32,
    _lpbiOutput: u32,
    _lpOutputData: u32,
    _lpbiInput: u32,
    _lpInputData: u32,
    _lpckid: u32,
) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn ICGetInfo(_ctx: &mut Context, _hic: u32, _lpicinfo: u32, _cb: u32) -> u32 {
    ICERR_OK
}

#[win32_derive::dllexport]
pub fn ICInfo(_ctx: &mut Context, _fccType: u32, _fccHandler: u32, _lpicinfo: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn ICLocate(
    _ctx: &mut Context,
    _fccType: u32,
    _fccHandler: u32,
    _lpbiIn: u32,
    _lpbiOut: u32,
    _wFlags: u32,
) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn ICGetDisplayFormat(
    _ctx: &mut Context,
    _hic: u32,
    _lpbiIn: u32,
    _lpbiOut: u32,
    _BitDepth: u32,
    _dx: u32,
    _dy: u32,
) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn ICSendMessage(_ctx: &mut Context, _hic: u32, _msg: u32, _dw1: u32, _dw2: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn ICOpenFunction(
    _ctx: &mut Context,
    _fccType: u32,
    _fccHandler: u32,
    _wMode: u32,
    _lpfnHandler: u32,
) -> u32 {
    0
}
