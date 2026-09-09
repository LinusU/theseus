use runtime::Context;

use crate::Ptr;

#[win32_derive::dllexport]
pub fn QSWaveMixGetDirectSound(ctx: &mut Context, lpDirectSound: Ptr<u32>) -> u32 {
    crate::dsound::DirectSoundCreate(ctx, 0, lpDirectSound.addr, 0)
}

#[win32_derive::dllexport]
pub fn QSWaveMixInitEx(_ctx: &mut Context, _options: Ptr<u8>) -> u32 {
    1
}

#[win32_derive::dllexport]
pub fn QSWaveMixActivate(_ctx: &mut Context, _channel: u32, _active: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixOpenChannel(_ctx: &mut Context, _channel: u32, _wave: u32, _flags: u32) -> u32 {
    0
}
