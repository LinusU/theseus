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
