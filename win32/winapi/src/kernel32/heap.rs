use runtime::Context;

use crate::{
    Ptr,
    dllexport::win32flags,
    heap::{Heap, Movable},
    kernel32::{self, HANDLE, lock, set_last_error},
    stub,
};

// Win32 error codes used by the global-memory APIs. Values from winerror.h.
const ERROR_INVALID_HANDLE: u32 = 6;
const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_DISCARDED: u32 = 157;
const ERROR_NOT_LOCKED: u32 = 158;
const NO_ERROR: u32 = 0;

win32flags! {
    pub struct GMEM {
        const MOVEABLE    = 0x0002;
        const NOCOMPACT   = 0x0010;
        const NODISCARD   = 0x0020;
        const ZEROINIT    = 0x0040;
        const MODIFY      = 0x0080;
        const DISCARDABLE = 0x0100;
        // const NOT_BANKED  = 0x1000;
        // const SHARE       = 0x2000;
        // const DDESHARE    = 0x2000;
        // const NOTIFY      = 0x4000;
        const DISCARDED   = 0x4000;
        const INVALID_HANDLE = 0x8000;
        // Lots of obsolete flags, ignore them
        const _ = !0;
    }
}

/// True when `ptr` is the data-pointer (base) of a live fixed allocation in the
/// process heap, i.e. a valid pointer-valued handle. Uses the allocator's
/// authoritative live-allocation registry rather than inspecting header bytes,
/// so ordinary payloads can never impersonate a header. A movable backing
/// pointer is excluded via the reverse-lookup map, so a fixed handle that has
/// been converted to movable no longer passes. Interior pointers are rejected
/// because only exact allocation bases are in the registry.
fn is_fixed_ptr(heap: &Heap, by_ptr: &std::collections::HashMap<u32, u32>, ptr: u32) -> bool {
    if by_ptr.contains_key(&ptr) {
        return false;
    }
    heap.contains(ptr)
}

#[win32_derive::dllexport]
pub fn GlobalAlloc(ctx: &mut Context, uFlags: GMEM, dwBytes: u32) -> u32 {
    let mut state = lock();
    let heap_end = state.process_heap.addr + state.process_heap.size;
    let movable = uFlags.contains(GMEM::MOVEABLE);
    let zeroinit = uFlags.contains(GMEM::ZEROINIT);
    let discardable = uFlags.contains(GMEM::DISCARDABLE);

    if movable {
        if dwBytes == 0 {
            // A zero-size movable allocation is a valid, nonzero discarded
            // handle with no backing storage. Retain the requested discardable
            // attribute, matching the nonzero-allocation-then-discard path so
            // direct zero-size allocation reports the same attribute. This is
            // an existing compatibility choice (Wine retains and reports the
            // obsolete GMEM_DISCARDABLE flag) rather than an unqualified
            // requirement of modern Windows.
            let obj = Movable {
                ptr: 0,
                size: 0,
                lock: 0,
                discardable,
            };
            return match state.global_mem.add(heap_end, obj) {
                Some(h) => h,
                None => {
                    set_last_error(ctx, ERROR_NOT_ENOUGH_MEMORY);
                    0
                }
            };
        }
        // Reserve a handle before allocating backing, so a failed reservation
        // leaves no backing storage to roll back.
        let Some(handle) = state.global_mem.reserve(heap_end) else {
            set_last_error(ctx, ERROR_NOT_ENOUGH_MEMORY);
            return 0;
        };
        let Some(ptr) = state.process_heap.try_alloc(&mut ctx.memory, dwBytes) else {
            set_last_error(ctx, ERROR_NOT_ENOUGH_MEMORY);
            return 0;
        };
        let size = state.process_heap.size(&mut ctx.memory, ptr);
        if zeroinit {
            // GlobalSize reports the rounded usable size; zero every exposed
            // byte, not merely the requested count.
            ctx.memory[ptr..][..size as usize].fill(0);
        }
        let obj = Movable {
            ptr,
            size,
            lock: 0,
            discardable,
        };
        state.global_mem.register(handle, obj);
        handle
    } else {
        // Fixed memory: the handle is the data pointer.
        // GlobalAlloc(GMEM_FIXED, 0) still returns a valid pointer (Wine
        // allocates a minimum of one byte; LocalAlloc allows 0-size fixed).
        let size = dwBytes.max(1);
        let Some(ptr) = state.process_heap.try_alloc(&mut ctx.memory, size) else {
            set_last_error(ctx, ERROR_NOT_ENOUGH_MEMORY);
            return 0;
        };
        if zeroinit {
            // A zero-byte fixed allocation still exposes the rounded usable
            // minimum; clear all of it.
            let usable = state.process_heap.size(&mut ctx.memory, ptr);
            ctx.memory[ptr..][..usable as usize].fill(0);
        }
        ptr
    }
}

#[win32_derive::dllexport]
pub fn GlobalFree(ctx: &mut Context, hMem: Ptr<()>) -> u32 {
    let h = hMem.addr;
    if h == 0 {
        // Null-handle no-op, success.
        return 0;
    }
    let mut state = lock();
    if let Some(obj) = state.global_mem.movable.remove(&h) {
        if obj.ptr != 0 {
            state.process_heap.free(&mut ctx.memory, obj.ptr);
            state.global_mem.by_ptr.remove(&obj.ptr);
        }
        return 0;
    }
    if is_fixed_ptr(&state.process_heap, &state.global_mem.by_ptr, h) {
        state.process_heap.free(&mut ctx.memory, h);
        return 0;
    }
    set_last_error(ctx, ERROR_INVALID_HANDLE);
    h // failure: return the original handle
}

#[win32_derive::dllexport]
pub fn GlobalLock(ctx: &mut Context, hMem: u32) -> u32 {
    if hMem == 0 {
        return 0;
    }
    let mut state = lock();
    if let Some(obj) = state.global_mem.movable.get_mut(&hMem) {
        if obj.ptr == 0 {
            set_last_error(ctx, ERROR_DISCARDED);
            return 0;
        }
        // Saturation: incrementing past 255 would wrap to zero and make a
        // locked object look unlocked, so clamp at 255.
        if obj.lock != u8::MAX {
            obj.lock += 1;
        }
        return obj.ptr;
    }
    if is_fixed_ptr(&state.process_heap, &state.global_mem.by_ptr, hMem) {
        return hMem;
    }
    set_last_error(ctx, ERROR_INVALID_HANDLE);
    0
}

#[win32_derive::dllexport]
pub fn GlobalUnlock(ctx: &mut Context, hMem: u32) -> bool {
    let mut state = lock();
    if let Some(obj) = state.global_mem.movable.get_mut(&hMem) {
        if obj.lock > 0 {
            obj.lock -= 1;
            if obj.lock > 0 {
                return true;
            }
            // Final unlock: memory object is now unlocked.
            set_last_error(ctx, NO_ERROR);
            return false;
        }
        // Already unlocked.
        set_last_error(ctx, ERROR_NOT_LOCKED);
        return false;
    }
    if is_fixed_ptr(&state.process_heap, &state.global_mem.by_ptr, hMem) {
        return true;
    }
    set_last_error(ctx, ERROR_INVALID_HANDLE);
    false
}

#[win32_derive::dllexport]
pub fn GlobalHandle(ctx: &mut Context, pMem: u32) -> u32 {
    let state = lock();
    if let Some(&handle) = state.global_mem.by_ptr.get(&pMem) {
        return handle;
    }
    if is_fixed_ptr(&state.process_heap, &state.global_mem.by_ptr, pMem) {
        return pMem;
    }
    set_last_error(ctx, ERROR_INVALID_HANDLE);
    0
}

