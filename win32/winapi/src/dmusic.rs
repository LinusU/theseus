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

use runtime::{ContFn, Context, Memory};

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
const DMUS_OBJ_CLASS: u32 = 0x1;
const DMUS_OBJECTDESC_SIZE: u32 = 0x180;
const E_OUTOFMEMORY: u32 = 0x8007_000E;

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
pub const CLSID_DirectMusicSegment: GUID = GUID::new(
    0xd2ac_2882,
    0xb39b,
    0x11d1,
    [0x87, 0x04, 0x00, 0x60, 0x08, 0x93, 0xb1, 0xbd],
);

const IID_IDirectMusicObject: GUID = GUID::new(
    0xd2ac_2880,
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
const IID_IDirectMusicSegment: GUID = GUID::new(
    0xf960_29a2,
    0x4282,
    0x11d2,
    [0x87, 0x17, 0x00, 0x60, 0x08, 0x93, 0xb1, 0xbd],
);
const IID_IDirectMusicSegment2: GUID = GUID::new(
    0xd388_94d1,
    0xc052,
    0x11d2,
    [0x87, 0x2f, 0x00, 0x60, 0x08, 0x93, 0xb1, 0xbd],
);
const IID_IDirectMusicSegment8: GUID = GUID::new(
    0xc678_4488,
    0x41a3,
    0x418f,
    [0xaa, 0x15, 0xb3, 0x50, 0x93, 0xba, 0x42, 0xd4],
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
fn fill_zero(memory: &mut Memory, addr: u32, len: u32) {
    let Some(end) = (addr as usize).checked_add(len as usize) else {
        return;
    };
    if let Some(dst) = memory.bytes.get_mut(addr as usize..end) {
        dst.fill(0);
    }
}

fn write_port_caps(ctx: &mut Context, caps: u32) -> u32 {
    if caps == 0 {
        return E_POINTER;
    }
    let Some(size) = crate::Ptr::<u32>::new(caps).read(&ctx.memory) else {
        return E_INVALIDARG;
    };
    if size < PORTCAPS_FIXED || caps as usize + size as usize > ctx.memory.bytes.len() {
        return E_INVALIDARG;
    }
    let write = size.min(PORTCAPS_SIZE);
    fill_zero(&mut ctx.memory, caps, write);
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
    if write >= PORTCAPS_SIZE {
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
            ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add((1 + $nargs) * 4);
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
            let out = ctx.memory.read::<u32>(esp.wrapping_add(($k + 1) * 4));
            let mut ret = $ret;
            if out != 0
                && crate::Ptr::<u64>::new(out)
                    .write(&mut ctx.memory, 0)
                    .is_none()
            {
                ret = E_POINTER;
            }
            log::debug!("dmusic {} (ret={return_addr:#x})", stringify!($name));
            ctx.cpu.regs.eax = ret;
            ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add((1 + $nargs) * 4);
            ctx.indirect(return_addr)
        }
    };
    ($name:ident, $nargs:expr, $k:expr, $ret:expr, u32) => {
        #[allow(non_snake_case)]
        pub fn $name(ctx: &mut Context) -> runtime::Cont {
            let esp = ctx.cpu.regs.esp;
            let return_addr = ctx.memory.read::<u32>(esp);
            let out = ctx.memory.read::<u32>(esp.wrapping_add(($k + 1) * 4));
            let mut ret = $ret;
            if out != 0
                && crate::Ptr::<u32>::new(out)
                    .write(&mut ctx.memory, 0)
                    .is_none()
            {
                ret = E_POINTER;
            }
            log::debug!("dmusic {} (ret={return_addr:#x})", stringify!($name));
            ctx.cpu.regs.eax = ret;
            ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add((1 + $nargs) * 4);
            ctx.indirect(return_addr)
        }
    };
}

/// Allocate a bare COM object: one u32 pointing at the given vtable.
fn new_object(ctx: &mut Context, vtable: u32) -> Option<u32> {
    if vtable == 0 {
        return None;
    }
    let kernel32 = kernel32::lock();
    let addr = kernel32.process_heap.try_alloc(&mut ctx.memory, 4)?;
    drop(kernel32);
    ctx.memory.write(addr, vtable);
    Some(addr)
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
                    if let Some((addr, blocks)) =
                        init_vtable(ctx, &mut kernel32.process_heap, $base, funcs)
                    {
                        $static_name = addr;
                        drop(kernel32);
                        add_blocks(ctx, blocks);
                    }
                }
                $static_name
            }
        }
    };
}

