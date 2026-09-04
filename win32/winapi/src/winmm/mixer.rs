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
    if !matches!(state.objects.get(hmxobj), Some(kernel32::Object::Mixer(_))) {
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

#[repr(C)]
#[derive(Debug, zerocopy::Immutable, zerocopy::IntoBytes)]
struct MIXERCONTROL {
    cbStruct: u32,
    dwControlID: u32,
    dwControlType: u32,
    fdwControl: u32,
    cMultipleItems: u32,
    szShortName: [u8; 16],
    szName: [u8; 64],
    bounds: [u32; 2],
    metrics: [u32; 6],
    reserved: [u32; 4],
}

#[win32_derive::dllexport]
pub fn mixerGetLineControlsA(ctx: &mut Context, hmxobj: u32, pmxlc: u32, fdwControls: u32) -> u32 {
    let hmxobj = crate::HANDLE::from_raw(hmxobj);
    let state = kernel32::lock();
    if !matches!(state.objects.get(hmxobj), Some(kernel32::Object::Mixer(_))) {
        return MMSYSERR_INVALHANDLE;
    }
    drop(state);
    if pmxlc < 0x1000 || ctx.memory.read::<u32>(pmxlc) < 24 {
        return MMSYSERR_INVALPARAM;
    }
    let cControls = ctx.memory.read::<u32>(pmxlc + 12);
    let cbmxctrl = ctx.memory.read::<u32>(pmxlc + 16);
    let pamxctrl = ctx.memory.read::<u32>(pmxlc + 20);
    if cControls != 1
        || cbmxctrl < std::mem::size_of::<MIXERCONTROL>() as u32
        || pamxctrl < 0x1000
        || !matches!(fdwControls, 0..=2)
    {
        return MMSYSERR_INVALPARAM;
    }
    let dwControlType = if fdwControls == 2 {
        ctx.memory.read::<u32>(pmxlc + 8)
    } else {
        0x5003_0001
    };
    let mut szShortName = [0; 16];
    szShortName[..6].copy_from_slice(b"Volume");
    let mut szName = [0; 64];
    szName[..13].copy_from_slice(b"Master Volume");
    ctx.memory.write(
        pamxctrl,
        MIXERCONTROL {
            cbStruct: std::mem::size_of::<MIXERCONTROL>() as u32,
            dwControlID: 1,
            dwControlType,
            fdwControl: 0,
            cMultipleItems: 0,
            szShortName,
            szName,
            bounds: [0, 65_535],
            metrics: [65_535, 0, 0, 0, 0, 0],
            reserved: [0; 4],
        },
    );
    MMSYSERR_NOERROR
}

#[win32_derive::dllexport]
pub fn mixerGetControlDetailsA(ctx: &mut Context, hmxobj: u32, pmxcd: u32, fdwDetails: u32) -> u32 {
    let hmxobj = crate::HANDLE::from_raw(hmxobj);
    let state = kernel32::lock();
    let volume = match state.objects.get(hmxobj) {
        Some(kernel32::Object::Mixer(volume)) => *volume,
        _ => return MMSYSERR_INVALHANDLE,
    };
    drop(state);
    if pmxcd < 0x1000 || ctx.memory.read::<u32>(pmxcd) < 24 {
        return MMSYSERR_INVALPARAM;
    }
    let dwControlID = ctx.memory.read::<u32>(pmxcd + 4);
    let cChannels = ctx.memory.read::<u32>(pmxcd + 8);
    let cbDetails = ctx.memory.read::<u32>(pmxcd + 16);
    let paDetails = ctx.memory.read::<u32>(pmxcd + 20);
    let Some(total) = (cChannels as usize).checked_mul(cbDetails as usize) else {
        return MMSYSERR_INVALPARAM;
    };
    let Some(end) = (paDetails as usize).checked_add(total) else {
        return MMSYSERR_INVALPARAM;
    };
    if dwControlID != 1
        || cChannels != volume.len() as u32
        || cbDetails < 4
        || fdwDetails != 0
        || paDetails < 0x1000
        || end > ctx.memory.bytes.len()
    {
        return MMSYSERR_INVALPARAM;
    }
    for channel in 0..cChannels {
        ctx.memory
            .write(paDetails + channel * cbDetails, volume[channel as usize]);
    }
    MMSYSERR_NOERROR
}

#[win32_derive::dllexport]
pub fn mixerSetControlDetails(ctx: &mut Context, hmxobj: u32, pmxcd: u32, fdwDetails: u32) -> u32 {
    let hmxobj = crate::HANDLE::from_raw(hmxobj);
    let state = kernel32::lock();
    if !matches!(state.objects.get(hmxobj), Some(kernel32::Object::Mixer(_))) {
        return MMSYSERR_INVALHANDLE;
    }
    drop(state);
    if pmxcd < 0x1000 || ctx.memory.read::<u32>(pmxcd) < 24 {
        return MMSYSERR_INVALPARAM;
    }
    let dwControlID = ctx.memory.read::<u32>(pmxcd + 4);
    let cChannels = ctx.memory.read::<u32>(pmxcd + 8);
    let cbDetails = ctx.memory.read::<u32>(pmxcd + 16);
    let paDetails = ctx.memory.read::<u32>(pmxcd + 20);
    let Some(total) = (cChannels as usize).checked_mul(cbDetails as usize) else {
        return MMSYSERR_INVALPARAM;
    };
    let Some(end) = (paDetails as usize).checked_add(total) else {
        return MMSYSERR_INVALPARAM;
    };
    if dwControlID != 1
        || cChannels != 2
        || cbDetails < 4
        || fdwDetails != 0
        || paDetails < 0x1000
        || end > ctx.memory.bytes.len()
    {
        return MMSYSERR_INVALPARAM;
    }
    let volume = [
        ctx.memory.read::<u32>(paDetails),
        ctx.memory.read::<u32>(paDetails + cbDetails),
    ];
    let mut state = kernel32::lock();
    let Some(kernel32::Object::Mixer(current)) = state.objects.get_mut(hmxobj) else {
        return MMSYSERR_INVALHANDLE;
    };
    *current = volume;
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
    let hmx = kernel32::lock()
        .objects
        .add(kernel32::Object::Mixer([u32::MAX; 2]));
    ctx.memory.write(phmx, hmx.to_raw());
    MMSYSERR_NOERROR
}

#[win32_derive::dllexport]
pub fn mixerClose(_ctx: &mut Context, hmx: u32) -> u32 {
    let hmx = crate::HANDLE::from_raw(hmx);
    let mut state = kernel32::lock();
    if matches!(state.objects.get(hmx), Some(kernel32::Object::Mixer(_))) {
        state.objects.remove(hmx);
        MMSYSERR_NOERROR
    } else {
        MMSYSERR_INVALHANDLE
    }
}

#[cfg(test)]
mod tests {
    use super::{MIXERCONTROL, MIXERLINEA};

    #[test]
    fn mixer_line_abi_matches_windows() {
        assert_eq!(std::mem::size_of::<MIXERLINEA>(), 0xa8);
    }

    #[test]
    fn mixer_control_abi_matches_the_game() {
        assert_eq!(std::mem::size_of::<MIXERCONTROL>(), 0x94);
    }
}
