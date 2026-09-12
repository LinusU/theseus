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
    dplayx::{
        IID_IUnknown, IID_NullUnknown, add_blocks, add_ref_object, init_vtable, read_guid,
        register_object, release_object,
    },
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
pub const CLSID_DirectMusicStyle: GUID = GUID::new(
    0xd2ac_288a,
    0xb39b,
    0x11d1,
    [0x87, 0x04, 0x00, 0x60, 0x08, 0x93, 0xb1, 0xbd],
);
pub const CLSID_DirectMusicChordMap: GUID = GUID::new(
    0xd2ac_288f,
    0xb39b,
    0x11d1,
    [0x87, 0x04, 0x00, 0x60, 0x08, 0x93, 0xb1, 0xbd],
);
/// The band class lives outside the `d2ac28xx` performance family.
pub const CLSID_DirectMusicBand: GUID = GUID::new(
    0x79ba_9e00,
    0xb6ee,
    0x11d1,
    [0x86, 0xbe, 0x00, 0xc0, 0x4f, 0xbf, 0x8f, 0xef],
);

const IID_IDirectMusicObject: GUID = GUID::new(
    0xd2ac_28b5,
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
const IID_IDirectMusicStyle: GUID = GUID::new(
    0xd2ac_28bd,
    0xb39b,
    0x11d1,
    [0x87, 0x04, 0x00, 0x60, 0x08, 0x93, 0xb1, 0xbd],
);
const IID_IDirectMusicBand: GUID = GUID::new(
    0xd2ac_28c0,
    0xb39b,
    0x11d1,
    [0x87, 0x04, 0x00, 0x60, 0x08, 0x93, 0xb1, 0xbd],
);
const IID_IDirectMusicChordMap: GUID = GUID::new(
    0xd2ac_28be,
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
const IID_IPersist: GUID = GUID::new(
    0x0000_010c,
    0x0000,
    0x0000,
    [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
);
const IID_IPersistStream: GUID = GUID::new(
    0x0000_0109,
    0x0000,
    0x0000,
    [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
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
    register_object(addr);
    Some(addr)
}

/// Shared `AddRef(this)` for the stub objects.
#[allow(non_snake_case)]
pub fn AddRef_stub(ctx: &mut Context) -> runtime::Cont {
    let esp = ctx.cpu.regs.esp;
    let return_addr = ctx.memory.read::<u32>(esp);
    let this = ctx.memory.read::<u32>(esp.wrapping_add(4));
    ctx.cpu.regs.eax = add_ref_object(this);
    ctx.cpu.regs.esp = esp.wrapping_add(2 * 4);
    ctx.indirect(return_addr)
}

/// Shared `Release(this)`: frees the heap block on the last reference.
#[allow(non_snake_case)]
pub fn Release_stub(ctx: &mut Context) -> runtime::Cont {
    let esp = ctx.cpu.regs.esp;
    let return_addr = ctx.memory.read::<u32>(esp);
    let this = ctx.memory.read::<u32>(esp.wrapping_add(4));
    let remaining = release_object(this);
    if remaining == 0 {
        kernel32::lock().process_heap.free(&mut ctx.memory, this);
    }
    ctx.cpu.regs.eax = remaining;
    ctx.cpu.regs.esp = esp.wrapping_add(2 * 4);
    ctx.indirect(return_addr)
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

/// Allocate a bare object on `get_vtable` and write it to `ppv`, refcounted —
/// for callers that produce an object of a known type directly (a band's
/// `CreateSegment`, a style's `GetBand`) rather than through `QueryInterface`.
fn alloc_to(ctx: &mut Context, ppv: u32, get_vtable: fn(&mut Context) -> u32) -> u32 {
    if !crate::ddraw::guest_range(ctx, ppv, 4) {
        return E_POINTER;
    }
    let out = crate::Ptr::<u32>::new(ppv);
    let vtable = get_vtable(ctx);
    let Some(obj) = new_object(ctx, vtable) else {
        if out.write(&mut ctx.memory, 0).is_none() {
            return E_POINTER;
        }
        return E_OUTOFMEMORY;
    };
    if out.write(&mut ctx.memory, obj).is_none() {
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
                        } else {
                            // QueryInterface AddRefs the returned pointer.
                            add_ref_object(this);
                        }
                    }
                    Some(_) => {
                        if ppv.write(&mut ctx.memory, 0).is_none() {
                            ret = E_POINTER;
                        } else {
                            ret = E_NOINTERFACE;
                        }
                    }
                    None => {
                        let _ = ppv.write(&mut ctx.memory, 0);
                        ret = E_POINTER;
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
        let segment = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let pp_segment_state = ctx.memory.read::<u32>(esp.wrapping_add(24));
        let mut ret = S_OK;
        if pp_segment_state != 0
            && crate::Ptr::<u32>::new(pp_segment_state)
                .write(&mut ctx.memory, 0)
                .is_none()
        {
            ret = E_POINTER;
        }
        log::debug!("dmusic PlaySegment seg={segment:#x} (ret={return_addr:#x}) = {ret:#x}");
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

const DMUS_OBJECTDESC_CLASS_OFFSET: u32 = 24;

/// Fill a `DMUS_OBJECTDESC` with `class` when the caller has supplied a
/// valid descriptor pointer. The descriptor `dwSize` field at offset 0 is
/// left unchanged; `dwValidData` gets the `DMUS_OBJ_CLASS` flag and
/// `guidClass` is set to `class`.
fn fill_object_desc(ctx: &mut Context, p_desc: u32, class: GUID) -> u32 {
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
        .write(&mut ctx.memory, class)
        .is_none()
    {
        return E_POINTER;
    }
    S_OK
}

pub mod music_object {
    use super::*;

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
                } else {
                    add_ref_object(this);
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
            } else if iid == IID_IPersistStream || iid == IID_IPersist {
                ret = super::persist_stream::create(ctx, riid, ppv);
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
            let _ = crate::Ptr::<u32>::new(ppv).write(&mut ctx.memory, 0);
            ret = E_POINTER;
        }
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }

    /// GetDescriptor(this, pDesc): report that this object is a segment.
    #[allow(non_snake_case)]
    pub fn GetDescriptor_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let p_desc = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let ret = fill_object_desc(ctx, p_desc, CLSID_DirectMusicSegment);
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
        let ret = fill_object_desc(ctx, p_desc, CLSID_DirectMusicSegment);
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
        let ppv_out = crate::Ptr::<u32>::new(ppv);
        let Some(iid) = read_guid(ctx, riid) else {
            if ppv_out.write(&mut ctx.memory, 0).is_none() {
                return E_POINTER;
            }
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
        } else if iid == IID_IPersistStream {
            super::persist_stream::create(ctx, riid, ppv)
        } else if ppv_out.write(&mut ctx.memory, 0).is_none() {
            E_POINTER
        } else {
            E_NOINTERFACE
        }
    }
}

pub mod persist_stream {
    use super::*;

    /// GetClassID(this, pClassID): report the segment class.
    #[allow(non_snake_case)]
    pub fn GetClassID_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let p_classid = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let mut ret = S_OK;
        if !crate::ddraw::guest_range(ctx, p_classid, 16)
            || crate::Ptr::<GUID>::new(p_classid)
                .write(&mut ctx.memory, CLSID_DirectMusicSegment)
                .is_none()
        {
            ret = E_POINTER;
        }
        log::debug!("dmusic persist_stream GetClassID (ret={return_addr:#x}) = {ret:#x}");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(3 * 4);
        ctx.indirect(return_addr)
    }

    /// IsDirty(this): not dirty.
    #[allow(non_snake_case)]
    pub fn IsDirty_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        log::debug!("dmusic persist_stream IsDirty (ret={return_addr:#x}) = S_FALSE");
        ctx.cpu.regs.eax = S_FALSE;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(2 * 4);
        ctx.indirect(return_addr)
    }

    /// Load(this, pStm): ignore the missing stream and succeed.
    #[allow(non_snake_case)]
    pub fn Load_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let pstm = ctx.memory.read::<u32>(esp.wrapping_add(8));
        log::debug!("dmusic persist_stream Load pstm={pstm:#x} (ret={return_addr:#x}) = S_OK");
        ctx.cpu.regs.eax = S_OK;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(3 * 4);
        ctx.indirect(return_addr)
    }

    /// Save(this, pStm, fClearDirty): no-op.
    #[allow(non_snake_case)]
    pub fn Save_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        log::debug!("dmusic persist_stream Save (ret={return_addr:#x}) = S_OK");
        ctx.cpu.regs.eax = S_OK;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }

    /// GetSizeMax(this, pcbSize): report zero.
    #[allow(non_snake_case)]
    pub fn GetSizeMax_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let pcb_size = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let mut ret = S_OK;
        if pcb_size != 0
            && (!crate::ddraw::guest_range(ctx, pcb_size, 8)
                || crate::Ptr::<u32>::new(pcb_size)
                    .write(&mut ctx.memory, 0)
                    .is_none()
                || crate::Ptr::<u32>::new(pcb_size.wrapping_add(4))
                    .write(&mut ctx.memory, 0)
                    .is_none())
        {
            ret = E_POINTER;
        }
        log::debug!("dmusic persist_stream GetSizeMax (ret={return_addr:#x}) = {ret:#x}");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(3 * 4);
        ctx.indirect(return_addr)
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
            if iid == IID_IPersistStream
                || iid == IID_IPersist
                || iid_matches(&iid, &[IID_IUnknown])
            {
                if crate::Ptr::<u32>::new(ppv)
                    .write(&mut ctx.memory, this)
                    .is_none()
                {
                    ret = E_POINTER;
                } else {
                    add_ref_object(this);
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
            let _ = crate::Ptr::<u32>::new(ppv).write(&mut ctx.memory, 0);
            ret = E_POINTER;
        }
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }

    vtable!(
        PERSIST_STREAM_VTABLE,
        get_vtable,
        0xfafc_8000,
        [
            QueryInterface_stub,
            AddRef_stub,
            Release_stub,
            GetClassID_stub,
            IsDirty_stub,
            Load_stub,
            Save_stub,
            GetSizeMax_stub,
        ]
    );

    pub fn create(ctx: &mut Context, riid: u32, ppv: u32) -> u32 {
        if !crate::ddraw::guest_range(ctx, ppv, 4) {
            return E_POINTER;
        }
        let ppv_out = crate::Ptr::<u32>::new(ppv);
        let Some(iid) = read_guid(ctx, riid) else {
            if ppv_out.write(&mut ctx.memory, 0).is_none() {
                return E_POINTER;
            }
            return E_NOINTERFACE;
        };
        if iid == IID_IPersistStream || iid == IID_IPersist || iid_matches(&iid, &[IID_IUnknown]) {
            super::create(ctx, riid, ppv, &[IID_IPersistStream], get_vtable)
        } else if iid_matches(
            &iid,
            &[
                IID_IDirectMusicSegment,
                IID_IDirectMusicSegment2,
                IID_IDirectMusicSegment8,
            ],
        ) {
            super::segment::create(ctx, riid, ppv)
        } else if iid == IID_IDirectMusicObject {
            super::music_object::create(ctx, riid, ppv)
        } else if ppv_out.write(&mut ctx.memory, 0).is_none() {
            E_POINTER
        } else {
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
                } else {
                    add_ref_object(this);
                }
            } else if iid == IID_IDirectMusicObject {
                ret = super::music_object::create(ctx, riid, ppv);
            } else if iid == IID_IPersistStream || iid == IID_IPersist {
                ret = super::persist_stream::create(ctx, riid, ppv);
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
            let _ = crate::Ptr::<u32>::new(ppv).write(&mut ctx.memory, 0);
            ret = E_POINTER;
        }
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }

    stub_out!(GetLength_stub, 2, 1, S_OK, u32);
    stub!(SetLength_stub, 2);
    stub_out!(GetRepeats_stub, 2, 1, S_OK, u32);
    stub!(SetRepeats_stub, 2);
    stub_out!(GetDefaultResolution_stub, 2, 1, S_OK, u32);
    stub!(SetDefaultResolution_stub, 2);
    stub_out!(GetTrack_stub, 5, 4, S_FALSE, u32);
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
        let ppv_out = crate::Ptr::<u32>::new(ppv);
        let Some(iid) = read_guid(ctx, riid) else {
            if ppv_out.write(&mut ctx.memory, 0).is_none() {
                return E_POINTER;
            }
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
        } else if iid == IID_IPersistStream || iid == IID_IPersist {
            super::persist_stream::create(ctx, riid, ppv)
        } else if ppv_out.write(&mut ctx.memory, 0).is_none() {
            E_POINTER
        } else {
            E_NOINTERFACE
        }
    }

    /// Allocate a segment for a caller that produces one directly (a band's
    /// `CreateSegment`, a style's `GetMotif`) rather than through
    /// `QueryInterface`.
    pub fn create_direct(ctx: &mut Context, ppv: u32) -> u32 {
        super::alloc_to(ctx, ppv, get_vtable)
    }
}