#[win32_derive::dllexport]
pub fn GlobalSize(ctx: &mut Context, hMem: u32) -> u32 {
    let state = lock();
    if let Some(obj) = state.global_mem.movable.get(&hMem) {
        return obj.size; // discarded objects report zero
    }
    if is_fixed_ptr(&state.process_heap, &state.global_mem.by_ptr, hMem) {
        return state.process_heap.size(&mut ctx.memory, hMem);
    }
    set_last_error(ctx, ERROR_INVALID_HANDLE);
    0
}

#[win32_derive::dllexport]
pub fn GlobalFlags(ctx: &mut Context, hMem: u32) -> u32 {
    let state = lock();
    if let Some(obj) = state.global_mem.movable.get(&hMem) {
        // Lock count lives in the low byte; GMEM_DISCARDABLE/DISCARDED in the
        // high byte of the low word. Do not echo the original allocation flags:
        // GMEM_MOVEABLE (0x2) overlaps the lock-count byte.
        let mut flags = obj.lock as u32;
        if obj.discardable {
            flags |= GMEM::DISCARDABLE.bits();
        }
        if obj.ptr == 0 {
            flags |= GMEM::DISCARDED.bits();
        }
        return flags;
    }
    if is_fixed_ptr(&state.process_heap, &state.global_mem.by_ptr, hMem) {
        // Fixed objects have a zero lock count and no flags.
        return 0;
    }
    set_last_error(ctx, ERROR_INVALID_HANDLE);
    GMEM::INVALID_HANDLE.bits()
}

/// Copy `len` bytes from `src` to `dst` in guest memory.
fn copy_bytes(ctx: &mut Context, src: u32, dst: u32, len: u32) {
    let src = src as usize;
    let dst = dst as usize;
    let len = len as usize;
    ctx.memory.bytes.copy_within(src..src + len, dst);
}

#[win32_derive::dllexport]
pub fn GlobalReAlloc(ctx: &mut Context, hMem: u32, dwBytes: u32, uFlags: GMEM) -> u32 {
    let mut state = lock();
    let heap_end = state.process_heap.addr + state.process_heap.size;
    let zeroinit = uFlags.contains(GMEM::ZEROINIT);
    let modify = uFlags.contains(GMEM::MODIFY);
    let moveable = uFlags.contains(GMEM::MOVEABLE);
    let discardable = uFlags.contains(GMEM::DISCARDABLE);

    if let Some(obj) = state.global_mem.movable.get(&hMem) {
        let obj = obj.clone();
        if modify {
            // Attribute-only: ignore dwBytes.
            if discardable {
                state.global_mem.movable.get_mut(&hMem).unwrap().discardable = true;
            }
            return hMem;
        }
        if discardable {
            set_last_error(ctx, ERROR_INVALID_PARAMETER);
            return 0;
        }
        if dwBytes == 0 {
            // Discard operation (GlobalDiscard = GlobalReAlloc(h, 0, GMEM_MOVEABLE)).
            if !moveable {
                set_last_error(ctx, ERROR_INVALID_PARAMETER);
                return 0;
            }
            if obj.lock > 0 {
                // Reject discarding a locked object without losing its data.
                set_last_error(ctx, ERROR_INVALID_PARAMETER);
                return 0;
            }
            if obj.ptr != 0 {
                state.process_heap.free(&mut ctx.memory, obj.ptr);
                state.global_mem.by_ptr.remove(&obj.ptr);
            }
            let m = state.global_mem.movable.get_mut(&hMem).unwrap();
            m.ptr = 0;
            m.size = 0;
            return hMem;
        }

        // Resize with a positive size.
        let old_ptr = obj.ptr;
        let old_size = obj.size;
        let allow_move = moveable || obj.lock == 0;
        let new_ptr = if old_ptr == 0 {
            // Discarded object: restore storage under the same handle.
            let Some(p) = state.process_heap.try_alloc(&mut ctx.memory, dwBytes) else {
                set_last_error(ctx, ERROR_NOT_ENOUGH_MEMORY);
                return 0;
            };
            p
        } else {
            match state
                .process_heap
                .try_realloc_in_place(&mut ctx.memory, old_ptr, dwBytes)
            {
                Some(p) => p,
                None if allow_move => {
                    // Relocate: allocate a replacement first, copy the retained
                    // prefix, then free the old backing (transactional).
                    let Some(p) = state.process_heap.try_alloc(&mut ctx.memory, dwBytes) else {
                        set_last_error(ctx, ERROR_NOT_ENOUGH_MEMORY);
                        return 0;
                    };
                    let copy = old_size.min(dwBytes);
                    copy_bytes(ctx, old_ptr, p, copy);
                    state.process_heap.free(&mut ctx.memory, old_ptr);
                    p
                }
                None => {
                    set_last_error(ctx, ERROR_NOT_ENOUGH_MEMORY);
                    return 0;
                }
            }
        };
        let new_size = state.process_heap.size(&mut ctx.memory, new_ptr);
        // Zero-init: preserve the retained prefix and clear the newly exposed
        // range. In place, growth exposes [old_size, new_size); a relocate or
        // restore draws its storage from a fresh block, so clear the tail after
        // the copied prefix.
        let copy_end = if old_ptr == 0 {
            0
        } else {
            old_size.min(dwBytes)
        };
        let clear_start = if new_ptr == old_ptr {
            old_size.min(new_size)
        } else {
            copy_end
        };
        if zeroinit && clear_start < new_size {
            ctx.memory[new_ptr + clear_start..][..(new_size - clear_start) as usize].fill(0);
        }
        // Refresh reverse lookup and backing on the live handle.
        if old_ptr != new_ptr {
            if old_ptr != 0 {
                state.global_mem.by_ptr.remove(&old_ptr);
            }
            state.global_mem.by_ptr.insert(new_ptr, hMem);
        }
        let m = state.global_mem.movable.get_mut(&hMem).unwrap();
        m.ptr = new_ptr;
        m.size = new_size;
        return hMem;
    }

    if is_fixed_ptr(&state.process_heap, &state.global_mem.by_ptr, hMem) {
        let old_ptr = hMem;
        if modify {
            if moveable {
                // Fixed -> movable conversion (attribute-only): keep the
                // existing backing, hand out a movable handle that points at it.
                let size = state.process_heap.size(&mut ctx.memory, old_ptr);
                let obj = Movable {
                    ptr: old_ptr,
                    size,
                    lock: 0,
                    discardable,
                };
                // Reserve a handle and register; on failure the original fixed
                // pointer stays live and untouched.
                return match state.global_mem.add(heap_end, obj) {
                    Some(h) => h,
                    None => {
                        set_last_error(ctx, ERROR_NOT_ENOUGH_MEMORY);
                        0
                    }
                };
            }
            // MODIFY without MOVEABLE on a fixed block: no-op.
            return hMem;
        }
        if discardable {
            set_last_error(ctx, ERROR_INVALID_PARAMETER);
            return 0;
        }
        if dwBytes == 0 {
            // Fixed zero-size reallocation is a resize, not a free: shrink to a
            // minimum in place, keeping the pointer-valued handle.
            return match state
                .process_heap
                .try_realloc_in_place(&mut ctx.memory, old_ptr, 1)
            {
                Some(p) => p,
                None => {
                    set_last_error(ctx, ERROR_NOT_ENOUGH_MEMORY);
                    0
                }
            };
        }
        let allow_move = moveable;
        let old_size = state.process_heap.size(&mut ctx.memory, old_ptr);
        let new_ptr =
            match state
                .process_heap
                .try_realloc_in_place(&mut ctx.memory, old_ptr, dwBytes)
            {
                Some(p) => p,
                None if allow_move => {
                    let Some(p) = state.process_heap.try_alloc(&mut ctx.memory, dwBytes) else {
                        set_last_error(ctx, ERROR_NOT_ENOUGH_MEMORY);
                        return 0;
                    };
                    let copy = old_size.min(dwBytes);
                    copy_bytes(ctx, old_ptr, p, copy);
                    state.process_heap.free(&mut ctx.memory, old_ptr);
                    p
                }
                None => {
                    set_last_error(ctx, ERROR_NOT_ENOUGH_MEMORY);
                    return 0;
                }
            };
        let new_size = state.process_heap.size(&mut ctx.memory, new_ptr);
        // Zero-init: preserve the retained prefix and clear the newly exposed
        // range. In place, growth exposes [old_size, new_size); a relocate draws
        // its storage from a fresh block, so clear the tail after the copied
        // prefix.
        let copy_end = old_size.min(dwBytes);
        let clear_start = if new_ptr == old_ptr {
            old_size.min(new_size)
        } else {
            copy_end
        };
        if zeroinit && clear_start < new_size {
            ctx.memory[new_ptr + clear_start..][..(new_size - clear_start) as usize].fill(0);
        }
        return new_ptr;
    }

    set_last_error(ctx, ERROR_INVALID_HANDLE);
    0
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
    _ctx: &mut Context,
    _hHeap: HANDLE,
    dwFlags: u32, /* HEAP_FLAGS */
    _lpMem: Ptr<()>,
    _dwBytes: u32,
) -> u32 {
    if dwFlags != 0 {
        log::warn!("HeapReAlloc flags: {:x}", dwFlags);
    }
    stub!(0)
    /*
    let memory = sys.memory();
    let heap = match memory.heaps.get(&hHeap) {
        None => {
            log::error!("HeapSize({hHeap:x}): no such heap");
            return 0;
        }
        Some(heap) => heap,
    };
    let mem = memory.mem();
    let old_size = heap.size(mem, lpMem);
    let new_addr = heap.alloc(mem, dwBytes);
    let copy_size = old_size.min(dwBytes);
    mem.copy(lpMem, new_addr, copy_size);
    heap.free(mem, lpMem);
    new_addr
    */
}

