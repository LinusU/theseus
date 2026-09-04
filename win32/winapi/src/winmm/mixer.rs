use runtime::Context;

const MMSYSERR_NOERROR: u32 = 0;
const MMSYSERR_BADDEVICEID: u32 = 2;
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
