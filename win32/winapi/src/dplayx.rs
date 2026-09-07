//! DirectPlay/DirectPlayLobby emulation.
//!
//! The MM2 target only reaches the lobby through `CoCreateInstance`, so the
//! COM object lives here and answers the `IDirectPlayLobby3A` vtable.

use runtime::{ContFn, Context};

use crate::{Ptr, ddraw::GUID, heap::Heap, kernel32};

const S_OK: u32 = 0;
const E_POINTER: u32 = 0x8000_4003;
const E_NOINTERFACE: u32 = 0x8000_4002;
const E_NOTIMPL: u32 = 0x8000_4001;
const E_FAIL: u32 = 0x8000_4005;
const E_OUTOFMEMORY: u32 = 0x8007_000E;

/// The canonical `IUnknown` IID (`00000000-0000-0000-C000-000000000046`).
pub(crate) const IID_IUnknown: GUID = GUID::new(
    0x0000_0000,
    0x0000,
    0x0000,
    [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
);

/// Legacy null `IUnknown` shortcut used elsewhere in the COM helpers.
pub(crate) const IID_NullUnknown: GUID = GUID::new(0, 0, 0, [0; 8]);

pub const IID_IDirectPlayLobby3: GUID = GUID::new(
    0x2db7_2490,
    0x652c,
    0x11d1,
    [0xa7, 0xa8, 0x00, 0x00, 0xf8, 0x03, 0xab, 0xfc],
);
pub const IID_IDirectPlayLobby3A: GUID = GUID::new(
    0x2db7_2491,
    0x652c,
    0x11d1,
    [0xa7, 0xa8, 0x00, 0x00, 0xf8, 0x03, 0xab, 0xfc],
);

pub const CLSID_DirectPlayLobby: GUID = GUID::new(
    0x2fe8_f810,
    0xb2a5,
    0x11d0,
    [0xa7, 0x87, 0x00, 0x00, 0xf8, 0x03, 0xab, 0xfc],
);

pub const CLSID_DirectPlay: GUID = GUID::new(
    0xd1eb_6d20,
    0x8923,
    0x11d0,
    [0x9d, 0x97, 0x00, 0xa0, 0xc9, 0x0a, 0x43, 0xcb],
);
pub const IID_IDirectPlay2: GUID = GUID::new(
    0x2b74_f7c0,
    0x9154,
    0x11cf,
    [0xa9, 0xcd, 0x00, 0xaa, 0x00, 0x68, 0x86, 0xe3],
);
pub const IID_IDirectPlay2A: GUID = GUID::new(
    0x9d46_0580,
    0xa822,
    0x11cf,
    [0x96, 0x0c, 0x00, 0x80, 0xc7, 0x53, 0x4e, 0x82],
);
pub const IID_IDirectPlay3: GUID = GUID::new(
    0x133e_fe40,
    0x32dc,
    0x11d0,
    [0x9c, 0xfb, 0x00, 0xa0, 0xc9, 0x0a, 0x43, 0xcb],
);
pub const IID_IDirectPlay3A: GUID = GUID::new(
    0x133e_fe41,
    0x32dc,
    0x11d0,
    [0x9c, 0xfb, 0x00, 0xa0, 0xc9, 0x0a, 0x43, 0xcb],
);
pub const IID_IDirectPlay4: GUID = GUID::new(
    0x0ab1_c530,
    0x4745,
    0x11d1,
    [0xa7, 0xa1, 0x00, 0x00, 0xf8, 0x03, 0xab, 0xfc],
);
pub const IID_IDirectPlay4A: GUID = GUID::new(
    0x0ab1_c531,
    0x4745,
    0x11d1,
    [0xa7, 0xa1, 0x00, 0x00, 0xf8, 0x03, 0xab, 0xfc],
);

pub(crate) fn read_guid(ctx: &Context, addr: u32) -> Option<GUID> {
    Ptr::<GUID>::new(addr).read(&ctx.memory)
}

pub(crate) fn init_vtable(
    ctx: &mut Context,
    heap: &mut Heap,
    base: u32,
    funcs: &[ContFn],
) -> Option<(u32, Vec<(u32, ContFn)>)> {
    let size = (funcs.len() * 4) as u32;
    let addr = heap.try_alloc(&mut ctx.memory, size)?;
    let mut blocks = Vec::with_capacity(funcs.len());
    for (i, &func) in funcs.iter().enumerate() {
        let fn_addr = base + i as u32;
        ctx.memory.write::<u32>(addr + i as u32 * 4, fn_addr);
        blocks.push((fn_addr, func));
    }
    Some((addr, blocks))
}

pub(crate) fn add_blocks(ctx: &mut Context, mut blocks: Vec<(u32, ContFn)>) {
    if blocks.is_empty() {
        return;
    }
    blocks.extend_from_slice(ctx.blocks);
    blocks.sort_by_key(|(addr, _)| *addr);
    ctx.blocks = Box::leak(blocks.into_boxed_slice());
}

pub mod IDirectPlayLobby3A {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 19] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Connect",
        "CreateAddress",
        "EnumAddress",
        "EnumAddressTypes",
        "EnumLocalApplications",
        "GetConnectionSettings",
        "ReceiveLobbyMessage",
        "RunApplication",
        "SendLobbyMessage",
        "SetConnectionSettings",
        "SetLobbyMessageEvent",
        "CreateCompoundAddress",
        "ConnectEx",
        "RegisterApplication",
        "UnregisterApplication",
        "WaitForConnectionSettings",
    ];

    pub const VTABLE_FUNCS: [ContFn; 19] = [
        QueryInterface_stdcall,
        AddRef_stdcall,
        Release_stdcall,
        Connect_stdcall,
        CreateAddress_stdcall,
        EnumAddress_stdcall,
        EnumAddressTypes_stdcall,
        EnumLocalApplications_stdcall,
        GetConnectionSettings_stdcall,
        ReceiveLobbyMessage_stdcall,
        RunApplication_stdcall,
        SendLobbyMessage_stdcall,
        SetConnectionSettings_stdcall,
        SetLobbyMessageEvent_stdcall,
        CreateCompoundAddress_stdcall,
        ConnectEx_stdcall,
        RegisterApplication_stdcall,
        UnregisterApplication_stdcall,
        WaitForConnectionSettings_stdcall,
    ];

    pub static mut VTABLE: u32 = 0;

    /// # Safety
    /// Must be called once before any DirectPlay interface methods are invoked.
    pub unsafe fn init_vtables(ctx: &mut Context) {
        if unsafe { VTABLE } == 0 {
            let mut kernel32 = kernel32::lock();
            let Some((addr, blocks)) =
                init_vtable(ctx, &mut kernel32.process_heap, 0xfafd_1000, &VTABLE_FUNCS)
            else {
                return;
            };
            unsafe { VTABLE = addr };
            drop(kernel32);
            add_blocks(ctx, blocks);
            log::debug!("IDirectPlayLobby3A vtable allocated at {addr:#x}");
        }
    }

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> Option<u32> {
        if unsafe { VTABLE } == 0 {
            return None;
        }
        let addr = heap.try_alloc(&mut ctx.memory, 4)?;
        ctx.memory.write(addr, unsafe { VTABLE });
        Some(addr)
    }

    pub fn create(ctx: &mut Context, riid: u32, ppv: u32) -> u32 {
        if !crate::ddraw::guest_range(ctx, ppv, 4) {
            return E_POINTER;
        }
        if riid == 0 {
            ctx.memory.write::<u32>(ppv, 0);
            return E_NOINTERFACE;
        }
        let iid = match read_guid(ctx, riid) {
            Some(iid) => iid,
            None => {
                ctx.memory.write::<u32>(ppv, 0);
                return E_NOINTERFACE;
            }
        };
        if iid == IID_IUnknown
            || iid == IID_NullUnknown
            || iid == IID_IDirectPlayLobby3
            || iid == IID_IDirectPlayLobby3A
        {
            unsafe { init_vtables(ctx) };
            let mut kernel32 = kernel32::lock();
            let Some(addr) = new(ctx, &mut kernel32.process_heap) else {
                return E_OUTOFMEMORY;
            };
            drop(kernel32);
            ctx.memory.write::<u32>(ppv, addr);
            S_OK
        } else {
            ctx.memory.write::<u32>(ppv, 0);
            E_NOINTERFACE
        }
    }

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppv: u32) -> u32 {
        if !crate::ddraw::guest_range(ctx, ppv, 4) {
            return E_POINTER;
        }
        if riid == 0 {
            ctx.memory.write::<u32>(ppv, 0);
            return E_NOINTERFACE;
        }
        let iid = match read_guid(ctx, riid) {
            Some(iid) => iid,
            None => {
                ctx.memory.write::<u32>(ppv, 0);
                return E_NOINTERFACE;
            }
        };
        if iid == IID_IUnknown
            || iid == IID_NullUnknown
            || iid == IID_IDirectPlayLobby3
            || iid == IID_IDirectPlayLobby3A
        {
            ctx.memory.write::<u32>(ppv, this);
            S_OK
        } else {
            ctx.memory.write::<u32>(ppv, 0);
            E_NOINTERFACE
        }
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, _this: u32) -> u32 {
        0
    }

    #[win32_derive::dllexport]
    pub fn Connect(
        _ctx: &mut Context,
        _this: u32,
        _dwFlags: u32,
        lplpDP2: u32,
        _pUnkOuter: u32,
    ) -> u32 {
        if lplpDP2 != 0
            && crate::Ptr::<u32>::new(lplpDP2)
                .write(&mut _ctx.memory, 0)
                .is_none()
        {
            return E_POINTER;
        }
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn CreateAddress(
        _ctx: &mut Context,
        _this: u32,
        _lpguidSP: u32,
        _lpguidAddressType: u32,
        _lpAddress: u32,
        _dwAddressSize: u32,
        _lpAddressBuffer: u32,
        lpdwAddressBufferSize: u32,
    ) -> u32 {
        if lpdwAddressBufferSize != 0
            && crate::Ptr::<u32>::new(lpdwAddressBufferSize)
                .write(&mut _ctx.memory, 0)
                .is_none()
        {
            return E_POINTER;
        }
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn EnumAddress(
        _ctx: &mut Context,
        _this: u32,
        _lpEnumAddressCallback: u32,
        _lpAddress: u32,
        _dwAddressSize: u32,
        _lpContext: u32,
    ) -> u32 {
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn EnumAddressTypes(
        _ctx: &mut Context,
        _this: u32,
        _lpEnumAddressTypeCallback: u32,
        _lpguidSP: u32,
        _lpContext: u32,
        _dwFlags: u32,
    ) -> u32 {
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn EnumLocalApplications(
        _ctx: &mut Context,
        _this: u32,
        _lpEnumLocalAppCallback: u32,
        _lpContext: u32,
        _dwFlags: u32,
    ) -> u32 {
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn GetConnectionSettings(
        _ctx: &mut Context,
        _this: u32,
        _dwLobbyID: u32,
        _lpConnectionSettings: u32,
        lpdwSize: u32,
    ) -> u32 {
        if lpdwSize == 0 {
            return E_POINTER;
        }
        if crate::Ptr::<u32>::new(lpdwSize)
            .write(&mut _ctx.memory, 0)
            .is_none()
        {
            return E_POINTER;
        }
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn ReceiveLobbyMessage(
        _ctx: &mut Context,
        _this: u32,
        _dwLobbyID: u32,
        _dwFlags: u32,
        lpdwMessageFlags: u32,
        _lpData: u32,
        lpdwDataSize: u32,
    ) -> u32 {
        if lpdwMessageFlags != 0
            && crate::Ptr::<u32>::new(lpdwMessageFlags)
                .write(&mut _ctx.memory, 0)
                .is_none()
        {
            return E_POINTER;
        }
        if lpdwDataSize != 0
            && crate::Ptr::<u32>::new(lpdwDataSize)
                .write(&mut _ctx.memory, 0)
                .is_none()
        {
            return E_POINTER;
        }
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn RunApplication(
        _ctx: &mut Context,
        _this: u32,
        _dwFlags: u32,
        lpdwAppID: u32,
        _lpConn: u32,
        _hReceiveEvent: u32,
    ) -> u32 {
        if lpdwAppID != 0
            && crate::Ptr::<u32>::new(lpdwAppID)
                .write(&mut _ctx.memory, 0)
                .is_none()
        {
            return E_POINTER;
        }
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn SendLobbyMessage(
        _ctx: &mut Context,
        _this: u32,
        _dwLobbyID: u32,
        _dwFlags: u32,
        _lpData: u32,
        _dwDataSize: u32,
    ) -> u32 {
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn SetConnectionSettings(
        _ctx: &mut Context,
        _this: u32,
        _dwFlags: u32,
        _dwLobbyID: u32,
        _lpConn: u32,
    ) -> u32 {
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn SetLobbyMessageEvent(
        _ctx: &mut Context,
        _this: u32,
        _dwLobbyID: u32,
        _dwFlags: u32,
        _hEvent: u32,
    ) -> u32 {
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn CreateCompoundAddress(
        _ctx: &mut Context,
        _this: u32,
        _lpElements: u32,
        _dwElementCount: u32,
        _lpAddress: u32,
        lpdwAddressSize: u32,
    ) -> u32 {
        if lpdwAddressSize != 0
            && crate::Ptr::<u32>::new(lpdwAddressSize)
                .write(&mut _ctx.memory, 0)
                .is_none()
        {
            return E_POINTER;
        }
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn ConnectEx(
        _ctx: &mut Context,
        _this: u32,
        _dwFlags: u32,
        _lpguidSP: u32,
        lplpDP3: u32,
        _pUnkOuter: u32,
    ) -> u32 {
        if lplpDP3 != 0
            && crate::Ptr::<u32>::new(lplpDP3)
                .write(&mut _ctx.memory, 0)
                .is_none()
        {
            return E_POINTER;
        }
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn RegisterApplication(
        _ctx: &mut Context,
        _this: u32,
        _dwFlags: u32,
        _lpAppDesc: u32,
    ) -> u32 {
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn UnregisterApplication(
        _ctx: &mut Context,
        _this: u32,
        _dwFlags: u32,
        _lpguidApplication: u32,
    ) -> u32 {
        E_FAIL
    }

    #[win32_derive::dllexport]
    pub fn WaitForConnectionSettings(_ctx: &mut Context, _this: u32, _dwFlags: u32) -> u32 {
        E_FAIL
    }
}

/// `IDirectPlay4A` object: a DirectPlay instance with no service providers.
///
/// The game creates this through `CoCreateInstance(CLSID_DirectPlay)` inside
/// `NETMGR.Initialize()`; without it the manager stores a null interface
/// pointer and later virtual calls crash the guest. The object answers every
/// vtable entry the way real DirectPlay does on a machine with no usable
/// service provider: enumeration entry points succeed with empty results,
/// and session/provider-dependent methods return the matching DPERR code.
pub mod directplay {
    use super::*;

    const DP_OK: u32 = S_OK;
    const DPERR_NOCONNECTION: u32 = 0x8877_00AA;
    const DPERR_UNAVAILABLE: u32 = 0x8877_00FA;
    const DPERR_NOSERVICEPROVIDER: u32 = 0x8877_0410;

    /// A DirectPlay method that returns `ret` and stdcall-pops `nargs`
    /// parameters (including `this`).
    macro_rules! dp_stub {
        ($name:ident, $nargs:expr) => {
            dp_stub!($name, $nargs, DPERR_NOCONNECTION);
        };
        ($name:ident, $nargs:expr, $ret:expr) => {
            #[allow(non_snake_case)]
            pub fn $name(ctx: &mut Context) -> runtime::Cont {
                let esp = ctx.cpu.regs.esp;
                let return_addr = ctx.memory.read::<u32>(esp);
                log::debug!("dplayx {} (ret={return_addr:#x})", stringify!($name));
                ctx.cpu.regs.eax = $ret;
                ctx.cpu.regs.esp = esp.wrapping_add((1 + $nargs) * 4);
                ctx.indirect(return_addr)
            }
        };
    }

    /// Interfaces the object answers for. IDirectPlay2 through IDirectPlay4
    /// (A and W) share one vtable prefix, so the same vtable satisfies all
    /// of them; the legacy IDirectPlay v1 layout differs and is not offered.
    const IIDS: &[GUID] = &[
        IID_IDirectPlay2,
        IID_IDirectPlay2A,
        IID_IDirectPlay3,
        IID_IDirectPlay3A,
        IID_IDirectPlay4,
        IID_IDirectPlay4A,
    ];

    /// QueryInterface(this, riid, ppv).
    pub fn QueryInterface(ctx: &mut Context) -> runtime::Cont {
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
                Some(iid)
                    if iid == IID_IUnknown || iid == IID_NullUnknown || IIDS.contains(&iid) =>
                {
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
        ctx.cpu.regs.esp = esp.wrapping_add(4 * 4);
        ctx.indirect(return_addr)
    }

    /// Initialize(this, lpGUID): a null GUID leaves the object unbound, which
    /// real DirectPlay allows; a specific provider cannot be loaded.
    pub fn Initialize(ctx: &mut Context) -> runtime::Cont {
        let esp = ctx.cpu.regs.esp;
        let return_addr = ctx.memory.read::<u32>(esp);
        let guid = ctx.memory.read::<u32>(esp.wrapping_add(8));
        log::debug!("dplayx Initialize guid={guid:#x} (ret={return_addr:#x})");
        ctx.cpu.regs.eax = if guid == 0 {
            DP_OK
        } else {
            DPERR_NOSERVICEPROVIDER
        };
        ctx.cpu.regs.esp = esp.wrapping_add(3 * 4);
        ctx.indirect(return_addr)
    }

    dp_stub!(AddRef, 1, 1);
    dp_stub!(Release, 1, 0);
    dp_stub!(Close, 1, DP_OK);
    dp_stub!(EnumSessions, 6, DP_OK);
    dp_stub!(EnumConnections, 5, DP_OK);
    dp_stub!(InitializeConnection, 3, DPERR_UNAVAILABLE);
    dp_stub!(AddPlayerToGroup, 3);
    dp_stub!(CreateGroup, 6);
    dp_stub!(CreatePlayer, 7);
    dp_stub!(DeletePlayerFromGroup, 3);
    dp_stub!(DestroyGroup, 2);
    dp_stub!(DestroyPlayer, 2);
    dp_stub!(EnumGroupPlayers, 6);
    dp_stub!(EnumGroups, 5);
    dp_stub!(EnumPlayers, 5);
    dp_stub!(GetCaps, 3);
    dp_stub!(GetGroupData, 5);
    dp_stub!(GetGroupName, 4);
    dp_stub!(GetMessageCount, 3);
    dp_stub!(GetPlayerAddress, 4);
    dp_stub!(GetPlayerCaps, 4);
    dp_stub!(GetPlayerData, 5);
    dp_stub!(GetPlayerName, 4);
    dp_stub!(GetSessionDesc, 3);
    dp_stub!(Open, 3);
    dp_stub!(Receive, 6);
    dp_stub!(Send, 6);
    dp_stub!(SetGroupData, 5);
    dp_stub!(SetGroupName, 4);
    dp_stub!(SetPlayerData, 5);
    dp_stub!(SetPlayerName, 4);
    dp_stub!(SetSessionDesc, 3);
    dp_stub!(AddGroupToGroup, 3);
    dp_stub!(CreateGroupInGroup, 7);
    dp_stub!(DeleteGroupFromGroup, 3);
    dp_stub!(EnumGroupsInGroup, 6);
    dp_stub!(GetGroupConnectionSettings, 5);
    dp_stub!(SecureOpen, 5);
    dp_stub!(SendChatMessage, 5);
    dp_stub!(SetGroupConnectionSettings, 4);
    dp_stub!(StartSession, 3);
    dp_stub!(GetGroupFlags, 3);
    dp_stub!(GetGroupParent, 3);
    dp_stub!(GetPlayerAccount, 5);
    dp_stub!(GetPlayerFlags, 3);
    dp_stub!(GetGroupOwner, 3);
    dp_stub!(SetGroupOwner, 3);
    dp_stub!(SendEx, 10);
    dp_stub!(GetMessageQueue, 6);
    dp_stub!(CancelMessage, 3);
    dp_stub!(CancelPriority, 4);

    pub const VTABLE_FUNCS: [ContFn; 53] = [
        QueryInterface,
        AddRef,
        Release,
        // IDirectPlay2
        AddPlayerToGroup,
        Close,
        CreateGroup,
        CreatePlayer,
        DeletePlayerFromGroup,
        DestroyGroup,
        DestroyPlayer,
        EnumGroupPlayers,
        EnumGroups,
        EnumPlayers,
        EnumSessions,
        GetCaps,
        GetGroupData,
        GetGroupName,
        GetMessageCount,
        GetPlayerAddress,
        GetPlayerCaps,
        GetPlayerData,
        GetPlayerName,
        GetSessionDesc,
        Initialize,
        Open,
        Receive,
        Send,
        SetGroupData,
        SetGroupName,
        SetPlayerData,
        SetPlayerName,
        SetSessionDesc,
        // IDirectPlay3
        AddGroupToGroup,
        CreateGroupInGroup,
        DeleteGroupFromGroup,
        EnumConnections,
        EnumGroupsInGroup,
        GetGroupConnectionSettings,
        InitializeConnection,
        SecureOpen,
        SendChatMessage,
        SetGroupConnectionSettings,
        StartSession,
        GetGroupFlags,
        GetGroupParent,
        GetPlayerAccount,
        GetPlayerFlags,
        // IDirectPlay4
        GetGroupOwner,
        SetGroupOwner,
        SendEx,
        GetMessageQueue,
        CancelMessage,
        CancelPriority,
    ];

    static mut VTABLE: u32 = 0;

    fn vtable(ctx: &mut Context) -> u32 {
        unsafe {
            if VTABLE == 0 {
                let mut kernel32 = kernel32::lock();
                if let Some((addr, blocks)) =
                    init_vtable(ctx, &mut kernel32.process_heap, 0xfafd_2000, &VTABLE_FUNCS)
                {
                    VTABLE = addr;
                    drop(kernel32);
                    add_blocks(ctx, blocks);
                    log::debug!("IDirectPlay4A vtable allocated at {addr:#x}");
                }
            }
            VTABLE
        }
    }

    /// `CoCreateInstance` body for `CLSID_DirectPlay`.
    pub fn create(ctx: &mut Context, riid: u32, ppv: u32) -> u32 {
        if !crate::ddraw::guest_range(ctx, ppv, 4) {
            return E_POINTER;
        }
        let ppv_ptr = crate::Ptr::<u32>::new(ppv);
        let iid = match read_guid(ctx, riid) {
            Some(iid) => iid,
            None => {
                if ppv_ptr.write(&mut ctx.memory, 0).is_none() {
                    return E_POINTER;
                }
                return E_NOINTERFACE;
            }
        };
        if !(iid == IID_IUnknown || iid == IID_NullUnknown || IIDS.contains(&iid)) {
            if ppv_ptr.write(&mut ctx.memory, 0).is_none() {
                return E_POINTER;
            }
            return E_NOINTERFACE;
        }
        let vtable = vtable(ctx);
        if vtable == 0 {
            if ppv_ptr.write(&mut ctx.memory, 0).is_none() {
                return E_POINTER;
            }
            return E_OUTOFMEMORY;
        }
        let kernel32 = kernel32::lock();
        let Some(obj) = kernel32.process_heap.try_alloc(&mut ctx.memory, 4) else {
            return E_OUTOFMEMORY;
        };
        drop(kernel32);
        ctx.memory.write(obj, vtable);
        if ppv_ptr.write(&mut ctx.memory, obj).is_none() {
            return E_POINTER;
        }
        S_OK
    }
}

/// The original `dplayx.dll` exported `DirectPlayCreate`; MM2 does not import
/// it directly, but keep the existing no-op for completeness.
#[win32_derive::dllexport]
pub fn ordinal1(_ctx: &mut Context, _lpGuid: u32, _lplpDirectPlay: u32, _pUnkOuter: u32) -> u32 {
    E_NOTIMPL
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

    fn write_guid(ctx: &mut Context, addr: u32, guid: &GUID) {
        ctx.memory.write::<GUID>(addr, *guid);
    }

    #[test]
    fn query_interface_answers_for_iunknown_and_lobby3a() {
        let mut ctx = context();
        write_guid(&mut ctx, 0x1000, &IID_IUnknown);
        write_guid(&mut ctx, 0x1020, &IID_IDirectPlayLobby3A);

        assert_eq!(
            IDirectPlayLobby3A::QueryInterface(&mut ctx, 0x2000, 0x1000, 0x1100),
            S_OK
        );
        assert_eq!(ctx.memory.read::<u32>(0x1100), 0x2000);

        assert_eq!(
            IDirectPlayLobby3A::QueryInterface(&mut ctx, 0x2000, 0x1020, 0x1100),
            S_OK
        );
        assert_eq!(ctx.memory.read::<u32>(0x1100), 0x2000);

        assert_eq!(
            IDirectPlayLobby3A::QueryInterface(&mut ctx, 0x2000, 0x1000, 0),
            E_POINTER
        );
    }

    #[test]
    fn query_interface_rejects_unknown_iid() {
        let mut ctx = context();
        let unknown = GUID::new(0x1234_5678, 0x9abc, 0xdef0, [1, 2, 3, 4, 5, 6, 7, 8]);
        write_guid(&mut ctx, 0x1000, &unknown);

        assert_eq!(
            IDirectPlayLobby3A::QueryInterface(&mut ctx, 0x2000, 0x1000, 0x1100),
            E_NOINTERFACE
        );
        assert_eq!(ctx.memory.read::<u32>(0x1100), 0);
    }

    #[test]
    fn connect_rejects_low_and_writes_valid_output_pointer() {
        let mut ctx = context();

        assert_eq!(
            IDirectPlayLobby3A::Connect(&mut ctx, 0, 0, 0x500, 0),
            E_POINTER
        );

        assert_eq!(
            IDirectPlayLobby3A::Connect(&mut ctx, 0, 0, 0x3000, 0),
            E_FAIL
        );
        assert_eq!(ctx.memory.read::<u32>(0x3000), 0);

        assert_eq!(IDirectPlayLobby3A::Connect(&mut ctx, 0, 0, 0, 0), E_FAIL);
    }
}
