//! Minimal DirectMusic COM objects.
//!
//! MM2 keeps a global IDirectMusicPerformance pointer and calls into it every
//! frame (PlaySegment, Stop, IsPlaying, ...) even when no music is playing, so
//! the objects have to exist and answer their vtables even though no audio is
//! produced. Every method is a no-op that returns a contract-correct value.
//!
//! Stubs are hand-rolled ContFns rather than #[dllexport] wrappers: they read
//! no argument values, so all that matters for the stdcall contract is popping
//! the right count. `nargs` counts the stack slots for `this` plus the real
//! arguments; the return address is popped on top of that. i64 arguments count
//! as two slots.

use runtime::{ContFn, Context};

use crate::{
    ddraw::GUID,
    dplayx::{IID_IUnknown, IID_NullUnknown, add_blocks, init_vtable, read_guid},
    kernel32,
};

const S_OK: u32 = 0;
const S_FALSE: u32 = 1;
const E_POINTER: u32 = 0x8000_4003;
const E_NOINTERFACE: u32 = 0x8000_4002;
const E_FAIL: u32 = 0x8000_4005;
const E_INVALIDARG: u32 = 0x8007_0057;

pub const CLSID_DirectMusicPerformance: GUID = GUID::new(
    0xd2ac_2881,
    0xb39b,
    0x11d1,
    [0x87, 0x04, 0x00, 0x60, 0x08, 0x93, 0xb1, 0xbd],
);
pub const CLSID_DirectMusicComposer: GUID = GUID::new(
    0xd2ac_2890,
    0xb39b,
    0x11d1,
    [0x87, 0x04, 0x00, 0x60, 0x08, 0x93, 0xb1, 0xbd],
);
pub const CLSID_DirectMusicLoader: GUID = GUID::new(
    0xd2ac_2892,
    0xb39b,
    0x11d1,
    [0x87, 0x04, 0x00, 0x60, 0x08, 0x93, 0xb1, 0xbd],
);

const IID_IDirectMusicPerformance: GUID = GUID::new(
    0x07d4_3d03,
    0x6523,
    0x11d2,
    [0x87, 0x1d, 0x00, 0x60, 0x08, 0x93, 0xb1, 0xbd],
);
const IID_IDirectMusicPerformance2: GUID = GUID::new(
    0x6fc2_cae0,
    0xbc78,
    0x11d2,
    [0xaf, 0xa6, 0x00, 0xaa, 0x00, 0x24, 0xd8, 0xb6],
);
const IID_IDirectMusic: GUID = GUID::new(
    0x6536_115a,
    0x7b2d,
    0x11d2,
    [0xba, 0x18, 0x00, 0x00, 0xf8, 0x75, 0xac, 0x12],
);
const IID_IDirectMusicLoader: GUID = GUID::new(
    0x2ffa_aca2,
    0x5dca,
    0x11d2,
    [0xaf, 0xa6, 0x00, 0xaa, 0x00, 0x24, 0xd8, 0xb6],
);
const IID_IDirectMusicPort: GUID = GUID::new(
    0x08f2_d8c9,
    0x37c2,
    0x11d2,
    [0xb9, 0xf9, 0x00, 0x00, 0xf8, 0x75, 0xac, 0x12],
);
const IID_IDirectMusicComposer: GUID = GUID::new(
    0xd2ac_28bf,
    0xb39b,
    0x11d1,
    [0x87, 0x04, 0x00, 0x60, 0x08, 0x93, 0xb1, 0xbd],
);

/// The GUID the single emulated port reports through `EnumPort` and
/// `GetDefaultPort`; callers pass it back to `IDirectMusic::CreatePort`.
/// Using the Microsoft software synthesizer's CLSID covers games that
/// match on it instead of on `DMUS_PC_SOFTWARESYNTH`.
const GUID_PortSynth: GUID = GUID::new(
    0x58c2_b4d0,
    0x46e7,
    0x11d1,
    [0x89, 0xac, 0x00, 0xa0, 0xc9, 0x05, 0x41, 0x29],
);

/// DMUS_PORTCAPS: fixed fields end at offset 52, then a 128-WCHAR
/// description, for a total of 308 bytes.
const PORTCAPS_FIXED: u32 = 52;
const PORTCAPS_SIZE: u32 = PORTCAPS_FIXED + 128 * 2;