fn iid_matches(iid: &GUID, known: &[GUID]) -> bool {
    iid == &IID_IUnknown || iid == &IID_NullUnknown || known.contains(iid)
}

/// Shared CoCreateInstance body for the stub objects: check `riid` against
/// `iids`, build a bare object on `get_vtable`'s vtable, and hand it back
/// through `ppv`.
fn create(
    ctx: &mut Context,
    riid: u32,
    ppv: u32,
    iids: &[GUID],
    get_vtable: fn(&mut Context) -> u32,
) -> u32 {
    if !crate::ddraw::guest_range(ctx, ppv, 4) {
        return E_POINTER;
    }
    let ppv = crate::Ptr::<u32>::new(ppv);
    let iid = match read_guid(ctx, riid) {
        Some(iid) => iid,
        None => {
            if ppv.write(&mut ctx.memory, 0).is_none() {
                return E_POINTER;
            }
            return E_NOINTERFACE;
        }
    };
    if !iid_matches(&iid, iids) {
        if ppv.write(&mut ctx.memory, 0).is_none() {
            return E_POINTER;
        }
        return E_NOINTERFACE;
    }
    let vtable = get_vtable(ctx);
    let Some(obj) = new_object(ctx, vtable) else {
        if ppv.write(&mut ctx.memory, 0).is_none() {
            return E_POINTER;
        }
        return E_OUTOFMEMORY;
    };
    if ppv.write(&mut ctx.memory, obj).is_none() {
        return E_POINTER;
    }
    S_OK
}