win32flags! {
    pub struct HEAP_FLAGS {
        const NO_SERIALIZE        = 0x01;
        const GENERATE_EXCEPTIONS = 0x04;
        const ZERO_MEMORY         = 0x08;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use runtime::{CPU, Context, Memory, RETURN_FROM_X86_ADDR32};

    use crate::{
        Ptr,
        heap::{HEADER, MAX_SKIP, Movable},
        kernel32::{self, GMEM, Object},
    };

    const ERROR_INVALID_HANDLE: u32 = 6;
    const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
    const ERROR_INVALID_PARAMETER: u32 = 87;
    const ERROR_NOT_LOCKED: u32 = 158;

    /// The global kernel32 state is a single process-wide mutex; serialize the
    /// tests that touch it so fixtures can't race each other.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// Build a Context with a small process heap and a real per-thread TEB,
    /// isolated from the other tests via SERIAL.
    fn new_ctx() -> Context {
        crate::trace::init("");
        let memory = Memory::leak_new(8 << 20);
        kernel32::init_state(0x400000, 0..0);
        let mut lock = kernel32::lock();
        // Reserve below 0x1000 so heap data pointers clear the null page.
        lock.mappings.alloc("test base".into(), 0x1000);
        let heap_size = 1 << 20;
        let heap_addr = lock.mappings.alloc("test heap".into(), heap_size);
        let process_heap = crate::heap::Heap::new(heap_addr, heap_size);
        lock.process_heap = process_heap;
        let heap_end = lock.process_heap.addr + lock.process_heap.size;
        lock.global_mem.init_handles(heap_end);
        let mut ctx = Context {
            cpu: CPU::default(),
            thread_handle: lock.objects.add(Object::Thread).to_raw(),
            thread_id: 1,
            memory,
            blocks: &[(RETURN_FROM_X86_ADDR32, Context::return_from_x86)],
            cache: Default::default(),
            recent: [Context::return_from_x86; 4],
        };
        lock.init_thread(&mut ctx, 0x400000);
        ctx
    }

    /// A second guest thread sharing the same guest memory but with its own TEB.
    fn second_ctx(first: &mut Context) -> Context {
        let mut lock = kernel32::lock();
        let mut ctx2 = Context {
            cpu: CPU::default(),
            thread_handle: lock.objects.add(Object::Thread).to_raw(),
            thread_id: lock.next_thread_id,
            memory: first.memory.unsafe_clone(),
            blocks: first.blocks,
            cache: Default::default(),
            recent: [Context::return_from_x86; 4],
        };
        lock.next_thread_id += 1;
        lock.init_thread(&mut ctx2, 0x400000);
        ctx2
    }

    fn gerr(ctx: &mut Context) -> u32 {
        kernel32::GetLastError(ctx)
    }

    fn gset(ctx: &mut Context, err: u32) {
        kernel32::SetLastError(ctx, err);
    }

    #[test]
    fn fixed_alloc() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 100);
        assert!(h != 0, "fixed alloc");
        // Pointer-valued handle: GlobalLock returns the handle itself.
        assert_eq!(kernel32::GlobalLock(&mut ctx, h), h);
        let sz = kernel32::GlobalSize(&mut ctx, h);
        assert!(sz >= 100, "size {sz}");
        ctx.memory.write::<u32>(h, 0x12345678);
        assert_eq!(ctx.memory.read::<u32>(h), 0x12345678);
        // Fixed memory unlocks trivially.
        assert!(kernel32::GlobalUnlock(&mut ctx, h));
        assert_eq!(kernel32::GlobalFree(&mut ctx, Ptr::new(h)), 0);
    }

    #[test]
    fn fixed_zeroinit() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        // GPTR = GMEM_FIXED | GMEM_ZEROINIT
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::ZEROINIT, 32);
        assert!(h != 0);
        assert!(ctx.memory[h..][..32].iter().all(|&b| b == 0));
        // Zero-init on a movable object.
        let m = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE | GMEM::ZEROINIT, 32);
        assert!(m != 0);
        let p = kernel32::GlobalLock(&mut ctx, m);
        assert!(ctx.memory[p..][..32].iter().all(|&b| b == 0));
        kernel32::GlobalUnlock(&mut ctx, m);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
        kernel32::GlobalFree(&mut ctx, Ptr::new(m));
    }

    #[test]
    fn movable_alloc() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 100);
        assert!(h != 0);
        let p = kernel32::GlobalLock(&mut ctx, h);
        assert!(p != 0);
        assert_ne!(p, h, "movable handle must be opaque, not the data pointer");
        ctx.memory.write::<u32>(p, 0xdeadbeef);
        assert_eq!(ctx.memory.read::<u32>(p), 0xdeadbeef);
        assert_eq!(kernel32::GlobalHandle(&mut ctx, p), h);
        // Final unlock of a single lock returns false and NO_ERROR.
        assert!(!kernel32::GlobalUnlock(&mut ctx, h));
        assert_eq!(gerr(&mut ctx), 0);
        // Independent objects have distinct handles and (while both live)
        // distinct backing pointers.
        let h2 = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 50);
        assert_ne!(h, h2);
        let p2 = kernel32::GlobalLock(&mut ctx, h2);
        assert_ne!(p, p2);
        kernel32::GlobalUnlock(&mut ctx, h2);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h2));
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
    }

    #[test]
    fn lock_state() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 64);
        assert_eq!(
            kernel32::GlobalFlags(&mut ctx, h) & 0xFF,
            0,
            "initial lock 0"
        );
        let p1 = kernel32::GlobalLock(&mut ctx, h);
        let p2 = kernel32::GlobalLock(&mut ctx, h);
        assert_eq!(p1, p2, "nested locks return the same pointer");
        assert_eq!(kernel32::GlobalFlags(&mut ctx, h) & 0xFF, 2);
        // Intermediate unlock returns true.
        assert!(kernel32::GlobalUnlock(&mut ctx, h));
        assert_eq!(kernel32::GlobalFlags(&mut ctx, h) & 0xFF, 1);
        // Final unlock returns false, NO_ERROR.
        assert!(!kernel32::GlobalUnlock(&mut ctx, h));
        assert_eq!(gerr(&mut ctx), 0);
        assert_eq!(kernel32::GlobalFlags(&mut ctx, h) & 0xFF, 0);
        // Extra unlock returns false, ERROR_NOT_LOCKED.
        assert!(!kernel32::GlobalUnlock(&mut ctx, h));
        assert_eq!(gerr(&mut ctx), ERROR_NOT_LOCKED);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
    }

    #[test]
    fn lock_count_saturation() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 8);
        // Lock past 255; the count must saturate, never wrap to look unlocked.
        for _ in 0..300 {
            kernel32::GlobalLock(&mut ctx, h);
        }
        assert_eq!(kernel32::GlobalFlags(&mut ctx, h) & 0xFF, 255);
        // Unlock back down to zero.
        let mut unlocks = 0;
        while kernel32::GlobalFlags(&mut ctx, h) & 0xFF > 0 {
            kernel32::GlobalUnlock(&mut ctx, h);
            unlocks += 1;
        }
        assert_eq!(unlocks, 255);
        // One more unlock is an error.
        assert!(!kernel32::GlobalUnlock(&mut ctx, h));
        assert_eq!(gerr(&mut ctx), ERROR_NOT_LOCKED);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
    }

    #[test]
    fn alignment() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let mut handles = vec![];
        for sz in [1u32, 3, 5, 7, 9, 11, 13] {
            let f = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), sz);
            assert!(f != 0);
            assert_eq!(f % 8, 0, "fixed data pointer {f:x}");
            let m = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, sz);
            assert!(m != 0);
            let p = kernel32::GlobalLock(&mut ctx, m);
            assert_eq!(p % 8, 0, "movable data pointer {p:x}");
            kernel32::GlobalUnlock(&mut ctx, m);
            handles.push((f, m));
        }
        // After freeing, fresh allocations stay 8-byte aligned.
        for &(f, m) in &handles {
            kernel32::GlobalFree(&mut ctx, Ptr::new(f));
            kernel32::GlobalFree(&mut ctx, Ptr::new(m));
        }
        let a = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 3);
        assert_eq!(a % 8, 0);
        let b = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 5);
        let pb = kernel32::GlobalLock(&mut ctx, b);
        assert_eq!(pb % 8, 0);
        kernel32::GlobalUnlock(&mut ctx, b);
        kernel32::GlobalFree(&mut ctx, Ptr::new(a));
        kernel32::GlobalFree(&mut ctx, Ptr::new(b));
    }

    #[test]
    fn discarded_lifecycle() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 0);
        assert!(h != 0, "zero-size movable returns a valid discarded handle");
        assert_eq!(
            kernel32::GlobalLock(&mut ctx, h),
            0,
            "locked discarded -> null"
        );
        assert_eq!(kernel32::GlobalSize(&mut ctx, h), 0);
        assert_ne!(
            kernel32::GlobalFlags(&mut ctx, h) & GMEM::DISCARDED.bits(),
            0
        );
        // Restore storage under the same handle.
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, h, 64, GMEM::MOVEABLE), h);
        let p = kernel32::GlobalLock(&mut ctx, h);
        assert!(p != 0);
        assert_eq!(kernel32::GlobalSize(&mut ctx, h), 64);
        assert_eq!(
            kernel32::GlobalFlags(&mut ctx, h) & GMEM::DISCARDED.bits(),
            0
        );
        kernel32::GlobalUnlock(&mut ctx, h);
        // Explicit discard.
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, h, 0, GMEM::MOVEABLE), h);
        assert_eq!(kernel32::GlobalLock(&mut ctx, h), 0);
        assert_eq!(kernel32::GlobalSize(&mut ctx, h), 0);
        assert_ne!(
            kernel32::GlobalFlags(&mut ctx, h) & GMEM::DISCARDED.bits(),
            0
        );
        // Restore again.
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, h, 32, GMEM::MOVEABLE), h);
        let p = kernel32::GlobalLock(&mut ctx, h);
        assert!(p != 0);
        assert_eq!(kernel32::GlobalSize(&mut ctx, h), 32);
        kernel32::GlobalUnlock(&mut ctx, h);
        assert_eq!(kernel32::GlobalFree(&mut ctx, Ptr::new(h)), 0);
    }

    #[test]
    fn resize_preserves_prefix() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 8);
        let p0 = kernel32::GlobalLock(&mut ctx, h);
        ctx.memory[p0..][..8].copy_from_slice(b"abcdefgh");
        kernel32::GlobalUnlock(&mut ctx, h);
        // Growth preserves the prefix.
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, h, 100, GMEM::MOVEABLE), h);
        let p1 = kernel32::GlobalLock(&mut ctx, h);
        assert_eq!(&ctx.memory[p1..][..8], b"abcdefgh");
        kernel32::GlobalUnlock(&mut ctx, h);
        // Shrink preserves the head.
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, h, 4, GMEM::MOVEABLE), h);
        let p2 = kernel32::GlobalLock(&mut ctx, h);
        assert_eq!(&ctx.memory[p2..][..4], b"abcd");
        kernel32::GlobalUnlock(&mut ctx, h);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
    }

    #[test]
    fn resize_zeroinit_growth() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 8);
        let p0 = kernel32::GlobalLock(&mut ctx, h);
        ctx.memory[p0..][..8].copy_from_slice(b"01234567");
        kernel32::GlobalUnlock(&mut ctx, h);
        // Grow with ZEROINIT: old contents kept, new growth zeroed.
        assert_eq!(
            kernel32::GlobalReAlloc(&mut ctx, h, 32, GMEM::MOVEABLE | GMEM::ZEROINIT),
            h
        );
        let p1 = kernel32::GlobalLock(&mut ctx, h);
        assert_eq!(&ctx.memory[p1..][..8], b"01234567");
        assert!(ctx.memory[p1 + 8..][..24].iter().all(|&b| b == 0));
        kernel32::GlobalUnlock(&mut ctx, h);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
    }

    #[test]
    fn resize_relocate_and_reverse_lookup() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 8);
        let p0 = kernel32::GlobalLock(&mut ctx, h);
        ctx.memory[p0..][..8].copy_from_slice(b"abcdefgh");
        kernel32::GlobalUnlock(&mut ctx, h);
        // Grow in place first (no obstruction yet).
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, h, 16, GMEM::MOVEABLE), h);
        let p1 = kernel32::GlobalLock(&mut ctx, h);
        assert_eq!(p1, p0);
        kernel32::GlobalUnlock(&mut ctx, h);
        // Obstruct the in-place growth with a neighbor right after the object.
        let n = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 200);
        // Force relocation: grow beyond what the neighbor permits.
        assert_eq!(
            kernel32::GlobalReAlloc(&mut ctx, h, 1000, GMEM::MOVEABLE),
            h
        );
        let p2 = kernel32::GlobalLock(&mut ctx, h);
        assert_ne!(
            p2, p1,
            "backing should have moved past the obstructing neighbor"
        );
        assert_eq!(&ctx.memory[p2..][..8], b"abcdefgh");
        // Reverse lookup follows the new backing.
        assert_eq!(kernel32::GlobalHandle(&mut ctx, p2), h);
        // The stale backing no longer maps to a handle.
        assert_eq!(kernel32::GlobalHandle(&mut ctx, p1), 0);
        kernel32::GlobalUnlock(&mut ctx, h);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
        kernel32::GlobalFree(&mut ctx, Ptr::new(n));
    }

    #[test]
    fn movement_fixed() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let f = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 8);
        // In-place growth with no MOVEABLE flag.
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, f, 16, GMEM::empty()), f);
        let n = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 200);
        // No MOVEABLE flag, obstructed -> failure, original preserved.
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, f, 1000, GMEM::empty()), 0);
        assert_eq!(gerr(&mut ctx), ERROR_NOT_ENOUGH_MEMORY);
        assert_eq!(kernel32::GlobalSize(&mut ctx, f), 16);
        // With the MOVEABLE flag a fixed block can relocate; it stays fixed.
        let f2 = kernel32::GlobalReAlloc(&mut ctx, f, 1000, GMEM::MOVEABLE);
        assert!(f2 != 0);
        assert_ne!(f2, f);
        assert_eq!(kernel32::GlobalLock(&mut ctx, f2), f2);
        assert_eq!(kernel32::GlobalFree(&mut ctx, Ptr::new(f2)), 0);
        kernel32::GlobalFree(&mut ctx, Ptr::new(n));
    }

    #[test]
    fn movement_locked_movable() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 8);
        let p = kernel32::GlobalLock(&mut ctx, h);
        // Locked movable, no MOVEABLE flag: in-place only, succeeds.
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, h, 16, GMEM::empty()), h);
        let p2 = kernel32::GlobalLock(&mut ctx, h);
        assert_eq!(p2, p);
        // Unlock back down.
        assert!(kernel32::GlobalUnlock(&mut ctx, h));
        assert!(!kernel32::GlobalUnlock(&mut ctx, h));
        // Obstruct with a neighbor.
        let n = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 200);
        let p3 = kernel32::GlobalLock(&mut ctx, h);
        // Locked + no MOVEABLE flag + obstructed -> failure, object intact.
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, h, 1000, GMEM::empty()), 0);
        assert_eq!(gerr(&mut ctx), ERROR_NOT_ENOUGH_MEMORY);
        assert_eq!(
            kernel32::GlobalFlags(&mut ctx, h) & 0xFF,
            1,
            "lock preserved"
        );
        assert_eq!(kernel32::GlobalSize(&mut ctx, h), 16);
        // Locked + MOVEABLE flag permits relocation; lock state preserved.
        assert_eq!(
            kernel32::GlobalReAlloc(&mut ctx, h, 1000, GMEM::MOVEABLE),
            h
        );
        let p4 = kernel32::GlobalLock(&mut ctx, h);
        assert_ne!(p4, p3);
        assert_eq!(kernel32::GlobalFlags(&mut ctx, h) & 0xFF, 2);
        assert!(kernel32::GlobalUnlock(&mut ctx, h));
        assert!(!kernel32::GlobalUnlock(&mut ctx, h));
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
        kernel32::GlobalFree(&mut ctx, Ptr::new(n));
    }

    #[test]
    fn modify_conversion() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let f = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 16);
        // GMEM_MODIFY ignores the requested byte count.
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, f, 0, GMEM::MODIFY), f);
        assert_eq!(kernel32::GlobalSize(&mut ctx, f), 16);
        // Fixed -> movable conversion via MODIFY|MOVEABLE.
        let h = kernel32::GlobalReAlloc(&mut ctx, f, 0, GMEM::MODIFY | GMEM::MOVEABLE);
        assert!(h != 0);
        assert_ne!(h, f);
        let p = kernel32::GlobalLock(&mut ctx, h);
        assert_eq!(p, f, "conversion keeps the same backing");
        assert_eq!(kernel32::GlobalSize(&mut ctx, h), 16);
        assert_eq!(kernel32::GlobalHandle(&mut ctx, p), h);
        // The old fixed handle is now a movable backing, not a fixed handle.
        assert_eq!(kernel32::GlobalLock(&mut ctx, f), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        kernel32::GlobalUnlock(&mut ctx, h);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
        // Ordinary resizing with MOVEABLE must not convert fixed to movable.
        let f2 = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 8);
        let r = kernel32::GlobalReAlloc(&mut ctx, f2, 16, GMEM::MOVEABLE);
        assert!(r != 0);
        assert_eq!(kernel32::GlobalLock(&mut ctx, r), r);
        kernel32::GlobalFree(&mut ctx, Ptr::new(r));
    }

    #[test]
    fn release_validation() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        // Null handle frees are a success no-op.
        assert_eq!(kernel32::GlobalFree(&mut ctx, Ptr::new(0)), 0);
        // Invalid handle returns it and sets an error.
        let bad = 0x12345678;
        assert_eq!(kernel32::GlobalFree(&mut ctx, Ptr::new(bad)), bad);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        // Freeing a locked object is allowed.
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 32);
        let _p = kernel32::GlobalLock(&mut ctx, h);
        assert_eq!(kernel32::GlobalFree(&mut ctx, Ptr::new(h)), 0);
        // The stale handle is now invalid across APIs.
        assert_eq!(kernel32::GlobalFree(&mut ctx, Ptr::new(h)), h);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(kernel32::GlobalLock(&mut ctx, h), 0);
        // Freeing a discarded object.
        let h2 = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 0);
        assert_eq!(kernel32::GlobalFree(&mut ctx, Ptr::new(h2)), 0);
        // Discarding a locked object fails without losing data.
        let h3 = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 32);
        let _p = kernel32::GlobalLock(&mut ctx, h3);
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, h3, 0, GMEM::MOVEABLE), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_PARAMETER);
        assert_eq!(kernel32::GlobalSize(&mut ctx, h3), 32);
        kernel32::GlobalUnlock(&mut ctx, h3);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h3));
        // Nothing unrelated was damaged: a fresh object works.
        let h4 = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 16);
        let p4 = kernel32::GlobalLock(&mut ctx, h4);
        ctx.memory.write::<u32>(p4, 0x99);
        assert_eq!(ctx.memory.read::<u32>(p4), 0x99);
        kernel32::GlobalUnlock(&mut ctx, h4);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h4));
    }

    #[test]
    fn failure_atomicity() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        // Near-u32::MAX requests fail cleanly, no wrap/panic.
        assert_eq!(kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), u32::MAX), 0);
        assert_eq!(gerr(&mut ctx), ERROR_NOT_ENOUGH_MEMORY);
        assert_eq!(kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, u32::MAX), 0);
        // A failed realloc leaves the original data and handle usable.
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 16);
        let p = kernel32::GlobalLock(&mut ctx, h);
        ctx.memory[p..][..16].copy_from_slice(b"0123456789abcdef");
        kernel32::GlobalUnlock(&mut ctx, h);
        assert_eq!(
            kernel32::GlobalReAlloc(&mut ctx, h, u32::MAX, GMEM::MOVEABLE),
            0
        );
        assert_eq!(gerr(&mut ctx), ERROR_NOT_ENOUGH_MEMORY);
        let p2 = kernel32::GlobalLock(&mut ctx, h);
        assert_eq!(&ctx.memory[p2..][..16], b"0123456789abcdef");
        kernel32::GlobalUnlock(&mut ctx, h);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
    }

    #[test]
    fn per_thread_errors() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        gset(&mut ctx, 0x1234);
        assert_eq!(gerr(&mut ctx), 0x1234);
        // Final unlock clears a seeded error to NO_ERROR.
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 16);
        kernel32::GlobalLock(&mut ctx, h);
        gset(&mut ctx, 0x9999);
        assert!(!kernel32::GlobalUnlock(&mut ctx, h));
        assert_eq!(gerr(&mut ctx), 0);
        // Extra unlock sets ERROR_NOT_LOCKED.
        gset(&mut ctx, 0x9999);
        assert!(!kernel32::GlobalUnlock(&mut ctx, h));
        assert_eq!(gerr(&mut ctx), ERROR_NOT_LOCKED);
        // Independence between two guest TEBs.
        let mut ctx2 = second_ctx(&mut ctx);
        gset(&mut ctx, 0x1111);
        gset(&mut ctx2, 0x2222);
        assert_eq!(gerr(&mut ctx), 0x1111);
        assert_eq!(gerr(&mut ctx2), 0x2222);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
    }

    #[test]
    fn invalid_handles() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        // An arbitrary integer must not be accepted as a handle or pointer.
        let bad = 0x12345678;
        assert_eq!(kernel32::GlobalLock(&mut ctx, bad), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert!(!kernel32::GlobalUnlock(&mut ctx, bad));
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(kernel32::GlobalSize(&mut ctx, bad), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(
            kernel32::GlobalFlags(&mut ctx, bad),
            GMEM::INVALID_HANDLE.bits()
        );
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(kernel32::GlobalHandle(&mut ctx, bad), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, bad, 10, GMEM::empty()), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        // No panic, no heap corruption: a valid allocation still works after.
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 8);
        assert!(h != 0);
        let p = kernel32::GlobalLock(&mut ctx, h);
        assert!(p != 0);
        kernel32::GlobalUnlock(&mut ctx, h);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
    }

    #[test]
    fn abi_integration() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        // Exercise the generated stdcall wrappers through the guest ABI:
        // args are read off the guest stack and the return lands in eax.
        ctx.push32(100); // dwBytes
        ctx.push32(GMEM::MOVEABLE.bits()); // uFlags
        ctx.call_builtin(0, kernel32::GlobalAlloc_stdcall);
        let h = ctx.cpu.regs.eax;
        assert!(h != 0);
        ctx.push32(h);
        ctx.call_builtin(0, kernel32::GlobalLock_stdcall);
        let p = ctx.cpu.regs.eax;
        assert!(p != 0);
        assert_ne!(p, h);
        ctx.push32(h);
        ctx.call_builtin(0, kernel32::GlobalSize_stdcall);
        assert!(ctx.cpu.regs.eax >= 100);
        ctx.push32(h);
        ctx.call_builtin(0, kernel32::GlobalUnlock_stdcall);
        assert_eq!(ctx.cpu.regs.eax, 0); // final unlock
        ctx.push32(h);
        ctx.call_builtin(0, kernel32::GlobalFree_stdcall);
        assert_eq!(ctx.cpu.regs.eax, 0);
    }

    // ---- Finding 1: authoritative fixed-pointer identity -----------------

    /// A crafted 8-aligned interior pointer whose `ptr - HEADER` word is a
    /// plausible block size must be rejected by every Global* entry point,
    /// leaving the original allocation (bytes, size, registry, capacity)
    /// intact so a later allocation cannot overlap it.
    #[test]
    fn interior_pointer_rejected() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let p = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 64);
        assert!(p != 0);
        ctx.memory.write::<u32>(p, 24); // legitimate payload that looks like a header
        let interior = p + 8;
        assert_eq!(kernel32::GlobalLock(&mut ctx, interior), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert!(!kernel32::GlobalUnlock(&mut ctx, interior));
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(kernel32::GlobalSize(&mut ctx, interior), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(
            kernel32::GlobalFlags(&mut ctx, interior),
            GMEM::INVALID_HANDLE.bits()
        );
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(kernel32::GlobalHandle(&mut ctx, interior), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(
            kernel32::GlobalReAlloc(&mut ctx, interior, 16, GMEM::empty()),
            0
        );
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(kernel32::GlobalFree(&mut ctx, Ptr::new(interior)), interior);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);

        // The original allocation is untouched and still live.
        assert_eq!(kernel32::GlobalSize(&mut ctx, p), 64);
        assert_eq!(ctx.memory.read::<u32>(p), 24);
        assert_eq!(kernel32::GlobalLock(&mut ctx, p), p);
        kernel32::GlobalUnlock(&mut ctx, p);

        // A later allocation must not overlap the still-live p.
        let q = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 64);
        assert!(q != 0);
        let p_end = p + kernel32::GlobalSize(&mut ctx, p);
        let q_end = q + kernel32::GlobalSize(&mut ctx, q);
        assert!(
            p_end <= q || q_end <= p,
            "later allocation overlaps the original"
        );
        kernel32::GlobalFree(&mut ctx, Ptr::new(p));
        kernel32::GlobalFree(&mut ctx, Ptr::new(q));
    }

    /// An interior pointer into a movable object's backing memory must not be
    /// accepted as a fixed handle, and the movable object's reverse lookup must
    /// keep working.
    #[test]
    fn interior_movable_backing_rejected() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 64);
        let b = kernel32::GlobalLock(&mut ctx, h);
        assert!(b != 0);
        ctx.memory.write::<u32>(b, 24);
        let interior = b + 8;
        assert_eq!(kernel32::GlobalLock(&mut ctx, interior), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(kernel32::GlobalSize(&mut ctx, interior), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(
            kernel32::GlobalReAlloc(&mut ctx, interior, 16, GMEM::empty()),
            0
        );
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(kernel32::GlobalFree(&mut ctx, Ptr::new(interior)), interior);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);

        // The movable object is intact and its backing still maps to its handle.
        assert_eq!(kernel32::GlobalSize(&mut ctx, h), 64);
        assert_eq!(ctx.memory.read::<u32>(b), 24);
        assert_eq!(kernel32::GlobalHandle(&mut ctx, b), h);
        kernel32::GlobalUnlock(&mut ctx, h);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
    }

    /// Unaligned pointers and addresses near the heap boundaries must be
    /// rejected without out-of-range reads (the authoritative registry never
    /// touches guest memory).
    #[test]
    fn unaligned_and_boundary_pointers_rejected() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let p = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 64);
        assert!(p != 0);
        let (haddr, hsize) = {
            let state = kernel32::lock();
            (state.process_heap.addr, state.process_heap.size)
        };

        // Unaligned interior.
        assert_eq!(kernel32::GlobalLock(&mut ctx, p + 4), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(kernel32::GlobalFree(&mut ctx, Ptr::new(p + 4)), p + 4);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);

        // Just past the heap end.
        let past_end = haddr + hsize + 4;
        assert_eq!(kernel32::GlobalLock(&mut ctx, past_end), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(kernel32::GlobalSize(&mut ctx, past_end), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);

        // The heap base itself is not a live data pointer.
        assert_eq!(kernel32::GlobalLock(&mut ctx, haddr), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);

        // The real allocation still works.
        assert_eq!(kernel32::GlobalLock(&mut ctx, p), p);
        kernel32::GlobalUnlock(&mut ctx, p);
        kernel32::GlobalFree(&mut ctx, Ptr::new(p));
    }

    /// Freeing B, then growing A in place across B's old header, must not
    /// revive B's stale pointer even when a plausible header-sized value is
    /// written at that location inside A's now-live payload.
    #[test]
    fn stale_header_not_revived() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let a = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 8);
        let b = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 8);
        assert!(a != 0 && b != 0);
        assert_eq!(kernel32::GlobalFree(&mut ctx, Ptr::new(b)), 0);
        // Grow A in place across B's old header.
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, a, 16, GMEM::empty()), a);
        assert_eq!(kernel32::GlobalSize(&mut ctx, a), 16);
        // b - HEADER lies inside A's now-live payload.
        ctx.memory.write::<u32>(b - HEADER, 32); // plausible header-sized value
        // B's old pointer stays invalid.
        assert_eq!(kernel32::GlobalLock(&mut ctx, b), 0);
        assert_eq!(gerr(&mut ctx), ERROR_INVALID_HANDLE);
        assert_eq!(kernel32::GlobalSize(&mut ctx, b), 0);
        assert_eq!(kernel32::GlobalFree(&mut ctx, Ptr::new(b)), b);
        // A remains intact.
        assert_eq!(kernel32::GlobalSize(&mut ctx, a), 16);
        kernel32::GlobalFree(&mut ctx, Ptr::new(a));
    }

    // ---- Finding 2: zero every exposed byte ------------------------------

    /// Dirty a 16-byte usable block, free it, prove the freed storage is
    /// reused dirty, then allocate `flags`/`sz` with ZEROINIT and require every
    /// byte reported by GlobalSize to be zero (not merely the requested size).
    fn dirty_reuse_zeroinit(ctx: &mut Context, flags: GMEM, sz: u32) {
        let d = kernel32::GlobalAlloc(ctx, GMEM::empty(), 16);
        let du = kernel32::GlobalSize(ctx, d);
        ctx.memory[d..][..du as usize].fill(0xA5);
        kernel32::GlobalFree(ctx, Ptr::new(d));
        // Prove the fixture actually supplies dirty reused storage.
        let r = kernel32::GlobalAlloc(ctx, GMEM::empty(), 16);
        assert!(
            ctx.memory[r..][..16].iter().any(|&b| b != 0),
            "freed storage not reused as dirty"
        );
        kernel32::GlobalFree(ctx, Ptr::new(r));
        let z = kernel32::GlobalAlloc(ctx, flags, sz);
        assert!(z != 0);
        let usable = kernel32::GlobalSize(ctx, z);
        assert!(usable >= sz);
        assert!(
            ctx.memory[z..][..usable as usize].iter().all(|&b| b == 0),
            "size {sz}: usable {usable} not fully zeroed"
        );
        kernel32::GlobalFree(ctx, Ptr::new(z));
    }

    /// Dirty a usable block, free it, and force reuse for odd-sized
    /// GMEM_ZEROINIT requests; every byte reported by GlobalSize must be zero,
    /// not merely the requested range. Covers fixed, movable, and the fixed
    /// zero-byte case, each with its own dirty fixture.
    #[test]
    fn zeroinit_clears_full_usable() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();

        for sz in [1u32, 3, 7, 9] {
            dirty_reuse_zeroinit(&mut ctx, GMEM::ZEROINIT, sz);
        }
        dirty_reuse_zeroinit(&mut ctx, GMEM::MOVEABLE | GMEM::ZEROINIT, 3);
        dirty_reuse_zeroinit(&mut ctx, GMEM::ZEROINIT, 0);
    }

    /// Zero-initialized in-place growth (dirty neighbor freed for storage)
    /// must preserve the retained prefix and clear the newly exposed tail
    /// beyond the requested size.
    #[test]
    fn zeroinit_in_place_growth() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 8);
        let p0 = kernel32::GlobalLock(&mut ctx, h);
        ctx.memory[p0..][..8].copy_from_slice(b"abcdefgh");
        kernel32::GlobalUnlock(&mut ctx, h);
        // Dirty a neighbor right after h, then free it for contiguous storage.
        let n = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 24);
        let nu = kernel32::GlobalSize(&mut ctx, n);
        ctx.memory[n..][..nu as usize].fill(0xBB);
        kernel32::GlobalFree(&mut ctx, Ptr::new(n));
        // Grow to an odd size so usable != requested.
        assert_eq!(
            kernel32::GlobalReAlloc(&mut ctx, h, 17, GMEM::MOVEABLE | GMEM::ZEROINIT),
            h
        );
        let p1 = kernel32::GlobalLock(&mut ctx, h);
        assert_eq!(p1, p0, "growth should be in place");
        assert_eq!(&ctx.memory[p1..][..8], b"abcdefgh");
        let usable = kernel32::GlobalSize(&mut ctx, h);
        assert!(usable >= 17);
        assert!(
            ctx.memory[p1 + 8..][..(usable - 8) as usize]
                .iter()
                .all(|&b| b == 0),
            "in-place growth tail not zeroed"
        );
        kernel32::GlobalUnlock(&mut ctx, h);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
    }

    /// Zero-initialized relocated growth into dirty free space must preserve
    /// the retained prefix and clear the tail beyond the requested size.
    #[test]
    fn zeroinit_relocated_growth() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        // Dirty a target region that relocation will draw from.
        let big = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 64);
        let bu = kernel32::GlobalSize(&mut ctx, big);
        ctx.memory[big..][..bu as usize].fill(0xEE);
        kernel32::GlobalFree(&mut ctx, Ptr::new(big));
        let h2 = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 8);
        let q0 = kernel32::GlobalLock(&mut ctx, h2);
        ctx.memory[q0..][..8].copy_from_slice(b"ABCDEFGH");
        kernel32::GlobalUnlock(&mut ctx, h2);
        // Obstruct in-place growth.
        let obs = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 24);
        assert_eq!(
            kernel32::GlobalReAlloc(&mut ctx, h2, 9, GMEM::MOVEABLE | GMEM::ZEROINIT),
            h2
        );
        let q1 = kernel32::GlobalLock(&mut ctx, h2);
        assert_ne!(q1, q0, "growth should have relocated");
        assert_eq!(&ctx.memory[q1..][..8], b"ABCDEFGH");
        let usable = kernel32::GlobalSize(&mut ctx, h2);
        assert!(usable >= 9);
        assert!(
            ctx.memory[q1 + 8..][..(usable - 8) as usize]
                .iter()
                .all(|&b| b == 0),
            "relocated growth tail not zeroed"
        );
        kernel32::GlobalUnlock(&mut ctx, h2);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h2));
        kernel32::GlobalFree(&mut ctx, Ptr::new(obs));
    }

    /// Restoration from a discarded object with ZEROINIT must clear its entire
    /// exposed backing allocation, including bytes past the requested size,
    /// reusing dirty free space.
    #[test]
    fn zeroinit_restore_dirty() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        // Dirty a reusable region, free it.
        let db = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 16);
        let dbu = kernel32::GlobalSize(&mut ctx, db);
        ctx.memory[db..][..dbu as usize].fill(0xDD);
        kernel32::GlobalFree(&mut ctx, Ptr::new(db));
        let h = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 8);
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, h, 0, GMEM::MOVEABLE), h);
        assert_eq!(kernel32::GlobalSize(&mut ctx, h), 0);
        // Restore to an odd size so usable != requested.
        assert_eq!(
            kernel32::GlobalReAlloc(&mut ctx, h, 9, GMEM::MOVEABLE | GMEM::ZEROINIT),
            h
        );
        let p1 = kernel32::GlobalLock(&mut ctx, h);
        assert!(p1 != 0);
        let usable = kernel32::GlobalSize(&mut ctx, h);
        assert!(usable >= 9);
        assert!(
            ctx.memory[p1..][..usable as usize].iter().all(|&b| b == 0),
            "restored backing not fully zeroed"
        );
        kernel32::GlobalUnlock(&mut ctx, h);
        kernel32::GlobalFree(&mut ctx, Ptr::new(h));
    }

    // ---- Finding 3: TlsGetValue clears last error ------------------------

    #[test]
    fn tls_get_value_clears_last_error() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let idx = kernel32::TlsAlloc(&mut ctx);

        // Stored zero: successful retrieval of zero clears a seeded error.
        kernel32::TlsSetValue(&mut ctx, idx, 0);
        gset(&mut ctx, ERROR_NOT_LOCKED);
        assert_eq!(kernel32::TlsGetValue(&mut ctx, idx), 0);
        assert_eq!(gerr(&mut ctx), 0, "zero return should clear last error");

        // Nonzero value: also clears last error.
        kernel32::TlsSetValue(&mut ctx, idx, 0x1234);
        gset(&mut ctx, ERROR_NOT_LOCKED);
        assert_eq!(kernel32::TlsGetValue(&mut ctx, idx), 0x1234);
        assert_eq!(gerr(&mut ctx), 0);

        // A second guest thread with its own TEB: a successful lookup in one
        // must leave the other thread's error untouched. Reseed immediately
        // before each operation.
        let mut ctx2 = second_ctx(&mut ctx);
        gset(&mut ctx, 0x1111);
        gset(&mut ctx2, 0x2222);
        kernel32::TlsSetValue(&mut ctx2, idx, 0x55);
        assert_eq!(kernel32::TlsGetValue(&mut ctx2, idx), 0x55);
        assert_eq!(gerr(&mut ctx2), 0);
        assert_eq!(
            gerr(&mut ctx),
            0x1111,
            "ctx2 lookup must not touch ctx's error"
        );
        // Each thread has its own TEB/TLS slots: ctx's slot still holds the
        // value it stored, and ctx's own lookup clears only its own error.
        assert_eq!(kernel32::TlsGetValue(&mut ctx, idx), 0x1234);
        assert_eq!(gerr(&mut ctx), 0);
    }

    // ---- Finding 5: discardable-flag policy ------------------------------

    /// Direct zero-size movable allocation with DISCARDABLE must retain the
    /// attribute like a nonzero allocation followed by discard, keeping it
    /// through restore/lock/discard while discarded state and lock count move
    /// independently.
    #[test]
    fn discardable_attribute_consistent() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let d = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE | GMEM::DISCARDABLE, 0);
        assert!(d != 0);
        let n = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE | GMEM::DISCARDABLE, 8);
        assert!(n != 0);
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, n, 0, GMEM::MOVEABLE), n);

        let mask = GMEM::DISCARDABLE.bits() | GMEM::DISCARDED.bits();
        let df = kernel32::GlobalFlags(&mut ctx, d) & mask;
        let nf = kernel32::GlobalFlags(&mut ctx, n) & mask;
        assert_eq!(df, nf, "direct zero-size differs from alloc-then-discard");
        assert_ne!(df & GMEM::DISCARDABLE.bits(), 0);
        assert_ne!(df & GMEM::DISCARDED.bits(), 0);
        assert_eq!(kernel32::GlobalFlags(&mut ctx, d) & 0xFF, 0);

        // Restore d: attribute survives, discarded state clears.
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, d, 8, GMEM::MOVEABLE), d);
        assert_ne!(
            kernel32::GlobalFlags(&mut ctx, d) & GMEM::DISCARDABLE.bits(),
            0
        );
        assert_eq!(
            kernel32::GlobalFlags(&mut ctx, d) & GMEM::DISCARDED.bits(),
            0
        );

        // Lock changes the lock count independently of the attribute.
        let p = kernel32::GlobalLock(&mut ctx, d);
        assert!(p != 0);
        assert_eq!(kernel32::GlobalFlags(&mut ctx, d) & 0xFF, 1);
        assert_ne!(
            kernel32::GlobalFlags(&mut ctx, d) & GMEM::DISCARDABLE.bits(),
            0
        );
        kernel32::GlobalUnlock(&mut ctx, d);

        // Discard again: attribute survives, discarded state and lock count
        // change independently.
        assert_eq!(kernel32::GlobalReAlloc(&mut ctx, d, 0, GMEM::MOVEABLE), d);
        assert_ne!(
            kernel32::GlobalFlags(&mut ctx, d) & GMEM::DISCARDABLE.bits(),
            0
        );
        assert_ne!(
            kernel32::GlobalFlags(&mut ctx, d) & GMEM::DISCARDED.bits(),
            0
        );
        assert_eq!(kernel32::GlobalFlags(&mut ctx, d) & 0xFF, 0);

        // Control object without the attribute.
        let c = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 0);
        assert_eq!(
            kernel32::GlobalFlags(&mut ctx, c) & GMEM::DISCARDABLE.bits(),
            0
        );

        kernel32::GlobalFree(&mut ctx, Ptr::new(d));
        kernel32::GlobalFree(&mut ctx, Ptr::new(n));
        kernel32::GlobalFree(&mut ctx, Ptr::new(c));
    }

    // ---- Finding 4: collision-safe, fallible handles ---------------------

    /// Keep an early movable handle alive, move the counter near its boundary,
    /// and allocate further; the collision-skip policy must never replace the
    /// early object.
    #[test]
    fn movable_handle_collision_safe() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let early = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 32);
        assert!(early != 0);
        let early_p = kernel32::GlobalLock(&mut ctx, early);
        ctx.memory[early_p..][..32].copy_from_slice(b"0123456789abcdef0123456789abcdef");
        kernel32::GlobalLock(&mut ctx, early); // lock count 2
        {
            let mut state = kernel32::lock();
            state.global_mem.next_handle = u32::MAX;
        }
        let a = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 16);
        assert!(a != 0);
        let b = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 16);
        assert!(b != 0);

        // Early object: data, size, lock count, backing pointer, reverse lookup.
        assert_eq!(kernel32::GlobalSize(&mut ctx, early), 32);
        assert_eq!(kernel32::GlobalFlags(&mut ctx, early) & 0xFF, 2);
        assert_eq!(kernel32::GlobalLock(&mut ctx, early), early_p);
        assert_eq!(
            &ctx.memory[early_p..][..32],
            b"0123456789abcdef0123456789abcdef"
        );
        assert_eq!(kernel32::GlobalHandle(&mut ctx, early_p), early);

        kernel32::GlobalUnlock(&mut ctx, early);
        kernel32::GlobalUnlock(&mut ctx, early);
        kernel32::GlobalFree(&mut ctx, Ptr::new(early));
        kernel32::GlobalFree(&mut ctx, Ptr::new(a));
        kernel32::GlobalFree(&mut ctx, Ptr::new(b));
    }

    /// Deterministic handle-exhaustion: with the restart region fully occupied
    /// and the counter near u32::MAX, backed allocation, discarded allocation,
    /// and fixed-to-movable conversion all fail cleanly without losing heap
    /// capacity or registry entries.
    #[test]
    fn movable_handle_exhaustion() {
        let _g = SERIAL.lock();
        let mut ctx = new_ctx();
        let f = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 32);
        assert!(f != 0);
        ctx.memory[f..][..32].copy_from_slice(b"0123456789abcdef0123456789abcdef");
        // Fill the restart region with occupied handles and point the counter
        // at u32::MAX so the next reservation wraps back into it.
        {
            let mut state = kernel32::lock();
            let heap_end = state.process_heap.addr + state.process_heap.size;
            let ns = heap_end + HEADER;
            for i in 0..=MAX_SKIP {
                state.global_mem.movable.insert(
                    ns + i,
                    Movable {
                        ptr: 0,
                        size: 0,
                        lock: 0,
                        discardable: false,
                    },
                );
            }
            state.global_mem.next_handle = u32::MAX;
        }
        // First backed allocation consumes u32::MAX.
        let a = kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 16);
        assert!(a != 0);
        // Now the counter wraps to the occupied restart region: every caller
        // must fail cleanly.
        assert_eq!(kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 16), 0);
        assert_eq!(gerr(&mut ctx), ERROR_NOT_ENOUGH_MEMORY);
        assert_eq!(kernel32::GlobalAlloc(&mut ctx, GMEM::MOVEABLE, 0), 0);
        assert_eq!(gerr(&mut ctx), ERROR_NOT_ENOUGH_MEMORY);
        assert_eq!(
            kernel32::GlobalReAlloc(&mut ctx, f, 0, GMEM::MODIFY | GMEM::MOVEABLE),
            0
        );
        assert_eq!(gerr(&mut ctx), ERROR_NOT_ENOUGH_MEMORY);

        // The original fixed pointer and the surviving backed allocation are
        // intact; no registry entry or heap capacity was lost.
        assert_eq!(kernel32::GlobalSize(&mut ctx, f), 32);
        assert_eq!(&ctx.memory[f..][..32], b"0123456789abcdef0123456789abcdef");
        assert_eq!(kernel32::GlobalLock(&mut ctx, f), f);
        kernel32::GlobalUnlock(&mut ctx, f);
        let ap = kernel32::GlobalLock(&mut ctx, a);
        assert!(ap != 0);
        assert_eq!(kernel32::GlobalSize(&mut ctx, a), 16);
        kernel32::GlobalUnlock(&mut ctx, a);
        {
            let state = kernel32::lock();
            assert!(state.global_mem.movable.contains_key(&a));
        }
        // A fixed allocation (no handle needed) still works.
        let g = kernel32::GlobalAlloc(&mut ctx, GMEM::empty(), 16);
        assert!(g != 0);

        kernel32::GlobalFree(&mut ctx, Ptr::new(a));
        kernel32::GlobalFree(&mut ctx, Ptr::new(f));
        kernel32::GlobalFree(&mut ctx, Ptr::new(g));
    }
}