const DMUS_PC_DLS: u32 = 0x1;
const DMUS_PC_SOFTWARESYNTH: u32 = 0x4;
const DMUS_PC_SHAREABLE: u32 = 0x200;
const DMUS_PC_OUTPUTCLASS: u32 = 1;
const DMUS_PORT_USER_MODE_SYNTH: u32 = 1;

/// Serialize the emulated port's DMUS_PORTCAPS. The caller initializes
/// dwSize; fields past the caller's struct size are left alone.
fn write_port_caps(ctx: &mut Context, caps: u32) -> u32 {
    if caps == 0 {
        return E_POINTER;
    }
    let size = ctx.memory.read::<u32>(caps);
    if size < PORTCAPS_FIXED || caps as usize + size as usize > ctx.memory.bytes.len() {
        return E_INVALIDARG;
    }
    let write = size.min(PORTCAPS_SIZE) as usize;
    ctx.memory[caps..][..write].fill(0);
    ctx.memory.write::<u32>(
        caps + 4,
        DMUS_PC_DLS | DMUS_PC_SOFTWARESYNTH | DMUS_PC_SHAREABLE,
    );
    ctx.memory.write::<GUID>(caps + 8, GUID_PortSynth);
    ctx.memory.write::<u32>(caps + 24, DMUS_PC_OUTPUTCLASS);
    ctx.memory
        .write::<u32>(caps + 28, DMUS_PORT_USER_MODE_SYNTH);
    ctx.memory.write::<u32>(caps + 32, 4 * 1024 * 1024); // dwMemorySize
    ctx.memory.write::<u32>(caps + 36, 32); // dwMaxChannelGroups
    ctx.memory.write::<u32>(caps + 40, 128); // dwMaxVoices
    ctx.memory.write::<u32>(caps + 44, 32); // dwMaxAudioChannels
    // dwEffectFlags at 48 stays zero: the emulated port has no effects.
    if write >= PORTCAPS_SIZE as usize {
        // MM2 matches on this exact name when looking for the software synth.
        let desc = "Microsoft Synthesizer";
        for (i, unit) in desc.encode_utf16().chain(std::iter::once(0)).enumerate() {
            ctx.memory
                .write::<u16>(caps + PORTCAPS_FIXED + i as u32 * 2, unit);
        }
    }
    S_OK
}

/// A no-op COM method returning S_OK.
macro_rules! stub {
    ($name:ident, $nargs:expr) => {
        stub!($name, $nargs, S_OK);
    };
    ($name:ident, $nargs:expr, $ret:expr) => {
        #[allow(non_snake_case)]
        pub fn $name(ctx: &mut Context) -> runtime::Cont {
            let return_addr = ctx.memory.read::<u32>(ctx.cpu.regs.esp);
            log::debug!("dmusic {} (ret={return_addr:#x})", stringify!($name));
            ctx.cpu.regs.eax = $ret;
            ctx.cpu.regs.esp += (1 + $nargs) * 4;
            ctx.indirect(return_addr)
        }
    };
}

/// A no-op COM method that also writes zero to the pointer in param `k`
/// (params are counted with `this` = 0).
macro_rules! stub_out {
    ($name:ident, $nargs:expr, $k:expr) => {
        stub_out!($name, $nargs, $k, S_OK, u32);
    };
    ($name:ident, $nargs:expr, $k:expr, $ret:expr) => {
        stub_out!($name, $nargs, $k, $ret, u32);
    };
    ($name:ident, $nargs:expr, $k:expr, $ret:expr, u64) => {
        #[allow(non_snake_case)]
        pub fn $name(ctx: &mut Context) -> runtime::Cont {
            let esp = ctx.cpu.regs.esp;
            let return_addr = ctx.memory.read::<u32>(esp);
            let out = ctx.memory.read::<u32>(esp + ($k + 1) * 4);
            if out != 0 {
                ctx.memory.write::<u64>(out, 0);
            }
            log::debug!("dmusic {} (ret={return_addr:#x})", stringify!($name));
            ctx.cpu.regs.eax = $ret;
            ctx.cpu.regs.esp += (1 + $nargs) * 4;
            ctx.indirect(return_addr)
        }
    };
    ($name:ident, $nargs:expr, $k:expr, $ret:expr, u32) => {
        #[allow(non_snake_case)]
        pub fn $name(ctx: &mut Context) -> runtime::Cont {
            let esp = ctx.cpu.regs.esp;
            let return_addr = ctx.memory.read::<u32>(esp);
            let out = ctx.memory.read::<u32>(esp + ($k + 1) * 4);
            if out != 0 {
                ctx.memory.write::<u32>(out, 0);
            }
            log::debug!("dmusic {} (ret={return_addr:#x})", stringify!($name));
            ctx.cpu.regs.eax = $ret;
            ctx.cpu.regs.esp += (1 + $nargs) * 4;
            ctx.indirect(return_addr)
        }
    };
}