/// Shared QueryInterface for the stub objects (this, riid, ppv).
macro_rules! query_interface {
    ($name:ident, $iids:expr) => {
        #[allow(non_snake_case)]
        pub fn $name(ctx: &mut Context) -> runtime::Cont {
            let esp = ctx.cpu.regs.esp;
            let return_addr = ctx.memory.read::<u32>(esp);
            let this = ctx.memory.read::<u32>(esp.wrapping_add(4));
            let riid = ctx.memory.read::<u32>(esp.wrapping_add(8));
            let ppv = crate::Ptr::<u32>::new(ctx.memory.read::<u32>(esp.wrapping_add(12)));
            let mut ret = S_OK;
            if ppv.addr == 0 {
                ret = E_POINTER;
            } else {
                match read_guid(ctx, riid) {
                    Some(iid) if iid_matches(&iid, $iids) => {
                        if ppv.write(&mut ctx.memory, this).is_none() {
                            ret = E_POINTER;
                        }
                    }
                    _ => {
                        if ppv.write(&mut ctx.memory, 0).is_none() {
                            ret = E_POINTER;
                        } else {
                            ret = E_NOINTERFACE;
                        }
                    }
                }
            }
            ctx.cpu.regs.eax = ret;
            ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
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
        let pp_direct_music = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let mut ret = S_OK;
        if pp_direct_music != 0 {
            let vtable = super::directmusic::get_vtable(ctx);
            match new_object(ctx, vtable) {
                Some(dmusic) => {
                    if crate::Ptr::<u32>::new(pp_direct_music)
                        .write(&mut ctx.memory, dmusic)
                        .is_none()
                    {
                        ret = E_POINTER;
                    }
                }
                None => ret = E_OUTOFMEMORY,
            }
        }
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(5 * 4);
        ctx.indirect(return_addr)
    }

    /// PlaySegment(this, pSegment, dwFlags, i64StartTime, ppSegmentState).
    #[allow(non_snake_case)]
    pub fn PlaySegment_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let pp_segment_state = ctx.memory.read::<u32>(esp.wrapping_add(24));
        let mut ret = S_OK;
        if pp_segment_state != 0
            && crate::Ptr::<u32>::new(pp_segment_state)
                .write(&mut ctx.memory, 0)
                .is_none()
        {
            ret = E_POINTER;
        }
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(7 * 4);
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
        let prt_now = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let pmt_now = ctx.memory.read::<u32>(esp.wrapping_add(12));
        let mut ret = S_OK;
        if prt_now != 0
            && crate::Ptr::<u64>::new(prt_now)
                .write(&mut ctx.memory, 0)
                .is_none()
        {
            ret = E_POINTER;
        }
        if pmt_now != 0
            && crate::Ptr::<u32>::new(pmt_now)
                .write(&mut ctx.memory, 0)
                .is_none()
        {
            ret = E_POINTER;
        }
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
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
        let mut ret = S_OK;
        for k in 2..=4 {
            let out = ctx.memory.read::<u32>(esp.wrapping_add((k + 1) * 4));
            if out != 0
                && crate::Ptr::<u32>::new(out)
                    .write(&mut ctx.memory, 0)
                    .is_none()
            {
                ret = E_POINTER;
            }
        }
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(6 * 4);
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
        super::create(
            ctx,
            riid,
            ppv,
            &[IID_IDirectMusicPerformance, IID_IDirectMusicPerformance2],
            get_vtable,
        )
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
        let index = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let caps = ctx.memory.read::<u32>(esp.wrapping_add(12));
        let ret = if index == 0 {
            write_port_caps(ctx, caps)
        } else if caps == 0 {
            E_POINTER
        } else {
            S_FALSE
        };
        log::debug!("dmusic EnumPort(index={index}) = {ret:#x} (ret={return_addr:#x})");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }

    stub_out!(CreateMusicBuffer_stub, 4, 2, E_FAIL);

    /// CreatePort(this, rclsidPort, pPortParams, ppPort, pUnkOuter).
    #[allow(non_snake_case)]
    pub fn CreatePort_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let pp_port = ctx.memory.read::<u32>(esp.wrapping_add(16));
        let mut ret = S_OK;
        if pp_port == 0 {
            ret = E_POINTER;
        } else {
            let vtable = super::port::get_vtable(ctx);
            match new_object(ctx, vtable) {
                Some(port) => {
                    if crate::Ptr::<u32>::new(pp_port)
                        .write(&mut ctx.memory, port)
                        .is_none()
                    {
                        ret = E_POINTER;
                    }
                }
                None => ret = E_OUTOFMEMORY,
            }
        }
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(6 * 4);
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
        let out = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let ret = if out == 0 {
            E_POINTER
        } else {
            match crate::Ptr::<GUID>::new(out).write(&mut ctx.memory, GUID_PortSynth) {
                Some(()) => S_OK,
                None => E_POINTER,
            }
        };
        log::debug!("dmusic GetDefaultPort = {ret:#x} (ret={return_addr:#x})");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(3 * 4);
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
        let caps = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let ret = write_port_caps(ctx, caps);
        log::debug!("dmusic port GetCaps = {ret:#x} (ret={return_addr:#x})");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(3 * 4);
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

pub mod music_object {
    use super::*;

    const DMUS_OBJECTDESC_CLASS_OFFSET: u32 = 32;

    /// Fill a `DMUS_OBJECTDESC` with the segment class when the caller has
    /// supplied a valid descriptor pointer. The descriptor `dwSize` field at
    /// offset 0 is left unchanged; `dwValidData` gets the `DMUS_OBJ_CLASS` flag
    /// and `guidClass` is set to `CLSID_DirectMusicSegment`.
    fn fill_object_desc(ctx: &mut Context, p_desc: u32) -> u32 {
        if p_desc == 0 {
            return E_POINTER;
        }
        if !crate::ddraw::guest_range(ctx, p_desc, DMUS_OBJECTDESC_SIZE) {
            return E_POINTER;
        }
        let Some(size) = crate::Ptr::<u32>::new(p_desc).read(&ctx.memory) else {
            return E_POINTER;
        };
        if size < DMUS_OBJECTDESC_CLASS_OFFSET + 16 {
            return E_INVALIDARG;
        }
        let Some(valid) = crate::Ptr::<u32>::new(p_desc.saturating_add(4)).read(&ctx.memory) else {
            return E_POINTER;
        };
        if crate::Ptr::<u32>::new(p_desc.saturating_add(4))
            .write(&mut ctx.memory, valid | DMUS_OBJ_CLASS)
            .is_none()
        {
            return E_POINTER;
        }
        if crate::Ptr::<GUID>::new(p_desc.saturating_add(DMUS_OBJECTDESC_CLASS_OFFSET))
            .write(&mut ctx.memory, CLSID_DirectMusicSegment)
            .is_none()
        {
            return E_POINTER;
        }
        S_OK
    }

    #[allow(non_snake_case)]
    pub fn QueryInterface_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let this = ctx.memory.read::<u32>(esp.wrapping_add(4));
        let riid = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let ppv = ctx.memory.read::<u32>(esp.wrapping_add(12));
        let mut ret = S_OK;
        if !crate::ddraw::guest_range(ctx, ppv, 4) {
            ret = E_POINTER;
        } else if let Some(iid) = read_guid(ctx, riid) {
            if iid_matches(&iid, &[IID_IDirectMusicObject]) {
                if crate::Ptr::<u32>::new(ppv)
                    .write(&mut ctx.memory, this)
                    .is_none()
                {
                    ret = E_POINTER;
                }
            } else if iid_matches(
                &iid,
                &[
                    IID_IDirectMusicSegment,
                    IID_IDirectMusicSegment2,
                    IID_IDirectMusicSegment8,
                ],
            ) {
                ret = super::segment::create(ctx, riid, ppv);
            } else {
                if crate::Ptr::<u32>::new(ppv)
                    .write(&mut ctx.memory, 0)
                    .is_none()
                {
                    ret = E_POINTER;
                } else {
                    ret = E_NOINTERFACE;
                }
            }
        } else {
            if crate::Ptr::<u32>::new(ppv)
                .write(&mut ctx.memory, 0)
                .is_none()
            {
                ret = E_POINTER;
            } else {
                ret = E_NOINTERFACE;
            }
        }
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }

    stub!(AddRef_stub, 1, 1);
    stub!(Release_stub, 1, 0);

    /// GetDescriptor(this, pDesc): report that this object is a segment.
    #[allow(non_snake_case)]
    pub fn GetDescriptor_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let p_desc = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let ret = fill_object_desc(ctx, p_desc);
        log::debug!("dmusic music_object GetDescriptor (ret={return_addr:#x}) = {ret:#x}");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(3 * 4);
        ctx.indirect(return_addr)
    }

    stub!(SetDescriptor_stub, 2);

    /// ParseDescriptor(this, pStream, pDesc): mark the descriptor as a segment.
    #[allow(non_snake_case)]
    pub fn ParseDescriptor_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let p_desc = ctx.memory.read::<u32>(esp.wrapping_add(12));
        let ret = fill_object_desc(ctx, p_desc);
        log::debug!("dmusic music_object ParseDescriptor (ret={return_addr:#x}) = {ret:#x}");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }

    vtable!(
        MUSIC_OBJECT_VTABLE,
        get_vtable,
        0xfafc_7000,
        [
            QueryInterface_stub,
            AddRef_stub,
            Release_stub,
            GetDescriptor_stub,
            SetDescriptor_stub,
            ParseDescriptor_stub,
        ]
    );

    pub fn create(ctx: &mut Context, riid: u32, ppv: u32) -> u32 {
        if !crate::ddraw::guest_range(ctx, ppv, 4) {
            return E_POINTER;
        }
        let Some(iid) = read_guid(ctx, riid) else {
            let _ = crate::Ptr::<u32>::new(ppv).write(&mut ctx.memory, 0);
            return E_NOINTERFACE;
        };
        if iid_matches(&iid, &[IID_IDirectMusicObject]) {
            super::create(ctx, riid, ppv, &[IID_IDirectMusicObject], get_vtable)
        } else if iid_matches(
            &iid,
            &[
                IID_IDirectMusicSegment,
                IID_IDirectMusicSegment2,
                IID_IDirectMusicSegment8,
            ],
        ) {
            super::segment::create(ctx, riid, ppv)
        } else {
            let _ = crate::Ptr::<u32>::new(ppv).write(&mut ctx.memory, 0);
            E_NOINTERFACE
        }
    }
}

