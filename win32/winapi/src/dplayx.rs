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

pub(crate) fn read_guid(ctx: &Context, addr: u32) -> Option<GUID> {
    Ptr::<GUID>::new(addr).read(&ctx.memory)
}

pub(crate) fn init_vtable(
    ctx: &mut Context,
    heap: &mut Heap,
    base: u32,
    funcs: &[ContFn],
) -> (u32, Vec<(u32, ContFn)>) {
    let size = (funcs.len() * 4) as u32;
    let addr = heap.alloc(&mut ctx.memory, size);
    let mut blocks = Vec::with_capacity(funcs.len());
    for (i, &func) in funcs.iter().enumerate() {
        let fn_addr = base + i as u32;
        ctx.memory.write::<u32>(addr + i as u32 * 4, fn_addr);
        blocks.push((fn_addr, func));
    }
    (addr, blocks)
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

    pub unsafe fn init_vtables(ctx: &mut Context) {
        if unsafe { VTABLE } == 0 {
            let mut kernel32 = kernel32::lock();
            let (addr, blocks) =
                init_vtable(ctx, &mut kernel32.process_heap, 0xfafd_1000, &VTABLE_FUNCS);
            unsafe { VTABLE = addr };
            drop(kernel32);
            add_blocks(ctx, blocks);
            log::debug!("IDirectPlayLobby3A vtable allocated at {addr:#x}");
        }
    }

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> u32 {
        let addr = heap.alloc(&mut ctx.memory, 4);
        ctx.memory.write(addr, unsafe { VTABLE });
        addr
    }

    pub fn create(ctx: &mut Context, riid: u32, ppv: u32) -> u32 {
        if ppv == 0 {
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
            let addr = new(ctx, &mut kernel32.process_heap);
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
        if ppv == 0 {
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
        if lplpDP2 != 0 {
            _ctx.memory.write::<u32>(lplpDP2, 0);
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
        if lpdwAddressBufferSize != 0 {
            _ctx.memory.write::<u32>(lpdwAddressBufferSize, 0);
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
        lpConnectionSettings: u32,
        lpdwSize: u32,
    ) -> u32 {
        if lpdwSize == 0 {
            return E_POINTER;
        }
        if lpConnectionSettings != 0 {
            _ctx.memory.write::<u32>(lpdwSize, 0);
        } else {
            _ctx.memory.write::<u32>(lpdwSize, 0);
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
        if lpdwMessageFlags != 0 {
            _ctx.memory.write::<u32>(lpdwMessageFlags, 0);
        }
        if lpdwDataSize != 0 {
            _ctx.memory.write::<u32>(lpdwDataSize, 0);
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
        if lpdwAppID != 0 {
            _ctx.memory.write::<u32>(lpdwAppID, 0);
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
        if lpdwAddressSize != 0 {
            _ctx.memory.write::<u32>(lpdwAddressSize, 0);
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
        if lplpDP3 != 0 {
            _ctx.memory.write::<u32>(lplpDP3, 0);
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
}