/// Allocate a bare COM object: one u32 pointing at the given vtable.
fn new_object(ctx: &mut Context, vtable: u32) -> u32 {
    let kernel32 = kernel32::lock();
    let addr = kernel32.process_heap.alloc(&mut ctx.memory, 4);
    drop(kernel32);
    ctx.memory.write(addr, vtable);
    addr
}

/// Build a guest vtable once per interface and register its stubs.
macro_rules! vtable {
    ($static_name:ident, $get:ident, $base:expr, [ $( $func:expr ),* $(,)? ]) => {
        static mut $static_name: u32 = 0;
        #[allow(static_mut_refs)]
        pub fn $get(ctx: &mut Context) -> u32 {
            unsafe {
                if $static_name == 0 {
                    let funcs: &[ContFn] = &[$( $func ),*];
                    let mut kernel32 = kernel32::lock();
                    let (addr, blocks) =
                        init_vtable(ctx, &mut kernel32.process_heap, $base, funcs);
                    $static_name = addr;
                    drop(kernel32);
                    add_blocks(ctx, blocks);
                }
                $static_name
            }
        }
    };
}

fn iid_matches(iid: &GUID, known: &[GUID]) -> bool {
    iid == &IID_IUnknown || iid == &IID_NullUnknown || known.contains(iid)
}

/// Shared QueryInterface for the stub objects (this, riid, ppv).
macro_rules! query_interface {
    ($name:ident, $iids:expr) => {
        #[allow(non_snake_case)]
        pub fn $name(ctx: &mut Context) -> runtime::Cont {
            let esp = ctx.cpu.regs.esp;
            let return_addr = ctx.memory.read::<u32>(esp);
            let this = ctx.memory.read::<u32>(esp + 4);
            let riid = ctx.memory.read::<u32>(esp + 8);
            let ppv = ctx.memory.read::<u32>(esp + 12);
            let mut ret = S_OK;
            if ppv == 0 {
                ret = E_POINTER;
            } else {
                match read_guid(ctx, riid) {
                    Some(iid) if iid_matches(&iid, $iids) => {
                        ctx.memory.write::<u32>(ppv, this);
                    }
                    _ => {
                        ctx.memory.write::<u32>(ppv, 0);
                        ret = E_NOINTERFACE;
                    }
                }
            }
            ctx.cpu.regs.eax = ret;
            ctx.cpu.regs.esp += 4 * 4;
            ctx.indirect(return_addr)
        }
    };
}

pub mod performance {
    use super::*;

    query_interface!(
        QueryInterface_stub,
        &[IID_IDirectMusicPerformance, IID_IDirectMusicPerformance2]
    );
    stub!(AddRef_stub, 1, 1);
    stub!(Release_stub, 1, 0);

