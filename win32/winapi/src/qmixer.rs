use runtime::Context;

use crate::Ptr;

#[win32_derive::dllexport]
pub fn QSWaveMixGetDirectSound(_ctx: &mut Context, _hSession: u32, lpDirectSound: Ptr<u32>) -> u32 {
    crate::dsound::DirectSoundCreate(_ctx, 0, lpDirectSound.addr, 0)
}

#[win32_derive::dllexport]
pub fn QSWaveMixInitEx(_ctx: &mut Context, _options: u32) -> u32 {
    1
}

#[win32_derive::dllexport]
pub fn QSWaveMixActivate(_ctx: &mut Context, _hSession: u32, _active: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixOpenChannel(_ctx: &mut Context, _hSession: u32, _index: u32, _flags: u32) -> u32 {
    1
}

#[win32_derive::dllexport]
pub fn QSWaveMixOpenWaveEx(_ctx: &mut Context, _hSession: u32, _wave: u32, _flags: u32) -> u32 {
    1
}

#[win32_derive::dllexport]
pub fn QSWaveMixFreeWave(_ctx: &mut Context, _hSession: u32, _hWave: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixPlayEx(
    _ctx: &mut Context,
    _hSession: u32,
    _iChannel: u32,
    _flags: u32,
    _hWave: u32,
    _loops: u32,
    _params: u32,
) -> u32 {
    1
}

#[win32_derive::dllexport]
pub fn QSWaveMixConfigureChannel(_ctx: &mut Context, _hSession: u32, _hChannel: u32, _what: u32, _value: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixEnableChannel(_ctx: &mut Context, _hSession: u32, _hChannel: u32, _enabled: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixCloseSession(_ctx: &mut Context, _hSession: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixPump(_ctx: &mut Context) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixRestartChannel(_ctx: &mut Context, _hSession: u32, _hChannel: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixPauseChannel(_ctx: &mut Context, _hSession: u32, _hChannel: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixStopChannel(_ctx: &mut Context, _hSession: u32, _hChannel: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixSetOptions(_ctx: &mut Context, _hSession: u32, _options: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixSetSpeakerPlacement(_ctx: &mut Context, _hSession: u32, _placement: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixSetSpeedOfSound(_ctx: &mut Context, _hSession: u32, _speed: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixSetPanRate(_ctx: &mut Context, _hSession: u32, _hChannel: u32, _rate: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixSetListenerOrientation(_ctx: &mut Context, _hSession: u32, _frontX: u32, _frontY: u32, _topZ: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixSetListenerPosition(_ctx: &mut Context, _hSession: u32, _x: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixSetVolume(_ctx: &mut Context, _hSession: u32, _hChannel: u32, _volume: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixSetDistanceMapping(_ctx: &mut Context, _hSession: u32, _start: u32, _end: u32, _pMap: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixSetSourceCone(_ctx: &mut Context, _hSession: u32, _hChannel: u32, _innerAngle: u32, _outerAngle: u32, _outerVolume: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixSetFrequency(_ctx: &mut Context, _hSession: u32, _hChannel: u32, _freq: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixSetListenerVelocity(_ctx: &mut Context, _hSession: u32, _x: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixSetSourceVelocity(_ctx: &mut Context, _hSession: u32, _hChannel: u32, _x: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixSetSourcePosition(_ctx: &mut Context, _hSession: u32, _hChannel: u32, _x: u32, _flags: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn QSWaveMixSetPosition(_ctx: &mut Context, _hSession: u32, _hChannel: u32, _pos: u32, _flags: u32) -> u32 {
    0
}
