use runtime::Context;

use crate::{Ptr, ddraw::GUID, dplayx};

#[win32_derive::dllexport]
pub fn CoInitialize(_ctx: &mut Context, _pvReserved: u32) -> u32 /* HRESULT */ {
    0 // S_OK
}

#[win32_derive::dllexport]
pub fn CoUninitialize(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn CoCreateInstance(
    ctx: &mut Context,
    _rclsid: u32,
    _pUnkOuter: u32,
    _dwClsContext: u32,
    _riid: u32,
    ppv: Ptr<u32>,
) -> u32 /* HRESULT */ {
    const REGDB_E_CLASSNOTREG: u32 = 0x8004_0154;

    if _rclsid != 0 {
        if let Some(clsid) = Ptr::<GUID>::new(_rclsid).read(&ctx.memory) {
            if clsid == dplayx::CLSID_DirectPlayLobby {
                return dplayx::IDirectPlayLobby3A::create(ctx, _riid, ppv.addr);
            }
        }
    }

    // There is no COM class registry or interface model; report the class as
    // unregistered and null the caller's out pointer as the contract requires.
    ppv.write(&mut ctx.memory, 0);
    REGDB_E_CLASSNOTREG
}

#[cfg(test)]
mod tests {
    use super::CoCreateInstance;
    use crate::Ptr;
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

    #[test]
    fn cocreate_instance_fails_without_a_com_registry() {
        let mut ctx = context();
        ctx.memory.write::<u32>(0x1000, 0xdead_beef);

        let hr = CoCreateInstance(&mut ctx, 0, 0, 0, 0, Ptr::new(0x1000));
        assert_eq!(hr, 0x8004_0154); // REGDB_E_CLASSNOTREG
        assert_eq!(ctx.memory.read::<u32>(0x1000), 0);
    }
}