    /// Init(this, ppDirectMusic, pDirectSound, hWnd): hands back an
    /// IDirectMusic stub so the game's port setup has something to call.
    #[allow(non_snake_case)]
    pub fn Init_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let pp_direct_music = ctx.memory.read::<u32>(esp + 8);
        if pp_direct_music != 0 {
            let vtable = super::directmusic::get_vtable(ctx);
            let dmusic = new_object(ctx, vtable);
            ctx.memory.write::<u32>(pp_direct_music, dmusic);
        }
        ctx.cpu.regs.eax = S_OK;
        ctx.cpu.regs.esp += 5 * 4;
        ctx.indirect(return_addr)
    }

    /// PlaySegment(this, pSegment, dwFlags, i64StartTime, ppSegmentState).
    #[allow(non_snake_case)]
    pub fn PlaySegment_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let pp_segment_state = ctx.memory.read::<u32>(esp + 24);
        if pp_segment_state != 0 {
            ctx.memory.write::<u32>(pp_segment_state, 0);
        }
        ctx.cpu.regs.eax = S_OK;
        ctx.cpu.regs.esp += 7 * 4;
        ctx.indirect(return_addr)
    }

    stub!(Stop_stub, 5);
    stub_out!(GetSegmentState_stub, 3, 1);
    stub!(SetPrepareTime_stub, 2);
    stub_out!(GetPrepareTime_stub, 2, 1);
    stub!(SetBumperLength_stub, 2);
    stub_out!(GetBumperLength_stub, 2, 1);
    stub!(SendPMsg_stub, 2);
    stub_out!(MusicToReferenceTime_stub, 3, 2, S_OK, u64);
    stub_out!(ReferenceToMusicTime_stub, 4, 3);
    // BOOL IsPlaying: FALSE, nothing is playing.
    stub!(IsPlaying_stub, 3, 0);

    /// GetTime(this, prtNow, pmtNow): zero both out params.
    #[allow(non_snake_case)]
    pub fn GetTime_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let prt_now = ctx.memory.read::<u32>(esp + 8);
        let pmt_now = ctx.memory.read::<u32>(esp + 12);
        if prt_now != 0 {
            ctx.memory.write::<u64>(prt_now, 0);
        }
        if pmt_now != 0 {
            ctx.memory.write::<u32>(pmt_now, 0);
        }
        ctx.cpu.regs.eax = S_OK;
        ctx.cpu.regs.esp += 4 * 4;
        ctx.indirect(return_addr)
    }

    stub_out!(AllocPMsg_stub, 3, 2);
    stub!(FreePMsg_stub, 2);
    stub_out!(GetGraph_stub, 2, 1);
    stub!(SetGraph_stub, 2);
    stub!(SetNotificationHandle_stub, 4);
    stub_out!(GetNotificationPMsg_stub, 2, 1);
    stub!(AddNotificationType_stub, 2);
    stub!(RemoveNotificationType_stub, 2);
    stub!(AddPort_stub, 2);
    stub!(RemovePort_stub, 2);
    stub!(AssignPChannelBlock_stub, 4);
    stub!(AssignPChannel_stub, 5);

    /// PChannelInfo(this, dwPChannel, ppPort, pdwGroup, pdwMChannel).
    #[allow(non_snake_case)]
    pub fn PChannelInfo_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        for k in 2..=4 {
            let out = ctx.memory.read::<u32>(esp + (k + 1) * 4);
            if out != 0 {
                ctx.memory.write::<u32>(out, 0);
            }
        }
        ctx.cpu.regs.eax = S_OK;
        ctx.cpu.regs.esp += 6 * 4;
        ctx.indirect(return_addr)
    }

    stub_out!(DownloadInstrument_stub, 8, 3);
    stub!(Invalidate_stub, 3);
    stub!(GetParam_stub, 7);
    stub!(SetParam_stub, 6);
    stub!(GetGlobalParam_stub, 4);
    stub!(SetGlobalParam_stub, 4);
    stub_out!(GetLatencyTime_stub, 2, 1, S_OK, u64);
    stub_out!(GetQueueTime_stub, 2, 1, S_OK, u64);
    stub!(AdjustTime_stub, 3);
    stub!(CloseDown_stub, 1);
    stub_out!(GetResolvedTime_stub, 5, 3, S_OK, u64);
    stub!(MIDIToMusic_stub, 6);
    stub!(MusicToMIDI_stub, 6);
    stub!(TimeToRhythm_stub, 7);
    stub!(RhythmToTime_stub, 7);

    // IDirectMusicPerformance2 additions.
    stub!(InitAudio_stub, 8);
    stub_out!(PlaySegmentEx_stub, 10, 7);
    stub!(StopEx_stub, 5);
    stub_out!(ClonePMsg_stub, 3, 2);
    stub_out!(CreateAudioPath_stub, 4, 3);
    stub_out!(CreateStandardAudioPath_stub, 5, 4);
    stub!(SetDefaultAudioPath_stub, 2);
    stub_out!(GetDefaultAudioPath_stub, 2, 1);
    stub!(GetParamEx_stub, 8);

    vtable!(
        PERF_VTABLE,
        get_vtable,
        0xfafc_0000,
        [
            QueryInterface_stub,
            AddRef_stub,
            Release_stub,
            Init_stub,
            PlaySegment_stub,
            Stop_stub,
            GetSegmentState_stub,
            SetPrepareTime_stub,
            GetPrepareTime_stub,
            SetBumperLength_stub,
            GetBumperLength_stub,
            SendPMsg_stub,
            MusicToReferenceTime_stub,
            ReferenceToMusicTime_stub,
            IsPlaying_stub,
            GetTime_stub,
            AllocPMsg_stub,
            FreePMsg_stub,
            GetGraph_stub,
            SetGraph_stub,
            SetNotificationHandle_stub,
            GetNotificationPMsg_stub,
            AddNotificationType_stub,
            RemoveNotificationType_stub,
            AddPort_stub,
            RemovePort_stub,
            AssignPChannelBlock_stub,
            AssignPChannel_stub,
            PChannelInfo_stub,
            DownloadInstrument_stub,
            Invalidate_stub,
            GetParam_stub,
            SetParam_stub,
            GetGlobalParam_stub,
            SetGlobalParam_stub,
            GetLatencyTime_stub,
            GetQueueTime_stub,
            AdjustTime_stub,
            CloseDown_stub,
            GetResolvedTime_stub,
            MIDIToMusic_stub,
            MusicToMIDI_stub,
            TimeToRhythm_stub,
            RhythmToTime_stub,
            InitAudio_stub,
            PlaySegmentEx_stub,
            StopEx_stub,
            ClonePMsg_stub,
            CreateAudioPath_stub,
            CreateStandardAudioPath_stub,
            SetDefaultAudioPath_stub,
            GetDefaultAudioPath_stub,
            GetParamEx_stub,
        ]
    );

    pub fn create(ctx: &mut Context, riid: u32, ppv: u32) -> u32 {
        if ppv == 0 {
            return E_POINTER;
        }
        let iid = match read_guid(ctx, riid) {
            Some(iid) => iid,
            None => {
                ctx.memory.write::<u32>(ppv, 0);
                return E_NOINTERFACE;
            }
        };
        if !iid_matches(
            &iid,
            &[IID_IDirectMusicPerformance, IID_IDirectMusicPerformance2],
        ) {
            ctx.memory.write::<u32>(ppv, 0);
            return E_NOINTERFACE;
        }
        let vtable = get_vtable(ctx);
        let obj = new_object(ctx, vtable);
        ctx.memory.write::<u32>(ppv, obj);
        S_OK
    }
}

