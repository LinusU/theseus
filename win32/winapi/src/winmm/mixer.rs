//! Audio mixer API. No mixer devices are reported; programs fall back to
//! adjusting volume themselves.

use runtime::Context;

const MMSYSERR_NODRIVER: u32 = 6;

#[win32_derive::dllexport]
pub fn mixerGetNumDevs(_ctx: &mut Context) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn mixerOpen(
    _ctx: &mut Context,
    _phmx: u32,
    _uMxId: u32,
    _dwCallback: u32,
    _dwInstance: u32,
    _fdwOpen: u32,
) -> u32 {
    MMSYSERR_NODRIVER
}

#[win32_derive::dllexport]
pub fn mixerClose(_ctx: &mut Context, _hmx: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn mixerGetLineInfoA(_ctx: &mut Context, _hmxobj: u32, _pmxl: u32, _fdwInfo: u32) -> u32 {
    MMSYSERR_NODRIVER
}

#[win32_derive::dllexport]
pub fn mixerGetLineControlsA(_ctx: &mut Context, _hmxobj: u32, _pmxlc: u32, _fdwControls: u32) -> u32 {
    MMSYSERR_NODRIVER
}

#[win32_derive::dllexport]
pub fn mixerGetControlDetailsA(_ctx: &mut Context, _hmxobj: u32, _pmxcd: u32, _fdwDetails: u32) -> u32 {
    MMSYSERR_NODRIVER
}

#[win32_derive::dllexport]
pub fn mixerSetControlDetails(_ctx: &mut Context, _hmxobj: u32, _pmxcd: u32, _fdwDetails: u32) -> u32 {
    MMSYSERR_NODRIVER
}