pub mod loader {
    use super::*;

    query_interface!(QueryInterface_stub, &[IID_IDirectMusicLoader]);

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
        let iid = if riid != 0 {
            crate::Ptr::<GUID>::new(riid).read(&ctx.memory)
        } else {
            None
        };
        let mut ret = E_FAIL;
        log::debug!(
            "dmusic loader GetObject p_desc={p_desc:#x} riid={riid:#x} iid={iid:?} ppv={ppv:#x}"
        );

        if !crate::ddraw::guest_range(ctx, ppv, 4) {
            ret = E_POINTER;
        } else {
            let mut wants_segment = false;
            if let Some(iid) = read_guid(ctx, riid)
                && (iid == IID_IDirectMusicSegment
                    || iid == IID_IDirectMusicSegment2
                    || iid == IID_IDirectMusicSegment8
                    || iid == IID_IDirectMusicObject
                    || iid == IID_IPersistStream
                    || iid == IID_IPersist)
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

/// A DirectMusic content object the game instantiates by class — the Style,
/// Band, ChordMap and other `aud/dmusic` types it creates through
/// `CoCreateInstance`. The primary vtable is `IDirectMusicObject`, which
/// reports the class the object was created for so the descriptor the game
/// reads back matches what it asked for; `IPersistStream` and the class
/// interface are produced through `QueryInterface`.
pub mod dmusic_obj {
    use super::*;

    /// Object body: a vtable pointer, then the 16-byte class GUID at +4.
    const BODY: u32 = 4 + 16;
    const CLASS_OFF: u32 = 4;

    fn new(ctx: &mut Context, class: GUID) -> Option<u32> {
        let vtable = get_vtable(ctx);
        if vtable == 0 {
            return None;
        }
        let kernel32 = kernel32::lock();
        let addr = kernel32.process_heap.try_alloc(&mut ctx.memory, BODY)?;
        drop(kernel32);
        ctx.memory.write::<u32>(addr, vtable);
        ctx.memory.write::<GUID>(addr + CLASS_OFF, class);
        register_object(addr);
        Some(addr)
    }

    /// GetDescriptor(this, pDesc): report the class this object was created for.
    #[allow(non_snake_case)]
    pub fn GetDescriptor_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let this = ctx.memory.read::<u32>(esp.wrapping_add(4));
        let p_desc = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let ret = match crate::Ptr::<GUID>::new(this.wrapping_add(CLASS_OFF)).read(&ctx.memory) {
            Some(class) => fill_object_desc(ctx, p_desc, class),
            None => E_POINTER,
        };
        log::debug!("dmusic obj GetDescriptor (ret={return_addr:#x}) = {ret:#x}");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(3 * 4);
        ctx.indirect(return_addr)
    }

    stub!(SetDescriptor_stub, 2);

    /// ParseDescriptor(this, pStream, pDesc): mark the descriptor with the
    /// object's class, like `music_object::ParseDescriptor`.
    #[allow(non_snake_case)]
    pub fn ParseDescriptor_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let this = ctx.memory.read::<u32>(esp.wrapping_add(4));
        let p_desc = ctx.memory.read::<u32>(esp.wrapping_add(12));
        let ret = match crate::Ptr::<GUID>::new(this.wrapping_add(CLASS_OFF)).read(&ctx.memory) {
            Some(class) => fill_object_desc(ctx, p_desc, class),
            None => E_POINTER,
        };
        log::debug!("dmusic obj ParseDescriptor (ret={return_addr:#x}) = {ret:#x}");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }

    /// QueryInterface(this, riid, ppv): the `IDirectMusicObject` view returns
    /// `this`; `IPersistStream` and the matching class interface delegate to a
    /// fresh interface object.
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
            let class = crate::Ptr::<GUID>::new(this.wrapping_add(CLASS_OFF)).read(&ctx.memory);
            if iid_matches(&iid, &[IID_IDirectMusicObject]) {
                if crate::Ptr::<u32>::new(ppv)
                    .write(&mut ctx.memory, this)
                    .is_none()
                {
                    ret = E_POINTER;
                } else {
                    add_ref_object(this);
                }
            } else if iid == IID_IPersistStream || iid == IID_IPersist {
                ret = super::persist_stream::create(ctx, riid, ppv);
            } else if class == Some(CLSID_DirectMusicBand) && iid == IID_IDirectMusicBand {
                ret = super::band::create(ctx, riid, ppv);
            } else if class == Some(CLSID_DirectMusicStyle) && iid == IID_IDirectMusicStyle {
                ret = super::style::create(ctx, riid, ppv);
            } else if class == Some(CLSID_DirectMusicChordMap) && iid == IID_IDirectMusicChordMap {
                ret = super::chordmap::create(ctx, riid, ppv);
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
            let _ = crate::Ptr::<u32>::new(ppv).write(&mut ctx.memory, 0);
            ret = E_POINTER;
        }
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }

    vtable!(
        DMUSIC_OBJ_VTABLE,
        get_vtable,
        0xfafc_a000,
        [
            QueryInterface_stub,
            AddRef_stub,
            Release_stub,
            GetDescriptor_stub,
            SetDescriptor_stub,
            ParseDescriptor_stub,
        ]
    );

    /// CoCreateInstance entry for a content class: only `IDirectMusicObject`,
    /// `IPersistStream`, or the matching class interface can be the initial
    /// interface, mirroring `music_object::create`.
    pub fn create(ctx: &mut Context, riid: u32, ppv: u32, class: GUID) -> u32 {
        if !crate::ddraw::guest_range(ctx, ppv, 4) {
            return E_POINTER;
        }
        let ppv_out = crate::Ptr::<u32>::new(ppv);
        let Some(iid) = read_guid(ctx, riid) else {
            if ppv_out.write(&mut ctx.memory, 0).is_none() {
                return E_POINTER;
            }
            return E_NOINTERFACE;
        };
        if iid_matches(&iid, &[IID_IDirectMusicObject]) {
            let Some(obj) = new(ctx, class) else {
                if ppv_out.write(&mut ctx.memory, 0).is_none() {
                    return E_POINTER;
                }
                return E_OUTOFMEMORY;
            };
            if ppv_out.write(&mut ctx.memory, obj).is_none() {
                return E_POINTER;
            }
            S_OK
        } else if iid == IID_IPersistStream || iid == IID_IPersist {
            super::persist_stream::create(ctx, riid, ppv)
        } else if class == CLSID_DirectMusicBand && iid == IID_IDirectMusicBand {
            super::band::create(ctx, riid, ppv)
        } else if class == CLSID_DirectMusicStyle && iid == IID_IDirectMusicStyle {
            super::style::create(ctx, riid, ppv)
        } else if class == CLSID_DirectMusicChordMap && iid == IID_IDirectMusicChordMap {
            super::chordmap::create(ctx, riid, ppv)
        } else if ppv_out.write(&mut ctx.memory, 0).is_none() {
            E_POINTER
        } else {
            E_NOINTERFACE
        }
    }
}