pub mod directmusic {
    use super::*;

    query_interface!(QueryInterface_stub, &[IID_IDirectMusic]);
    stub!(AddRef_stub, 1, 1);
    stub!(Release_stub, 1, 0);
    /// EnumPort(this, dwIndex, pPortCaps): report the one emulated
    /// software-synth port at index 0 and S_FALSE past the end of the
    /// list, which is how callers know enumeration is done.
    #[allow(non_snake_case)]
    pub fn EnumPort_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let index = ctx.memory.read::<u32>(esp + 8);
        let caps = ctx.memory.read::<u32>(esp + 12);
        let ret = if index == 0 {
            write_port_caps(ctx, caps)
        } else if caps == 0 {
            E_POINTER
        } else {
            S_FALSE
        };
        log::debug!("dmusic EnumPort(index={index}) = {ret:#x} (ret={return_addr:#x})");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp += 4 * 4;
        ctx.indirect(return_addr)
    }

    stub_out!(CreateMusicBuffer_stub, 4, 2, E_FAIL);

    /// CreatePort(this, rclsidPort, pPortParams, ppPort, pUnkOuter).
    #[allow(non_snake_case)]
    pub fn CreatePort_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let pp_port = ctx.memory.read::<u32>(esp + 16);
        let mut ret = S_OK;
        if pp_port == 0 {
            ret = E_POINTER;
        } else {
            let vtable = super::port::get_vtable(ctx);
            let port = new_object(ctx, vtable);
            ctx.memory.write::<u32>(pp_port, port);
        }
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp += 6 * 4;
        ctx.indirect(return_addr)
    }

    stub_out!(EnumMasterClock_stub, 3, 2, E_FAIL);
    stub_out!(GetMasterClock_stub, 3, 2, E_FAIL);
    stub!(SetMasterClock_stub, 2);
    stub!(Activate_stub, 2);

    /// GetDefaultPort(this, pguidDefaultPort): report the emulated
    /// software-synth port's GUID.
    #[allow(non_snake_case)]
    pub fn GetDefaultPort_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let out = ctx.memory.read::<u32>(esp + 8);
        let ret = if out == 0 || out as usize + 16 > ctx.memory.bytes.len() {
            E_POINTER
        } else {
            ctx.memory.write::<GUID>(out, GUID_PortSynth);
            S_OK
        };
        log::debug!("dmusic GetDefaultPort = {ret:#x} (ret={return_addr:#x})");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp += 3 * 4;
        ctx.indirect(return_addr)
    }
    stub!(SetDirectSound_stub, 3);
    stub!(SetExternalMasterClock_stub, 2);

    vtable!(
        MUSIC_VTABLE,
        get_vtable,
        0xfafc_1000,
        [
            QueryInterface_stub,
            AddRef_stub,
            Release_stub,
            EnumPort_stub,
            CreateMusicBuffer_stub,
            CreatePort_stub,
            EnumMasterClock_stub,
            GetMasterClock_stub,
            SetMasterClock_stub,
            Activate_stub,
            GetDefaultPort_stub,
            SetDirectSound_stub,
            SetExternalMasterClock_stub,
        ]
    );
}

