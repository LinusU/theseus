use runtime::Context;

use crate::kernel32;

const MMSYSERR_NOERROR: u32 = 0;
const MMSYSERR_BADDEVICEID: u32 = 2;
const MMSYSERR_INVALHANDLE: u32 = 5;
const MMSYSERR_INVALPARAM: u32 = 11;
const MIXER_DEVICE_ID: u32 = 0;

#[win32_derive::dllexport]
pub fn mixerGetNumDevs(_ctx: &mut Context) -> u32 {
    1
}

#[repr(C)]
#[derive(Debug, zerocopy::Immutable, zerocopy::IntoBytes)]
struct MIXERCAPSA {
    wMid: u16,
    wPid: u16,
    vDriverVersion: u32,
    szPname: [u8; 32],
    fdwSupport: u32,
    cDestinations: u32,
}

#[win32_derive::dllexport]
pub fn mixerGetDevCapsA(ctx: &mut Context, uMxId: u32, pmxcaps: u32, cbmxcaps: u32) -> u32 {
    if uMxId != MIXER_DEVICE_ID {
        return MMSYSERR_BADDEVICEID;
    }
    if cbmxcaps < std::mem::size_of::<MIXERCAPSA>() as u32 {
        return MMSYSERR_INVALPARAM;
    }
    let mut szPname = [0; 32];
    szPname[..13].copy_from_slice(b"Theseus Mixer");
    ctx.memory.write(
        pmxcaps,
        MIXERCAPSA {
            wMid: 0,
            wPid: 0,
            vDriverVersion: 1,
            szPname,
            fdwSupport: 0,
            cDestinations: 1,
        },
    );
    MMSYSERR_NOERROR
}

#[repr(C)]
#[derive(Debug, zerocopy::Immutable, zerocopy::IntoBytes)]
struct MIXERLINE_TARGETA {
    dwType: u32,
    dwDeviceID: u32,
    wMid: u16,
    wPid: u16,
    vDriverVersion: u32,
    szPname: [u8; 32],
}

#[repr(C)]
#[derive(Debug, zerocopy::Immutable, zerocopy::IntoBytes)]
struct MIXERLINEA {
    cbStruct: u32,
    dwDestination: u32,
    dwSource: u32,
    dwLineID: u32,
    fdwLine: u32,
    dwUser: u32,
    dwComponentType: u32,
    cChannels: u32,
    cConnections: u32,
    cControls: u32,
    szShortName: [u8; 16],
    szName: [u8; 64],
    Target: MIXERLINE_TARGETA,
}

#[win32_derive::dllexport]
pub fn mixerGetLineInfoA(ctx: &mut Context, hmxobj: u32, pmxl: u32, fdwInfo: u32) -> u32 {
    let hmxobj = crate::HANDLE::from_raw(hmxobj);
    let state = kernel32::lock();
    if !matches!(state.objects.get(hmxobj), Some(kernel32::Object::Mixer)) {
        return MMSYSERR_INVALHANDLE;
    }
    drop(state);
    if pmxl < 0x1000 || ctx.memory.read::<u32>(pmxl) < std::mem::size_of::<MIXERLINEA>() as u32 {
        return MMSYSERR_INVALPARAM;
    }
    let component_type = ctx.memory.read::<u32>(pmxl + 0x18);
    if fdwInfo != 0 && fdwInfo != 3 {
        return MMSYSERR_INVALPARAM;
    }
    let mut szShortName = [0; 16];
    szShortName[..8].copy_from_slice(b"Speakers");
    let mut szName = [0; 64];
    szName[..16].copy_from_slice(b"Theseus Speakers");
    let mut szPname = [0; 32];
    szPname[..13].copy_from_slice(b"Theseus Mixer");
    ctx.memory.write(
        pmxl,
        MIXERLINEA {
            cbStruct: std::mem::size_of::<MIXERLINEA>() as u32,
            dwDestination: 0,
            dwSource: 0,
            dwLineID: 1,
            fdwLine: 0,
            dwUser: 0,
            dwComponentType: if fdwInfo == 3 { component_type } else { 4 },
            cChannels: 2,
            cConnections: 0,
            cControls: 1,
            szShortName,
            szName,
            Target: MIXERLINE_TARGETA {
                dwType: 1,
                dwDeviceID: 0,
                wMid: 0,
                wPid: 0,
                vDriverVersion: 1,
                szPname,
            },
        },
    );
    MMSYSERR_NOERROR
}

#[win32_derive::dllexport]
pub fn mixerOpen(
    ctx: &mut Context,
    phmx: u32,
    uMxId: u32,
    _dwCallback: u32,
    _dwInstance: u32,
    _fdwOpen: u32,
) -> u32 {
    if uMxId != MIXER_DEVICE_ID {
        return MMSYSERR_BADDEVICEID;
    }
    if phmx < 0x1000 {
        return MMSYSERR_INVALPARAM;
    }
    let hmx = kernel32::lock().objects.add(kernel32::Object::Mixer);
    ctx.memory.write(phmx, hmx.to_raw());
    MMSYSERR_NOERROR
}

#[win32_derive::dllexport]
pub fn mixerClose(_ctx: &mut Context, hmx: u32) -> u32 {
    let hmx = crate::HANDLE::from_raw(hmx);
    let mut state = kernel32::lock();
    if matches!(state.objects.get(hmx), Some(kernel32::Object::Mixer)) {
        state.objects.remove(hmx);
        MMSYSERR_NOERROR
    } else {
        MMSYSERR_INVALHANDLE
    }
}

#[cfg(test)]
mod tests {
    use super::MIXERLINEA;

    #[test]
    fn mixer_line_abi_matches_windows() {
        assert_eq!(std::mem::size_of::<MIXERLINEA>(), 0xa8);
    }
}