/// `IDirectMusicBand`: `CreateSegment` produces a playable segment for the
/// band's instruments; `Download`/`Unload` only move data to the performance,
/// which the emulation has no real audio path for, so they no-op.
pub mod band {
    use super::*;

    query_interface!(QueryInterface_stub, &[IID_IDirectMusicBand]);

    /// CreateSegment(this, ppSegment): hand back an emulated segment object.
    #[allow(non_snake_case)]
    pub fn CreateSegment_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let pp_segment = ctx.memory.read::<u32>(esp.wrapping_add(8));
        let ret = super::segment::create_direct(ctx, pp_segment);
        log::debug!("dmusic band CreateSegment (ret={return_addr:#x}) = {ret:#x}");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(3 * 4);
        ctx.indirect(return_addr)
    }

    stub!(Download_stub, 2);
    stub!(Unload_stub, 2);

    vtable!(
        BAND_VTABLE,
        get_vtable,
        0xfafc_b000,
        [
            QueryInterface_stub,
            AddRef_stub,
            Release_stub,
            CreateSegment_stub,
            Download_stub,
            Unload_stub,
        ]
    );

    pub fn create(ctx: &mut Context, riid: u32, ppv: u32) -> u32 {
        super::create(ctx, riid, ppv, &[IID_IDirectMusicBand], get_vtable)
    }

    /// Allocate a band for a caller that produces one directly (a style's
    /// `GetBand`) rather than through `QueryInterface`.
    pub fn create_direct(ctx: &mut Context, ppv: u32) -> u32 {
        super::alloc_to(ctx, ppv, get_vtable)
    }
}