pub mod port {
    use super::*;

    query_interface!(QueryInterface_stub, &[IID_IDirectMusicPort]);
    stub!(AddRef_stub, 1, 1);
    stub!(Release_stub, 1, 0);
    stub!(PlayBuffer_stub, 2);
    stub!(SetReadNotificationHandle_stub, 2);
    stub!(Read_stub, 2);
    stub_out!(DownloadInstrument_stub, 5, 2);
    stub!(UnloadInstrument_stub, 2);
    stub_out!(GetLatencyClock_stub, 2, 1);
    stub_out!(GetRunningStats_stub, 2, 1);
    stub!(Compact_stub, 1);

    /// GetCaps(this, pPortCaps): report the same DMUS_PORTCAPS the
    /// enumerator does for the emulated port.
    #[allow(non_snake_case)]
    pub fn GetCaps_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let caps = ctx.memory.read::<u32>(esp + 8);
        let ret = write_port_caps(ctx, caps);
        log::debug!("dmusic port GetCaps = {ret:#x} (ret={return_addr:#x})");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp += 3 * 4;
        ctx.indirect(return_addr)
    }

    stub!(DeviceIoControl_stub, 8);
    stub!(SetNumChannelGroups_stub, 2);
    stub_out!(GetNumChannelGroups_stub, 2, 1);
    stub!(Activate_stub, 2);
    stub!(SetChannelPriority_stub, 4);
    stub_out!(GetChannelPriority_stub, 4, 3);
    stub!(SetDirectSound_stub, 3);
    stub_out!(GetFormat_stub, 4, 2);

    vtable!(
        PORT_VTABLE,
        get_vtable,
        0xfafc_2000,
        [
            QueryInterface_stub,
            AddRef_stub,
            Release_stub,
            PlayBuffer_stub,
            SetReadNotificationHandle_stub,
            Read_stub,
            DownloadInstrument_stub,
            UnloadInstrument_stub,
            GetLatencyClock_stub,
            GetRunningStats_stub,
            Compact_stub,
            GetCaps_stub,
            DeviceIoControl_stub,
            SetNumChannelGroups_stub,
            GetNumChannelGroups_stub,
            Activate_stub,
            SetChannelPriority_stub,
            GetChannelPriority_stub,
            SetDirectSound_stub,
            GetFormat_stub,
        ]
    );
}

pub mod loader {
    use super::*;

    query_interface!(QueryInterface_stub, &[IID_IDirectMusicLoader]);
    stub!(AddRef_stub, 1, 1);
    stub!(Release_stub, 1, 0);

    // GetObject always fails: there is no .sgt segment loading, so callers
    // get a clean failure instead of a null object they might call through.
    stub_out!(GetObject_stub, 4, 3, E_FAIL);

    stub!(SetObject_stub, 2);
    stub!(SetSearchDirectory_stub, 4);
    stub!(ScanDirectory_stub, 4);
    stub!(CacheObject_stub, 2);
    stub!(ReleaseObject_stub, 2);
    stub!(ClearCache_stub, 2);
    stub!(EnableCache_stub, 3);
    stub_out!(EnumObject_stub, 4, 3, E_FAIL);

