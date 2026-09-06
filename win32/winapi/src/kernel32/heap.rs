use runtime::Context;

use crate::{
    Ptr,
    dllexport::win32flags,
    heap::Heap,
    kernel32::{self, HANDLE, lock},
};

fn fill_zero(memory: &mut runtime::Memory, addr: u32, len: u32) {
    let Some(end) = (addr as usize).checked_add(len as usize) else {
        return;
    };
    if let Some(dst) = memory.bytes.get_mut(addr as usize..end) {
        dst.fill(0);
    }
}

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
    let Some(heap) = state.heaps.get(&hHeap) else {
        log::error!("HeapAlloc({hHeap:x}): no such heap");
        return 0;
    };
    let Some(addr) = heap.try_alloc(&mut ctx.memory, dwBytes) else {
        // HeapAlloc returns NULL when the heap is exhausted.
        return 0;
    };
    drop(state);
    if addr != 0 && dwFlags.contains(HEAP_FLAGS::ZERO_MEMORY) {
        fill_zero(&mut ctx.memory, addr, dwBytes);
    }
    addr
}

#[win32_derive::dllexport]
pub fn HeapCreate(
    ctx: &mut Context,
    _flOptions: u32, /* HEAP_FLAGS */
    dwInitialSize: u32,
    _dwMaximumSize: u32,
) -> HANDLE {
    // Currently none of the flags will affect behavior, but we might need to revisit this
    // with exceptions or threads support...
    let size = dwInitialSize.max(20 << 20);
    // The heap's address range has to fit inside emulated memory; report
    // failure with NULL rather than vending an unbacked region.
    let limit = u32::try_from(ctx.memory.bytes.len()).unwrap_or(u32::MAX);
    let mut state = kernel32::lock();
    let Some(addr) = state.mappings.try_alloc("HeapCreate".into(), size, limit) else {
        return 0;
    };
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
    _ctx: &mut Context,
    hHeap: HANDLE,
    dwFlags: u32, /* HEAP_FLAGS */
    lpMem: Ptr<()>,
) -> u32 {
    if dwFlags != 0 {
        log::warn!("HeapFree flags {dwFlags:x}");
    }
    let state = kernel32::lock();
    let Some(heap) = state.heaps.get(&hHeap) else {
        log::error!("HeapSize({hHeap:x}): no such heap");
        return u32::MAX;
    };
    // HeapSize documents (SIZE_T)-1 for a pointer that is not a live block.
    heap.block_size(lpMem.addr).unwrap_or(u32::MAX)
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
    let Some(heap) = state.heaps.get(&hHeap) else {
        log::error!("HeapFree({hHeap:x}): no such heap");
        return false;
    };
    heap.free(&mut ctx.memory, lpMem.addr)
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
        return heap.try_alloc(&mut ctx.memory, dwBytes).unwrap_or(0);
    }
    if dwBytes == 0 {
        heap.free(&mut ctx.memory, lpMem.addr);
        return 0;
    }
    // Validate lpMem against the live-block table rather than the in-band
    // header so a stale or interior pointer cannot feed a garbage size to
    // copy_within.
    let Some(old_size) = heap.block_size(lpMem.addr) else {
        // lpMem is not a live block on this heap.
        return 0;
    };
    if dwBytes <= old_size {
        // Shrinking always succeeds in place.
        return lpMem.addr;
    }
    if dwFlags & HEAP_REALLOC_IN_PLACE_ONLY != 0 {
        // The free-list allocator cannot extend a live block in place.
        return 0;
    }
    let Some(new_addr) = heap.try_alloc(&mut ctx.memory, dwBytes) else {
        return 0;
    };
    ctx.memory.bytes.copy_within(
        lpMem.addr as usize..lpMem.addr as usize + old_size as usize,
        new_addr as usize,
    );
    if dwFlags & HEAP_FLAGS::ZERO_MEMORY.bits() != 0 {
        let grown = new_addr + old_size;
        fill_zero(&mut ctx.memory, grown, dwBytes - old_size);
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
    // Handles are identity pointers and GlobalLock/Unlock are no-ops, so
    // GMEM_MOVEABLE is satisfied transparently: blocks never move.
    let Some(ptr) = lock().process_heap.try_alloc(&mut ctx.memory, dwBytes) else {
        return 0;
    };
    if uFlags.contains(GMEM::ZEROINIT) {
        fill_zero(&mut ctx.memory, ptr, dwBytes);
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
        kernel32::ensure_test_state();
        let mut ctx = context();
        // Each test uses a distinct heap base so parallel tests cannot evict
        // one another's live-block table through the shared `heaps` map.
        let heap = crate::heap::Heap::new(0x100_000, 0x10_000);
        let hheap = heap.addr;
        lock().heaps.insert(hheap, heap);

        let mem = HeapAlloc(&mut ctx, hheap, HEAP_FLAGS::empty(), 8);
        ctx.memory[mem..][..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);

        // HeapSize reports the usable payload size from the live-block
        // table (the 8-byte request pads to a 16-byte block, i.e. 12
        // usable bytes), not whatever a guest wrote over the in-band header.
        ctx.memory.write::<u32>(mem - 4, 0xdead_beef);
        assert_eq!(HeapSize(&mut ctx, hheap, 0, Ptr::new(mem)), 12);
        ctx.memory.write::<u32>(mem - 4, 16); // restore the real header

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

    #[test]
    fn heap_apis_degrade_on_bad_handles() {
        kernel32::ensure_test_state();
        let mut ctx = context();
        let heap = crate::heap::Heap::new(0x200_000, 0x10_000);
        let hheap = heap.addr;
        lock().heaps.insert(hheap, heap);

        // An unknown heap handle fails without panicking.
        let bad_heap = 0x7777;
        assert_eq!(HeapAlloc(&mut ctx, bad_heap, HEAP_FLAGS::empty(), 8), 0);
        assert_eq!(HeapSize(&mut ctx, bad_heap, 0, Ptr::new(0)), u32::MAX);
        assert!(!HeapFree(&mut ctx, bad_heap, 0, Ptr::new(0)));

        // A bad lpMem fails without panicking.
        assert!(!HeapFree(&mut ctx, hheap, 0, Ptr::new(0)));
        assert_eq!(HeapSize(&mut ctx, hheap, 0, Ptr::new(0)), u32::MAX);
        assert_eq!(HeapReAlloc(&mut ctx, hheap, 0, Ptr::new(0x40), 8), 0);
        lock().heaps.remove(&hheap);
    }

    #[test]
    fn heap_alloc_rejects_overflowing_sizes() {
        kernel32::ensure_test_state();
        let mut ctx = context();
        let heap = crate::heap::Heap::new(0x300_000, 0x10_000);
        let hheap = heap.addr;
        lock().heaps.insert(hheap, heap);

        // A request whose size+4 header adjustment overflows must fail
        // rather than panic on the ZERO_MEMORY fill or vend a tiny block
        // the guest believes is nearly 4 GiB.
        assert_eq!(
            HeapAlloc(&mut ctx, hheap, HEAP_FLAGS::ZERO_MEMORY, u32::MAX - 3),
            0
        );
        assert_eq!(HeapAlloc(&mut ctx, hheap, HEAP_FLAGS::empty(), u32::MAX), 0);
        assert_eq!(GlobalAlloc(&mut ctx, GMEM::ZEROINIT, u32::MAX - 3), 0);
        lock().heaps.remove(&hheap);
    }
}
