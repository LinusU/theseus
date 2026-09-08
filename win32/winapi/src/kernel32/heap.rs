use runtime::Context;

use crate::{
    Ptr,
    dllexport::win32flags,
    heap::Heap,
    kernel32::{self, HANDLE, lock},
    stub,
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
pub fn HeapDestroy(_ctx: &mut Context, _hHeap: HANDLE) -> bool {
    stub!(true) // success
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

#[win32_derive::dllexport]
pub fn HeapReAlloc(
    ctx: &mut Context,
    hHeap: HANDLE,
    dwFlags: u32, /* HEAP_FLAGS */
    lpMem: Ptr<()>,
    dwBytes: u32,
) -> u32 {
    const HEAP_ZERO_MEMORY: u32 = 0x08;
    const HEAP_REALLOC_IN_PLACE_ONLY: u32 = 0x10;
    if dwFlags & !(HEAP_ZERO_MEMORY | HEAP_REALLOC_IN_PLACE_ONLY) != 0 {
        log::warn!("HeapReAlloc flags: {:x}", dwFlags);
    }
    let state = kernel32::lock();
    let Some(heap) = state.heaps.get(&hHeap) else {
        log::error!("HeapReAlloc({hHeap:?}): no such heap");
        return 0;
    };
    let old_size = heap.size(&mut ctx.memory, lpMem.addr);
    if dwFlags & HEAP_REALLOC_IN_PLACE_ONLY != 0 && dwBytes > old_size {
        return 0;
    }
    let new_addr = heap.alloc(&mut ctx.memory, dwBytes);
    let copy = old_size.min(dwBytes) as usize;
    ctx.memory.bytes.copy_within(
        lpMem.addr as usize..lpMem.addr as usize + copy,
        new_addr as usize,
    );
    if dwFlags & HEAP_ZERO_MEMORY != 0 && dwBytes > old_size {
        ctx.memory[new_addr + old_size..new_addr + dwBytes].fill(0);
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
    // Moveable memory is handed out as fixed: the handle is the pointer, which
    // GlobalLock returns unchanged, and nothing ever moves.
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

/// Reallocate a block on the process heap: allocate, copy, free.
fn process_heap_realloc(ctx: &mut Context, addr: u32, new_size: u32, zero_init: bool) -> u32 {
    let kernel32 = lock();
    let heap = &kernel32.process_heap;
    let old_size = heap.size(&mut ctx.memory, addr);
    let new_addr = heap.alloc(&mut ctx.memory, new_size);
    let copy = old_size.min(new_size) as usize;
    ctx.memory
        .bytes
        .copy_within(addr as usize..addr as usize + copy, new_addr as usize);
    if zero_init && new_size > old_size {
        ctx.memory[new_addr + old_size..new_addr + new_size].fill(0);
    }
    heap.free(&mut ctx.memory, addr);
    new_addr
}

#[win32_derive::dllexport]
pub fn GlobalReAlloc(ctx: &mut Context, hMem: u32, dwBytes: u32, uFlags: GMEM) -> u32 {
    // GMEM_MODIFY (0x80) changes flags without resizing; nothing to do.
    if uFlags.bits() & 0x80 != 0 {
        return hMem;
    }
    process_heap_realloc(ctx, hMem, dwBytes, uFlags.contains(GMEM::ZEROINIT))
}

// LocalAlloc and friends share the process heap with GlobalAlloc; the LMEM_*
// flags have the same values as their GMEM_* counterparts.

#[win32_derive::dllexport]
pub fn LocalAlloc(ctx: &mut Context, uFlags: u32, uBytes: u32) -> u32 {
    // Like GlobalAlloc, only fixed memory is handed out, so ignore LMEM_MOVEABLE.
    let flags = GMEM::from_bits_retain(uFlags & !GMEM::MOVEABLE.bits());
    GlobalAlloc(ctx, flags, uBytes)
}

#[win32_derive::dllexport]
pub fn LocalReAlloc(ctx: &mut Context, hMem: u32, uBytes: u32, uFlags: u32) -> u32 {
    let flags = GMEM::from_bits_retain(uFlags & !GMEM::MOVEABLE.bits());
    GlobalReAlloc(ctx, hMem, uBytes, flags)
}

#[win32_derive::dllexport]
pub fn LocalFree(ctx: &mut Context, hMem: Ptr<()>) -> u32 {
    if hMem.addr == 0 {
        return 0;
    }
    GlobalFree(ctx, hMem)
}

#[win32_derive::dllexport]
pub fn GlobalFlags(_ctx: &mut Context, _hMem: u32) -> u32 {
    0 // fixed memory, lock count 0
}
