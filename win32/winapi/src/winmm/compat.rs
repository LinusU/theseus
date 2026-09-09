use runtime::Context;

use crate::Ptr;

#[win32_derive::dllexport]
pub fn auxGetDevCapsA(_ctx: &mut Context, _uDeviceID: u32, _lpCaps: Ptr<u8>, _uSize: u32) -> u32 {
    1
}

#[win32_derive::dllexport]
pub fn auxGetNumDevs(_ctx: &mut Context) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn auxGetVolume(_ctx: &mut Context, _uDeviceID: u32, lpdwVolume: Ptr<u32>) -> u32 {
    if lpdwVolume.addr != 0 {
        lpdwVolume.write(&mut _ctx.memory, 0xffff_ffff);
    }
    0
}

#[win32_derive::dllexport]
pub fn auxSetVolume(_ctx: &mut Context, _uDeviceID: u32, _dwVolume: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn midiOutClose(_ctx: &mut Context, _hmo: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn midiOutGetDevCapsA(
    _ctx: &mut Context,
    _uDeviceID: u32,
    _lpCaps: Ptr<u8>,
    _uSize: u32,
) -> u32 {
    1
}

#[win32_derive::dllexport]
pub fn midiOutGetNumDevs(_ctx: &mut Context) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn midiOutLongMsg(_ctx: &mut Context, _hmo: u32, _lpMidiOutHdr: Ptr<u8>, _uSize: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn midiOutOpen(
    _ctx: &mut Context,
    _lphmo: Ptr<u32>,
    _uDeviceID: u32,
    _dwCallback: u32,
    _dwInstance: u32,
    _fdwOpen: u32,
) -> u32 {
    1
}

#[win32_derive::dllexport]
pub fn midiOutPrepareHeader(
    _ctx: &mut Context,
    _hmo: u32,
    _lpMidiOutHdr: Ptr<u8>,
    _uSize: u32,
) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn midiOutShortMsg(_ctx: &mut Context, _hmo: u32, _dwMsg: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn mmioSetBuffer(
    _ctx: &mut Context,
    _hmmio: u32,
    _pchBuffer: Ptr<u8>,
    _cchBuffer: u32,
    _fuBuffer: u32,
) -> u32 {
    0
}
