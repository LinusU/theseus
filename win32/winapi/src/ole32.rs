use runtime::Context;

use crate::{Ptr, ddraw::GUID, dmusic, dplayx};

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
    const E_INVALIDARG: u32 = 0x8007_0057;

    if ppv.addr < 0x1000 || !crate::ddraw::guest_range(ctx, ppv.addr, 4) {
        return E_INVALIDARG;
    }

    if _rclsid != 0
        && let Some(clsid) = Ptr::<GUID>::new(_rclsid).read(&ctx.memory)
    {
        let iid = if _riid != 0 {
            Ptr::<GUID>::new(_riid).read(&ctx.memory)
        } else {
            None
        };
        log::debug!("CoCreateInstance clsid={clsid:?} riid={_riid:#x} iid={iid:?}");
        if clsid == dplayx::CLSID_DirectPlayLobby {
            return dplayx::IDirectPlayLobby3A::create(ctx, _riid, ppv.addr);
        }
        if clsid == dplayx::CLSID_DirectPlay {
            return dplayx::directplay::create(ctx, _riid, ppv.addr);
        }
        if clsid == dmusic::CLSID_DirectMusicPerformance {
            return dmusic::performance::create(ctx, _riid, ppv.addr);
        }
        if clsid == dmusic::CLSID_DirectMusicLoader {
            return dmusic::loader::create(ctx, _riid, ppv.addr);
        }
        if clsid == dmusic::CLSID_DirectMusicComposer {
            return dmusic::composer::create(ctx, _riid, ppv.addr);
        }
        if clsid == dmusic::CLSID_DirectMusicSegment {
            let ret = dmusic::segment::create(ctx, _riid, ppv.addr);
            log::debug!("CoCreateInstance(CLSID_DirectMusicSegment) = {ret:#x}");
            return ret;
        }
        log::debug!("CoCreateInstance: unregistered class {clsid:?}");
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