    // IDirectMusicLoader8 additions.
    stub!(CollectGarbage_stub, 1);
    stub!(ReleaseObjectByUnknown_stub, 2);
    stub_out!(LoadObjectFromFile_stub, 5, 4, E_FAIL);

    vtable!(
        LOADER_VTABLE,
        get_vtable,
        0xfafc_3000,
        [
            QueryInterface_stub,
            AddRef_stub,
            Release_stub,
            GetObject_stub,
            SetObject_stub,
            SetSearchDirectory_stub,
            ScanDirectory_stub,
            CacheObject_stub,
            ReleaseObject_stub,
            ClearCache_stub,
            EnableCache_stub,
            EnumObject_stub,
            CollectGarbage_stub,
            ReleaseObjectByUnknown_stub,
            LoadObjectFromFile_stub,
        ]
    );

    pub fn create(ctx: &mut Context, riid: u32, ppv: u32) -> u32 {
        if ppv == 0 {
            return E_POINTER;
        }
        let iid = match read_guid(ctx, riid) {
            Some(iid) => iid,
            None => {
                ctx.memory.write::<u32>(ppv, 0);
                return E_NOINTERFACE;
            }
        };
        if !iid_matches(&iid, &[IID_IDirectMusicLoader]) {
            ctx.memory.write::<u32>(ppv, 0);
            return E_NOINTERFACE;
        }
        let vtable = get_vtable(ctx);
        let obj = new_object(ctx, vtable);
        ctx.memory.write::<u32>(ppv, obj);
        S_OK
    }
}

pub mod composer {
    use super::*;

    query_interface!(QueryInterface_stub, &[IID_IDirectMusicComposer]);
    stub!(AddRef_stub, 1, 1);
    stub!(Release_stub, 1, 0);

    // Every composition method needs a real composition engine, so they
    // fail explicitly after honoring the out-pointer contract.
    stub_out!(ComposeSegmentFromTemplate_stub, 6, 5, E_FAIL);
    stub_out!(ComposeSegmentFromShape_stub, 9, 8, E_FAIL);
    stub_out!(ComposeTransition_stub, 9, 8, E_FAIL);

    /// AutoTransition(this, pPerformance, pToSeg, wCommand, dwFlags,
    /// pChordMap, ppTransSeg, ppTransPerf, ppToPerf): zero all three out
    /// pointers, then fail.
    #[allow(non_snake_case)]
    pub fn AutoTransition_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        for k in 6..=8 {
            let out = ctx.memory.read::<u32>(esp + (k + 1) * 4);
            if out != 0 {
                ctx.memory.write::<u32>(out, 0);
            }
        }
        log::debug!("dmusic AutoTransition (ret={return_addr:#x})");
        ctx.cpu.regs.eax = E_FAIL;
        ctx.cpu.regs.esp += 10 * 4;
        ctx.indirect(return_addr)
    }

    stub_out!(ComposeTemplateFromShape_stub, 7, 6, E_FAIL);
    stub!(ChangeChordMap_stub, 4, E_FAIL);

    vtable!(
        COMPOSER_VTABLE,
        get_vtable,
        0xfafc_4000,
        [
            QueryInterface_stub,
            AddRef_stub,
            Release_stub,
            ComposeSegmentFromTemplate_stub,
            ComposeSegmentFromShape_stub,
            ComposeTransition_stub,
            AutoTransition_stub,
            ComposeTemplateFromShape_stub,
            ChangeChordMap_stub,
        ]
    );

    pub fn create(ctx: &mut Context, riid: u32, ppv: u32) -> u32 {
        if ppv == 0 {
            return E_POINTER;
        }
        let iid = match read_guid(ctx, riid) {
            Some(iid) => iid,
            None => {
                ctx.memory.write::<u32>(ppv, 0);
                return E_NOINTERFACE;
            }
        };
        if !iid_matches(&iid, &[IID_IDirectMusicComposer]) {
            ctx.memory.write::<u32>(ppv, 0);
            return E_NOINTERFACE;
        }
        let vtable = get_vtable(ctx);
        let obj = new_object(ctx, vtable);
        ctx.memory.write::<u32>(ppv, obj);
        S_OK
    }
}