/// `IDirectMusicStyle`: `GetBand`/`GetMotif`/`GetChordMap` return real emulated
/// objects so callers can compose with them; the query and enumeration methods
/// answer with empty results.
pub mod style {
    use super::*;

    query_interface!(QueryInterface_stub, &[IID_IDirectMusicStyle]);

    /// GetBand(this, wIndex, ppBand): produce a band object.
    #[allow(non_snake_case)]
    pub fn GetBand_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let pp_band = ctx.memory.read::<u32>(esp.wrapping_add(12));
        let ret = super::band::create_direct(ctx, pp_band);
        log::debug!("dmusic style GetBand (ret={return_addr:#x}) = {ret:#x}");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }

    stub_out!(EnumBand_stub, 3, 2, S_FALSE);
    stub_out!(GetDefaultBand_stub, 2, 1, S_FALSE);
    stub_out!(EnumMotif_stub, 3, 2, S_FALSE);

    /// GetMotif(this, wIndex, ppMotif): produce a segment object — a motif is
    /// played as a segment state.
    #[allow(non_snake_case)]
    pub fn GetMotif_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let pp_motif = ctx.memory.read::<u32>(esp.wrapping_add(12));
        let ret = super::segment::create_direct(ctx, pp_motif);
        log::debug!("dmusic style GetMotif (ret={return_addr:#x}) = {ret:#x}");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }

    stub_out!(GetDefaultChordMap_stub, 2, 1, S_FALSE);
    stub_out!(EnumChordMap_stub, 3, 2, S_FALSE);

    /// GetChordMap(this, pwszName, ppChordMap): produce a chord map object.
    #[allow(non_snake_case)]
    pub fn GetChordMap_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let pp_chordmap = ctx.memory.read::<u32>(esp.wrapping_add(12));
        let ret = super::chordmap::create_direct(ctx, pp_chordmap);
        log::debug!("dmusic style GetChordMap (ret={return_addr:#x}) = {ret:#x}");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }

    stub_out!(GetTimeSignature_stub, 2, 1);

    /// GetEmbellishmentLength(this, dwType, dwLevel, pdwMin, pdwMax): zero the
    /// min and max out pointers.
    #[allow(non_snake_case)]
    pub fn GetEmbellishmentLength_stub(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let mut ret = S_OK;
        for off in [16u32, 20u32] {
            let out = ctx.memory.read::<u32>(esp.wrapping_add(off));
            if out != 0
                && crate::Ptr::<u32>::new(out)
                    .write(&mut ctx.memory, 0)
                    .is_none()
            {
                ret = E_POINTER;
            }
        }
        log::debug!("dmusic style GetEmbellishmentLength (ret={return_addr:#x})");
        ctx.cpu.regs.eax = ret;
        ctx.cpu.regs.esp = ctx.cpu.regs.esp.wrapping_add(6 * 4);
        ctx.indirect(return_addr)
    }

    // GetTempo(this, pTempo): the out param is a double.
    stub_out!(GetTempo_stub, 2, 1, S_OK, u64);

    vtable!(
        STYLE_VTABLE,
        get_vtable,
        0xfafc_c000,
        [
            QueryInterface_stub,
            AddRef_stub,
            Release_stub,
            GetBand_stub,
            EnumBand_stub,
            GetDefaultBand_stub,
            EnumMotif_stub,
            GetMotif_stub,
            GetDefaultChordMap_stub,
            EnumChordMap_stub,
            GetChordMap_stub,
            GetTimeSignature_stub,
            GetEmbellishmentLength_stub,
            GetTempo_stub,
        ]
    );

    pub fn create(ctx: &mut Context, riid: u32, ppv: u32) -> u32 {
        super::create(ctx, riid, ppv, &[IID_IDirectMusicStyle], get_vtable)
    }
}

