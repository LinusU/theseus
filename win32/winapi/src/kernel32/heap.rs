use runtime::Context;

use crate::{
    Ptr,
    dllexport::win32flags,
    heap::Heap,
    kernel32::{self, HANDLE, lock},
};

win32flags! {
    pub struct HEAP_FLAGS {
        const NO_SERIALIZE        = 0x01;
        const GENERATE_EXCEPTIONS = 0x04;
        const ZERO_MEMORY         = 0x08;
    }
}

#[win32_derive::dllexport]
pub fn HeapAlloc(ctx: &mut Context, hHeap: HANDLE, dwFlags: HEAP_FLAGS, dwBytes: u32) -> u32 {
    let state = kernel32::lock();
    let heap = state.heaps.get(&hHeap).unwrap();
    let addr = heap.alloc(&mut ctx.memory, dwBytes);
    drop(state);
    if addr != 0 && dwFlags.contains(HEAP_FLAGS::ZERO_MEMORY) {
        ctx.memory[addr..][..dwBytes as usize].fill(0);
    }
    addr
}

#[win32_derive::dllexport]
pub fn HeapCreate(
    _ctx: &mut Context,
    _flOptions: u32, /* HEAP_FLAGS */
    dwInitialSize: u32,
    _dwMaximumSize: u32,
) -> HANDLE {
    // Currently none of the flags will affect behavior, but we might need to revisit this
    // with exceptions or threads support...
    let size = dwInitialSize.max(20 << 20);
    let mut state = kernel32::lock();
    let addr = state.mappings.alloc("HeapCreate".into(), size);
    let heap = Heap::new(addr, size);
    state.heaps.insert(addr, heap);
    addr
}

#[win32_derive::dllexport]
pub fn HeapDestroy(_ctx: &mut Context, hHeap: HANDLE) -> bool {
    let mut state = kernel32::lock();
    if state.heaps.remove(&hHeap).is_none() {
        log::warn!("HeapDestroy({hHeap:x}): no such heap");
        return false;
    }
    true
}

#[win32_derive::dllexport]
pub fn HeapSize(
    ctx: &mut Context,
    hHeap: HANDLE,
    dwFlags: u32, /* HEAP_FLAGS */
    lpMem: Ptr<()>,
) -> u32 {
    if dwFlags != 0 {
        log::warn!("HeapFree flags {dwFlags:x}");
    }
    let state = kernel32::lock();
    let heap = state.heaps.get(&hHeap).unwrap();
    heap.size(&mut ctx.memory, lpMem.addr)
}

#[win32_derive::dllexport]
pub fn HeapFree(
    ctx: &mut Context,
    hHeap: HANDLE,
    dwFlags: u32, /* HEAP_FLAGS */
    lpMem: Ptr<()>,
) -> bool {
    if dwFlags != 0 {
        log::warn!("HeapFree flags {dwFlags:x}");
    }
    let state = kernel32::lock();
    let heap = state.heaps.get(&hHeap).unwrap();
    heap.free(&mut ctx.memory, lpMem.addr);
    true
}

const HEAP_REALLOC_IN_PLACE_ONLY: u32 = 0x10;

#[win32_derive::dllexport]
pub fn HeapReAlloc(
    ctx: &mut Context,
    hHeap: HANDLE,
    dwFlags: u32, /* HEAP_FLAGS */
    lpMem: Ptr<()>,
    dwBytes: u32,
) -> u32 {
    let known = HEAP_FLAGS::NO_SERIALIZE.bits()
        | HEAP_FLAGS::GENERATE_EXCEPTIONS.bits()
        | HEAP_FLAGS::ZERO_MEMORY.bits()
        | HEAP_REALLOC_IN_PLACE_ONLY;
    if dwFlags & !known != 0 {
        log::warn!("HeapReAlloc flags: {:x}", dwFlags);
    }
    let state = kernel32::lock();
    let Some(heap) = state.heaps.get(&hHeap) else {
        log::error!("HeapReAlloc({hHeap:x}): no such heap");
        return 0;
    };
    // A null lpMem behaves like HeapAlloc.
    if lpMem.addr == 0 {
        return heap.alloc(&mut ctx.memory, dwBytes);
    }
    if dwBytes == 0 {
        heap.free(&mut ctx.memory, lpMem.addr);
        return 0;
    }
    let old_size = heap.size(&mut ctx.memory, lpMem.addr);
    if dwBytes <= old_size {
        // Shrinking always succeeds in place.
        return lpMem.addr;
    }
    if dwFlags & HEAP_REALLOC_IN_PLACE_ONLY != 0 {
        // The free-list allocator cannot extend a live block in place.
        return 0;
    }
    let new_addr = heap.alloc(&mut ctx.memory, dwBytes);
    ctx.memory.bytes.copy_within(
        lpMem.addr as usize..lpMem.addr as usize + old_size as usize,
        new_addr as usize,
    );
    if dwFlags & HEAP_FLAGS::ZERO_MEMORY.bits() != 0 {
        let grown = new_addr + old_size;
        ctx.memory[grown..][..(dwBytes - old_size) as usize].fill(0);
    }
    heap.free(&mut ctx.memory, lpMem.addr);
    new_addr
}