pub mod segment {
    use super::*;

    /// QueryInterface(this, riid, ppv): segment interfaces return `this`;
    /// `IID_IDirectMusicObject` is delegated to a fresh music-object stub so
    /// callers can use the object for parsing/loading metadata.
    #[allow(non_snake_case)]
    pub fn QueryInterface_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let this = ctx.memory.read::<u32>(esp.wrapping_add(4));
        let riid = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let ppv = ctx.memory.read::<u32>(esp.wrapping_add(12));
        let mut ret = S_OK;
        if !crate::ddraw::guest_range(ctx, ppv, 4) {
            ret = E_POINTER;
        } else if let Some(iid) = read_guid(ctx, riid) {
            if iid_matches(
                &iid,
                &[
                    IID_IDirectMusicSegment,
                    IID_IDirectMusicSegment2,
                    IID_IDirectMusicSegment8,
                ],
            ) {
                if crate::Ptr::<u32>::new(ppv)
                    .write(&mut ctx.memory, this)
                    .is_none()
                {
                    ret = E_POINTER;
                }
            } else if iid == IID_IDirectMusicObject {
                ret = super::music_object::create(ctx, riid, ppv);
            } else {
                if crate::Ptr::<u32>::new(ppv)
                    .write(&mut ctx.memory, 0)
                    .is_none()
                {
                    ret = E_POINTER;
                } else {
                    ret = E_NOINTERFACE;
                }
            }
        } else {
            if crate::Ptr::<u32>::new(ppv)
                .write(&mut ctx.memory, 0)
                .is_none()
            {
                ret = E_POINTER;
            } else {
                ret = E_NOINTERFACE;
            }
        }
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }
    stub!(AddRef_stub, 1, 1);
    stub!(Release_stub, 1, 0);

    stub_out!(GetLength_stub, 2, 1, S_OK, u32);
    stub!(SetLength_stub, 2);
    stub_out!(GetRepeats_stub, 2, 1, S_OK, u32);
    stub!(SetRepeats_stub, 2);
    stub_out!(GetDefaultResolution_stub, 2, 1, S_OK, u32);
    stub!(SetDefaultResolution_stub, 2);
    stub_out!(GetTrack_stub, 5, 4, S_OK, u32);
    stub_out!(GetTrackGroup_stub, 3, 2, S_OK, u32);
    stub!(InsertTrack_stub, 3);
    stub!(RemoveTrack_stub, 2);
    stub_out!(InitPlay_stub, 4, 1, S_OK, u32);
    stub_out!(GetGraph_stub, 2, 1, S_OK, u32);
    stub!(SetGraph_stub, 2);
    stub!(AddNotificationType_stub, 2);
    stub!(RemoveNotificationType_stub, 2);

    /// GetParam(this, rguidType, dwGroupBits, dwIndex, mtTime, pmtNext, pParam):
    /// zero `pmtNext` if provided; leave `pParam` untouched because its size is
    /// determined by the parameter type.
    #[allow(non_snake_case)]
    pub fn GetParam_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let pmt_next = ctx.memory.read::<u32>(esp.wrapping_add(6 * 4));
        let mut ret = S_OK;
        if pmt_next != 0
            && crate::Ptr::<u32>::new(pmt_next)
                .write(&mut ctx.memory, 0)
                .is_none()
        {
            ret = E_POINTER;
        }
        log::debug!("dmusic GetParam (ret={return_addr:#x})");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(8 * 4);
        ctx.indirect(return_addr)
    }

    stub!(SetParam_stub, 6);
    stub_out!(Clone_stub, 4, 3, S_OK, u32);
    stub!(SetStartPoint_stub, 2);
    stub_out!(GetStartPoint_stub, 2, 1, S_OK, u32);
    stub!(SetLoopPoints_stub, 3);

    /// GetLoopPoints(this, pmtStart, pmtEnd): zero both out parameters.
    #[allow(non_snake_case)]
    pub fn GetLoopPoints_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let pmt_start = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let pmt_end = ctx.memory.read::<u32>(esp.wrapping_add(12));
        let mut ret = S_OK;
        for out in [pmt_start, pmt_end] {
            if out != 0
                && crate::Ptr::<u32>::new(out)
                    .write(&mut ctx.memory, 0)
                    .is_none()
            {
                ret = E_POINTER;
            }
        }
        log::debug!("dmusic GetLoopPoints (ret={return_addr:#x})");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }

    stub!(SetPChannelsUsed_stub, 3);
    stub!(SetTrackConfig_stub, 6);
    stub_out!(GetAudioPathConfig_stub, 2, 1, S_OK, u32);
    stub_out!(Compose_stub, 5, 4, S_OK, u32);
    stub!(Download_stub, 2);
    stub!(Unload_stub, 2);

    vtable!(
        SEGMENT_VTABLE,
        get_vtable,
        0xfafc_5000,
        [
            QueryInterface_stub,
            AddRef_stub,
            Release_stub,
            GetLength_stub,
            SetLength_stub,
            GetRepeats_stub,
            SetRepeats_stub,
            GetDefaultResolution_stub,
            SetDefaultResolution_stub,
            GetTrack_stub,
            GetTrackGroup_stub,
            InsertTrack_stub,
            RemoveTrack_stub,
            InitPlay_stub,
            GetGraph_stub,
            SetGraph_stub,
            AddNotificationType_stub,
            RemoveNotificationType_stub,
            GetParam_stub,
            SetParam_stub,
            Clone_stub,
            SetStartPoint_stub,
            GetStartPoint_stub,
            SetLoopPoints_stub,
            GetLoopPoints_stub,
            SetPChannelsUsed_stub,
            SetTrackConfig_stub,
            GetAudioPathConfig_stub,
            Compose_stub,
            Download_stub,
            Unload_stub,
        ]
    );

    pub fn create(ctx: &mut Context, riid: u32, ppv: u32) -> u32 {
        if !crate::ddraw::guest_range(ctx, ppv, 4) {
            return E_POINTER;
        }
        let Some(iid) = read_guid(ctx, riid) else {
            let _ = crate::Ptr::<u32>::new(ppv).write(&mut ctx.memory, 0);
            return E_NOINTERFACE;
        };
        if iid_matches(
            &iid,
            &[
                IID_IDirectMusicSegment,
                IID_IDirectMusicSegment2,
                IID_IDirectMusicSegment8,
            ],
        ) {
            super::create(
                ctx,
                riid,
                ppv,
                &[
                    IID_IDirectMusicSegment,
                    IID_IDirectMusicSegment2,
                    IID_IDirectMusicSegment8,
                ],
                get_vtable,
            )
        } else if iid == IID_IDirectMusicObject {
            super::music_object::create(ctx, riid, ppv)
        } else {
            let _ = crate::Ptr::<u32>::new(ppv).write(&mut ctx.memory, 0);
            E_NOINTERFACE
        }
    }
}