/// `IDirectMusicChordMap`: `GetScale` writes a chord scale, which the stub
/// reports as zeroed.
pub mod chordmap {
    use super::*;

    query_interface!(QueryInterface_stub, &[IID_IDirectMusicChordMap]);

    stub_out!(GetScale_stub, 2, 1);

    vtable!(
        CHORDMAP_VTABLE,
        get_vtable,
        0xfafc_d000,
        [
            QueryInterface_stub,
            AddRef_stub,
            Release_stub,
            GetScale_stub,
        ]
    );

    pub fn create(ctx: &mut Context, riid: u32, ppv: u32) -> u32 {
        super::create(ctx, riid, ppv, &[IID_IDirectMusicChordMap], get_vtable)
    }

    /// Allocate a chord map for a caller that produces one directly (a style's
    /// `GetChordMap`) rather than through `QueryInterface`.
    pub fn create_direct(ctx: &mut Context, ppv: u32) -> u32 {
        super::alloc_to(ctx, ppv, get_vtable)
    }
}

/// `CoCreateInstance` entry for a DirectMusic content class. Returns `Some(hr)`
/// for a class the emulation answers, `None` for anything else.
pub fn create_object(ctx: &mut Context, riid: u32, ppv: u32, clsid: GUID) -> Option<u32> {
    let known = clsid == CLSID_DirectMusicStyle
        || clsid == CLSID_DirectMusicBand
        || clsid == CLSID_DirectMusicChordMap
        || is_track_class(clsid);
    if !known {
        return None;
    }
    Some(dmusic_obj::create(ctx, riid, ppv, clsid))
}