win32flags! {
    pub struct GMEM {
        const MOVEABLE    = 0x0002;
        // const NOCOMPACT   = 0x0010;
        // const NODISCARD   = 0x0020;
        const ZEROINIT    = 0x0040;
        // const MODIFY      = 0x0080;
        // const DISCARDABLE = 0x0100;
        // const NOT_BANKED  = 0x1000;
        // const SHARE       = 0x2000;
        // const DDESHARE    = 0x2000;
        // const NOTIFY      = 0x4000;
        // Lots of obsolete flags, ignore them
        const _ = !0;
    }
}

#[win32_derive::dllexport]
pub fn GlobalAlloc(ctx: &mut Context, uFlags: GMEM, dwBytes: u32) -> u32 {
    assert!(!uFlags.contains(GMEM::MOVEABLE));
    let ptr = lock().process_heap.alloc(&mut ctx.memory, dwBytes);
    if uFlags.contains(GMEM::ZEROINIT) {
        ctx.memory[ptr..][..dwBytes as usize].fill(0);
    }
    ptr
}

#[win32_derive::dllexport]
pub fn GlobalFree(ctx: &mut Context, hMem: Ptr<()>) -> u32 {
    lock().process_heap.free(&mut ctx.memory, hMem.addr);
    0 // success
}

// GlobalAlloc only hands out fixed (non-moveable) memory, so handles and
// pointers are the same value.

#[win32_derive::dllexport]
pub fn GlobalLock(_ctx: &mut Context, hMem: u32) -> u32 {
    hMem
}

#[win32_derive::dllexport]
pub fn GlobalUnlock(_ctx: &mut Context, _hMem: u32) -> bool {
    true
}

#[win32_derive::dllexport]
pub fn GlobalHandle(_ctx: &mut Context, pMem: u32) -> u32 {
    pMem
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::{BlockCache, CPU, Memory};

    fn context() -> Context {
        Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x400_000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    #[test]
    fn heap_realloc_grows_and_preserves() {
        kernel32::init_state(0x400000, 0..0);
        let mut ctx = context();
        let heap = crate::heap::Heap::new(0x100_000, 0x10_000);
        let hheap = heap.addr;
        lock().heaps.insert(hheap, heap);

        let mem = HeapAlloc(&mut ctx, hheap, HEAP_FLAGS::empty(), 8);
        ctx.memory[mem..][..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);

        // Growing relocates but keeps the old contents.
        let grown = HeapReAlloc(&mut ctx, hheap, 0, Ptr::new(mem), 32);
        assert_ne!(grown, 0);
        assert_eq!(&ctx.memory[grown..][..8], &[1, 2, 3, 4, 5, 6, 7, 8]);

        // Shrinking keeps the same address.
        let shrunk = HeapReAlloc(&mut ctx, hheap, 0, Ptr::new(grown), 4);
        assert_eq!(shrunk, grown);

        // HEAP_REALLOC_IN_PLACE_ONLY fails rather than moving the block.
        let in_place = HeapReAlloc(
            &mut ctx,
            hheap,
            HEAP_REALLOC_IN_PLACE_ONLY,
            Ptr::new(grown),
            0x8000,
        );
        assert_eq!(in_place, 0);

        // Destroying the heap removes it from the state.
        assert!(HeapDestroy(&mut ctx, hheap));
        assert!(!HeapDestroy(&mut ctx, hheap));
    }
}