pub mod loader {
    use super::*;

    query_interface!(QueryInterface_stub, &[IID_IDirectMusicLoader]);
    stub!(AddRef_stub, 1, 1);
    stub!(Release_stub, 1, 0);

    /// GetObject(this, pDesc, riid, ppv): create a dummy `IDirectMusicSegment`
    /// for segment requests so callers like `OpenSegmentFile` can proceed even
    /// when the underlying `.sgt` file is missing.
    #[allow(non_snake_case)]
    pub fn GetObject_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let p_desc = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let riid = ctx.memory.read::<u32>(esp.wrapping_add(12));
        let ppv = ctx.memory.read::<u32>(esp.wrapping_add(16));
        let mut ret = E_FAIL;

        if !crate::ddraw::guest_range(ctx, ppv, 4) {
            ret = E_POINTER;
        } else {
            let mut wants_segment = false;
            if let Some(iid) = read_guid(ctx, riid)
                && (iid == IID_IDirectMusicSegment
                    || iid == IID_IDirectMusicSegment2
                    || iid == IID_IDirectMusicSegment8)
            {
                wants_segment = true;
            }
            if !wants_segment
                && p_desc >= 0x1000
                && let Some(class) =
                    crate::Ptr::<GUID>::new(p_desc.saturating_add(24)).read(&ctx.memory)
                && class == CLSID_DirectMusicSegment
            {
                wants_segment = true;
            }
            if wants_segment {
                ret = super::segment::create(ctx, riid, ppv);
            }
        }
        log::debug!("dmusic loader GetObject (ret={return_addr:#x}) = {ret:#x}");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(5 * 4);
        ctx.indirect(return_addr)
    }

    stub!(SetObject_stub, 2);
    stub!(SetSearchDirectory_stub, 4);
    /// ScanDirectory(this, rguidClass, pwzSearchPath, pwzFileExtension) walks
    /// a directory and adds matching objects to the loader cache. The emulated
    /// loader has no real filesystem search, so return `S_FALSE` (success but
    /// nothing found) instead of a misleading `S_OK`.
    #[allow(non_snake_case)]
    pub fn ScanDirectory_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        log::debug!("dmusic loader ScanDirectory (ret={return_addr:#x}) = S_FALSE");
        ctx.cpu.regs.eax = S_FALSE;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(5 * 4);
        ctx.indirect(return_addr)
    }
    stub!(CacheObject_stub, 2);
    stub!(ReleaseObject_stub, 2);
    stub!(ClearCache_stub, 2);
    stub!(EnableCache_stub, 3);

    /// EnumObject(this, pClassFilter, pCallback, pData) enumerates objects
    /// matching a class filter by calling the supplied callback. The emulated
    /// loader has no real content database, so return `S_FALSE` (success, zero
    /// objects) without calling the callback or touching the callback context.
    #[allow(non_snake_case)]
    pub fn EnumObject_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        log::debug!("dmusic loader EnumObject (ret={return_addr:#x}) = S_FALSE");
        ctx.cpu.regs.eax = S_FALSE;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(5 * 4);
        ctx.indirect(return_addr)
    }

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
        super::create(ctx, riid, ppv, &[IID_IDirectMusicLoader], get_vtable)
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
        let mut ret = E_FAIL;
        for k in 6..=8 {
            let out = ctx.memory.read::<u32>(esp.wrapping_add((k + 1) * 4));
            if out != 0
                && crate::Ptr::<u32>::new(out)
                    .write(&mut ctx.memory, 0)
                    .is_none()
            {
                ret = E_POINTER;
            }
        }
        log::debug!("dmusic AutoTransition (ret={return_addr:#x})");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(10 * 4);
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
        super::create(ctx, riid, ppv, &[IID_IDirectMusicComposer], get_vtable)
    }
}