/// The `d2ac28xx-b39b-11d1-8704-00600893b1bd` family holds the performance
/// classes and every track/segment state class the loader can instantiate; any
/// of them resolves to the generic content object.
fn is_track_class(clsid: GUID) -> bool {
    const DM8_TAIL: [u8; 8] = [0x87, 0x04, 0x00, 0x60, 0x08, 0x93, 0xb1, 0xbd];
    clsid.data1 >> 16 == 0xd2ac
        && clsid.data2 == 0xb39b
        && clsid.data3 == 0x11d1
        && clsid.data4 == DM8_TAIL
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::{BlockCache, CPU, Context, Memory};

    fn context() -> Context {
        Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x4000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    fn test_heap() {
        crate::kernel32::ensure_test_state();
        let mut k32 = crate::kernel32::lock();
        k32.process_heap = crate::heap::Heap::new(0x1000, 0x1000);
    }

    fn write_guid(ctx: &mut Context, addr: u32, guid: &GUID) {
        ctx.memory.write::<GUID>(addr, *guid);
    }

    /// Call a raw vtable `ContFn` with a return address and args on a stack
    /// in guest memory; the unknown return address lands on `halt`.
    fn call_raw(ctx: &mut Context, f: ContFn, args: &[u32]) {
        const ESP: u32 = 0x3e00;
        ctx.cpu.regs.esp = ESP;
        ctx.memory.write::<u32>(ESP, 0);
        for (i, &arg) in args.iter().enumerate() {
            ctx.memory.write::<u32>(ESP + 4 * (i as u32 + 1), arg);
        }
        f(ctx);
    }

    fn block_alive(addr: u32) -> bool {
        crate::kernel32::lock()
            .process_heap
            .block_size(addr)
            .is_some()
    }

    #[test]
    fn stub_objects_count_real_references() {
        // Objects created through CoCreateInstance start at refs=1,
        // QueryInterface AddRefs the returned pointer, and the shared
        // Release frees the heap block when the count reaches zero.
        let mut ctx = context();
        test_heap();

        const PPV: u32 = 0x3000;
        const RIID: u32 = 0x3800;
        write_guid(&mut ctx, RIID, &IID_IDirectMusicPerformance);
        assert_eq!(performance::create(&mut ctx, RIID, PPV), S_OK);
        let perf = ctx.memory.read::<u32>(PPV);
        assert_ne!(perf, 0);
        assert!(block_alive(perf));

        write_guid(&mut ctx, RIID, &IID_IUnknown);
        call_raw(
            &mut ctx,
            performance::QueryInterface_stub,
            &[perf, RIID, PPV + 4],
        );
        assert_eq!(ctx.cpu.regs.eax, S_OK);
        assert_eq!(ctx.memory.read::<u32>(PPV + 4), perf);

        call_raw(&mut ctx, AddRef_stub, &[perf]);
        assert_eq!(ctx.cpu.regs.eax, 3);
        call_raw(&mut ctx, Release_stub, &[perf]);
        assert_eq!(ctx.cpu.regs.eax, 2);
        call_raw(&mut ctx, Release_stub, &[perf]);
        assert_eq!(ctx.cpu.regs.eax, 1);
        call_raw(&mut ctx, Release_stub, &[perf]);
        assert_eq!(ctx.cpu.regs.eax, 0);
        assert!(!block_alive(perf));
    }

    #[test]
    fn custom_query_interfaces_addref_this() {
        // The hand-rolled QueryInterface bodies (music_object here) return
        // `this` for their own IIDs and must AddRef it; the delegated
        // `create` branches hand back a fresh registered object.
        let mut ctx = context();
        test_heap();

        const PPV: u32 = 0x3000;
        const RIID: u32 = 0x3800;
        write_guid(&mut ctx, RIID, &IID_IDirectMusicObject);
        assert_eq!(music_object::create(&mut ctx, RIID, PPV), S_OK);
        let obj = ctx.memory.read::<u32>(PPV);
        assert_ne!(obj, 0);

        call_raw(
            &mut ctx,
            music_object::QueryInterface_stub,
            &[obj, RIID, PPV + 4],
        );
        assert_eq!(ctx.cpu.regs.eax, S_OK);
        assert_eq!(ctx.memory.read::<u32>(PPV + 4), obj);

        call_raw(&mut ctx, Release_stub, &[obj]);
        assert_eq!(ctx.cpu.regs.eax, 1);
        call_raw(&mut ctx, Release_stub, &[obj]);
        assert_eq!(ctx.cpu.regs.eax, 0);
        assert!(!block_alive(obj));
    }

    #[test]
    fn content_objects_report_their_class() {
        // The game instantiates Style/Band/ChordMap by CLSID for
        // IDirectMusicObject; GetDescriptor must report the class it was
        // created for so the loader routes the loaded stream correctly.
        let mut ctx = context();
        test_heap();

        const PPV: u32 = 0x3000;
        const RIID: u32 = 0x3800;
        const DESC: u32 = 0x3c00;
        for class in [CLSID_DirectMusicStyle, CLSID_DirectMusicBand] {
            write_guid(&mut ctx, RIID, &IID_IDirectMusicObject);
            assert_eq!(
                create_object(&mut ctx, RIID, PPV, class),
                Some(S_OK),
                "{class:?}"
            );
            let obj = ctx.memory.read::<u32>(PPV);
            assert_ne!(obj, 0);

            // DMUS_OBJECTDESC: dwSize, dwValidData, guidClass at +24.
            ctx.memory.write::<u32>(DESC, DMUS_OBJECTDESC_SIZE);
            ctx.memory.write::<u32>(DESC + 4, 0);
            call_raw(&mut ctx, dmusic_obj::GetDescriptor_stub, &[obj, DESC]);
            assert_eq!(ctx.cpu.regs.eax, S_OK);
            assert_eq!(
                ctx.memory.read::<u32>(DESC + 4) & DMUS_OBJ_CLASS,
                DMUS_OBJ_CLASS
            );
            assert_eq!(
                crate::Ptr::<GUID>::new(DESC + DMUS_OBJECTDESC_CLASS_OFFSET).read(&ctx.memory),
                Some(class),
            );

            // QueryInterface for IPersistStream and the class interface must
            // produce fresh interface objects.
            write_guid(&mut ctx, RIID, &IID_IDirectMusicBand);
            call_raw(
                &mut ctx,
                dmusic_obj::QueryInterface_stub,
                &[obj, RIID, PPV + 4],
            );
            if class == CLSID_DirectMusicBand {
                assert_eq!(ctx.cpu.regs.eax, S_OK);
                let band = ctx.memory.read::<u32>(PPV + 4);
                assert_ne!(band, 0);
                assert_ne!(band, obj);
            } else {
                assert_eq!(ctx.cpu.regs.eax, E_NOINTERFACE);
            }
        }
    }

    #[test]
    fn unregistered_class_is_rejected() {
        // A GUID outside the DirectMusic content family is not ours to build.
        let mut ctx = context();
        test_heap();
        const PPV: u32 = 0x3000;
        const RIID: u32 = 0x3800;
        write_guid(&mut ctx, RIID, &IID_IDirectMusicObject);
        let other = GUID::new(0x1234_5678, 0, 0, [0; 8]);
        assert_eq!(create_object(&mut ctx, RIID, PPV, other), None);
    }
}
